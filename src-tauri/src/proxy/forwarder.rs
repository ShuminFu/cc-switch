//! 请求转发器
//!
//! Drives one client request through the staged pipeline in
//! [`crate::proxy::pipeline`]: provider selection has already happened in
//! `RequestContext`; here each provider gets an attempt
//! (budget check → breaker permit → transform → send → validate → record), with
//! at most one same-provider rectifier retry, until one serves the request.

use super::circuit_breaker::HalfOpenPermit;
use super::hyper_client::ProxyResponse;
use super::pipeline::{
    attempt::{plan_rectification, AttemptOutcome, RectifierKind, RectifyDecision},
    auth::{copilot_base_url, resolve_auth},
    endpoint::{is_streaming_request, resolve_upstream_target, Route},
    headers::{
        anthropic_beta_value, apply_copilot_hint_headers, build_codex_oauth_session_headers,
        custom_user_agent, reject_proxy_placeholder_for_managed_account_upstream, HeaderPolicy,
        PROXY_AUTH_PLACEHOLDER,
    },
    metrics::{ProxyMetrics, RequestOutcome},
    timeout::TimeoutPolicy,
    transport::{self, should_preserve_exact_header_case, UpstreamRequest},
    validate::{validate_buffered_success, validate_stream_start, StreamFormat},
};
use super::{
    error::*,
    failover_switch::FailoverSwitchManager,
    log_codes::fwd as log_fwd,
    provider_router::ProviderRouter,
    providers::{
        codex_chat_history::CodexChatHistoryStore, gemini_shadow::GeminiShadowStore, get_adapter,
        ProviderAdapter, ProviderType,
    },
    types::{CopilotOptimizerConfig, OptimizerConfig, RectifierConfig},
    ProxyError,
};
use crate::commands::CopilotAuthState;
use crate::{app_config::AppType, provider::Provider};
use http::Extensions;
use serde_json::Value;
use std::borrow::Cow;
use std::sync::Arc;
use tauri::Manager;
use tokio::sync::RwLock;

fn validate_codex_official_authorization(headers: &http::HeaderMap) -> Result<(), ProxyError> {
    let authorization = headers
        .get(http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .map(str::trim);
    match authorization {
        None | Some("") => Err(ProxyError::AuthError(
            "Codex official login is unavailable; sign in to ChatGPT in Codex first".to_string(),
        )),
        Some(value) if value.contains(PROXY_AUTH_PLACEHOLDER) => Err(ProxyError::AuthError(
            "Switched to the official OpenAI provider; restart Codex or start a new session to load the official login".to_string(),
        )),
        Some(_) => Ok(()),
    }
}

pub struct ForwardResult {
    pub response: ProxyResponse,
    pub provider: Provider,
    pub claude_api_format: Option<String>,
    /// 实际发往上游的模型名（路由接管/模型映射后的真值）。
    ///
    /// usage 归因不能依赖 ctx.request_model（映射前的客户端别名）：上游响应
    /// 缺失 model 或回显别名时，接管流量会被记成 claude-* 并按其定价计费。
    pub outbound_model: Option<String>,
    /// 活跃连接 RAII guard：随响应一起流转到 response_processor / handle_claude_transform，
    /// 最终被 move 进流式 body future（或非流式响应作用域），覆盖整个响应生命周期。
    pub(crate) connection_guard: Option<ActiveConnectionGuard>,
    /// 流式响应的最终结果上报器（PROXY-09）；由 `forward_with_retry` 挂到
    /// `connection_guard` 上，随流一起走到流结束。
    pub(crate) stream_health: Option<StreamHealthReporter>,
}

pub struct ForwardError {
    pub error: ProxyError,
    pub provider: Option<Provider>,
}

/// 活跃连接 RAII guard
///
/// 构造时 `active_connections` +1；Drop 时 -1（原子操作，无需调度任务），因此可以
/// 把 guard move 进流式 body future，让计数覆盖整个响应生命周期（PROXY-34）。
pub(crate) struct ActiveConnectionGuard {
    metrics: Arc<ProxyMetrics>,
    stream_health: Option<StreamHealthReporter>,
}

impl ActiveConnectionGuard {
    pub(crate) fn acquire(metrics: Arc<ProxyMetrics>) -> Self {
        metrics.connection_opened();
        Self {
            metrics,
            stream_health: None,
        }
    }

    /// 流在首包之后因上游错误 / 静默超时中断：计入熔断器与健康度（PROXY-09）。
    pub(crate) fn report_stream_failure(&mut self, message: &str) {
        if let Some(reporter) = self.stream_health.take() {
            reporter.report(false, Some(message.to_string()));
        }
    }

    /// 流正常结束：记录 provider 成功。
    pub(crate) fn report_stream_success(&mut self) {
        if let Some(reporter) = self.stream_health.take() {
            reporter.report(true, None);
        }
    }
}

impl Drop for ActiveConnectionGuard {
    fn drop(&mut self) {
        self.metrics.connection_closed();
    }
}

/// 流式响应的 provider 健康结果上报器（PROXY-09）。
///
/// 流式响应在首包到达时只是「看起来成功」：上游可能发完 `message_start` 就挂住或
/// 断开。非 HalfOpen 探测的流式成功不在首包时记账，而是延迟到流结束：
/// 正常结束记成功，上游错误/静默超时记失败。未显式上报就被丢弃（客户端中断、
/// 非流式消费路径等）时按成功记账，保持旧语义。
pub(crate) struct StreamHealthReporter {
    router: Arc<ProviderRouter>,
    metrics: Arc<ProxyMetrics>,
    provider_id: String,
    provider_name: String,
    app_type: String,
    reported: bool,
}

impl StreamHealthReporter {
    fn report(mut self, success: bool, error: Option<String>) {
        self.send(success, error);
    }

    fn send(&mut self, success: bool, error: Option<String>) {
        if self.reported {
            return;
        }
        self.reported = true;
        if !success {
            self.metrics.set_last_error(format!(
                "Provider {} 流式响应中断: {}",
                self.provider_name,
                error.as_deref().unwrap_or("unknown")
            ));
        }
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let router = self.router.clone();
        let provider_id = std::mem::take(&mut self.provider_id);
        let app_type = std::mem::take(&mut self.app_type);
        handle.spawn(async move {
            if let Err(e) = router
                .record_result(&provider_id, &app_type, false, success, error)
                .await
            {
                log::warn!(
                    "[{app_type}] 记录流式 Provider 结果失败: provider_id={provider_id}, error={e}"
                );
            }
        });
    }
}

impl Drop for StreamHealthReporter {
    fn drop(&mut self) {
        self.send(true, None);
    }
}

/// The client request one attempt forwards (shared by every provider attempt).
struct AttemptRequest<'a> {
    app_type: &'a AppType,
    method: &'a http::Method,
    endpoint: &'a str,
    headers: &'a http::HeaderMap,
    extensions: &'a Extensions,
    adapter: &'a dyn ProviderAdapter,
}

/// Per-request settings and shared stores handed to the forwarder.
pub struct ForwarderSettings {
    pub timeouts: TimeoutPolicy,
    /// `AppProxyConfig.max_retries` (retries after the first attempt).
    pub max_retries: u32,
    pub current_provider_id_at_start: String,
    /// Internal session key (`codex_<id>` for Codex), used for local stores and
    /// prompt-cache identities.
    pub session_id: String,
    /// The client's own session id, sent in upstream session headers (PROXY-44).
    pub upstream_session_id: String,
    pub session_client_provided: bool,
    pub rectifier_config: RectifierConfig,
    pub optimizer_config: OptimizerConfig,
    pub copilot_optimizer_config: CopilotOptimizerConfig,
}

pub struct RequestForwarder {
    /// 共享的 ProviderRouter（持有熔断器状态）
    router: Arc<ProviderRouter>,
    pub(crate) metrics: Arc<ProxyMetrics>,
    current_providers: Arc<RwLock<std::collections::HashMap<String, (String, String)>>>,
    pub(crate) gemini_shadow: Arc<GeminiShadowStore>,
    pub(crate) codex_chat_history: Arc<CodexChatHistoryStore>,
    /// 故障转移切换管理器
    failover_manager: Arc<FailoverSwitchManager>,
    /// AppHandle，用于发射事件、更新托盘和读取托管账号凭据
    pub(crate) app_handle: Option<tauri::AppHandle>,
    /// 请求开始时的"当前供应商 ID"（用于判断是否需要同步 UI/托盘）
    current_provider_id_at_start: String,
    /// 代理会话 ID（内部键，用于 Gemini Native shadow replay / 缓存身份）
    pub(crate) session_id: String,
    /// 客户端自己的会话 ID（上游会话头使用）
    upstream_session_id: String,
    /// Session ID 是否由客户端提供；生成值不能作为上游缓存身份。
    pub(crate) session_client_provided: bool,
    /// 整流器配置
    pub(crate) rectifier_config: RectifierConfig,
    /// 优化器配置
    pub(crate) optimizer_config: OptimizerConfig,
    /// Copilot 优化器配置
    pub(crate) copilot_optimizer_config: CopilotOptimizerConfig,
    /// 本次请求的超时策略（PROXY-08）
    timeouts: TimeoutPolicy,
    /// 单个客户端请求最多尝试的 provider 数。
    ///
    /// 由 `AppProxyConfig.max_retries` (UI: "请求失败时的重试次数, 0-10") 派生：
    /// `max_attempts = max_retries + 1`，所以 max_retries=0 表示仅尝试一家、
    /// max_retries=3（默认）表示最多 4 家。loop 同时受 providers.len() 自然限制。
    max_attempts: usize,
}

