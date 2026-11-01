//! OpenAI Responses codec: the single implementation of Responses usage,
//! `status`/`incomplete_details`, and response mapping.

use super::{anthropic, Part, Response, StopReason, ToolInput, Usage};
use crate::proxy::error::ProxyError;
use crate::proxy::json_canonical::{canonical_json_string, canonicalize_tool_arguments};
use crate::proxy::providers::reasoning_bridge::anthropic_block_from_openai_reasoning_item;
use crate::proxy::providers::transform_codex_anthropic::responses_reasoning_item_from_anthropic_block;
use crate::proxy::providers::transform_codex_chat::{
    response_tool_call_item_from_chat_name, response_tool_call_item_id_from_chat_name,
    CodexToolContext,
};
use crate::proxy::providers::transform_responses::sanitize_anthropic_tool_use_input;
use serde_json::{json, Value};

// ---------------------------------------------------------------------------
// Usage
// ---------------------------------------------------------------------------

/// Responses `usage` → IR.
///
/// `input_tokens` includes cache reads and writes; both are subtracted to get
/// fresh input. Field resolution: `input_tokens` → `prompt_tokens`;
/// `output_tokens` → `completion_tokens`; cache read: direct
/// `cache_read_input_tokens` → `input_tokens_details.cached_tokens` →
/// `prompt_tokens_details.cached_tokens`; cache write: direct
/// `cache_creation_input_tokens` → nested `cache_write_tokens` (GPT-5.6+).
/// A reported zero bucket stays `Some(0)`.
pub(crate) fn decode_usage(usage: Option<&Value>) -> Usage {
    let Some(u) = usage.filter(|v| v.is_object()) else {
        return Usage::default();
    };
    if u.as_object().is_some_and(|obj| obj.is_empty()) {
        log::warn!("[Responses] Empty usage object received, using defaults");
        return Usage::default();
    }
    let num = |pointer: &str| u.pointer(pointer).and_then(Value::as_u64);
    let input = num("/input_tokens").or_else(|| num("/prompt_tokens"));
    let output = num("/output_tokens").or_else(|| num("/completion_tokens"));
    let (input, output) = (input.unwrap_or(0), output.unwrap_or(0));
    if (input == 0 && output > 0) || (input > 0 && output == 0) {
        // Streaming chunks legitimately carry partial usage.
        log::debug!("[Responses] Partial usage object: {u:?}");
    }
    let cache_read = num("/cache_read_input_tokens")
        .or_else(|| num("/input_tokens_details/cached_tokens"))
        .or_else(|| num("/prompt_tokens_details/cached_tokens"));
    let cache_write = num("/cache_creation_input_tokens")
        .or_else(|| num("/input_tokens_details/cache_write_tokens"))
        .or_else(|| num("/prompt_tokens_details/cache_write_tokens"));
    Usage {
        input: input
            .saturating_sub(cache_read.unwrap_or(0))
            .saturating_sub(cache_write.unwrap_or(0)),
        output,
        cache_read,
        cache_write,
        cache_write_breakdown: u.get("cache_creation").filter(|v| !v.is_null()).cloned(),
        reasoning: num("/output_tokens_details/reasoning_tokens"),
        total: num("/total_tokens"),
    }
}

/// IR → Responses `usage`.
///
/// `input_tokens = fresh + cache_read + cache_write`, with the cache buckets as
/// subsets in `input_tokens_details`. The top-level `cache_creation_input_tokens`
/// alias is kept for one compatibility window; new readers use the nested field.
pub(crate) fn encode_usage(usage: &Usage) -> Value {
    let input = usage.inclusive_input();
    let cache_read = usage.cache_read.unwrap_or(0);
    let cache_write = usage.cache_write.unwrap_or(0);
    let mut result = json!({
        "input_tokens": input,
        "output_tokens": usage.output,
        "total_tokens": usage.total.unwrap_or_else(|| input.saturating_add(usage.output)),
        "output_tokens_details": { "reasoning_tokens": usage.reasoning.unwrap_or(0) }
    });
    if cache_read > 0 || cache_write > 0 {
        result["input_tokens_details"] = json!({
            "cached_tokens": cache_read,
            "cache_write_tokens": cache_write
        });
    }
    if cache_write > 0 {
        result["cache_creation_input_tokens"] = json!(cache_write);
    }
    result
}

