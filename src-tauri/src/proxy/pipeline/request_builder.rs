//! 上游请求体构建
//!
//! [`BodyStages`] 把客户端请求体按固定顺序改写为上游请求体：
//!
//! 1. **ModelMapping** — 模型映射 / Claude Desktop 路由、thinking 归一化、Grok Build
//!    上游模型、Copilot 模型归一化与 live 解析、`[1m]` 后缀剥离；
//! 2. **VendorNormalize** — Copilot 优化器、解析 Claude API 格式、按供应商归一化
//!    Anthropic messages 与预防式 media 降级；
//! 3. **Encode** — 转换为上游线协议（Codex→Chat / Codex→Anthropic / Claude→其他格式）；
//! 4. **Canonicalize** — 过滤 `_` 前缀私有字段并规范化键序；
//! 5. **LocalOverrides** — provider 级请求体覆盖（改动后再规范化一次）。

use super::copilot::{self, CopilotOptimization};
use crate::app_config::AppType;
use crate::provider::{LocalProxyRequestOverrides, Provider};
use crate::proxy::body_filter::filter_private_params_with_whitelist;
use crate::proxy::codec::side_store::{SessionSideStore, SideStoreScope};
use crate::proxy::error::ProxyError;
use crate::proxy::forwarder::{ForwarderDeps, RequestScope};
use crate::proxy::json_canonical::{canonicalize_value, short_value_hash};
use crate::proxy::protocol::{ClaudeApiFormat, WireFormat};
use crate::proxy::providers::{self, AuthStrategy, ProviderAdapter};
use crate::proxy::routing::{RoutePlan, SideStores, VendorQuirks};
use crate::proxy::thinking_rectifier::normalize_thinking_type;
use crate::proxy::types::{OptimizerConfig, RectifierConfig};
use serde_json::Value;

const CLAUDE_CODE_SYSTEM_IDENTITY: &str =
    "You are Claude Code, Anthropic's official CLI for Claude.";

/// Anthropic requires max_tokens; fall back to this default only when the
/// Codex request omits max_output_tokens (rare — Codex normally sends it).
/// Kept conservative so a low-output-ceiling model or relay does not hard-400
/// on the fallback (a too-high default 400s and is non-retryable); 8192 is
/// accepted by every current Claude model and virtually all gateways. The
/// transform clamps any thinking budget below this value.
const DEFAULT_CODEX_ANTHROPIC_MAX_TOKENS: u64 = 8192;

/// 本次尝试在改写请求体之前就能确定的路由判定
#[derive(Debug, Clone, Copy)]
pub(crate) struct RouteFlags {
    pub is_full_url: bool,
    /// GitHub Copilot API 使用 /chat/completions（无 /v1 前缀）
    pub is_copilot: bool,
    pub codex_responses_to_chat: bool,
    pub codex_responses_to_anthropic: bool,
    pub codex_official_auth_passthrough: bool,
    /// Codex → Anthropic: Claude Code emulation is off by default and only
    /// enabled when the user explicitly turns it on in the UI, so requests can
    /// pass a gateway's "Claude Code only" fingerprint check (User-Agent /
    /// anthropic-beta / x-app / system prompt first line). Defaulting to off
    /// avoids leaking the Claude Code fingerprint and identity prompt to
    /// general-purpose gateways.
    pub codex_impersonate_claude_code: bool,
}

impl RouteFlags {
    pub(crate) fn resolve(
        app_type: &AppType,
        provider: &Provider,
        endpoint: &str,
        base_url: &str,
    ) -> Self {
        let meta = provider.meta.as_ref();
        let is_codex_like = matches!(app_type, AppType::Codex | AppType::GrokBuild);
        let codex_responses_to_anthropic = is_codex_like
            && providers::should_convert_codex_responses_to_anthropic(provider, endpoint);
        Self {
            is_full_url: meta.and_then(|meta| meta.is_full_url).unwrap_or(false),
            is_copilot: meta.and_then(|m| m.provider_type.as_deref()) == Some("github_copilot")
                || base_url.contains("githubcopilot.com"),
            codex_responses_to_chat: is_codex_like
                && providers::should_convert_codex_responses_to_chat(provider, endpoint),
            codex_responses_to_anthropic,
            codex_official_auth_passthrough: matches!(app_type, AppType::Codex)
                && providers::is_codex_official_provider(provider),
            codex_impersonate_claude_code: codex_responses_to_anthropic
                && meta.and_then(|meta| meta.impersonate_claude_code) == Some(true),
        }
    }
}

