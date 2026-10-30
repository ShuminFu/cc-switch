//! [`StreamEvent`] → Anthropic Messages SSE
//!
//! 事件与 SSE 帧一一对应，块序号直接沿用 IR 的 `index`（解码端负责按
//! Anthropic 的要求从 0 开始依次分配）。`message_delta` 携带最后一次
//! [`StreamEvent::Usage`]；收到 `MessageStop` 或 `Error` 后不再输出任何帧。
//! `message_start` 默认只报告输入侧 usage（输出 token 置 0）；
//! [`AnthropicEventEncoder::with_start_output_tokens`] 原样报告开始时的完整 usage。

use bytes::Bytes;
use serde_json::{json, Value};

use super::super::EventEncoder;
use crate::proxy::protocol::ir::{Part, PartDelta, StreamEvent};
use crate::proxy::usage::ir::UsageIR;

/// Anthropic SSE 编码状态机
#[derive(Debug, Default)]
pub struct AnthropicEventEncoder {
    started: bool,
    finished: bool,
    usage: Option<UsageIR>,
    /// `message_start` 保留开始时 usage 的输出 token（旧 Gemini 转换器的形状）
    start_output_tokens: bool,
    /// `tool_use` 的 `content_block_start` 带空 `input` 对象（Anthropic 官方流的形状）
    tool_start_input: bool,
}

fn sse(event: &str, payload: &Value) -> Bytes {
    Bytes::from(format!(
        "event: {event}\ndata: {}\n\n",
        serde_json::to_string(payload).unwrap_or_default()
    ))
}

fn zero_usage() -> Value {
    json!({"input_tokens": 0, "output_tokens": 0})
}

impl AnthropicEventEncoder {
    pub fn new() -> Self {
        Self::default()
    }

    /// `message_start` 原样报告 [`StreamEvent::MessageStart`] 携带的 usage（含输出 token）
    pub fn with_start_output_tokens() -> Self {
        Self {
            start_output_tokens: true,
            ..Self::default()
        }
    }

    /// `tool_use` 块的开始事件带 `"input": {}`
    pub fn with_tool_start_input(mut self) -> Self {
        self.tool_start_input = true;
        self
    }

    fn message_start(&mut self, id: &str, model: &str, usage: Option<UsageIR>) -> Bytes {
        self.started = true;
        // 开始时只报告输入侧 usage，输出 token 由 message_delta 给出
        let keep_output = self.start_output_tokens;
        let usage = usage.map_or_else(zero_usage, |usage| {
            let mut usage = usage.to_anthropic();
            if !keep_output {
                usage["output_tokens"] = json!(0);
            }
            usage
        });
        sse(
            "message_start",
            &json!({
                "type": "message_start",
                "message": {
                    "id": id,
                    "type": "message",
                    "role": "assistant",
                    "model": model,
                    "usage": usage
                }
            }),
        )
    }

    /// 解码端未发 `MessageStart` 就开始输出时补一个空的开始事件
    fn ensure_started(&mut self, frames: &mut Vec<Bytes>) {
        if !self.started {
            frames.push(self.message_start("", "", None));
        }
    }

    fn block_start(&self, index: usize, part: &Part) -> Option<(Bytes, Option<PartDelta>)> {
        // 可增量的字段在开始事件中置空，已有内容作为第一个增量补发
        let (block, delta) = match part {
            Part::Text { text } => (
                json!({"type": "text", "text": ""}),
                (!text.is_empty()).then(|| PartDelta::Text(text.clone())),
            ),
            Part::Reasoning { text, signature } => {
                let mut block = json!({"type": "thinking", "thinking": ""});
                if let Some(signature) = signature {
                    block["signature"] = json!(signature);
                }
                (
                    block,
                    (!text.is_empty()).then(|| PartDelta::Reasoning(text.clone())),
                )
            }
            Part::RedactedReasoning { data } => {
                (json!({"type": "redacted_thinking", "data": data}), None)
            }
            Part::ToolCall {
                id,
                name,
                arguments,
            } => {
                let mut block = json!({"type": "tool_use", "id": id, "name": name});
                if self.tool_start_input {
                    block["input"] = json!({});
                }
                (
                    block,
                    (!arguments.is_empty()).then(|| PartDelta::ToolArguments(arguments.clone())),
                )
            }
            Part::Image { .. } | Part::Document { .. } | Part::ToolResult { .. } => {
                log::warn!("[Codec/Anthropic] Dropping a non-assistant content part from a stream");
                return None;
            }
        };
        let frame = sse(
            "content_block_start",
            &json!({
                "type": "content_block_start",
                "index": index,
                "content_block": block
            }),
        );
        Some((frame, delta))
    }

    fn block_delta(index: usize, delta: &PartDelta) -> Bytes {
        let delta = match delta {
            PartDelta::Text(text) => json!({"type": "text_delta", "text": text}),
            PartDelta::Reasoning(text) => json!({"type": "thinking_delta", "thinking": text}),
            PartDelta::Signature(signature) => {
                json!({"type": "signature_delta", "signature": signature})
            }
            PartDelta::ToolArguments(json) => {
                json!({"type": "input_json_delta", "partial_json": json})
            }
        };
        sse(
            "content_block_delta",
            &json!({
                "type": "content_block_delta",
                "index": index,
                "delta": delta
            }),
        )
    }
}

impl EventEncoder for AnthropicEventEncoder {
    fn encode(&mut self, event: &StreamEvent) -> Vec<Bytes> {
        if self.finished {
            return Vec::new();
        }
        let mut frames = Vec::new();
        match event {
            StreamEvent::MessageStart { id, model, usage } => {
                if self.started {
                    log::debug!("[Codec/Anthropic] Ignoring a second message_start");
                } else {
                    if usage.is_some() {
                        self.usage = *usage;
                    }
                    frames.push(self.message_start(id, model, *usage));
                }
            }
            StreamEvent::PartStart { index, part } => {
                self.ensure_started(&mut frames);
                if let Some((frame, delta)) = self.block_start(*index, part) {
                    frames.push(frame);
                    frames.extend(delta.map(|delta| Self::block_delta(*index, &delta)));
                }
            }
            StreamEvent::PartDelta { index, delta } => {
                self.ensure_started(&mut frames);
                frames.push(Self::block_delta(*index, delta));
            }
            StreamEvent::PartStop { index } => {
                self.ensure_started(&mut frames);
                frames.push(sse(
                    "content_block_stop",
                    &json!({"type": "content_block_stop", "index": index}),
                ));
            }
            StreamEvent::Usage(usage) => self.usage = Some(*usage),
            StreamEvent::MessageStop { stop_reason } => {
                self.ensure_started(&mut frames);
                let usage = self.usage.map_or_else(zero_usage, UsageIR::to_anthropic);
                frames.push(sse(
                    "message_delta",
                    &json!({
                        "type": "message_delta",
                        "delta": {
                            "stop_reason": stop_reason.map(|reason| reason.as_anthropic_str()),
                            "stop_sequence": null
                        },
                        "usage": usage
                    }),
                ));
                frames.push(sse("message_stop", &json!({"type": "message_stop"})));
                self.finished = true;
            }
            StreamEvent::Error { message, code } => {
                // 错误即终止：不补发 message_start，也不关闭已打开的块
                frames.push(sse(
                    "error",
                    &json!({
                        "type": "error",
                        "error": {
                            "type": code.as_deref().unwrap_or("upstream_error"),
                            "message": message
                        }
                    }),
                ));
                self.finished = true;
            }
        }
        frames
    }
}
