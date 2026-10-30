//! Anthropic Messages codec: the single implementation of Anthropic usage,
//! stop-reason, content-block, response and SSE-event mapping.

use super::stream::{BlockStart, Delta, StreamEvent};
use super::{Part, Response, StopReason, ToolInput, Usage};
use crate::proxy::error::ProxyError;
use crate::proxy::providers::transform::{anthropic_message_id, parse_tool_call_arguments};
use crate::proxy::providers::transform_responses::sanitize_anthropic_tool_use_input;
use bytes::Bytes;
use serde_json::{json, Value};

// ---------------------------------------------------------------------------
// Usage
// ---------------------------------------------------------------------------

/// Anthropic `usage` → IR. Anthropic input is already cache-exclusive.
pub(crate) fn decode_usage(usage: Option<&Value>) -> Usage {
    let Some(u) = usage.filter(|v| v.is_object()) else {
        return Usage::default();
    };
    Usage {
        input: u.get("input_tokens").and_then(Value::as_u64).unwrap_or(0),
        output: u.get("output_tokens").and_then(Value::as_u64).unwrap_or(0),
        cache_read: u.get("cache_read_input_tokens").and_then(Value::as_u64),
        cache_write: u.get("cache_creation_input_tokens").and_then(Value::as_u64),
        cache_write_breakdown: u.get("cache_creation").filter(|v| !v.is_null()).cloned(),
        reasoning: u
            .pointer("/output_tokens_details/thinking_tokens")
            .and_then(Value::as_u64),
        total: None,
    }
}

/// IR → Anthropic `usage`. Cache buckets are emitted only when the upstream
/// reported them.
pub(crate) fn encode_usage(usage: &Usage) -> Value {
    let mut result = json!({
        "input_tokens": usage.input,
        "output_tokens": usage.output
    });
    if let Some(cache_read) = usage.cache_read {
        result["cache_read_input_tokens"] = json!(cache_read);
    }
    if let Some(cache_write) = usage.cache_write {
        result["cache_creation_input_tokens"] = json!(cache_write);
    }
    if let Some(breakdown) = &usage.cache_write_breakdown {
        result["cache_creation"] = breakdown.clone();
    }
    result
}

// ---------------------------------------------------------------------------
// Stop reason
// ---------------------------------------------------------------------------

pub(crate) fn decode_stop(stop_reason: &str) -> StopReason {
    match stop_reason {
        "end_turn" => StopReason::EndTurn,
        "max_tokens" => StopReason::MaxTokens,
        "tool_use" => StopReason::ToolUse,
        "stop_sequence" => StopReason::StopSequence,
        "refusal" => StopReason::Refusal,
        "pause_turn" => StopReason::PauseTurn,
        "model_context_window_exceeded" => StopReason::ContextWindowExceeded,
        other => StopReason::Other(other.to_string()),
    }
}

pub(crate) fn encode_stop(stop: &StopReason) -> &str {
    match stop {
        StopReason::EndTurn => "end_turn",
        StopReason::MaxTokens => "max_tokens",
        StopReason::ToolUse => "tool_use",
        StopReason::StopSequence => "stop_sequence",
        StopReason::Refusal => "refusal",
        StopReason::PauseTurn => "pause_turn",
        StopReason::ContextWindowExceeded => "model_context_window_exceeded",
        StopReason::Other(other) => other.as_str(),
    }
}

fn encode_optional_stop(stop: Option<&StopReason>) -> Value {
    stop.map(|s| json!(encode_stop(s))).unwrap_or(Value::Null)
}

// ---------------------------------------------------------------------------
// Content blocks
// ---------------------------------------------------------------------------

/// One Anthropic content block → IR part. Blocks the IR does not model (or
/// malformed known blocks) are carried as [`Part::Other`].
pub(crate) fn decode_block(block: &Value) -> Part {
    let str_field = |key: &str| block.get(key).and_then(Value::as_str);
    match str_field("type") {
        Some("text") => match str_field("text") {
            Some(text) => Part::Text(text.to_string()),
            None => Part::Other(block.clone()),
        },
        Some("thinking") => Part::Thinking {
            text: str_field("thinking").unwrap_or("").to_string(),
            signature: str_field("signature").map(ToString::to_string),
        },
        Some("redacted_thinking") => match str_field("data") {
            Some(data) => Part::RedactedThinking {
                data: data.to_string(),
            },
            None => Part::Other(block.clone()),
        },
        Some("tool_use") => Part::ToolUse {
            id: str_field("id").unwrap_or("").to_string(),
            name: str_field("name").unwrap_or("").to_string(),
            input: ToolInput::Json(block.get("input").cloned().unwrap_or_else(|| json!({}))),
        },
        _ => Part::Other(block.clone()),
    }
}