impl RequestForwarder {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        router: Arc<ProviderRouter>,
        metrics: Arc<ProxyMetrics>,
        current_providers: Arc<RwLock<std::collections::HashMap<String, (String, String)>>>,
        gemini_shadow: Arc<GeminiShadowStore>,
        codex_chat_history: Arc<CodexChatHistoryStore>,
        failover_manager: Arc<FailoverSwitchManager>,
        app_handle: Option<tauri::AppHandle>,
        settings: ForwarderSettings,
    ) -> Self {
        Self {
            router,
            metrics,
            current_providers,
            gemini_shadow,
            codex_chat_history,
            failover_manager,
            app_handle,
            current_provider_id_at_start: settings.current_provider_id_at_start,
            session_id: settings.session_id,
            upstream_session_id: settings.upstream_session_id,
            session_client_provided: settings.session_client_provided,
            rectifier_config: settings.rectifier_config,
            optimizer_config: settings.optimizer_config,
            copilot_optimizer_config: settings.copilot_optimizer_config,
            timeouts: settings.timeouts,
            // max_retries 是「失败后重试次数」语义，attempt 上限 = retries + 1。
            max_attempts: (settings.max_retries as usize).saturating_add(1),
        }
    }

    /// 预防式 media 降级：发送前对 text-only 模型把图片块替换为标记。
    ///
    /// 受 `enabled && request_media_fallback` 管辖；其中"启发式模型名单预测"
    /// 再受 `request_media_heuristic` 单独管辖（显式声明 text-only 始终生效）。
    /// 返回被替换的图片块数量（0 = 未触发或开关关闭）。
    pub(crate) fn apply_media_prevention(&self, body: &mut Value, provider: &Provider) -> usize {
        if !(self.rectifier_config.enabled && self.rectifier_config.request_media_fallback) {
            return 0;
        }
        let replaced_images = super::media_sanitizer::replace_images_for_text_only_model(
            body,
            provider,
            self.rectifier_config.request_media_heuristic,
        );
        if replaced_images > 0 {
            let model = body.get("model").and_then(Value::as_str).unwrap_or("");
            log::info!(
                "[Media] Replaced {replaced_images} image block(s) with {} for text-only provider={}, model={}",
                super::media_sanitizer::UNSUPPORTED_IMAGE_MARKER,
                provider.id,
                model
            );
        }
        replaced_images
    }

    async fn record_success_result(
        &self,
        provider_id: &str,
        app_type: &str,
        used_half_open_permit: bool,
    ) {
        if used_half_open_permit {
            if let Err(e) = self
                .router
                .record_result(provider_id, app_type, true, true, None)
                .await
            {
                log::warn!(
                    "[{app_type}] 记录 Provider 成功结果失败: provider_id={provider_id}, error={e}"
                );
            }
            return;
        }

        let router = self.router.clone();
        let provider_id = provider_id.to_string();
        let app_type = app_type.to_string();
        tokio::spawn(async move {
            if let Err(e) = router
                .record_result(&provider_id, &app_type, false, true, None)
                .await
            {
                log::warn!(
                    "[{app_type}] 异步记录 Provider 成功结果失败: provider_id={provider_id}, error={e}"
                );
            }
        });
    }

    /// 成功响应的熔断/健康记账。
    ///
    /// HalfOpen 探测与非流式响应立即记录（与旧行为一致）；普通流式响应返回
    /// [`StreamHealthReporter`]，把结果延迟到流结束再记（PROXY-09）。
    async fn record_success_or_defer(
        &self,
        provider: &Provider,
        app_type: &str,
        used_half_open_permit: bool,
        response_is_sse: bool,
    ) -> Option<StreamHealthReporter> {
        if used_half_open_permit || !response_is_sse {
            self.record_success_result(&provider.id, app_type, used_half_open_permit)
                .await;
            return None;
        }
        Some(StreamHealthReporter {
            router: self.router.clone(),
            metrics: self.metrics.clone(),
            provider_id: provider.id.clone(),
            provider_name: provider.name.clone(),
            app_type: app_type.to_string(),
            reported: false,
        })
    }

    /// 转发请求（带故障转移）
    ///
    /// Thin wrapper: counts the client request and holds the active-connection
    /// guard, which travels with the response so streams keep it until they end.
    #[allow(clippy::too_many_arguments)]
    pub async fn forward_with_retry(
        &self,
        app_type: &AppType,
        method: http::Method,
        endpoint: &str,
        body: &Value,
        headers: axum::http::HeaderMap,
        extensions: Extensions,
        providers: Vec<Provider>,
    ) -> Result<ForwardResult, ForwardError> {
        let mut guard = ActiveConnectionGuard::acquire(self.metrics.clone());
        self.metrics.record_request_start();
        let result = self
            .forward_with_retry_inner(
                app_type, method, endpoint, body, headers, extensions, providers,
            )
            .await;
        // Err 路径：guard 在函数 scope 内随返回值落地时自动 drop。
        result.map(|mut fr| {
            guard.stream_health = fr.stream_health.take();
            fr.connection_guard = Some(guard);
            fr
        })
    }

    /// The attempt loop: one attempt per provider in failover order.
    #[allow(clippy::too_many_arguments)]
    async fn forward_with_retry_inner(
        &self,
        app_type: &AppType,
        method: http::Method,
        endpoint: &str,
        body: &Value,
        headers: axum::http::HeaderMap,
        extensions: Extensions,
        providers: Vec<Provider>,
    ) -> Result<ForwardResult, ForwardError> {
        let adapter = get_adapter(app_type);
        let app_type_str = app_type.as_str();
        if providers.is_empty() {
            return Err(ForwardError {
                error: ProxyError::NoAvailableProvider,
                provider: None,
            });
        }
        let request = AttemptRequest {
            app_type,
            method: &method,
            endpoint,
            headers: &headers,
            extensions: &extensions,
            adapter: adapter.as_ref(),
        };

        let mut last_error = None;
        let mut last_provider = None;
        let mut attempted_providers = 0usize;
        // 单 Provider 场景下跳过熔断器检查（故障转移关闭时），避免熔断器阻塞所有请求
        let bypass_circuit_breaker = providers.len() == 1;
        // 跨 provider 携带的请求体：thinking 签名 / budget 整流生效后替换为整流后的
        // body，让下一家 provider 首次请求就用整流后的内容（PROXY-24）。
        let mut carried_body: Option<Value> = None;

        for provider in providers.iter() {
            // 上限检查放在熔断器 allow 之前，避免超限时还占用 HalfOpen 探测名额。
            if attempted_providers >= self.max_attempts {
                log::warn!(
                    "[{app_type_str}] 已达最大尝试次数上限 ({}/{}), 停止故障转移",
                    attempted_providers,
                    self.max_attempts
                );
                break;
            }

            // Breaker permit. `HalfOpenPermit` is an RAII guard: if this future is
            // dropped before the result is recorded (client disconnect), Drop
            // releases the HalfOpen probe slot.
            let mut half_open_permit = if bypass_circuit_breaker {
                HalfOpenPermit::none()
            } else {
                let (permit, guard) = self
                    .router
                    .allow_provider_request_guarded(&provider.id, app_type_str)
                    .await;
                if !permit.allowed {
                    continue;
                }
                guard
            };

            // Budget hard limit (PRODUCT-05): skip a provider over its limit.
            if let Some((budget_err, limit_status)) =
                self.check_budget_breach(app_type_str, provider).await
            {
                if let Some(app) = self.app_handle.as_ref() {
                    notify_budget_breach(app, app_type_str, provider, &limit_status);
                }
                last_error = Some(ProxyError::UpstreamError {
                    status: 429,
                    body: Some(budget_err.to_string()),
                });
                last_provider = Some(provider.clone());
                continue;
            }

            let base_body: &Value = carried_body.as_ref().unwrap_or(body);
            let provider_specific_body =
                self.optimizer_config.enabled && is_bedrock_provider(provider);
            let provider_body = self.provider_body(base_body, provider_specific_body);

            attempted_providers += 1;
            self.metrics
                .record_attempt_target(&provider.id, &provider.name);

            let outcome = self
                .attempt_provider(
                    &request,
                    provider,
                    provider_body,
                    base_body,
                    provider_specific_body,
                    &mut half_open_permit,
                    (attempted_providers, providers.len()),
                )
                .await;
            match outcome {
                AttemptOutcome::Served {
                    response,
                    claude_api_format,
                    outbound_model,
                } => {
                    return Ok(self
                        .commit_success(
                            app_type_str,
                            provider,
                            &mut half_open_permit,
                            response,
                            claude_api_format,
                            outbound_model,
                            last_error.is_some(),
                        )
                        .await);
                }
                AttemptOutcome::Failover {
                    error,
                    carried_body: next_body,
                } => {
                    if let Some(next_body) = next_body {
                        carried_body = Some(next_body);
                    }
                    last_error = Some(error);
                    last_provider = Some(provider.clone());
                }
                AttemptOutcome::Abort { error } => {
                    self.metrics.record_outcome(RequestOutcome::Failed {
                        error: error.to_string(),
                    });
                    return Err(ForwardError {
                        error,
                        provider: Some(provider.clone()),
                    });
                }
            }
        }

        if attempted_providers == 0 {
            // providers 列表非空，但全部被熔断器拒绝（典型：HalfOpen 探测名额被占用）
            self.metrics.record_outcome(RequestOutcome::Failed {
                error: "所有供应商暂时不可用（熔断器限制）".to_string(),
            });
            return Err(ForwardError {
                error: last_error.unwrap_or(ProxyError::NoAvailableProvider),
                provider: last_provider,
            });
        }

        self.metrics.record_outcome(RequestOutcome::Failed {
            error: "所有供应商都失败".to_string(),
        });
        if let Some((log_code, log_message)) =
            build_terminal_failure_log(attempted_providers, providers.len(), last_error.as_ref())
        {
            log::warn!("[{app_type_str}] [{log_code}] {log_message}");
        }

        Err(ForwardError {
            error: last_error.unwrap_or(ProxyError::MaxRetriesExceeded),
            provider: last_provider,
        })
    }

    /// PRE-SEND 优化器：仅 Bedrock 优化时 clone（避免优化字段泄漏到非 Bedrock
    /// provider）；其余情况借用，只有整流真正改写时才写时复制（PERF-04）。
    fn provider_body<'a>(&self, base_body: &'a Value, provider_specific: bool) -> Cow<'a, Value> {
        if !provider_specific {
            return Cow::Borrowed(base_body);
        }
        let mut body = base_body.clone();
        if self.optimizer_config.thinking_optimizer {
            super::thinking_optimizer::optimize(&mut body, &self.optimizer_config);
        }
        if self.optimizer_config.cache_injection {
            super::cache_injector::inject(&mut body, &self.optimizer_config);
        }
        Cow::Owned(body)
    }

    /// One provider's attempt: send, and on a rectifiable error retry the same
    /// provider once with the rewritten body.
    #[allow(clippy::too_many_arguments)]
    async fn attempt_provider(
        &self,
        request: &AttemptRequest<'_>,
        provider: &Provider,
        mut provider_body: Cow<'_, Value>,
        base_body: &Value,
        provider_specific_body: bool,
        permit: &mut HalfOpenPermit,
        position: (usize, usize),
    ) -> AttemptOutcome {
        let app_type_str = request.app_type.as_str();
        let error = match self.forward(request, provider, &provider_body).await {
            Ok(served) => return AttemptOutcome::served(served),
            Err(error) => error,
        };

        let provider_type = ProviderType::from_app_type_and_config(request.app_type, provider);
        let is_anthropic_provider = matches!(
            provider_type,
            ProviderType::Claude | ProviderType::ClaudeAuth
        );
        let max_output_tokens = provider_max_output_tokens(provider);
        let decision = plan_rectification(
            &self.rectifier_config,
            request.adapter.name(),
            is_anthropic_provider,
            &error,
            &mut provider_body,
            max_output_tokens,
        );

        let kind = match decision {
            RectifyDecision::NotApplicable => {
                return self
                    .classify_failure(error, provider, app_type_str, permit, None, position)
                    .await;
            }
            RectifyDecision::RejectClient => {
                // The rectifier matched but nothing could be rewritten: the request
                // is invalid for every provider. Neutral release (client problem).
                self.router
                    .release_permit_neutral(&provider.id, app_type_str, permit.take())
                    .await;
                return AttemptOutcome::Abort { error };
            }
            RectifyDecision::Retry(kind) => kind,
        };

        log::info!(
            "[{app_type_str}] [{}] {} rectifier rewrote the request; retrying provider={}",
            kind.log_code(),
            kind.label(),
            provider.id
        );
        match self.forward(request, provider, &provider_body).await {
            Ok(served) => {
                log::info!(
                    "[{app_type_str}] [{}] {} rectifier retry succeeded",
                    kind.log_code(),
                    kind.label()
                );
                AttemptOutcome::served(served)
            }
            Err(retry_err) => {
                log::warn!(
                    "[{app_type_str}] [{}] {} rectifier retry still failed: {retry_err}",
                    kind.log_code(),
                    kind.label()
                );
                // PROXY-42: the retry failure is classified exactly like a first
                // failure (429/401/... still fail over and count against the breaker).
                match self
                    .classify_failure(
                        retry_err,
                        provider,
                        app_type_str,
                        permit,
                        Some(kind),
                        position,
                    )
                    .await
                {
                    AttemptOutcome::Failover { error, .. } if kind.carries_to_next_provider() => {
                        let carried = carry_rectified_body(
                            base_body,
                            provider_body,
                            provider_specific_body,
                            |b| kind.reapply(b, max_output_tokens),
                        );
                        AttemptOutcome::Failover {
                            error,
                            carried_body: Some(carried),
                        }
                    }
                    other => other,
                }
            }
        }
    }

    /// Record a failed attempt and decide between failover and abort.
    async fn classify_failure(
        &self,
        error: ProxyError,
        provider: &Provider,
        app_type_str: &str,
        permit: &mut HalfOpenPermit,
        after_rectifier: Option<RectifierKind>,
        (attempted, total): (usize, usize),
    ) -> AttemptOutcome {
        // NonRetryable / ClientAbort are client-side problems every provider would
        // reject: they must not pollute the breaker or health (neutral release).
        match self.categorize_proxy_error(&error, provider) {
            ErrorCategory::Retryable => {
                let _ = self
                    .router
                    .record_result(
                        &provider.id,
                        app_type_str,
                        permit.take(),
                        false,
                        Some(error.to_string()),
                    )
                    .await;
                self.metrics.set_last_error(match after_rectifier {
                    Some(kind) => format!(
                        "Provider {} {}重试失败: {}",
                        provider.name,
                        kind.label(),
                        error
                    ),
                    None => format!("Provider {} 失败: {}", provider.name, error),
                });
                let (log_code, log_message) =
                    build_retryable_failure_log(&provider.name, attempted, total, &error);
                log::warn!("[{app_type_str}] [{log_code}] {log_message}");
                AttemptOutcome::Failover {
                    error,
                    carried_body: None,
                }
            }
            ErrorCategory::NonRetryable | ErrorCategory::ClientAbort => {
                self.router
                    .release_permit_neutral(&provider.id, app_type_str, permit.take())
                    .await;
                AttemptOutcome::Abort { error }
            }
        }
    }

    /// Success bookkeeping (one place for every success path).
    #[allow(clippy::too_many_arguments)]
    async fn commit_success(
        &self,
        app_type_str: &str,
        provider: &Provider,
        permit: &mut HalfOpenPermit,
        response: ProxyResponse,
        claude_api_format: Option<String>,
        outbound_model: Option<String>,
        after_failover: bool,
    ) -> ForwardResult {
        // 普通闭合熔断状态异步记录，避免阻塞流式首包返回；HalfOpen 探测同步等待，
        // 保证 permit 与熔断状态及时释放。
        let stream_health = self
            .record_success_or_defer(provider, app_type_str, permit.take(), response.is_sse())
            .await;
        self.current_providers.write().await.insert(
            app_type_str.to_string(),
            (provider.id.clone(), provider.name.clone()),
        );
        // PROXY-27: a failover is an earlier provider failing for this request, not
        // "served provider differs from the locally selected one".
        self.metrics.record_outcome(RequestOutcome::Served {
            failover: after_failover,
        });
        if self.current_provider_id_at_start != provider.id {
            // 同步 UI/托盘的"当前供应商"；FailoverSwitchManager 对重复目标去重。
            let fm = self.failover_manager.clone();
            let ah = self.app_handle.clone();
            let pid = provider.id.clone();
            let pname = provider.name.clone();
            let at = app_type_str.to_string();
            tokio::spawn(async move {
                let _ = fm.try_switch(ah.as_ref(), &at, &pid, &pname).await;
            });
        }
        ForwardResult {
            response,
            provider: provider.clone(),
            claude_api_format,
            outbound_model,
            connection_guard: None,
            stream_health,
        }
    }

    /// 转发单个请求：route → body → endpoint → auth → headers → transport → validate。
    ///
    /// 成功时返回 `(response, claude_api_format, outbound_model)`，其中
    /// `outbound_model` 是最终发往上游的模型名（所有映射/改写之后）。
    async fn forward(
        &self,
        request: &AttemptRequest<'_>,
        provider: &Provider,
        body: &Value,
    ) -> Result<(ProxyResponse, Option<String>, Option<String>), ProxyError> {
        let AttemptRequest {
            app_type,
            method,
            endpoint,
            headers,
            extensions,
            adapter,
        } = *request;

        // Route.
        let base_url = adapter.extract_base_url(provider)?;
        let route = Route::resolve(app_type, provider, endpoint, &base_url);
        if route.codex_official_auth_passthrough {
            validate_codex_official_authorization(headers)?;
        }

        // Body: model mapping and optimizers.
        let (mut mapped, copilot_hints) = self
            .map_request_body(app_type, provider, &route, body, headers)
            .await?;
        let base_url = if route.is_copilot && !route.is_full_url {
            copilot_base_url(self.app_handle.as_ref(), provider, base_url).await
        } else {
            base_url
        };
        let claude_api_format = if adapter.name() == "Claude" {
            let format = self
                .resolve_claude_api_format(provider, &mapped, route.is_copilot)
                .await;
            super::providers::normalize_anthropic_messages_for_provider(
                &mut mapped,
                provider,
                &format,
            );
            self.apply_media_prevention(&mut mapped, provider);
            Some(format)
        } else {
            None
        };
        let needs_transform = match claude_api_format.as_deref() {
            Some(format) => super::providers::claude_api_format_needs_transform(format),
            None => adapter.needs_transform(provider),
        };

        // Endpoint.
        let target = resolve_upstream_target(
            adapter,
            provider,
            &base_url,
            endpoint,
            &route,
            claude_api_format.as_deref(),
            needs_transform,
            &mapped,
        );

        // Body: protocol transform + outbound policy. The pre-transform model is
        // kept when the transform moves it into the URL (gemini_native).
        let mut outbound_model = model_of(&mapped);
        let transformed = self
            .transform_request_body(
                app_type,
                provider,
                &route,
                adapter,
                mapped,
                claude_api_format.as_deref(),
                needs_transform,
            )
            .await?;
        let request_body = transformed.body;
        if let Some(model) = model_of(&request_body) {
            outbound_model = Some(model);
        }
        super::pipeline::body::log_prompt_cache_trace(
            app_type,
            provider,
            &target.effective_endpoint,
            claude_api_format.as_deref(),
            &request_body,
            self.session_client_provided,
        );
        let streaming = is_streaming_request(&target.effective_endpoint, &request_body, headers);

        // Auth + headers.
        let native_anthropic =
            adapter.name() == "Claude" && matches!(claude_api_format.as_deref(), Some("anthropic"));
        let upstream_headers = self
            .upstream_headers(
                request,
                provider,
                &route,
                &target.url,
                copilot_hints.as_ref(),
                HeaderFlags {
                    native_anthropic,
                    force_identity_encoding: needs_transform || route.is_bridged() || streaming,
                    codex_anthropic_one_m: transformed.codex_anthropic_one_m,
                },
            )
            .await?;
        reject_proxy_placeholder_for_managed_account_upstream(&target.url, &upstream_headers)?;

        // GET/HEAD carry no body (e.g. Gemini models.list rejects one).
        let body_bytes = if matches!(*method, http::Method::GET | http::Method::HEAD) {
            Vec::new()
        } else {
            serde_json::to_vec(&request_body).map_err(|e| {
                ProxyError::Internal(format!("Failed to serialize request body: {e}"))
            })?
        };
        log_outbound_request(adapter.name(), &target.url, &request_body, &body_bytes);

        // Transport.
        let upstream_proxy_url = super::http_client::get_current_proxy_url();
        let (response, started) = transport::send(
            UpstreamRequest {
                url: target.url,
                method: method.clone(),
                headers: upstream_headers,
                extensions: extensions.clone(),
                body: body_bytes,
                streaming,
                preserve_exact_header_case: should_preserve_exact_header_case(
                    adapter.name(),
                    provider,
                    claude_api_format.as_deref(),
                    route.is_copilot,
                ),
            },
            &self.timeouts,
            upstream_proxy_url.as_deref(),
        )
        .await?;

        // Classify.
        let response = match self
            .validate_response(
                response,
                started,
                streaming,
                ResponseChecks::for_route(
                    &route,
                    adapter.name(),
                    claude_api_format.as_deref(),
                    &target.effective_endpoint,
                ),
            )
            .await
        {
            Ok(response) => response,
            Err(err) => {
                // Copilot: a `model_not_supported` 400 means the cached model list
                // is stale; refresh it so the next request can recover.
                if route.is_copilot {
                    if let (ProxyError::UpstreamError { status, body }, Some(app)) =
                        (&err, &self.app_handle)
                    {
                        let account = provider
                            .meta
                            .as_ref()
                            .and_then(|m| m.managed_account_id_for("github_copilot"));
                        app.state::<CopilotAuthState>()
                            .0
                            .read()
                            .await
                            .note_upstream_error(account.as_deref(), *status, body.as_deref())
                            .await;
                    }
                }
                return Err(err);
            }
        };
        Ok((response, claude_api_format, outbound_model))
    }

    /// Credentials and the header policy for one attempt.
    async fn upstream_headers(
        &self,
        request: &AttemptRequest<'_>,
        provider: &Provider,
        route: &Route,
        url: &str,
        copilot_hints: Option<&super::pipeline::body::CopilotHints>,
        flags: HeaderFlags,
    ) -> Result<http::HeaderMap, ProxyError> {
        let auth = resolve_auth(request.adapter, provider, self.app_handle.as_ref()).await?;
        let mut auth_headers = auth.headers;
        if let Some(hints) = copilot_hints {
            apply_copilot_hint_headers(&mut auth_headers, hints, &self.copilot_optimizer_config);
        }
        Ok(HeaderPolicy {
            client_headers: request.headers,
            auth_headers: &auth_headers,
            upstream_host: upstream_authority(url),
            passthrough_client_authorization: route.codex_official_auth_passthrough,
            codex_responses_to_anthropic: route.codex_responses_to_anthropic,
            codex_impersonate_claude_code: route.codex_impersonate_claude_code,
            force_identity_encoding: flags.force_identity_encoding,
            is_copilot: route.is_copilot,
            custom_user_agent: custom_user_agent(
                provider,
                route.is_copilot,
                route.codex_impersonate_claude_code,
            ),
            anthropic_beta: anthropic_beta_value(
                request.headers,
                flags.native_anthropic,
                route.codex_impersonate_claude_code,
                flags.codex_anthropic_one_m,
            ),
            send_anthropic_headers: flags.native_anthropic,
            // Codex OAuth mirrors the official CLI's session routing headers, but
            // only with a client-provided id (a generated one breaks prefix caching).
            override_headers: if auth.codex_oauth && self.session_client_provided {
                build_codex_oauth_session_headers(&self.upstream_session_id)
            } else {
                Vec::new()
            },
            local_overrides: provider
                .meta
                .as_ref()
                .and_then(|meta| meta.local_proxy_request_overrides.as_ref()),
        }
        .build())
    }

    /// Classify the upstream response inside the retry loop: non-2xx become
    /// `UpstreamError`; 2xx bodies/streams are primed and semantically validated
    /// so an error disguised as success can still fail over.
    async fn validate_response(
        &self,
        response: ProxyResponse,
        started: tokio::time::Instant,
        streaming: bool,
        checks: ResponseChecks,
    ) -> Result<ProxyResponse, ProxyError> {
        if !response.status().is_success() {
            return Err(transport::upstream_error(
                response,
                self.timeouts.response_headers(streaming),
            )
            .await);
        }
        let response =
            transport::prepare_success_response(response, streaming, &self.timeouts, started)
                .await?;
        // A streaming request answered with JSON (or a non-streaming one) is
        // buffered and checked for an error envelope. Unknown content types are
        // not buffered: some gateways omit the SSE header.
        if !streaming || response.is_json() {
            return match checks.envelope {
                Some(format) => validate_buffered_success(response, format).await,
                None => Ok(response),
            };
        }
        match checks.stream {
            // Responses streams are always validated (they keep priming through
            // lifecycle events); other formats only when a first-byte limit is set
            // (PROXY-41).
            Some(StreamFormat::Responses) => {
                validate_stream_start(response, StreamFormat::Responses, self.timeouts.first_byte)
                    .await
            }
            Some(format) if self.timeouts.first_byte.is_some() => {
                validate_stream_start(response, format, self.timeouts.first_byte).await
            }
            _ => Ok(response),
        }
    }

    async fn resolve_claude_api_format(
        &self,
        provider: &Provider,
        body: &Value,
        is_copilot: bool,
    ) -> String {
        if !is_copilot {
            return super::providers::get_claude_api_format(provider).to_string();
        }

        let model = body.get("model").and_then(|value| value.as_str());
        if let Some(model_id) = model {
            if self
                .is_copilot_openai_vendor_model(provider, model_id)
                .await
            {
                return "openai_responses".to_string();
            }
        }

        "openai_chat".to_string()
    }

    /// 用 Copilot live `/models` 列表确认 model ID 真实可用，找不到时按 family 降级。
    /// 命中缓存后是同步的；首次请求或 5 min 缓存过期后会触发一次 HTTP。
    pub(crate) async fn apply_copilot_live_model_resolution(
        &self,
        provider: &Provider,
        body: &mut serde_json::Value,
    ) {
        let Some(model_id) = body.get("model").and_then(|v| v.as_str()) else {
            return;
        };
        let model_id = model_id.to_string();

        let Some(app_handle) = &self.app_handle else {
            return;
        };
        let copilot_state = app_handle.state::<CopilotAuthState>();
        let copilot_auth = copilot_state.0.read().await;
        let account_id = provider
            .meta
            .as_ref()
            .and_then(|m| m.managed_account_id_for("github_copilot"));

        let models_result = match account_id.as_deref() {
            Some(id) => copilot_auth.fetch_models_for_account(id).await,
            None => copilot_auth.fetch_models().await,
        };

        let models = match models_result {
            Ok(m) => m,
            Err(err) => {
                log::debug!("[Copilot] live model list unavailable, skip resolution: {err}");
                return;
            }
        };

        if let Some(resolved) =
            super::providers::copilot_model_map::resolve_against_models(&model_id, &models)
        {
            log::info!("[Copilot] live-model resolve: {model_id} → {resolved}");
            body["model"] = serde_json::Value::String(resolved);
        }
    }

    async fn is_copilot_openai_vendor_model(&self, provider: &Provider, model_id: &str) -> bool {
        let Some(app_handle) = &self.app_handle else {
            log::debug!("[Copilot] AppHandle unavailable, fallback to chat/completions");
            return false;
        };

        let copilot_state = app_handle.state::<CopilotAuthState>();
        let copilot_auth = copilot_state.0.read().await;
        let account_id = provider
            .meta
            .as_ref()
            .and_then(|m| m.managed_account_id_for("github_copilot"));

        let vendor_result = match account_id.as_deref() {
            Some(id) => {
                copilot_auth
                    .get_model_vendor_for_account(id, model_id)
                    .await
            }
            None => copilot_auth.get_model_vendor(model_id).await,
        };

        match vendor_result {
            Ok(Some(vendor)) => vendor.eq_ignore_ascii_case("openai"),
            Ok(None) => {
                log::debug!(
                    "[Copilot] Model vendor unavailable for {model_id}, fallback to chat/completions"
                );
                false
            }
            Err(err) => {
                log::warn!(
                    "[Copilot] Failed to resolve model vendor for {model_id}, fallback to chat/completions: {err}"
                );
                false
            }
        }
    }

    fn categorize_proxy_error(&self, error: &ProxyError, provider: &Provider) -> ErrorCategory {
        // Authentication belongs to the Codex client for the built-in official
        // route. Retrying another provider would silently move the conversation
        // away from the selected official account and poison its health state.
        if super::providers::is_codex_official_provider(provider)
            && (matches!(error, ProxyError::AuthError(_))
                || matches!(
                    error,
                    ProxyError::UpstreamError {
                        status: 401 | 403,
                        ..
                    }
                ))
        {
            return ErrorCategory::NonRetryable;
        }

        match error {
            // 网络和上游错误：都应该尝试下一个供应商
            ProxyError::Timeout(_) => ErrorCategory::Retryable,
            ProxyError::ForwardFailed(_) => ErrorCategory::Retryable,
            ProxyError::ProviderUnhealthy(_) => ErrorCategory::Retryable,
            // 上游 HTTP 错误：按状态码分桶。
            //
            // 客户端请求自身有问题的状态码无论换哪个 provider 都会被拒绝，
            // 继续轮询只会放大错误率、污染熔断器健康度、浪费配额：
            //   400 Bad Request / 422 Unprocessable Entity   ← 请求体格式或语义错误
            //   405 Method Not Allowed / 406 Not Acceptable  ← 方法或 Accept 错误
            //   413 Payload Too Large / 414 URI Too Long     ← 客户端构造超限
            //   415 Unsupported Media Type                    ← Content-Type 错误
            //   501 Not Implemented                           ← 上游协议确实不支持
            //
            // 其他 4xx（401/403/404/408/409/429/451 等）和全部 5xx 都保留
            // Retryable —— 换一家 provider 可能持有不同的 key、配额、地域或模型映射。
            ProxyError::UpstreamError { status, .. } => match *status {
                400 | 405 | 406 | 413 | 414 | 415 | 422 | 501 => ErrorCategory::NonRetryable,
                _ => ErrorCategory::Retryable,
            },
            // Provider 级配置/转换问题：换一个 Provider 可能就能成功
            ProxyError::ConfigError(_) => ErrorCategory::Retryable,
            ProxyError::TransformError(_) => ErrorCategory::Retryable,
            ProxyError::AuthError(_) => ErrorCategory::Retryable,
            ProxyError::StreamIdleTimeout(_) => ErrorCategory::Retryable,
            // 无可用供应商：所有供应商都试过了，无法重试
            ProxyError::NoAvailableProvider => ErrorCategory::NonRetryable,
            // 其他错误（数据库/内部错误等）：不是换供应商能解决的问题
            _ => ErrorCategory::NonRetryable,
        }
    }
}

