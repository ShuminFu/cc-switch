//! [`ChatResponse`] → Gemini `GenerateContentResponse`（Gemini 作为客户端）
//!
//! - 文本 → `{text}`；可见 reasoning → `{text, thought: true}`；工具调用 →
//!   `{functionCall: {id, name, args}}`（Gemini CLI 在 `functionResponse.id` 中带回 id）；
//! - Gemini 没有承载上游签名 thinking 的字段：本轮的签名 thinking / redacted_thinking
//!   整体装进首个 `functionCall` 的 `thoughtSignature`（不透明，客户端在历史中原样带回），
//!   请求解码时还原，工具结果续接的请求因此仍能开启 thinking；纯文本回复不需要回放；
//! - usage：`promptTokenCount` 为含缓存读写的总输入，缓存读取记为
//!   `cachedContentTokenCount`。Gemini 没有缓存写入字段，写入 token 计入普通输入。

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde_json::{json, Map, Value};

use crate::proxy::protocol::ir::{ChatResponse, Part};
use crate::proxy::protocol::StopReason;
use crate::proxy::usage::ir::UsageIR;

/// `thoughtSignature` 中签名 thinking 信封的前缀，把其他来源的签名（真实 Gemini
/// 签名、Gemini CLI 合成的占位签名）隔离开
pub(crate) const THINKING_ENVELOPE_PREFIX: &str = "ccswitch-anthropic-thinking-list-v1:";

/// 签名 thinking / redacted_thinking 块（Anthropic 形状）
pub(super) fn thinking_block(part: &Part) -> Option<Value> {
    match part {
        Part::Reasoning {
            text,
            signature: Some(signature),
        } if !signature.is_empty() => Some(json!({
            "type": "thinking",
            "thinking": text,
            "signature": signature
        })),
        Part::RedactedReasoning { data } if !data.is_empty() => {
            Some(json!({ "type": "redacted_thinking", "data": data }))
        }
        _ => None,
    }
}

/// 把签名 thinking 块装进 `thoughtSignature`；没有块时返回 `None`
pub(super) fn encode_thinking_envelope(blocks: &[Value]) -> Option<String> {
    if blocks.is_empty() {
        return None;
    }
    let bytes = serde_json::to_vec(blocks).ok()?;
    Some(format!(
        "{THINKING_ENVELOPE_PREFIX}{}",
        URL_SAFE_NO_PAD.encode(bytes)
    ))
}

/// 还原 [`encode_thinking_envelope`] 装入的块；其他来源的签名与损坏的信封返回空
pub(super) fn decode_thinking_envelope(signature: &str) -> Vec<Part> {
    let Some(encoded) = signature.strip_prefix(THINKING_ENVELOPE_PREFIX) else {
        return Vec::new();
    };
    let Some(blocks) = URL_SAFE_NO_PAD
        .decode(encoded)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Vec<Value>>(&bytes).ok())
    else {
        log::warn!("[Gemini] Ignoring a malformed cc-switch thinking envelope");
        return Vec::new();
    };
    blocks
        .iter()
        .filter_map(|block| {
            let str_field = |key: &str| {
                block
                    .get(key)
                    .and_then(Value::as_str)
                    .filter(|value| !value.is_empty())
            };
            match block.get("type").and_then(Value::as_str) {
                Some("thinking") => Some(Part::Reasoning {
                    text: block
                        .get("thinking")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    signature: Some(str_field("signature")?.to_string()),
                }),
                Some("redacted_thinking") => Some(Part::RedactedReasoning {
                    data: str_field("data")?.to_string(),
                }),
                _ => None,
            }
        })
        .collect()
}

/// IR 停止原因 → Gemini `finishReason`（工具调用在 Gemini 中同样以 `STOP` 结束）
pub(super) fn finish_reason(stop_reason: Option<StopReason>) -> &'static str {
    match stop_reason {
        Some(StopReason::MaxTokens) => "MAX_TOKENS",
        Some(StopReason::Refusal) => "SAFETY",
        Some(StopReason::EndTurn) | Some(StopReason::ToolUse) | None => "STOP",
    }
}

/// [`UsageIR`] → `usageMetadata`
pub(super) fn usage_metadata(usage: &UsageIR) -> Value {
    let prompt = usage.total_input();
    let mut metadata = json!({
        "promptTokenCount": prompt,
        "candidatesTokenCount": usage.output,
        "totalTokenCount": prompt.saturating_add(usage.output)
    });
    if usage.cache_read > 0 {
        metadata["cachedContentTokenCount"] = json!(usage.cache_read);
    }
    metadata
}

