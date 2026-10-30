//! 流式响应转换模块
//!
//! 实现 OpenAI SSE → Anthropic SSE 格式转换

use super::chat_tool_call_slots::ChatToolCallSlots;
use crate::proxy::sse::{strip_sse_field, take_sse_block};
use bytes::Bytes;
use futures::stream::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

/// Deserialize a field that may be explicitly `null` as its `Default` value.
fn null_as_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

/// OpenAI 流式响应数据结构
#[derive(Debug, Deserialize)]
struct OpenAIStreamChunk {
    #[serde(default, deserialize_with = "null_as_default")]
    id: String,
    #[serde(default, deserialize_with = "null_as_default")]
    model: String,
    #[serde(default, deserialize_with = "null_as_default")]
    choices: Vec<StreamChoice>,
    #[serde(default)]
    usage: Option<Usage>,
}

#[derive(Debug, Deserialize)]
struct StreamChoice {
    // Some gateways omit `delta` (or send `null`) on the final finish_reason chunk.
    #[serde(default, deserialize_with = "null_as_default")]
    delta: Delta,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct Delta {
    #[serde(default)]
    content: Option<String>,
    // OpenRouter/Kimi/其它 使用 reasoning，DeepSeek 使用 reasoning_content
    #[serde(default, alias = "reasoning_content")]
    reasoning: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<DeltaToolCall>>,
}

#[derive(Debug, Deserialize, Serialize)]
struct DeltaToolCall {
    /// Optional: some OpenAI-compatible gateways omit it; see `ChatToolCallSlots`.
    #[serde(default)]
    index: Option<usize>,
    #[serde(default)]
    id: Option<String>,
    #[serde(rename = "type", default)]
    call_type: Option<String>,
    #[serde(default)]
    function: Option<DeltaFunction>,
}

#[derive(Debug, Deserialize, Serialize)]
struct DeltaFunction {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}

/// OpenAI 流式响应的 usage 信息（完整版）
#[derive(Debug, Deserialize)]
struct Usage {
    #[serde(default, deserialize_with = "null_as_default")]
    prompt_tokens: u32,
    #[serde(default, deserialize_with = "null_as_default")]
    completion_tokens: u32,
    #[serde(default)]
    prompt_tokens_details: Option<PromptTokensDetails>,
    /// Some compatible servers return Anthropic-style cache fields directly
    #[serde(default)]
    cache_read_input_tokens: Option<u32>,
    #[serde(default)]
    cache_creation_input_tokens: Option<u32>,
}

/// Nested token details from OpenAI format
#[derive(Debug, Deserialize)]
struct PromptTokensDetails {
    #[serde(default, deserialize_with = "null_as_default")]
    cached_tokens: u32,
    #[serde(default, deserialize_with = "null_as_default")]
    cache_write_tokens: u32,
}

#[derive(Debug, Clone)]
struct ToolBlockState {
    anthropic_index: u32,
    id: String,
    name: String,
    started: bool,
    pending_args: String,
    /// 连续空白字符计数 — 用于检测 Copilot 无限换行 bug
    /// 当 function call 参数中的连续空白字符达到阈值时，强制终止流
    consecutive_whitespace: usize,
    /// 是否已因无限空白 bug 被中止
    aborted: bool,
}

/// 无限空白 bug 的连续空白字符阈值
const INFINITE_WHITESPACE_THRESHOLD: usize = 500;

fn build_anthropic_usage_json(usage: &Usage) -> Value {
    // OpenAI prompt_tokens 含缓存，Anthropic input_tokens 不含，需减去 cache_read 与 cache_creation
    // （三桶互斥，恒等 input + cache_read + cache_creation == prompt_tokens）。
    let cached = extract_cache_read_tokens(usage).unwrap_or(0);
    let cache_creation = extract_cache_write_tokens(usage).unwrap_or(0);
    let input_tokens = usage
        .prompt_tokens
        .saturating_sub(cached)
        .saturating_sub(cache_creation);
    let mut usage_json = json!({
        "input_tokens": input_tokens,
        "output_tokens": usage.completion_tokens
    });
    if cached > 0 {
        usage_json["cache_read_input_tokens"] = json!(cached);
    }
    if cache_creation > 0 {
        usage_json["cache_creation_input_tokens"] = json!(cache_creation);
    }
    usage_json
}

fn default_anthropic_usage_json() -> Value {
    json!({
        "input_tokens": 0,
        "output_tokens": 0
    })
}

fn build_message_delta_event(stop_reason: Option<String>, usage_json: Option<Value>) -> Value {
    let usage = usage_json
        .filter(|usage| usage.is_object())
        .unwrap_or_else(default_anthropic_usage_json);

    json!({
        "type": "message_delta",
        "delta": {
            "stop_reason": stop_reason,
            "stop_sequence": null
        },
        "usage": usage
    })
}

fn sse_event_bytes(event_name: &str, payload: &Value) -> Bytes {
    Bytes::from(format!(
        "event: {event_name}\ndata: {}\n\n",
        serde_json::to_string(payload).unwrap_or_default()
    ))
}

fn anthropic_error_event(error_type: &str, message: &str) -> Bytes {
    sse_event_bytes(
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

/// Returns `(message, error_type)` when an upstream Chat SSE payload is an error
/// (`event: error`, or a JSON body carrying a non-null `error`).
fn upstream_chat_error(event_name: Option<&str>, payload: &Value) -> Option<(String, String)> {
    let error = payload.get("error").filter(|value| !value.is_null());
    if event_name != Some("error") && error.is_none() {
        return None;
    }
    let error = error.unwrap_or(payload);
    let message = error
        .as_str()
        .map(ToString::to_string)
        .or_else(|| {
            error
                .get("message")
                .or_else(|| error.get("detail"))
                .and_then(|v| v.as_str())
                .map(ToString::to_string)
        })
        .unwrap_or_else(|| error.to_string());
    let error_type = error
        .get("type")
        .or_else(|| error.get("code"))
        .or_else(|| error.get("status"))
        .and_then(|v| v.as_str())
        .filter(|v| !v.is_empty())
        .unwrap_or("api_error")
        .to_string();
    Some((message, error_type))
}

/// Close every open content block (the current text/thinking block and all
/// tool_use blocks), first late-starting tool calls that accumulated a payload
/// before their id/name arrived. Returns the SSE events plus whether any
/// tool_use block was part of the message.
fn close_open_blocks(
    current_non_tool_block_index: &mut Option<u32>,
    current_non_tool_block_type: &mut Option<&'static str>,
    tool_blocks_by_index: &mut HashMap<usize, ToolBlockState>,
    open_tool_block_indices: &mut HashSet<u32>,
) -> (Vec<Bytes>, bool) {
    let mut events = Vec::new();

    if let Some(index) = current_non_tool_block_index.take() {
        events.push(sse_event_bytes(
            "content_block_stop",
            &json!({"type": "content_block_stop", "index": index}),
        ));
    }
    *current_non_tool_block_type = None;

    // Late start for blocks that accumulated args before id/name arrived.
    let mut late_tool_starts: Vec<(u32, String, String, String)> = Vec::new();
    for (tool_idx, state) in tool_blocks_by_index.iter_mut() {
        if state.started {
            continue;
        }
        let has_payload =
            !state.pending_args.is_empty() || !state.id.is_empty() || !state.name.is_empty();
        if !has_payload {
            continue;
        }
        let fallback_id = if state.id.is_empty() {
            format!("tool_call_{tool_idx}")
        } else {
            state.id.clone()
        };
        let fallback_name = if state.name.is_empty() {
            "unknown_tool".to_string()
        } else {
            state.name.clone()
        };
        state.started = true;
        let pending = std::mem::take(&mut state.pending_args);
        late_tool_starts.push((state.anthropic_index, fallback_id, fallback_name, pending));
    }
    late_tool_starts.sort_unstable_by_key(|(index, _, _, _)| *index);
    for (index, id, name, pending) in late_tool_starts {
        events.push(sse_event_bytes(
            "content_block_start",
            &json!({
                "type": "content_block_start",
                "index": index,
                "content_block": {
                    "type": "tool_use",
                    "id": id,
                    "name": name
                }
            }),
        ));
        open_tool_block_indices.insert(index);
        if !pending.is_empty() {
            events.push(sse_event_bytes(
                "content_block_delta",
                &json!({
                    "type": "content_block_delta",
                    "index": index,
                    "delta": {
                        "type": "input_json_delta",
                        "partial_json": pending
                    }
                }),
            ));
        }
    }

    let had_tool_blocks = tool_blocks_by_index.values().any(|state| state.started);

    if !open_tool_block_indices.is_empty() {
        let mut tool_indices: Vec<u32> = open_tool_block_indices.iter().copied().collect();
        tool_indices.sort_unstable();
        for index in tool_indices {
            events.push(sse_event_bytes(
                "content_block_stop",
                &json!({"type": "content_block_stop", "index": index}),
            ));
        }
        open_tool_block_indices.clear();
    }

    (events, had_tool_blocks)
}

/// 创建 Anthropic SSE 流
pub fn create_anthropic_sse_stream<E: std::error::Error + Send + 'static>(
    stream: impl Stream<Item = Result<Bytes, E>> + Send + 'static,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> + Send {
    async_stream::stream! {
        let mut buffer = String::new();
        let mut utf8_remainder: Vec<u8> = Vec::new();
        let mut message_id = None;
        let mut current_model = None;
        let mut next_content_index: u32 = 0;
        let mut has_sent_message_start = false;
        // 某些上游 provider（如 OpenRouter 的 kimi-k2.6）会在 tool_use 后发送多个
        // 带 finish_reason 的 SSE chunk。Anthropic 协议要求每个消息流只能有一个
        // message_delta，重复会导致 Claude Code abort 连接。因此需要：
        // 1) has_emitted_message_delta: 去重，只处理第一个 finish_reason
        // 2) pending_message_delta: 缓存延迟到 [DONE] 发送，确保 usage 完整
        let mut has_emitted_message_delta = false;
        let mut pending_message_delta: Option<(Option<String>, Option<Value>)> = None;
        let mut has_sent_message_stop = false;
        let mut stream_ended_with_error = false;
        let mut latest_usage: Option<Value> = None;
        let mut current_non_tool_block_type: Option<&'static str> = None;
        let mut current_non_tool_block_index: Option<u32> = None;
        let mut tool_blocks_by_index: HashMap<usize, ToolBlockState> = HashMap::new();
        let mut open_tool_block_indices: HashSet<u32> = HashSet::new();
        let mut tool_call_slots = ChatToolCallSlots::default();

        tokio::pin!(stream);

        'upstream: while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(bytes) => {
                    crate::proxy::sse::append_utf8_safe(&mut buffer, &mut utf8_remainder, &bytes);

                    while let Some(line) = take_sse_block(&mut buffer) {
                        if line.trim().is_empty() {
                            continue;
                        }

                        let mut block_event: Option<String> = None;
                        for l in line.lines() {
                            if let Some(event) = strip_sse_field(l, "event") {
                                block_event = Some(event.trim().to_string());
                                continue;
                            }
                            if let Some(data) = strip_sse_field(l, "data") {
                                if data.trim().is_empty() {
                                    continue;
                                }
                                if data.trim() == "[DONE]" {
                                    log::debug!("[Claude/OpenRouter] <<< OpenAI SSE: [DONE]");
                                    if has_sent_message_stop {
                                        continue;
                                    }

                                    if !has_sent_message_start {
                                        // [DONE] without any content: never pretend it was a
                                        // successful (empty) completion.
                                        log::warn!("[Claude/OpenRouter] upstream sent [DONE] before any chunk");
                                        yield Ok(anthropic_error_event(
                                            "stream_truncated",
                                            "Upstream Chat Completions stream ended before sending any content",
                                        ));
                                        stream_ended_with_error = true;
                                        break 'upstream;
                                    }

                                    // [DONE] arrived without a finish_reason: close dangling
                                    // blocks and synthesize the message_delta.
                                    if !has_emitted_message_delta {
                                        has_emitted_message_delta = true;
                                        let (events, had_tool_blocks) = close_open_blocks(
                                            &mut current_non_tool_block_index,
                                            &mut current_non_tool_block_type,
                                            &mut tool_blocks_by_index,
                                            &mut open_tool_block_indices,
                                        );
                                        for event in events {
                                            yield Ok(event);
                                        }
                                        let stop_reason =
                                            if had_tool_blocks { "tool_use" } else { "end_turn" };
                                        pending_message_delta =
                                            Some((Some(stop_reason.to_string()), latest_usage.clone()));
                                    }

                                    // 流正常结束，发出缓存的 message_delta（含完整 usage）。
                                    if let Some((stop_reason, usage_json)) = pending_message_delta.take() {
                                        let event = build_message_delta_event(stop_reason, usage_json);
                                        log::debug!("[Claude/OpenRouter] >>> Anthropic SSE: message_delta (from pending)");
                                        yield Ok(sse_event_bytes("message_delta", &event));
                                    }

                                    log::debug!("[Claude/OpenRouter] >>> Anthropic SSE: message_stop");
                                    yield Ok(sse_event_bytes("message_stop", &json!({"type": "message_stop"})));
                                    has_sent_message_stop = true;
                                    continue;
                                }

                                let payload: Value = match serde_json::from_str(data) {
                                    Ok(value) => value,
                                    Err(e) => {
                                        log::warn!("[Claude/OpenRouter] unparsable upstream SSE payload: {e}");
                                        yield Ok(anthropic_error_event(
                                            "api_error",
                                            &format!("Upstream sent an unparsable SSE payload: {e}"),
                                        ));
                                        stream_ended_with_error = true;
                                        break 'upstream;
                                    }
                                };

                                if let Some((message, error_type)) =
                                    upstream_chat_error(block_event.as_deref(), &payload)
                                {
                                    log::warn!("[Claude/OpenRouter] upstream stream error ({error_type}): {message}");
                                    yield Ok(anthropic_error_event(&error_type, &message));
                                    stream_ended_with_error = true;
                                    break 'upstream;
                                }

                                let chunk = match serde_json::from_value::<OpenAIStreamChunk>(payload) {
                                    Ok(chunk) => chunk,
                                    Err(e) => {
                                        log::warn!("[Claude/OpenRouter] skipping SSE chunk with unexpected shape: {e}");
                                        continue;
                                    }
                                };
                                {
                                    log::debug!("[Claude/OpenRouter] <<< SSE chunk received");

                                    if message_id.is_none() && !chunk.id.is_empty() {
                                        message_id = Some(chunk.id.clone());
                                    }
                                    if current_model.is_none() && !chunk.model.is_empty() {
                                        current_model = Some(chunk.model.clone());
                                    }

                                    let chunk_usage_json =
                                        chunk.usage.as_ref().map(build_anthropic_usage_json);
                                    if let Some(usage_json) = &chunk_usage_json {
                                        latest_usage = Some(usage_json.clone());
                                        if let Some((_, pending_usage)) = pending_message_delta.as_mut() {
                                            *pending_usage = Some(usage_json.clone());
                                        }
                                    }

                                    if let Some(choice) = chunk.choices.first() {
                                        if !has_sent_message_start {
                                            // Build usage with cache tokens if available from first chunk
                                            let mut start_usage = json!({
                                                "input_tokens": 0,
                                                "output_tokens": 0
                                            });
                                            if let Some(u) = &chunk.usage {
                                                let cached = extract_cache_read_tokens(u).unwrap_or(0);
                                                let cache_creation =
                                                    extract_cache_write_tokens(u).unwrap_or(0);
                                                let input = u
                                                    .prompt_tokens
                                                    .saturating_sub(cached)
                                                    .saturating_sub(cache_creation);
                                                start_usage["input_tokens"] = json!(input);
                                                if cached > 0 {
                                                    start_usage["cache_read_input_tokens"] = json!(cached);
                                                }
                                                if cache_creation > 0 {
                                                    start_usage["cache_creation_input_tokens"] =
                                                        json!(cache_creation);
                                                }
                                            }

                                            let event = json!({
                                                "type": "message_start",
                                                "message": {
                                                    "id": super::transform::ensure_message_id(&mut message_id),
                                                    "type": "message",
                                                    "role": "assistant",
                                                    "model": current_model.clone().unwrap_or_default(),
                                                    "usage": start_usage
                                                }
                                            });
                                            let sse_data = format!("event: message_start\ndata: {}\n\n",
                                                serde_json::to_string(&event).unwrap_or_default());
                                            yield Ok(Bytes::from(sse_data));
                                            has_sent_message_start = true;
                                        }

                                        // PROV-16: every block was closed when the first
                                        // finish_reason arrived; trailing deltas would target
                                        // closed indices (rejected by Anthropic clients), so drop
                                        // them. Usage on such chunks was already captured above.
                                        if has_emitted_message_delta {
                                            let has_payload = choice
                                                .delta
                                                .content
                                                .as_deref()
                                                .is_some_and(|c| !c.is_empty())
                                                || choice
                                                    .delta
                                                    .reasoning
                                                    .as_deref()
                                                    .is_some_and(|r| !r.is_empty())
                                                || choice
                                                    .delta
                                                    .tool_calls
                                                    .as_ref()
                                                    .is_some_and(|t| !t.is_empty());
                                            if has_payload {
                                                log::warn!("[Claude/OpenRouter] dropping delta received after finish_reason");
                                            }
                                            continue;
                                        }

                                        // 处理 reasoning（thinking）；空 reasoning_content 不开 thinking 块 (PROV-16)
                                        if let Some(reasoning) =
                                            choice.delta.reasoning.as_ref().filter(|r| !r.is_empty())
                                        {
                                            if current_non_tool_block_type != Some("thinking") {
                                                if let Some(index) = current_non_tool_block_index.take() {
                                                    let event = json!({
                                                        "type": "content_block_stop",
                                                        "index": index
                                                    });
                                                    let sse_data = format!("event: content_block_stop\ndata: {}\n\n",
                                                        serde_json::to_string(&event).unwrap_or_default());
                                                    yield Ok(Bytes::from(sse_data));
                                                }
                                                let index = next_content_index;
                                                next_content_index += 1;
                                                let event = json!({
                                                    "type": "content_block_start",
                                                    "index": index,
                                                    "content_block": {
                                                        "type": "thinking",
                                                        "thinking": ""
                                                    }
                                                });
                                                let sse_data = format!("event: content_block_start\ndata: {}\n\n",
                                                    serde_json::to_string(&event).unwrap_or_default());
                                                yield Ok(Bytes::from(sse_data));
                                                current_non_tool_block_type = Some("thinking");
                                                current_non_tool_block_index = Some(index);
                                            }

                                            if let Some(index) = current_non_tool_block_index {
                                                let event = json!({
                                                    "type": "content_block_delta",
                                                    "index": index,
                                                    "delta": {
                                                        "type": "thinking_delta",
                                                        "thinking": reasoning
                                                    }
                                                });
                                                let sse_data = format!("event: content_block_delta\ndata: {}\n\n",
                                                    serde_json::to_string(&event).unwrap_or_default());
                                                yield Ok(Bytes::from(sse_data));
                                            }
                                        }

                                        // 处理文本内容
                                        if let Some(content) = &choice.delta.content {
                                            if !content.is_empty() {
                                                if current_non_tool_block_type != Some("text") {
                                                    if let Some(index) = current_non_tool_block_index.take() {
                                                        let event = json!({
                                                            "type": "content_block_stop",
                                                            "index": index
                                                        });
                                                        let sse_data = format!("event: content_block_stop\ndata: {}\n\n",
                                                            serde_json::to_string(&event).unwrap_or_default());
                                                        yield Ok(Bytes::from(sse_data));
                                                    }

                                                    let index = next_content_index;
                                                    next_content_index += 1;
                                                    let event = json!({
                                                        "type": "content_block_start",
                                                        "index": index,
                                                        "content_block": {
                                                            "type": "text",
                                                            "text": ""
                                                        }
                                                    });
                                                    let sse_data = format!("event: content_block_start\ndata: {}\n\n",
                                                        serde_json::to_string(&event).unwrap_or_default());
                                                    yield Ok(Bytes::from(sse_data));
                                                    current_non_tool_block_type = Some("text");
                                                    current_non_tool_block_index = Some(index);
                                                }

                                                if let Some(index) = current_non_tool_block_index {
                                                    let event = json!({
                                                        "type": "content_block_delta",
                                                        "index": index,
                                                        "delta": {
                                                            "type": "text_delta",
                                                            "text": content
                                                        }
                                                    });
                                                    let sse_data = format!("event: content_block_delta\ndata: {}\n\n",
                                                        serde_json::to_string(&event).unwrap_or_default());
                                                    yield Ok(Bytes::from(sse_data));
                                                }
                                            }
                                        }

                                        // 处理工具调用
                                        if let Some(tool_calls) = &choice.delta.tool_calls {
                                            if !tool_calls.is_empty() {
                                                if let Some(index) = current_non_tool_block_index.take() {
                                                    let event = json!({
                                                        "type": "content_block_stop",
                                                        "index": index
                                                    });
                                                    let sse_data = format!("event: content_block_stop\ndata: {}\n\n",
                                                        serde_json::to_string(&event).unwrap_or_default());
                                                    yield Ok(Bytes::from(sse_data));
                                                }
                                                current_non_tool_block_type = None;

                                                for tool_call in tool_calls {
                                                    let (
                                                        anthropic_index,
                                                        id,
                                                        name,
                                                        should_start,
                                                        pending_after_start,
                                                        immediate_delta,
                                                    ) = {
                                                        let slot = tool_call_slots.resolve(
                                                            tool_call.index,
                                                            tool_call.id.as_deref(),
                                                            tool_call
                                                                .function
                                                                .as_ref()
                                                                .and_then(|f| f.name.as_deref()),
                                                        );
                                                        let state = tool_blocks_by_index
                                                            .entry(slot)
                                                            .or_insert_with(|| {
                                                                let index = next_content_index;
                                                                next_content_index += 1;
                                                                ToolBlockState {
                                                                    anthropic_index: index,
                                                                    id: String::new(),
                                                                    name: String::new(),
                                                                    started: false,
                                                                    pending_args: String::new(),
                                                                    consecutive_whitespace: 0,
                                                                    aborted: false,
                                                                }
                                                            });

                                                        // 如果此 tool call 已被中止（无限空白 bug），跳过后续处理
                                                        if state.aborted {
                                                            continue;
                                                        }

                                                        if let Some(id) = &tool_call.id {
                                                            state.id = id.clone();
                                                        }
                                                        if let Some(function) = &tool_call.function {
                                                            if let Some(name) = &function.name {
                                                                state.name = name.clone();
                                                            }
                                                        }

                                                        let should_start =
                                                            !state.started
                                                                && !state.id.is_empty()
                                                                && !state.name.is_empty();
                                                        if should_start {
                                                            state.started = true;
                                                        }
                                                        let pending_after_start = if should_start
                                                            && !state.pending_args.is_empty()
                                                        {
                                                            Some(std::mem::take(&mut state.pending_args))
                                                        } else {
                                                            None
                                                        };
                                                        let args_delta = tool_call
                                                            .function
                                                            .as_ref()
                                                            .and_then(|f| f.arguments.clone());
                                                        let immediate_delta = if let Some(args) = args_delta {
                                                            // 无限空白 bug 检测：跟踪连续空白字符
                                                            for ch in args.chars() {
                                                                if ch.is_whitespace() {
                                                                    state.consecutive_whitespace += 1;
                                                                } else {
                                                                    state.consecutive_whitespace = 0;
                                                                }
                                                            }
                                                            if state.consecutive_whitespace >= INFINITE_WHITESPACE_THRESHOLD {
                                                                log::warn!(
                                                                    "[Copilot] 检测到无限空白 bug (tool: {}), 中止此 tool call 流",
                                                                    state.name
                                                                );
                                                                state.aborted = true;
                                                                None
                                                            } else if state.started {
                                                                Some(args)
                                                            } else {
                                                                state.pending_args.push_str(&args);
                                                                None
                                                            }
                                                        } else {
                                                            None
                                                        };
                                                        (
                                                            state.anthropic_index,
                                                            state.id.clone(),
                                                            state.name.clone(),
                                                            should_start,
                                                            pending_after_start,
                                                            immediate_delta,
                                                        )
                                                    };

                                                    if should_start {
                                                        let event = json!({
                                                            "type": "content_block_start",
                                                            "index": anthropic_index,
                                                            "content_block": {
                                                                "type": "tool_use",
                                                                "id": id,
                                                                "name": name
                                                            }
                                                        });
                                                        let sse_data = format!("event: content_block_start\ndata: {}\n\n",
                                                            serde_json::to_string(&event).unwrap_or_default());
                                                        yield Ok(Bytes::from(sse_data));
                                                        open_tool_block_indices.insert(anthropic_index);
                                                    }

                                                    if let Some(args) = pending_after_start {
                                                        let event = json!({
                                                            "type": "content_block_delta",
                                                            "index": anthropic_index,
                                                            "delta": {
                                                                "type": "input_json_delta",
                                                                "partial_json": args
                                                            }
                                                        });
                                                        let sse_data = format!("event: content_block_delta\ndata: {}\n\n",
                                                            serde_json::to_string(&event).unwrap_or_default());
                                                        yield Ok(Bytes::from(sse_data));
                                                    }

                                                    if let Some(args) = immediate_delta {
                                                        let event = json!({
                                                            "type": "content_block_delta",
                                                            "index": anthropic_index,
                                                            "delta": {
                                                                "type": "input_json_delta",
                                                                "partial_json": args
                                                            }
                                                        });
                                                        let sse_data = format!("event: content_block_delta\ndata: {}\n\n",
                                                            serde_json::to_string(&event).unwrap_or_default());
                                                        yield Ok(Bytes::from(sse_data));
                                                    }
                                                }
                                            }
                                        }

                                        // 处理 finish_reason。
                                        // 注意：OpenRouter 某些 provider 会发送多个带 finish_reason 的 chunk
                                        // （第一个 usage 为 null，后续才补全）。此处只做缓存，不立即发送，
                                        // 等到 [DONE] 或流末尾再统一发出，确保 usage 完整且只发一次。
                                        if let Some(finish_reason) = &choice.finish_reason {
                                            let stop_reason = map_stop_reason(Some(finish_reason));
                                            let usage_json =
                                                chunk_usage_json.clone().or_else(|| latest_usage.clone());

                                            if has_emitted_message_delta {
                                                // 更新缓存的 message_delta usage（如果有更完整的 usage）
                                                if let (Some((_, ref mut usage)), Some(uj)) = (&mut pending_message_delta, usage_json) {
                                                    *usage = Some(uj);
                                                }
                                                continue;
                                            }
                                            has_emitted_message_delta = true;

                                            let (events, _) = close_open_blocks(
                                                &mut current_non_tool_block_index,
                                                &mut current_non_tool_block_type,
                                                &mut tool_blocks_by_index,
                                                &mut open_tool_block_indices,
                                            );
                                            for event in events {
                                                yield Ok(event);
                                            }

                                            // 缓存 message_delta，等到 [DONE] 时发送（以便收集完整的 usage）
                                            pending_message_delta = Some((stop_reason, usage_json));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    log::error!("Stream error: {e}");
                    stream_ended_with_error = true;
                    let error_event = json!({
                        "type": "error",
                        "error": {
                            "type": "stream_error",
                            "message": format!("Stream error: {e}")
                        }
                    });
                    let sse_data = format!("event: error\ndata: {}\n\n",
                        serde_json::to_string(&error_event).unwrap_or_default());
                    yield Ok(Bytes::from(sse_data));
                    break;
                }
            }
        }

        // 流自然结束但未收到 [DONE] 时，确保发送缓存的 message_delta 和 message_stop。
        // 若上游已显式报错，则只保留 error 事件，避免把失败伪装成成功完成。
        if !stream_ended_with_error && !has_sent_message_stop {
            if let Some((stop_reason, usage_json)) = pending_message_delta.take() {
                let event = build_message_delta_event(stop_reason, usage_json);
                log::debug!("[Claude/OpenRouter] >>> Anthropic SSE: message_delta (at stream end)");
                yield Ok(sse_event_bytes("message_delta", &event));
                log::debug!("[Claude/OpenRouter] >>> Anthropic SSE: message_stop (at stream end)");
                yield Ok(sse_event_bytes("message_stop", &json!({"type": "message_stop"})));
            } else {
                // Neither finish_reason nor [DONE]: the upstream connection ended
                // early. Report it instead of leaving the client with an
                // unterminated (or empty) stream.
                let message = if has_sent_message_start {
                    "Upstream Chat Completions stream ended before sending finish_reason"
                } else {
                    "Upstream Chat Completions stream ended before sending any content"
                };
                log::warn!("[Claude/OpenRouter] {message}");
                yield Ok(anthropic_error_event("stream_truncated", message));
            }
        }
    }
}

/// Extract cache_read tokens from Usage, checking both direct field and nested details
fn extract_cache_read_tokens(usage: &Usage) -> Option<u32> {
    // Direct field takes priority (compatible servers)
    if let Some(v) = usage.cache_read_input_tokens {
        return Some(v);
    }
    // OpenAI standard: prompt_tokens_details.cached_tokens
    usage
        .prompt_tokens_details
        .as_ref()
        .map(|d| d.cached_tokens)
        .filter(|&v| v > 0)
}

/// Extract cache-write tokens from direct compatibility fields or OpenAI details.
fn extract_cache_write_tokens(usage: &Usage) -> Option<u32> {
    if let Some(value) = usage.cache_creation_input_tokens {
        return Some(value);
    }
    usage
        .prompt_tokens_details
        .as_ref()
        .map(|details| details.cache_write_tokens)
        .filter(|value| *value > 0)
}

/// 映射停止原因
fn map_stop_reason(finish_reason: Option<&str>) -> Option<String> {
    finish_reason.map(|r| {
        match r {
            "tool_calls" | "function_call" => "tool_use",
            "stop" => "end_turn",
            "length" => "max_tokens",
            // PROV-24: surface filtered output as a refusal, not a normal end.
            "content_filter" => "refusal",
            other => {
                log::warn!("[Claude/OpenRouter] Unknown finish_reason in streaming: {other}");
                "end_turn"
            }
        }
        .to_string()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::stream;
    use futures::StreamExt;
    use serde_json::Value;
    use std::collections::HashMap;

    async fn collect_anthropic_events(input: &str) -> Vec<Value> {
        let upstream = stream::iter(vec![Ok::<_, std::io::Error>(Bytes::from(
            input.as_bytes().to_vec(),
        ))]);
        let converted = create_anthropic_sse_stream(upstream);
        let chunks: Vec<_> = converted.collect().await;
        let merged = chunks
            .into_iter()
            .map(|chunk| String::from_utf8_lossy(chunk.unwrap().as_ref()).to_string())
            .collect::<String>();

        merged
            .split("\n\n")
            .filter_map(|block| {
                let data = block
                    .lines()
                    .find_map(|line| strip_sse_field(line, "data"))?;
                serde_json::from_str::<Value>(data).ok()
            })
            .collect()
    }

    fn event_type(event: &Value) -> Option<&str> {
        event.get("type").and_then(|v| v.as_str())
    }

    #[tokio::test]
    async fn test_streaming_empty_reasoning_content_opens_no_thinking_block() {
        let input = concat!(
            "data: {\"id\":\"c1\",\"model\":\"m\",\"choices\":[{\"delta\":{\"reasoning_content\":\"\",\"content\":\"Hel\"}}]}\n\n",
            "data: {\"id\":\"c1\",\"model\":\"m\",\"choices\":[{\"delta\":{\"reasoning_content\":\"\",\"content\":\"lo\"}}]}\n\n",
            "data: {\"id\":\"c1\",\"model\":\"m\",\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
            "data: [DONE]\n\n"
        );
        let events = collect_anthropic_events(input).await;
        let starts: Vec<_> = events
            .iter()
            .filter(|e| event_type(e) == Some("content_block_start"))
            .collect();
        assert_eq!(starts.len(), 1);
        assert_eq!(starts[0]["content_block"]["type"], "text");
    }

    #[tokio::test]
    async fn test_streaming_tool_delta_after_finish_reason_is_dropped() {
        let input = concat!(
            "data: {\"id\":\"c1\",\"model\":\"m\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_a\",\"type\":\"function\",\"function\":{\"name\":\"t\",\"arguments\":\"{}\"}}]}}]}\n\n",
            "data: {\"id\":\"c1\",\"model\":\"m\",\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: {\"id\":\"c1\",\"model\":\"m\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\" \"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: {\"id\":\"c1\",\"model\":\"m\",\"choices\":[{\"delta\":{\"content\":\"late\"}}]}\n\n",
            "data: [DONE]\n\n"
        );
        let events = collect_anthropic_events(input).await;
        let stop_pos = events
            .iter()
            .position(|e| event_type(e) == Some("content_block_stop"))
            .expect("tool block closed");
        assert!(events[stop_pos..]
            .iter()
            .all(|e| event_type(e) != Some("content_block_delta")
                && event_type(e) != Some("content_block_start")));
        assert_eq!(
            events
                .iter()
                .filter(|e| event_type(e) == Some("message_delta"))
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn test_streaming_missing_upstream_id_synthesizes_message_id() {
        let input = concat!(
            "data: {\"model\":\"m\",\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\n",
            "data: {\"model\":\"m\",\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":1}}\n\n",
            "data: [DONE]\n\n"
        );
        let events = collect_anthropic_events(input).await;
        let start = events
            .iter()
            .find(|e| event_type(e) == Some("message_start"))
            .expect("message_start");
        let id = start
            .pointer("/message/id")
            .and_then(|v| v.as_str())
            .unwrap();
        assert!(id.starts_with("msg_") && id.len() > 4, "id = {id}");
        let usage = crate::proxy::usage::parser::TokenUsage::from_claude_stream_events(&events)
            .expect("usage");
        assert_ne!(
            usage.dedup_request_id(),
            crate::proxy::usage::parser::SESSION_REQUEST_ID_PREFIX
        );
    }

    #[test]
    fn test_map_stop_reason_legacy_and_filtered_values() {
        assert_eq!(
            map_stop_reason(Some("function_call")),
            Some("tool_use".to_string())
        );
        assert_eq!(
            map_stop_reason(Some("content_filter")),
            Some("refusal".to_string())
        );
    }

    #[tokio::test]
    async fn test_streaming_tool_calls_routed_by_index() {
        let input = concat!(
            "data: {\"id\":\"chatcmpl_1\",\"model\":\"gpt-4o\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_0\",\"type\":\"function\",\"function\":{\"name\":\"first_tool\"}}]}}]}\n\n",
            "data: {\"id\":\"chatcmpl_1\",\"model\":\"gpt-4o\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":1,\"id\":\"call_1\",\"type\":\"function\",\"function\":{\"name\":\"second_tool\"}}]}}]}\n\n",
            "data: {\"id\":\"chatcmpl_1\",\"model\":\"gpt-4o\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":1,\"function\":{\"arguments\":\"{\\\"b\\\":2}\"}}]}}]}\n\n",
            "data: {\"id\":\"chatcmpl_1\",\"model\":\"gpt-4o\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"a\\\":1}\"}}]}}]}\n\n",
            "data: {\"id\":\"chatcmpl_1\",\"model\":\"gpt-4o\",\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}],\"usage\":{\"prompt_tokens\":8,\"completion_tokens\":4}}\n\n",
            "data: [DONE]\n\n"
        );

        let upstream = stream::iter(vec![Ok::<_, std::io::Error>(Bytes::from(
            input.as_bytes().to_vec(),
        ))]);
        let converted = create_anthropic_sse_stream(upstream);
        let chunks: Vec<_> = converted.collect().await;

        let merged = chunks
            .into_iter()
            .map(|chunk| String::from_utf8_lossy(chunk.unwrap().as_ref()).to_string())
            .collect::<String>();

        let events: Vec<Value> = merged
            .split("\n\n")
            .filter_map(|block| {
                let data = block
                    .lines()
                    .find_map(|line| strip_sse_field(line, "data"))?;
                serde_json::from_str::<Value>(data).ok()
            })
            .collect();

        let mut tool_index_by_call: HashMap<String, u64> = HashMap::new();
        for event in &events {
            if event.get("type").and_then(|v| v.as_str()) == Some("content_block_start")
                && event
                    .pointer("/content_block/type")
                    .and_then(|v| v.as_str())
                    == Some("tool_use")
            {
                if let (Some(call_id), Some(index)) = (
                    event.pointer("/content_block/id").and_then(|v| v.as_str()),
                    event.get("index").and_then(|v| v.as_u64()),
                ) {
                    tool_index_by_call.insert(call_id.to_string(), index);
                }
            }
        }

        assert_eq!(tool_index_by_call.len(), 2);
        assert_ne!(
            tool_index_by_call.get("call_0"),
            tool_index_by_call.get("call_1")
        );

        let deltas: Vec<(u64, String)> = events
            .iter()
            .filter(|event| {
                event.get("type").and_then(|v| v.as_str()) == Some("content_block_delta")
                    && event.pointer("/delta/type").and_then(|v| v.as_str())
                        == Some("input_json_delta")
            })
            .filter_map(|event| {
                let index = event.get("index").and_then(|v| v.as_u64())?;
                let partial_json = event
                    .pointer("/delta/partial_json")
                    .and_then(|v| v.as_str())?
                    .to_string();
                Some((index, partial_json))
            })
            .collect();

        assert_eq!(deltas.len(), 2);
        let second_idx = deltas
            .iter()
            .find_map(|(index, payload)| (payload == "{\"b\":2}").then_some(*index))
            .unwrap();
        let first_idx = deltas
            .iter()
            .find_map(|(index, payload)| (payload == "{\"a\":1}").then_some(*index))
            .unwrap();

        assert_eq!(second_idx, *tool_index_by_call.get("call_1").unwrap());
        assert_eq!(first_idx, *tool_index_by_call.get("call_0").unwrap());

        assert!(events.iter().any(|event| {
            event.get("type").and_then(|v| v.as_str()) == Some("message_delta")
                && event.pointer("/delta/stop_reason").and_then(|v| v.as_str()) == Some("tool_use")
        }));
    }

    #[tokio::test]
    async fn test_streaming_delays_tool_start_until_id_and_name_ready() {
        let input = concat!(
            "data: {\"id\":\"chatcmpl_2\",\"model\":\"gpt-4o\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"a\\\":\"}}]}}]}\n\n",
            "data: {\"id\":\"chatcmpl_2\",\"model\":\"gpt-4o\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_0\",\"type\":\"function\",\"function\":{\"name\":\"first_tool\"}}]}}]}\n\n",
            "data: {\"id\":\"chatcmpl_2\",\"model\":\"gpt-4o\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"1}\"}}]}}]}\n\n",
            "data: {\"id\":\"chatcmpl_2\",\"model\":\"gpt-4o\",\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}],\"usage\":{\"prompt_tokens\":6,\"completion_tokens\":2}}\n\n",
            "data: [DONE]\n\n"
        );

        let upstream = stream::iter(vec![Ok::<_, std::io::Error>(Bytes::from(
            input.as_bytes().to_vec(),
        ))]);
        let converted = create_anthropic_sse_stream(upstream);
        let chunks: Vec<_> = converted.collect().await;
        let merged = chunks
            .into_iter()
            .map(|chunk| String::from_utf8_lossy(chunk.unwrap().as_ref()).to_string())
            .collect::<String>();

        let events: Vec<Value> = merged
            .split("\n\n")
            .filter_map(|block| {
                let data = block
                    .lines()
                    .find_map(|line| strip_sse_field(line, "data"))?;
                serde_json::from_str::<Value>(data).ok()
            })
            .collect();

        let starts: Vec<&Value> = events
            .iter()
            .filter(|event| {
                event.get("type").and_then(|v| v.as_str()) == Some("content_block_start")
                    && event
                        .pointer("/content_block/type")
                        .and_then(|v| v.as_str())
                        == Some("tool_use")
            })
            .collect();
        assert_eq!(starts.len(), 1);
        assert_eq!(
            starts[0]
                .pointer("/content_block/id")
                .and_then(|v| v.as_str())
                .unwrap_or(""),
            "call_0"
        );
        assert_eq!(
            starts[0]
                .pointer("/content_block/name")
                .and_then(|v| v.as_str())
                .unwrap_or(""),
            "first_tool"
        );

        let deltas: Vec<&str> = events
            .iter()
            .filter(|event| {
                event.get("type").and_then(|v| v.as_str()) == Some("content_block_delta")
                    && event.pointer("/delta/type").and_then(|v| v.as_str())
                        == Some("input_json_delta")
            })
            .filter_map(|event| {
                event
                    .pointer("/delta/partial_json")
                    .and_then(|v| v.as_str())
            })
            .collect();
        assert!(deltas.contains(&"{\"a\":"));
        assert!(deltas.contains(&"1}"));
    }

    #[tokio::test]
    async fn test_streaming_chinese_split_across_chunks_no_replacement_chars() {
        // "你好" split across two TCP chunks inside a streaming text delta.
        // Before the fix, from_utf8_lossy would produce U+FFFD for each half.
        let full = concat!(
            "data: {\"id\":\"chatcmpl_3\",\"model\":\"gpt-4o\",\"choices\":[{\"delta\":{\"content\":\"你好\"}}]}\n\n",
            "data: {\"id\":\"chatcmpl_3\",\"model\":\"gpt-4o\",\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":2}}\n\n",
            "data: [DONE]\n\n"
        );
        let bytes = full.as_bytes();

        // Find "你" in the byte stream and split inside it
        let ni_start = bytes.windows(3).position(|w| w == "你".as_bytes()).unwrap();
        let split_point = ni_start + 1; // split after first byte of "你"

        let chunk1 = Bytes::from(bytes[..split_point].to_vec());
        let chunk2 = Bytes::from(bytes[split_point..].to_vec());

        let upstream = stream::iter(vec![
            Ok::<_, std::io::Error>(chunk1),
            Ok::<_, std::io::Error>(chunk2),
        ]);
        let converted = create_anthropic_sse_stream(upstream);
        let chunks: Vec<_> = converted.collect().await;

        let merged = chunks
            .into_iter()
            .map(|chunk| String::from_utf8_lossy(chunk.unwrap().as_ref()).to_string())
            .collect::<String>();

        // Must contain the original Chinese characters, not replacement chars
        assert!(
            merged.contains("你好"),
            "expected '你好' in output, got replacement chars (U+FFFD)"
        );
        assert!(
            !merged.contains('\u{FFFD}'),
            "output must not contain U+FFFD replacement characters"
        );
    }

    #[tokio::test]
    async fn test_duplicate_finish_reason_emits_only_one_message_delta() {
        // Simulates OpenRouter behavior where two chunks carry finish_reason:
        // first with null usage, second with populated usage.
        let input = concat!(
            "data: {\"id\":\"chatcmpl_dup\",\"model\":\"gpt-4o\",\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: {\"id\":\"chatcmpl_dup\",\"model\":\"gpt-4o\",\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}],\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":5}}\n\n",
            "data: [DONE]\n\n"
        );

        let upstream = stream::iter(vec![Ok::<_, std::io::Error>(Bytes::from(
            input.as_bytes().to_vec(),
        ))]);
        let converted = create_anthropic_sse_stream(upstream);
        let chunks: Vec<_> = converted.collect().await;

        let merged = chunks
            .into_iter()
            .map(|chunk| String::from_utf8_lossy(chunk.unwrap().as_ref()).to_string())
            .collect::<String>();

        let events: Vec<Value> = merged
            .split("\n\n")
            .filter_map(|block| {
                let data = block
                    .lines()
                    .find_map(|line| strip_sse_field(line, "data"))?;
                serde_json::from_str::<Value>(data).ok()
            })
            .collect();

        let message_deltas: Vec<&Value> = events
            .iter()
            .filter(|e| e.get("type").and_then(|v| v.as_str()) == Some("message_delta"))
            .collect();

        assert_eq!(
            message_deltas.len(),
            1,
            "duplicate finish_reason chunks must produce exactly one message_delta, got {}: {:?}",
            message_deltas.len(),
            message_deltas
        );

        assert_eq!(message_deltas[0]["usage"]["input_tokens"], 10);
        assert_eq!(message_deltas[0]["usage"]["output_tokens"], 5);

        let message_stops = events
            .iter()
            .filter(|e| e.get("type").and_then(|v| v.as_str()) == Some("message_stop"))
            .count();
        assert_eq!(message_stops, 1, "message_stop must only be emitted once");
    }

    #[tokio::test]
    async fn test_usage_only_chunk_after_finish_reason_updates_message_delta_usage() {
        let input = concat!(
            "data: {\"id\":\"chatcmpl_split\",\"model\":\"glm-5.1\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"tool-0924\",\"type\":\"function\",\"function\":{\"name\":\"Bash\",\"arguments\":\"{\\\"command\\\":\\\"pwd\\\"}\"}}]}}]}\n\n",
            "data: {\"id\":\"chatcmpl_split\",\"model\":\"glm-5.1\",\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":13312,\"completion_tokens\":79,\"prompt_tokens_details\":{\"cached_tokens\":100}}}\n\n",
            "data: [DONE]\n\n"
        );

        let events = collect_anthropic_events(input).await;
        let message_deltas: Vec<&Value> = events
            .iter()
            .filter(|event| event_type(event) == Some("message_delta"))
            .collect();
        let message_stops = events
            .iter()
            .filter(|event| event_type(event) == Some("message_stop"))
            .count();

        assert_eq!(message_deltas.len(), 1);
        assert_eq!(message_stops, 1);

        let message_delta = message_deltas[0];
        assert_eq!(
            message_delta
                .pointer("/delta/stop_reason")
                .and_then(|v| v.as_str()),
            Some("tool_use")
        );
        assert_eq!(
            message_delta
                .pointer("/usage/input_tokens")
                .and_then(|v| v.as_u64()),
            Some(13212)
        );
        assert_eq!(
            message_delta
                .pointer("/usage/output_tokens")
                .and_then(|v| v.as_u64()),
            Some(79)
        );
        assert_eq!(
            message_delta
                .pointer("/usage/cache_read_input_tokens")
                .and_then(|v| v.as_u64()),
            Some(100)
        );
    }

    #[tokio::test]
    async fn test_usage_chunk_subtracts_cache_read_and_creation_from_input() {
        // prompt_tokens(1000) 含 cache_read(600) 与 cache_creation(300)；转 Anthropic 后
        // input 应为 fresh，守恒：input(100) + cache_read(600) + cache_creation(300) == prompt(1000)。
        let input = concat!(
            "data: {\"id\":\"chatcmpl_cc\",\"model\":\"glm-5.1\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"tool-1\",\"type\":\"function\",\"function\":{\"name\":\"Bash\",\"arguments\":\"{\\\"command\\\":\\\"pwd\\\"}\"}}]}}]}\n\n",
            "data: {\"id\":\"chatcmpl_cc\",\"model\":\"glm-5.1\",\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":1000,\"completion_tokens\":50,\"prompt_tokens_details\":{\"cached_tokens\":600,\"cache_write_tokens\":300}}}\n\n",
            "data: [DONE]\n\n"
        );

        let events = collect_anthropic_events(input).await;
        let message_delta = events
            .iter()
            .find(|event| event_type(event) == Some("message_delta"))
            .expect("should emit message_delta with usage");

        // fresh input = 1000 - 600 - 300 = 100
        assert_eq!(
            message_delta
                .pointer("/usage/input_tokens")
                .and_then(|v| v.as_u64()),
            Some(100)
        );
        assert_eq!(
            message_delta
                .pointer("/usage/cache_read_input_tokens")
                .and_then(|v| v.as_u64()),
            Some(600)
        );
        assert_eq!(
            message_delta
                .pointer("/usage/cache_creation_input_tokens")
                .and_then(|v| v.as_u64()),
            Some(300)
        );
    }

    #[tokio::test]
    async fn test_usage_chunk_clamps_input_to_zero_when_cache_exceeds_prompt() {
        // prompt(100) < cache_read(80)+cache_creation(50)=130：saturating 钳到 0，防下溢。
        // 钉桩：阻止未来把 saturating_sub 误改成普通减法(debug panic / release wrap)。
        let input = concat!(
            "data: {\"id\":\"chatcmpl_uf\",\"model\":\"glm-5.1\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"tool-1\",\"type\":\"function\",\"function\":{\"name\":\"Bash\",\"arguments\":\"{\\\"command\\\":\\\"pwd\\\"}\"}}]}}]}\n\n",
            "data: {\"id\":\"chatcmpl_uf\",\"model\":\"glm-5.1\",\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":100,\"completion_tokens\":50,\"prompt_tokens_details\":{\"cached_tokens\":80},\"cache_creation_input_tokens\":50}}\n\n",
            "data: [DONE]\n\n"
        );

        let events = collect_anthropic_events(input).await;
        let message_delta = events
            .iter()
            .find(|event| event_type(event) == Some("message_delta"))
            .expect("should emit message_delta with usage");

        assert_eq!(
            message_delta
                .pointer("/usage/input_tokens")
                .and_then(|v| v.as_u64()),
            Some(0)
        );
        assert_eq!(
            message_delta
                .pointer("/usage/cache_read_input_tokens")
                .and_then(|v| v.as_u64()),
            Some(80)
        );
        assert_eq!(
            message_delta
                .pointer("/usage/cache_creation_input_tokens")
                .and_then(|v| v.as_u64()),
            Some(50)
        );
    }

    #[tokio::test]
    async fn test_message_delta_includes_zero_usage_when_stream_has_no_usage() {
        let input = concat!(
            "data: {\"id\":\"chatcmpl_no_usage\",\"model\":\"gpt-5.5\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_0\",\"type\":\"function\",\"function\":{\"name\":\"get_time\",\"arguments\":\"{}\"}}]}}]}\n\n",
            "data: {\"id\":\"chatcmpl_no_usage\",\"model\":\"gpt-5.5\",\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: [DONE]\n\n"
        );

        let events = collect_anthropic_events(input).await;
        let message_deltas: Vec<&Value> = events
            .iter()
            .filter(|event| event_type(event) == Some("message_delta"))
            .collect();

        assert_eq!(message_deltas.len(), 1);
        let message_delta = message_deltas[0];
        assert_eq!(
            message_delta
                .pointer("/delta/stop_reason")
                .and_then(|v| v.as_str()),
            Some("tool_use")
        );
        assert_eq!(
            message_delta
                .pointer("/usage/input_tokens")
                .and_then(|v| v.as_u64()),
            Some(0)
        );
        assert_eq!(
            message_delta
                .pointer("/usage/output_tokens")
                .and_then(|v| v.as_u64()),
            Some(0)
        );
    }

    #[tokio::test]
    async fn test_streaming_finalizes_after_finish_when_done_is_missing() {
        let input = concat!(
            "data: {\"id\":\"chatcmpl_no_done\",\"model\":\"gpt-4o\",\"choices\":[{\"delta\":{\"content\":\"hello\"}}]}\n\n",
            "data: {\"id\":\"chatcmpl_no_done\",\"model\":\"gpt-4o\",\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n"
        );

        let events = collect_anthropic_events(input).await;

        assert!(events.iter().any(|event| {
            event_type(event) == Some("message_delta")
                && event.pointer("/delta/stop_reason").and_then(|v| v.as_str()) == Some("end_turn")
        }));
        assert_eq!(
            events.last().and_then(|event| event_type(event)),
            Some("message_stop")
        );
    }

    #[tokio::test]
    async fn test_stream_end_without_finish_reason_does_not_emit_success_terminal_events() {
        let input = "data: {\"id\":\"chatcmpl_truncated\",\"model\":\"gpt-4o\",\"choices\":[{\"delta\":{\"content\":\"hello\"}}]}\n\n";

        let events = collect_anthropic_events(input).await;

        assert!(!events
            .iter()
            .any(|event| event_type(event) == Some("message_delta")));
        assert!(!events
            .iter()
            .any(|event| event_type(event) == Some("message_stop")));
    }

    fn has_event(events: &[Value], ty: &str) -> bool {
        events.iter().any(|event| event_type(event) == Some(ty))
    }

    // PROV-01
    #[tokio::test]
    async fn test_upstream_error_chunk_emits_error_without_message_stop() {
        let input = concat!(
            "data: {\"error\":{\"message\":\"quota exceeded\",\"type\":\"insufficient_quota\"}}\n\n",
            "data: [DONE]\n\n"
        );
        let events = collect_anthropic_events(input).await;
        let error = events
            .iter()
            .find(|event| event_type(event) == Some("error"))
            .expect("error event");
        assert_eq!(
            error.pointer("/error/message").and_then(|v| v.as_str()),
            Some("quota exceeded")
        );
        assert_eq!(
            error.pointer("/error/type").and_then(|v| v.as_str()),
            Some("insufficient_quota")
        );
        assert!(!has_event(&events, "message_stop"));
        assert!(!has_event(&events, "message_delta"));
    }

    #[tokio::test]
    async fn test_mid_stream_sse_error_event_terminates_stream() {
        let input = concat!(
            "data: {\"id\":\"c1\",\"model\":\"m\",\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\n",
            "event: error\ndata: {\"message\":\"rate limited\",\"code\":\"rate_limit\"}\n\n",
            "data: {\"id\":\"c1\",\"model\":\"m\",\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
            "data: [DONE]\n\n"
        );
        let events = collect_anthropic_events(input).await;
        let error = events
            .iter()
            .find(|event| event_type(event) == Some("error"))
            .expect("error event");
        assert_eq!(
            error.pointer("/error/message").and_then(|v| v.as_str()),
            Some("rate limited")
        );
        assert_eq!(events.last().and_then(event_type), Some("error"));
        assert!(!has_event(&events, "message_stop"));
    }

    #[tokio::test]
    async fn test_unparsable_payload_emits_error() {
        let input = concat!(
            "data: {\"id\":\"c1\",\"model\":\"m\",\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\n",
            "data: <html>Bad Gateway</html>\n\n",
            "data: [DONE]\n\n"
        );
        let events = collect_anthropic_events(input).await;
        assert!(has_event(&events, "error"));
        assert!(!has_event(&events, "message_stop"));
    }

    #[tokio::test]
    async fn test_chunk_without_delta_keeps_finish_reason() {
        let input = concat!(
            "data: {\"id\":\"c1\",\"model\":\"m\",\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\n",
            "data: {\"id\":\"c1\",\"model\":\"m\",\"choices\":[{\"finish_reason\":\"length\"}],\"usage\":{\"prompt_tokens\":null,\"completion_tokens\":3}}\n\n",
            "data: [DONE]\n\n"
        );
        let events = collect_anthropic_events(input).await;
        let delta = events
            .iter()
            .find(|event| event_type(event) == Some("message_delta"))
            .expect("message_delta");
        assert_eq!(
            delta.pointer("/delta/stop_reason").and_then(|v| v.as_str()),
            Some("max_tokens")
        );
        assert_eq!(
            delta
                .pointer("/usage/output_tokens")
                .and_then(|v| v.as_u64()),
            Some(3)
        );
        assert_eq!(events.last().and_then(event_type), Some("message_stop"));
    }

    #[tokio::test]
    async fn test_done_only_stream_emits_error_not_message_stop() {
        let events = collect_anthropic_events("data: [DONE]\n\n").await;
        let error = events
            .iter()
            .find(|event| event_type(event) == Some("error"))
            .expect("error event");
        assert_eq!(
            error.pointer("/error/type").and_then(|v| v.as_str()),
            Some("stream_truncated")
        );
        assert!(!has_event(&events, "message_stop"));
    }

    #[tokio::test]
    async fn test_empty_stream_emits_truncation_error() {
        let events = collect_anthropic_events("").await;
        assert!(has_event(&events, "error"));
        assert!(!has_event(&events, "message_stop"));
    }

    // PROV-37
    #[tokio::test]
    async fn test_done_without_finish_reason_closes_blocks_and_emits_message_delta() {
        let input = concat!(
            "data: {\"id\":\"c1\",\"model\":\"m\",\"choices\":[{\"delta\":{\"content\":\"Let me check\"},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"c1\",\"model\":\"m\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"type\":\"function\",\"function\":{\"name\":\"Bash\",\"arguments\":\"{\\\"command\\\":\"}}]},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"c1\",\"model\":\"m\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"\\\"ls\\\"}\"}}]},\"finish_reason\":null}],\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":5}}\n\n",
            "data: [DONE]\n\n"
        );
        let events = collect_anthropic_events(input).await;

        let started: HashSet<u64> = events
            .iter()
            .filter(|event| event_type(event) == Some("content_block_start"))
            .filter_map(|event| event.get("index").and_then(|v| v.as_u64()))
            .collect();
        let stopped: HashSet<u64> = events
            .iter()
            .filter(|event| event_type(event) == Some("content_block_stop"))
            .filter_map(|event| event.get("index").and_then(|v| v.as_u64()))
            .collect();
        assert_eq!(started.len(), 2);
        assert_eq!(started, stopped);

        let deltas: Vec<&Value> = events
            .iter()
            .filter(|event| event_type(event) == Some("message_delta"))
            .collect();
        assert_eq!(deltas.len(), 1);
        assert_eq!(
            deltas[0]
                .pointer("/delta/stop_reason")
                .and_then(|v| v.as_str()),
            Some("tool_use")
        );
        assert_eq!(events.last().and_then(event_type), Some("message_stop"));

        let usage = crate::proxy::usage::parser::TokenUsage::from_claude_stream_events(&events);
        assert!(usage.is_some());
    }

