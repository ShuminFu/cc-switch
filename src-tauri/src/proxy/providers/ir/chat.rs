//! OpenAI Chat Completions codec (decode side): the single implementation of
//! Chat usage, `finish_reason` and response mapping, shared by the
//! Chat→Anthropic and Chat→Responses bridges, streaming and non-streaming.

use super::{Part, Response, StopReason, ToolInput, Usage};
use crate::proxy::error::ProxyError;
use crate::proxy::providers::codex_chat_common::{
    extract_reasoning_field_text, split_leading_think_block,
};
use crate::proxy::providers::transform::normalize_tool_call_id;
use serde_json::Value;

/// Chat `usage` → IR.
///
/// `prompt_tokens` includes cache hits and writes, so both are subtracted to get
/// fresh input. Cache buckets come from the Anthropic-style direct fields first
/// (some compatible servers send them), then OpenAI's nested
/// `prompt_tokens_details` / `input_tokens_details`; a zero bucket is treated as
/// "not reported".
pub(crate) fn decode_usage(usage: Option<&Value>) -> Usage {
    let Some(u) = usage.filter(|v| v.is_object()) else {
        return Usage::default();
    };
    let num = |pointer: &str| u.pointer(pointer).and_then(Value::as_u64);
    let prompt = num("/prompt_tokens").or_else(|| num("/input_tokens"));
    let output = num("/completion_tokens")
        .or_else(|| num("/output_tokens"))
        .unwrap_or(0);
    let cache_read = num("/cache_read_input_tokens")
        .or_else(|| num("/prompt_tokens_details/cached_tokens"))
        .or_else(|| num("/input_tokens_details/cached_tokens"))
        .filter(|v| *v > 0);
    let cache_write = num("/cache_creation_input_tokens")
        .or_else(|| num("/prompt_tokens_details/cache_write_tokens"))
        .or_else(|| num("/input_tokens_details/cache_write_tokens"))
        .filter(|v| *v > 0);
    let input = prompt
        .unwrap_or(0)
        .saturating_sub(cache_read.unwrap_or(0))
        .saturating_sub(cache_write.unwrap_or(0));
    Usage {
        input,
        output,
        cache_read,
        cache_write,
        cache_write_breakdown: None,
        reasoning: num("/completion_tokens_details/reasoning_tokens"),
        total: num("/total_tokens"),
    }
}

/// Chat `finish_reason` → IR stop reason. A missing finish reason on a
/// response that carries tool calls means `tool_use`.
pub(crate) fn decode_finish_reason(
    finish_reason: Option<&str>,
    has_tool_use: bool,
) -> Option<StopReason> {
    match finish_reason {
        Some("stop") => Some(StopReason::EndTurn),
        Some("length") => Some(StopReason::MaxTokens),
        Some("tool_calls" | "function_call") => Some(StopReason::ToolUse),
        // PROV-24: a filtered answer must not look like a normal empty reply.
        Some("content_filter") => Some(StopReason::Refusal),
        Some(other) => {
            log::warn!("[Chat] Unknown finish_reason `{other}`, using end_turn");
            Some(StopReason::EndTurn)
        }
        None if has_tool_use => Some(StopReason::ToolUse),
        None => None,
    }
}

/// Bridge policy for [`decode_response`].
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct DecodeOptions {
    /// Split a leading `<think>…</think>` block out of string content into a
    /// thinking part (reasoning models served through plain Chat gateways).
    pub split_think_tags: bool,
}

/// Non-streaming Chat Completions response → IR.
///
/// Part order: reasoning, text/refusal, tool calls. Tool-call ids missing from
/// the upstream are synthesized from the call position (PROV-14); arguments are
/// kept raw so each client encoder applies its own parsing rule.
pub(crate) fn decode_response(
    body: &Value,
    options: DecodeOptions,
) -> Result<Response, ProxyError> {
    let choices = body
        .get("choices")
        .and_then(Value::as_array)
        .ok_or_else(|| ProxyError::TransformError("No choices in chat response".to_string()))?;
    let choice = choices
        .first()
        .ok_or_else(|| ProxyError::TransformError("Empty choices in chat response".to_string()))?;
    let message = choice
        .get("message")
        .ok_or_else(|| ProxyError::TransformError("No message in chat choice".to_string()))?;

    let mut reasoning = extract_reasoning_field_text(message);
    let mut content = Vec::new();

    match message.get("content") {
        Some(Value::String(text)) => {
            let mut text = text.clone();
            if options.split_think_tags {
                if let Some((think, answer)) = split_leading_think_block(&text) {
                    if reasoning.is_none() && !think.is_empty() {
                        reasoning = Some(think);
                    }
                    text = answer;
                }
            }
            if !text.is_empty() {
                content.push(Part::Text(text));
            }
        }
        Some(Value::Array(parts)) => {
            for part in parts {
                let field = |key: &str| {
                    part.get(key)
                        .and_then(Value::as_str)
                        .filter(|text| !text.is_empty())
                        .map(ToString::to_string)
                };
                match part.get("type").and_then(Value::as_str).unwrap_or("") {
                    "text" | "output_text" => content.extend(field("text").map(Part::Text)),
                    "refusal" => content.extend(field("refusal").map(Part::Refusal)),
                    _ => {}
                }
            }
        }
        _ => {}
    }
    if let Some(refusal) = message
        .get("refusal")
        .and_then(Value::as_str)
        .filter(|r| !r.is_empty())
    {
        content.push(Part::Refusal(refusal.to_string()));
    }
    if let Some(reasoning) = reasoning.filter(|r| !r.is_empty()) {
        content.insert(
            0,
            Part::Thinking {
                text: reasoning,
                signature: None,
            },
        );
    }

    let mut has_tool_use = false;
    if let Some(tool_calls) = message.get("tool_calls").and_then(Value::as_array) {
        has_tool_use = !tool_calls.is_empty();
        for (index, call) in tool_calls.iter().enumerate() {
            let function = call.get("function").unwrap_or(&Value::Null);
            content.push(Part::ToolUse {
                id: normalize_tool_call_id(call.get("id").and_then(Value::as_str), index),
                name: function
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                input: ToolInput::Raw(function.get("arguments").cloned()),
            });
        }
    }
    // Legacy single `function_call`.
    if !has_tool_use {
        if let Some(function_call) = message.get("function_call") {
            let name = function_call
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("");
            let arguments = function_call.get("arguments");
            if !name.is_empty() || arguments.is_some() {
                content.push(Part::ToolUse {
                    id: normalize_tool_call_id(function_call.get("id").and_then(Value::as_str), 0),
                    name: name.to_string(),
                    input: ToolInput::Raw(arguments.cloned()),
                });
                has_tool_use = true;
            }
        }
    }

    Ok(Response {
        id: body
            .get("id")
            .and_then(Value::as_str)
            .map(ToString::to_string),
        model: body
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        created: body.get("created").and_then(Value::as_u64),
        content,
        stop: decode_finish_reason(
            choice.get("finish_reason").and_then(Value::as_str),
            has_tool_use,
        ),
        usage: decode_usage(body.get("usage")),
    })
}
