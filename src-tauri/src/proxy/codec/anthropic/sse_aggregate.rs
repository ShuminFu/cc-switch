//! 非流式请求收到 Anthropic SSE 体时的兜底聚合（#2234）

use std::collections::BTreeMap;

use serde_json::{json, Value};

use crate::proxy::error::ProxyError;
use crate::proxy::sse::{strip_sse_field, take_sse_block};

/// Aggregates an Anthropic Messages **SSE stream** (with no Content-Type marker)
/// back into a single Anthropic non-streaming message JSON.
///
/// Used as a fallback: the upstream returned an SSE body for a `stream:false`
/// request but without the `text/event-stream` header (symmetric to the
/// `body_looks_like_sse` fallback on the chat / claude side, see #2234). The
/// aggregated message can be handed directly to [`anthropic_response_to_responses`].
///
/// It also tolerates the last event missing a trailing blank line (truncated
/// stream): after looping over complete event blocks, it processes the residual
/// buffer as the last event.
pub fn anthropic_sse_to_message_value(body: &str) -> Result<Value, ProxyError> {
    let mut message: Option<Value> = None;
    // Collect blocks by content index along with the partial_json accumulator for their tool_use.
    let mut blocks: BTreeMap<u64, Value> = BTreeMap::new();
    let mut json_accum: BTreeMap<u64, String> = BTreeMap::new();
    let mut stop_reason: Option<String> = None;
    let mut delta_output_tokens: Option<u64> = None;
    let mut saw_message_stop = false;

    let mut buffer = body.to_string();
    let process_block = |block: &str,
                         message: &mut Option<Value>,
                         blocks: &mut BTreeMap<u64, Value>,
                         json_accum: &mut BTreeMap<u64, String>,
                         stop_reason: &mut Option<String>,
                         delta_output_tokens: &mut Option<u64>,
                         saw_message_stop: &mut bool|
     -> Result<(), ProxyError> {
        let mut data = String::new();
        for line in block.lines() {
            if let Some(chunk) = strip_sse_field(line, "data") {
                if !data.is_empty() {
                    data.push('\n');
                }
                data.push_str(chunk);
            }
        }
        if data.trim().is_empty() || data.trim() == "[DONE]" {
            return Ok(());
        }
        let value: Value = match serde_json::from_str(data.trim()) {
            Ok(v) => v,
            Err(_) => return Ok(()), // Skip events that cannot be parsed (ping, etc.)
        };
        match value.get("type").and_then(|t| t.as_str()).unwrap_or("") {
            "message_start" => {
                if let Some(msg) = value.get("message") {
                    *message = Some(msg.clone());
                }
            }
            "content_block_start" => {
                if let Some(index) = value.get("index").and_then(|v| v.as_u64()) {
                    let block = value.get("content_block").cloned().unwrap_or(json!({}));
                    blocks.insert(index, block);
                    json_accum.entry(index).or_default();
                }
            }
            "content_block_delta" => {
                if let Some(index) = value.get("index").and_then(|v| v.as_u64()) {
                    let delta = value.get("delta").cloned().unwrap_or(json!({}));
                    match delta.get("type").and_then(|t| t.as_str()).unwrap_or("") {
                        "text_delta" => {
                            if let Some(text) = delta.get("text").and_then(|t| t.as_str()) {
                                append_str_field(
                                    blocks.entry(index).or_insert(json!({})),
                                    "text",
                                    text,
                                );
                            }
                        }
                        "thinking_delta" => {
                            if let Some(text) = delta.get("thinking").and_then(|t| t.as_str()) {
                                append_str_field(
                                    blocks.entry(index).or_insert(json!({})),
                                    "thinking",
                                    text,
                                );
                            }
                        }
                        "signature_delta" => {
                            if let Some(sig) = delta.get("signature").and_then(|t| t.as_str()) {
                                blocks.entry(index).or_insert(json!({}))["signature"] = json!(sig);
                            }
                        }
                        "input_json_delta" => {
                            if let Some(partial) =
                                delta.get("partial_json").and_then(|t| t.as_str())
                            {
                                json_accum.entry(index).or_default().push_str(partial);
                            }
                        }
                        _ => {}
                    }
                }
            }
            "content_block_stop" => {
                if let Some(index) = value.get("index").and_then(|v| v.as_u64()) {
                    if let Some(accum) = json_accum.get(&index) {
                        if !accum.trim().is_empty() {
                            let parsed: Value =
                                serde_json::from_str(accum).unwrap_or_else(|_| json!({}));
                            if let Some(block) = blocks.get_mut(&index) {
                                block["input"] = parsed;
                            }
                        }
                    }
                }
            }
            "message_delta" => {
                if let Some(reason) = value.pointer("/delta/stop_reason").and_then(|v| v.as_str()) {
                    *stop_reason = Some(reason.to_string());
                }
                if let Some(output) = value
                    .pointer("/usage/output_tokens")
                    .and_then(|v| v.as_u64())
                {
                    *delta_output_tokens = Some(output);
                }
            }
            "message_stop" => *saw_message_stop = true,
            "error" => {
                let msg = value
                    .pointer("/error/message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("upstream anthropic SSE error");
                return Err(ProxyError::TransformError(format!(
                    "anthropic SSE error event: {msg}"
                )));
            }
            _ => {}
        }
        Ok(())
    };

    while let Some(block) = take_sse_block(&mut buffer) {
        process_block(
            &block,
            &mut message,
            &mut blocks,
            &mut json_accum,
            &mut stop_reason,
            &mut delta_output_tokens,
            &mut saw_message_stop,
        )?;
    }
    // Tolerate the last event missing a trailing blank line (truncated stream).
    if !buffer.trim().is_empty() {
        process_block(
            &buffer.clone(),
            &mut message,
            &mut blocks,
            &mut json_accum,
            &mut stop_reason,
            &mut delta_output_tokens,
            &mut saw_message_stop,
        )?;
    }

    let mut message = message.ok_or_else(|| {
        ProxyError::TransformError(
            "anthropic SSE aggregation: missing message_start event".to_string(),
        )
    })?;

    if !saw_message_stop && stop_reason.is_none() {
        if blocks.is_empty() {
            return Err(ProxyError::TransformError(
                "anthropic SSE aggregation: stream ended before message_stop".to_string(),
            ));
        }
        // Preserve partial content but make the truncation visible to Codex instead
        // of returning a normal completed response.
        stop_reason = Some("max_tokens".to_string());
    }

    // Merge in the content blocks (ordered by index), stop_reason, and the cumulative output_tokens.
    let content: Vec<Value> = blocks.into_values().collect();
    message["content"] = json!(content);
    if let Some(reason) = stop_reason {
        message["stop_reason"] = json!(reason);
    }
    if let Some(output) = delta_output_tokens {
        // message_delta's usage.output_tokens is a cumulative value, overriding the 0 from message_start.
        if let Some(usage) = message.get_mut("usage").and_then(|u| u.as_object_mut()) {
            usage.insert("output_tokens".to_string(), json!(output));
        }
    }

    Ok(message)
}

/// Appends content to a string field of a JSON object (creating it if absent).
fn append_str_field(block: &mut Value, field: &str, text: &str) {
    let existing = block
        .get(field)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    block[field] = json!(format!("{existing}{text}"));
}

#[cfg(test)]
mod tests {
    use super::*;

    // ==================== SSE aggregation fallback ====================

    #[test]
    fn test_anthropic_sse_aggregation_text_and_usage() {
        let sse = "event: message_start\n\
data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"model\":\"claude\",\"content\":[],\"stop_reason\":null,\"usage\":{\"input_tokens\":10,\"output_tokens\":0}}}\n\n\
event: content_block_start\n\
data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n\
event: content_block_delta\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello\"}}\n\n\
event: content_block_delta\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\" world\"}}\n\n\
event: content_block_stop\n\
data: {\"type\":\"content_block_stop\",\"index\":0}\n\n\
event: message_delta\n\
data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":7}}\n\n\
event: message_stop\n\
data: {\"type\":\"message_stop\"}\n\n";
        let msg = anthropic_sse_to_message_value(sse).unwrap();
        assert_eq!(msg["content"][0]["type"], "text");
        assert_eq!(msg["content"][0]["text"], "Hello world");
        assert_eq!(msg["stop_reason"], "end_turn");
        assert_eq!(msg["usage"]["input_tokens"], 10);
        assert_eq!(msg["usage"]["output_tokens"], 7);

        // The aggregated result can be converted directly into Responses.
        let resp = crate::proxy::codec::bridge::anthropic_to_responses_response(
            msg,
            &Default::default(),
            crate::proxy::codec::CodecEngine::Ir,
        )
        .unwrap();
        assert_eq!(resp["status"], "completed");
        assert_eq!(resp["output"][0]["content"][0]["text"], "Hello world");
    }

    #[test]
    fn test_anthropic_sse_aggregation_tool_use_partial_json() {
        let sse = "data: {\"type\":\"message_start\",\"message\":{\"id\":\"m\",\"type\":\"message\",\"role\":\"assistant\",\"model\":\"c\",\"content\":[],\"usage\":{\"input_tokens\":1,\"output_tokens\":0}}}\n\n\
data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"tool_use\",\"id\":\"call_1\",\"name\":\"get_weather\",\"input\":{}}}\n\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"city\\\":\"}}\n\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"\\\"Tokyo\\\"}\"}}\n\n\
data: {\"type\":\"content_block_stop\",\"index\":0}\n\n\
data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\"},\"usage\":{\"output_tokens\":3}}\n\n";
        let msg = anthropic_sse_to_message_value(sse).unwrap();
        assert_eq!(msg["content"][0]["type"], "tool_use");
        assert_eq!(msg["content"][0]["name"], "get_weather");
        assert_eq!(msg["content"][0]["input"]["city"], "Tokyo");
        assert_eq!(msg["stop_reason"], "tool_use");
    }

    #[test]
    fn test_anthropic_sse_aggregation_tool_use_input_only_in_start() {
        // Parity guard with the live streaming emitter's
        // `test_tool_use_input_only_in_start_event`: a gateway that carries the full tool
        // `input` on content_block_start and emits NO input_json_delta must still resolve
        // the same arguments (the empty-accum fallback keeps the start-carried input).
        let sse = "data: {\"type\":\"message_start\",\"message\":{\"id\":\"m\",\"type\":\"message\",\"role\":\"assistant\",\"model\":\"c\",\"content\":[],\"usage\":{\"input_tokens\":1,\"output_tokens\":0}}}\n\n\
data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"tool_use\",\"id\":\"call_1\",\"name\":\"get_weather\",\"input\":{\"city\":\"Tokyo\"}}}\n\n\
data: {\"type\":\"content_block_stop\",\"index\":0}\n\n\
data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\"},\"usage\":{\"output_tokens\":3}}\n\n";
        let msg = anthropic_sse_to_message_value(sse).unwrap();
        assert_eq!(msg["content"][0]["type"], "tool_use");
        assert_eq!(msg["content"][0]["name"], "get_weather");
        // Identical to the deltas-only case above — neither path may drop start input.
        assert_eq!(msg["content"][0]["input"]["city"], "Tokyo");
        assert_eq!(msg["stop_reason"], "tool_use");
    }

    #[test]
    fn test_anthropic_sse_aggregation_missing_message_start_errors() {
        let sse = "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n";
        assert!(anthropic_sse_to_message_value(sse).is_err());
    }

    #[test]
    fn test_anthropic_sse_aggregation_error_event_errors() {
        let sse = "data: {\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\",\"message\":\"overloaded\"}}\n\n";
        assert!(anthropic_sse_to_message_value(sse).is_err());
    }

    #[test]
    fn test_anthropic_sse_aggregation_tolerates_missing_trailing_blank_line() {
        // The last event missing a trailing blank line (truncated stream) should still be processed.
        let sse = "data: {\"type\":\"message_start\",\"message\":{\"id\":\"m\",\"type\":\"message\",\"role\":\"assistant\",\"model\":\"c\",\"content\":[],\"usage\":{\"input_tokens\":1,\"output_tokens\":0}}}\n\n\
data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"hi\"}}\n\n\
data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":2}}";
        let msg = anthropic_sse_to_message_value(sse).unwrap();
        assert_eq!(msg["stop_reason"], "end_turn");
        assert_eq!(msg["usage"]["output_tokens"], 2);
    }

    #[test]
    fn test_anthropic_sse_aggregation_truncated_output_is_incomplete() {
        let sse = "data: {\"type\":\"message_start\",\"message\":{\"id\":\"m\",\"content\":[]}}\n\n\
data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"partial\"}}\n\n";
        let message = anthropic_sse_to_message_value(sse).unwrap();
        let response = crate::proxy::codec::bridge::anthropic_to_responses_response(
            message,
            &Default::default(),
            crate::proxy::codec::CodecEngine::Ir,
        )
        .unwrap();
        assert_eq!(response["status"], "incomplete");
        assert_eq!(
            response["incomplete_details"]["reason"],
            "max_output_tokens"
        );
    }

    #[test]
    fn test_anthropic_sse_aggregation_truncated_without_output_errors() {
        let sse =
            "data: {\"type\":\"message_start\",\"message\":{\"id\":\"m\",\"content\":[]}}\n\n";
        assert!(anthropic_sse_to_message_value(sse).is_err());
    }
}