    #[tokio::test]
    async fn test_done_without_finish_reason_text_only_is_end_turn() {
        let input = concat!(
            "data: {\"id\":\"c1\",\"model\":\"m\",\"choices\":[{\"delta\":{\"content\":\"hello\"}}]}\n\n",
            "data: [DONE]\n\n"
        );
        let events = collect_anthropic_events(input).await;
        let delta = events
            .iter()
            .find(|event| event_type(event) == Some("message_delta"))
            .expect("message_delta");
        assert_eq!(
            delta.pointer("/delta/stop_reason").and_then(|v| v.as_str()),
            Some("end_turn")
        );
        assert!(has_event(&events, "content_block_stop"));
        assert_eq!(events.last().and_then(event_type), Some("message_stop"));
    }

    // PROV-02
    #[tokio::test]
    async fn test_tool_calls_without_index_stay_distinct() {
        let input = concat!(
            "data: {\"id\":\"c1\",\"model\":\"m\",\"choices\":[{\"delta\":{\"content\":\"Running\",\"tool_calls\":[{\"id\":\"call_a\",\"type\":\"function\",\"function\":{\"name\":\"read\",\"arguments\":\"{\\\"p\\\":\"}}]}}]}\n\n",
            "data: {\"id\":\"c1\",\"model\":\"m\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"function\":{\"arguments\":\"\\\"a\\\"}\"}}]}}]}\n\n",
            "data: {\"id\":\"c1\",\"model\":\"m\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"id\":\"call_b\",\"type\":\"function\",\"function\":{\"name\":\"write\",\"arguments\":\"{\\\"q\\\":1}\"}}]}}]}\n\n",
            "data: {\"id\":\"c1\",\"model\":\"m\",\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: [DONE]\n\n"
        );
        let events = collect_anthropic_events(input).await;

        let text: String = events
            .iter()
            .filter_map(|event| event.pointer("/delta/text").and_then(|v| v.as_str()))
            .collect();
        assert_eq!(text, "Running");

        let mut args_by_index: HashMap<u64, String> = HashMap::new();
        let mut id_by_index: HashMap<u64, String> = HashMap::new();
        for event in &events {
            let Some(index) = event.get("index").and_then(|v| v.as_u64()) else {
                continue;
            };
            if event
                .pointer("/content_block/type")
                .and_then(|v| v.as_str())
                == Some("tool_use")
            {
                id_by_index.insert(
                    index,
                    event
                        .pointer("/content_block/id")
                        .and_then(|v| v.as_str())
                        .unwrap()
                        .to_string(),
                );
            }
            if let Some(partial) = event
                .pointer("/delta/partial_json")
                .and_then(|v| v.as_str())
            {
                args_by_index.entry(index).or_default().push_str(partial);
            }
        }
        assert_eq!(id_by_index.len(), 2);
        let args_for = |id: &str| {
            let index = id_by_index
                .iter()
                .find(|(_, value)| value.as_str() == id)
                .map(|(index, _)| *index)
                .unwrap();
            args_by_index.get(&index).cloned().unwrap_or_default()
        };
        assert_eq!(args_for("call_a"), "{\"p\":\"a\"}");
        assert_eq!(args_for("call_b"), "{\"q\":1}");
    }

    #[tokio::test]
    async fn test_stream_error_does_not_emit_success_terminal_events() {
        let upstream = stream::iter(vec![Err::<Bytes, _>(std::io::Error::other(
            "upstream disconnected",
        ))]);
        let converted = create_anthropic_sse_stream(upstream);
        let chunks: Vec<_> = converted.collect().await;

        let merged = chunks
            .into_iter()
            .map(|chunk| String::from_utf8_lossy(chunk.unwrap().as_ref()).to_string())
            .collect::<String>();

        let events: Vec<Value> = merged
            .split("\n\n")
            .filter_map(|block| {
                let data = block
                    .lines()
                    .find_map(|line| strip_sse_field(line, "data"))?;
                serde_json::from_str::<Value>(data).ok()
            })
            .collect();

        assert!(events
            .iter()
            .any(|e| e.get("type").and_then(|v| v.as_str()) == Some("error")));
        assert!(!events
            .iter()
            .any(|e| e.get("type").and_then(|v| v.as_str()) == Some("message_delta")));
        assert!(!events
            .iter()
            .any(|e| e.get("type").and_then(|v| v.as_str()) == Some("message_stop")));
    }
}