/// 工具调用参数（JSON 文本）→ `functionCall.args` 对象
pub(super) fn function_call_args(arguments: &str) -> Value {
    match serde_json::from_str::<Value>(arguments) {
        Ok(Value::Object(object)) => Value::Object(object),
        _ if arguments.trim().is_empty() => json!({}),
        // 上游给出的参数不是对象（截断或损坏）：原样放进字段，交给客户端报错
        Ok(other) => json!({ "value": other }),
        Err(_) => json!({ "raw_arguments": arguments }),
    }
}

pub(super) fn function_call_part(
    id: &str,
    name: &str,
    arguments: &str,
    thought_signature: Option<String>,
) -> Value {
    let mut call = Map::new();
    if !id.is_empty() {
        call.insert("id".into(), json!(id));
    }
    call.insert("name".into(), json!(name));
    call.insert("args".into(), function_call_args(arguments));
    let mut part = json!({ "functionCall": Value::Object(call) });
    if let Some(signature) = thought_signature {
        part["thoughtSignature"] = json!(signature);
    }
    part
}

/// 一个候选回复的 `GenerateContentResponse`
pub(super) fn response_chunk(
    parts: Vec<Value>,
    finish_reason: Option<&str>,
    usage: Option<&UsageIR>,
    model: &str,
    response_id: &str,
) -> Value {
    let mut candidate = json!({
        "content": { "role": "model", "parts": parts },
        "index": 0
    });
    if let Some(finish_reason) = finish_reason {
        candidate["finishReason"] = json!(finish_reason);
    }
    let mut chunk = json!({ "candidates": [candidate] });
    if let Some(usage) = usage {
        chunk["usageMetadata"] = usage_metadata(usage);
    }
    if !model.is_empty() {
        chunk["modelVersion"] = json!(model);
    }
    if !response_id.is_empty() {
        chunk["responseId"] = json!(response_id);
    }
    chunk
}

pub(super) fn encode_response(response: &ChatResponse) -> Value {
    let blocks: Vec<Value> = response.parts.iter().filter_map(thinking_block).collect();
    let mut envelope = encode_thinking_envelope(&blocks);
    let mut parts = Vec::new();
    for part in &response.parts {
        match part {
            Part::Text { text } if !text.is_empty() => parts.push(json!({ "text": text })),
            Part::Reasoning { text, .. } if !text.is_empty() => {
                parts.push(json!({ "text": text, "thought": true }))
            }
            Part::ToolCall {
                id,
                name,
                arguments,
            } => parts.push(function_call_part(id, name, arguments, envelope.take())),
            _ => {}
        }
    }
    response_chunk(
        parts,
        Some(finish_reason(response.stop_reason)),
        response.usage.as_ref(),
        &response.model,
        &response.id,
    )
}

/// Anthropic 错误类型 → Gemini 错误的 HTTP 状态码与 `status`
pub(crate) fn gemini_error_status(
    error_type: Option<&str>,
    http_status: u16,
) -> (u16, &'static str) {
    let code = match error_type {
        Some("invalid_request_error") => 400,
        Some("authentication_error") => 401,
        Some("permission_error") => 403,
        Some("not_found_error") => 404,
        Some("request_too_large") => 413,
        Some("rate_limit_error") => 429,
        Some("overloaded_error") => 503,
        Some("timeout_error") => 504,
        Some("api_error") => 500,
        _ if (400..600).contains(&http_status) => http_status,
        _ => 500,
    };
    let status = match code {
        400 | 413 => "INVALID_ARGUMENT",
        401 => "UNAUTHENTICATED",
        403 => "PERMISSION_DENIED",
        404 => "NOT_FOUND",
        429 => "RESOURCE_EXHAUSTED",
        503 | 529 => "UNAVAILABLE",
        504 => "DEADLINE_EXCEEDED",
        _ => "INTERNAL",
    };
    (code, status)
}

/// Gemini 错误体：`{"error": {code, message, status}}`
pub(crate) fn gemini_error_body(
    message: &str,
    error_type: Option<&str>,
    http_status: u16,
) -> Value {
    let (code, status) = gemini_error_status(error_type, http_status);
    json!({
        "error": {
            "code": code,
            "message": message,
            "status": status
        }
    })
}

/// Anthropic 错误信封（`{"type":"error","error":{type,message}}`）→ Gemini 错误体；
/// 其他形状返回 `None`
pub(crate) fn gemini_error_from_anthropic(body: &Value, http_status: u16) -> Option<Value> {
    if body.get("type").and_then(Value::as_str) != Some("error") {
        return None;
    }
    let error = body.get("error")?;
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("Anthropic upstream returned an error");
    let error_type = error.get("type").and_then(Value::as_str);
    Some(gemini_error_body(message, error_type, http_status))
}
