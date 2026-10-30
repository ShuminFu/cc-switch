//! Gemini Native streaming conversion module.
//!
//! Converts Gemini `streamGenerateContent?alt=sse` chunks into Anthropic-style
//! SSE events for Claude-compatible clients.

use super::gemini_shadow::{GeminiShadowStore, GeminiToolCallMeta};
use super::transform_gemini::{
    build_anthropic_usage, is_synthesized_tool_call_id, rectify_tool_call_parts,
    synthesize_tool_call_id, AnthropicToolSchemaHints,
};
use crate::proxy::sse::{append_utf8_safe, strip_sse_field, take_sse_block};
use bytes::Bytes;
use futures::stream::{Stream, StreamExt};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::sync::Arc;

fn map_finish_reason(reason: Option<&str>, has_tool_use: bool, blocked: bool) -> &'static str {
    if blocked {
        return "refusal";
    }

    match reason {
        Some("MAX_TOKENS") => "max_tokens",
        Some("SAFETY")
        | Some("RECITATION")
        | Some("SPII")
        | Some("BLOCKLIST")
        | Some("PROHIBITED_CONTENT") => "refusal",
        _ if has_tool_use => "tool_use",
        _ => "end_turn",
    }
}

fn extract_thought_text(parts: &[Value]) -> String {
    parts
        .iter()
        .filter(|part| part.get("thought").and_then(|value| value.as_bool()) == Some(true))
        .filter_map(|part| part.get("text").and_then(|value| value.as_str()))
        .collect::<String>()
}

fn extract_visible_text(parts: &[Value]) -> String {
    parts
        .iter()
        .filter(|part| part.get("thought").and_then(|value| value.as_bool()) != Some(true))
        .filter_map(|part| part.get("text").and_then(|value| value.as_str()))
        .collect::<String>()
}

fn extract_tool_calls(
    parts: &[Value],
    tool_schema_hints: Option<&AnthropicToolSchemaHints>,
) -> Vec<GeminiToolCallMeta> {
    let mut rectified_parts = parts.to_vec();
    rectify_tool_call_parts(&mut rectified_parts, tool_schema_hints);

    rectified_parts
        .iter()
        .filter_map(|part| {
            let function_call = part.get("functionCall")?;
            // Treat an explicit empty-string id as equivalent to a missing
            // one. Some Gemini relays serialize absent ids as `"id": ""`;
            // without this filter the `Some("")` value would flow into
            // `merge_tool_call_snapshots`, match itself across chunks, and
            // collapse parallel no-id calls into a single snapshot with an
            // empty-string tool_use id.
            let id = function_call
                .get("id")
                .and_then(|value| value.as_str())
                .filter(|s| !s.is_empty())
                .map(ToString::to_string);
            Some(GeminiToolCallMeta::new(
                id,
                function_call
                    .get("name")
                    .and_then(|value| value.as_str())
                    .unwrap_or(""),
                function_call
                    .get("args")
                    .cloned()
                    .unwrap_or_else(|| json!({})),
                part.get("thoughtSignature")
                    .or_else(|| part.get("thought_signature"))
                    .and_then(|value| value.as_str()),
            ))
        })
        .collect()
}

fn extract_text_thought_signature(parts: &[Value]) -> Option<String> {
    parts
        .iter()
        .filter(|part| part.get("text").is_some() && part.get("functionCall").is_none())
        .filter_map(|part| {
            part.get("thoughtSignature")
                .or_else(|| part.get("thought_signature"))
                .and_then(|value| value.as_str())
        })
        .next_back()
        .map(ToString::to_string)
}

fn merge_tool_call_snapshots(
    tool_call_snapshots: &mut Vec<GeminiToolCallMeta>,
    incoming: Vec<GeminiToolCallMeta>,
) {
    // Gemini's `streamGenerateContent?alt=sse` delivers each chunk as the
    // cumulative snapshot of `content.parts`. For the same tool call across
    // chunks we therefore need to map an incoming entry back to whichever
    // snapshot entry it describes:
    //
    // 1. If both sides carry a genuine Gemini id, match by id.
    // 2. Otherwise match by position in the cumulative `parts` array — this
    //    is how parallel no-id calls stay distinguishable.
    //
    // A previous implementation fell back to matching by `name`, which silently
    // merged two parallel calls to the same function into one entry (losing
    // the first call's args). That fallback is removed here.
    for (position, mut tool_call) in incoming.into_iter().enumerate() {
        // Treat an empty-string id as "missing" throughout this function.
        // `extract_tool_calls` already filters `""` at the source, but upstream
        // callers that build `GeminiToolCallMeta` by hand (tests, future code)
        // could still send `Some("")` — collapsing it here keeps the invariant
        // local to this merge step.
        if tool_call.id.as_deref() == Some("") {
            tool_call.id = None;
        }

        let existing_index = match tool_call.id.as_deref() {
            Some(incoming_id) => tool_call_snapshots
                .iter()
                .position(|existing| existing.id.as_deref() == Some(incoming_id))
                .or_else(|| {
                    // Fallback for the "synth -> real id upgrade" case:
                    // Gemini's cumulative stream may deliver the first chunk
                    // of a tool call without an id (we synthesize one) and
                    // then upgrade it to a genuine id on a later chunk. A
                    // pure id-match would miss the existing synthesized
                    // snapshot and push a second entry, yielding duplicate
                    // `tool_use` content blocks at stream end. If the
                    // same-position slot currently holds a synthesized id,
                    // merge into it — `or(preserved_id)` below will keep
                    // the real id, dropping the synthesized one.
                    tool_call_snapshots
                        .get(position)
                        .filter(|existing| {
                            matches!(
                                existing.id.as_deref(),
                                Some(id) if is_synthesized_tool_call_id(id)
                            )
                        })
                        .map(|_| position)
                }),
            None => tool_call_snapshots
                .get(position)
                .filter(|existing| match existing.id.as_deref() {
                    // Only merge into a positional match when the prior
                    // snapshot was itself id-less (or we synthesized one).
                    // A snapshot with a genuine Gemini id at this index is
                    // treated as a different call — the incoming entry gets
                    // its own synthesized id below.
                    Some(id) => is_synthesized_tool_call_id(id),
                    None => true,
                })
                .map(|_| position),
        };

        if let Some(index) = existing_index {
            // Preserve any synthesized id assigned on a previous chunk so the
            // Anthropic-visible id stays stable across the whole stream.
            // When incoming carries a real Gemini id and the slot holds a
            // synthesized one, `Some(real).or(Some(synth)) == Some(real)`
            // so the upgrade wins naturally.
            let preserved_id = tool_call_snapshots[index].id.clone();
            tool_call.id = tool_call.id.or(preserved_id);

            // Preserve `thought_signature` across chunks. Gemini's cumulative
            // stream may include `thoughtSignature` on one chunk and omit it
            // on a subsequent cumulative snapshot of the same part, even
            // though the signature still belongs to the call. A blind
            // overwrite would drop it, so the shadow turn we record (and
            // later replay) would be missing `thoughtSignature` and the
            // upstream would reject the follow-up for invalid signature.
            if tool_call.thought_signature.is_none() {
                tool_call
                    .thought_signature
                    .clone_from(&tool_call_snapshots[index].thought_signature);
            }
        }
        if tool_call.id.is_none() {
            tool_call.id = Some(synthesize_tool_call_id());
        }

        match existing_index {
            Some(index) => tool_call_snapshots[index] = tool_call,
            None => tool_call_snapshots.push(tool_call),
        }
    }
}

fn build_shadow_assistant_parts(
    text: Option<&str>,
    text_thought_signature: Option<&str>,
    tool_calls: &[GeminiToolCallMeta],
) -> Vec<Value> {
    let mut parts = Vec::new();

    if text.filter(|text| !text.is_empty()).is_some() || text_thought_signature.is_some() {
        let mut part = json!({
            "text": text.unwrap_or("")
        });
        if let Some(signature) = text_thought_signature {
            part["thoughtSignature"] = json!(signature);
        }
        parts.push(part);
    }

    for tool_call in tool_calls {
        let mut part = json!({
            "functionCall": {
                "id": tool_call.id.clone().unwrap_or_default(),
                "name": tool_call.name,
                "args": tool_call.args
            }
        });

        if let Some(signature) = &tool_call.thought_signature {
            part["thoughtSignature"] = json!(signature);
        }

        parts.push(part);
    }

    parts
}

