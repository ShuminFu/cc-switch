//! OpenAI Chat Completions SSE → [`StreamEvent`]（Codex Responses 客户端）
//!
//! 与 Anthropic 客户端使用的 [`super::OpenAiChatEventDecoder`] 不同，这里沿用旧
//! `streaming_codex_chat` 的规则，Responses 编码端的输出逐字节相同：
//! - 整个回复只有一个文本块与一个 reasoning 块；任何文本或工具调用都会关闭 reasoning 块，
//!   之后到达的 reasoning 仍追加到原块；文本块在结束时才关闭；
//! - 内容开头的 `<think>…</think>` 拆成 reasoning 与正文（流式缓冲到能判断为止）；
//! - 工具调用按 Chat 的 `index` 依次开始：拿到 id 与 name 后才开始，较小的 index 没开始前
//!   较大的不开始，避免迟到的身份信息打乱顺序；结束时补发仍没开始的调用（缺 id 时为
//!   `call_{index}`，缺 name 的调用丢弃）；
//! - 结束事件推迟到 `[DONE]` 或流末尾；上游错误（`event: error` 或带 `error` 字段的 chunk）
//!   立即以 [`StreamEvent::Error`] 终止；没有 finish_reason 就结束时，有部分输出按
//!   `length` 收尾，否则报 `stream_truncated` 错误。

use std::collections::BTreeMap;

use serde_json::Value;

use crate::proxy::codec::{CodecError, EventDecoder};
use crate::proxy::protocol::ir::{Part, PartDelta, StreamEvent};
use crate::proxy::protocol::StopReason;
use crate::proxy::providers::codex_chat_common::{
    extract_reasoning_field_text, split_leading_think_block, strip_leading_think_open_tag,
};
use crate::proxy::usage::ir::UsageIR;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum ThinkMode {
    #[default]
    Detecting,
    Reasoning,
    Text,
}

enum ThinkPrefix {
    NeedMore,
    Reasoning,
    Text,
}

fn think_prefix(buffer: &str) -> ThinkPrefix {
    let trimmed = buffer.trim_start();
    if trimmed.is_empty() || "<think>".starts_with(trimmed) && !trimmed.starts_with("<think>") {
        return ThinkPrefix::NeedMore;
    }
    if trimmed.starts_with("<think>") {
        ThinkPrefix::Reasoning
    } else {
        ThinkPrefix::Text
    }
}

/// 文本 / reasoning 块的状态
#[derive(Debug, Default)]
struct BlockState {
    index: Option<usize>,
    text: String,
    done: bool,
}

#[derive(Debug, Default)]
struct ToolState {
    index: Option<usize>,
    call_id: String,
    name: String,
    arguments: String,
    done: bool,
}

/// Chat Completions SSE 解码状态机（Codex 客户端）
#[derive(Debug, Default)]
pub struct OpenAiChatResponsesDecoder {
    started: bool,
    completed: bool,
    failed: bool,
    next_index: usize,
    text: BlockState,
    reasoning: BlockState,
    think_mode: ThinkMode,
    think_buffer: String,
    tools: BTreeMap<usize, ToolState>,
    next_tool_to_start: usize,
    closed_items: usize,
    finish_reason: Option<String>,
}