/// Resolve a tool input into the JSON object Anthropic clients expect.
///
/// Raw (Chat) arguments are parsed here; unparsable arguments are an error
/// unless the response was cut off by the token limit (`truncated`), PROV-14.
fn resolve_tool_input(name: &str, input: &ToolInput, truncated: bool) -> Result<Value, ProxyError> {
    let value = match input {
        ToolInput::Json(value) => value.clone(),
        ToolInput::Raw(raw) => parse_tool_call_arguments(raw.as_ref(), name, truncated)?,
    };
    Ok(sanitize_anthropic_tool_use_input(name, value))
}

/// IR part → Anthropic content block. `truncated` relaxes raw tool-argument
/// parsing (see [`resolve_tool_input`]).
pub(crate) fn encode_part(part: &Part, truncated: bool) -> Result<Value, ProxyError> {
    Ok(match part {
        Part::Text(text) | Part::Refusal(text) => json!({"type": "text", "text": text}),
        Part::Thinking { text, signature } => {
            let mut block = json!({"type": "thinking", "thinking": text});
            if let Some(signature) = signature {
                block["signature"] = json!(signature);
            }
            block
        }
        Part::RedactedThinking { data } => json!({"type": "redacted_thinking", "data": data}),
        Part::ToolUse { id, name, input } => json!({
            "type": "tool_use",
            "id": id,
            "name": name,
            "input": resolve_tool_input(name, input, truncated)?
        }),
        Part::Other(value) => value.clone(),
    })
}

// ---------------------------------------------------------------------------
// Response
// ---------------------------------------------------------------------------

/// Anthropic Messages response → IR.
pub(crate) fn decode_response(body: &Value) -> Response {
    Response {
        id: body
            .get("id")
            .and_then(Value::as_str)
            .map(ToString::to_string),
        model: body
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        created: None,
        content: body
            .get("content")
            .and_then(Value::as_array)
            .map(|blocks| blocks.iter().map(decode_block).collect())
            .unwrap_or_default(),
        stop: body
            .get("stop_reason")
            .and_then(Value::as_str)
            .map(decode_stop),
        usage: decode_usage(body.get("usage")),
    }
}

/// IR → Anthropic Messages response. A missing/empty id is synthesized
/// (`msg_<uuid>`, PROV-35).
pub(crate) fn encode_response(response: &Response) -> Result<Value, ProxyError> {
    let truncated = response.stop == Some(StopReason::MaxTokens);
    let content = response
        .content
        .iter()
        .map(|part| encode_part(part, truncated))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(json!({
        "id": anthropic_message_id(response.id.as_deref()),
        "type": "message",
        "role": "assistant",
        "content": content,
        "model": response.model,
        "stop_reason": encode_optional_stop(response.stop.as_ref()),
        "stop_sequence": null,
        "usage": encode_usage(&response.usage)
    }))
}

// ---------------------------------------------------------------------------
// Stream events
// ---------------------------------------------------------------------------