fn encode_sse(event_name: &str, payload: &Value) -> Bytes {
    Bytes::from(format!(
        "event: {event_name}\ndata: {}\n\n",
        serde_json::to_string(payload).unwrap_or_default()
    ))
}

fn encode_error(error_type: &str, message: &str) -> Bytes {
    encode_sse(
        "error",
        &json!({
            "type": "error",
            "error": {
                "type": error_type,
                "message": message
            }
        }),
    )
}

/// Returns `(error_type, message)` for a Gemini error payload delivered inside
/// a 200 SSE body (`{"error": {"code": 429, "message": "...", "status": "..."}}`).
fn extract_gemini_stream_error(chunk: &Value) -> Option<(String, String)> {
    let error = chunk.get("error").filter(|value| !value.is_null())?;
    let message = error
        .as_str()
        .map(ToString::to_string)
        .or_else(|| {
            error
                .get("message")
                .and_then(|value| value.as_str())
                .map(ToString::to_string)
        })
        .unwrap_or_else(|| error.to_string());
    let status = error.get("status").and_then(|value| value.as_str());
    let code = error.get("code").and_then(|value| value.as_i64());
    let error_type = match (status, code) {
        (Some("RESOURCE_EXHAUSTED"), _) | (_, Some(429)) => "rate_limit_error",
        (Some("UNAVAILABLE"), _) | (_, Some(503)) => "overloaded_error",
        (Some("UNAUTHENTICATED"), _) | (_, Some(401)) => "authentication_error",
        (Some("PERMISSION_DENIED"), _) | (_, Some(403)) => "permission_error",
        (Some("INVALID_ARGUMENT"), _) | (_, Some(400)) => "invalid_request_error",
        _ => "api_error",
    };
    let detail = match (status, code) {
        (Some(status), Some(code)) => format!("{message} ({code} {status})"),
        (Some(status), None) => format!("{message} ({status})"),
        (None, Some(code)) => format!("{message} ({code})"),
        (None, None) => message,
    };
    Some((error_type.to_string(), detail))
}

/// How the upstream delivers visible text across stream chunks.
///
/// The Gemini API streams *incremental* text (each chunk carries only new
/// text), but some relays resend the *cumulative* text snapshot on every chunk.
/// A per-chunk "does it start with what we have" test cannot tell the two
/// apart (`"}"` then `"}\n"` is valid in both), so the mode is decided once per
/// stream.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum GeminiTextStreamMode {
    /// Decide once per stream. Text is held back until the stream proves to be
    /// incremental (a chunk that does not extend the previous one) or
    /// cumulative (two consecutive chunks that each strictly extend the
    /// previous snapshot). A stream that ends undecided is treated as
    /// incremental, the documented Gemini API behaviour.
    #[default]
    Auto,
    /// Every chunk carries new text only; always append.
    Incremental,
    /// Every chunk carries the full text so far; emit only the new suffix.
    Cumulative,
}

/// Optional per-provider override for the Gemini text chunking mode.
///
/// `ProviderMeta` has no field for this, so it is read from the optional
/// `settings_config.geminiTextStreamMode` key (`"auto"` | `"incremental"` |
/// `"cumulative"`, case-insensitive). Missing or unknown values keep `Auto`.
///
/// The streaming handler (handlers.rs, owned by the proxy-core cluster) still
/// has to pass `GeminiTextStreamMode::for_provider(&ctx.provider)` to
/// [`create_anthropic_sse_stream_from_gemini_with_text_mode`].
#[allow(dead_code)] // wired by the gemini_native streaming handler
pub const GEMINI_TEXT_STREAM_MODE_KEY: &str = "geminiTextStreamMode";

#[allow(dead_code)] // wired by the gemini_native streaming handler
impl GeminiTextStreamMode {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "auto" => Some(Self::Auto),
            "incremental" | "delta" => Some(Self::Incremental),
            "cumulative" | "snapshot" => Some(Self::Cumulative),
            _ => None,
        }
    }

    /// Resolve the mode configured on a provider (see
    /// [`GEMINI_TEXT_STREAM_MODE_KEY`]).
    pub fn for_provider(provider: &crate::provider::Provider) -> Self {
        let Some(raw) = provider
            .settings_config
            .get(GEMINI_TEXT_STREAM_MODE_KEY)
            .and_then(|value| value.as_str())
        else {
            return Self::Auto;
        };
        Self::parse(raw).unwrap_or_else(|| {
            log::warn!(
                "[Gemini] ignoring unknown {GEMINI_TEXT_STREAM_MODE_KEY} value `{raw}` on provider {}",
                provider.id
            );
            Self::Auto
        })
    }
}

/// Number of consecutive strict-prefix extensions required before `Auto`
/// commits to cumulative mode. An incremental stream would need each of
/// three consecutive chunks to start with the whole previous chunk.
const CUMULATIVE_EXTENSIONS_REQUIRED: usize = 2;

#[derive(Debug)]
struct GeminiTextAccumulator {
    mode: GeminiTextStreamMode,
    /// Full visible text emitted so far (the shadow-store text).
    emitted: String,
    /// `Auto` only: chunks held back while the mode is undecided.
    pending: Vec<String>,
    extensions: usize,
}

impl GeminiTextAccumulator {
    fn new(mode: GeminiTextStreamMode) -> Self {
        Self {
            mode,
            emitted: String::new(),
            pending: Vec::new(),
            extensions: 0,
        }
    }

    /// Feed one chunk's visible text; returns the text to emit now.
    fn push(&mut self, chunk: String) -> String {
        match self.mode {
            GeminiTextStreamMode::Incremental => {
                self.emitted.push_str(&chunk);
                chunk
            }
            GeminiTextStreamMode::Cumulative => {
                if let Some(delta) = chunk.strip_prefix(self.emitted.as_str()) {
                    let delta = delta.to_string();
                    self.emitted = chunk;
                    delta
                } else if self.emitted.starts_with(&chunk) {
                    // A shorter (or repeated) snapshot carries nothing new.
                    String::new()
                } else {
                    log::warn!(
                        "[Gemini] cumulative text snapshot diverged from emitted text; appending chunk"
                    );
                    self.emitted.push_str(&chunk);
                    chunk
                }
            }
            GeminiTextStreamMode::Auto => {
                let Some(previous) = self.pending.last() else {
                    self.pending.push(chunk);
                    return String::new();
                };
                if !chunk.starts_with(previous.as_str()) {
                    self.pending.push(chunk);
                    return self.commit_incremental();
                }
                if chunk.len() > previous.len() {
                    self.extensions += 1;
                }
                if self.extensions >= CUMULATIVE_EXTENSIONS_REQUIRED {
                    self.mode = GeminiTextStreamMode::Cumulative;
                    self.pending.clear();
                    self.emitted.clone_from(&chunk);
                    return chunk;
                }
                self.pending.push(chunk);
                String::new()
            }
        }
    }

    fn commit_incremental(&mut self) -> String {
        self.mode = GeminiTextStreamMode::Incremental;
        let text = std::mem::take(&mut self.pending).concat();
        self.emitted.push_str(&text);
        text
    }

    /// Flush text still held back at the end of the stream.
    fn finish(&mut self) -> String {
        if self.mode == GeminiTextStreamMode::Auto && !self.pending.is_empty() {
            self.commit_incremental()
        } else {
            String::new()
        }
    }

    fn text(&self) -> &str {
        &self.emitted
    }

    fn has_text(&self) -> bool {
        !self.emitted.is_empty() || self.pending.iter().any(|chunk| !chunk.is_empty())
    }
}

/// Emit a thinking delta into the (single) thinking block, opening it on
/// first use (PROV-06).
fn thinking_delta_events(
    delta: &str,
    thinking_block_index: &mut Option<u32>,
    next_content_index: &mut u32,
    open_indices: &mut HashSet<u32>,
) -> Vec<Bytes> {
    let mut events = Vec::new();
    if delta.is_empty() {
        return events;
    }
    let index = *thinking_block_index.get_or_insert_with(|| {
        let assigned = *next_content_index;
        *next_content_index += 1;
        assigned
    });
    if open_indices.insert(index) {
        events.push(encode_sse(
            "content_block_start",
            &json!({
                "type": "content_block_start",
                "index": index,
                "content_block": {
                    "type": "thinking",
                    "thinking": ""
                }
            }),
        ));
    }
    events.push(encode_sse(
        "content_block_delta",
        &json!({
            "type": "content_block_delta",
            "index": index,
            "delta": {
                "type": "thinking_delta",
                "thinking": delta
            }
        }),
    ));
    events
}

