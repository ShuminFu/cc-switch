//! [`StreamEvent`] → Gemini `streamGenerateContent?alt=sse`（Gemini 作为客户端）
//!
//! 每个 SSE 帧是一个完整的 `GenerateContentResponse`（`data: {...}`，没有事件名）：
//! - 文本与可见 reasoning 增量逐个输出；Gemini 的 `functionCall` 不分片，工具调用在块
//!   结束时以完整参数输出（首个调用带上本轮签名 thinking 的信封，见
//!   [`super::client_response`]）；
//! - 与 Gemini 官方流一致，`finishReason` 与最终 usage 附在最后一个内容帧上：编码端
//!   总是暂存最近一帧，下一帧到来或回复结束时才输出，没有内容时单独输出一帧；
//! - usage 已知后每帧都带累计的 `usageMetadata`（Gemini 客户端取最后一个）；
//! - 上游错误输出 `{"error": {code, message, status}}` 后结束，不补发 `finishReason`。

use std::collections::HashMap;

use bytes::Bytes;
use serde_json::{json, Value};

use super::client_response::{
    encode_thinking_envelope, finish_reason, function_call_part, gemini_error_body, response_chunk,
    thinking_block,
};
use crate::proxy::codec::EventEncoder;
use crate::proxy::protocol::ir::{Part, PartDelta, StreamEvent};
use crate::proxy::usage::ir::UsageIR;

/// 进行中的内容块
#[derive(Debug)]
enum OpenPart {
    Text,
    Reasoning {
        text: String,
        signature: String,
    },
    Tool {
        id: String,
        name: String,
        arguments: String,
    },
    Other,
}

/// Gemini SSE 编码状态机
#[derive(Debug, Default)]
pub struct GeminiEventEncoder {
    id: String,
    model: String,
    usage: Option<UsageIR>,
    parts: HashMap<usize, OpenPart>,
    /// 等待装进首个 `functionCall` 的签名 thinking 块
    thinking_blocks: Vec<Value>,
    /// 暂存的最近一帧的 parts（`finishReason` 附在它上面）
    held: Option<Vec<Value>>,
    finished: bool,
}

fn sse(payload: &Value) -> Bytes {
    Bytes::from(format!(
        "data: {}\n\n",
        serde_json::to_string(payload).unwrap_or_default()
    ))
}

impl GeminiEventEncoder {
    pub fn new() -> Self {
        Self::default()
    }

    fn chunk(&self, parts: Vec<Value>, finish: Option<&str>) -> Bytes {
        sse(&response_chunk(
            parts,
            finish,
            self.usage.as_ref(),
            &self.model,
            &self.id,
        ))
    }

    /// 输出上一帧并暂存 `parts`
    fn push(&mut self, parts: Vec<Value>) -> Vec<Bytes> {
        let previous = self.held.replace(parts);
        previous
            .map(|parts| self.chunk(parts, None))
            .into_iter()
            .collect()
    }

    /// 输出暂存帧（`finish` 存在时附在暂存帧上，没有暂存帧则单独输出）
    fn flush(&mut self, finish: Option<&str>) -> Vec<Bytes> {
        match (self.held.take(), finish) {
            (Some(parts), finish) => vec![self.chunk(parts, finish)],
            (None, Some(finish)) => vec![self.chunk(Vec::new(), Some(finish))],
            (None, None) => Vec::new(),
        }
    }

    fn part_start(&mut self, index: usize, part: &Part) -> Vec<Bytes> {
        let (open, frames) = match part {
            Part::Text { text } => (
                OpenPart::Text,
                if text.is_empty() {
                    Vec::new()
                } else {
                    self.push(vec![json!({ "text": text })])
                },
            ),
            Part::Reasoning { text, signature } => (
                OpenPart::Reasoning {
                    text: text.clone(),
                    signature: signature.clone().unwrap_or_default(),
                },
                if text.is_empty() {
                    Vec::new()
                } else {
                    self.push(vec![json!({ "text": text, "thought": true })])
                },
            ),
            Part::ToolCall {
                id,
                name,
                arguments,
            } => (
                OpenPart::Tool {
                    id: id.clone(),
                    name: name.clone(),
                    arguments: arguments.clone(),
                },
                Vec::new(),
            ),
            Part::RedactedReasoning { .. } => {
                self.thinking_blocks.extend(thinking_block(part));
                (OpenPart::Other, Vec::new())
            }
            _ => (OpenPart::Other, Vec::new()),
        };
        self.parts.insert(index, open);
        frames
    }

    fn part_delta(&mut self, index: usize, delta: &PartDelta) -> Vec<Bytes> {
        let Some(open) = self.parts.get_mut(&index) else {
            return Vec::new();
        };
        let parts = match (open, delta) {
            (OpenPart::Text, PartDelta::Text(text)) if !text.is_empty() => {
                vec![json!({ "text": text })]
            }
            (OpenPart::Reasoning { text, .. }, PartDelta::Reasoning(more)) if !more.is_empty() => {
                text.push_str(more);
                vec![json!({ "text": more, "thought": true })]
            }
            (OpenPart::Reasoning { signature, .. }, PartDelta::Signature(more)) => {
                signature.push_str(more);
                return Vec::new();
            }
            (OpenPart::Tool { arguments, .. }, PartDelta::ToolArguments(more)) => {
                arguments.push_str(more);
                return Vec::new();
            }
            _ => return Vec::new(),
        };
        self.push(parts)
    }

    fn part_stop(&mut self, index: usize) -> Vec<Bytes> {
        match self.parts.remove(&index) {
            Some(OpenPart::Reasoning { text, signature }) => {
                let part = Part::Reasoning {
                    text,
                    signature: Some(signature),
                };
                self.thinking_blocks.extend(thinking_block(&part));
                Vec::new()
            }
            Some(OpenPart::Tool {
                id,
                name,
                arguments,
            }) => {
                let envelope = encode_thinking_envelope(&std::mem::take(&mut self.thinking_blocks));
                let part = function_call_part(&id, &name, &arguments, envelope);
                self.push(vec![part])
            }
            _ => Vec::new(),
        }
    }
}

impl EventEncoder for GeminiEventEncoder {
    fn encode(&mut self, event: &StreamEvent) -> Vec<Bytes> {
        if self.finished {
            return Vec::new();
        }
        match event {
            StreamEvent::MessageStart {
                id, model, usage, ..
            } => {
                self.id = id.clone();
                self.model = model.clone();
                if usage.is_some() {
                    self.usage = *usage;
                }
                Vec::new()
            }
            StreamEvent::PartStart { index, part } => self.part_start(*index, part),
            StreamEvent::PartDelta { index, delta } => self.part_delta(*index, delta),
            StreamEvent::PartStop { index } => self.part_stop(*index),
            StreamEvent::Usage(usage) => {
                self.usage = Some(*usage);
                Vec::new()
            }
            StreamEvent::UpstreamTruncated => Vec::new(),
            StreamEvent::MessageStop { stop_reason } => {
                self.finished = true;
                self.flush(Some(finish_reason(*stop_reason)))
            }
            StreamEvent::Error { message, code } => {
                self.finished = true;
                let mut frames = self.flush(None);
                frames.push(sse(&gemini_error_body(message, code.as_deref(), 500)));
                frames
            }
        }
    }

    fn finish(&mut self) -> Vec<Bytes> {
        if self.finished {
            return Vec::new();
        }
        self.finished = true;
        self.flush(None)
    }
}
