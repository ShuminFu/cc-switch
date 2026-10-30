//! [`StreamEvent`] → OpenAI Responses SSE（Responses 作为客户端，Codex）
//!
//! 每个内容块对应一个输出条目：文本 → `message`，reasoning → `reasoning`，工具调用按
//! [`CodexToolContext`] 还原为 `function_call`（带 namespace）、`custom_tool_call` 或
//! `tool_search_call`。事件信封由 [`crate::proxy::providers::codex_responses_sse`] 统一构造。
//!
//! 块的初始内容（`PartStart` 中的文本、工具参数）计入条目但不作为增量发出；块关闭后
//! 到达的增量仍按原条目发出（旧 Chat 转换器在工具调用之后继续的 reasoning 就是这样）。
//!
//! 两个旧转换器的条目布局不同，迁移期间由 [`ResponsesFlavor`] 保留各自的形状：
//! - Anthropic 上游：每个块一个条目（id 带输出序号），thinking 只有带签名时才成为条目
//!   （整个块装进 `encrypted_content` 信封），`Read` 工具的参数清理空 `pages`，
//!   截断收尾时工具条目为 `incomplete` 且不发参数 done 事件；
//! - Chat 上游：整个回复一个文本条目与一个 reasoning 条目，工具条目带 `reasoning_content`，
//!   custom 工具在结束时补发输入增量，usage 按 Chat 的字段顺序输出。

use std::collections::HashMap;

use bytes::Bytes;
use serde_json::{json, Value};

use super::reasoning::responses_reasoning_item_from_anthropic_block;
use super::tools::{
    custom_tool_input_from_chat_arguments, response_tool_call_item_from_chat_name,
    response_tool_call_item_id_from_chat_name, CodexToolContext,
};
use super::{sanitize_anthropic_tool_use_input_json, ResponsesFlavor};
use crate::proxy::codec::EventEncoder;
use crate::proxy::json_canonical::canonicalize_tool_arguments_str;
use crate::proxy::protocol::ir::{Part, PartDelta, StreamEvent};
use crate::proxy::protocol::StopReason;
use crate::proxy::providers::codex_responses_sse as sse;
use crate::proxy::usage::ir::UsageIR;

/// 客户端看到的 response id：上游 id 加 `resp_` 前缀，缺失时为 `resp_ccswitch`
pub(super) fn response_id(id: &str) -> String {
    if id.is_empty() {
        "resp_ccswitch".to_string()
    } else if id.starts_with("resp_") {
        id.to_string()
    } else {
        format!("resp_{id}")
    }
}

/// [`StopReason`] → Responses `status` 与 `incomplete_details.reason`
pub(super) fn status(stop_reason: Option<StopReason>) -> (&'static str, Option<&'static str>) {
    match stop_reason {
        Some(StopReason::MaxTokens) => ("incomplete", Some("max_output_tokens")),
        // 安全拒绝报告为 incomplete，避免 Codex 把它当作正常完成的空回复
        Some(StopReason::Refusal) => ("incomplete", Some("content_filter")),
        _ => ("completed", None),
    }
}

