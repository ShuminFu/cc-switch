//! 按客户端线协议渲染代理错误
//!
//! 每种客户端 [`WireFormat`] 一个渲染器：
//! - Anthropic Messages / Gemini Native：沿用 [`ProxyError`] 的 `IntoResponse`
//! - OpenAI Chat / Responses（Codex 系客户端）：富化的 `{"error": {...}}` 错误体，
//!   附带供应商、模型、端点等现场信息

use crate::proxy::{
    codec::responses::errors::chat_error_to_response_error,
    error_mapper::{get_error_message, map_proxy_error_to_status},
    handler_context::RequestContext,
    hyper_client::ProxyResponse,
    protocol::WireFormat,
    response_processor::{
        read_decoded_body, strip_entity_headers_for_rebuilt_body, strip_hop_by_hop_response_headers,
    },
    ProxyError,
};
use axum::response::IntoResponse;
use serde_json::{json, Value};

/// 渲染转发层（非上游响应）失败，按客户端线协议选择渲染器
pub(crate) fn render_forward_error(
    client_format: WireFormat,
    ctx: &RequestContext,
    endpoint: &str,
    error: ProxyError,
) -> Result<axum::response::Response, ProxyError> {
    match client_format {
        _ if matches!(error, ProxyError::SpendCapExceeded(_)) => {
            Ok(render_spend_cap_error(client_format, &error))
        }
        WireFormat::AnthropicMessages => Ok(render_anthropic_error(error)),
        WireFormat::GeminiNative => Ok(render_gemini_error(error)),
        WireFormat::OpenAiChat | WireFormat::OpenAiResponses => {
            build_codex_proxy_error_response(ctx, endpoint, &error)
        }
    }
}

/// 渲染选路阶段（尚未建立 [`RequestContext`]）的失败
///
/// 消费限额耗尽（[`ProxyError::SpendCapExceeded`]）按客户端线协议渲染为 429，
/// 让客户端看到各自 SDK 认识的限流/额度错误；其余错误沿用 [`ProxyError`] 的默认渲染。
pub(crate) fn render_selection_error(
    client_format: WireFormat,
    error: ProxyError,
) -> axum::response::Response {
    match &error {
        ProxyError::SpendCapExceeded(_) => render_spend_cap_error(client_format, &error),
        _ => error.into_response(),
    }
}

/// 供应商均已达到消费限额时的 429 错误体（按客户端线协议）
fn render_spend_cap_error(
    client_format: WireFormat,
    error: &ProxyError,
) -> axum::response::Response {
    let status = axum::http::StatusCode::TOO_MANY_REQUESTS;
    let message = format!("CC Switch spend limit reached. {error}");
    let body = match client_format {
        // Anthropic Messages 错误信封
        WireFormat::AnthropicMessages => json!({
            "type": "error",
            "error": { "type": "rate_limit_error", "message": message },
        }),
        // Google API 错误体
        WireFormat::GeminiNative => json!({
            "error": {
                "code": status.as_u16(),
                "message": message,
                "status": "RESOURCE_EXHAUSTED",
            }
        }),
        // OpenAI 额度耗尽错误
        WireFormat::OpenAiChat | WireFormat::OpenAiResponses => json!({
            "error": {
                "message": message,
                "type": "insufficient_quota",
                "code": codex_proxy_error_code(error),
                "param": Value::Null,
            }
        }),
    };
    (status, axum::Json(body)).into_response()
}

/// Anthropic Messages 客户端：透传上游 JSON 错误体，其余错误包装为 `{"error": {...}}`
fn render_anthropic_error(error: ProxyError) -> axum::response::Response {
    error.into_response()
}