impl OpenAiChatResponsesDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    fn allocate_index(&mut self) -> usize {
        let index = self.next_index;
        self.next_index += 1;
        index
    }

    fn reasoning_delta(&mut self, delta: &str, events: &mut Vec<StreamEvent>) {
        let index = match self.reasoning.index {
            Some(index) => index,
            None => {
                let index = self.allocate_index();
                self.reasoning.index = Some(index);
                events.push(StreamEvent::PartStart {
                    index,
                    part: Part::Reasoning {
                        text: String::new(),
                        signature: None,
                    },
                });
                index
            }
        };
        self.reasoning.text.push_str(delta);
        events.push(StreamEvent::PartDelta {
            index,
            delta: PartDelta::Reasoning(delta.to_string()),
        });
    }

    fn text_delta(&mut self, delta: &str, events: &mut Vec<StreamEvent>) {
        let index = match self.text.index {
            Some(index) => index,
            None => {
                let index = self.allocate_index();
                self.text.index = Some(index);
                events.push(StreamEvent::PartStart {
                    index,
                    part: Part::text(""),
                });
                index
            }
        };
        self.text.text.push_str(delta);
        events.push(StreamEvent::PartDelta {
            index,
            delta: PartDelta::Text(delta.to_string()),
        });
    }

    fn close_reasoning(&mut self, events: &mut Vec<StreamEvent>) {
        if let Some(index) = self.reasoning.index.filter(|_| !self.reasoning.done) {
            self.reasoning.done = true;
            self.closed_items += 1;
            events.push(StreamEvent::PartStop { index });
        }
    }

    fn close_text(&mut self, events: &mut Vec<StreamEvent>) {
        if let Some(index) = self.text.index.filter(|_| !self.text.done) {
            self.text.done = true;
            self.closed_items += 1;
            events.push(StreamEvent::PartStop { index });
        }
    }

    fn content_delta(&mut self, delta: &str, events: &mut Vec<StreamEvent>) {
        match self.think_mode {
            ThinkMode::Text => {
                self.close_reasoning(events);
                self.text_delta(delta, events);
            }
            ThinkMode::Detecting => {
                self.think_buffer.push_str(delta);
                match think_prefix(&self.think_buffer) {
                    ThinkPrefix::NeedMore => {}
                    ThinkPrefix::Reasoning => {
                        self.think_mode = ThinkMode::Reasoning;
                        self.drain_think_block(events);
                    }
                    ThinkPrefix::Text => {
                        self.think_mode = ThinkMode::Text;
                        let text = std::mem::take(&mut self.think_buffer);
                        self.close_reasoning(events);
                        self.text_delta(&text, events);
                    }
                }
            }
            ThinkMode::Reasoning => {
                self.think_buffer.push_str(delta);
                self.drain_think_block(events);
            }
        }
    }

    /// 缓冲中已有完整的 `<think>…</think>` 时拆成 reasoning 与正文
    fn drain_think_block(&mut self, events: &mut Vec<StreamEvent>) {
        let Some((reasoning, answer)) = split_leading_think_block(&self.think_buffer) else {
            return;
        };
        self.think_mode = ThinkMode::Text;
        self.think_buffer.clear();
        if !reasoning.is_empty() {
            self.reasoning_delta(&reasoning, events);
            self.close_reasoning(events);
        }
        if !answer.is_empty() {
            self.text_delta(&answer, events);
        }
    }

    /// 工具调用或结束前：把仍在缓冲的内容按目前能判断的类型发出
    fn flush_think_at_boundary(&mut self, events: &mut Vec<StreamEvent>) {
        match self.think_mode {
            ThinkMode::Text => {}
            ThinkMode::Detecting => {
                self.think_mode = ThinkMode::Text;
                let text = std::mem::take(&mut self.think_buffer);
                if !text.is_empty() {
                    self.close_reasoning(events);
                    self.text_delta(&text, events);
                }
            }
            ThinkMode::Reasoning => {
                let buffered = std::mem::take(&mut self.think_buffer);
                self.think_mode = ThinkMode::Text;
                if let Some((reasoning, answer)) = split_leading_think_block(&buffered) {
                    if !reasoning.is_empty() {
                        self.reasoning_delta(&reasoning, events);
                        self.close_reasoning(events);
                    }
                    if !answer.is_empty() {
                        self.text_delta(&answer, events);
                    }
                    return;
                }
                let reasoning = strip_leading_think_open_tag(&buffered).unwrap_or(buffered);
                if !reasoning.is_empty() {
                    self.reasoning_delta(&reasoning, events);
                    self.close_reasoning(events);
                }
            }
        }
    }

    fn tool_call_delta(&mut self, call: &Value, events: &mut Vec<StreamEvent>) {
        let chat_index = call.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
        let function = call.get("function").unwrap_or(&Value::Null);
        let arguments = function
            .get("arguments")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let state = self.tools.entry(chat_index).or_default();
        if let Some(id) = call
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
        {
            state.call_id = id.to_string();
        }
        if let Some(name) = function
            .get("name")
            .and_then(Value::as_str)
            .filter(|name| !name.is_empty())
        {
            state.name = name.to_string();
        }
        state.arguments.push_str(arguments);
        if let (Some(index), false) = (state.index, arguments.is_empty()) {
            events.push(StreamEvent::PartDelta {
                index,
                delta: PartDelta::ToolArguments(arguments.to_string()),
            });
        }
        self.start_ready_tools(events);
    }

    /// 依次开始 id 与 name 齐全的工具调用，迟到的身份信息不会打乱顺序
    fn start_ready_tools(&mut self, events: &mut Vec<StreamEvent>) {
        loop {
            let key = self.next_tool_to_start;
            let Some(state) = self.tools.get(&key) else {
                break;
            };
            if state.index.is_some() || state.done {
                self.next_tool_to_start += 1;
                continue;
            }
            if state.call_id.is_empty() || state.name.is_empty() {
                break;
            }
            let index = self.allocate_index();
            let Some(state) = self.tools.get_mut(&key) else {
                break;
            };
            state.index = Some(index);
            events.push(StreamEvent::PartStart {
                index,
                part: Part::ToolCall {
                    id: state.call_id.clone(),
                    name: state.name.clone(),
                    arguments: String::new(),
                },
            });
            if !state.arguments.is_empty() {
                events.push(StreamEvent::PartDelta {
                    index,
                    delta: PartDelta::ToolArguments(state.arguments.clone()),
                });
            }
            self.next_tool_to_start += 1;
        }
    }

    fn finalize_tools(&mut self, events: &mut Vec<StreamEvent>) {
        let keys: Vec<usize> = self.tools.keys().copied().collect();
        for key in keys {
            let late_start = match self.tools.get(&key) {
                Some(state) if state.done => continue,
                // 缺 name 的工具调用无法还原，丢弃
                Some(state) if state.name.is_empty() => {
                    log::warn!("[Codex] Skipping streaming tool call with missing name");
                    None
                }
                Some(state) => Some(state.index.is_none()),
                None => continue,
            };
            let Some(late_start) = late_start else {
                if let Some(state) = self.tools.get_mut(&key) {
                    state.done = true;
                }
                continue;
            };
            let next_index = if late_start {
                Some(self.allocate_index())
            } else {
                None
            };
            let Some(state) = self.tools.get_mut(&key) else {
                continue;
            };
            if let Some(index) = next_index {
                if state.call_id.is_empty() {
                    state.call_id = format!("call_{key}");
                }
                state.index = Some(index);
                // 开始时携带已累积的参数（不作为增量发出）
                events.push(StreamEvent::PartStart {
                    index,
                    part: Part::ToolCall {
                        id: state.call_id.clone(),
                        name: state.name.clone(),
                        arguments: state.arguments.clone(),
                    },
                });
            }
            state.done = true;
            self.closed_items += 1;
            if let Some(index) = state.index {
                events.push(StreamEvent::PartStop { index });
            }
        }
    }

    fn ensure_started(&mut self, chunk: &Value, events: &mut Vec<StreamEvent>) {
        if self.started {
            return;
        }
        self.started = true;
        events.push(StreamEvent::MessageStart {
            id: chunk
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            model: chunk
                .get("model")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            usage: None,
            created: chunk.get("created").and_then(Value::as_u64),
        });
    }

    fn finalize(&mut self, events: &mut Vec<StreamEvent>) {
        if self.completed {
            return;
        }
        if !self.started {
            self.ensure_started(&Value::Null, events);
        }
        self.flush_think_at_boundary(events);
        self.close_reasoning(events);
        self.close_text(events);
        self.finalize_tools(events);
        events.push(StreamEvent::MessageStop {
            stop_reason: Some(match self.finish_reason.as_deref() {
                Some("length") => StopReason::MaxTokens,
                _ => StopReason::EndTurn,
            }),
        });
        self.completed = true;
    }

    fn chunk(&mut self, chunk: &Value, events: &mut Vec<StreamEvent>) {
        self.ensure_started(chunk, events);

        if let Some(usage) = chunk.get("usage").filter(|usage| !usage.is_null()) {
            events.push(StreamEvent::Usage(if usage.is_object() {
                UsageIR::from_chat_completions(usage)
            } else {
                UsageIR::default()
            }));
        }

        let Some(choice) = chunk
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|choices| choices.first())
        else {
            return;
        };

        if let Some(delta) = choice.get("delta") {
            if let Some(reasoning) = extract_reasoning_field_text(delta) {
                self.reasoning_delta(&reasoning, events);
            }
            if let Some(content) = delta
                .get("content")
                .and_then(Value::as_str)
                .filter(|content| !content.is_empty())
            {
                self.content_delta(content, events);
            }
            if let Some(calls) = delta.get("tool_calls").and_then(Value::as_array) {
                self.flush_think_at_boundary(events);
                self.close_reasoning(events);
                for call in calls {
                    self.tool_call_delta(call, events);
                }
            }
        }

        if let Some(finish_reason) = choice.get("finish_reason").and_then(Value::as_str) {
            self.finish_reason = Some(finish_reason.to_string());
        }
    }

    /// 是否已有部分输出（用于区分「截断但有输出」与「什么都没有」）
    fn has_substantive_output(&self) -> bool {
        !self.text.text.trim().is_empty()
            || !self.reasoning.text.trim().is_empty()
            || !self.think_buffer.trim().is_empty()
            || self.closed_items > 0
            || self.tools.values().any(|state| {
                state.index.is_some()
                    || !state.call_id.trim().is_empty()
                    || !state.name.trim().is_empty()
                    || !state.arguments.trim().is_empty()
            })
    }
}