// ---------------------------------------------------------------------------
// Stop reason
// ---------------------------------------------------------------------------

/// Responses `status` + `incomplete_details.reason` → IR stop reason.
pub(crate) fn decode_status(
    status: Option<&str>,
    has_tool_use: bool,
    incomplete_reason: Option<&str>,
) -> Option<StopReason> {
    status.map(|status| match (status, incomplete_reason) {
        ("completed", _) if has_tool_use => StopReason::ToolUse,
        ("incomplete", None | Some("max_output_tokens" | "max_tokens")) => StopReason::MaxTokens,
        // PROV-24: content filtering is a refusal, not a normal end of turn.
        ("incomplete", Some("content_filter")) => StopReason::Refusal,
        _ => StopReason::EndTurn,
    })
}

/// IR stop reason → Responses `(status, incomplete_details.reason)`.
pub(crate) fn encode_stop(stop: Option<&StopReason>) -> (&'static str, Option<&'static str>) {
    match stop {
        Some(StopReason::MaxTokens | StopReason::ContextWindowExceeded) => {
            ("incomplete", Some("max_output_tokens"))
        }
        // A safety refusal is incomplete so Codex does not treat it as a normal
        // empty reply.
        Some(StopReason::Refusal) => ("incomplete", Some("content_filter")),
        Some(StopReason::PauseTurn) => {
            // Unreachable for Codex requests (no Anthropic server tools declared).
            log::warn!("[Responses] Unexpected stop_reason=pause_turn, treating it as completed");
            ("completed", None)
        }
        _ => ("completed", None),
    }
}

/// Responses response id from an upstream id (`resp_` prefixed; a missing or
/// empty id becomes `resp_ccswitch`).
pub(crate) fn response_id(id: Option<&str>) -> String {
    match id.filter(|id| !id.is_empty()) {
        None => "resp_ccswitch".to_string(),
        Some(id) if id.starts_with("resp_") => id.to_string(),
        Some(id) => format!("resp_{id}"),
    }
}

// ---------------------------------------------------------------------------
// Response decode
// ---------------------------------------------------------------------------

fn error_message(body: &Value, fallback: &str) -> String {
    body.pointer("/error/message")
        .and_then(Value::as_str)
        .or_else(|| body.get("message").and_then(Value::as_str))
        .or_else(|| body.get("error").and_then(Value::as_str))
        .filter(|message| !message.trim().is_empty())
        .unwrap_or(fallback)
        .to_string()
}

/// A Responses failure can arrive inside an HTTP 2xx object; reject it before
/// looking at `output`, otherwise `{status:"failed", output:[]}` would become a
/// successful empty reply.
fn validate_terminal_status(body: &Value) -> Result<(), ProxyError> {
    let has_error = body.get("error").is_some_and(|error| !error.is_null());
    match body.get("status").and_then(Value::as_str) {
        Some("failed") => Err(ProxyError::TransformError(format!(
            "Responses upstream failed: {}",
            error_message(body, "response generation failed")
        ))),
        Some("cancelled") => Err(ProxyError::TransformError(format!(
            "Responses upstream cancelled the response: {}",
            error_message(body, "response generation was cancelled")
        ))),
        _ if has_error => Err(ProxyError::TransformError(format!(
            "Responses upstream returned an error envelope: {}",
            error_message(body, "unknown upstream error")
        ))),
        _ => Ok(()),
    }
}

/// Parse a `function_call.arguments` string. An incomplete response may carry
/// cut-off arguments, which become `{}`; a completed one must be a JSON object.
fn decode_function_call_arguments(
    name: &str,
    arguments: &str,
    completed: bool,
) -> Result<Value, ProxyError> {
    if arguments.trim().is_empty() {
        return Ok(json!({}));
    }
    let input = match serde_json::from_str::<Value>(arguments) {
        Ok(value) => value,
        Err(error) if !completed => {
            log::warn!(
                "[Responses] Replacing incomplete function_call '{name}' arguments with an empty object: {error}"
            );
            return Ok(json!({}));
        }
        Err(error) => {
            return Err(ProxyError::TransformError(format!(
                "Invalid function_call arguments for '{name}': {error}"
            )))
        }
    };
    if input.is_object() {
        return Ok(input);
    }
    if !completed {
        log::warn!(
            "[Responses] Replacing incomplete function_call '{name}' non-object arguments with an empty object"
        );
        return Ok(json!({}));
    }
    Err(ProxyError::TransformError(format!(
        "Function call arguments for '{name}' must be a JSON object"
    )))
}

