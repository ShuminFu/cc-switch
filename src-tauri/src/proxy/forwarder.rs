//! 请求转发器
//!
//! 负责将请求转发到上游Provider，支持故障转移。
//!
//! 本文件只是门面：故障转移循环与单次尝试的编排。每次尝试依次经过
//! [`pipeline`](super::pipeline) 中的各阶段——请求体构建（`request_builder`）、
//! 端点解析（`endpoint`）、认证（`auth`）、出站头策略（`headers`）、发送
//! （`transport`）与响应校验（`validate`）。

use super::hyper_client::ProxyResponse;
use super::{
    capture::CaptureHandle,
    error_mapper::get_error_message,
    failover_switch::FailoverSwitchManager,
    pipeline::{
        auth,
        classify::build_terminal_failure_log,
        copilot,
        endpoint::UpstreamTarget,
        failover::{settle_failure, AttemptFailure, AttemptFailureInput, AttemptRecorder},
        headers::{self as header_policy, HeaderPolicy, HeaderPolicyInput},
        recovery::{RecoveryDecision, RecoveryInput, RecoveryState},
        request_builder::{self, BodyStages, RouteFlags},
        transport::{self, TransportTimeouts, UpstreamRequest},
        validate::{ResponseValidator, SuccessCheck},
    },
    provider_router::ProviderRouter,
    providers::{
        codex_chat_history::CodexChatHistoryStore, gemini_shadow::GeminiShadowStore, get_adapter,
        ProviderAdapter, ProviderType,
    },
    trace::TraceHandle,
    types::{CopilotOptimizerConfig, OptimizerConfig, ProxyStatus, RectifierConfig},
    ProxyError,
};
use crate::proxy::protocol::{ClaudeApiFormat, WireFormat};
use crate::proxy::routing::RoutePlan;
use crate::{app_config::AppType, provider::Provider};
use http::Extensions;
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct ForwardResult {
    pub response: ProxyResponse,
    pub provider: Provider,
    /// 成功那次尝试的路由计划；handler 只按它解码响应，不再按 provider 重新推导
    pub plan: RoutePlan,
    /// 实际发往上游的模型名（路由接管/模型映射后的真值）。
    ///
    /// usage 归因不能依赖 ctx.request_model（映射前的客户端别名）：上游响应
    /// 缺失 model 或回显别名时，接管流量会被记成 claude-* 并按其定价计费。
    pub outbound_model: Option<String>,
    /// 活跃连接 RAII guard：随响应一起流转到 response_processor / handle_claude_transform，
    /// 最终被 move 进流式 body future（或非流式响应作用域），覆盖整个响应生命周期。
    pub(crate) connection_guard: Option<ActiveConnectionGuard>,
}

pub struct ForwardError {
    pub error: ProxyError,
    pub provider: Option<Provider>,
}

/// 活跃连接 RAII guard
///
/// 构造时把 `ProxyStatus.active_connections` +1；Drop 时在 tokio runtime 上调度
/// 一个异步任务执行 -1，从而支持把 guard move 进流式 body future（stream 自然结束
/// 时 guard 与 future 一起 drop）。
///
/// 设计动机：之前在 `forward_with_retry` 出口处同步 -1，但流式响应的 body 实际
/// 在 `create_logged_passthrough_stream` 内还会继续 yield 字节流，导致 UI 的
/// `active_connections` 计数过早归零。RAII guard 让"减量"由 Rust 类型系统驱动，
/// 不需要每条出口路径都手动调用。
pub(crate) struct ActiveConnectionGuard {
    status: Arc<RwLock<ProxyStatus>>,
}

impl ActiveConnectionGuard {
    pub(crate) async fn acquire(status: Arc<RwLock<ProxyStatus>>) -> Self {
        {
            let mut s = status.write().await;
            s.active_connections = s.active_connections.saturating_add(1);
        }
        Self { status }
    }
}

impl Drop for ActiveConnectionGuard {
    fn drop(&mut self) {
        // Drop 不能 await：把减量操作调度到 tokio runtime
        let status = self.status.clone();
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                let mut s = status.write().await;
                s.active_connections = s.active_connections.saturating_sub(1);
            });
        }
        // 没有 runtime 时静默丢失计数（仅 UI 展示用，可接受最终一致性）
    }
}

