//! Gemini `streamGenerateContent?alt=sse` → [`StreamEvent`]
//!
//! 规则与旧转换器（`providers::streaming_gemini`）一致：
//! - 第一个可解析的 JSON 块即发出 `MessageStart`，携带该块的 usage
//! - 可见文本经 [`GeminiTextDeltaTracker`] 增量化后作为块 0 实时输出
//! - functionCall 在流中是累计快照，按 id / 位置合并，流结束时才统一输出
//!   （位于文本块之后；交错的文本与工具调用顺序因此不保留，详见旧转换器的说明）
//! - `data: [DONE]`（OpenAI 兼容中转站）之后的事件全部忽略，无法解析的块跳过
//! - 提示被拦截且没有任何文本时补一段说明文本，以 refusal 结束
//! - 流正常结束时把助手轮次记入 shadow 存储
//!
//! 上游没有流内错误事件；传输错误由驱动端处理。

use serde_json::Value;

use super::response::blocked_text;
use super::text::{extract_text_thought_signature, extract_visible_text, GeminiTextDeltaTracker};
use super::tool_calls::{
    build_shadow_assistant_parts, extract_tool_calls, merge_tool_call_snapshots,
    rectify_tool_call_parts, AnthropicToolSchemaHints,
};
use super::GeminiShadowSink;
use crate::proxy::codec::{CodecError, EventDecoder};
use crate::proxy::protocol::ir::{Part, PartDelta, StreamEvent};
use crate::proxy::protocol::StopReason;
use crate::proxy::providers::gemini_shadow::{GeminiAssistantTurn, GeminiToolCallMeta};
use crate::proxy::usage::ir::UsageIR;

/// Gemini SSE 解码状态机
#[derive(Debug, Default)]
pub struct GeminiEventDecoder {
    tool_schema_hints: Option<AnthropicToolSchemaHints>,
    shadow_sink: Option<GeminiShadowSink>,
    started: bool,
    /// 收到 `[DONE]` 后不再处理任何事件
    done: bool,
    text_tracker: GeminiTextDeltaTracker,
    /// 文本块的序号（首次输出文本时分配）及其是否仍未关闭
    text_index: Option<usize>,
    text_open: bool,
    next_index: usize,
    tool_call_snapshots: Vec<GeminiToolCallMeta>,
    text_thought_signature: Option<String>,
    latest_usage: Option<Value>,
    latest_finish_reason: Option<String>,
    blocked_text: Option<String>,
}

impl GeminiEventDecoder {
    pub fn new(
        tool_schema_hints: Option<AnthropicToolSchemaHints>,
        shadow_sink: Option<GeminiShadowSink>,
    ) -> Self {
        Self {
            tool_schema_hints,
            shadow_sink,
            ..Self::default()
        }
    }

    /// 文本增量；第一次输出时打开文本块。空增量不产生事件
    fn text_delta(&mut self, delta: String, events: &mut Vec<StreamEvent>) {
        if delta.is_empty() {
            return;
        }
        let index = match self.text_index {
            Some(index) => index,
            None => {
                let index = self.next_index;
                self.next_index += 1;
                self.text_index = Some(index);
                index
            }
        };
        if !self.text_open {
            events.push(StreamEvent::PartStart {
                index,
                part: Part::text(""),
            });
            self.text_open = true;
        }
        events.push(StreamEvent::PartDelta {
            index,
            delta: PartDelta::Text(delta),
        });
    }

