//! Anthropic Messages SSE → [`StreamEvent`]（Anthropic 作为上游）
//!
//! 与旧转换器（`providers::streaming_codex_anthropic`）的规则一致：
//! - 块按上游 `index` 定位；未知类型的块与未知序号的增量忽略，重复的 `content_block_stop`
//!   只处理一次；`content_block_start` 携带的内容（文本、`tool_use.input`、thinking
//!   签名）作为块的初始内容放进 `PartStart`；
//! - `message_start` / `message_delta` 的 usage 按字段合并，每次合并后发出累计 usage；
//! - `message_stop` 关闭仍打开的块并结束；`error` 事件以 [`StreamEvent::Error`] 终止；
//! - 没有 `message_stop` 就 EOF：已收到 `stop_reason` 按正常结束收尾；有部分输出时先发
//!   [`StreamEvent::UpstreamTruncated`] 再按 `max_tokens` 收尾；什么都没有则报
//!   `stream_truncated` 错误。

use std::collections::BTreeMap;

use serde_json::{Map, Value};

use super::upstream_response::{error_details, stop_reason};
use crate::proxy::codec::{CodecError, EventDecoder};
use crate::proxy::protocol::ir::{Part, PartDelta, StreamEvent};
use crate::proxy::usage::ir::UsageIR;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BlockKind {
    Text,
    Tool,
    Thinking,
    RedactedThinking,
}

#[derive(Debug)]
struct BlockState {
    kind: BlockKind,
    accum: String,
    call_id: String,
    name: String,
    /// thinking 的签名 / redacted_thinking 的数据（决定关闭后是否成为输出条目）
    opaque: String,
    done: bool,
}

/// Anthropic SSE 解码状态机
#[derive(Debug, Default)]
pub struct AnthropicEventDecoder {
    started: bool,
    completed: bool,
    message_id: String,
    model: String,
    usage: Map<String, Value>,
    stop_reason: Option<String>,
    blocks: BTreeMap<u64, BlockState>,
    /// 已关闭并成为输出条目的块数（未签名的 thinking 不算）
    closed_items: usize,
}