/// 跨请求共享的 forwarder 依赖，随 `ProxyState` 构建一次、每个请求只克隆 `Arc`
pub struct ForwarderDeps {
    /// 共享的 ProviderRouter（持有熔断器状态）
    pub router: Arc<ProviderRouter>,
    pub status: Arc<RwLock<ProxyStatus>>,
    pub current_providers: Arc<RwLock<std::collections::HashMap<String, (String, String)>>>,
    pub gemini_shadow: Arc<GeminiShadowStore>,
    pub codex_chat_history: Arc<CodexChatHistoryStore>,
    /// 故障转移切换管理器
    pub failover_manager: Arc<FailoverSwitchManager>,
    /// AppHandle，用于发射事件和更新托盘
    pub app_handle: Option<tauri::AppHandle>,
}

/// 单个请求的 forwarder 参数（来自请求上下文与运行时配置快照）
pub struct RequestScope {
    /// 客户端说的线协议（来自端点表）
    pub client_format: WireFormat,
    /// 请求开始时的"当前供应商 ID"（用于判断是否需要同步 UI/托盘）
    pub current_provider_id_at_start: String,
    /// 代理会话 ID（用于 Gemini Native shadow replay）
    pub session_id: String,
    /// Session ID 是否由客户端提供；生成值不能作为上游缓存身份。
    pub session_client_provided: bool,
    /// 整流器配置
    pub rectifier_config: RectifierConfig,
    /// 优化器配置
    pub optimizer_config: OptimizerConfig,
    /// Copilot 优化器配置
    pub copilot_optimizer_config: CopilotOptimizerConfig,
    /// 非流式请求超时（0 表示不限）
    pub non_streaming_timeout: std::time::Duration,
    /// 流式请求响应头等待超时（0 表示不限）
    pub streaming_first_byte_timeout: std::time::Duration,
    /// 单个客户端请求最多尝试的 provider 数。
    ///
    /// 由 `AppProxyConfig.max_retries` (UI: "请求失败时的重试次数, 0-10") 派生：
    /// `max_attempts = max_retries + 1`，所以 max_retries=0 表示仅尝试一家、
    /// max_retries=3（默认）表示最多 4 家。loop 同时受 providers.len() 自然限制。
    /// 使用 [`RequestScope::max_attempts_for`] 计算。
    pub max_attempts: usize,
    /// 缺陷报告抓包句柄：每次尝试记录实际发往上游的请求
    pub capture: Option<CaptureHandle>,
    /// 请求追踪句柄：记录每次尝试的路由计划、结果与原因
    pub trace: Option<TraceHandle>,
}

impl RequestScope {
    /// max_retries 是「失败后重试次数」语义，attempt 上限 = retries + 1。
    /// saturating_add 防止 u32::MAX + 1 溢出。
    pub fn max_attempts_for(max_retries: u32) -> usize {
        (max_retries as usize).saturating_add(1)
    }
}

pub struct RequestForwarder {
    shared: Arc<ForwarderDeps>,
    req: RequestScope,
}

impl RequestForwarder {
    pub fn new(shared: Arc<ForwarderDeps>, req: RequestScope) -> Self {
        Self { shared, req }
    }

    /// 转发请求（带故障转移）
    ///
    /// 这是 thin wrapper：在客户端请求维度记一次 `total_requests` / 调整
    /// `active_connections` / 刷新 `last_request_at`，无论 inner 走哪条出口路径，
    /// 出口处都会把 `active_connections` 回收。Per-attempt 维度（成功/失败/熔断
    /// 等）仍由 inner 内自行更新 `success_requests` / `failed_requests`。
    #[allow(clippy::too_many_arguments)]
    pub async fn forward_with_retry(
        &self,
        app_type: &AppType,
        method: http::Method,
        endpoint: &str,
        body: Value,
        headers: axum::http::HeaderMap,
        extensions: Extensions,
        providers: Vec<Provider>,
    ) -> Result<ForwardResult, ForwardError> {
        let guard = ActiveConnectionGuard::acquire(self.shared.status.clone()).await;
        {
            let mut s = self.shared.status.write().await;
            s.total_requests = s.total_requests.saturating_add(1);
            s.last_request_at = Some(chrono::Utc::now().to_rfc3339());
        }
        let request = ClientRequest {
            app_type,
            method: &method,
            endpoint,
            body: &body,
            headers: &headers,
            extensions: &extensions,
        };
        let result = self.forward_with_retry_inner(&request, providers).await;
        // 把 guard 注入到 Ok 结果，让它随响应一起流转到 response_processor，
        // 在流式 body 的 future 内才真正 drop。
        // Err 路径：guard 在函数 scope 内随返回值落地时自动 drop。
        result.map(|mut fr| {
            fr.connection_guard = Some(guard);
            fr
        })
    }