/// One parsed Anthropic SSE `data:` payload → canonical stream event.
/// Returns `None` for events the IR ignores (`ping`, unknown types).
pub(crate) fn decode_stream_event(value: &Value) -> Option<StreamEvent> {
    let index = || value.get("index").and_then(Value::as_u64);
    match value.get("type").and_then(Value::as_str)? {
        "message_start" => {
            let message = value.get("message")?;
            Some(StreamEvent::MessageStart {
                id: message
                    .get("id")
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
                model: message
                    .get("model")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                usage: decode_usage(message.get("usage")),
            })
        }
        "content_block_start" => {
            let block = value.get("content_block").cloned().unwrap_or(json!({}));
            Some(StreamEvent::BlockStart {
                index: index()?,
                block: match decode_block(&block) {
                    Part::Text(_) => BlockStart::Text,
                    Part::Thinking { signature, .. } => BlockStart::Thinking { signature },
                    Part::RedactedThinking { data } => BlockStart::RedactedThinking { data },
                    Part::ToolUse { id, name, input } => BlockStart::ToolUse {
                        id,
                        name,
                        input: match input {
                            ToolInput::Json(value) => Some(value),
                            ToolInput::Raw(_) => None,
                        },
                    },
                    Part::Refusal(_) => BlockStart::Text,
                    Part::Other(value) => BlockStart::Other(value),
                },
            })
        }
        "content_block_delta" => {
            let delta = value.get("delta")?;
            let text = |key: &str| delta.get(key).and_then(Value::as_str).map(str::to_string);
            let delta = match delta.get("type").and_then(Value::as_str)? {
                "text_delta" => Delta::Text(text("text")?),
                "thinking_delta" => Delta::Thinking(text("thinking")?),
                "signature_delta" => Delta::Signature(text("signature")?),
                "input_json_delta" => Delta::InputJson(text("partial_json")?),
                _ => return None,
            };
            Some(StreamEvent::BlockDelta {
                index: index()?,
                delta,
            })
        }
        "content_block_stop" => Some(StreamEvent::BlockStop { index: index()? }),
        "message_delta" => Some(StreamEvent::MessageDelta {
            stop: value
                .pointer("/delta/stop_reason")
                .and_then(Value::as_str)
                .map(decode_stop),
            output_tokens: value
                .pointer("/usage/output_tokens")
                .and_then(Value::as_u64),
            usage: None,
        }),
        "message_stop" => Some(StreamEvent::MessageStop),
        "error" => Some(StreamEvent::Error {
            error_type: value
                .pointer("/error/type")
                .and_then(Value::as_str)
                .unwrap_or("error")
                .to_string(),
            message: value
                .pointer("/error/message")
                .and_then(Value::as_str)
                .unwrap_or("upstream anthropic SSE error")
                .to_string(),
        }),
        _ => None,
    }
}

fn sse(event_name: &str, payload: &Value) -> Bytes {
    Bytes::from(format!(
        "event: {event_name}\ndata: {}\n\n",
        serde_json::to_string(payload).unwrap_or_default()
    ))
}

/// Canonical stream event → Anthropic SSE bytes.
pub(crate) fn encode_stream_event(event: &StreamEvent) -> Bytes {
    match event {
        StreamEvent::MessageStart { id, model, usage } => {
            let mut start_usage = encode_usage(usage);
            start_usage["output_tokens"] = json!(0);
            sse(
                "message_start",
                &json!({
                    "type": "message_start",
                    "message": {
                        "id": id.clone().unwrap_or_default(),
                        "type": "message",
                        "role": "assistant",
                        "model": model,
                        "usage": start_usage
                    }
                }),
            )
        }
        StreamEvent::BlockStart { index, block } => {
            let content_block = match block {
                BlockStart::Text => json!({"type": "text", "text": ""}),
                BlockStart::Thinking { .. } => json!({"type": "thinking", "thinking": ""}),
                BlockStart::RedactedThinking { data } => {
                    json!({"type": "redacted_thinking", "data": data})
                }
                BlockStart::ToolUse { id, name, .. } => {
                    json!({"type": "tool_use", "id": id, "name": name, "input": {}})
                }
                BlockStart::Other(value) => value.clone(),
            };
            sse(
                "content_block_start",
                &json!({"type": "content_block_start", "index": index, "content_block": content_block}),
            )
        }
        StreamEvent::BlockDelta { index, delta } => {
            let delta = match delta {
                Delta::Text(text) => json!({"type": "text_delta", "text": text}),
                Delta::Thinking(text) => json!({"type": "thinking_delta", "thinking": text}),
                Delta::Signature(sig) => json!({"type": "signature_delta", "signature": sig}),
                Delta::InputJson(partial) => {
                    json!({"type": "input_json_delta", "partial_json": partial})
                }
            };
            sse(
                "content_block_delta",
                &json!({"type": "content_block_delta", "index": index, "delta": delta}),
            )
        }
        StreamEvent::BlockStop { index } => sse(
            "content_block_stop",
            &json!({"type": "content_block_stop", "index": index}),
        ),
        StreamEvent::MessageDelta {
            stop,
            usage,
            output_tokens,
        } => {
            let usage = match (usage, output_tokens) {
                (Some(usage), _) => encode_usage(usage),
                (None, output) => json!({"input_tokens": 0, "output_tokens": output.unwrap_or(0)}),
            };
            sse(
                "message_delta",
                &json!({
                    "type": "message_delta",
                    "delta": {"stop_reason": encode_optional_stop(stop.as_ref()), "stop_sequence": null},
                    "usage": usage
                }),
            )
        }
        StreamEvent::MessageStop => sse("message_stop", &json!({"type": "message_stop"})),
        StreamEvent::Error {
            error_type,
            message,
        } => sse(
            "error",
            &json!({"type": "error", "error": {"type": error_type, "message": message}}),
        ),
    }
}

