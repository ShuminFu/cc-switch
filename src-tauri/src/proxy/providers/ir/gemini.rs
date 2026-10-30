//! Gemini `generateContent` codec (decode side): the single implementation of
//! Gemini usage, `finishReason` and candidate-part mapping, shared by the
//! streaming and non-streaming Gemini→Anthropic bridges.

use super::{Part, Response, StopReason, ToolInput, Usage};
use serde_json::{json, Value};

/// Gemini `usageMetadata` → IR.
///
/// `promptTokenCount` includes `cachedContentTokenCount`, so the cache hit is
/// subtracted to get fresh input; output is `totalTokenCount - promptTokenCount`
/// (thoughts included). A zero cache bucket is "not reported".
pub(crate) fn decode_usage(usage: Option<&Value>) -> Usage {
    let Some(usage) = usage else {
        return Usage::default();
    };
    let num = |key: &str| usage.get(key).and_then(Value::as_u64).unwrap_or(0);
    let prompt = num("promptTokenCount");
    let cached = num("cachedContentTokenCount");
    let total = num("totalTokenCount");
    Usage {
        input: prompt.saturating_sub(cached),
        output: total.saturating_sub(prompt),
        cache_read: (cached > 0).then_some(cached),
        cache_write: None,
        cache_write_breakdown: None,
        reasoning: usage.get("thoughtsTokenCount").and_then(Value::as_u64),
        total: None,
    }
}

/// Gemini `finishReason` → IR stop reason. `blocked` is set when the prompt
/// itself was blocked (`promptFeedback.blockReason`). Normal and unknown
/// reasons map to `tool_use` when the turn produced function calls.
pub(crate) fn decode_finish_reason(
    reason: Option<&str>,
    has_tool_use: bool,
    blocked: bool,
) -> StopReason {
    if blocked {
        return StopReason::Refusal;
    }
    match reason {
        Some("MAX_TOKENS") => StopReason::MaxTokens,
        Some("SAFETY" | "RECITATION" | "SPII" | "BLOCKLIST" | "PROHIBITED_CONTENT") => {
            StopReason::Refusal
        }
        Some("STOP" | "FINISH_REASON_UNSPECIFIED") | None => {
            if has_tool_use {
                StopReason::ToolUse
            } else {
                StopReason::EndTurn
            }
        }
        Some(other) => {
            log::warn!("[Gemini] Unknown finishReason `{other}`, treating it as a normal stop");
            if has_tool_use {
                StopReason::ToolUse
            } else {
                StopReason::EndTurn
            }
        }
    }
}

/// Candidate `content.parts` → IR parts. `thought: true` parts become thinking
/// (PROV-06); function calls must already carry an id (the bridge synthesizes
/// missing ones before calling this so its shadow store sees the same ids).
pub(crate) fn decode_parts(parts: &[Value]) -> Vec<Part> {
    let mut content = Vec::new();
    for part in parts {
        let text = part
            .get("text")
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty());
        if part.get("thought").and_then(Value::as_bool) == Some(true) {
            if let Some(text) = text {
                content.push(Part::Thinking {
                    text: text.to_string(),
                    signature: None,
                });
            }
            continue;
        }
        if part.get("text").and_then(Value::as_str).is_some() {
            content.extend(text.map(|t| Part::Text(t.to_string())));
            continue;
        }
        if let Some(call) = part.get("functionCall") {
            content.push(Part::ToolUse {
                id: call
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                name: call
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                input: ToolInput::Json(call.get("args").cloned().unwrap_or_else(|| json!({}))),
            });
        }
    }
    content
}

/// Gemini response → IR, given the candidate parts the bridge already
/// normalized. A blocked prompt becomes a refusal with an explanatory text.
pub(crate) fn decode_response(body: &Value, parts: Option<&[Value]>) -> Response {
    let block_reason = body
        .pointer("/promptFeedback/blockReason")
        .and_then(Value::as_str);
    let content = match block_reason {
        Some(reason) => vec![Part::Text(format!(
            "Request blocked by Gemini safety filters: {reason}"
        ))],
        None => decode_parts(parts.unwrap_or_default()),
    };
    let has_tool_use = content.iter().any(|p| matches!(p, Part::ToolUse { .. }));
    let finish_reason = body
        .pointer("/candidates/0/finishReason")
        .and_then(Value::as_str);
    Response {
        id: body
            .get("responseId")
            .and_then(Value::as_str)
            .map(ToString::to_string),
        model: body
            .get("modelVersion")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        created: None,
        content,
        stop: Some(decode_finish_reason(
            finish_reason,
            has_tool_use,
            block_reason.is_some(),
        )),
        usage: decode_usage(body.get("usageMetadata")),
    }
}