/// Non-streaming Responses response → IR. Reasoning items keep their full
/// opaque state through the reasoning bridge envelope.
pub(crate) fn decode_response(body: &Value) -> Result<Response, ProxyError> {
    validate_terminal_status(body)?;
    let output = body
        .get("output")
        .and_then(Value::as_array)
        .ok_or_else(|| ProxyError::TransformError("No output in response".to_string()))?;
    let completed = body.get("status").and_then(Value::as_str) == Some("completed");

    let mut content = Vec::new();
    for item in output {
        match item.get("type").and_then(Value::as_str).unwrap_or("") {
            "message" => {
                for block in item
                    .get("content")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    let field = |key: &str| {
                        block
                            .get(key)
                            .and_then(Value::as_str)
                            .filter(|text| !text.is_empty())
                            .map(ToString::to_string)
                    };
                    match block.get("type").and_then(Value::as_str) {
                        Some("output_text") => content.extend(field("text").map(Part::Text)),
                        Some("refusal") => content.extend(field("refusal").map(Part::Refusal)),
                        _ => {}
                    }
                }
            }
            "function_call" => {
                let name = item.get("name").and_then(Value::as_str).unwrap_or("");
                let arguments = item
                    .get("arguments")
                    .and_then(Value::as_str)
                    .unwrap_or("{}");
                content.push(Part::ToolUse {
                    id: item
                        .get("call_id")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                    name: name.to_string(),
                    input: ToolInput::Json(decode_function_call_arguments(
                        name, arguments, completed,
                    )?),
                });
            }
            "reasoning" => {
                if let Some(block) = anthropic_block_from_openai_reasoning_item(item) {
                    content.push(anthropic::decode_block(&block));
                }
            }
            _ => {}
        }
    }

    let mut response = Response {
        id: body
            .get("id")
            .and_then(Value::as_str)
            .map(ToString::to_string),
        model: body
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        created: body.get("created_at").and_then(Value::as_u64),
        stop: None,
        usage: decode_usage(body.get("usage")),
        content,
    };
    response.stop = decode_status(
        body.get("status").and_then(Value::as_str),
        response.has_tool_use(),
        body.pointer("/incomplete_details/reason")
            .and_then(Value::as_str),
    );
    Ok(response)
}

// ---------------------------------------------------------------------------
// Response encode
// ---------------------------------------------------------------------------

/// Output-item layout for [`encode_response`]; a bridge policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Layout {
    /// Items follow content order: consecutive text parts form one message item
    /// (`{id}_msg_{n}`), each thinking part becomes a reasoning item
    /// (`rs_{id}_{n}`) carrying the Anthropic block in `encrypted_content`.
    Interleaved,
    /// Chat-compatible layout: one summary-only reasoning item (`rs_{id}`), one
    /// message item (`{id}_msg`), then the tool calls, each carrying the
    /// reasoning text as `reasoning_content` for replay through Chat gateways.
    ChatCompat,
}

fn tool_call_arguments(name: &str, input: &ToolInput) -> String {
    match input {
        ToolInput::Json(value) => {
            canonical_json_string(&sanitize_anthropic_tool_use_input(name, value.clone()))
        }
        ToolInput::Raw(raw) => canonicalize_tool_arguments(raw.as_ref()),
    }
}

fn tool_call_item(
    id: &str,
    name: &str,
    input: &ToolInput,
    reasoning: Option<&str>,
    tool_context: &CodexToolContext,
) -> Value {
    let item_id = response_tool_call_item_id_from_chat_name(id, name, tool_context);
    response_tool_call_item_from_chat_name(
        &item_id,
        "completed",
        id,
        name,
        &tool_call_arguments(name, input),
        reasoning,
        tool_context,
    )
}

