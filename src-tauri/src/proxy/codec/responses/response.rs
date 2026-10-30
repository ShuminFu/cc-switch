//! OpenAI Responses 响应体 → [`ChatResponse`]（Responses 作为上游）
//!
//! 与旧转换器一致：HTTP 2xx 中的 `failed` / `cancelled` / 错误信封是转换错误，
//! 而不是一个空的成功回复；`completed` 回复里的工具参数必须是 JSON 对象，
//! 未完成（截断）的参数替换为空对象。

use serde_json::{json, Value};

use super::{invalid, sanitize_anthropic_tool_use_input};
use crate::proxy::codec::CodecError;
use crate::proxy::protocol::ir::{ChatResponse, Part};
use crate::proxy::protocol::StopReason;
use crate::proxy::providers::reasoning_bridge::{
    encode_openai_reasoning_item, reasoning_summary_text,
};
use crate::proxy::usage::ir::UsageIR;

pub(super) fn decode_response(body: &Value) -> Result<ChatResponse, CodecError> {
    validate_terminal_status(body)?;

    let output = body
        .get("output")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("No output in response"))?;
    let completed = body.get("status").and_then(Value::as_str) == Some("completed");

    let mut parts = Vec::new();
    let mut has_tool_use = false;
    for item in output {
        match item.get("type").and_then(Value::as_str).unwrap_or_default() {
            "message" => {
                for block in item
                    .get("content")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    let text = match block.get("type").and_then(Value::as_str) {
                        Some("output_text") => block.get("text"),
                        Some("refusal") => block.get("refusal"),
                        _ => None,
                    };
                    if let Some(text) = text.and_then(Value::as_str).filter(|t| !t.is_empty()) {
                        parts.push(Part::text(text));
                    }
                }
            }
            "function_call" => {
                parts.push(function_call(item, completed)?);
                has_tool_use = true;
            }
            "reasoning" => parts.extend(reasoning_part(item)),
            _ => {}
        }
    }

    let stop_reason = body.get("status").and_then(Value::as_str).map(|status| {
        StopReason::from_responses_status(
            status,
            has_tool_use,
            body.pointer("/incomplete_details/reason")
                .and_then(Value::as_str),
        )
    });

    Ok(ChatResponse {
        id: str_or_empty(body.get("id")),
        model: str_or_empty(body.get("model")),
        parts,
        stop_reason,
        usage: Some(usage(body.get("usage"))),
    })
}

/// usage：非对象或缺失时为全 0；input 含缓存命中，由 [`UsageIR`] 拆成互斥的三个桶
pub(super) fn usage(usage: Option<&Value>) -> UsageIR {
    match usage {
        Some(usage) if usage.is_object() => UsageIR::from_responses(usage),
        _ => UsageIR::default(),
    }
}

fn str_or_empty(value: Option<&Value>) -> String {
    value
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn error_message(body: &Value, fallback: &str) -> String {
    body.pointer("/error/message")
        .and_then(Value::as_str)
        .or_else(|| body.get("message").and_then(Value::as_str))
        .or_else(|| body.get("error").and_then(Value::as_str))
        .filter(|message| !message.trim().is_empty())
        .unwrap_or(fallback)
        .to_string()
}

fn validate_terminal_status(body: &Value) -> Result<(), CodecError> {
    let has_error = body.get("error").is_some_and(|error| !error.is_null());
    match body.get("status").and_then(Value::as_str) {
        Some("failed") => Err(invalid(format!(
            "Responses upstream failed: {}",
            error_message(body, "response generation failed")
        ))),
        Some("cancelled") => Err(invalid(format!(
            "Responses upstream cancelled the response: {}",
            error_message(body, "response generation was cancelled")
        ))),
        _ if has_error => Err(invalid(format!(
            "Responses upstream returned an error envelope: {}",
            error_message(body, "unknown upstream error")
        ))),
        _ => Ok(()),
    }
}

/// `function_call` 条目 → 工具调用；参数以对象的紧凑 JSON 文本保存
fn function_call(item: &Value, completed: bool) -> Result<Part, CodecError> {
    let id = str_or_empty(item.get("call_id"));
    let name = str_or_empty(item.get("name"));
    let raw = item
        .get("arguments")
        .and_then(Value::as_str)
        .unwrap_or("{}");
    let input: Value = if raw.trim().is_empty() {
        json!({})
    } else {
        match serde_json::from_str(raw) {
            Ok(value) => value,
            Err(error) if !completed => {
                log::warn!(
                    "[Responses] Replacing incomplete function_call '{name}' arguments with an empty object: {error}"
                );
                json!({})
            }
            Err(error) => {
                return Err(invalid(format!(
                    "Invalid function_call arguments for '{name}': {error}"
                )))
            }
        }
    };
    let input = if input.is_object() {
        sanitize_anthropic_tool_use_input(&name, input)
    } else if !completed {
        log::warn!(
            "[Responses] Replacing incomplete function_call '{name}' non-object arguments with an empty object"
        );
        json!({})
    } else {
        return Err(invalid(format!(
            "Function call arguments for '{name}' must be a JSON object"
        )));
    };
    Ok(Part::ToolCall {
        id,
        name,
        arguments: serde_json::to_string(&input).unwrap_or_else(|_| "{}".to_string()),
    })
}

/// `reasoning` 条目 → reasoning 块
///
/// 带加密内容的条目整体装进信封（有摘要时作为 thinking 签名，否则为 redacted_thinking），
/// 客户端回放时由请求编码还原；只有摘要的条目为不带签名的 thinking，两者皆无则丢弃。
pub(super) fn reasoning_part(item: &Value) -> Option<Part> {
    let text = reasoning_summary_text(item);
    let encrypted = item
        .get("encrypted_content")
        .and_then(Value::as_str)
        .is_some_and(|value| !value.is_empty());
    if encrypted {
        let envelope = encode_openai_reasoning_item(item)?;
        return Some(if text.is_empty() {
            Part::RedactedReasoning { data: envelope }
        } else {
            Part::Reasoning {
                text,
                signature: Some(envelope),
            }
        });
    }
    (!text.is_empty()).then_some(Part::Reasoning {
        text,
        signature: None,
    })
}