/// Header-policy inputs derived from the route and the transformed body.
#[derive(Debug, Clone, Copy)]
struct HeaderFlags {
    native_anthropic: bool,
    force_identity_encoding: bool,
    codex_anthropic_one_m: bool,
}

/// Which semantic checks a 2xx response gets inside the retry loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct ResponseChecks {
    /// Buffered/JSON bodies: error-envelope check.
    envelope: Option<StreamFormat>,
    /// Streams: stream-start check for this upstream format.
    stream: Option<StreamFormat>,
}

impl ResponseChecks {
    fn for_route(
        route: &Route,
        adapter_name: &str,
        claude_api_format: Option<&str>,
        effective_endpoint: &str,
    ) -> Self {
        let envelope = if route.codex_responses_to_anthropic {
            Some(StreamFormat::Anthropic)
        } else if claude_api_format == Some("openai_responses") {
            Some(StreamFormat::Responses)
        } else {
            None
        };
        let stream = if route.codex_responses_to_anthropic {
            Some(StreamFormat::Anthropic)
        } else if route.codex_responses_to_chat {
            Some(StreamFormat::OpenAiChat)
        } else {
            match claude_api_format {
                Some("openai_responses") => Some(StreamFormat::Responses),
                Some("anthropic") => Some(StreamFormat::Anthropic),
                Some("openai_chat") => Some(StreamFormat::OpenAiChat),
                Some("gemini_native") => Some(StreamFormat::Gemini),
                Some(_) => None,
                None if adapter_name == "Gemini" => Some(StreamFormat::Gemini),
                None if adapter_name == "Codex"
                    && super::pipeline::endpoint::split_endpoint_and_query(effective_endpoint)
                        .0
                        .ends_with("/chat/completions") =>
                {
                    Some(StreamFormat::OpenAiChat)
                }
                None => None,
            }
        };
        Self { envelope, stream }
    }
}