fn output_text(text: &str) -> Value {
    json!({"type": "output_text", "text": text, "annotations": []})
}

fn message_item(id: String, content: Vec<Value>) -> Value {
    json!({
        "id": id,
        "type": "message",
        "status": "completed",
        "role": "assistant",
        "content": content
    })
}

fn encode_interleaved(
    response: &Response,
    response_id: &str,
    tool_context: &CodexToolContext,
) -> Vec<Value> {
    let mut output: Vec<Value> = Vec::new();
    let mut text_parts: Vec<Value> = Vec::new();
    let flush = |output: &mut Vec<Value>, text_parts: &mut Vec<Value>| {
        if !text_parts.is_empty() {
            let id = format!("{response_id}_msg_{}", output.len());
            output.push(message_item(id, std::mem::take(text_parts)));
        }
    };
    for part in &response.content {
        match part {
            Part::Text(text) => text_parts.push(output_text(text)),
            Part::Refusal(text) => text_parts.push(json!({"type": "refusal", "refusal": text})),
            Part::ToolUse { id, name, input } => {
                flush(&mut output, &mut text_parts);
                output.push(tool_call_item(id, name, input, None, tool_context));
            }
            Part::Thinking { .. } | Part::RedactedThinking { .. } => {
                flush(&mut output, &mut text_parts);
                let item_id = format!("rs_{response_id}_{}", output.len());
                if let Some(item) = anthropic::encode_part(part, false).ok().and_then(|block| {
                    responses_reasoning_item_from_anthropic_block(&item_id, &block)
                }) {
                    output.push(item);
                }
            }
            Part::Other(_) => {
                log::debug!("[Responses] Dropping a content block Responses cannot represent");
            }
        }
    }
    flush(&mut output, &mut text_parts);
    output
}

fn encode_chat_compat(
    response: &Response,
    response_id: &str,
    tool_context: &CodexToolContext,
) -> Vec<Value> {
    let reasoning = response.content.iter().find_map(|part| match part {
        Part::Thinking { text, .. } if !text.is_empty() => Some(text.as_str()),
        _ => None,
    });
    let mut output = Vec::new();
    if let Some(reasoning) = reasoning {
        output.push(json!({
            "id": format!("rs_{response_id}"),
            "type": "reasoning",
            "summary": [{ "type": "summary_text", "text": reasoning }]
        }));
    }
    let message_content: Vec<Value> = response
        .content
        .iter()
        .filter_map(|part| match part {
            Part::Text(text) if !text.is_empty() => Some(output_text(text)),
            Part::Refusal(text) if !text.is_empty() => {
                Some(json!({"type": "refusal", "refusal": text}))
            }
            _ => None,
        })
        .collect();
    if !message_content.is_empty() {
        output.push(message_item(format!("{response_id}_msg"), message_content));
    }
    for part in &response.content {
        if let Part::ToolUse { id, name, input } = part {
            // Defensive: some models emit tool calls without a function name.
            if name.is_empty() {
                log::warn!("[Codex] Skipping tool call with missing name");
                continue;
            }
            output.push(tool_call_item(id, name, input, reasoning, tool_context));
        }
    }
    output
}

/// IR → non-streaming Responses response. `tool_context` restores Codex tool
/// names/kinds (custom tools, namespaces, tool_search) from the original request.
pub(crate) fn encode_response(
    response: &Response,
    tool_context: &CodexToolContext,
    layout: Layout,
) -> Value {
    let id = response_id(response.id.as_deref());
    let output = match layout {
        Layout::Interleaved => encode_interleaved(response, &id, tool_context),
        Layout::ChatCompat => encode_chat_compat(response, &id, tool_context),
    };
    let (status, incomplete_reason) = encode_stop(response.stop.as_ref());
    let mut result = json!({
        "id": id,
        "object": "response",
        "created_at": response.created.unwrap_or(0),
        "status": status,
        "model": response.model,
        "output": output,
        "usage": encode_usage(&response.usage)
    });
    if let Some(reason) = incomplete_reason {
        result["incomplete_details"] = json!({ "reason": reason });
    }
    result
}