impl AnthropicEventDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    fn usage_ir(&self) -> Option<UsageIR> {
        (!self.usage.is_empty())
            .then(|| UsageIR::from_anthropic(&Value::Object(self.usage.clone())))
    }

    fn merge_usage(&mut self, usage: &Value) {
        if let Some(object) = usage.as_object() {
            for (key, value) in object {
                if !value.is_null() {
                    self.usage.insert(key.clone(), value.clone());
                }
            }
        }
    }

    fn ensure_started(&mut self, events: &mut Vec<StreamEvent>) {
        if !self.started {
            self.started = true;
            events.push(StreamEvent::MessageStart {
                id: self.message_id.clone(),
                model: self.model.clone(),
                usage: self.usage_ir(),
                created: None,
            });
        }
    }

    fn message_start(&mut self, data: &Value, events: &mut Vec<StreamEvent>) {
        if let Some(message) = data.get("message") {
            if let Some(id) = message.get("id").and_then(Value::as_str) {
                self.message_id = id.to_string();
            }
            if let Some(model) = message
                .get("model")
                .and_then(Value::as_str)
                .filter(|model| !model.is_empty())
            {
                self.model = model.to_string();
            }
            if let Some(usage) = message.get("usage") {
                self.merge_usage(usage);
            }
        }
        if self.started {
            if let Some(usage) = self.usage_ir() {
                events.push(StreamEvent::Usage(usage));
            }
        }
        self.ensure_started(events);
    }

    fn block_start(&mut self, data: &Value, events: &mut Vec<StreamEvent>) {
        self.ensure_started(events);
        let Some(index) = data.get("index").and_then(Value::as_u64) else {
            return;
        };
        let block = data.get("content_block").unwrap_or(&Value::Null);
        let str_field = |key: &str| block.get(key).and_then(Value::as_str).unwrap_or_default();
        let (kind, part) = match str_field("type") {
            "text" => (BlockKind::Text, Part::text(str_field("text"))),
            "tool_use" => (
                BlockKind::Tool,
                Part::ToolCall {
                    id: str_field("id").to_string(),
                    name: str_field("name").to_string(),
                    // 部分网关把完整 input 放在开始事件里、不发增量
                    arguments: block
                        .get("input")
                        .filter(|input| input.as_object().is_some_and(|o| !o.is_empty()))
                        .map(Value::to_string)
                        .unwrap_or_default(),
                },
            ),
            "thinking" => (
                BlockKind::Thinking,
                Part::Reasoning {
                    text: str_field("thinking").to_string(),
                    signature: block
                        .get("signature")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                },
            ),
            "redacted_thinking" => (
                BlockKind::RedactedThinking,
                Part::RedactedReasoning {
                    data: str_field("data").to_string(),
                },
            ),
            _ => return,
        };
        let state = BlockState {
            kind,
            accum: match &part {
                Part::Text { text } | Part::Reasoning { text, .. } => text.clone(),
                _ => String::new(),
            },
            call_id: if kind == BlockKind::Tool {
                str_field("id").to_string()
            } else {
                String::new()
            },
            name: if kind == BlockKind::Tool {
                str_field("name").to_string()
            } else {
                String::new()
            },
            opaque: match &part {
                Part::Reasoning { signature, .. } => signature.clone().unwrap_or_default(),
                Part::RedactedReasoning { data } => data.clone(),
                _ => String::new(),
            },
            done: false,
        };
        self.blocks.insert(index, state);
        events.push(StreamEvent::PartStart {
            index: index as usize,
            part,
        });
    }

    fn block_delta(&mut self, data: &Value, events: &mut Vec<StreamEvent>) {
        let Some(index) = data.get("index").and_then(Value::as_u64) else {
            return;
        };
        let Some(block) = self.blocks.get_mut(&index) else {
            return;
        };
        let delta = data.get("delta").unwrap_or(&Value::Null);
        let text = |key: &str| {
            delta
                .get(key)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string()
        };
        let delta = match delta
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default()
        {
            "text_delta" => PartDelta::Text(text("text")),
            "input_json_delta" => PartDelta::ToolArguments(text("partial_json")),
            "thinking_delta" => PartDelta::Reasoning(text("thinking")),
            "signature_delta" => match delta.get("signature").and_then(Value::as_str) {
                Some(signature) => {
                    block.opaque = signature.to_string();
                    PartDelta::Signature(signature.to_string())
                }
                None => return,
            },
            _ => return,
        };
        if let PartDelta::Text(more) | PartDelta::ToolArguments(more) | PartDelta::Reasoning(more) =
            &delta
        {
            block.accum.push_str(more);
        }
        events.push(StreamEvent::PartDelta {
            index: index as usize,
            delta,
        });
    }

    fn close_block(&mut self, index: u64, events: &mut Vec<StreamEvent>) {
        let Some(block) = self.blocks.get_mut(&index) else {
            return;
        };
        if block.done {
            return;
        }
        block.done = true;
        // 未签名的 thinking 无法回放，不成为输出条目
        let becomes_item = match block.kind {
            BlockKind::Text | BlockKind::Tool => true,
            BlockKind::Thinking | BlockKind::RedactedThinking => !block.opaque.is_empty(),
        };
        if becomes_item {
            self.closed_items += 1;
        }
        events.push(StreamEvent::PartStop {
            index: index as usize,
        });
    }

    fn finalize(&mut self, events: &mut Vec<StreamEvent>) {
        if self.completed {
            return;
        }
        self.ensure_started(events);
        let open: Vec<u64> = self
            .blocks
            .iter()
            .filter(|(_, block)| !block.done)
            .map(|(index, _)| *index)
            .collect();
        for index in open {
            self.close_block(index, events);
        }
        events.push(StreamEvent::MessageStop {
            stop_reason: self.stop_reason.as_deref().map(stop_reason),
        });
        self.completed = true;
    }

    /// 是否已有部分输出（已完成的条目、累积的内容或已开始的工具调用）
    fn has_substantive_output(&self) -> bool {
        self.closed_items > 0
            || self.blocks.values().any(|block| {
                !block.accum.trim().is_empty()
                    || !block.call_id.trim().is_empty()
                    || !block.name.trim().is_empty()
            })
    }
}