/// VendorNormalize 之后、格式转换之前的请求体
pub(crate) struct NormalizedBody {
    pub body: Value,
    /// Claude 客户端解析出的上游 API 格式（非 Claude adapter 为 `None`）
    pub api_format: Option<ClaudeApiFormat>,
    pub needs_transform: bool,
    pub copilot: Option<CopilotOptimization>,
}

/// 定稿的上游请求体
pub(crate) struct BuiltBody {
    pub body: Value,
    /// 实际发往上游的模型名（所有映射/改写之后）
    pub outbound_model: Option<String>,
    /// Codex→Anthropic：模型名带 `[1m]` 标记，需要 context-1m beta
    pub codex_anthropic_one_m: bool,
}

/// 请求体改写阶段
pub(crate) struct BodyStages<'a> {
    pub app_type: &'a AppType,
    pub provider: &'a Provider,
    pub adapter: &'a dyn ProviderAdapter,
    pub flags: RouteFlags,
    pub scope: &'a RequestScope,
    pub deps: &'a ForwarderDeps,
}

impl BodyStages<'_> {
    fn app_handle(&self) -> Option<&tauri::AppHandle> {
        self.deps.app_handle.as_ref()
    }

    /// 生成值不能作为上游缓存身份：只透传客户端提供的 session ID
    fn client_session_id(&self) -> Option<&str> {
        self.scope
            .session_client_provided
            .then_some(self.scope.session_id.as_str())
    }

    /// ModelMapping + VendorNormalize
    pub(crate) async fn normalize(
        &self,
        client_body: &Value,
        headers: &http::HeaderMap,
    ) -> Result<NormalizedBody, ProxyError> {
        let body = self.model_mapping(client_body).await?;
        Ok(self.vendor_normalize(body, client_body, headers).await)
    }

    async fn model_mapping(&self, client_body: &Value) -> Result<Value, ProxyError> {
        // 应用模型映射（独立于格式转换）
        // Claude Desktop proxy 模式必须先把 Desktop 可见的 claude-* route
        // 映射成真实上游模型名，并且未知 route 要直接报错，不能使用默认模型兜底。
        let mapped_body = if matches!(self.app_type, AppType::ClaudeDesktop) {
            crate::claude_desktop_config::map_proxy_request_model(
                client_body.clone(),
                self.provider,
            )
            .map_err(|e| ProxyError::InvalidRequest(e.to_string()))?
        } else {
            let (mapped_body, _original_model, _mapped_model) =
                crate::proxy::model_mapper::apply_model_mapping(client_body.clone(), self.provider);
            mapped_body
        };

        // 与 CCH 对齐：请求前不做 thinking 主动改写（仅保留兼容入口）
        let mut body = normalize_thinking_type(mapped_body);

        // Grok Build exposes a stable client-side model profile in config.toml.
        // Route requests to the provider's real upstream model before applying
        // the optional Responses -> Chat/Anthropic bridge.
        if matches!(self.app_type, AppType::GrokBuild) {
            providers::apply_codex_upstream_model(self.provider, &mut body);
        }

        if self.flags.is_copilot {
            body = providers::copilot_model_map::apply_copilot_model_normalization(body);
            copilot::apply_live_model_resolution(self.app_handle(), self.provider, &mut body).await;
        } else if !self.flags.codex_responses_to_anthropic {
            // Skip on the Codex→Anthropic path: stripping [1m] here would break both the
            // model-catalog match (apply_codex_upstream_model) and the transform's own
            // strip+`context-1m` beta detection. The marker is stripped later, on the
            // final anthropic_body.
            body = crate::proxy::model_mapper::strip_one_m_suffix_for_upstream_from_body(body);
        }
        Ok(body)
    }

    async fn vendor_normalize(
        &self,
        mut body: Value,
        client_body: &Value,
        headers: &http::HeaderMap,
    ) -> NormalizedBody {
        let copilot_config = &self.scope.copilot_optimizer_config;
        let copilot = (self.flags.is_copilot && copilot_config.enabled)
            .then(|| copilot::optimize_request(&mut body, client_body, headers, copilot_config));

        let api_format = if self.adapter.name() == "Claude" {
            let api_format = copilot::resolve_claude_api_format(
                self.app_handle(),
                self.provider,
                &body,
                self.flags.is_copilot,
            )
            .await;
            providers::normalize_anthropic_messages_for_provider(
                &mut body,
                self.provider,
                api_format,
            );
            apply_media_prevention(&self.scope.rectifier_config, &mut body, self.provider);
            Some(api_format)
        } else {
            None
        };
        let needs_transform = match api_format {
            Some(api_format) => providers::claude_api_format_needs_transform(api_format),
            None => self.adapter.needs_transform(self.provider),
        };

        NormalizedBody {
            body,
            api_format,
            needs_transform,
            copilot,
        }
    }

    /// Encode + Canonicalize + LocalOverrides
    pub(crate) async fn finish(
        &self,
        normalized: NormalizedBody,
    ) -> Result<(BuiltBody, Option<CopilotOptimization>), ProxyError> {
        let NormalizedBody {
            body,
            api_format,
            needs_transform,
            copilot,
        } = normalized;
        // 记录映射后的出站模型名（此时已完成接管映射 / [1m] 剥离 / Copilot 归一化）。
        // 格式转换后若 body 仍带 model 字段会在下方刷新覆盖；
        // gemini_native 等模型在 URL 中的格式则保留此处的转换前真值。
        let mut outbound_model = model_of(&body);

        let (request_body, codex_anthropic_one_m) =
            self.encode(body, api_format, needs_transform).await?;
        let body = canonicalize(request_body);
        let body = self.local_overrides(body);

        // 出站 body 定稿后刷新真值（覆盖 Codex chat 上游模型覆写、转换层模型改写）
        if let Some(model) = model_of(&body) {
            outbound_model = Some(model);
        }
        Ok((
            BuiltBody {
                body,
                outbound_model,
                codex_anthropic_one_m,
            },
            copilot,
        ))
    }

    async fn encode(
        &self,
        body: Value,
        api_format: Option<ClaudeApiFormat>,
        needs_transform: bool,
    ) -> Result<(Value, bool), ProxyError> {
        let (mut request_body, one_m) = if self.flags.codex_responses_to_chat {
            (self.encode_codex_chat(body).await?, false)
        } else if self.flags.codex_responses_to_anthropic {
            self.encode_codex_anthropic(body)?
        } else if needs_transform {
            if self.adapter.name() == "Claude" {
                let api_format =
                    api_format.unwrap_or_else(|| providers::get_claude_api_format(self.provider));
                let request_body = providers::transform_claude_request_for_api_format(
                    body,
                    self.provider,
                    api_format,
                    self.client_session_id(),
                    Some(self.deps.gemini_shadow.as_ref()),
                )?;
                (request_body, false)
            } else {
                (self.adapter.transform_request(body, self.provider)?, false)
            }
        } else {
            (body, false)
        };

        if matches!(self.app_type, AppType::Codex | AppType::GrokBuild) {
            apply_media_prevention(
                &self.scope.rectifier_config,
                &mut request_body,
                self.provider,
            );
        }
        Ok((request_body, one_m))
    }

    async fn encode_codex_chat(&self, mut body: Value) -> Result<Value, ProxyError> {
        let explicit_prompt_cache_key = body
            .get("prompt_cache_key")
            .and_then(|value| value.as_str())
            .map(ToString::to_string);
        let restored = self.deps.codex_chat_history.restore(
            &SideStoreScope::new(&self.provider.id, self.client_session_id()),
            &mut body,
        );
        if restored > 0 {
            log::debug!(
                "[Codex] Restored or enriched {restored} cached function call item(s) for Chat upstream"
            );
        }
        providers::apply_codex_chat_upstream_model(self.provider, &mut body);
        let reasoning_config = providers::resolve_codex_chat_reasoning_config(self.provider, &body);
        let mut chat_body =
            providers::transform_codex_chat::responses_to_chat_completions_with_reasoning(
                body,
                reasoning_config.as_ref(),
            )?;
        providers::inject_codex_chat_prompt_cache_key(
            self.provider,
            &mut chat_body,
            explicit_prompt_cache_key.as_deref(),
            self.client_session_id(),
        );
        Ok(chat_body)
    }

    /// 返回 Anthropic 请求体，以及模型名是否带 `[1m]` 标记
    fn encode_codex_anthropic(&self, mut body: Value) -> Result<(Value, bool), ProxyError> {
        providers::apply_codex_upstream_model(self.provider, &mut body);
        // Per-provider output ceiling override. Codex does not forward its
        // `model_max_output_tokens` in the request body, so honor the value
        // configured on the provider here — it takes precedence over any
        // request-supplied `max_output_tokens` and over the default.
        // Injecting it into the body (rather than overriding after transform)
        // lets the thinking-budget clamp size its headroom against the real
        // ceiling too. Kept per-provider to avoid a global large default that
        // would 400 on low-output-ceiling gateways.
        if let Some(max_out) = self
            .provider
            .meta
            .as_ref()
            .and_then(|meta| meta.max_output_tokens)
            .filter(|v| *v > 0)
        {
            body["max_output_tokens"] = Value::from(max_out);
        }
        let mut anthropic_body =
            providers::transform_codex_anthropic::responses_request_to_anthropic(
                body,
                DEFAULT_CODEX_ANTHROPIC_MAX_TOKENS,
            )?;
        // Handle the 1M-context marker [1m]: strip the model-name suffix (the
        // gateway doesn't recognize it) and set the flag so the beta header is
        // added. apply_codex_upstream_model may have just written back a model
        // name carrying [1m] from the provider config, so strip it once more on
        // the final body here.
        let mut one_m = false;
        if let Some(model) = anthropic_body.get("model").and_then(|v| v.as_str()) {
            let stripped = crate::proxy::model_mapper::strip_one_m_suffix_for_upstream(model);
            if stripped != model {
                one_m = true;
                anthropic_body["model"] = Value::String(stripped.to_string());
            }
        }
        if self.flags.codex_impersonate_claude_code {
            prepend_claude_code_system_prompt(&mut anthropic_body);
        }
        // Enable Anthropic prompt caching (no beta header required). Reuse the
        // configured TTL rather than silently forcing 5m on this conversion path.
        // otherwise system/tools/history are re-sent at full price every round,
        // inflating cost and first-token latency. The injector handles the
        // string→array `system` conversion and the new-breakpoint budget.
        crate::proxy::cache_injector::inject(
            &mut anthropic_body,
            &codex_anthropic_cache_config(&self.scope.optimizer_config),
        );
        Ok((anthropic_body, one_m))
    }

    /// 过滤私有参数后应用 provider 级请求体覆盖（Copilot 不允许覆盖）
    fn local_overrides(&self, mut body: Value) -> Value {
        if self.flags.is_copilot {
            return body;
        }
        if let Some(overrides) = self
            .provider
            .meta
            .as_ref()
            .and_then(|meta| meta.local_proxy_request_overrides.as_ref())
        {
            if apply_local_proxy_body_overrides(&mut body, overrides) {
                body = canonicalize(body);
            }
        }
        body
    }

    /// 本次尝试的路由计划：上游格式、URL、认证与供应商特性在此定稿，
    /// 随响应交给 handler 作为解码依据。
    pub(crate) fn route_plan(
        &self,
        api_format: Option<ClaudeApiFormat>,
        url: &str,
        auth: Option<AuthStrategy>,
    ) -> RoutePlan {
        let flags = &self.flags;
        RoutePlan {
            provider_id: self.provider.id.clone(),
            client: self.scope.client_format,
            upstream: if flags.codex_responses_to_chat {
                WireFormat::OpenAiChat
            } else if flags.codex_responses_to_anthropic {
                WireFormat::AnthropicMessages
            } else if let Some(api_format) = api_format {
                api_format.wire_format()
            } else {
                self.scope.client_format
            },
            claude_api_format: api_format,
            url: url.to_string(),
            auth,
            quirks: VendorQuirks {
                copilot: flags.is_copilot,
                codex_oauth: self.provider.is_codex_oauth(),
                full_url: flags.is_full_url,
                codex_official_auth_passthrough: flags.codex_official_auth_passthrough,
                impersonate_claude_code: flags.codex_impersonate_claude_code,
            },
            side_stores: SideStores {
                codex_chat_history: flags.codex_responses_to_chat,
                gemini_shadow: api_format == Some(ClaudeApiFormat::GeminiNative),
            },
        }
    }
}