/// Flush held-back thought text and close the thinking block so the text /
/// tool blocks that follow never interleave with it.
fn close_thinking_block(
    thought_acc: &mut GeminiTextAccumulator,
    thinking_block_index: &mut Option<u32>,
    thinking_closed: &mut bool,
    next_content_index: &mut u32,
    open_indices: &mut HashSet<u32>,
) -> Vec<Bytes> {
    if *thinking_closed {
        return Vec::new();
    }
    *thinking_closed = true;
    let tail = thought_acc.finish();
    let mut events = thinking_delta_events(
        &tail,
        thinking_block_index,
        next_content_index,
        open_indices,
    );
    if let Some(index) = *thinking_block_index {
        if open_indices.remove(&index) {
            events.push(encode_sse(
                "content_block_stop",
                &json!({ "type": "content_block_stop", "index": index }),
            ));
        }
    }
    events
}

/// Emit a text delta into the (single) text block, opening it on first use.
fn text_delta_events(
    delta: &str,
    text_block_index: &mut Option<u32>,
    next_content_index: &mut u32,
    open_indices: &mut HashSet<u32>,
) -> Vec<Bytes> {
    let mut events = Vec::new();
    if delta.is_empty() {
        return events;
    }
    let index = *text_block_index.get_or_insert_with(|| {
        let assigned = *next_content_index;
        *next_content_index += 1;
        assigned
    });
    if open_indices.insert(index) {
        events.push(encode_sse(
            "content_block_start",
            &json!({
                "type": "content_block_start",
                "index": index,
                "content_block": {
                    "type": "text",
                    "text": ""
                }
            }),
        ));
    }
    events.push(encode_sse(
        "content_block_delta",
        &json!({
            "type": "content_block_delta",
            "index": index,
            "delta": {
                "type": "text_delta",
                "text": delta
            }
        }),
    ));
    events
}

pub fn create_anthropic_sse_stream_from_gemini<E: std::error::Error + Send + 'static>(
    stream: impl Stream<Item = Result<Bytes, E>> + Send + 'static,
    shadow_store: Option<Arc<GeminiShadowStore>>,
    provider_id: Option<String>,
    session_id: Option<String>,
    tool_schema_hints: Option<AnthropicToolSchemaHints>,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> + Send {
    create_anthropic_sse_stream_from_gemini_with_text_mode(
        stream,
        shadow_store,
        provider_id,
        session_id,
        tool_schema_hints,
        GeminiTextStreamMode::Auto,
    )
}

/// Like [`create_anthropic_sse_stream_from_gemini`], with an explicit
/// [`GeminiTextStreamMode`] for upstreams whose text chunking is known.
pub fn create_anthropic_sse_stream_from_gemini_with_text_mode<
    E: std::error::Error + Send + 'static,