impl EventDecoder for AnthropicEventDecoder {
    fn decode(&mut self, event: &str, data: &str) -> Result<Vec<StreamEvent>, CodecError> {
        let mut events = Vec::new();
        if self.completed {
            return Ok(events);
        }
        let Ok(value) = serde_json::from_str::<Value>(data) else {
            return Ok(events);
        };
        let kind = value
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or(event)
            .to_string();
        match kind.as_str() {
            "message_start" => self.message_start(&value, &mut events),
            "content_block_start" => self.block_start(&value, &mut events),
            "content_block_delta" => self.block_delta(&value, &mut events),
            "content_block_stop" => {
                if let Some(index) = value.get("index").and_then(Value::as_u64) {
                    self.close_block(index, &mut events);
                }
            }
            "message_delta" => {
                if let Some(reason) = value.pointer("/delta/stop_reason").and_then(Value::as_str) {
                    self.stop_reason = Some(reason.to_string());
                }
                if let Some(usage) = value.get("usage") {
                    self.merge_usage(usage);
                    if let Some(usage) = self.usage_ir() {
                        events.push(StreamEvent::Usage(usage));
                    }
                }
            }
            "message_stop" => self.finalize(&mut events),
            "error" => {
                let (message, code) = error_details(&value);
                events.push(StreamEvent::Error { message, code });
                self.completed = true;
            }
            _ => {}
        }
        Ok(events)
    }

    fn finish(&mut self) -> Result<Vec<StreamEvent>, CodecError> {
        let mut events = Vec::new();
        if self.completed {
            return Ok(events);
        }
        if self.stop_reason.is_some() {
            // 收到了 message_delta（stop_reason 与最终 usage）但没有 message_stop：
            // 回合在语义上已完成，正常收尾
            self.finalize(&mut events);
        } else if self.has_substantive_output() {
            // 上游中途截断（例如代理没有报 I/O 错误就关闭了连接）：报告为不完整，
            // 避免 Codex 把截断的输出当作正常完成
            events.push(StreamEvent::UpstreamTruncated);
            self.stop_reason = Some("max_tokens".to_string());
            self.finalize(&mut events);
        } else {
            events.push(StreamEvent::Error {
                message: "Upstream Anthropic stream ended before message_stop".to_string(),
                code: Some("stream_truncated".to_string()),
            });
            self.completed = true;
        }
        Ok(events)
    }
}

/// 上游忽略 `stream: true` 返回的完整消息 → 与实时流相同的事件序列
///
/// 与旧转换器一致：开始事件携带消息的 usage，块的内容作为初始内容（文本与 thinking
/// 另以增量给出），工具 input 不以增量给出。
pub(crate) fn message_events(body: &Value) -> Vec<StreamEvent> {
    if super::upstream_response::is_error_envelope(body) {
        let (message, code) = error_details(body);
        return vec![StreamEvent::Error { message, code }];
    }
    let mut decoder = AnthropicEventDecoder::new();
    let mut events = Vec::new();
    let mut message = body.clone();
    message["content"] = Value::Array(Vec::new());
    decoder.message_start(&serde_json::json!({ "message": message }), &mut events);
    for (index, block) in body
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .enumerate()
    {
        let block_type = block.get("type").and_then(Value::as_str).unwrap_or("");
        let mut start = block.clone();
        match block_type {
            "text" => start["text"] = Value::String(String::new()),
            "thinking" => start["thinking"] = Value::String(String::new()),
            _ => {}
        }
        decoder.block_start(
            &serde_json::json!({ "index": index, "content_block": start }),
            &mut events,
        );
        let delta = match block_type {
            "text" => block
                .get("text")
                .and_then(Value::as_str)
                .map(|text| serde_json::json!({ "type": "text_delta", "text": text })),
            "thinking" => block
                .get("thinking")
                .and_then(Value::as_str)
                .map(|text| serde_json::json!({ "type": "thinking_delta", "thinking": text })),
            _ => None,
        };
        if let Some(delta) = delta {
            decoder.block_delta(
                &serde_json::json!({ "index": index, "delta": delta }),
                &mut events,
            );
        }
        decoder.close_block(index as u64, &mut events);
    }
    if let Some(reason) = body.get("stop_reason").and_then(Value::as_str) {
        decoder.stop_reason = Some(reason.to_string());
    }
    decoder.finalize(&mut events);
    events
}
