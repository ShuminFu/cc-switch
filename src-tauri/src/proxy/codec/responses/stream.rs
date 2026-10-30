//! OpenAI Responses SSE → [`StreamEvent`]（Responses 作为上游）
//!
//! Responses 是命名事件的生命周期模型（`response.created` → `output_item.added` →
//! `content_part.added` → `output_text.delta` → … → `response.completed`）。状态机与旧
//! 转换器（`providers::streaming_responses`）一致，块序号按出现顺序分配，Anthropic
//! 编码端的输出逐字节相同：
//! - 文本 / refusal 共用一个「当前文本块」，按 `item_id` + `content_index`（或
//!   `output_index` + `content_index`）定位；工具块与 reasoning 块按 `item_id` 定位，
//!   缺失时依次回退到 `output_index` 与最近的工具块；
//! - 没有显式 `message_start` 的流，在第一个需要它的事件前补发；
//! - `Read` 工具的参数缓存到结束时清理空 `pages` 后一次发出；网关只在 done 事件里
//!   给出完整参数时补发参数；
//! - reasoning 块在 `output_item.done` 带来加密内容时补发签名（整个条目的信封），
//!   只有加密内容的条目成为 redacted_thinking；
//! - `response.failed` / `error` / 状态为失败的完成事件以 [`StreamEvent::Error`] 终止，
//!   之后的事件全部忽略；没有终止事件就 EOF 时，只有文本输出按 `max_tokens` 截断收尾，
//!   工具参数或 reasoning 可能不完整则报 `stream_truncated` 错误。

use std::collections::{HashMap, HashSet};

use serde_json::Value;

use super::response::usage;
use super::sanitize_anthropic_tool_use_input_json;
use crate::proxy::codec::{CodecError, EventDecoder};
use crate::proxy::protocol::ir::{Part, PartDelta, StreamEvent};
use crate::proxy::protocol::StopReason;
use crate::proxy::providers::reasoning_bridge::{
    encode_openai_reasoning_item, reasoning_summary_text,
};
use crate::proxy::usage::ir::UsageIR;

fn response_object(data: &Value) -> &Value {
    data.get("response").unwrap_or(data)
}

fn str_at<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

fn content_part_key(data: &Value) -> Option<String> {
    let content_index = data.get("content_index").and_then(Value::as_u64)?;
    if let Some(item_id) = str_at(data, "item_id") {
        return Some(format!("part:{item_id}:{content_index}"));
    }
    data.get("output_index")
        .and_then(Value::as_u64)
        .map(|output_index| format!("part:out:{output_index}:{content_index}"))
}

fn tool_key_from_event(data: &Value) -> Option<String> {
    if let Some(item_id) = str_at(data, "item_id") {
        return Some(format!("tool:{item_id}"));
    }
    data.get("output_index")
        .and_then(Value::as_u64)
        .map(|output_index| format!("tool:out:{output_index}"))
}

fn tool_key_from_added(data: &Value, item: &Value) -> Option<String> {
    match str_at(item, "id") {
        Some(item_id) => Some(format!("tool:{item_id}")),
        None => tool_key_from_event(data),
    }
}

fn reasoning_key(data: &Value, item: Option<&Value>) -> Option<String> {
    if let Some(item_id) = item
        .and_then(|item| str_at(item, "id"))
        .or_else(|| str_at(data, "item_id"))
    {
        return Some(format!("reasoning:{item_id}"));
    }
    data.get("output_index")
        .and_then(Value::as_u64)
        .map(|index| format!("reasoning:out:{index}"))
}

/// 上游错误事件的 message 与类型
fn error_details(data: &Value, fallback: &str) -> (String, String) {
    let response = response_object(data);
    let error = response.get("error").unwrap_or(response);
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .or_else(|| error.as_str())
        .filter(|message| !message.trim().is_empty())
        .unwrap_or(fallback)
        .to_string();
    let code = error
        .get("type")
        .and_then(Value::as_str)
        .or_else(|| error.get("code").and_then(Value::as_str))
        .unwrap_or("upstream_error")
        .to_string();
    (message, code)
}

fn empty_reasoning() -> Part {
    Part::Reasoning {
        text: String::new(),
        signature: None,
    }
}