/// IR response → complete Anthropic SSE lifecycle (`message_start` …
/// `message_stop`), for clients that asked for a stream when the upstream
/// answered with plain JSON. Tool inputs are resolved exactly as in
/// [`encode_response`], and the message id is synthesized once when missing.
pub(crate) fn encode_response_as_stream(response: &Response) -> Result<Vec<Bytes>, ProxyError> {
    let truncated = response.stop == Some(StopReason::MaxTokens);
    let mut resolved = response.clone();
    resolved.id = Some(anthropic_message_id(response.id.as_deref()));
    for part in &mut resolved.content {
        if let Part::ToolUse { name, input, .. } = part {
            *input = ToolInput::Json(resolve_tool_input(name, input, truncated)?);
        }
    }
    Ok(super::stream::events_from_response(&resolved)
        .iter()
        .map(encode_stream_event)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proxy::providers::transform_codex_anthropic::anthropic_sse_to_message_value;
    use crate::proxy::providers::transform_responses::responses_to_anthropic;

    /// PROV-18 equivalence: the same upstream content delivered as JSON, or
    /// expanded to an Anthropic SSE stream and aggregated back, yields an equal
    /// Anthropic message.
    #[test]
    fn json_and_stream_then_aggregate_agree() {
        let body = json!({
            "id": "resp_eq", "model": "gpt-5", "status": "completed",
            "output": [
                {"type": "reasoning", "summary": [{"type": "summary_text", "text": "plan"}],
                 "encrypted_content": "opaque"},
                {"type": "message", "role": "assistant",
                 "content": [{"type": "output_text", "text": "Reading."}]},
                {"type": "function_call", "call_id": "call_1", "name": "Read",
                 "arguments": "{\"file_path\":\"a.rs\",\"pages\":\"\"}"}
            ],
            "usage": {"input_tokens": 100, "output_tokens": 20,
                      "input_tokens_details": {"cached_tokens": 40}}
        });
        let direct = responses_to_anthropic(body.clone()).unwrap();

        let response = super::super::responses::decode_response(&body).unwrap();
        let sse: String = encode_response_as_stream(&response)
            .unwrap()
            .iter()
            .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
            .collect();
        let aggregated = anthropic_sse_to_message_value(&sse).unwrap();

        assert_eq!(aggregated, direct);
        assert_eq!(direct["content"][2]["input"], json!({"file_path": "a.rs"}));
        assert_eq!(direct["usage"]["input_tokens"], 60);
    }

    #[test]
    fn usage_round_trips_through_ir() {
        let usage = json!({
            "input_tokens": 5, "output_tokens": 7,
            "cache_read_input_tokens": 3, "cache_creation_input_tokens": 0,
            "cache_creation": {"ephemeral_5m_input_tokens": 0}
        });
        assert_eq!(encode_usage(&decode_usage(Some(&usage))), usage);
    }

    #[test]
    fn unknown_blocks_and_stop_reasons_pass_through() {
        let block = json!({"type": "server_tool_use", "id": "srv_1", "name": "web_search"});
        assert_eq!(encode_part(&decode_block(&block), false).unwrap(), block);
        assert_eq!(encode_stop(&decode_stop("future_reason")), "future_reason");
    }
}
