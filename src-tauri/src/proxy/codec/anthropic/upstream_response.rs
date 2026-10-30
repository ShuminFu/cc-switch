//! Anthropic Messages 响应体 → [`ChatResponse`]（Anthropic 作为上游）

use serde_json::{json, Value};

use crate::proxy::codec::CodecError;
use crate::proxy::protocol::ir::{ChatResponse, Part};
use crate::proxy::protocol::{StopReason, WireFormat};
use crate::proxy::usage::ir::UsageIR;

/// 是否为错误信封：`{"type":"error","error":{...}}` 或带 `error` 字段的对象
pub(super) fn is_error_envelope(body: &Value) -> bool {
    body.get("type").and_then(Value::as_str) == Some("error") || body.get("error").is_some()
}

/// SSE `error` 事件（或错误信封）的 message 与类型
pub(super) fn error_details(value: &Value) -> (String, Option<String>) {
    let error = value.get("error").unwrap_or(value);
    let message = error
        .as_str()
        .map(str::to_string)
        .or_else(|| {
            error
                .get("message")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_else(|| error.to_string());
    let error_type = error
        .get("type")
        .and_then(Value::as_str)
        .map(str::to_string);
    (message, error_type)
}

pub(super) fn decode_response(body: &Value) -> Result<ChatResponse, CodecError> {
    if is_error_envelope(body) {
        let error = body.get("error").unwrap_or(body);
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .or_else(|| error.as_str())
            .unwrap_or("Anthropic upstream returned an error envelope");
        let error_type = error.get("type").and_then(Value::as_str).unwrap_or("error");
        return Err(CodecError::Invalid {
            format: WireFormat::AnthropicMessages,
            message: format!("Anthropic upstream {error_type}: {message}"),
        });
    }

    let parts = body
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(decode_block)
        .collect();

    Ok(ChatResponse {
        id: str_or_empty(body.get("id")),
        model: str_or_empty(body.get("model")),
        parts,
        stop_reason: body
            .get("stop_reason")
            .and_then(Value::as_str)
            .map(stop_reason),
        usage: Some(usage(body.get("usage"))),
        created: None,
    })
}

fn str_or_empty(value: Option<&Value>) -> String {
    value
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

/// usage：非对象时为全 0；`input_tokens` 不含缓存读写
pub(super) fn usage(usage: Option<&Value>) -> UsageIR {
    usage
        .filter(|usage| usage.is_object())
        .map(UsageIR::from_anthropic)
        .unwrap_or_default()
}

/// Anthropic `stop_reason` → [`StopReason`]
pub(super) fn stop_reason(reason: &str) -> StopReason {
    match reason {
        "max_tokens" | "model_context_window_exceeded" => StopReason::MaxTokens,
        "refusal" => StopReason::Refusal,
        "tool_use" => StopReason::ToolUse,
        "pause_turn" => {
            // Codex 请求不声明 Anthropic 服务端工具，不应出现 pause_turn
            log::warn!(
                "[Codex] Received unexpected Anthropic stop_reason=pause_turn, treating it as completed"
            );
            StopReason::EndTurn
        }
        _ => StopReason::EndTurn,
    }
}

fn decode_block(block: &Value) -> Option<Part> {
    let str_field = |key: &str| block.get(key).and_then(Value::as_str);
    match str_field("type").unwrap_or_default() {
        "text" => str_field("text").map(Part::text),
        "tool_use" => {
            // 参数保留上游 input 的紧凑 JSON，规范化与 `Read` 清理由客户端编码端负责
            let input = block.get("input").cloned().unwrap_or_else(|| json!({}));
            Some(Part::ToolCall {
                id: str_field("id").unwrap_or_default().to_string(),
                name: str_field("name").unwrap_or_default().to_string(),
                arguments: input.to_string(),
            })
        }
        "thinking" => Some(Part::Reasoning {
            text: str_field("thinking").unwrap_or_default().to_string(),
            signature: str_field("signature").map(str::to_string),
        }),
        "redacted_thinking" => Some(Part::RedactedReasoning {
            data: str_field("data").unwrap_or_default().to_string(),
        }),
        _ => None,
    }
}