/// Chat SSE 错误（`error` 字段或整个 payload）的 message 与类型
fn error_details(value: &Value) -> (String, Option<String>) {
    let error = value.get("error").unwrap_or(value);
    let message = error
        .as_str()
        .map(str::to_string)
        .or_else(|| {
            error
                .get("message")
                .or_else(|| error.get("detail"))
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_else(|| error.to_string());
    let code = error
        .get("type")
        .or_else(|| error.get("code"))
        .and_then(Value::as_str)
        .map(str::to_string);
    (message, code)
}

impl EventDecoder for OpenAiChatResponsesDecoder {
    fn decode(&mut self, event: &str, data: &str) -> Result<Vec<StreamEvent>, CodecError> {
        let mut events = Vec::new();
        // 结束（或报错）之后的数据（如重复的 [DONE]）全部忽略
        if self.completed || self.failed {
            return Ok(events);
        }
        if data.trim() == "[DONE]" {
            self.finalize(&mut events);
            return Ok(events);
        }
        let Ok(chunk) = serde_json::from_str::<Value>(data) else {
            return Ok(events);
        };
        if event.trim() == "error" || chunk.get("error").is_some() {
            let (message, code) = error_details(&chunk);
            events.push(StreamEvent::Error { message, code });
            self.failed = true;
            return Ok(events);
        }
        self.chunk(&chunk, &mut events);
        Ok(events)
    }

    fn finish(&mut self) -> Result<Vec<StreamEvent>, CodecError> {
        let mut events = Vec::new();
        if self.completed || self.failed {
            return Ok(events);
        }
        if self.finish_reason.is_some() {
            self.finalize(&mut events);
        } else if self.has_substantive_output() {
            self.finish_reason = Some("length".to_string());
            self.finalize(&mut events);
        } else {
            events.push(StreamEvent::Error {
                message: "Upstream Chat Completions stream ended before sending finish_reason"
                    .to_string(),
                code: Some("stream_truncated".to_string()),
            });
            self.failed = true;
        }
        Ok(events)
    }
}
