//! GitHub Copilot 专属的请求阶段
//!
//! 需要 `CopilotAuthManager`（经 AppHandle 获取）的查询：动态 API endpoint、live
//! `/models` 模型解析与 vendor 判定；以及 Copilot 优化器对请求体的分类与改写。

use crate::commands::CopilotAuthState;
use crate::provider::Provider;
use crate::proxy::copilot_optimizer::{self, CopilotClassification};
use crate::proxy::protocol::ClaudeApiFormat;
use crate::proxy::types::CopilotOptimizerConfig;
use serde_json::Value;
use tauri::Manager;

/// Copilot 优化器的分类结果与派生的确定性 ID（用于改写认证指纹头）
pub(crate) struct CopilotOptimization {
    pub classification: CopilotClassification,
    /// 确定性 Request ID（x-request-id / x-agent-task-id）
    pub request_id: Option<String>,
    /// 从 session ID 派生的稳定 interaction ID（同一主对话共享）
    pub interaction_id: Option<String>,
}

fn account_id(provider: &Provider) -> Option<String> {
    provider
        .meta
        .as_ref()
        .and_then(|m| m.managed_account_id_for("github_copilot"))
}

/// Copilot 优化器：分类 + 请求体优化（在格式转换之前执行）
///
/// 执行顺序（与 copilot-api 对齐）：
///   1. 先在原始 body 上分类（保留 tool_result 语义，避免误判为 user）
///   2. 再清洗孤立 tool_result（防止上游 API 报错）
///   3. 再合并 tool_result + text（减少 premium 计费）
///
/// `client_body` 是映射前的客户端请求体，用于派生 session key。
pub(crate) fn optimize_request(
    body: &mut Value,
    client_body: &Value,
    headers: &http::HeaderMap,
    config: &CopilotOptimizerConfig,
) -> CopilotOptimization {
    // 1. 在原始 body 上分类 — 必须在清洗/合并之前执行
    //    孤立 tool_result 仍保持 tool_result 类型，分类能正确识别为 agent
    let has_anthropic_beta = headers.contains_key("anthropic-beta");
    let classification = copilot_optimizer::classify_request(
        body,
        has_anthropic_beta,
        config.compact_detection,
        config.subagent_detection,
    );

    log::debug!(
        "[Copilot] 优化器分类: initiator={}, is_warmup={}, is_compact={}, is_subagent={}",
        classification.initiator,
        classification.is_warmup,
        classification.is_compact,
        classification.is_subagent
    );

    // 2. 孤立 tool_result 清理 — 分类完成后再清洗
    //    防止上游 API 因不匹配的 tool_result 报错导致重试/重复计费
    let mut optimized = copilot_optimizer::sanitize_orphan_tool_results(std::mem::take(body));

    // 3. Tool result 合并 — 将 [tool_result, text] 变为 [tool_result(含text)]
    if config.tool_result_merging {
        optimized = copilot_optimizer::merge_tool_results(optimized);
    }

    // 3.5. 主动剥离 thinking block — Copilot 走 OpenAI 兼容端点不识别该块
    //      避免上游拒绝后由 rectifier 反应式重试（首次请求已消耗 quota）
    if config.strip_thinking {
        optimized = copilot_optimizer::strip_thinking_blocks(optimized);
    }

    // 4. Warmup 小模型降级
    if config.warmup_downgrade && classification.is_warmup {
        log::info!("[Copilot] Warmup 请求降级到模型: {}", config.warmup_model);
        optimized["model"] = serde_json::json!(&config.warmup_model);
    }
    *body = optimized;

    // 预计算确定性 Request ID（在 body 被格式转换 move 之前）
    let session_id = crate::proxy::session::copilot_session_key(headers, client_body);
    let request_id = if config.deterministic_request_id {
        Some(copilot_optimizer::deterministic_request_id(
            body,
            &session_id,
        ))
    } else {
        None
    };

    CopilotOptimization {
        classification,
        request_id,
        interaction_id: copilot_optimizer::deterministic_interaction_id(&session_id),
    }
}

/// GitHub Copilot 动态 endpoint 路由
///
/// 从 CopilotAuthManager 获取缓存的 API endpoint（支持企业版等非默认 endpoint），
/// 只在与当前 base_url 不同时替换。
pub(crate) async fn dynamic_api_endpoint(
    app_handle: Option<&tauri::AppHandle>,
    provider: &Provider,
    base_url: String,
) -> String {
    let Some(app_handle) = app_handle else {
        return base_url;
    };
    let copilot_state = app_handle.state::<CopilotAuthState>();
    let copilot_auth = copilot_state.manager().await;

    let dynamic_endpoint = match account_id(provider) {
        Some(id) => copilot_auth.get_api_endpoint(&id).await,
        None => copilot_auth.get_default_api_endpoint().await,
    };

    if dynamic_endpoint != base_url {
        log::debug!(
            "[Copilot] 使用动态 API endpoint: {} (原: {})",
            dynamic_endpoint,
            base_url
        );
        return dynamic_endpoint;
    }
    base_url
}

pub(crate) async fn resolve_claude_api_format(
    app_handle: Option<&tauri::AppHandle>,
    provider: &Provider,
    body: &Value,
    is_copilot: bool,
) -> ClaudeApiFormat {
    if !is_copilot {
        return crate::proxy::providers::get_claude_api_format(provider);
    }

    let model = body.get("model").and_then(|value| value.as_str());
    if let Some(model_id) = model {
        if is_copilot_openai_vendor_model(app_handle, provider, model_id).await {
            return ClaudeApiFormat::OpenAiResponses;
        }
    }

    ClaudeApiFormat::OpenAiChat
}

/// 用 Copilot live `/models` 列表确认 model ID 真实可用，找不到时按 family 降级。
/// 命中缓存后是同步的；首次请求或 5 min 缓存过期后会触发一次 HTTP。
pub(crate) async fn apply_live_model_resolution(
    app_handle: Option<&tauri::AppHandle>,
    provider: &Provider,
    body: &mut Value,
) {
    let Some(model_id) = body.get("model").and_then(|v| v.as_str()) else {
        return;
    };
    let model_id = model_id.to_string();

    let Some(app_handle) = app_handle else {
        return;
    };
    let copilot_state = app_handle.state::<CopilotAuthState>();
    let copilot_auth = copilot_state.manager().await;

    let models_result = match account_id(provider).as_deref() {
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
        crate::proxy::providers::copilot_model_map::resolve_against_models(&model_id, &models)
    {
        log::info!("[Copilot] live-model resolve: {model_id} → {resolved}");
        body["model"] = Value::String(resolved);
    }
}

async fn is_copilot_openai_vendor_model(
    app_handle: Option<&tauri::AppHandle>,
    provider: &Provider,
    model_id: &str,
) -> bool {
    let Some(app_handle) = app_handle else {
        log::debug!("[Copilot] AppHandle unavailable, fallback to chat/completions");
        return false;
    };

    let copilot_state = app_handle.state::<CopilotAuthState>();
    let copilot_auth = copilot_state.manager().await;

    let vendor_result = match account_id(provider).as_deref() {
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