/// Gemini Native 客户端：与 Anthropic 共用 [`ProxyError`] 的默认错误形状；
/// Anthropic 上游（Gemini CLI → Anthropic，CCS-187）的错误信封改写为 Gemini 错误体
/// `{"error": {code, message, status}}`，保留 HTTP 状态码
fn render_gemini_error(error: ProxyError) -> axum::response::Response {
    if let ProxyError::UpstreamError {
        status,
        body: Some(body),
    } = &error
    {
        let gemini_body = serde_json::from_str::<Value>(body).ok().and_then(|body| {
            crate::proxy::codec::gemini::client_response::gemini_error_from_anthropic(
                &body, *status,
            )
        });
        if let Some(gemini_body) = gemini_body {
            let status = axum::http::StatusCode::from_u16(*status)
                .unwrap_or(axum::http::StatusCode::BAD_GATEWAY);
            return (status, axum::Json(gemini_body)).into_response();
        }
    }
    error.into_response()
}

/// 把上游 Chat Completions 的错误响应转换为 Responses API 错误形状。
///
/// 与正常响应分支配套：正常响应已经被改写成 Responses 形式，错误响应若仍保留
/// Chat 错误体（如 MiniMax 的 `{"base_resp": {"status_code": 2013}}`），Codex
/// 客户端的错误处理就无法对齐字段。这里读取上游 body、规整成
/// `{"error": {message, type, code, param}}` 并保留原始 HTTP 状态码。
pub(crate) async fn handle_codex_chat_error_response(
    response: ProxyResponse,
    ctx: &RequestContext,
    status: axum::http::StatusCode,
) -> Result<axum::response::Response, ProxyError> {
    let body_timeout = ctx.non_streaming_body_timeout();
    let (mut response_headers, _status, body_bytes) =
        read_decoded_body(response, ctx.tag, body_timeout).await?;

    // 非 JSON 上游错误体（Cloudflare HTML、纯文本 "Unauthorized" 等）若丢成 None，
    // 客户端就看不到原始诊断信息；包成 Value::String 走转换函数的字符串分支。
    let parsed_value: Value = match serde_json::from_slice::<Value>(&body_bytes) {
        Ok(value) => value,
        Err(_) => {
            const MAX_RAW_ERROR_BYTES: usize = 1024;
            let lossy = String::from_utf8_lossy(&body_bytes);
            let truncated = if lossy.len() > MAX_RAW_ERROR_BYTES {
                let mut end = MAX_RAW_ERROR_BYTES;
                while end > 0 && !lossy.is_char_boundary(end) {
                    end -= 1;
                }
                format!("{}…(truncated)", &lossy[..end])
            } else {
                lossy.into_owned()
            };
            log::warn!("[Codex] Chat 错误响应不是合法 JSON，按文本透传: {truncated}");
            Value::String(truncated)
        }
    };

    let responses_error = chat_error_to_response_error(Some(&parsed_value));

    strip_entity_headers_for_rebuilt_body(&mut response_headers);
    strip_hop_by_hop_response_headers(&mut response_headers);
    // Builder::header 是 append 语义；不先 remove 会和上游 Content-Type 双发。
    response_headers.remove(axum::http::header::CONTENT_TYPE);

    let mut builder = axum::response::Response::builder().status(status);
    for (key, value) in response_headers.iter() {
        builder = builder.header(key, value);
    }
    builder = builder.header(
        axum::http::header::CONTENT_TYPE,
        axum::http::HeaderValue::from_static("application/json"),
    );

    let body = serde_json::to_vec(&responses_error).map_err(|e| {
        log::error!("[Codex] 序列化 Responses 错误体失败: {e}");
        ProxyError::TransformError(format!("Failed to serialize responses error: {e}"))
    })?;

    builder.body(axum::body::Body::from(body)).map_err(|e| {
        log::error!("[Codex] 构建 Responses 错误响应失败: {e}");
        ProxyError::Internal(format!("Failed to build response: {e}"))
    })
}