/// Responses usage 对象
pub(super) fn usage_json(usage: Option<UsageIR>, flavor: ResponsesFlavor) -> Value {
    let usage = usage.unwrap_or_default();
    match flavor {
        ResponsesFlavor::AnthropicUpstream => usage.to_responses(),
        ResponsesFlavor::ChatUpstream => {
            // 旧 Chat 转换器的字段顺序（客户端快照依赖顺序）
            let responses = usage.to_responses();
            let mut result = json!({
                "input_tokens": responses["input_tokens"],
                "output_tokens": responses["output_tokens"],
                "total_tokens": responses["total_tokens"],
            });
            if let Some(details) = responses.get("input_tokens_details") {
                result["input_tokens_details"] = details.clone();
            }
            result["output_tokens_details"] = responses["output_tokens_details"].clone();
            if let Some(cache_write) = responses.get("cache_creation_input_tokens") {
                result["cache_creation_input_tokens"] = cache_write.clone();
            }
            result
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ItemKind {
    Text,
    Tool,
    Reasoning,
    RedactedReasoning,
}

#[derive(Debug)]
struct ItemState {
    kind: ItemKind,
    output_index: u32,
    item_id: String,
    call_id: String,
    name: String,
    accum: String,
    /// 工具调用开始时携带的参数（没有增量时使用）
    start_arguments: String,
    /// Chat 上游：工具条目附带的 reasoning
    reasoning_content: String,
    /// thinking 签名 / redacted_thinking 数据
    opaque: Option<String>,
    done: bool,
}

/// Responses SSE 编码状态机
#[derive(Debug)]
pub struct ResponsesEventEncoder {
    flavor: ResponsesFlavor,
    tool_context: CodexToolContext,
    started: bool,
    finished: bool,
    truncated: bool,
    response_id: String,
    model: String,
    created_at: u64,
    next_output_index: u32,
    items: HashMap<usize, ItemState>,
    output_items: Vec<(u32, Value)>,
    usage: Option<UsageIR>,
    /// Chat 上游：本次回复至今的全部 reasoning 文本
    reasoning_text: String,
}

impl ResponsesEventEncoder {
    pub fn new(flavor: ResponsesFlavor, tool_context: CodexToolContext) -> Self {
        Self {
            flavor,
            tool_context,
            started: false,
            finished: false,
            truncated: false,
            response_id: response_id(""),
            model: String::new(),
            created_at: 0,
            next_output_index: 0,
            items: HashMap::new(),
            output_items: Vec::new(),
            usage: None,
            reasoning_text: String::new(),
        }
    }

    fn chat(&self) -> bool {
        self.flavor == ResponsesFlavor::ChatUpstream
    }

    fn base_response(&self, status: &str, output: Vec<Value>) -> Value {
        json!({
            "id": self.response_id,
            "object": "response",
            "created_at": self.created_at,
            "status": status,
            "model": self.model,
            "output": output,
            "usage": usage_json(self.usage, self.flavor)
        })
    }

    fn sorted_output(&self) -> Vec<Value> {
        let mut output = self.output_items.clone();
        output.sort_by_key(|(output_index, _)| *output_index);
        output.into_iter().map(|(_, item)| item).collect()
    }

    fn ensure_started(&mut self, frames: &mut Vec<Bytes>) {
        if self.started {
            return;
        }
        self.started = true;
        let response = self.base_response("in_progress", Vec::new());
        frames.push(sse::response_created(&response));
        frames.push(sse::response_in_progress(&response));
    }

    fn next_output_index(&mut self) -> u32 {
        let index = self.next_output_index;
        self.next_output_index += 1;
        index
    }

    fn item_id(&self, prefix_kind: ItemKind, output_index: u32) -> String {
        match (prefix_kind, self.chat()) {
            (ItemKind::Text, true) => format!("{}_msg", self.response_id),
            (ItemKind::Text, false) => format!("{}_msg_{output_index}", self.response_id),
            (_, true) => format!("rs_{}", self.response_id),
            (_, false) => format!("rs_{}_{output_index}", self.response_id),
        }
    }

    fn reasoning_for_tools(&self, state: &ItemState) -> Option<String> {
        self.chat().then(|| state.reasoning_content.clone())
    }

    fn part_start(&mut self, index: usize, part: &Part, frames: &mut Vec<Bytes>) {
        let (kind, accum, call_id, name, start_arguments, opaque) = match part {
            Part::Text { text } => (ItemKind::Text, text.clone(), "", "", "", None),
            Part::ToolCall {
                id,
                name,
                arguments,
            } => (
                ItemKind::Tool,
                String::new(),
                id.as_str(),
                name.as_str(),
                arguments.as_str(),
                None,
            ),
            Part::Reasoning { text, signature } => (
                ItemKind::Reasoning,
                text.clone(),
                "",
                "",
                "",
                signature.clone(),
            ),
            Part::RedactedReasoning { data } => (
                ItemKind::RedactedReasoning,
                String::new(),
                "",
                "",
                "",
                Some(data.clone()),
            ),
            Part::Image { .. } | Part::Document { .. } | Part::ToolResult { .. } => {
                log::warn!("[Codec/Responses] Dropping a non-assistant content part from a stream");
                return;
            }
        };
        let output_index = self.next_output_index();
        let item_id = match kind {
            ItemKind::Tool => {
                response_tool_call_item_id_from_chat_name(call_id, name, &self.tool_context)
            }
            other => self.item_id(other, output_index),
        };
        let reasoning_content = if kind == ItemKind::Tool && self.chat() {
            self.reasoning_text.trim().to_string()
        } else {
            String::new()
        };
        let state = ItemState {
            kind,
            output_index,
            item_id,
            call_id: call_id.to_string(),
            name: name.to_string(),
            accum,
            start_arguments: start_arguments.to_string(),
            reasoning_content,
            opaque,
            done: false,
        };
        match kind {
            ItemKind::Text => {
                frames.push(sse::message_item_added(output_index, &state.item_id));
                frames.push(sse::message_content_part_added(
                    output_index,
                    &state.item_id,
                ));
            }
            ItemKind::Tool => {
                let item = response_tool_call_item_from_chat_name(
                    &state.item_id,
                    "in_progress",
                    &state.call_id,
                    &state.name,
                    "",
                    self.reasoning_for_tools(&state).as_deref(),
                    &self.tool_context,
                );
                frames.push(sse::output_item_added(output_index, &item));
            }
            ItemKind::Reasoning => {
                frames.push(sse::reasoning_item_added(output_index, &state.item_id));
                frames.push(sse::reasoning_summary_part_added(
                    output_index,
                    &state.item_id,
                ));
            }
            ItemKind::RedactedReasoning => {
                frames.push(sse::reasoning_item_added(output_index, &state.item_id));
            }
        }
        self.items.insert(index, state);
    }

    fn part_delta(&mut self, index: usize, delta: &PartDelta, frames: &mut Vec<Bytes>) {
        if let PartDelta::Reasoning(text) = delta {
            if self.chat() {
                self.reasoning_text.push_str(text);
                // Chat 上游：思考型模型要求带回工具调用前后的 reasoning
                if !text.trim().is_empty() {
                    for state in self
                        .items
                        .values_mut()
                        .filter(|state| state.kind == ItemKind::Tool && !state.done)
                    {
                        if state.reasoning_content.is_empty() {
                            state.reasoning_content = text.trim_start().to_string();
                        } else {
                            state.reasoning_content.push_str(text);
                        }
                    }
                }
            }
        }
        let chat = self.chat();
        let Some(state) = self.items.get_mut(&index) else {
            return;
        };
        match delta {
            PartDelta::Text(text) => {
                state.accum.push_str(text);
                frames.push(sse::output_text_delta(
                    state.output_index,
                    &state.item_id,
                    text,
                ));
            }
            PartDelta::ToolArguments(arguments) => {
                state.accum.push_str(arguments);
                // Read 的参数在结束时清理后再给出；custom 工具的输入在结束时给出
                let buffered = (!chat && state.name == "Read")
                    || (chat && arguments.is_empty())
                    || self.tool_context.is_custom_tool_chat_name(&state.name);
                if !buffered {
                    frames.push(sse::function_call_arguments_delta(
                        state.output_index,
                        &state.item_id,
                        arguments,
                    ));
                }
            }
            PartDelta::Reasoning(text) => {
                state.accum.push_str(text);
                frames.push(sse::reasoning_summary_text_delta(
                    state.output_index,
                    &state.item_id,
                    text,
                ));
            }
            PartDelta::Signature(signature) => state.opaque = Some(signature.clone()),
        }
    }

    fn part_stop(&mut self, index: usize, frames: &mut Vec<Bytes>) {
        let chat = self.chat();
        let truncated = self.truncated;
        let Some(state) = self.items.get_mut(&index) else {
            return;
        };
        if state.done {
            return;
        }
        state.done = true;
        let output_index = state.output_index;
        let item = match state.kind {
            ItemKind::Text => {
                let (events, item) = sse::message_close(output_index, &state.item_id, &state.accum);
                frames.extend(events);
                item
            }
            ItemKind::Tool => {
                let is_custom = self.tool_context.is_custom_tool_chat_name(&state.name);
                let arguments = if chat {
                    canonicalize_tool_arguments_str(&state.accum)
                } else {
                    // 优先用流式增量；没有增量时用开始事件携带的 input
                    let raw = if state.accum.trim().is_empty() {
                        state.start_arguments.as_str()
                    } else {
                        state.accum.as_str()
                    };
                    if raw.trim().is_empty() {
                        "{}".to_string()
                    } else if state.name == "Read" {
                        sanitize_anthropic_tool_use_input_json("Read", raw)
                    } else {
                        canonicalize_tool_arguments_str(raw)
                    }
                };
                let item = response_tool_call_item_from_chat_name(
                    &state.item_id,
                    if truncated { "incomplete" } else { "completed" },
                    &state.call_id,
                    &state.name,
                    &arguments,
                    chat.then_some(state.reasoning_content.as_str()),
                    &self.tool_context,
                );
                if chat && is_custom {
                    let input = custom_tool_input_from_chat_arguments(&arguments);
                    if !input.is_empty() {
                        frames.push(sse::custom_tool_call_input_delta(
                            output_index,
                            &state.item_id,
                            &input,
                        ));
                    }
                    frames.push(sse::custom_tool_call_input_done(
                        output_index,
                        &state.item_id,
                        &input,
                    ));
                } else if !truncated {
                    if is_custom {
                        let input = item.get("input").and_then(Value::as_str).unwrap_or("");
                        frames.push(sse::custom_tool_call_input_done(
                            output_index,
                            &state.item_id,
                            input,
                        ));
                    } else {
                        frames.push(sse::function_call_arguments_done(
                            output_index,
                            &state.item_id,
                            &arguments,
                        ));
                    }
                }
                frames.push(sse::output_item_done(output_index, &item));
                item
            }
            ItemKind::Reasoning if chat => {
                let (events, item) =
                    sse::reasoning_close(output_index, &state.item_id, &state.accum);
                frames.extend(events);
                item
            }
            ItemKind::Reasoning | ItemKind::RedactedReasoning => {
                let has_visible_summary = state.kind == ItemKind::Reasoning;
                let block = if has_visible_summary {
                    let mut block = json!({ "type": "thinking", "thinking": state.accum });
                    if let Some(signature) = &state.opaque {
                        block["signature"] = json!(signature);
                    }
                    block
                } else {
                    json!({ "type": "redacted_thinking", "data": state.opaque.clone().unwrap_or_default() })
                };
                // 没有签名的 thinking 无法回放，不成为条目
                let Some(item) =
                    responses_reasoning_item_from_anthropic_block(&state.item_id, &block)
                else {
                    return;
                };
                frames.extend(sse::reasoning_close_with_item(
                    output_index,
                    &state.item_id,
                    &state.accum,
                    &item,
                    has_visible_summary,
                ));
                item
            }
        };
        self.output_items.push((output_index, item));
    }

    fn message_stop(&mut self, stop_reason: Option<StopReason>, frames: &mut Vec<Bytes>) {
        self.ensure_started(frames);
        let mut open: Vec<usize> = self
            .items
            .iter()
            .filter(|(_, state)| !state.done)
            .map(|(index, _)| *index)
            .collect();
        open.sort_unstable();
        for index in open {
            self.part_stop(index, frames);
        }
        let (status, incomplete_reason) = status(stop_reason);
        let mut response = self.base_response(status, self.sorted_output());
        if let Some(reason) = incomplete_reason {
            response["incomplete_details"] = json!({ "reason": reason });
        }
        frames.push(sse::response_completed(&response));
        self.finished = true;
    }

    fn failed(&mut self, message: &str, code: Option<&str>, frames: &mut Vec<Bytes>) {
        self.finished = true;
        let mut error = json!({ "message": message });
        if let Some(code) = code.filter(|code| !code.is_empty()) {
            error["type"] = json!(code);
        }
        let mut response = self.base_response("failed", self.sorted_output());
        response["error"] = error;
        frames.push(sse::response_failed(&response));
    }
}

impl EventEncoder for ResponsesEventEncoder {
    fn encode(&mut self, event: &StreamEvent) -> Vec<Bytes> {
        let mut frames = Vec::new();
        if self.finished {
            return frames;
        }
        match event {
            StreamEvent::MessageStart {
                id,
                model,
                usage,
                created,
            } => {
                if !self.started {
                    self.response_id = response_id(id);
                    if !model.is_empty() {
                        self.model = model.clone();
                    }
                    self.created_at = created.unwrap_or(0);
                    if usage.is_some() {
                        self.usage = *usage;
                    }
                    self.ensure_started(&mut frames);
                }
            }
            StreamEvent::PartStart { index, part } => {
                self.ensure_started(&mut frames);
                self.part_start(*index, part, &mut frames);
            }
            StreamEvent::PartDelta { index, delta } => self.part_delta(*index, delta, &mut frames),
            StreamEvent::PartStop { index } => self.part_stop(*index, &mut frames),
            StreamEvent::Usage(usage) => self.usage = Some(*usage),
            StreamEvent::UpstreamTruncated => self.truncated = true,
            StreamEvent::MessageStop { stop_reason } => {
                self.message_stop(*stop_reason, &mut frames)
            }
            StreamEvent::Error { message, code } => {
                self.failed(message, code.as_deref(), &mut frames)
            }
        }
        frames
    }
}
