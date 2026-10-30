//! OpenAI Chat Completions 响应体 → [`ChatResponse`]

use serde_json::Value;

use super::invalid;
use crate::proxy::codec::CodecError;
use crate::proxy::json_canonical::canonicalize_tool_arguments;
use crate::proxy::protocol::ir::{ChatResponse, Part};
use crate::proxy::protocol::StopReason;
use crate::proxy::providers::codex_chat_common::{
    extract_reasoning_field_text, split_leading_think_block,
};
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

/// Codex Responses 客户端的响应解码（旧 `transform_codex_chat` 的规则）
///
/// reasoning 取自 `reasoning_content` 等字段，没有时取正文开头的 `<think>…</think>`
/// （正文去掉该段）；拒绝内容作为文本；缺 name 的工具调用丢弃，缺 id 的按位置补
/// `call_{index}`；`length` 为截断，其余 finish_reason 都是正常完成。
pub(super) fn decode_response_for_responses(body: &Value) -> Result<ChatResponse, CodecError> {
    let choices = body
        .get("choices")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("No choices in chat response"))?;
    let choice = choices
        .first()
        .ok_or_else(|| invalid("Empty choices in chat response"))?;
    let message = choice
        .get("message")
        .ok_or_else(|| invalid("No message in chat choice"))?;

    let mut parts = Vec::new();
    let content = message.get("content").and_then(Value::as_str);
    let reasoning = extract_reasoning_field_text(message).or_else(|| {
        content
            .and_then(split_leading_think_block)
            .map(|(reasoning, _)| reasoning)
            .filter(|reasoning| !reasoning.is_empty())
    });
    if let Some(reasoning) = reasoning.filter(|reasoning| !reasoning.is_empty()) {
        parts.push(Part::Reasoning {
            text: reasoning,
            signature: None,
        });
    }

    if let Some(text) = content {
        let text = split_leading_think_block(text)
            .map(|(_, answer)| answer)
            .unwrap_or_else(|| text.to_string());
        push_text(&mut parts, Some(&text));
    } else if let Some(content) = message.get("content").and_then(Value::as_array) {
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
    push_text(&mut parts, message.get("refusal").and_then(Value::as_str));

    if let Some(tool_calls) = message.get("tool_calls").and_then(Value::as_array) {
        for (index, call) in tool_calls.iter().enumerate() {
            let function = call.get("function").unwrap_or(&Value::Null);
            let name = str_or_empty(function.get("name"));
            if name.is_empty() {
                log::warn!("[Codex] Skipping tool call with missing name");
                continue;
            }
            parts.push(Part::ToolCall {
                id: call
                    .get("id")
                    .and_then(Value::as_str)
                    .filter(|id| !id.is_empty())
                    .map(str::to_string)
                    .unwrap_or_else(|| format!("call_{index}")),
                name,
                arguments: canonicalize_tool_arguments(function.get("arguments")),
            });
        }
    } else if let Some(function_call) = message.get("function_call") {
        let name = str_or_empty(function_call.get("name"));
        if name.is_empty() {
            log::warn!("[Codex] Skipping legacy function_call with missing name");
        } else {
            parts.push(Part::ToolCall {
                id: function_call
                    .get("id")
                    .and_then(Value::as_str)
                    .filter(|id| !id.is_empty())
                    .unwrap_or("call_0")
                    .to_string(),
                name,
                arguments: canonicalize_tool_arguments(function_call.get("arguments")),
            });
        }
    }

    let stop_reason = match choice.get("finish_reason").and_then(Value::as_str) {
        Some("length") => StopReason::MaxTokens,
        _ => StopReason::EndTurn,
    };
    Ok(ChatResponse {
        id: str_or_empty(body.get("id")),
        model: str_or_empty(body.get("model")),
        parts,
        stop_reason: Some(stop_reason),
        usage: body
            .get("usage")
            .filter(|usage| usage.is_object())
            .map(UsageIR::from_chat_completions),
        created: body.get("created").and_then(Value::as_u64),
    })
}