/// 把转发层（非上游响应）的失败构造成富化的 Codex 错误响应。
///
/// 与 `handle_codex_chat_error_response`（处理上游真实错误响应、复制上游头）不同，
/// 这里没有上游响应可参照，只产出一个 `application/json` 错误体。状态码走
/// `map_proxy_error_to_status`，该函数已与 `ProxyError::into_response` 对齐。
///
/// 注意：`endpoint` 经 `endpoint_with_query` 可能携带 query（如 `?beta=true`）并被
/// 原样写入错误体。当前 Codex 端点不在 query 里放凭证，故安全；若将来复用到
/// query 携带密钥的端点（如 Gemini 的 `?key=`），需先脱敏再回显。
fn build_codex_proxy_error_response(
    ctx: &RequestContext,
    endpoint: &str,
    error: &ProxyError,
) -> Result<axum::response::Response, ProxyError> {
    let status = axum::http::StatusCode::from_u16(map_proxy_error_to_status(error))
        .unwrap_or(axum::http::StatusCode::INTERNAL_SERVER_ERROR);
    let body = codex_proxy_error_json(&ctx.provider.name, &ctx.request_model, endpoint, error);
    let body = serde_json::to_vec(&body).map_err(|e| {
        log::error!("[Codex] 序列化代理错误体失败: {e}");
        ProxyError::Internal(format!("Failed to serialize proxy error: {e}"))
    })?;

    axum::response::Response::builder()
        .status(status)
        .header(
            axum::http::header::CONTENT_TYPE,
            axum::http::HeaderValue::from_static("application/json"),
        )
        .body(axum::body::Body::from(body))
        .map_err(|e| {
            log::error!("[Codex] 构建代理错误响应失败: {e}");
            ProxyError::Internal(format!("Failed to build proxy error response: {e}"))
        })
}

fn codex_proxy_error_json(
    provider_name: &str,
    request_model: &str,
    endpoint: &str,
    error: &ProxyError,
) -> Value {
    let (mut body, upstream_status) = match error {
        ProxyError::UpstreamError { status, body } => {
            let parsed_body = body
                .as_deref()
                .map(|body| serde_json::from_str::<Value>(body).unwrap_or_else(|_| json!(body)));
            (
                chat_error_to_response_error(parsed_body.as_ref()),
                Some(*status),
            )
        }
        _ => (
            json!({
                "error": {
                    "message": get_error_message(error),
                    "type": "proxy_error",
                    "code": codex_proxy_error_code(error),
                    "param": Value::Null,
                }
            }),
            None,
        ),
    };

    let Some(error_obj) = body
        .get_mut("error")
        .and_then(|value| value.as_object_mut())
    else {
        return body;
    };

    let message = if upstream_status == Some(413) {
        // 413 来自上游渠道商的网关（典型是 nginx 的 client_max_body_size），不是 CC
        // Switch 本地代理的限制（本地 DefaultBodyLimit 已放到 200MB）。上游响应体往往是
        // 一整段 nginx HTML，对用户毫无价值，这里替换成明确指向上游 + 可操作的指引，
        // 避免「以为是 CC Switch 封装了 nginx / 是本地代理的锅」这种反复出现的误解。
        format!(
            concat!(
                "Upstream provider rejected the request with HTTP 413 (Payload Too Large). ",
                "The request body exceeds the upstream gateway's size limit; this is the ",
                "provider's server-side limit, not a CC Switch limit. ",
                "Provider: {provider}; model: {model}; endpoint: {endpoint}. ",
                "To recover, shrink the request: run /compact, remove large pasted logs or ",
                "inline images, or ask the provider to raise its request body limit ",
                "(e.g. nginx client_max_body_size)."
            ),
            provider = provider_name,
            model = request_model,
            endpoint = endpoint,
        )
    } else {
        let cause = error_obj
            .get("message")
            .and_then(|value| value.as_str())
            .map(ToString::to_string)
            .filter(|message| !message.trim().is_empty())
            .unwrap_or_else(|| get_error_message(error));
        let status_fragment = upstream_status
            .map(|status| format!("; upstream_status: HTTP {status}"))
            .unwrap_or_default();
        format!(
            "CC Switch local proxy failed while handling Codex endpoint {endpoint}. Provider: {provider_name}; model: {request_model}{status_fragment}; cause: {cause}"
        )
    };

    error_obj.insert(
        "message".to_string(),
        Value::String(compact_error_message(&message, 1800)),
    );

    if error_obj
        .get("type")
        .and_then(|value| value.as_str())
        .map(|value| value.trim().is_empty())
        .unwrap_or(true)
    {
        error_obj.insert("type".to_string(), Value::String("proxy_error".to_string()));
    }

    if error_obj.get("code").map(Value::is_null).unwrap_or(true) {
        error_obj.insert(
            "code".to_string(),
            Value::String(codex_proxy_error_code(error).to_string()),
        );
    }

    if !error_obj.contains_key("param") {
        error_obj.insert("param".to_string(), Value::Null);
    }

    error_obj.insert(
        "provider".to_string(),
        Value::String(provider_name.to_string()),
    );
    error_obj.insert(
        "model".to_string(),
        Value::String(request_model.to_string()),
    );
    // 仅用于 Codex 本地路由；不要复用到 query 可能携带凭证的端点。
    error_obj.insert("endpoint".to_string(), Value::String(endpoint.to_string()));
    if let Some(status) = upstream_status {
        error_obj.insert(
            "upstream_status".to_string(),
            Value::Number(serde_json::Number::from(status)),
        );
    }

    body
}