/// Responses SSE 解码状态机
#[derive(Debug, Default)]
pub struct ResponsesEventDecoder {
    message_id: Option<String>,
    model: Option<String>,
    started: bool,
    has_tool_use: bool,
    next_index: usize,
    index_by_key: HashMap<String, usize>,
    open: HashSet<usize>,
    fallback_open_index: Option<usize>,
    current_text_index: Option<usize>,
    tool_index_by_item_id: HashMap<String, usize>,
    tool_name_by_index: HashMap<usize, String>,
    tool_args_by_index: HashMap<usize, String>,
    tool_had_delta: HashSet<usize>,
    last_tool_index: Option<usize>,
    reasoning_index_by_item_id: HashMap<String, usize>,
    reasoning_item_by_index: HashMap<usize, Value>,
    reasoning_text_by_index: HashMap<usize, String>,
    legacy_reasoning_index: Option<usize>,
    has_substantive_output: bool,
    terminated: bool,
}

impl ResponsesEventDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    fn allocate_index(&mut self) -> usize {
        let index = self.next_index;
        self.next_index += 1;
        index
    }

    /// 按键查找或分配块序号
    fn index_for_key(&mut self, key: Option<String>) -> usize {
        match key {
            Some(key) => match self.index_by_key.get(&key) {
                Some(index) => *index,
                None => {
                    let index = self.allocate_index();
                    self.index_by_key.insert(key, index);
                    index
                }
            },
            None => self.allocate_index(),
        }
    }

    fn message_start(&mut self, usage: Option<UsageIR>) -> StreamEvent {
        self.started = true;
        StreamEvent::MessageStart {
            id: self.message_id.clone().unwrap_or_default(),
            model: self.model.clone().unwrap_or_default(),
            usage,
            created: None,
        }
    }

    fn ensure_started(&mut self, events: &mut Vec<StreamEvent>) {
        if !self.started {
            events.push(self.message_start(None));
        }
    }

    /// 当前文本块的序号（必要时按 content part 键分配）
    fn text_index(&mut self, data: &Value) -> usize {
        if let Some(index) = self.current_text_index {
            return index;
        }
        let index = match content_part_key(data) {
            Some(key) => self.index_for_key(Some(key)),
            None => match self.fallback_open_index {
                Some(index) => index,
                None => {
                    let index = self.allocate_index();
                    self.fallback_open_index = Some(index);
                    index
                }
            },
        };
        self.current_text_index = Some(index);
        index
    }

    fn open_part(&mut self, index: usize, part: Part, events: &mut Vec<StreamEvent>) {
        events.push(StreamEvent::PartStart { index, part });
        self.open.insert(index);
    }

    fn close_part(&mut self, index: usize, events: &mut Vec<StreamEvent>) -> bool {
        let was_open = self.open.remove(&index);
        if was_open {
            events.push(StreamEvent::PartStop { index });
        }
        was_open
    }

    /// 关闭当前文本块（工具 / reasoning 块开始前，或文本结束时）
    fn close_text(&mut self, events: &mut Vec<StreamEvent>) {
        if let Some(index) = self.current_text_index.take() {
            self.close_part(index, events);
            if self.fallback_open_index == Some(index) {
                self.fallback_open_index = None;
            }
        }
    }

    fn text_delta(&mut self, data: &Value, events: &mut Vec<StreamEvent>) {
        let Some(delta) = str_at(data, "delta") else {
            return;
        };
        let index = self.text_index(data);
        if !self.open.contains(&index) {
            self.open_part(index, Part::text(""), events);
        }
        events.push(StreamEvent::PartDelta {
            index,
            delta: PartDelta::Text(delta.to_string()),
        });
    }

    fn tool_index_for_event(&self, data: &Value) -> Option<usize> {
        str_at(data, "item_id")
            .and_then(|id| self.tool_index_by_item_id.get(id).copied())
            .or_else(|| {
                tool_key_from_event(data).and_then(|key| self.index_by_key.get(&key).copied())
            })
            .or(self.last_tool_index)
    }

    fn reasoning_index_for_event(&self, data: &Value) -> Option<usize> {
        let item_id = str_at(data, "item_id");
        let key = reasoning_key(data, None);
        let keyless = item_id.is_none() && key.is_none();
        item_id
            .and_then(|id| self.reasoning_index_by_item_id.get(id).copied())
            .or_else(|| key.and_then(|key| self.index_by_key.get(&key).copied()))
            .or_else(|| keyless.then_some(self.legacy_reasoning_index).flatten())
    }

    fn forget_tool(&mut self, index: usize, item_id: Option<&str>) {
        if let Some(item_id) = item_id {
            self.tool_index_by_item_id.remove(item_id);
        }
        self.tool_name_by_index.remove(&index);
        self.tool_args_by_index.remove(&index);
        self.tool_had_delta.remove(&index);
    }

    fn output_item_added(&mut self, data: &Value, events: &mut Vec<StreamEvent>) {
        let Some(item) = data.get("item") else {
            return;
        };
        match str_at(item, "type").unwrap_or_default() {
            "function_call" => {
                self.has_tool_use = true;
                self.has_substantive_output = true;
                self.close_text(events);
                self.ensure_started(events);

                let index = self.index_for_key(tool_key_from_added(data, item));
                if let Some(item_id) = str_at(item, "id").or_else(|| str_at(data, "item_id")) {
                    self.tool_index_by_item_id
                        .insert(item_id.to_string(), index);
                }
                let name = str_at(item, "name").unwrap_or_default().to_string();
                self.tool_name_by_index.insert(index, name.clone());
                self.last_tool_index = Some(index);
                if self.open.contains(&index) {
                    return;
                }
                self.tool_args_by_index.insert(index, String::new());
                self.open_part(
                    index,
                    Part::ToolCall {
                        id: str_at(item, "call_id").unwrap_or_default().to_string(),
                        name,
                        arguments: String::new(),
                    },
                    events,
                );
            }
            "reasoning" => {
                self.ensure_started(events);
                let index = self.index_for_key(reasoning_key(data, Some(item)));
                if let Some(item_id) = str_at(item, "id").or_else(|| str_at(data, "item_id")) {
                    self.reasoning_index_by_item_id
                        .insert(item_id.to_string(), index);
                }
                self.reasoning_item_by_index.insert(index, item.clone());
                self.reasoning_text_by_index.entry(index).or_default();
            }
            // message 条目由 content_part.added 处理
            _ => {}
        }
    }

    fn arguments_delta(&mut self, data: &Value, events: &mut Vec<StreamEvent>) {
        let Some(delta) = str_at(data, "delta") else {
            return;
        };
        self.has_tool_use = true;
        let item_id = str_at(data, "item_id");
        let index = match self.tool_index_for_event(data) {
            Some(index) => index,
            None => self.allocate_index(),
        };
        if let Some(item_id) = item_id {
            self.tool_index_by_item_id
                .insert(item_id.to_string(), index);
        }
        match str_at(data, "name") {
            Some(name) => {
                self.tool_name_by_index.insert(index, name.to_string());
            }
            None => {
                self.tool_name_by_index.entry(index).or_default();
            }
        }
        self.last_tool_index = Some(index);

        if !self.open.contains(&index) {
            let part = Part::ToolCall {
                id: str_at(data, "call_id")
                    .or(item_id)
                    .unwrap_or_default()
                    .to_string(),
                name: str_at(data, "name").unwrap_or_default().to_string(),
                arguments: String::new(),
            };
            self.open_part(index, part, events);
        }
        self.tool_args_by_index
            .entry(index)
            .or_default()
            .push_str(delta);
        self.tool_had_delta.insert(index);

        // Read 的参数在结束时清理后一次发出
        if self.tool_name_by_index.get(&index).map(String::as_str) == Some("Read") {
            return;
        }
        events.push(StreamEvent::PartDelta {
            index,
            delta: PartDelta::ToolArguments(delta.to_string()),
        });
    }

    fn arguments_done(&mut self, data: &Value, events: &mut Vec<StreamEvent>) {
        self.has_tool_use = true;
        let Some(index) = self.tool_index_for_event(data) else {
            return;
        };
        if !self.open.remove(&index) {
            return;
        }
        let done_arguments = data
            .get("arguments")
            .or_else(|| data.pointer("/item/arguments"))
            .and_then(Value::as_str);
        if self.tool_name_by_index.get(&index).map(String::as_str) == Some("Read") {
            let raw = done_arguments.map(str::to_string).unwrap_or_else(|| {
                self.tool_args_by_index
                    .get(&index)
                    .cloned()
                    .unwrap_or_default()
            });
            let sanitized = sanitize_anthropic_tool_use_input_json("Read", &raw);
            if !sanitized.is_empty() {
                events.push(StreamEvent::PartDelta {
                    index,
                    delta: PartDelta::ToolArguments(sanitized),
                });
            }
        } else if !self.tool_had_delta.contains(&index) {
            // 部分兼容网关不发参数增量，只在 done 事件里给出完整参数
            if let Some(arguments) = done_arguments.filter(|value| !value.is_empty()) {
                events.push(StreamEvent::PartDelta {
                    index,
                    delta: PartDelta::ToolArguments(arguments.to_string()),
                });
            }
        }
        events.push(StreamEvent::PartStop { index });
        self.forget_tool(index, str_at(data, "item_id"));
    }

    fn refusal_done(&mut self, data: &Value, events: &mut Vec<StreamEvent>) {
        let index = self
            .current_text_index
            .take()
            .or_else(|| match content_part_key(data) {
                Some(key) => self.index_by_key.get(&key).copied(),
                None => self.fallback_open_index,
            });
        if let Some(index) = index {
            if self.close_part(index, events) && self.fallback_open_index == Some(index) {
                self.fallback_open_index = None;
            }
        }
    }

    fn reasoning_delta(&mut self, data: &Value, events: &mut Vec<StreamEvent>) {
        let Some(delta) = data
            .get("delta")
            .or_else(|| data.get("text"))
            .and_then(Value::as_str)
        else {
            return;
        };
        self.close_text(events);
        let item_id = str_at(data, "item_id");
        let key = reasoning_key(data, None);
        let keyless = item_id.is_none() && key.is_none();
        let index = match self.reasoning_index_for_event(data) {
            Some(index) => index,
            None => {
                let index = self.allocate_index();
                if let Some(key) = key {
                    self.index_by_key.insert(key, index);
                }
                if let Some(item_id) = item_id {
                    self.reasoning_index_by_item_id
                        .insert(item_id.to_string(), index);
                } else if keyless {
                    self.legacy_reasoning_index = Some(index);
                }
                index
            }
        };
        if !self.open.contains(&index) {
            self.open_part(index, empty_reasoning(), events);
        }
        self.reasoning_text_by_index
            .entry(index)
            .or_default()
            .push_str(delta);
        events.push(StreamEvent::PartDelta {
            index,
            delta: PartDelta::Reasoning(delta.to_string()),
        });
    }

    /// 官方 done 事件带完整可见文本；网关省略了增量时在这里补发。块保持打开，
    /// 等 `output_item.done` 带来加密内容
    fn reasoning_text_done(&mut self, data: &Value, events: &mut Vec<StreamEvent>) {
        let Some(index) = self.reasoning_index_for_event(data) else {
            return;
        };
        let already_emitted = self
            .reasoning_text_by_index
            .get(&index)
            .is_some_and(|text| !text.is_empty());
        if already_emitted {
            return;
        }
        let Some(text) = str_at(data, "text").filter(|text| !text.is_empty()) else {
            return;
        };
        if !self.open.contains(&index) {
            self.open_part(index, empty_reasoning(), events);
        }
        self.reasoning_text_by_index
            .entry(index)
            .or_default()
            .push_str(text);
        events.push(StreamEvent::PartDelta {
            index,
            delta: PartDelta::Reasoning(text.to_string()),
        });
    }

    /// 旧网关不发 `output_item.done`，保留其非标准 done 事件的关闭行为
    fn legacy_reasoning_done(&mut self, data: &Value, events: &mut Vec<StreamEvent>) {
        if let Some(index) = self.reasoning_index_for_event(data) {
            self.close_part(index, events);
            if self.legacy_reasoning_index == Some(index) {
                self.legacy_reasoning_index = None;
            }
        }
    }

    fn output_item_done(&mut self, data: &Value, events: &mut Vec<StreamEvent>) {
        let Some(item) = data.get("item") else {
            return;
        };
        let item_id = str_at(item, "id").or_else(|| str_at(data, "item_id"));
        match str_at(item, "type") {
            Some("function_call") => {
                self.has_tool_use = true;
                let index = item_id
                    .and_then(|id| self.tool_index_by_item_id.get(id).copied())
                    .or_else(|| {
                        tool_key_from_event(data)
                            .and_then(|key| self.index_by_key.get(&key).copied())
                    })
                    .or(self.last_tool_index);
                let Some(index) = index.filter(|index| self.open.contains(index)) else {
                    return;
                };
                let name = self
                    .tool_name_by_index
                    .get(&index)
                    .cloned()
                    .unwrap_or_default();
                if !self.tool_had_delta.contains(&index) || name == "Read" {
                    let raw = str_at(item, "arguments")
                        .filter(|value| !value.is_empty())
                        .map(str::to_string)
                        .unwrap_or_else(|| {
                            self.tool_args_by_index
                                .get(&index)
                                .cloned()
                                .unwrap_or_default()
                        });
                    let arguments = if name == "Read" {
                        sanitize_anthropic_tool_use_input_json(&name, &raw)
                    } else {
                        raw
                    };
                    if !arguments.is_empty() {
                        events.push(StreamEvent::PartDelta {
                            index,
                            delta: PartDelta::ToolArguments(arguments),
                        });
                    }
                }
                self.open.remove(&index);
                events.push(StreamEvent::PartStop { index });
                self.forget_tool(index, item_id);
            }
            Some("reasoning") => {
                let index = match item_id
                    .and_then(|id| self.reasoning_index_by_item_id.get(id).copied())
                    .or_else(|| {
                        reasoning_key(data, Some(item))
                            .and_then(|key| self.index_by_key.get(&key).copied())
                    }) {
                    Some(index) => index,
                    None => self.allocate_index(),
                };
                let full_text = reasoning_summary_text(item);
                let emitted = self
                    .reasoning_text_by_index
                    .get(&index)
                    .is_some_and(|text| !text.is_empty());
                if !emitted && !full_text.is_empty() {
                    self.open_part(index, empty_reasoning(), events);
                    events.push(StreamEvent::PartDelta {
                        index,
                        delta: PartDelta::Reasoning(full_text),
                    });
                }

                let encrypted = str_at(item, "encrypted_content").is_some_and(|v| !v.is_empty());
                if let Some(envelope) = encrypted
                    .then(|| encode_openai_reasoning_item(item))
                    .flatten()
                {
                    if self.open.contains(&index) {
                        events.push(StreamEvent::PartDelta {
                            index,
                            delta: PartDelta::Signature(envelope),
                        });
                    } else {
                        self.open_part(index, Part::RedactedReasoning { data: envelope }, events);
                    }
                }
                self.close_part(index, events);
                if let Some(item_id) = item_id {
                    self.reasoning_index_by_item_id.remove(item_id);
                }
                self.reasoning_item_by_index.remove(&index);
                self.reasoning_text_by_index.remove(&index);
            }
            _ => {}
        }
    }

    fn terminal(&mut self, event_name: &str, data: &Value, events: &mut Vec<StreamEvent>) {
        let response = response_object(data);
        let failed = matches!(str_at(response, "status"), Some("failed" | "cancelled"))
            || response.get("error").is_some_and(|error| !error.is_null());
        if failed {
            let (message, code) = error_details(
                data,
                "Responses upstream returned a failed terminal response",
            );
            events.push(StreamEvent::Error {
                message,
                code: Some(code),
            });
            self.terminated = true;
            return;
        }
        if !self.started {
            if let Some(id) = str_at(response, "id") {
                self.message_id = Some(id.to_string());
            }
            if let Some(model) = str_at(response, "model") {
                self.model = Some(model.to_string());
            }
            events.push(self.message_start(None));
        }
        let status = str_at(response, "status").unwrap_or(match event_name {
            "response.incomplete" => "incomplete",
            _ => "completed",
        });
        let stop_reason = StopReason::from_responses_status(
            status,
            self.has_tool_use,
            response
                .pointer("/incomplete_details/reason")
                .and_then(Value::as_str),
        );

        // 尽力关闭仍打开的块
        let mut remaining: Vec<usize> = self.open.drain().collect();
        remaining.sort_unstable();
        events.extend(
            remaining
                .into_iter()
                .map(|index| StreamEvent::PartStop { index }),
        );
        self.fallback_open_index = None;

        events.push(StreamEvent::Usage(usage(Some(
            response.get("usage").unwrap_or(&Value::Null),
        ))));
        events.push(StreamEvent::MessageStop {
            stop_reason: Some(stop_reason),
        });
        self.terminated = true;
    }

    fn handle(&mut self, event_name: &str, data: &Value, events: &mut Vec<StreamEvent>) {
        // 终止之后的事件全部忽略（尤其不能为失败后的迟到增量补发 message_start）
        if self.terminated {
            return;
        }

        let needs_message_start = matches!(
            event_name,
            "response.output_text.delta"
                | "response.refusal.delta"
                | "response.function_call_arguments.delta"
                | "response.reasoning_summary_text.delta"
                | "response.reasoning_text.delta"
                | "response.reasoning.delta"
        );
        if needs_message_start {
            self.has_substantive_output = true;
            self.ensure_started(events);
        }

        match event_name {
            "response.created" => {
                let response = response_object(data);
                if let Some(id) = str_at(response, "id") {
                    self.message_id = Some(id.to_string());
                }
                if let Some(model) = str_at(response, "model") {
                    self.model = Some(model.to_string());
                }
                let start_usage = usage(Some(response.get("usage").unwrap_or(&Value::Null)));
                events.push(self.message_start(Some(start_usage)));
            }
            "response.content_part.added" => {
                self.ensure_started(events);
                let is_text = matches!(
                    data.pointer("/part/type").and_then(Value::as_str),
                    Some("output_text" | "refusal")
                );
                if is_text {
                    let index = self.text_index(data);
                    if !self.open.contains(&index) {
                        self.open_part(index, Part::text(""), events);
                    }
                }
            }
            "response.output_text.delta" | "response.refusal.delta" => {
                self.text_delta(data, events)
            }
            "response.output_item.added" => self.output_item_added(data, events),
            "response.function_call_arguments.delta" => self.arguments_delta(data, events),
            "response.function_call_arguments.done" => self.arguments_done(data, events),
            "response.refusal.done" => self.refusal_done(data, events),
            "response.reasoning_summary_text.delta"
            | "response.reasoning_text.delta"
            | "response.reasoning.delta" => self.reasoning_delta(data, events),
            "response.reasoning_summary_text.done" | "response.reasoning_text.done" => {
                self.reasoning_text_done(data, events)
            }
            "response.reasoning.done" => self.legacy_reasoning_done(data, events),
            "response.completed" | "response.incomplete" => self.terminal(event_name, data, events),
            // HTTP 2xx 的 SSE 里也可能带语义失败，保留上游细节而不是静默结束
            "response.failed" | "error" => {
                let fallback = if event_name == "response.failed" {
                    "Responses upstream reported response.failed"
                } else {
                    "Responses upstream emitted an error event"
                };
                let (message, code) = error_details(data, fallback);
                events.push(StreamEvent::Error {
                    message,
                    code: Some(code),
                });
                self.terminated = true;
            }
            "response.output_text.done" => self.close_text(events),
            "response.output_item.done" => self.output_item_done(data, events),
            // 不需要对应事件的生命周期事件与未知事件
            _ => {}
        }
    }
}