    /// 实际转发逻辑（不包含客户端维度的入口/出口计数）
    ///
    /// `providers` 是已选择的 Provider 列表（由 RequestContext 提供，避免重复调用 select_providers）
    async fn forward_with_retry_inner(
        &self,
        request: &ClientRequest<'_>,
        providers: Vec<Provider>,
    ) -> Result<ForwardResult, ForwardError> {
        let adapter = get_adapter(request.app_type);
        let app_type_str = request.app_type.as_str();

        if providers.is_empty() {
            return Err(ForwardError {
                error: ProxyError::NoAvailableProvider,
                provider: None,
            });
        }

        let mut last_error = None;
        let mut last_provider = None;
        let mut attempted_providers = 0usize;

        // 单 Provider 场景下跳过熔断器检查（故障转移关闭时）
        let bypass_circuit_breaker = providers.len() == 1;
        let recorder = AttemptRecorder::new(
            &self.shared,
            app_type_str,
            &self.req.current_provider_id_at_start,
        );

        for provider in providers.iter() {
            // 上限检查：尊重用户在 AppProxyConfig.max_retries 上配置的「重试次数」。
            // 放在熔断器 allow 检查之前，避免在已经超限时还占用 HalfOpen 探测名额。
            if attempted_providers >= self.req.max_attempts {
                log::warn!(
                    "[{app_type_str}] 已达最大尝试次数上限 ({}/{}), 停止故障转移",
                    attempted_providers,
                    self.req.max_attempts
                );
                break;
            }

            // 发起请求前先获取熔断器放行许可（HalfOpen 会占用探测名额）；单 Provider 场景下跳过。
            // `attempt` 在本轮迭代内持有 HalfOpen 探测许可：若请求在记录结果前被取消
            // （客户端断开等导致 future 被 drop），守卫 Drop 时会释放探测名额
            let Some(attempt) = recorder.begin(provider, bypass_circuit_breaker).await else {
                if let Some(trace) = &self.req.trace {
                    trace.skip_attempt(&provider.id, &provider.name, "熔断器未放行，跳过该供应商");
                }
                continue;
            };
            attempted_providers += 1;
            if let Some(trace) = &self.req.trace {
                trace.begin_attempt(&provider.id, &provider.name);
            }

            let (outcome, decision, recovery) = self
                .send_with_recovery(request, provider, adapter.as_ref())
                .await;
            if let Some(trace) = &self.req.trace {
                let applied: Vec<&str> = recovery
                    .applied()
                    .iter()
                    .map(|strategy| strategy.as_str())
                    .collect();
                match &outcome {
                    Ok((response, _, _)) => {
                        trace.succeed_attempt(response.status().as_u16(), &applied)
                    }
                    Err(error) => {
                        let upstream_status = match error {
                            ProxyError::UpstreamError { status, .. } => Some(*status),
                            _ => None,
                        };
                        trace.fail_attempt(&get_error_message(error), upstream_status, &applied);
                    }
                }
            }
            let error = match outcome {
                Ok((response, plan, outbound_model)) => {
                    if let Some(strategy) = recovery.last() {
                        strategy.log_retry_succeeded(app_type_str);
                    }
                    attempt.succeed().await;
                    return Ok(ForwardResult {
                        response,
                        provider: provider.clone(),
                        plan,
                        outbound_model,
                        connection_guard: None,
                    });
                }
                Err(error) => error,
            };

            let failure = AttemptFailureInput {
                error,
                decision,
                recovery: &recovery,
                provider,
                attempted_providers,
                total_providers: providers.len(),
                app_type: app_type_str,
            };
            match settle_failure(failure, attempt).await {
                AttemptFailure::Abort(error) => return Err(*error),
                AttemptFailure::Continue(error) => {
                    last_error = Some(error);
                    last_provider = Some(provider.clone());
                }
            }
        }

        if attempted_providers == 0 {
            // providers 列表非空，但全部被熔断器拒绝（典型：HalfOpen 探测名额被占用）
            recorder
                .fail_request("所有供应商暂时不可用（熔断器限制）".to_string())
                .await;
            return Err(ForwardError {
                error: ProxyError::NoAvailableProvider,
                provider: None,
            });
        }

        // 所有供应商都失败了
        recorder.fail_request("所有供应商都失败".to_string()).await;

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

    /// 对同一 provider 发送请求：每个 Provider 只发送原始请求一次；上游拒绝请求体时，
    /// 命中的恢复策略改写请求体后对同一 Provider 再发送（每个策略最多一次，不计入熔断器）
    async fn send_with_recovery(
        &self,
        request: &ClientRequest<'_>,
        provider: &Provider,
        adapter: &dyn ProviderAdapter,
    ) -> (
        Result<ForwardSuccess, ProxyError>,
        RecoveryDecision,
        RecoveryState,
    ) {
        // PRE-SEND 优化器：每个 provider 独立决定是否优化
        let mut provider_body =
            request_builder::pre_send_body(request.body, provider, &self.req.optimizer_config);

        // 恢复策略状态：每个 provider 独立持有，避免标记跨 provider 短路故障转移
        // —— 首家 provider 恢复后被 5xx/timeout 击落时，下家仍能独立走恢复流程
        let mut recovery = RecoveryState::default();
        let provider_type = ProviderType::from_app_type_and_config(request.app_type, provider);
        let is_anthropic_provider = matches!(
            provider_type,
            ProviderType::Claude | ProviderType::ClaudeAuth
        );

        loop {
            let attempt_request = ClientRequest {
                body: &provider_body,
                ..*request
            };
            match self.forward(&attempt_request, provider, adapter).await {
                Ok(success) => return (Ok(success), RecoveryDecision::Unhandled, recovery),
                Err(e) => {
                    let decision = recovery.recover(
                        &RecoveryInput {
                            app_type: request.app_type.as_str(),
                            provider_id: &provider.id,
                            adapter_name: adapter.name(),
                            is_anthropic_provider,
                            config: &self.req.rectifier_config,
                            error: &e,
                        },
                        &mut provider_body,
                    );
                    if matches!(decision, RecoveryDecision::Retry(_)) {
                        continue;
                    }
                    return (Err(e), decision, recovery);
                }
            }
        }
    }

    /// 转发单个请求：依次经过请求体构建、端点解析、认证、头策略、发送与响应校验
    ///
    /// 成功时返回 `(response, plan, outbound_model)`，其中 `plan` 是本次尝试的路由计划，
    /// `outbound_model` 是最终发往上游的模型名（所有映射/改写之后）。
    async fn forward(
        &self,
        request: &ClientRequest<'_>,
        provider: &Provider,
        adapter: &dyn ProviderAdapter,
    ) -> Result<ForwardSuccess, ProxyError> {
        let app_handle = self.shared.app_handle.as_ref();
        let mut base_url = adapter.extract_base_url(provider)?;
        let flags = RouteFlags::resolve(request.app_type, provider, request.endpoint, &base_url);
        if flags.codex_official_auth_passthrough {
            auth::validate_codex_official_authorization(request.headers)?;
        }

        let stages = BodyStages {
            app_type: request.app_type,
            provider,
            adapter,
            flags,
            scope: &self.req,
            deps: &self.shared,
        };
        let normalized = stages.normalize(request.body, request.headers).await?;
        if flags.is_copilot && !flags.is_full_url {
            base_url = copilot::dynamic_api_endpoint(app_handle, provider, base_url).await;
        }
        let api_format = normalized.api_format;
        let target = UpstreamTarget::resolve(
            adapter,
            provider,
            &flags,
            &base_url,
            request.endpoint,
            api_format,
            normalized.needs_transform,
            &normalized.body,
        );
        let force_identity_encoding = normalized.needs_transform
            || flags.codex_responses_to_chat
            || flags.codex_responses_to_anthropic;
        let (built, copilot_optimization) = stages.finish(normalized).await?;
        request_builder::log_prompt_cache_trace(
            request.app_type,
            provider,
            &target.endpoint,
            api_format,
            &built.body,
            self.req.session_client_provided,
        );
        let is_streaming =
            request_builder::is_streaming_request(&target.endpoint, &built.body, request.headers);

        let auth_info = adapter.extract_auth(provider);
        let plan = stages.route_plan(
            api_format,
            &target.url,
            auth_info.as_ref().map(|auth| auth.strategy),
        );
        log::info!("[{}] 路由计划: {plan}", adapter.name());

        let mut resolved_auth = auth::resolve(app_handle, adapter, provider, auth_info).await?;
        if let Some(optimization) = &copilot_optimization {
            header_policy::apply_copilot_optimization(
                &mut resolved_auth.headers,
                optimization,
                &self.req.copilot_optimizer_config,
            );
        }
        let send_anthropic_headers =
            adapter.name() == "Claude" && api_format == Some(ClaudeApiFormat::Anthropic);
        let policy = HeaderPolicy::for_route(HeaderPolicyInput {
            upstream_host: header_policy::upstream_authority(&target.url),
            auth_headers: resolved_auth.headers,
            codex_official_auth_passthrough: flags.codex_official_auth_passthrough,
            codex_responses_to_anthropic: flags.codex_responses_to_anthropic,
            codex_impersonate_claude_code: flags.codex_impersonate_claude_code,
            is_copilot: flags.is_copilot,
            force_identity_encoding: force_identity_encoding || is_streaming,
            custom_user_agent: header_policy::custom_user_agent(
                provider,
                flags.is_copilot,
                flags.codex_impersonate_claude_code,
            ),
            anthropic_beta: header_policy::anthropic_beta_value(
                request.headers,
                send_anthropic_headers,
                flags.codex_impersonate_claude_code,
                built.codex_anthropic_one_m,
            ),
            send_anthropic_headers,
        });
        let mut ordered_headers = policy.apply(request.headers);

        let body_bytes = request_builder::serialize_body(request.method, &built.body)?;

        let session_headers = if resolved_auth.codex_oauth && self.req.session_client_provided {
            header_policy::build_codex_oauth_session_headers(&self.req.session_id)
        } else {
            Vec::new()
        };
        header_policy::finalize(
            &mut ordered_headers,
            session_headers,
            provider,
            flags.is_copilot,
            &target.url,
        )?;

        log_outbound_request(adapter.name(), &target.url, &built.body);
        if let Some(capture) = &self.req.capture {
            capture.record_upstream_request(
                request.method,
                &target.url,
                &ordered_headers,
                &built.body,
            );
        }
        if let Some(trace) = &self.req.trace {
            trace.record_send(&plan, &ordered_headers);
        }

        let response = transport::send(
            UpstreamRequest {
                method: request.method,
                url: &target.url,
                headers: ordered_headers,
                extensions: request.extensions,
                body: body_bytes,
                is_streaming,
                preserve_exact_header_case: transport::should_preserve_exact_header_case(
                    adapter.name(),
                    provider,
                    plan.claude_api_format,
                    flags.is_copilot,
                ),
            },
            TransportTimeouts {
                non_streaming: self.req.non_streaming_timeout,
                streaming_first_byte: self.req.streaming_first_byte_timeout,
            },
        )
        .await?;

        let check =
            SuccessCheck::for_route(flags.codex_responses_to_anthropic, plan.claude_api_format);
        let response = self
            .validator()
            .validate(response, is_streaming, check)
            .await?;
        Ok((response, plan, built.outbound_model))
    }

    fn validator(&self) -> ResponseValidator {
        ResponseValidator {
            non_streaming_timeout: self.req.non_streaming_timeout,
            streaming_first_byte_timeout: self.req.streaming_first_byte_timeout,
        }
    }
}

/// 一次尝试成功的结果：`(response, plan, outbound_model)`
type ForwardSuccess = (ProxyResponse, RoutePlan, Option<String>);

/// 客户端请求（每次尝试共用；恢复策略改写后只替换 `body`）
#[derive(Clone, Copy)]
struct ClientRequest<'a> {
    app_type: &'a AppType,
    method: &'a http::Method,
    endpoint: &'a str,
    body: &'a Value,
    headers: &'a axum::http::HeaderMap,
    extensions: &'a Extensions,
}

/// 输出请求信息日志
fn log_outbound_request(tag: &str, url: &str, body: &Value) {
    let request_model = body
        .get("model")
        .and_then(|v| v.as_str())
        .unwrap_or("<none>");
    log::info!("[{tag}] >>> 请求 URL: {url} (model={request_model})");
    if log::log_enabled!(log::Level::Debug) {
        if let Ok(body_str) = serde_json::to_string(body) {
            log::debug!(
                "[{tag}] >>> 请求体内容 ({}字节): {}",
                body_str.len(),
                body_str
            );
        }
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod wire_golden;