fn codex_proxy_error_code(error: &ProxyError) -> &'static str {
    match error {
        ProxyError::ForwardFailed(_) => "cc_switch_forward_failed",
        ProxyError::Timeout(_) | ProxyError::StreamIdleTimeout(_) => "cc_switch_timeout",
        ProxyError::NoAvailableProvider => "cc_switch_no_available_provider",
        ProxyError::AllProvidersCircuitOpen => "cc_switch_all_providers_circuit_open",
        ProxyError::NoProvidersConfigured => "cc_switch_no_providers_configured",
        ProxyError::SpendCapExceeded(_) => "cc_switch_spend_cap_exceeded",
        ProxyError::MaxRetriesExceeded => "cc_switch_max_retries_exceeded",
        ProxyError::ProviderUnhealthy(_) => "cc_switch_provider_unhealthy",
        ProxyError::ConfigError(_) => "cc_switch_config_error",
        ProxyError::TransformError(_) => "cc_switch_transform_error",
        ProxyError::InvalidRequest(_) => "cc_switch_invalid_request",
        ProxyError::PayloadTooLarge(_) => "cc_switch_payload_too_large",
        ProxyError::AuthError(_) => "cc_switch_auth_error",
        ProxyError::UpstreamError { .. } => "cc_switch_upstream_error",
        ProxyError::DatabaseError(_) => "cc_switch_database_error",
        ProxyError::Internal(_) => "cc_switch_internal_error",
        ProxyError::AlreadyRunning
        | ProxyError::NotRunning
        | ProxyError::BindFailed(_)
        | ProxyError::StopTimeout
        | ProxyError::StopFailed(_) => "cc_switch_proxy_error",
    }
}

fn compact_error_message(message: &str, max_chars: usize) -> String {
    let normalized = message.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.chars().count() <= max_chars {
        return normalized;
    }

    let truncated = normalized
        .chars()
        .take(max_chars)
        .collect::<String>()
        .trim_end()
        .to_string();
    format!("{truncated}…(truncated)")
}

#[cfg(test)]
mod tests {
    use super::{codex_proxy_error_json, render_selection_error};
    use crate::proxy::{protocol::WireFormat, ProxyError};
    use http_body_util::BodyExt;

    #[test]
    fn codex_proxy_forward_error_includes_context_and_cause() {
        let error = ProxyError::ForwardFailed("连接失败: dns lookup failed".to_string());
        let body = codex_proxy_error_json("DeepSeek", "deepseek-chat", "/responses", &error);

        let message = body["error"]["message"].as_str().unwrap();
        assert!(message.contains("CC Switch local proxy failed"));
        assert!(message.contains("DeepSeek"));
        assert!(message.contains("deepseek-chat"));
        assert!(message.contains("/responses"));
        assert!(message.contains("dns lookup failed"));
        assert_eq!(body["error"]["code"], "cc_switch_forward_failed");
        assert_eq!(body["error"]["provider"], "DeepSeek");
        assert_eq!(body["error"]["model"], "deepseek-chat");
    }