impl EventDecoder for ResponsesEventDecoder {
    fn decode(&mut self, event: &str, data: &str) -> Result<Vec<StreamEvent>, CodecError> {
        let mut events = Vec::new();
        let Ok(value) = serde_json::from_str::<Value>(data) else {
            return Ok(events);
        };
        // 官方流同时带 `event:` 行与 payload `type`；部分网关省略 `event:` 行
        let name = if event.is_empty() {
            str_at(&value, "type").unwrap_or_default().to_string()
        } else {
            event.to_string()
        };
        log::debug!("[Claude/Responses] <<< SSE event: {name}");
        self.handle(&name, &value, &mut events);
        Ok(events)
    }

    fn finish(&mut self) -> Result<Vec<StreamEvent>, CodecError> {
        let mut events = Vec::new();
        if self.terminated {
            return Ok(events);
        }
        self.terminated = true;
        let has_open_tool = self.open.iter().any(|index| {
            self.tool_name_by_index.contains_key(index)
                || self.tool_args_by_index.contains_key(index)
        });
        let has_open_reasoning = self.open.iter().any(|index| {
            self.reasoning_item_by_index.contains_key(index)
                || self.reasoning_text_by_index.contains_key(index)
                || self.legacy_reasoning_index == Some(*index)
        });
        if self.has_substantive_output && !has_open_tool && !has_open_reasoning {
            // 只有文本的部分输出可以安全地作为 max_tokens 截断的回合收尾
            let mut remaining: Vec<usize> = self.open.drain().collect();
            remaining.sort_unstable();
            events.extend(
                remaining
                    .into_iter()
                    .map(|index| StreamEvent::PartStop { index }),
            );
            self.ensure_started(&mut events);
            events.push(StreamEvent::Usage(UsageIR::default()));
            events.push(StreamEvent::MessageStop {
                stop_reason: Some(StopReason::MaxTokens),
            });
        } else {
            // 截断的工具参数可能不完整，thinking 可能缺签名：不能安全收尾
            events.push(StreamEvent::Error {
                message: "Responses upstream stream ended before a terminal event".to_string(),
                code: Some("stream_truncated".to_string()),
            });
        }
        Ok(events)
    }
}