fn model_of(body: &Value) -> Option<String> {
    body.get("model")
        .and_then(|m| m.as_str())
        .filter(|m| !m.is_empty())
        .map(str::to_string)
}

/// PRE-SEND 优化器：每个 provider 独立决定是否优化
///
/// 返回 body 的副本，避免 Bedrock 优化字段泄漏到非 Bedrock provider（failover 场景）
pub(crate) fn pre_send_body(body: &Value, provider: &Provider, config: &OptimizerConfig) -> Value {
    let mut body = body.clone();
    if config.enabled && is_bedrock_provider(provider) {
        if config.thinking_optimizer {
            crate::proxy::thinking_optimizer::optimize(&mut body, config);
        }
        if config.cache_injection {
            crate::proxy::cache_injector::inject(&mut body, config);
        }
    }
    body
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

/// 预防式 media 降级：发送前对 text-only 模型把图片块替换为标记。
///
/// 受 `enabled && request_media_fallback` 管辖；其中"启发式模型名单预测"
/// 再受 `request_media_heuristic` 单独管辖（显式声明 text-only 始终生效）。
/// 返回被替换的图片块数量（0 = 未触发或开关关闭）。
pub(crate) fn apply_media_prevention(
    config: &RectifierConfig,
    body: &mut Value,
    provider: &Provider,
) -> usize {
    if !(config.enabled && config.request_media_fallback) {
        return 0;
    }
    let replaced_images = crate::proxy::media_sanitizer::replace_images_for_text_only_model(
        body,
        provider,
        config.request_media_heuristic,
    );
    if replaced_images > 0 {
        let model = body.get("model").and_then(Value::as_str).unwrap_or("");
        log::info!(
            "[Media] Replaced {replaced_images} image block(s) with {} for text-only provider={}, model={}",
            crate::proxy::media_sanitizer::UNSUPPORTED_IMAGE_MARKER,
            provider.id,
            model
        );
    }
    replaced_images
}

/// Insert the Claude Code identity as the first line before the `system` field in
/// the Anthropic request body.
///
/// Anthropic subscription/OAuth plans require the first system block to be exactly
/// this identity line. After conversion `system` is a string (from Codex
/// instructions); normalize it into an array here: [identity line, original system...].
fn prepend_claude_code_system_prompt(body: &mut Value) {
    let identity = serde_json::json!({ "type": "text", "text": CLAUDE_CODE_SYSTEM_IDENTITY });
    let mut blocks: Vec<Value> = vec![identity];
    match body.get("system") {
        Some(Value::String(existing)) if !existing.is_empty() => {
            blocks.push(serde_json::json!({ "type": "text", "text": existing }));
        }
        Some(Value::Array(existing)) => {
            // Idempotent: skip re-injection if the first block is already the identity line.
            if existing
                .first()
                .and_then(|b| b.get("text"))
                .and_then(|t| t.as_str())
                == Some(CLAUDE_CODE_SYSTEM_IDENTITY)
            {
                return;
            }
            blocks.extend(existing.iter().cloned());
        }
        _ => {}
    }
    body["system"] = Value::Array(blocks);
}

/// Prompt caching is part of the Codex→Anthropic protocol bridge rather than an
/// optional Bedrock optimizer. Codex requests do not contain Anthropic
/// `cache_control`, so keep bridge caching on by default while still honoring the
/// dedicated cache-injection switch. Injected breakpoints always use Anthropic's
/// standard 5-minute TTL.
fn codex_anthropic_cache_config(config: &OptimizerConfig) -> OptimizerConfig {
    OptimizerConfig {
        enabled: true,
        thinking_optimizer: false,
        cache_injection: config.cache_injection,
    }
}

/// 过滤私有参数（以 `_` 开头的字段），防止内部信息泄露到上游；并规范化键序
///
/// 默认使用空白名单，过滤所有 _ 前缀字段
fn canonicalize(request_body: Value) -> Value {
    canonicalize_value(filter_private_params_with_whitelist(request_body, &[]))
}

fn apply_local_proxy_body_overrides(
    body: &mut Value,
    overrides: &LocalProxyRequestOverrides,
) -> bool {
    let Some(override_body) = overrides.body.as_ref() else {
        return false;
    };

    if !override_body.is_object() {
        log::warn!("[LocalProxyOverrides] Ignoring body override because it is not an object");
        return false;
    }

    merge_json_override(body, override_body)
}

fn merge_json_override(target: &mut Value, patch: &Value) -> bool {
    merge_json_override_inner(target, patch, true)
}

fn merge_json_override_inner(target: &mut Value, patch: &Value, is_top_level: bool) -> bool {
    match (target, patch) {
        (Value::Object(target_map), Value::Object(patch_map)) => {
            let mut changed = false;
            for (key, patch_value) in patch_map {
                if is_top_level && key == "stream" {
                    log::warn!(
                        "[LocalProxyOverrides] Ignoring body override for protected field: stream"
                    );
                    continue;
                }
                match target_map.get_mut(key) {
                    Some(target_value) => {
                        changed |= merge_json_override_inner(target_value, patch_value, false);
                    }
                    None => {
                        target_map.insert(key.clone(), patch_value.clone());
                        changed = true;
                    }
                }
            }
            changed
        }
        (target_value, patch_value) => {
            if target_value == patch_value {
                false
            } else {
                *target_value = patch_value.clone();
                true
            }
        }
    }
}

/// 序列化请求体。GET/HEAD 是 idempotent/safe 方法，按 HTTP 语义不应携带 body；
/// 强行附带 JSON body 会让某些上游（如 Google Gemini 的 models.list）拒绝请求。
pub(crate) fn serialize_body(method: &http::Method, body: &Value) -> Result<Vec<u8>, ProxyError> {
    if matches!(method, &http::Method::GET | &http::Method::HEAD) {
        return Ok(Vec::new());
    }
    serde_json::to_vec(body)
        .map_err(|e| ProxyError::Internal(format!("Failed to serialize request body: {e}")))
}

pub(crate) fn is_streaming_request(
    endpoint: &str,
    body: &Value,
    headers: &axum::http::HeaderMap,
) -> bool {
    if body
        .get("stream")
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
    {
        return true;
    }

    if endpoint.contains("streamGenerateContent") || endpoint.contains("alt=sse") {
        return true;
    }

    headers
        .get(axum::http::header::ACCEPT)
        .and_then(|value| value.to_str().ok())
        .map(|accept| accept.contains("text/event-stream"))
        .unwrap_or(false)
}

pub(crate) fn log_prompt_cache_trace(
    app_type: &AppType,
    provider: &Provider,
    endpoint: &str,
    api_format: Option<ClaudeApiFormat>,
    body: &Value,
    session_client_provided: bool,
) {
    if !log::log_enabled!(log::Level::Debug) {
        return;
    }

    let prompt_cache_key = body
        .get("prompt_cache_key")
        .and_then(|value| value.as_str())
        .map(|key| format!("present(len={})", key.len()))
        .unwrap_or_else(|| "absent".to_string());
    let store = body
        .get("store")
        .map(value_for_log)
        .unwrap_or_else(|| "absent".to_string());
    let stream = body
        .get("stream")
        .map(value_for_log)
        .unwrap_or_else(|| "absent".to_string());
    let cache_controls = cache_control_summary(body);

    log::debug!(
        "[CacheTrace] app={}, provider={}, endpoint={}, api_format={}, session_client_provided={}, prompt_cache_key={}, store={}, stream={}, instructions_hash={}, system_hash={}, tools_hash={}, input_hash={}, messages_hash={}, include_hash={}, cache_controls={}, body_hash={}",
        app_type.as_str(),
        provider.id,
        endpoint,
        api_format.map_or("native", ClaudeApiFormat::as_str),
        session_client_provided,
        prompt_cache_key,
        store,
        stream,
        short_value_hash(body.get("instructions")),
        short_value_hash(body.get("system")),
        short_value_hash(body.get("tools")),
        short_value_hash(body.get("input")),
        short_value_hash(body.get("messages")),
        short_value_hash(body.get("include")),
        cache_controls,
        short_value_hash(Some(body)),
    );
}

fn cache_control_summary(value: &Value) -> String {
    fn walk(value: &Value, count: &mut usize, ttls: &mut std::collections::BTreeSet<String>) {
        match value {
            Value::Object(object) => {
                if let Some(cache_control) = object.get("cache_control") {
                    *count += 1;
                    let ttl = cache_control
                        .get("ttl")
                        .and_then(Value::as_str)
                        .unwrap_or("default");
                    ttls.insert(ttl.to_string());
                }
                for child in object.values() {
                    walk(child, count, ttls);
                }
            }
            Value::Array(items) => {
                for child in items {
                    walk(child, count, ttls);
                }
            }
            _ => {}
        }
    }

    let mut count = 0;
    let mut ttls = std::collections::BTreeSet::new();
    walk(value, &mut count, &mut ttls);
    format!(
        "count={count},ttls={}",
        if ttls.is_empty() {
            "none".to_string()
        } else {
            ttls.into_iter().collect::<Vec<_>>().join("|")
        }
    )
}

fn value_for_log(value: &Value) -> String {
    match value {
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::String(value) => value.clone(),
        Value::Null => "null".to_string(),
        Value::Array(values) => format!("array(len={})", values.len()),
        Value::Object(values) => format!("object(len={})", values.len()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proxy::pipeline::recovery::media_downgrade_should_trigger;
    use axum::http::header::{HeaderValue, ACCEPT};
    use axum::http::HeaderMap;
    use serde_json::json;
    use std::collections::HashMap;

    fn test_provider_with_type(provider_type: Option<&str>) -> Provider {
        let mut provider = Provider::with_id(
            "provider-1".to_string(),
            "Provider 1".to_string(),
            json!({}),
            None,
        );
        provider.meta = provider_type.map(|value| crate::provider::ProviderMeta {
            provider_type: Some(value.to_string()),
            ..Default::default()
        });
        provider
    }

    fn rectifier(config: RectifierConfig) -> RectifierConfig {
        config
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
    fn canonicalize_filters_private_fields_and_canonicalizes_order() {
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

        let prepared = canonicalize(body);

        assert!(prepared.get("_internal").is_none());
        assert!(prepared["tools"][0]["parameters"]["properties"]
            .get("_id")
            .is_some());
        assert!(prepared["tools"][0]["parameters"]["properties"]["_id"]
            .get("_private_note")
            .is_none());
        assert_eq!(
            serde_json::to_string(&prepared).unwrap(),
            r#"{"a":2,"tools":[{"name":"lookup","parameters":{"properties":{"_id":{"type":"string"},"a":{"type":"string"},"b":{"type":"number"}},"type":"object"}}],"z":1}"#
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
    fn force_identity_for_stream_flag_requests() {
        let headers = HeaderMap::new();

        assert!(is_streaming_request(
            "/v1/responses",
            &json!({ "stream": true }),
            &headers
        ));
    }

    #[test]
    fn force_identity_for_gemini_stream_endpoints() {
        let headers = HeaderMap::new();

        assert!(is_streaming_request(
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

        assert!(is_streaming_request(
            "/v1/responses",
            &json!({ "model": "gpt-5" }),
            &headers
        ));
    }

    #[test]
    fn non_streaming_requests_allow_automatic_compression() {
        let headers = HeaderMap::new();

        assert!(!is_streaming_request(
            "/v1/responses",
            &json!({ "model": "gpt-5" }),
            &headers
        ));
    }

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
        let fwd = rectifier(RectifierConfig::default());
        let provider = provider_with_settings(json!({}));
        let mut body = body_with_image("deepseek-v4-pro");

        let replaced = apply_media_prevention(&fwd, &mut body, &provider);

        assert_eq!(replaced, 1, "默认全开 + 名单内模型应预替换");
        assert_eq!(body["messages"][0]["content"][0]["type"], "text");
    }

    #[test]
    fn prevention_skipped_when_media_fallback_off() {
        // 关闭 request_media_fallback：即使名单命中也不预替换。
        let fwd = rectifier(RectifierConfig {
            request_media_fallback: false,
            ..RectifierConfig::default()
        });
        let provider = provider_with_settings(json!({}));
        let mut body = body_with_image("deepseek-v4-pro");

        let replaced = apply_media_prevention(&fwd, &mut body, &provider);

        assert_eq!(replaced, 0);
        assert_eq!(body["messages"][0]["content"][0]["type"], "image");
    }

    #[test]
    fn prevention_skipped_when_master_switch_off() {
        let fwd = rectifier(RectifierConfig {
            enabled: false,
            ..RectifierConfig::default()
        });
        let provider = provider_with_settings(json!({}));
        let mut body = body_with_image("deepseek-v4-pro");

        assert_eq!(apply_media_prevention(&fwd, &mut body, &provider), 0);
        assert_eq!(body["messages"][0]["content"][0]["type"], "image");
    }

    #[test]
    fn prevention_heuristic_off_skips_list_but_keeps_explicit_text_only() {
        // 关闭 request_media_heuristic：名单预测失效，但显式声明 text-only 仍预替换。
        let fwd = rectifier(RectifierConfig {
            request_media_heuristic: false,
            ..RectifierConfig::default()
        });

        // (a) 名单内模型、无显式声明 → 不再预替换
        let bare_provider = provider_with_settings(json!({}));
        let mut list_body = body_with_image("deepseek-v4-pro");
        assert_eq!(
            apply_media_prevention(&fwd, &mut list_body, &bare_provider),
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
            apply_media_prevention(&fwd, &mut declared_body, &declared_provider),
            1,
            "显式 text-only 即使关闭 heuristic 也应预替换"
        );
        assert_eq!(declared_body["messages"][0]["content"][0]["type"], "text");
    }

    #[test]
    fn reactive_triggers_when_all_switches_on() {
        let fwd = rectifier(RectifierConfig::default());
        let body = body_with_image("any-model");
        assert!(media_downgrade_should_trigger(
            &fwd,
            "Claude",
            &body,
            &image_unsupported_error()
        ));
    }

    #[test]
    fn reactive_triggers_for_codex_image_url_deserialize_errors() {
        let fwd = rectifier(RectifierConfig::default());
        let body = body_with_codex_input_image("deepseek-v4-flash");
        let error = ProxyError::UpstreamError {
            status: 400,
            body: Some(
                r#"{"error":{"message":"Failed to deserialize the JSON body into the target type: messages[11]: unknown variant image_url, expected text"}}"#
                    .to_string(),
            ),
        };

        assert!(media_downgrade_should_trigger(&fwd, "Codex", &body, &error));
    }

    #[test]
    fn reactive_skipped_when_media_fallback_off() {
        // 关闭 request_media_fallback：上游报图片错误也不触发兜底重试。
        let fwd = rectifier(RectifierConfig {
            request_media_fallback: false,
            ..RectifierConfig::default()
        });
        let body = body_with_image("any-model");
        assert!(!media_downgrade_should_trigger(
            &fwd,
            "Claude",
            &body,
            &image_unsupported_error()
        ));
    }

    #[test]
    fn reactive_skipped_when_master_switch_off() {
        let fwd = rectifier(RectifierConfig {
            enabled: false,
            ..RectifierConfig::default()
        });
        let body = body_with_image("any-model");
        assert!(!media_downgrade_should_trigger(
            &fwd,
            "Claude",
            &body,
            &image_unsupported_error()
        ));
    }

    #[test]
    fn reactive_unaffected_by_heuristic_switch() {
        // 关闭 request_media_heuristic 不影响反应式兜底——它是上游实测错误后的恢复，不是预测。
        let fwd = rectifier(RectifierConfig {
            request_media_heuristic: false,
            ..RectifierConfig::default()
        });
        let body = body_with_image("any-model");
        assert!(media_downgrade_should_trigger(
            &fwd,
            "Claude",
            &body,
            &image_unsupported_error()
        ));
    }
}