fn model_of(body: &Value) -> Option<String> {
    body.get("model")
        .and_then(|m| m.as_str())
        .filter(|m| !m.is_empty())
        .map(str::to_string)
}

/// `host[:port]` of the upstream URL, written in place of the client's `host`.
fn upstream_authority(url: &str) -> Option<String> {
    url.parse::<http::Uri>()
        .ok()
        .and_then(|u| u.authority().map(|a| a.to_string()))
}

fn provider_max_output_tokens(provider: &Provider) -> Option<u64> {
    provider
        .meta
        .as_ref()
        .and_then(|meta| meta.max_output_tokens)
        .filter(|v| *v > 0)
}

fn log_outbound_request(tag: &str, url: &str, body: &Value, body_bytes: &[u8]) {
    let request_model = body
        .get("model")
        .and_then(|v| v.as_str())
        .unwrap_or("<none>");
    log::info!("[{tag}] >>> 请求 URL: {url} (model={request_model})");
    if log::log_enabled!(log::Level::Debug) {
        let body_str = String::from_utf8_lossy(body_bytes);
        log::debug!(
            "[{tag}] >>> 请求体内容 ({}字节): {}",
            body_bytes.len(),
            super::http_client::log_body_excerpt(&body_str)
        );
    }
}

/// 检测 Provider 是否为 Bedrock（通过 CLAUDE_CODE_USE_BEDROCK 环境变量判断）
fn is_bedrock_provider(provider: &Provider) -> bool {
    provider
        .settings_config
        .get("env")
        .and_then(|e| e.get("CLAUDE_CODE_USE_BEDROCK"))
        .and_then(|v| v.as_str())
        .map(|v| v == "1")
        .unwrap_or(false)
}

fn build_retryable_failure_log(
    provider_name: &str,
    attempted_providers: usize,
    total_providers: usize,
    error: &ProxyError,
) -> (&'static str, String) {
    let error_summary = summarize_proxy_error(error);

    if total_providers <= 1 {
        (
            log_fwd::SINGLE_PROVIDER_FAILED,
            format!("Provider {provider_name} 请求失败: {error_summary}"),
        )
    } else {
        (
            log_fwd::PROVIDER_FAILED_RETRY,
            format!(
                "Provider {provider_name} 失败，继续尝试下一个 ({attempted_providers}/{total_providers}): {error_summary}"
            ),
        )
    }
}

fn build_terminal_failure_log(
    attempted_providers: usize,
    total_providers: usize,
    last_error: Option<&ProxyError>,
) -> Option<(&'static str, String)> {
    if total_providers <= 1 {
        return None;
    }

    let error_summary = last_error
        .map(summarize_proxy_error)
        .unwrap_or_else(|| "未知错误".to_string());

    Some((
        log_fwd::ALL_PROVIDERS_FAILED,
        format!(
            "已尝试 {attempted_providers}/{total_providers} 个 Provider，均失败。最后错误: {error_summary}"
        ),
    ))
}

fn summarize_proxy_error(error: &ProxyError) -> String {
    match error {
        ProxyError::UpstreamError { status, body } => {
            let body_summary = body
                .as_deref()
                .map(summarize_upstream_body)
                .filter(|summary| !summary.is_empty());

            match body_summary {
                Some(summary) => format!("上游 HTTP {status}: {summary}"),
                None => format!("上游 HTTP {status}"),
            }
        }
        ProxyError::Timeout(message) => {
            format!("请求超时: {}", summarize_text_for_log(message, 180))
        }
        ProxyError::ForwardFailed(message) => {
            format!("请求转发失败: {}", summarize_text_for_log(message, 180))
        }
        ProxyError::TransformError(message) => {
            format!("响应转换失败: {}", summarize_text_for_log(message, 180))
        }
        ProxyError::ConfigError(message) => {
            format!("配置错误: {}", summarize_text_for_log(message, 180))
        }
        ProxyError::AuthError(message) => {
            format!("认证失败: {}", summarize_text_for_log(message, 180))
        }
        _ => summarize_text_for_log(&error.to_string(), 180),
    }
}

fn summarize_upstream_body(body: &str) -> String {
    if let Ok(json_body) = serde_json::from_str::<Value>(body) {
        if let Some(message) = extract_json_error_message(&json_body) {
            return summarize_text_for_log(&message, 180);
        }

        if let Ok(compact_json) = serde_json::to_string(&json_body) {
            return summarize_text_for_log(&compact_json, 180);
        }
    }

    summarize_text_for_log(body, 180)
}

fn extract_json_error_message(body: &Value) -> Option<String> {
    let candidates = [
        body.pointer("/error/message"),
        body.pointer("/message"),
        body.pointer("/detail"),
        body.pointer("/error"),
    ];

    candidates
        .into_iter()
        .flatten()
        .find_map(|value| value.as_str().map(ToString::to_string))
}

fn summarize_text_for_log(text: &str, max_chars: usize) -> String {
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let trimmed = normalized.trim();

    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }

    let truncated: String = trimmed.chars().take(max_chars).collect();
    let truncated = truncated.trim_end();
    format!("{truncated}...")
}

/// 计算故障转移时下一家 provider 的起始请求体（PROXY-24）。
///
/// 整流（thinking 签名 / budget）与 provider 无关：非 provider 专属 body 时直接
/// 复用已整流的 `provider_body`（零额外 clone）；Bedrock 优化过的 body 含专属字段，
/// 此时在 `base_body` 副本上重放同一整流。
fn carry_rectified_body(
    base_body: &Value,
    provider_body: Cow<'_, Value>,
    provider_specific_body: bool,
    rectify: impl FnOnce(&mut Value),
) -> Value {
    if provider_specific_body {
        let mut next = base_body.clone();
        rectify(&mut next);
        next
    } else {
        provider_body.into_owned()
    }
}

impl RequestForwarder {
    /// Run the (blocking) budget check off the async workers. Any internal error
    /// fails open: a broken usage query must never block the user's request.
    async fn check_budget_breach(
        &self,
        app_type: &str,
        provider: &Provider,
    ) -> Option<(
        crate::error::AppError,
        crate::services::usage_stats::ProviderLimitStatus,
    )> {
        let db = self.router.db().clone();
        let app_type = app_type.to_string();
        let provider_id = provider.id.clone();
        match tokio::task::spawn_blocking(move || {
            crate::services::budget::hard_limit_breach(&db, &app_type, &provider_id)
        })
        .await
        {
            Ok(Ok(breach)) => breach,
            Ok(Err(e)) => {
                log::debug!("budget check failed (allowing request): {e}");
                None
            }
            Err(e) => {
                log::debug!("budget check task failed (allowing request): {e}");
                None
            }
        }
    }
}

