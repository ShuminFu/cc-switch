//! OpenAI Chat Completions 响应体 → [`ChatResponse`]

use serde_json::Value;

use super::invalid;
use crate::proxy::codec::CodecError;
use crate::proxy::protocol::ir::{ChatResponse, Part};
use crate::proxy::protocol::StopReason;
use crate::proxy::usage::ir::UsageIR;

pub(super) fn decode_response(body: &Value) -> Result<ChatResponse, CodecError> {
    let choices = body
        .get("choices")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("No choices in response"))?;
    let choice = choices
        .first()
        .ok_or_else(|| invalid("Empty choices array"))?;
    let message = choice
        .get("message")
        .ok_or_else(|| invalid("No message in choice"))?;

    let mut parts = Vec::new();

    // DeepSeek 等把思考内容放在 message.reasoning_content
    if let Some(reasoning) = message
        .get("reasoning_content")
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
    {
        parts.push(Part::Reasoning {
            text: reasoning.to_string(),
            signature: None,
        });
    }

    // 文本 / 拒绝内容
    match message.get("content") {
        Some(Value::String(text)) => push_text(&mut parts, Some(text)),
        Some(Value::Array(content)) => {
            for part in content {
                match part.get("type").and_then(Value::as_str).unwrap_or_default() {
                    "text" | "output_text" => {
                        push_text(&mut parts, part.get("text").and_then(Value::as_str))
                    }
                    "refusal" => push_text(&mut parts, part.get("refusal").and_then(Value::as_str)),
                    _ => {}
                }
            }
        }
        _ => {}
    }
    // 部分上游把 refusal 放在 message 层
    push_text(&mut parts, message.get("refusal").and_then(Value::as_str));

    let tool_calls = message
        .get("tool_calls")
        .and_then(Value::as_array)
        .filter(|calls| !calls.is_empty());
    let mut has_tool_use = tool_calls.is_some();
    for call in tool_calls.into_iter().flatten() {
        let function = call.get("function");
        parts.push(Part::ToolCall {
            id: str_or_empty(call.get("id")),
            name: str_or_empty(function.and_then(|f| f.get("name"))),
            arguments: function
                .and_then(|f| f.get("arguments"))
                .and_then(Value::as_str)
                .unwrap_or("{}")
                .to_string(),
        });
    }

    // 兼容旧格式（function_call）
    if !has_tool_use {
        if let Some(function_call) = message.get("function_call") {
            let name = str_or_empty(function_call.get("name"));
            let arguments = function_call.get("arguments");
            if !name.is_empty() || arguments.is_some() {
                parts.push(Part::ToolCall {
                    id: str_or_empty(function_call.get("id")),
                    name,
                    arguments: match arguments {
                        Some(Value::String(text)) => text.clone(),
                        Some(value @ (Value::Object(_) | Value::Array(_))) => value.to_string(),
                        _ => "{}".to_string(),
                    },
                });
                has_tool_use = true;
            }
        }
    }

    // 与流式路径共用 StopReason 表：Gemini 兼容层 / Ollama / vLLM 携带 tool_calls 时
    // 也会返回 "stop"，此时映射为 tool_use，否则 Claude Code 会结束回合而不执行工具
    let stop_reason = choice
        .get("finish_reason")
        .and_then(Value::as_str)
        .map(|reason| StopReason::from_chat_finish_reason(reason, has_tool_use))
        .or(has_tool_use.then_some(StopReason::ToolUse));

    Ok(ChatResponse {
        id: str_or_empty(body.get("id")),
        model: str_or_empty(body.get("model")),
        parts,
        stop_reason,
        // prompt_tokens 含缓存命中，UsageIR 拆为互斥的 fresh / cache_read / cache_write
        usage: body.get("usage").map(UsageIR::from_chat_completions),
        created: body.get("created").and_then(Value::as_u64),
    })
}

/// 非空文本才成为内容块
fn push_text(parts: &mut Vec<Part>, text: Option<&str>) {
    if let Some(text) = text.filter(|text| !text.is_empty()) {
        parts.push(Part::text(text));
    }
}

fn str_or_empty(value: Option<&Value>) -> String {
    value
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}