    fn decode_chunk(&mut self, chunk: &Value, events: &mut Vec<StreamEvent>) {
        if !self.started {
            self.started = true;
            events.push(StreamEvent::MessageStart {
                id: str_field(chunk, "responseId").to_string(),
                model: str_field(chunk, "modelVersion").to_string(),
                usage: chunk.get("usageMetadata").map(UsageIR::from_gemini),
            });
        }
        if self.latest_usage.is_none() {
            self.latest_usage = chunk.get("usageMetadata").cloned();
        }

        if let Some(reason) = chunk
            .pointer("/promptFeedback/blockReason")
            .and_then(Value::as_str)
        {
            self.blocked_text = Some(blocked_text(reason));
        }

        let Some(candidate) = chunk
            .get("candidates")
            .and_then(Value::as_array)
            .and_then(|candidates| candidates.first())
        else {
            return;
        };
        if let Some(reason) = candidate.get("finishReason").and_then(Value::as_str) {
            self.latest_finish_reason = Some(reason.to_string());
        }
        if let Some(usage) = chunk.get("usageMetadata") {
            self.latest_usage = Some(usage.clone());
        }
        let Some(parts) = candidate
            .pointer("/content/parts")
            .and_then(Value::as_array)
        else {
            return;
        };

        let hints = self.tool_schema_hints.as_ref();
        let mut rectified_parts = parts.clone();
        rectify_tool_call_parts(&mut rectified_parts, hints);
        if let Some(signature) = extract_text_thought_signature(parts) {
            self.text_thought_signature = Some(signature);
        }
        merge_tool_call_snapshots(
            &mut self.tool_call_snapshots,
            extract_tool_calls(&rectified_parts, hints),
        );
        let visible_text = extract_visible_text(&rectified_parts);
        if !visible_text.is_empty() {
            let delta = self.text_tracker.push(visible_text);
            self.text_delta(delta, events);
        }
    }

    fn record_shadow(&self) {
        let Some(sink) = &self.shadow_sink else {
            return;
        };
        let accumulated = self.text_tracker.text();
        let text = if accumulated.is_empty() {
            self.blocked_text.as_deref()
        } else {
            Some(accumulated)
        };
        let parts = build_shadow_assistant_parts(
            text,
            self.text_thought_signature.as_deref(),
            &self.tool_call_snapshots,
        );
        if !parts.is_empty() {
            sink.record(&mut GeminiAssistantTurn::new(
                serde_json::json!({ "parts": parts }),
                self.tool_call_snapshots.clone(),
            ));
        }
    }
}

fn str_field<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or_default()
}

impl EventDecoder for GeminiEventDecoder {
    fn decode(&mut self, _event: &str, data: &str) -> Result<Vec<StreamEvent>, CodecError> {
        let mut events = Vec::new();
        if self.done {
            return Ok(events);
        }
        if data.trim() == "[DONE]" {
            self.done = true;
            return Ok(events);
        }
        // 与旧转换器一致：无法解析的块直接跳过，不中断流
        if let Ok(chunk) = serde_json::from_str::<Value>(data) {
            self.decode_chunk(&chunk, &mut events);
        }
        Ok(events)
    }

    fn finish(&mut self) -> Result<Vec<StreamEvent>, CodecError> {
        let mut events = Vec::new();
        if !self.started {
            self.started = true;
            events.push(StreamEvent::MessageStart {
                id: String::new(),
                model: String::new(),
                usage: self.latest_usage.as_ref().map(UsageIR::from_gemini),
            });
        }

        let tail = self.text_tracker.finish();
        self.text_delta(tail, &mut events);
        if self.text_tracker.text().is_empty() {
            if let Some(blocked) = self.blocked_text.clone() {
                self.text_delta(blocked, &mut events);
            }
        }
        if let (Some(index), true) = (self.text_index, self.text_open) {
            events.push(StreamEvent::PartStop { index });
            self.text_open = false;
        }

        self.record_shadow();

        for tool_call in &self.tool_call_snapshots {
            let index = self.next_index;
            self.next_index += 1;
            events.push(StreamEvent::PartStart {
                index,
                part: Part::ToolCall {
                    id: tool_call.id.clone().unwrap_or_default(),
                    name: tool_call.name.clone(),
                    arguments: String::new(),
                },
            });
            events.push(StreamEvent::PartDelta {
                index,
                delta: PartDelta::ToolArguments(
                    serde_json::to_string(&tool_call.args).unwrap_or_else(|_| "{}".to_string()),
                ),
            });
            events.push(StreamEvent::PartStop { index });
        }

        let stop_reason = if self.blocked_text.is_some() {
            StopReason::Refusal
        } else {
            StopReason::from_gemini_finish_reason(
                self.latest_finish_reason.as_deref(),
                !self.tool_call_snapshots.is_empty(),
            )
        };
        events.push(StreamEvent::Usage(
            self.latest_usage
                .as_ref()
                .map(UsageIR::from_gemini)
                .unwrap_or_default(),
        ));
        events.push(StreamEvent::MessageStop {
            stop_reason: Some(stop_reason),
        });
        Ok(events)
    }
}