fn notify_budget_breach(
    app: &tauri::AppHandle,
    app_type: &str,
    provider: &Provider,
    status: &crate::services::usage_stats::ProviderLimitStatus,
) {
    use crate::services::notifications::{notify_budget_exceeded, BudgetPeriod};
    let parse = |v: &str| v.parse::<f64>().unwrap_or(0.0);
    let (period, spent, limit) = if status.daily_exceeded {
        (
            BudgetPeriod::Daily,
            parse(&status.daily_usage),
            status.daily_limit.as_deref().map(parse).unwrap_or(0.0),
        )
    } else {
        (
            BudgetPeriod::Monthly,
            parse(&status.monthly_usage),
            status.monthly_limit.as_deref().map(parse).unwrap_or(0.0),
        )
    };
    notify_budget_exceeded(
        app,
        app_type,
        &provider.id,
        &provider.name,
        period,
        spent,
        limit,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;
    use crate::provider::LocalProxyRequestOverrides;
    use crate::proxy::json_canonical::short_value_hash;
    use crate::proxy::pipeline::body::CLAUDE_CODE_SYSTEM_IDENTITY;
    use crate::proxy::pipeline::{
        body::{
            apply_local_proxy_body_overrides, codex_anthropic_cache_config,
            prepare_upstream_request_body, prepend_claude_code_system_prompt,
        },
        endpoint::{
            append_query_to_full_url, base_url_is_full_endpoint, rewrite_claude_transform_endpoint,
            rewrite_codex_responses_endpoint_to_anthropic,
            rewrite_codex_responses_endpoint_to_chat,
        },
        headers::{
            apply_local_proxy_header_overrides, connection_listed_request_headers,
            is_codex_client_fingerprint_header, is_hop_by_hop_request_header,
        },
        validate::{
            anthropic_error_envelope_message as codex_anthropic_error_envelope_message,
            inspect_responses_json_document, inspect_responses_start_event,
            responses_error_envelope_message,
        },
    };
    use axum::http::header::{HeaderValue, ACCEPT};
    use axum::http::HeaderMap;
    use bytes::Bytes;
    use futures::StreamExt;
    use http::StatusCode;
    use serde_json::json;
    use std::collections::HashMap;
    use std::time::Duration;

    fn test_provider_with_type(provider_type: Option<&str>) -> Provider {
        Provider {
            id: "provider-1".to_string(),
            name: "Provider 1".to_string(),
            settings_config: json!({}),
            website_url: None,
            category: None,
            created_at: None,
            sort_index: None,
            notes: None,
            meta: provider_type.map(|value| crate::provider::ProviderMeta {
                provider_type: Some(value.to_string()),
                ..Default::default()
            }),
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        }
    }

    fn test_forwarder(
        non_streaming_timeout: Duration,
        streaming_first_byte_timeout: Duration,
    ) -> RequestForwarder {
        let db = Arc::new(Database::memory().expect("memory db"));

        RequestForwarder {
            router: Arc::new(ProviderRouter::new(db.clone())),
            metrics: ProxyMetrics::new(),
            current_providers: Arc::new(RwLock::new(HashMap::new())),
            gemini_shadow: Arc::new(GeminiShadowStore::new()),
            codex_chat_history: Arc::new(CodexChatHistoryStore::default()),
            failover_manager: Arc::new(FailoverSwitchManager::new(db)),
            app_handle: None,
            current_provider_id_at_start: String::new(),
            session_id: String::new(),
            upstream_session_id: String::new(),
            session_client_provided: false,
            rectifier_config: RectifierConfig::default(),
            optimizer_config: OptimizerConfig::default(),
            copilot_optimizer_config: CopilotOptimizerConfig::default(),
            timeouts: TimeoutPolicy::from_secs(
                non_streaming_timeout.as_secs(),
                streaming_first_byte_timeout.as_secs(),
                0,
            ),
            max_attempts: 1,
        }
    }

    impl RequestForwarder {
        /// Test seam kept from the pre-pipeline forwarder.
        async fn prepare_success_response_for_failover(
            &self,
            response: ProxyResponse,
            streaming: bool,
        ) -> Result<ProxyResponse, ProxyError> {
            transport::prepare_success_response(
                response,
                streaming,
                &self.timeouts,
                tokio::time::Instant::now(),
            )
            .await
        }

        fn media_retry_should_trigger(
            &self,
            adapter_name: &str,
            already_retried: bool,
            body: &Value,
            error: &ProxyError,
        ) -> bool {
            !already_retried
                && plan_rectification(
                    &self.rectifier_config,
                    adapter_name,
                    false,
                    error,
                    &mut Cow::Borrowed(body),
                    None,
                ) == RectifyDecision::Retry(RectifierKind::Media)
        }
    }

    fn should_force_identity_encoding(
        endpoint: &str,
        body: &Value,
        headers: &axum::http::HeaderMap,
    ) -> bool {
        is_streaming_request(endpoint, body, headers)
    }

    #[test]
    fn single_provider_retryable_log_uses_single_provider_code() {
        let error = ProxyError::UpstreamError {
            status: 429,
            body: Some(r#"{"error":{"message":"rate limit exceeded"}}"#.to_string()),
        };

        let (code, message) = build_retryable_failure_log("PackyCode-response", 1, 1, &error);

        assert_eq!(code, log_fwd::SINGLE_PROVIDER_FAILED);
        assert!(message.contains("Provider PackyCode-response 请求失败"));
        assert!(message.contains("上游 HTTP 429"));
        assert!(message.contains("rate limit exceeded"));
        assert!(!message.contains("切换下一个"));
    }

    #[test]
    fn multi_provider_retryable_log_keeps_failover_wording() {
        let error = ProxyError::Timeout("upstream timed out after 30s".to_string());

        let (code, message) = build_retryable_failure_log("primary", 1, 3, &error);

        assert_eq!(code, log_fwd::PROVIDER_FAILED_RETRY);
        assert!(message.contains("继续尝试下一个 (1/3)"));
        assert!(message.contains("请求超时"));
    }

    #[test]
    fn single_provider_has_no_terminal_all_failed_log() {
        assert!(build_terminal_failure_log(1, 1, None).is_none());
    }

    #[test]
    fn multi_provider_terminal_log_contains_last_error_summary() {
        let error = ProxyError::ForwardFailed("connection reset by peer".to_string());

        let (code, message) =
            build_terminal_failure_log(2, 2, Some(&error)).expect("expected terminal log");

        assert_eq!(code, log_fwd::ALL_PROVIDERS_FAILED);
        assert!(message.contains("已尝试 2/2 个 Provider，均失败"));
        assert!(message.contains("connection reset by peer"));
    }

    #[test]
    fn summarize_upstream_body_prefers_json_message() {
        let body = json!({
            "error": {
                "message": "invalid_request_error: unsupported field"
            },
            "request_id": "req_123"
        });

        let summary = summarize_upstream_body(&body.to_string());

        assert_eq!(summary, "invalid_request_error: unsupported field");
    }

    #[test]
    fn summarize_text_for_log_collapses_whitespace_and_truncates() {
        let summary = summarize_text_for_log("line1\n\n line2   line3", 12);

        assert_eq!(summary, "line1 line2...");
    }

    #[test]
    fn canonical_json_sorts_object_keys_for_cache_trace_hashes() {
        let left = json!({
            "tools": [
                {
                    "parameters": {
                        "properties": {
                            "b": {"type": "string"},
                            "a": {"type": "number"}
                        },
                        "type": "object"
                    },
                    "name": "lookup"
                }
            ]
        });
        let right = json!({
            "tools": [
                {
                    "name": "lookup",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "a": {"type": "number"},
                            "b": {"type": "string"}
                        }
                    }
                }
            ]
        });

        assert_eq!(
            crate::proxy::json_canonical::canonical_json_string(&left),
            crate::proxy::json_canonical::canonical_json_string(&right)
        );
        assert_eq!(
            short_value_hash(Some(&left)),
            short_value_hash(Some(&right))
        );
    }

    #[test]
    fn prepare_upstream_request_body_filters_private_fields_and_canonicalizes_order() {
        let body = json!({
            "z": 1,
            "_internal": "drop",
            "tools": [
                {
                    "name": "lookup",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "_id": {
                                "_private_note": "drop",
                                "type": "string"
                            },
                            "b": {"type": "number"},
                            "a": {"type": "string"}
                        }
                    }
                }
            ],
            "a": 2
        });

        let prepared = prepare_upstream_request_body(body);

        assert!(prepared.get("_internal").is_none());
        assert!(prepared["tools"][0]["parameters"]["properties"]
            .get("_id")
            .is_some());
        // PROXY-37: only the request's own top-level private keys are stripped;
        // tool schemas are user-owned payloads and keep their `_` keys.
        assert!(prepared["tools"][0]["parameters"]["properties"]["_id"]
            .get("_private_note")
            .is_some());
        assert_eq!(
            serde_json::to_string(&prepared).unwrap(),
            r#"{"a":2,"tools":[{"name":"lookup","parameters":{"properties":{"_id":{"_private_note":"drop","type":"string"},"a":{"type":"string"},"b":{"type":"number"}},"type":"object"}}],"z":1}"#
        );
    }

    #[test]
    fn local_proxy_body_overrides_deep_merge_final_body_without_stream() {
        let mut body = json!({
            "model": "before",
            "stream": false,
            "metadata": {
                "keep": true,
                "temperature": 1
            },
            "messages": [{ "role": "user", "content": "hello" }]
        });
        let overrides = LocalProxyRequestOverrides {
            headers: HashMap::new(),
            body: Some(json!({
                "model": "after",
                "stream": true,
                "metadata": {
                    "temperature": 0.2,
                    "top_p": 0.9
                },
                "messages": []
            })),
        };

        assert!(apply_local_proxy_body_overrides(&mut body, &overrides));

        assert_eq!(body["model"], "after");
        assert_eq!(body["stream"], false);
        assert_eq!(body["metadata"]["keep"], true);
        assert_eq!(body["metadata"]["temperature"], 0.2);
        assert_eq!(body["metadata"]["top_p"], 0.9);
        assert_eq!(body["messages"], json!([]));
    }

    #[test]
    fn local_proxy_header_overrides_replace_allowed_headers_only() {
        let mut headers = http::HeaderMap::new();
        headers.insert(
            http::header::USER_AGENT,
            http::HeaderValue::from_static("original"),
        );
        headers.insert(
            http::header::AUTHORIZATION,
            http::HeaderValue::from_static("Bearer good"),
        );
        headers.insert(
            http::header::CONTENT_TYPE,
            http::HeaderValue::from_static("application/json"),
        );

        let overrides = LocalProxyRequestOverrides {
            headers: HashMap::from([
                ("User-Agent".to_string(), "custom".to_string()),
                ("X-Test".to_string(), "ok".to_string()),
                ("Authorization".to_string(), "Bearer bad".to_string()),
                ("Content-Type".to_string(), "text/plain".to_string()),
                ("X-Bad".to_string(), "bad\nvalue".to_string()),
            ]),
            body: None,
        };

        apply_local_proxy_header_overrides(&mut headers, Some(&overrides), false);

        assert_eq!(
            headers
                .get(http::header::USER_AGENT)
                .and_then(|value| value.to_str().ok()),
            Some("custom")
        );
        assert_eq!(
            headers
                .get(http::header::AUTHORIZATION)
                .and_then(|value| value.to_str().ok()),
            Some("Bearer good")
        );
        assert_eq!(
            headers
                .get(http::header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok()),
            Some("application/json")
        );
        assert_eq!(
            headers.get("x-test").and_then(|value| value.to_str().ok()),
            Some("ok")
        );
        assert!(headers.get("x-bad").is_none());
    }

    #[test]
    fn local_proxy_header_overrides_are_skipped_for_copilot() {
        let mut headers = http::HeaderMap::new();
        headers.insert(
            http::header::USER_AGENT,
            http::HeaderValue::from_static("copilot"),
        );
        let overrides = LocalProxyRequestOverrides {
            headers: HashMap::from([("User-Agent".to_string(), "custom".to_string())]),
            body: None,
        };

        apply_local_proxy_header_overrides(&mut headers, Some(&overrides), true);

        assert_eq!(
            headers
                .get(http::header::USER_AGENT)
                .and_then(|value| value.to_str().ok()),
            Some("copilot")
        );
    }

    /// Mock Anthropic upstream: answers each POST with the next scripted
    /// `(status, body)` (the last entry repeats) and records every request body.
    async fn spawn_scripted_upstream(
        script: Vec<(u16, Value)>,
    ) -> (String, Arc<std::sync::Mutex<Vec<Value>>>) {
        let seen = Arc::new(std::sync::Mutex::new(Vec::<Value>::new()));
        let script = Arc::new(script);
        let app = {
            let seen = seen.clone();
            axum::Router::new().fallback(move |body: axum::body::Bytes| {
                let seen = seen.clone();
                let script = script.clone();
                async move {
                    let mut guard = seen.lock().unwrap();
                    guard.push(serde_json::from_slice(&body).unwrap_or(Value::Null));
                    let idx = (guard.len() - 1).min(script.len() - 1);
                    let (status, body) = script[idx].clone();
                    (
                        StatusCode::from_u16(status).unwrap(),
                        [(http::header::CONTENT_TYPE, "application/json")],
                        serde_json::to_string(&body).unwrap(),
                    )
                }
            })
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        (format!("http://{addr}"), seen)
    }

    fn claude_provider(id: &str, base_url: &str) -> Provider {
        let mut provider = test_provider_with_type(None);
        provider.id = id.to_string();
        provider.name = id.to_string();
        provider.settings_config = json!({
            "env": {
                "ANTHROPIC_BASE_URL": base_url,
                "ANTHROPIC_AUTH_TOKEN": "sk-test"
            }
        });
        provider
    }

    fn contains_thinking_block(body: &Value) -> bool {
        body["messages"].as_array().is_some_and(|messages| {
            messages.iter().any(|message| {
                message["content"].as_array().is_some_and(|blocks| {
                    blocks.iter().any(|block| {
                        block["type"] == "thinking" || block.get("signature").is_some()
                    })
                })
            })
        })
    }

    #[tokio::test]
    async fn rectified_body_is_carried_to_the_next_provider() {
        // The pooled HTTP client builds a rustls config; the app installs this at startup.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let signature_error = json!({
            "type": "error",
            "error": {
                "type": "invalid_request_error",
                "message": "messages.1.content.0: Invalid `signature` in `thinking` block"
            }
        });
        let (p1_url, p1_seen) = spawn_scripted_upstream(vec![
            (400, signature_error),
            (500, json!({"error": {"message": "overloaded"}})),
        ])
        .await;
        let (p2_url, p2_seen) = spawn_scripted_upstream(vec![(
            200,
            json!({
                "id": "msg_1",
                "type": "message",
                "role": "assistant",
                "model": "claude-test",
                "content": [{"type": "text", "text": "ok"}],
                "usage": {"input_tokens": 1, "output_tokens": 1}
            }),
        )])
        .await;

        let mut forwarder = test_forwarder(Duration::from_secs(5), Duration::from_secs(5));
        forwarder.max_attempts = 2;
        let body = json!({
            "model": "claude-test",
            "max_tokens": 16,
            "messages": [
                {"role": "user", "content": "hi"},
                {"role": "assistant", "content": [
                    {"type": "thinking", "thinking": "hmm", "signature": "sig-from-elsewhere"},
                    {"type": "text", "text": "hello"}
                ]},
                {"role": "user", "content": "again"}
            ]
        });
        let mut headers = HeaderMap::new();
        headers.insert(
            http::header::CONTENT_TYPE,
            http::HeaderValue::from_static("application/json"),
        );

        let result = forwarder
            .forward_with_retry_inner(
                &AppType::Claude,
                http::Method::POST,
                "/v1/messages",
                &body,
                headers,
                Extensions::new(),
                vec![
                    claude_provider("p1", &p1_url),
                    claude_provider("p2", &p2_url),
                ],
            )
            .await;
        let result = match result {
            Ok(result) => result,
            Err(err) => panic!("expected failover success, got {}", err.error),
        };
        assert_eq!(result.provider.id, "p2");

        let p1_seen = p1_seen.lock().unwrap().clone();
        assert_eq!(p1_seen.len(), 2, "P1: original + rectified retry");
        assert!(contains_thinking_block(&p1_seen[0]));
        assert!(!contains_thinking_block(&p1_seen[1]));

        let p2_seen = p2_seen.lock().unwrap().clone();
        assert_eq!(p2_seen.len(), 1, "P2 must succeed on its first call");
        assert!(
            !contains_thinking_block(&p2_seen[0]),
            "P2 must receive the rectified body"
        );
        // The client's own body is never mutated.
        assert!(contains_thinking_block(&body));
    }

    fn sse_headers() -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            http::header::CONTENT_TYPE,
            HeaderValue::from_static("text/event-stream"),
        );
        headers
    }

    /// Runs one stream that sends `message_start` and then hangs, through the
    /// passthrough stream with a 1s idle timeout. Returns the client-visible bytes.
    async fn run_hanging_stream(forwarder: &RequestForwarder, provider: &Provider) -> String {
        let upstream = futures::stream::once(async {
            Ok::<Bytes, std::io::Error>(Bytes::from_static(
                b"event: message_start\ndata: {\"type\":\"message_start\"}\n\n",
            ))
        })
        .chain(futures::stream::pending());
        let response = ProxyResponse::streamed(StatusCode::OK, sse_headers(), upstream);
        let reporter = forwarder
            .record_success_or_defer(provider, "claude", false, response.is_sse())
            .await;
        assert!(reporter.is_some(), "streamed success must be deferred");
        let mut guard = ActiveConnectionGuard::acquire(forwarder.metrics.clone());
        guard.stream_health = reporter;

        let stream = crate::proxy::response_processor::create_logged_passthrough_stream(
            response.bytes_stream(),
            "test",
            None,
            crate::proxy::handler_context::StreamingTimeoutConfig {
                first_byte_timeout: 0,
                idle_timeout: 1,
            },
            Some(guard),
            crate::proxy::handler_config::StreamErrorFormat::Anthropic,
        );
        let chunks: Vec<_> = stream.collect().await;
        let mut out = String::new();
        for chunk in chunks {
            out.push_str(&String::from_utf8_lossy(&chunk.expect("no io error")));
        }
        out
    }

    #[tokio::test]
    async fn mid_stream_idle_timeouts_open_the_breaker_and_end_with_error_event() {
        let forwarder = test_forwarder(Duration::from_secs(5), Duration::from_secs(5));
        let provider = test_provider_with_type(None);

        // The per-app failure threshold comes from the DB config, so keep feeding
        // "200 + message_start, then hang" streams until the breaker opens.
        let mut streams = 0u32;
        let mut stats = None;
        'outer: for _ in 0..5 {
            let outputs = futures::future::join_all(
                (0..4).map(|_| run_hanging_stream(&forwarder, &provider)),
            )
            .await;
            streams += 4;
            for out in &outputs {
                assert!(out.starts_with("event: message_start"));
                assert!(
                    out.ends_with("\n\n") && out.contains("event: error\ndata: "),
                    "client must receive a terminal error event: {out}"
                );
            }
            // Health is recorded on a spawned task.
            for _ in 0..25 {
                stats = forwarder
                    .router
                    .get_circuit_breaker_stats(&provider.id, "claude")
                    .await;
                if stats
                    .as_ref()
                    .is_some_and(|s| s.state == crate::proxy::circuit_breaker::CircuitState::Open)
                {
                    break 'outer;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        }
        let stats = stats.expect("breaker exists");
        assert_eq!(
            stats.state,
            crate::proxy::circuit_breaker::CircuitState::Open,
            "breaker must open after {streams} mid-stream failures"
        );
        assert_eq!(stats.failed_requests, stats.total_requests);
        let last_error = forwarder.metrics.last_error();
        assert!(last_error.is_some_and(|e| e.contains("流式响应中断")));
    }

    #[tokio::test]
    async fn completed_or_dropped_streams_record_success() {
        let forwarder = test_forwarder(Duration::from_secs(5), Duration::from_secs(5));
        let provider = test_provider_with_type(None);
        let response = ProxyResponse::streamed(
            StatusCode::OK,
            sse_headers(),
            futures::stream::iter(vec![Ok::<Bytes, std::io::Error>(Bytes::from_static(
                b"data: {}\n\n",
            ))]),
        );
        // Dropping the reporter without an explicit outcome (client abort) keeps
        // the old "success" semantics.
        let reporter = forwarder
            .record_success_or_defer(&provider, "claude", false, response.is_sse())
            .await;
        drop(reporter);

        // Non-SSE responses are recorded immediately, no reporter.
        let json = ProxyResponse::buffered(StatusCode::OK, HeaderMap::new(), Bytes::new());
        assert!(forwarder
            .record_success_or_defer(&provider, "claude", false, json.is_sse())
            .await
            .is_none());

        let mut successes = 0;
        for _ in 0..50 {
            successes = forwarder
                .router
                .get_circuit_breaker_stats(&provider.id, "claude")
                .await
                .map(|s| s.total_requests - s.failed_requests)
                .unwrap_or(0);
            if successes >= 2 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(successes, 2);
    }

    #[tokio::test]
    async fn non_streaming_success_is_buffered_before_marking_provider_successful() {
        let forwarder = test_forwarder(Duration::from_secs(1), Duration::from_secs(1));
        let response = ProxyResponse::streamed(
            StatusCode::OK,
            HeaderMap::new(),
            futures::stream::once(async {
                tokio::time::sleep(Duration::from_millis(10)).await;
                Ok::<Bytes, std::io::Error>(Bytes::from_static(b"{\"ok\":true}"))
            }),
        );

        let prepared = forwarder
            .prepare_success_response_for_failover(response, false)
            .await
            .expect("response should be buffered");

        assert_eq!(
            prepared.bytes().await.unwrap(),
            Bytes::from_static(b"{\"ok\":true}")
        );
    }

    #[tokio::test]
    async fn non_streaming_body_read_error_is_retryable_before_success_record() {
        let forwarder = test_forwarder(Duration::from_secs(1), Duration::from_secs(1));
        let response = ProxyResponse::streamed(
            StatusCode::OK,
            HeaderMap::new(),
            futures::stream::once(async {
                Err::<Bytes, std::io::Error>(std::io::Error::other("body boom"))
            }),
        );

        let err = match forwarder
            .prepare_success_response_for_failover(response, false)
            .await
        {
            Ok(_) => panic!("body read errors should fail the attempt"),
            Err(err) => err,
        };

        assert!(matches!(err, ProxyError::ForwardFailed(_)));
    }

    #[tokio::test]
    async fn streaming_success_primes_first_chunk_and_replays_it() {
        let forwarder = test_forwarder(Duration::from_secs(1), Duration::from_secs(1));
        let response = ProxyResponse::streamed(
            StatusCode::OK,
            HeaderMap::new(),
            futures::stream::iter(vec![
                Ok::<Bytes, std::io::Error>(Bytes::from_static(b"first")),
                Ok::<Bytes, std::io::Error>(Bytes::from_static(b"second")),
            ]),
        );

        let prepared = forwarder
            .prepare_success_response_for_failover(response, true)
            .await
            .expect("stream should be primed");

        assert_eq!(
            prepared.bytes().await.unwrap(),
            Bytes::from_static(b"firstsecond")
        );
    }

    #[tokio::test]
    async fn streaming_first_chunk_error_is_retryable_before_success_record() {
        let forwarder = test_forwarder(Duration::from_secs(1), Duration::from_secs(1));
        let response = ProxyResponse::streamed(
            StatusCode::OK,
            HeaderMap::new(),
            futures::stream::once(async {
                Err::<Bytes, std::io::Error>(std::io::Error::other("first chunk boom"))
            }),
        );

        let err = match forwarder
            .prepare_success_response_for_failover(response, true)
            .await
        {
            Ok(_) => panic!("first chunk errors should fail the attempt"),
            Err(err) => err,
        };

        assert!(matches!(err, ProxyError::ForwardFailed(_)));
    }

    #[test]
    fn codex_oauth_session_headers_match_codex_cache_identity() {
        let headers = build_codex_oauth_session_headers("session-123");
        let mut map = HeaderMap::new();
        for (name, value) in headers {
            map.insert(name, value);
        }

        assert_eq!(
            map.get("session_id"),
            Some(&HeaderValue::from_static("session-123"))
        );
        assert_eq!(
            map.get("x-client-request-id"),
            Some(&HeaderValue::from_static("session-123"))
        );
        assert_eq!(
            map.get("x-codex-window-id"),
            Some(&HeaderValue::from_static("session-123:0"))
        );
    }

    #[test]
    fn managed_account_upstream_rejects_proxy_managed_placeholder_header() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            HeaderValue::from_static("Bearer PROXY_MANAGED"),
        );

        let err = reject_proxy_placeholder_for_managed_account_upstream(
            "https://api.githubcopilot.com/chat/completions",
            &headers,
        )
        .expect_err("placeholder should be rejected before upstream");

        assert!(matches!(
            err,
            ProxyError::AuthError(message) if message.contains("PROXY_MANAGED")
        ));
    }

    #[test]
    fn codex_oauth_upstream_rejects_proxy_managed_placeholder_header() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            HeaderValue::from_static("Bearer PROXY_MANAGED"),
        );

        let err = reject_proxy_placeholder_for_managed_account_upstream(
            "https://chatgpt.com/backend-api/codex/responses",
            &headers,
        )
        .expect_err("placeholder should be rejected before upstream");

        assert!(matches!(
            err,
            ProxyError::AuthError(message) if message.contains("PROXY_MANAGED")
        ));
    }

    #[test]
    fn non_managed_upstream_allows_proxy_managed_placeholder_guard() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            HeaderValue::from_static("Bearer PROXY_MANAGED"),
        );

        reject_proxy_placeholder_for_managed_account_upstream(
            "https://api.example.com/v1/messages",
            &headers,
        )
        .expect("guard is scoped to managed-account upstreams");
    }

    #[test]
    fn exact_header_case_preserved_for_native_claude_only() {
        let provider = test_provider_with_type(None);

        assert!(should_preserve_exact_header_case(
            "Claude",
            &provider,
            Some("anthropic"),
            false
        ));
        assert!(!should_preserve_exact_header_case(
            "Claude",
            &provider,
            Some("openai_responses"),
            false
        ));
        assert!(!should_preserve_exact_header_case(
            "Codex", &provider, None, false
        ));
        assert!(!should_preserve_exact_header_case(
            "Gemini", &provider, None, false
        ));
    }

    #[test]
    fn exact_header_case_skipped_for_codex_oauth_and_copilot() {
        let codex_oauth = test_provider_with_type(Some("codex_oauth"));
        let copilot = test_provider_with_type(Some("github_copilot"));

        assert!(!should_preserve_exact_header_case(
            "Claude",
            &codex_oauth,
            Some("openai_responses"),
            false
        ));
        assert!(!should_preserve_exact_header_case(
            "Claude",
            &copilot,
            Some("openai_chat"),
            true
        ));
    }

    #[test]
    fn rewrite_claude_transform_endpoint_strips_beta_for_chat_completions() {
        let (endpoint, passthrough_query) = rewrite_claude_transform_endpoint(
            "/v1/messages?beta=true&foo=bar",
            "openai_chat",
            false,
            &json!({ "model": "gpt-5.4" }),
        );

        assert_eq!(endpoint, "/v1/chat/completions?foo=bar");
        assert_eq!(passthrough_query.as_deref(), Some("foo=bar"));
    }

    #[test]
    fn rewrite_claude_transform_endpoint_strips_beta_for_responses() {
        let (endpoint, passthrough_query) = rewrite_claude_transform_endpoint(
            "/claude/v1/messages?beta=true&x-id=1",
            "openai_responses",
            false,
            &json!({ "model": "gpt-5.4" }),
        );

        assert_eq!(endpoint, "/v1/responses?x-id=1");
        assert_eq!(passthrough_query.as_deref(), Some("x-id=1"));
    }

    #[test]
    fn rewrite_codex_responses_endpoint_to_chat_preserves_query() {
        let (endpoint, passthrough_query) =
            rewrite_codex_responses_endpoint_to_chat("/v1/responses?foo=bar");

        assert_eq!(endpoint, "/chat/completions?foo=bar");
        assert_eq!(passthrough_query.as_deref(), Some("foo=bar"));
    }

    #[test]
    fn prepend_claude_code_system_prompt_from_string() {
        let mut body = json!({ "system": "You are a Codex agent." });
        prepend_claude_code_system_prompt(&mut body);
        let system = body["system"].as_array().unwrap();
        assert_eq!(system[0]["text"], CLAUDE_CODE_SYSTEM_IDENTITY);
        assert_eq!(system[1]["text"], "You are a Codex agent.");
    }

    #[test]
    fn prepend_claude_code_system_prompt_when_absent() {
        let mut body = json!({});
        prepend_claude_code_system_prompt(&mut body);
        let system = body["system"].as_array().unwrap();
        assert_eq!(system.len(), 1);
        assert_eq!(system[0]["text"], CLAUDE_CODE_SYSTEM_IDENTITY);
    }

    #[test]
    fn prepend_claude_code_system_prompt_is_idempotent() {
        let mut body = json!({ "system": "orig" });
        prepend_claude_code_system_prompt(&mut body);
        prepend_claude_code_system_prompt(&mut body);
        let system = body["system"].as_array().unwrap();
        assert_eq!(system.len(), 2);
        assert_eq!(system[0]["text"], CLAUDE_CODE_SYSTEM_IDENTITY);
        assert_eq!(system[1]["text"], "orig");
    }

    #[test]
    fn rewrite_codex_responses_endpoint_to_anthropic_preserves_query() {
        let (endpoint, passthrough_query) =
            rewrite_codex_responses_endpoint_to_anthropic("/responses?x=1");
        assert_eq!(endpoint, "/v1/messages?x=1");
        assert_eq!(passthrough_query.as_deref(), Some("x=1"));

        let (endpoint, _) = rewrite_codex_responses_endpoint_to_anthropic("/v1/responses");
        assert_eq!(endpoint, "/v1/messages");
    }

    #[test]
    fn codex_anthropic_full_endpoint_guard_avoids_double_messages() {
        // On the Codex→Anthropic path a base URL already ending in `/v1/messages` (switch
        // off) must be treated as a full endpoint by the real `base_url_is_full_endpoint`.

        // Without the guard, build_url would concatenate the pasted endpoint with the
        // rewritten `/v1/messages` target, producing a broken double suffix.
        use super::super::providers::ProviderAdapter;
        let doubled = super::super::providers::CodexAdapter::new()
            .build_url("https://host.example/v1/messages", "/v1/messages");
        assert_eq!(doubled, "https://host.example/v1/messages/v1/messages");

        // With the guard, the pasted URL is used verbatim (plus preserved query). Includes
        // query/fragment/whitespace suffixes, which must not hide the endpoint (fix: a base
        // like `.../v1/messages?beta=true` previously evaded the suffix check).
        for base in [
            "https://host.example/v1/messages",
            "https://host.example/v1/messages/",
            "https://host.example/api/v1/messages", // prefixed gateway
            "https://host.example/v1/messages?beta=true",
            "https://host.example/v1/messages/?beta=true",
            "https://host.example/v1/messages#frag",
            "  https://host.example/v1/messages  ",
        ] {
            assert!(
                base_url_is_full_endpoint(base, "/v1/messages"),
                "expected full-endpoint match: {base:?}"
            );
        }
        assert_eq!(
            append_query_to_full_url("https://host.example/v1/messages", Some("x=1")),
            "https://host.example/v1/messages?x=1"
        );
        // A base URL that already carries its own query is preserved verbatim (no double
        // `/v1/messages`, query kept).
        assert_eq!(
            append_query_to_full_url("https://host.example/v1/messages?beta=true", None),
            "https://host.example/v1/messages?beta=true"
        );

        // A non-endpoint base (origin/prefix) must NOT match, so build_url still appends.
        assert!(!base_url_is_full_endpoint(
            "https://host.example",
            "/v1/messages"
        ));
        assert!(!base_url_is_full_endpoint(
            "https://host.example/v1",
            "/v1/messages"
        ));
        // The shared helper also backs the Chat path's `/chat/completions` guard.
        assert!(base_url_is_full_endpoint(
            "https://host.example/v1/chat/completions?api-version=2024",
            "/chat/completions"
        ));
    }

    #[test]
    fn hop_by_hop_request_headers_are_not_forwarded() {
        let mut headers = http::HeaderMap::new();
        headers.append(
            http::header::CONNECTION,
            http::HeaderValue::from_static("close, x-hop-custom, Authorization"),
        );
        headers.insert("keep-alive", http::HeaderValue::from_static("timeout=5"));
        headers.insert("expect", http::HeaderValue::from_static("100-continue"));
        headers.insert("te", http::HeaderValue::from_static("trailers"));
        headers.insert("upgrade", http::HeaderValue::from_static("h2c"));
        headers.insert("x-hop-custom", http::HeaderValue::from_static("1"));
        headers.insert("authorization", http::HeaderValue::from_static("Bearer x"));
        headers.insert(
            "anthropic-version",
            http::HeaderValue::from_static("2023-06-01"),
        );

        let listed = connection_listed_request_headers(&headers);
        assert_eq!(
            listed,
            vec!["close".to_string(), "x-hop-custom".to_string()]
        );

        let forwarded: Vec<&str> = headers
            .keys()
            .map(|k| k.as_str())
            .filter(|k| !is_hop_by_hop_request_header(k, &listed))
            .collect();
        assert_eq!(forwarded, vec!["authorization", "anthropic-version"]);
    }

    #[test]
    fn codex_client_fingerprint_headers_are_dropped_for_anthropic_upstreams() {
        // Codex/OpenAI fingerprints a native Claude Code client never sends → must drop.
        for header in [
            "originator",
            "session_id",
            "session-id",
            "thread-id",
            "conversation_id",
            "chatgpt-account-id",
            "x-openai-subagent",
            "x-client-request-id",
            "x-codex-window-id",
            "openai-beta",
            "openai-organization",
            "openai-project",
            "x-stainless-lang",
            "x-stainless-runtime",
            "x-codex-turn-id",
        ] {
            assert!(
                is_codex_client_fingerprint_header(header),
                "expected {header} to be dropped while impersonating Claude Code"
            );
        }

        // Headers a real Claude Code client sends (or that the forwarder rebuilds) must
        // NOT be caught by the denylist.
        for header in [
            "anthropic-version",
            "anthropic-beta",
            "user-agent",
            "accept",
            "content-type",
            "x-app",
        ] {
            assert!(
                !is_codex_client_fingerprint_header(header),
                "{header} must be preserved while impersonating Claude Code"
            );
        }
    }

    #[test]
    fn codex_anthropic_2xx_error_envelope_is_detected_for_failover() {
        let body = br#"{"type":"error","error":{"type":"overloaded_error","message":"busy"}}"#;
        assert_eq!(
            codex_anthropic_error_envelope_message(body).as_deref(),
            Some("overloaded_error: busy")
        );
        assert!(
            codex_anthropic_error_envelope_message(br#"{"type":"message","content":[]}"#).is_none()
        );
    }

    #[test]
    fn responses_2xx_failure_is_detected_for_failover() {
        assert_eq!(
            responses_error_envelope_message(
                br#"{"status":"failed","error":{"type":"server_error","message":"busy"},"output":[]}"#
            )
            .as_deref(),
            Some("server_error: busy")
        );
        assert_eq!(
            responses_error_envelope_message(br#"{"status":"cancelled","output":[]}"#).as_deref(),
            Some("cancelled: response generation was cancelled")
        );
        assert!(responses_error_envelope_message(
            br#"{"status":"incomplete","incomplete_details":{"reason":"max_output_tokens"},"output":[]}"#
        )
        .is_none());
        assert!(responses_error_envelope_message(
            br#"{"status":"completed","error":null,"output":[]}"#
        )
        .is_none());
    }

    #[test]
    fn responses_stream_start_semantic_failure_is_retryable() {
        let created = concat!(
            "event: response.created\n",
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_1\"}}"
        );
        assert!(inspect_responses_start_event(created).is_none());

        let failed = concat!(
            "event: response.failed\n",
            "data: {\"type\":\"response.failed\",\"response\":{\"error\":{\"type\":\"server_error\",\"message\":\"boom\"}}}"
        );
        assert!(matches!(
            inspect_responses_start_event(failed),
            Some(Err(ProxyError::TransformError(message))) if message.contains("boom")
        ));

        let delta = concat!(
            "event: response.output_text.delta\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"hi\"}"
        );
        assert!(matches!(inspect_responses_start_event(delta), Some(Ok(()))));
    }

    #[test]
    fn responses_stream_start_accepts_unlabelled_whole_json() {
        assert!(matches!(
            inspect_responses_json_document(
                r#"{
                    "status": "completed",

                    "output": []
                }"#
            ),
            Some(Ok(()))
        ));
        assert!(inspect_responses_json_document(r#"{"status":"completed""#).is_none());

        let failed = inspect_responses_json_document(
            r#"{"status":"failed","error":{"message":"backend unavailable"}}"#,
        );
        assert!(
            matches!(failed, Some(Err(ProxyError::TransformError(message))) if message.contains("backend unavailable"))
        );
    }

    #[test]
    fn codex_anthropic_cache_is_default_on_but_honors_sub_switch() {
        let default = codex_anthropic_cache_config(&OptimizerConfig::default());
        assert!(default.enabled);
        assert!(default.cache_injection);

        let disabled = codex_anthropic_cache_config(&OptimizerConfig {
            cache_injection: false,
            ..OptimizerConfig::default()
        });
        assert!(disabled.enabled);
        assert!(!disabled.cache_injection);
    }

    #[test]
    fn invalid_client_history_is_not_retryable() {
        let forwarder = test_forwarder(Duration::ZERO, Duration::ZERO);
        let provider = test_provider_with_type(None);
        assert_eq!(
            forwarder.categorize_proxy_error(
                &ProxyError::InvalidRequest("invalid historical tool arguments".to_string()),
                &provider,
            ),
            ErrorCategory::NonRetryable
        );
    }

    #[test]
    fn official_codex_auth_failures_are_not_retryable() {
        let forwarder = test_forwarder(Duration::ZERO, Duration::ZERO);
        let mut provider = test_provider_with_type(None);
        provider.id = "codex-official".to_string();
        provider.category = Some("official".to_string());

        for error in [
            ProxyError::AuthError("restart Codex".to_string()),
            ProxyError::UpstreamError {
                status: 401,
                body: None,
            },
            ProxyError::UpstreamError {
                status: 403,
                body: None,
            },
        ] {
            assert_eq!(
                forwarder.categorize_proxy_error(&error, &provider),
                ErrorCategory::NonRetryable
            );
        }
    }

    #[test]
    fn official_codex_rejects_stale_proxy_placeholder_with_restart_hint() {
        let mut headers = HeaderMap::new();
        headers.insert(
            http::header::AUTHORIZATION,
            HeaderValue::from_static("Bearer PROXY_MANAGED"),
        );
        let error = validate_codex_official_authorization(&headers)
            .expect_err("stale placeholder must be rejected");
        assert!(
            matches!(error, ProxyError::AuthError(message) if message.contains("restart Codex"))
        );
    }

    #[test]
    fn rewrite_codex_responses_compact_endpoint_to_chat_preserves_query() {
        let (endpoint, passthrough_query) =
            rewrite_codex_responses_endpoint_to_chat("/v1/responses/compact?foo=bar");

        assert_eq!(endpoint, "/chat/completions?foo=bar");
        assert_eq!(passthrough_query.as_deref(), Some("foo=bar"));
    }

    #[test]
    fn rewrite_claude_transform_endpoint_uses_copilot_path() {
        let (endpoint, passthrough_query) = rewrite_claude_transform_endpoint(
            "/v1/messages?beta=true&x-id=1",
            "anthropic",
            true,
            &json!({ "model": "claude-sonnet-4-6" }),
        );

        assert_eq!(endpoint, "/chat/completions?x-id=1");
        assert_eq!(passthrough_query.as_deref(), Some("x-id=1"));
    }

    #[test]
    fn rewrite_claude_transform_endpoint_uses_copilot_responses_path() {
        let (endpoint, passthrough_query) = rewrite_claude_transform_endpoint(
            "/v1/messages?beta=true&x-id=1",
            "openai_responses",
            true,
            &json!({ "model": "gpt-5.4" }),
        );

        assert_eq!(endpoint, "/v1/responses?x-id=1");
        assert_eq!(passthrough_query.as_deref(), Some("x-id=1"));
    }

    #[test]
    fn rewrite_claude_transform_endpoint_maps_gemini_generate_content() {
        let (endpoint, passthrough_query) = rewrite_claude_transform_endpoint(
            "/v1/messages?beta=true&x-id=1",
            "gemini_native",
            false,
            &json!({ "model": "gemini-2.5-pro" }),
        );

        assert_eq!(
            endpoint,
            "/v1beta/models/gemini-2.5-pro:generateContent?x-id=1"
        );
        assert_eq!(passthrough_query.as_deref(), Some("x-id=1"));
    }

    /// Regression: body.model arriving as the resource-name form
    /// `models/gemini-2.5-pro` must not produce a doubled
    /// `/v1beta/models/models/...` path.
    #[test]
    fn rewrite_claude_transform_endpoint_strips_gemini_model_resource_prefix() {
        let (endpoint, _) = rewrite_claude_transform_endpoint(
            "/v1/messages",
            "gemini_native",
            false,
            &json!({ "model": "models/gemini-2.5-pro" }),
        );

        assert_eq!(endpoint, "/v1beta/models/gemini-2.5-pro:generateContent");
    }

    #[test]
    fn rewrite_claude_transform_endpoint_maps_gemini_streaming() {
        let (endpoint, passthrough_query) = rewrite_claude_transform_endpoint(
            "/v1/messages?beta=true",
            "gemini_native",
            false,
            &json!({ "model": "gemini-2.5-flash", "stream": true }),
        );

        assert_eq!(
            endpoint,
            "/v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse"
        );
        assert_eq!(passthrough_query.as_deref(), Some("alt=sse"));
    }

    #[test]
    fn append_query_to_full_url_preserves_existing_query_string() {
        let url = append_query_to_full_url("https://relay.example/api?foo=bar", Some("x-id=1"));

        assert_eq!(url, "https://relay.example/api?foo=bar&x-id=1");
    }

    #[test]
    fn build_gemini_native_url_uses_origin_when_base_ends_with_v1beta() {
        let url = crate::proxy::gemini_url::build_gemini_native_url(
            "https://generativelanguage.googleapis.com/v1beta",
            "/v1beta/models/gemini-2.5-pro:generateContent",
        );

        assert_eq!(
            url,
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-pro:generateContent"
        );
    }

    #[test]
    fn build_gemini_native_url_uses_origin_when_base_already_contains_models_prefix() {
        let url = crate::proxy::gemini_url::build_gemini_native_url(
            "https://generativelanguage.googleapis.com/v1beta/models",
            "/v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse",
        );

        assert_eq!(
            url,
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse"
        );
    }

    #[test]
    fn resolve_gemini_native_url_keeps_opaque_full_url_as_is() {
        let url = crate::proxy::gemini_url::resolve_gemini_native_url(
            "https://relay.example/custom/generate-content",
            "/v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse",
            true,
        );

        assert_eq!(url, "https://relay.example/custom/generate-content?alt=sse");
    }

    #[test]
    fn force_identity_for_stream_flag_requests() {
        let headers = HeaderMap::new();

        assert!(should_force_identity_encoding(
            "/v1/responses",
            &json!({ "stream": true }),
            &headers
        ));
    }

    #[test]
    fn force_identity_for_gemini_stream_endpoints() {
        let headers = HeaderMap::new();

        assert!(should_force_identity_encoding(
            "/v1beta/models/gemini-2.5-pro:streamGenerateContent?alt=sse",
            &json!({ "model": "gemini-2.5-pro" }),
            &headers
        ));
    }

    #[test]
    fn streaming_request_detects_gemini_sse_without_body_stream_flag() {
        let headers = HeaderMap::new();

        assert!(is_streaming_request(
            "/v1beta/models/gemini-2.5-pro:streamGenerateContent?alt=sse",
            &json!({ "model": "gemini-2.5-pro" }),
            &headers
        ));
    }

    #[test]
    fn force_identity_for_sse_accept_header() {
        let mut headers = HeaderMap::new();
        headers.insert(ACCEPT, HeaderValue::from_static("text/event-stream"));

        assert!(should_force_identity_encoding(
            "/v1/responses",
            &json!({ "model": "gpt-5" }),
            &headers
        ));
    }

    #[test]
    fn non_streaming_requests_allow_automatic_compression() {
        let headers = HeaderMap::new();

        assert!(!should_force_identity_encoding(
            "/v1/responses",
            &json!({ "model": "gpt-5" }),
            &headers
        ));
    }

    // ==================== Copilot 动态 endpoint 路由相关测试 ====================

    /// 验证 is_copilot 检测逻辑：通过 provider_type 判断
    #[test]
    fn copilot_detection_via_provider_type() {
        use crate::provider::{Provider, ProviderMeta};

        let provider = Provider {
            id: "test".to_string(),
            name: "Test Copilot".to_string(),
            settings_config: serde_json::json!({}),
            website_url: None,
            category: None,
            created_at: None,
            sort_index: None,
            notes: None,
            meta: Some(ProviderMeta {
                provider_type: Some("github_copilot".to_string()),
                ..Default::default()
            }),
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        };

        let is_copilot = provider
            .meta
            .as_ref()
            .and_then(|m| m.provider_type.as_deref())
            == Some("github_copilot");

        assert!(is_copilot, "应该通过 provider_type 检测为 Copilot");
    }

    /// 验证 is_copilot 检测逻辑：通过 base_url 判断
    #[test]
    fn copilot_detection_via_base_url() {
        let base_url = "https://api.githubcopilot.com";
        let is_copilot = base_url.contains("githubcopilot.com");
        assert!(is_copilot, "应该通过 base_url 检测为 Copilot");

        let non_copilot_url = "https://api.anthropic.com";
        let is_not_copilot = non_copilot_url.contains("githubcopilot.com");
        assert!(!is_not_copilot, "非 Copilot URL 不应被检测为 Copilot");
    }

    /// 验证企业版 endpoint（不包含 githubcopilot.com）场景下 is_copilot 仍然正确
    #[test]
    fn copilot_detection_for_enterprise_endpoint() {
        use crate::provider::{Provider, ProviderMeta};

        // 企业版场景：provider_type 是 github_copilot，但 base_url 可能是企业内部域名
        let provider = Provider {
            id: "enterprise".to_string(),
            name: "Enterprise Copilot".to_string(),
            settings_config: serde_json::json!({}),
            website_url: None,
            category: None,
            created_at: None,
            sort_index: None,
            notes: None,
            meta: Some(ProviderMeta {
                provider_type: Some("github_copilot".to_string()),
                ..Default::default()
            }),
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        };

        let enterprise_base_url = "https://copilot-api.corp.example.com";

        // is_copilot 应该通过 provider_type 检测成功，即使 base_url 不包含 githubcopilot.com
        let is_copilot = provider
            .meta
            .as_ref()
            .and_then(|m| m.provider_type.as_deref())
            == Some("github_copilot")
            || enterprise_base_url.contains("githubcopilot.com");

        assert!(
            is_copilot,
            "企业版 Copilot 应该通过 provider_type 被正确检测"
        );
    }

    /// 验证动态 endpoint 替换条件
    #[test]
    fn dynamic_endpoint_replacement_conditions() {
        // 条件：is_copilot && !is_full_url
        let test_cases = [
            (true, false, true, "Copilot + 非 full_url 应该替换"),
            (true, true, false, "Copilot + full_url 不应替换"),
            (false, false, false, "非 Copilot 不应替换"),
            (false, true, false, "非 Copilot + full_url 不应替换"),
        ];

        for (is_copilot, is_full_url, should_replace, desc) in test_cases {
            let will_replace = is_copilot && !is_full_url;
            assert_eq!(will_replace, should_replace, "{desc}");
        }
    }

    // ===== P3: forwarder 层 media 开关回归测试 =====
    // 验证 gate 在 forwarder 这一层的"接线"，而非 media_sanitizer 纯函数本身。

    fn forwarder_with_rectifier(config: RectifierConfig) -> RequestForwarder {
        let mut fwd = test_forwarder(Duration::from_secs(1), Duration::from_secs(1));
        fwd.rectifier_config = config;
        fwd
    }

    fn provider_with_settings(settings_config: Value) -> Provider {
        let mut p = test_provider_with_type(Some("anthropic"));
        p.settings_config = settings_config;
        p
    }

    fn body_with_image(model: &str) -> Value {
        json!({
            "model": model,
            "messages": [{
                "role": "user",
                "content": [
                    { "type": "image", "source": { "type": "base64", "media_type": "image/png", "data": "abc" } }
                ]
            }]
        })
    }

    fn body_with_codex_input_image(model: &str) -> Value {
        json!({
            "model": model,
            "input": [{
                "role": "user",
                "content": [
                    { "type": "input_image", "image_url": "data:image/png;base64,abc" }
                ]
            }]
        })
    }

    fn image_unsupported_error() -> ProxyError {
        ProxyError::UpstreamError {
            status: 400,
            body: Some(
                r#"{"error":{"message":"This model does not support image input"}}"#.to_string(),
            ),
        }
    }
    #[test]
    fn prevention_replaces_when_all_switches_on_and_model_in_heuristic_list() {
        let fwd = forwarder_with_rectifier(RectifierConfig::default());
        let provider = provider_with_settings(json!({}));
        let mut body = body_with_image("deepseek-v4-pro");

        let replaced = fwd.apply_media_prevention(&mut body, &provider);

        assert_eq!(replaced, 1, "默认全开 + 名单内模型应预替换");
        assert_eq!(body["messages"][0]["content"][0]["type"], "text");
    }

    #[test]
    fn prevention_skipped_when_media_fallback_off() {
        // 关闭 request_media_fallback：即使名单命中也不预替换。
        let fwd = forwarder_with_rectifier(RectifierConfig {
            request_media_fallback: false,
            ..RectifierConfig::default()
        });
        let provider = provider_with_settings(json!({}));
        let mut body = body_with_image("deepseek-v4-pro");

        let replaced = fwd.apply_media_prevention(&mut body, &provider);

        assert_eq!(replaced, 0);
        assert_eq!(body["messages"][0]["content"][0]["type"], "image");
    }

    #[test]
    fn prevention_skipped_when_master_switch_off() {
        let fwd = forwarder_with_rectifier(RectifierConfig {
            enabled: false,
            ..RectifierConfig::default()
        });
        let provider = provider_with_settings(json!({}));
        let mut body = body_with_image("deepseek-v4-pro");

        assert_eq!(fwd.apply_media_prevention(&mut body, &provider), 0);
        assert_eq!(body["messages"][0]["content"][0]["type"], "image");
    }

    #[test]
    fn prevention_heuristic_off_skips_list_but_keeps_explicit_text_only() {
        // 关闭 request_media_heuristic：名单预测失效，但显式声明 text-only 仍预替换。
        let fwd = forwarder_with_rectifier(RectifierConfig {
            request_media_heuristic: false,
            ..RectifierConfig::default()
        });

        // (a) 名单内模型、无显式声明 → 不再预替换
        let bare_provider = provider_with_settings(json!({}));
        let mut list_body = body_with_image("deepseek-v4-pro");
        assert_eq!(
            fwd.apply_media_prevention(&mut list_body, &bare_provider),
            0,
            "heuristic 关闭后名单模型不应被预替换"
        );
        assert_eq!(list_body["messages"][0]["content"][0]["type"], "image");

        // (b) 显式声明 text-only → 仍预替换（声明驱动，不受 heuristic 开关影响）
        let declared_provider = provider_with_settings(json!({
            "models": [ { "id": "some-text-model", "input": ["text"] } ]
        }));
        let mut declared_body = body_with_image("some-text-model");
        assert_eq!(
            fwd.apply_media_prevention(&mut declared_body, &declared_provider),
            1,
            "显式 text-only 即使关闭 heuristic 也应预替换"
        );
        assert_eq!(declared_body["messages"][0]["content"][0]["type"], "text");
    }

    #[test]
    fn reactive_triggers_when_all_switches_on() {
        let fwd = forwarder_with_rectifier(RectifierConfig::default());
        let body = body_with_image("any-model");
        assert!(fwd.media_retry_should_trigger("Claude", false, &body, &image_unsupported_error()));
    }

    #[test]
    fn reactive_triggers_for_codex_image_url_deserialize_errors() {
        let fwd = forwarder_with_rectifier(RectifierConfig::default());
        let body = body_with_codex_input_image("deepseek-v4-flash");
        let error = ProxyError::UpstreamError {
            status: 400,
            body: Some(
                r#"{"error":{"message":"Failed to deserialize the JSON body into the target type: messages[11]: unknown variant image_url, expected text"}}"#
                    .to_string(),
            ),
        };

        assert!(fwd.media_retry_should_trigger("Codex", false, &body, &error));
    }

    #[test]
    fn reactive_skipped_when_media_fallback_off() {
        // 关闭 request_media_fallback：上游报图片错误也不触发兜底重试。
        let fwd = forwarder_with_rectifier(RectifierConfig {
            request_media_fallback: false,
            ..RectifierConfig::default()
        });
        let body = body_with_image("any-model");
        assert!(!fwd.media_retry_should_trigger(
            "Claude",
            false,
            &body,
            &image_unsupported_error()
        ));
    }

    #[test]
    fn reactive_skipped_when_master_switch_off() {
        let fwd = forwarder_with_rectifier(RectifierConfig {
            enabled: false,
            ..RectifierConfig::default()
        });
        let body = body_with_image("any-model");
        assert!(!fwd.media_retry_should_trigger(
            "Claude",
            false,
            &body,
            &image_unsupported_error()
        ));
    }

    #[test]
    fn reactive_unaffected_by_heuristic_switch() {
        // 关闭 request_media_heuristic 不影响反应式兜底——它是上游实测错误后的恢复，不是预测。
        let fwd = forwarder_with_rectifier(RectifierConfig {
            request_media_heuristic: false,
            ..RectifierConfig::default()
        });
        let body = body_with_image("any-model");
        assert!(fwd.media_retry_should_trigger("Claude", false, &body, &image_unsupported_error()));
    }
}