    #[test]
    fn codex_proxy_upstream_error_normalizes_nonstandard_body() {
        let error = ProxyError::UpstreamError {
            status: 502,
            body: Some(
                r#"{"base_resp":{"status_code":2013,"status_msg":"upstream gateway failed"}}"#
                    .to_string(),
            ),
        };
        let body = codex_proxy_error_json("MiniMax", "abab6.5s", "/responses", &error);

        let message = body["error"]["message"].as_str().unwrap();
        assert!(message.contains("upstream_status: HTTP 502"));
        assert!(message.contains("upstream gateway failed"));
        assert_eq!(body["error"]["code"], 2013);
        assert_eq!(body["error"]["upstream_status"], 502);
    }

    #[test]
    fn codex_proxy_413_points_to_upstream_not_local_proxy() {
        // 模拟上游渠道商 nginx 因 client_max_body_size 返回的 413 HTML 页面
        // （见 issue #666：长上下文 / 大图 / 大日志撞上游体积上限）
        let error = ProxyError::UpstreamError {
            status: 413,
            body: Some(
                "<html>\r\n<head><title>413 Request Entity Too Large</title></head>\r\n\
                 <body>\r\n<center><h1>413 Request Entity Too Large</h1></center>\r\n\
                 <hr><center>nginx/1.29.6</center>\r\n</body>\r\n</html>"
                    .to_string(),
            ),
        };
        let body = codex_proxy_error_json("HCAI", "gpt-5.5", "/responses", &error);

        let message = body["error"]["message"].as_str().unwrap();
        // 不再误导成「本地代理失败」
        assert!(!message.contains("CC Switch local proxy failed"));
        // 明确指向上游 + 体积超限 + 可操作指引
        assert!(message.contains("413"));
        assert!(message.to_lowercase().contains("upstream"));
        assert!(message.contains("/compact"));
        // 关键：不把整段 nginx HTML 回显给用户
        assert!(!message.contains("<html>"));
        assert!(!message.contains("nginx/1.29.6"));
        // 结构化字段仍然保留，便于程序化消费 / UI 呈现
        assert_eq!(body["error"]["upstream_status"], 413);
        assert_eq!(body["error"]["provider"], "HCAI");
        assert_eq!(body["error"]["model"], "gpt-5.5");
        assert_eq!(body["error"]["endpoint"], "/responses");
    }

    async fn render_spend_cap(format: WireFormat) -> (u16, serde_json::Value) {
        let error = ProxyError::SpendCapExceeded("P1 (p1): daily spend".to_string());
        let response = render_selection_error(format, error);
        let status = response.status().as_u16();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (status, serde_json::from_slice(&bytes).unwrap())
    }

    #[tokio::test]
    async fn spend_cap_error_renders_per_client_format() {
        let (status, body) = render_spend_cap(WireFormat::AnthropicMessages).await;
        assert_eq!(status, 429);
        assert_eq!(body["type"], "error");
        assert_eq!(body["error"]["type"], "rate_limit_error");
        assert!(body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("P1 (p1): daily spend"));

        let (status, body) = render_spend_cap(WireFormat::GeminiNative).await;
        assert_eq!(status, 429);
        assert_eq!(body["error"]["code"], 429);
        assert_eq!(body["error"]["status"], "RESOURCE_EXHAUSTED");

        for format in [WireFormat::OpenAiChat, WireFormat::OpenAiResponses] {
            let (status, body) = render_spend_cap(format).await;
            assert_eq!(status, 429);
            assert_eq!(body["error"]["type"], "insufficient_quota");
            assert_eq!(body["error"]["code"], "cc_switch_spend_cap_exceeded");
            assert!(body["error"]["param"].is_null());
        }
    }

    #[tokio::test]
    async fn other_selection_errors_keep_the_default_rendering() {
        let response = render_selection_error(
            WireFormat::AnthropicMessages,
            ProxyError::NoProvidersConfigured,
        );
        assert_eq!(response.status().as_u16(), 503);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["error"]["type"], "proxy_error");
    }
}