>(
    stream: impl Stream<Item = Result<Bytes, E>> + Send + 'static,
    shadow_store: Option<Arc<GeminiShadowStore>>,
    provider_id: Option<String>,
    session_id: Option<String>,
    tool_schema_hints: Option<AnthropicToolSchemaHints>,
    text_mode: GeminiTextStreamMode,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> + Send {
    async_stream::stream! {
        let mut buffer = String::new();
        let mut utf8_remainder = Vec::new();
        let mut message_id: Option<String> = None;
        let mut current_model: Option<String> = None;
        let mut has_sent_message_start = false;
        let mut text_acc = GeminiTextAccumulator::new(text_mode);
        let mut thought_acc = GeminiTextAccumulator::new(text_mode);
        let mut thinking_block_index: Option<u32> = None;
        let mut thinking_closed = false;
        let mut text_block_index: Option<u32> = None;
        let mut next_content_index: u32 = 0;
        let mut open_indices: HashSet<u32> = HashSet::new();
        let mut tool_call_snapshots: Vec<GeminiToolCallMeta> = Vec::new();
        let mut text_thought_signature: Option<String> = None;
        let mut latest_usage: Option<Value> = None;
        let mut latest_finish_reason: Option<String> = None;
        let mut blocked_text: Option<String> = None;
        tokio::pin!(stream);

        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(bytes) => {
                    append_utf8_safe(&mut buffer, &mut utf8_remainder, &bytes);

                    while let Some(block) = take_sse_block(&mut buffer) {
                        if block.trim().is_empty() {
                            continue;
                        }

                        let mut data_lines: Vec<String> = Vec::new();
                        for line in block.lines() {
                            if let Some(data) = strip_sse_field(line, "data") {
                                data_lines.push(data.to_string());
                            }
                        }

                        if data_lines.is_empty() {
                            continue;
                        }

                        let data = data_lines.join("\n");
                        if data.trim() == "[DONE]" {
                            break;
                        }

                        let chunk_json: Value = match serde_json::from_str(&data) {
                            Ok(value) => value,
                            Err(error) => {
                                log::warn!("[Gemini] skipping unparsable SSE payload: {error}");
                                continue;
                            }
                        };

                        // An upstream error delivered inside the 200 SSE body: surface it
                        // and stop. No success terminal events and no shadow turn.
                        if let Some((error_type, message)) = extract_gemini_stream_error(&chunk_json) {
                            log::warn!("[Gemini] upstream stream error ({error_type}): {message}");
                            yield Ok(encode_error(&error_type, &message));
                            return;
                        }

                        if message_id.is_none() {
                            message_id = chunk_json
                                .get("responseId")
                                .and_then(|value| value.as_str())
                                .map(ToString::to_string);
                        }
                        if current_model.is_none() {
                            current_model = chunk_json
                                .get("modelVersion")
                                .and_then(|value| value.as_str())
                                .map(ToString::to_string);
                        }
                        if latest_usage.is_none() {
                            latest_usage = chunk_json.get("usageMetadata").cloned();
                        }

                        if !has_sent_message_start {
                            let event = json!({
                                "type": "message_start",
                                "message": {
                                    "id": super::transform::ensure_message_id(&mut message_id),
                                    "type": "message",
                                    "role": "assistant",
                                    "model": current_model.clone().unwrap_or_default(),
                                    "usage": build_anthropic_usage(chunk_json.get("usageMetadata"))
                                }
                            });
                            yield Ok(encode_sse("message_start", &event));
                            has_sent_message_start = true;
                        }

                        if let Some(reason) = chunk_json
                            .get("promptFeedback")
                            .and_then(|value| value.get("blockReason"))
                            .and_then(|value| value.as_str())
                        {
                            blocked_text = Some(format!("Request blocked by Gemini safety filters: {reason}"));
                        }

                        if let Some(candidate) = chunk_json
                            .get("candidates")
                            .and_then(|value| value.as_array())
                            .and_then(|value| value.first())
                        {
                            if let Some(reason) = candidate.get("finishReason").and_then(|value| value.as_str()) {
                                latest_finish_reason = Some(reason.to_string());
                            }
                            if let Some(usage) = chunk_json.get("usageMetadata") {
                                latest_usage = Some(usage.clone());
                            }
                            if let Some(parts) = candidate
                                .get("content")
                                .and_then(|value| value.get("parts"))
                                .and_then(|value| value.as_array())
                            {
                                let mut rectified_parts = parts.clone();
                                rectify_tool_call_parts(&mut rectified_parts, tool_schema_hints.as_ref());
                                if let Some(signature) = extract_text_thought_signature(parts) {
                                    text_thought_signature = Some(signature);
                                }
                                merge_tool_call_snapshots(
                                    &mut tool_call_snapshots,
                                    extract_tool_calls(&rectified_parts, tool_schema_hints.as_ref()),
                                );
                                let thought_text = extract_thought_text(&rectified_parts);
                                if !thought_text.is_empty() && !thinking_closed {
                                    let delta = thought_acc.push(thought_text);
                                    for event in thinking_delta_events(
                                        &delta,
                                        &mut thinking_block_index,
                                        &mut next_content_index,
                                        &mut open_indices,
                                    ) {
                                        yield Ok(event);
                                    }
                                }
                                let visible_text = extract_visible_text(&rectified_parts);
                                if !visible_text.is_empty() {
                                    for event in close_thinking_block(
                                        &mut thought_acc,
                                        &mut thinking_block_index,
                                        &mut thinking_closed,
                                        &mut next_content_index,
                                        &mut open_indices,
                                    ) {
                                        yield Ok(event);
                                    }
                                    let delta = text_acc.push(visible_text);
                                    for event in text_delta_events(
                                        &delta,
                                        &mut text_block_index,
                                        &mut next_content_index,
                                        &mut open_indices,
                                    ) {
                                        yield Ok(event);
                                    }
                                }
                            }
                        }
                    }
                }
                Err(error) => {
                    log::warn!("[Gemini] upstream stream error: {error}");
                    yield Ok(encode_error("stream_error", &format!("Stream error: {error}")));
                    return;
                }
            }
        }

        for event in close_thinking_block(
            &mut thought_acc,
            &mut thinking_block_index,
            &mut thinking_closed,
            &mut next_content_index,
            &mut open_indices,
        ) {
            yield Ok(event);
        }

        let tail = text_acc.finish();
        for event in text_delta_events(
            &tail,
            &mut text_block_index,
            &mut next_content_index,
            &mut open_indices,
        ) {
            yield Ok(event);
        }

        // Clean EOF without any finishReason: the upstream connection ended early.
        // Never present it as a normal `end_turn` completion.
        let truncated = latest_finish_reason.is_none() && blocked_text.is_none();
        if truncated && !text_acc.has_text() && tool_call_snapshots.is_empty() {
            log::warn!("[Gemini] stream ended before any content or finishReason");
            yield Ok(encode_error(
                "stream_truncated",
                "Upstream Gemini stream ended before sending any content",
            ));
            return;
        }
        if truncated {
            log::warn!("[Gemini] stream ended without finishReason; reporting max_tokens");
        }

        if !has_sent_message_start {
            let event = json!({
                "type": "message_start",
                "message": {
                    "id": super::transform::ensure_message_id(&mut message_id),
                    "type": "message",
                    "role": "assistant",
                    "model": current_model.clone().unwrap_or_default(),
                    "usage": build_anthropic_usage(latest_usage.as_ref())
                }
            });
            yield Ok(encode_sse("message_start", &event));
        }

        if text_acc.text().is_empty() {
            if let Some(blocked_text) = blocked_text.clone() {
                let index = *text_block_index.get_or_insert_with(|| {
                    let assigned = next_content_index;
                    next_content_index += 1;
                    assigned
                });

                if !open_indices.contains(&index) {
                    let start_event = json!({
                        "type": "content_block_start",
                        "index": index,
                        "content_block": {
                            "type": "text",
                            "text": ""
                        }
                    });
                    yield Ok(encode_sse("content_block_start", &start_event));
                    open_indices.insert(index);
                }

                let delta_event = json!({
                    "type": "content_block_delta",
                    "index": index,
                    "delta": {
                        "type": "text_delta",
                        "text": blocked_text
                    }
                });
                yield Ok(encode_sse("content_block_delta", &delta_event));
            }
        }

        if let Some(index) = text_block_index {
            if open_indices.remove(&index) {
                let stop_event = json!({
                    "type": "content_block_stop",
                    "index": index
                });
                yield Ok(encode_sse("content_block_stop", &stop_event));
            }
        }

        if let (Some(store), Some(provider_id), Some(session_id)) = (
            shadow_store.as_ref(),
            provider_id.as_deref(),
            session_id.as_deref(),
        ) {
            let tool_calls = tool_call_snapshots.clone();
            let shadow_text = if text_acc.text().is_empty() {
                blocked_text.as_deref()
            } else {
                Some(text_acc.text())
            };
            let shadow_parts = build_shadow_assistant_parts(
                shadow_text,
                text_thought_signature.as_deref(),
                &tool_calls,
            );
            if !shadow_parts.is_empty() {
                store.record_assistant_turn(
                    provider_id,
                    session_id,
                    json!({ "parts": shadow_parts }),
                    tool_calls.clone(),
                );
            }
        }

        // ------------------------------------------------------------------
        // Known trade-off: tool-call ordering vs. interleaved text.
        //
        // We emit all `tool_use` blocks *after* the final text
        // `content_block_stop` above. If Gemini returns parts interleaved
        // like `[text_a, functionCall_1, text_b, functionCall_2]`, the
        // Anthropic-facing stream reorders them into `[text(a+b),
        // tool_use_1, tool_use_2]`, whereas `gemini_to_anthropic_with_shadow_and_hints`
        // (non-streaming) preserves the original part order.
        //
        // This is intentional given the current design:
        //   1. Gemini `streamGenerateContent?alt=sse` delivers each chunk as
        //      a *cumulative* snapshot of `content.parts`. Emitting a
        //      `tool_use` content block on first observation would require
        //      closing the still-accumulating text block, then re-opening a
        //      new text block when more text arrives — producing many
        //      fragmented content blocks per message.
        //   2. Anthropic clients we target (claude-code and similar) consume
        //      a message's tool calls by scanning for `tool_use` blocks and
        //      do not depend on strict text ↔ tool interleaving for
        //      correctness of tool execution or result routing.
        //
        // If a future client requires strict part-order fidelity in the
        // streaming path, the fix is to track each part's original index,
        // segment the accumulated text into multiple content blocks at
        // tool-call boundaries, and flush in original order.
        // ------------------------------------------------------------------
        let tool_calls = tool_call_snapshots;
        for tool_call in &tool_calls {
            let index = next_content_index;
            next_content_index += 1;

            let start_event = json!({
                "type": "content_block_start",
                "index": index,
                "content_block": {
                    "type": "tool_use",
                    "id": tool_call.id.clone().unwrap_or_default(),
                    "name": tool_call.name
                }
            });
            yield Ok(encode_sse("content_block_start", &start_event));

            let delta_event = json!({
                "type": "content_block_delta",
                "index": index,
                "delta": {
                    "type": "input_json_delta",
                    "partial_json": serde_json::to_string(&tool_call.args).unwrap_or_else(|_| "{}".to_string())
                }
            });
            yield Ok(encode_sse("content_block_delta", &delta_event));

            let stop_event = json!({
                "type": "content_block_stop",
                "index": index
            });
            yield Ok(encode_sse("content_block_stop", &stop_event));
        }

        let stop_reason = if truncated {
            "max_tokens"
        } else {
            map_finish_reason(
                latest_finish_reason.as_deref(),
                !tool_calls.is_empty(),
                blocked_text.is_some(),
            )
        };
        let usage = build_anthropic_usage(latest_usage.as_ref());
        let message_delta = json!({
            "type": "message_delta",
            "delta": {
                "stop_reason": stop_reason,
                "stop_sequence": Value::Null
            },
            "usage": usage
        });
        yield Ok(encode_sse("message_delta", &message_delta));

        let message_stop = json!({ "type": "message_stop" });
        yield Ok(encode_sse("message_stop", &message_stop));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proxy::providers::gemini_shadow::GeminiShadowStore;
    use crate::proxy::providers::transform_gemini::anthropic_to_gemini_with_shadow;
    use std::sync::Arc;

    fn collect_stream_output(chunks: Vec<&str>) -> String {
        collect_stream_output_with_mode(chunks, GeminiTextStreamMode::Auto)
    }

    fn collect_stream_output_with_mode(chunks: Vec<&str>, mode: GeminiTextStreamMode) -> String {
        let owned_chunks: Vec<String> = chunks.into_iter().map(ToString::to_string).collect();
        let stream = futures::stream::iter(
            owned_chunks
                .into_iter()
                .map(|chunk| Ok::<Bytes, std::io::Error>(Bytes::from(chunk))),
        );
        let converted = create_anthropic_sse_stream_from_gemini_with_text_mode(
            stream, None, None, None, None, mode,
        );
        futures::executor::block_on(async move {
            converted
                .collect::<Vec<_>>()
                .await
                .into_iter()
                .map(|item| String::from_utf8(item.unwrap().to_vec()).unwrap())
                .collect::<Vec<_>>()
                .join("")
        })
    }

    fn provider_with_settings(settings: Value) -> crate::provider::Provider {
        crate::provider::Provider {
            id: "p".to_string(),
            name: "Gemini".to_string(),
            settings_config: settings,
            website_url: None,
            category: None,
            created_at: None,
            sort_index: None,
            notes: None,
            meta: None,
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        }
    }

    #[test]
    fn text_stream_mode_is_read_from_provider_settings() {
        assert_eq!(
            GeminiTextStreamMode::for_provider(&provider_with_settings(json!({}))),
            GeminiTextStreamMode::Auto
        );
        assert_eq!(
            GeminiTextStreamMode::for_provider(&provider_with_settings(
                json!({ "geminiTextStreamMode": "Cumulative" })
            )),
            GeminiTextStreamMode::Cumulative
        );
        assert_eq!(
            GeminiTextStreamMode::for_provider(&provider_with_settings(
                json!({ "geminiTextStreamMode": "incremental" })
            )),
            GeminiTextStreamMode::Incremental
        );
        assert_eq!(
            GeminiTextStreamMode::for_provider(&provider_with_settings(
                json!({ "geminiTextStreamMode": "bogus" })
            )),
            GeminiTextStreamMode::Auto
        );
    }

    #[test]
    fn thought_parts_stream_as_thinking_block_before_text() {
        let output = collect_stream_output(vec![
            "data: {\"responseId\":\"r1\",\"modelVersion\":\"gemini-2.5-pro\",\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"Let me think. \",\"thought\":true}]}}]}\n\n",
            "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"Done thinking.\",\"thought\":true}]}}]}\n\n",
            "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"Answer\"}]},\"finishReason\":\"STOP\"}],\"usageMetadata\":{\"promptTokenCount\":5,\"candidatesTokenCount\":2,\"thoughtsTokenCount\":7,\"totalTokenCount\":14}}\n\n",
        ]);
        let thinking_start = output
            .find("\"type\":\"thinking\"")
            .expect("thinking block start");
        let text_start = output.find("\"type\":\"text\"").expect("text block");
        assert!(thinking_start < text_start);
        assert!(output.contains("Let me think. "));
        assert!(output.contains("Done thinking."));
        assert!(output.contains("\"text\":\"Answer\""));
        // Thinking block (index 0) is closed before the text block (index 1) opens.
        let stop0 = output
            .find("{\"index\":0,\"type\":\"content_block_stop\"}")
            .or_else(|| output.find("\"type\":\"content_block_stop\",\"index\":0"))
            .expect("thinking block closed");
        assert!(stop0 < text_start);
        assert!(!output.contains("Let me think. Answer"));
        // thoughtsTokenCount stays in output tokens (total - prompt).
        assert!(output.contains("\"output_tokens\":9"));
    }

    fn collect_stream_output_with_shadow(
        chunks: Vec<&str>,
        store: Arc<GeminiShadowStore>,
        provider_id: &str,
        session_id: &str,
    ) -> String {
        let owned_chunks: Vec<String> = chunks.into_iter().map(ToString::to_string).collect();
        let stream = futures::stream::iter(
            owned_chunks
                .into_iter()
                .map(|chunk| Ok::<Bytes, std::io::Error>(Bytes::from(chunk))),
        );
        let converted = create_anthropic_sse_stream_from_gemini(
            stream,
            Some(store),
            Some(provider_id.to_string()),
            Some(session_id.to_string()),
            None,
        );
        futures::executor::block_on(async move {
            converted
                .collect::<Vec<_>>()
                .await
                .into_iter()
                .map(|item| String::from_utf8(item.unwrap().to_vec()).unwrap())
                .collect::<Vec<_>>()
                .join("")
        })
    }

    #[test]
    fn converts_text_stream_to_anthropic_sse() {
        // Cumulative-snapshot relay fixture: requires the explicit mode.
        let output = collect_stream_output_with_mode(vec![
            "data: {\"responseId\":\"resp_1\",\"modelVersion\":\"gemini-2.5-pro\",\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"Hel\"}]}}],\"usageMetadata\":{\"promptTokenCount\":10,\"totalTokenCount\":13}}\n\n",
            "data: {\"responseId\":\"resp_1\",\"modelVersion\":\"gemini-2.5-pro\",\"candidates\":[{\"finishReason\":\"STOP\",\"content\":{\"parts\":[{\"text\":\"Hello\"}]}}],\"usageMetadata\":{\"promptTokenCount\":10,\"totalTokenCount\":15}}\n\n",
        ], GeminiTextStreamMode::Cumulative);

        assert!(output.contains("event: message_start"));
        assert!(output.contains("\"type\":\"text_delta\""));
        assert!(output.contains("\"text\":\"Hel\""));
        assert!(output.contains("\"text\":\"lo\""));
        assert!(output.contains("\"stop_reason\":\"end_turn\""));
        assert!(output.contains("event: message_stop"));
    }

    #[test]
    fn converts_function_call_stream_to_tool_use_events() {
        let output = collect_stream_output(vec![
            "data: {\"responseId\":\"resp_2\",\"modelVersion\":\"gemini-2.5-pro\",\"candidates\":[{\"finishReason\":\"STOP\",\"content\":{\"parts\":[{\"functionCall\":{\"id\":\"call_1\",\"name\":\"get_weather\",\"args\":{\"city\":\"Tokyo\"}},\"thoughtSignature\":\"sig-1\"}]}}],\"usageMetadata\":{\"promptTokenCount\":5,\"totalTokenCount\":8}}\n\n",
        ]);

        assert!(output.contains("\"type\":\"tool_use\""));
        assert!(output.contains("\"name\":\"get_weather\""));
        assert!(output.contains("\"type\":\"input_json_delta\""));
        assert!(output.contains("\"stop_reason\":\"tool_use\""));
    }

    #[test]
    fn converts_crlf_delimited_stream_to_anthropic_sse() {
        // Cumulative-snapshot relay fixture: requires the explicit mode.
        let output = collect_stream_output_with_mode(vec![
            "data: {\"responseId\":\"resp_3\",\"modelVersion\":\"gemini-2.5-pro\",\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"Hi\"}]}}],\"usageMetadata\":{\"promptTokenCount\":4,\"totalTokenCount\":6}}\r\n\r\n",
            "data: {\"responseId\":\"resp_3\",\"modelVersion\":\"gemini-2.5-pro\",\"candidates\":[{\"finishReason\":\"STOP\",\"content\":{\"parts\":[{\"text\":\"Hi there\"}]}}],\"usageMetadata\":{\"promptTokenCount\":4,\"totalTokenCount\":9}}\r\n\r\n",
        ], GeminiTextStreamMode::Cumulative);

        assert!(output.contains("event: message_start"));
        assert!(output.contains("\"type\":\"text_delta\""));
        assert!(output.contains("\"text\":\"Hi\""));
        assert!(output.contains("\"text\":\" there\""));
        assert!(output.contains("event: message_stop"));
    }

    #[test]
    fn preserves_utf8_boundaries_when_json_payload_spans_chunks() {
        let payload = json!({
            "responseId": "resp_utf8",
            "modelVersion": "gemini-2.5-pro",
            "candidates": [{
                "finishReason": "STOP",
                "content": {
                    "parts": [{ "text": "你好，Gemini" }]
                }
            }],
            "usageMetadata": {
                "promptTokenCount": 4,
                "totalTokenCount": 8
            }
        });
        let chunk = format!("data: {}\n\n", serde_json::to_string(&payload).unwrap());
        let split_at = chunk.find("你好").unwrap() + 1;
        let chunk_bytes = chunk.into_bytes();
        let stream = futures::stream::iter([
            Ok::<Bytes, std::io::Error>(Bytes::from(chunk_bytes[..split_at].to_vec())),
            Ok::<Bytes, std::io::Error>(Bytes::from(chunk_bytes[split_at..].to_vec())),
        ]);
        let converted = create_anthropic_sse_stream_from_gemini(stream, None, None, None, None);
        let output = futures::executor::block_on(async move {
            converted
                .collect::<Vec<_>>()
                .await
                .into_iter()
                .map(|item| String::from_utf8(item.unwrap().to_vec()).unwrap())
                .collect::<Vec<_>>()
                .join("")
        });

        assert!(output.contains("你好，Gemini"));
        assert!(!output.contains('\u{fffd}'));
    }

    #[test]
    fn stores_full_text_for_shadow_replay_across_delta_chunks() {
        let store = Arc::new(GeminiShadowStore::with_limits(8, 4));
        let output = collect_stream_output_with_shadow(
            vec![
                "data: {\"responseId\":\"resp_4\",\"modelVersion\":\"gemini-2.5-pro\",\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"Hel\"}]}}],\"usageMetadata\":{\"promptTokenCount\":4,\"totalTokenCount\":6}}\n\n",
                "data: {\"responseId\":\"resp_4\",\"modelVersion\":\"gemini-2.5-pro\",\"candidates\":[{\"finishReason\":\"STOP\",\"content\":{\"parts\":[{\"text\":\"lo\"},{\"text\":\"\",\"thoughtSignature\":\"sig-1\"}]}}],\"usageMetadata\":{\"promptTokenCount\":4,\"totalTokenCount\":8}}\n\n",
            ],
            store.clone(),
            "provider-a",
            "session-1",
        );

        // Auto mode holds the first chunk until the stream proves incremental,
        // so both deltas may be flushed together; the text must be exact.
        assert_eq!(streamed_text(&output), "Hello");

        let shadow = store
            .latest_assistant_content("provider-a", "session-1")
            .unwrap();
        assert_eq!(shadow["parts"][0]["text"], "Hello");
        assert_eq!(shadow["parts"][0]["thoughtSignature"], "sig-1");

        let second_turn = anthropic_to_gemini_with_shadow(
            json!({
                "messages": [
                    { "role": "user", "content": "Hi" },
                    { "role": "assistant", "content": [{ "type": "text", "text": "Hello" }] },
                    { "role": "user", "content": "Continue" }
                ]
            }),
            Some(store.as_ref()),
            Some("provider-a"),
            Some("session-1"),
        )
        .unwrap();

        assert_eq!(second_turn["contents"][1]["role"], "model");
        assert_eq!(second_turn["contents"][1]["parts"][0]["text"], "Hello");
        assert_eq!(
            second_turn["contents"][1]["parts"][0]["thoughtSignature"],
            "sig-1"
        );
    }

    #[test]
    fn stores_tool_shadow_before_tool_use_events_are_fully_drained() {
        let store = Arc::new(GeminiShadowStore::with_limits(8, 4));
        let chunks = vec![
            "data: {\"responseId\":\"resp_tool_shadow\",\"modelVersion\":\"gemini-2.5-pro\",\"candidates\":[{\"finishReason\":\"STOP\",\"content\":{\"parts\":[{\"functionCall\":{\"id\":\"call_1\",\"name\":\"Bash\",\"args\":{\"command\":\"ls -R\"}},\"thoughtSignature\":\"sig-tool-1\"}]}}],\"usageMetadata\":{\"promptTokenCount\":5,\"totalTokenCount\":8}}\n\n".to_string(),
        ];
        let stream = futures::stream::iter(
            chunks
                .into_iter()
                .map(|chunk| Ok::<Bytes, std::io::Error>(Bytes::from(chunk))),
        );
        let mut converted = Box::pin(create_anthropic_sse_stream_from_gemini(
            stream,
            Some(store.clone()),
            Some("provider-a".to_string()),
            Some("session-1".to_string()),
            None,
        ));

        futures::executor::block_on(async {
            while let Some(item) = converted.next().await {
                let event = String::from_utf8(item.unwrap().to_vec()).unwrap();
                if event.contains("\"type\":\"tool_use\"") {
                    break;
                }
            }
        });

        let shadow = store
            .latest_assistant_content("provider-a", "session-1")
            .unwrap();
        assert_eq!(shadow["parts"][0]["functionCall"]["name"], "Bash");
        assert_eq!(shadow["parts"][0]["thoughtSignature"], "sig-tool-1");
    }

    #[test]
    fn rectifies_streamed_tool_call_args_from_tool_schema_hints() {
        let owned_chunks = vec![
            "data: {\"responseId\":\"resp_5\",\"modelVersion\":\"gemini-2.5-pro\",\"candidates\":[{\"finishReason\":\"STOP\",\"content\":{\"parts\":[{\"functionCall\":{\"id\":\"call_1\",\"name\":\"Bash\",\"args\":{\"args\":\"git status\"}}}]}}],\"usageMetadata\":{\"promptTokenCount\":5,\"totalTokenCount\":8}}\n\n".to_string(),
        ];
        let stream = futures::stream::iter(
            owned_chunks
                .into_iter()
                .map(|chunk| Ok::<Bytes, std::io::Error>(Bytes::from(chunk))),
        );
        let hints = super::super::transform_gemini::extract_anthropic_tool_schema_hints(&json!({
            "tools": [{
                "name": "Bash",
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "command": { "type": "string" },
                        "timeout": { "type": "number" }
                    },
                    "required": ["command"]
                }
            }]
        }));
        let converted =
            create_anthropic_sse_stream_from_gemini(stream, None, None, None, Some(hints));
        let output = futures::executor::block_on(async move {
            converted
                .collect::<Vec<_>>()
                .await
                .into_iter()
                .map(|item| String::from_utf8(item.unwrap().to_vec()).unwrap())
                .collect::<Vec<_>>()
                .join("")
        });

        assert!(output.contains("\"partial_json\":\"{\\\"command\\\":\\\"git status\\\"}\""));
    }

    #[test]
    fn rectifies_streamed_skill_args_from_nested_parameters() {
        let payload = json!({
            "responseId": "resp_6",
            "modelVersion": "gemini-2.5-pro",
            "candidates": [{
                "finishReason": "STOP",
                "content": {
                    "parts": [{
                        "functionCall": {
                            "id": "call_1",
                            "name": "Skill",
                            "args": {
                                "name": "git-commit",
                                "parameters": {
                                    "args": ["详细分析内容 编写提交信息 分多次提交代码"]
                                }
                            }
                        }
                    }]
                }
            }],
            "usageMetadata": {
                "promptTokenCount": 5,
                "totalTokenCount": 8
            }
        });
        let owned_chunks = vec![format!(
            "data: {}\n\n",
            serde_json::to_string(&payload).unwrap()
        )];
        let stream = futures::stream::iter(
            owned_chunks
                .into_iter()
                .map(|chunk| Ok::<Bytes, std::io::Error>(Bytes::from(chunk))),
        );
        let hints = super::super::transform_gemini::extract_anthropic_tool_schema_hints(&json!({
            "tools": [{
                "name": "Skill",
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "skill": { "type": "string" },
                        "args": { "type": "string" }
                    },
                    "required": ["skill"]
                }
            }]
        }));
        let converted =
            create_anthropic_sse_stream_from_gemini(stream, None, None, None, Some(hints));
        let output = futures::executor::block_on(async move {
            converted
                .collect::<Vec<_>>()
                .await
                .into_iter()
                .map(|item| String::from_utf8(item.unwrap().to_vec()).unwrap())
                .collect::<Vec<_>>()
                .join("")
        });

        assert!(output.contains("git-commit"));
        assert!(output.contains("详细分析内容 编写提交信息 分多次提交代码"));
        assert!(!output.contains("\\\"parameters\\\""));
    }

    /// Regression for the P1 finding: when Gemini emits two parallel calls to
    /// the same function without providing ids, both must be surfaced to the
    /// Anthropic client with distinct synthesized ids. The previous
    /// name-based fallback in `merge_tool_call_snapshots` collapsed them into
    /// a single entry, causing silent data loss for the first call.
    #[test]
    fn parallel_same_name_no_id_calls_preserve_both() {
        let output = collect_stream_output(vec![
            "data: {\"responseId\":\"r1\",\"modelVersion\":\"gemini-2.5-pro\",\"candidates\":[{\"finishReason\":\"STOP\",\"content\":{\"parts\":[{\"functionCall\":{\"name\":\"get_weather\",\"args\":{\"city\":\"Tokyo\"}}},{\"functionCall\":{\"name\":\"get_weather\",\"args\":{\"city\":\"Osaka\"}}}]}}],\"usageMetadata\":{\"promptTokenCount\":5,\"totalTokenCount\":8}}\n\n",
        ]);

        let tool_use_start_count = output.matches("\"type\":\"tool_use\"").count();
        assert_eq!(
            tool_use_start_count, 2,
            "both parallel calls must survive merge_tool_call_snapshots"
        );
        // `input_json_delta.partial_json` is a string, so the city keys appear
        // JSON-escaped inside the outer SSE `data:` payload. Match against
        // the raw escape sequences rather than the canonical JSON form.
        assert!(output.contains("Tokyo"));
        assert!(output.contains("Osaka"));
        // Each tool_use must carry a non-empty synthesized id so Claude Code
        // can disambiguate the two tool_result round-trips.
        let synth_count = output.matches("\"id\":\"gemini_synth_").count();
        assert_eq!(synth_count, 2);
    }

    /// When Gemini keeps sending the same no-id functionCall across cumulative
    /// chunks, the synthesized id must stay stable so the Anthropic client
    /// sees a single tool_use block with consistent args updates rather than
    /// duplicates.
    #[test]
    fn no_id_tool_call_reuses_synthesized_id_across_cumulative_chunks() {
        let output = collect_stream_output(vec![
            "data: {\"responseId\":\"r2\",\"modelVersion\":\"gemini-2.5-pro\",\"candidates\":[{\"content\":{\"parts\":[{\"functionCall\":{\"name\":\"get_weather\",\"args\":{\"city\":\"Tokyo\"}}}]}}],\"usageMetadata\":{\"promptTokenCount\":4,\"totalTokenCount\":6}}\n\n",
            "data: {\"responseId\":\"r2\",\"modelVersion\":\"gemini-2.5-pro\",\"candidates\":[{\"finishReason\":\"STOP\",\"content\":{\"parts\":[{\"functionCall\":{\"name\":\"get_weather\",\"args\":{\"city\":\"Tokyo\",\"units\":\"c\"}}}]}}],\"usageMetadata\":{\"promptTokenCount\":4,\"totalTokenCount\":9}}\n\n",
        ]);

        assert_eq!(output.matches("\"type\":\"tool_use\"").count(), 1);
        assert!(output.contains("\"units\\\":\\\"c\\\""));
    }

    /// Regression for the follow-up Codex P1: some Gemini relays serialize
    /// an absent functionCall id as `"id": ""` rather than omitting the
    /// field. Without a filter, `Some("")` would reach
    /// `merge_tool_call_snapshots`, two parallel no-id calls would match
    /// each other on the empty-string id, and the second would overwrite
    /// the first — silently losing a call. Also the emitted Anthropic
    /// `tool_use.id` would be the empty string, so tool_result
    /// correlation from the Claude client would break.
    #[test]
    fn parallel_empty_string_id_calls_are_treated_as_missing_and_preserved() {
        let output = collect_stream_output(vec![
            "data: {\"responseId\":\"r3\",\"modelVersion\":\"gemini-2.5-pro\",\"candidates\":[{\"finishReason\":\"STOP\",\"content\":{\"parts\":[{\"functionCall\":{\"id\":\"\",\"name\":\"get_weather\",\"args\":{\"city\":\"Tokyo\"}}},{\"functionCall\":{\"id\":\"\",\"name\":\"get_weather\",\"args\":{\"city\":\"Osaka\"}}}]}}],\"usageMetadata\":{\"promptTokenCount\":5,\"totalTokenCount\":8}}\n\n",
        ]);

        let tool_use_count = output.matches("\"type\":\"tool_use\"").count();
        assert_eq!(
            tool_use_count, 2,
            "both parallel calls must survive even when ids are explicit empty strings"
        );
        assert!(output.contains("Tokyo"));
        assert!(output.contains("Osaka"));
        // No tool_use may emit an empty id — each must get its own
        // synthesized id so tool_result correlation works.
        assert!(
            !output.contains("\"id\":\"\""),
            "empty tool_use id leaked through: {output}"
        );
        let synth_count = output.matches("\"id\":\"gemini_synth_").count();
        assert_eq!(synth_count, 2);
    }

    /// Companion regression: a single-chunk stream whose sole functionCall
    /// carries `"id": ""` must still emit exactly one tool_use with a
    /// synthesized id, not an empty one. This covers the non-parallel
    /// degraded-relay case that the parallel test above subsumes.
    #[test]
    fn single_empty_string_id_tool_call_gets_synthesized_id() {
        let output = collect_stream_output(vec![
            "data: {\"responseId\":\"r4\",\"modelVersion\":\"gemini-2.5-pro\",\"candidates\":[{\"finishReason\":\"STOP\",\"content\":{\"parts\":[{\"functionCall\":{\"id\":\"\",\"name\":\"get_weather\",\"args\":{\"city\":\"Tokyo\"}}}]}}],\"usageMetadata\":{\"promptTokenCount\":3,\"totalTokenCount\":5}}\n\n",
        ]);

        assert_eq!(output.matches("\"type\":\"tool_use\"").count(), 1);
        assert!(!output.contains("\"id\":\"\""));
        assert_eq!(output.matches("\"id\":\"gemini_synth_").count(), 1);
    }

    /// Regression for Codex P1: Gemini's cumulative stream may deliver a
    /// `functionCall` without an id (we synthesize one) and then upgrade
    /// to a genuine id on a later chunk. Without a positional fallback in
    /// the `Some(incoming_id)` branch of `merge_tool_call_snapshots`, the
    /// real id would fail to match the existing synthesized snapshot and
    /// push a second entry — yielding duplicate `tool_use` blocks at
    /// stream end (one synthesized, one real) and breaking tool_result
    /// correlation.
    #[test]
    fn upgraded_real_id_merges_into_existing_synthesized_snapshot() {
        let output = collect_stream_output(vec![
            // Chunk 1: no id -> a `gemini_synth_*` id is assigned.
            "data: {\"responseId\":\"rupg\",\"modelVersion\":\"gemini-2.5-pro\",\"candidates\":[{\"content\":{\"parts\":[{\"functionCall\":{\"name\":\"get_weather\",\"args\":{\"city\":\"Tokyo\"}}}]}}],\"usageMetadata\":{\"promptTokenCount\":4,\"totalTokenCount\":6}}\n\n",
            // Chunk 2: cumulative snapshot upgrades the same call to a
            // real Gemini id. Must merge into the existing slot, not
            // spawn a second snapshot.
            "data: {\"responseId\":\"rupg\",\"modelVersion\":\"gemini-2.5-pro\",\"candidates\":[{\"finishReason\":\"STOP\",\"content\":{\"parts\":[{\"functionCall\":{\"id\":\"real_id_abc\",\"name\":\"get_weather\",\"args\":{\"city\":\"Tokyo\",\"units\":\"c\"}}}]}}],\"usageMetadata\":{\"promptTokenCount\":4,\"totalTokenCount\":9}}\n\n",
        ]);

        // Exactly one tool_use block (not two).
        assert_eq!(
            output.matches("\"type\":\"tool_use\"").count(),
            1,
            "id upgrade must merge into the synthesized snapshot, not duplicate it: {output}"
        );
        // The emitted tool_use id is the real Gemini id, not the synthesized one.
        assert!(
            output.contains("\"id\":\"real_id_abc\""),
            "expected real id to win after upgrade: {output}"
        );
        assert!(
            !output.contains("\"id\":\"gemini_synth_"),
            "synthesized id must be dropped when a real id arrives: {output}"
        );
        // Args from the final cumulative snapshot are emitted.
        assert!(output.contains("units"));
    }

    /// Regression for Codex P2: Gemini's cumulative stream may include
    /// `thoughtSignature` on one chunk and omit it on a later cumulative
    /// snapshot of the same call. A blind `tool_call_snapshots[index] =
    /// tool_call` overwrite would drop the signature, so the shadow turn
    /// recorded (and later replayed to Gemini) would miss
    /// `thoughtSignature` and the upstream would reject the follow-up.
    /// `merge_tool_call_snapshots` must retain the prior signature when
    /// the incoming chunk does not carry one.
    #[test]
    fn thought_signature_preserved_when_later_chunk_omits_it() {
        let store = Arc::new(GeminiShadowStore::with_limits(8, 4));
        collect_stream_output_with_shadow(
            vec![
                // Chunk 1: carries thoughtSignature "sig-keep".
                "data: {\"responseId\":\"rsig\",\"modelVersion\":\"gemini-2.5-pro\",\"candidates\":[{\"content\":{\"parts\":[{\"functionCall\":{\"id\":\"call_1\",\"name\":\"get_weather\",\"args\":{\"city\":\"Tokyo\"}},\"thoughtSignature\":\"sig-keep\"}]}}],\"usageMetadata\":{\"promptTokenCount\":4,\"totalTokenCount\":6}}\n\n",
                // Chunk 2: cumulative update for the same call, but
                // thoughtSignature is omitted — common for Gemini's
                // one-shot signature fields.
                "data: {\"responseId\":\"rsig\",\"modelVersion\":\"gemini-2.5-pro\",\"candidates\":[{\"finishReason\":\"STOP\",\"content\":{\"parts\":[{\"functionCall\":{\"id\":\"call_1\",\"name\":\"get_weather\",\"args\":{\"city\":\"Tokyo\",\"units\":\"c\"}}}]}}],\"usageMetadata\":{\"promptTokenCount\":4,\"totalTokenCount\":9}}\n\n",
            ],
            store.clone(),
            "provider-sig",
            "session-sig",
        );

        let shadow = store
            .latest_assistant_content("provider-sig", "session-sig")
            .expect("shadow turn must be recorded");
        assert_eq!(shadow["parts"][0]["functionCall"]["id"], "call_1");
        assert_eq!(
            shadow["parts"][0]["thoughtSignature"], "sig-keep",
            "prior thoughtSignature must survive a later chunk that omits it: {shadow}"
        );
    }

    fn text_chunk(text: &str) -> String {
        format!(
            "data: {}\n\n",
            json!({
                "responseId": "resp_text",
                "modelVersion": "gemini-2.5-pro",
                "candidates": [{"content": {"parts": [{"text": text}]}}]
            })
        )
    }

    fn finish_chunk() -> String {
        format!(
            "data: {}\n\n",
            json!({
                "responseId": "resp_text",
                "candidates": [{"finishReason": "STOP", "content": {"parts": []}}],
                "usageMetadata": {"promptTokenCount": 3, "totalTokenCount": 5}
            })
        )
    }

    fn sse_payloads(output: &str) -> Vec<Value> {
        output
            .split("\n\n")
            .filter_map(|block| {
                let data = block
                    .lines()
                    .find_map(|line| strip_sse_field(line, "data"))?;
                serde_json::from_str::<Value>(data).ok()
            })
            .collect()
    }

    fn streamed_text(output: &str) -> String {
        sse_payloads(output)
            .iter()
            .filter(|event| {
                event.pointer("/delta/type").and_then(|v| v.as_str()) == Some("text_delta")
            })
            .filter_map(|event| event.pointer("/delta/text").and_then(|v| v.as_str()))
            .collect()
    }

    fn text_of(texts: &[&str], mode: GeminiTextStreamMode) -> String {
        let mut chunks: Vec<String> = texts.iter().map(|text| text_chunk(text)).collect();
        chunks.push(finish_chunk());
        let refs: Vec<&str> = chunks.iter().map(String::as_str).collect();
        streamed_text(&collect_stream_output_with_mode(refs, mode))
    }

    // PROV-03
    #[test]
    fn repeated_delta_chunks_are_not_dropped() {
        assert_eq!(text_of(&["a", "a"], GeminiTextStreamMode::Auto), "aa");
        assert_eq!(
            text_of(&["\n\n", "\n\n"], GeminiTextStreamMode::Auto),
            "\n\n\n\n"
        );
    }

    #[test]
    fn delta_chunk_extending_previous_chunk_is_not_truncated() {
        assert_eq!(text_of(&["}", "}\n"], GeminiTextStreamMode::Auto), "}}\n");
    }

    #[test]
    fn incremental_stream_is_appended() {
        assert_eq!(
            text_of(&["Hel", "lo", " world"], GeminiTextStreamMode::Auto),
            "Hello world"
        );
        assert_eq!(
            text_of(&["Hel", "Hello"], GeminiTextStreamMode::Incremental),
            "HelHello"
        );
    }

    #[test]
    fn cumulative_mode_only_when_requested_for_ambiguous_streams() {
        assert_eq!(
            text_of(&["Hel", "Hello"], GeminiTextStreamMode::Auto),
            "HelHello"
        );
        assert_eq!(
            text_of(&["Hel", "Hello"], GeminiTextStreamMode::Cumulative),
            "Hello"
        );
    }

    #[test]
    fn auto_mode_detects_unambiguous_cumulative_snapshots() {
        assert_eq!(
            text_of(
                &["Hel", "Hello", "Hello wor", "Hello world", "Hello world"],
                GeminiTextStreamMode::Auto
            ),
            "Hello world"
        );
    }

    #[test]
    fn auto_mode_shadow_text_matches_streamed_text() {
        let store = Arc::new(GeminiShadowStore::with_limits(8, 4));
        let chunks = [text_chunk("}"), text_chunk("}\n"), finish_chunk()];
        let refs: Vec<&str> = chunks.iter().map(String::as_str).collect();
        let output = collect_stream_output_with_shadow(refs, store.clone(), "p", "s");
        assert_eq!(streamed_text(&output), "}}\n");
        let shadow = store.latest_assistant_content("p", "s").unwrap();
        assert_eq!(shadow["parts"][0]["text"], "}}\n");
    }

    // PROV-34
    #[test]
    fn upstream_error_payload_emits_error_without_message_stop_or_shadow() {
        let store = Arc::new(GeminiShadowStore::with_limits(8, 4));
        let output = collect_stream_output_with_shadow(
            vec!["data: {\"error\":{\"code\":429,\"message\":\"quota exhausted\",\"status\":\"RESOURCE_EXHAUSTED\"}}\n\n"],
            store.clone(),
            "p",
            "s",
        );
        let events = sse_payloads(&output);
        let error = events
            .iter()
            .find(|event| event["type"] == "error")
            .expect("error event");
        assert!(error["error"]["message"]
            .as_str()
            .unwrap()
            .contains("quota"));
        assert_eq!(error["error"]["type"], "rate_limit_error");
        assert!(!output.contains("message_stop"));
        assert!(!output.contains("message_delta"));
        assert!(store.latest_assistant_content("p", "s").is_none());
    }

    #[test]
    fn mid_stream_error_payload_stops_without_success_events() {
        let first = text_chunk("partial");
        let output = collect_stream_output(vec![
            first.as_str(),
            "data: {\"error\":{\"code\":503,\"message\":\"overloaded\",\"status\":\"UNAVAILABLE\"}}\n\n",
        ]);
        assert!(output.contains("event: error"));
        assert!(output.contains("overloaded"));
        assert!(!output.contains("message_stop"));
    }

    #[test]
    fn partial_text_without_finish_reason_reports_max_tokens() {
        let first = text_chunk("Hel");
        let second = text_chunk("lo");
        let output = collect_stream_output(vec![first.as_str(), second.as_str()]);
        assert_eq!(streamed_text(&output), "Hello");
        assert!(output.contains("\"stop_reason\":\"max_tokens\""));
        assert!(!output.contains("\"stop_reason\":\"end_turn\""));
        assert!(output.contains("event: message_stop"));
    }

    #[test]
    fn stream_without_content_or_finish_reason_reports_truncation() {
        let output = collect_stream_output(vec![
            "data: {\"responseId\":\"r\",\"usageMetadata\":{\"promptTokenCount\":3}}\n\n",
        ]);
        assert!(output.contains("event: error"));
        assert!(output.contains("stream_truncated"));
        assert!(!output.contains("message_stop"));
    }

    #[test]
    fn upstream_transport_error_becomes_error_event() {
        let first = text_chunk("Hel");
        let stream = futures::stream::iter(vec![
            Ok::<Bytes, std::io::Error>(Bytes::from(first)),
            Err(std::io::Error::other("connection reset")),
        ]);
        let converted = create_anthropic_sse_stream_from_gemini(stream, None, None, None, None);
        let items = futures::executor::block_on(converted.collect::<Vec<_>>());
        assert!(items.iter().all(Result::is_ok));
        let output = items
            .into_iter()
            .map(|item| String::from_utf8(item.unwrap().to_vec()).unwrap())
            .collect::<String>();
        assert!(output.contains("event: error"));
        assert!(output.contains("connection reset"));
        assert!(!output.contains("message_stop"));
    }
}
