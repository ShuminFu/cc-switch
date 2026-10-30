//! OpenAI Chat Completions SSE → [`StreamEvent`]
//!
//! 与旧转换器（`providers::streaming`）的规则一致，以便 Anthropic 编码端输出逐字节相同：
//! - 块序号按出现顺序从 0 分配；reasoning 与文本共用一个「当前块」，切换类型时关闭旧块，
//!   工具调用开始时也关闭它；
//! - 工具块在拿到 id 与 name 后才开始，之前到达的参数先缓存；收尾时仍缺 id/name 的块以
//!   `tool_call_{index}` / `unknown_tool` 补发开始事件；
//! - 只采用第一个 finish_reason（部分上游重复发送），结束事件推迟到 `[DONE]` 或流末尾，
//!   以便收齐 usage；`[DONE]` 之后的数据全部忽略；
//! - 上游 in-band 错误（`event: error` 或顶层 `error` 字段）立即以 [`StreamEvent::Error`]
//!   终止；未收到 finish_reason 就结束时，只有文本 / 思考输出则按 `max_tokens` 截断收尾，
//!   否则报 `stream_truncated` 错误；
//! - Copilot 无限空白 bug：工具参数连续空白达到阈值时中止该工具调用。

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Deserializer};
use serde_json::Value;

use crate::proxy::codec::{CodecError, EventDecoder};
use crate::proxy::protocol::ir::{Part, PartDelta, StreamEvent};
use crate::proxy::protocol::StopReason;
use crate::proxy::usage::ir::UsageIR;

/// 无限空白 bug 的连续空白字符阈值
const INFINITE_WHITESPACE_THRESHOLD: usize = 500;

#[derive(Debug, Deserialize)]
struct StreamChunk {
    #[serde(default)]
    id: String,
    #[serde(default)]
    model: String,
    #[serde(default)]
    choices: Vec<StreamChoice>,
    #[serde(default)]
    usage: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct StreamChoice {
    // 部分兼容上游在收尾 chunk 中省略 delta 或发送 `"delta": null`
    #[serde(default, deserialize_with = "null_as_default")]
    delta: Delta,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct Delta {
    #[serde(default)]
    content: Option<String>,
    // OpenRouter/Kimi 等使用 reasoning，DeepSeek 使用 reasoning_content
    #[serde(default, alias = "reasoning_content")]
    reasoning: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<DeltaToolCall>>,
}

#[derive(Debug, Deserialize)]
struct DeltaToolCall {
    index: usize,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    function: Option<DeltaFunction>,
}

#[derive(Debug, Deserialize)]
struct DeltaFunction {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}

/// 将 JSON `null` 视为字段缺失，回退到类型默认值
fn null_as_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

/// 当前打开的文本 / 思考块
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OpenBlock {
    Text(usize),
    Reasoning(usize),
}

impl OpenBlock {
    fn index(self) -> usize {
        match self {
            Self::Text(index) | Self::Reasoning(index) => index,
        }
    }
}

#[derive(Debug, Default)]
struct ToolState {
    /// IR 块序号（首次出现时分配，开始事件可能推迟）
    index: usize,
    id: String,
    name: String,
    started: bool,
    pending_args: String,
    consecutive_whitespace: usize,
    aborted: bool,
}

/// Chat Completions SSE 解码状态机
#[derive(Debug, Default)]
pub struct OpenAiChatEventDecoder {
    message_id: Option<String>,
    model: Option<String>,
    started: bool,
    stopped: bool,
    failed: bool,
    next_index: usize,
    open_block: Option<OpenBlock>,
    /// Chat 的 tool_calls[].index → 工具块状态
    tools: HashMap<usize, ToolState>,
    open_tools: HashSet<usize>,
    /// 第一个 finish_reason 对应的停止原因（推迟到流结束时发出）
    pending_stop: Option<StopReason>,
}

impl OpenAiChatEventDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    fn allocate_index(&mut self) -> usize {
        let index = self.next_index;
        self.next_index += 1;
        index
    }

    fn any_tool_started(&self) -> bool {
        self.tools.values().any(|state| state.started)
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

    fn close_open_block(&mut self, events: &mut Vec<StreamEvent>) {
        if let Some(block) = self.open_block.take() {
            events.push(StreamEvent::PartStop {
                index: block.index(),
            });
        }
    }

    /// 关闭所有打开的块；已累积参数但缺 id/name 的工具块先以兜底 id/name 开始，
    /// 保证每个工具调用都在消息结束前完整开闭
    fn close_all(&mut self, events: &mut Vec<StreamEvent>) {
        self.close_open_block(events);

        let mut late_starts: Vec<(usize, String, String, String)> = Vec::new();
        for (tool_index, state) in self.tools.iter_mut() {
            if state.started {
                continue;
            }
            if state.pending_args.is_empty() && state.id.is_empty() && state.name.is_empty() {
                continue;
            }
            let id = if state.id.is_empty() {
                format!("tool_call_{tool_index}")
            } else {
                state.id.clone()
            };
            let name = if state.name.is_empty() {
                "unknown_tool".to_string()
            } else {
                state.name.clone()
            };
            state.started = true;
            late_starts.push((
                state.index,
                id,
                name,
                std::mem::take(&mut state.pending_args),
            ));
        }
        late_starts.sort_unstable_by_key(|(index, ..)| *index);
        for (index, id, name, pending) in late_starts {
            events.push(StreamEvent::PartStart {
                index,
                part: Part::ToolCall {
                    id,
                    name,
                    arguments: String::new(),
                },
            });
            self.open_tools.insert(index);
            if !pending.is_empty() {
                events.push(StreamEvent::PartDelta {
                    index,
                    delta: PartDelta::ToolArguments(pending),
                });
            }
        }

        let mut open: Vec<usize> = self.open_tools.drain().collect();
        open.sort_unstable();
        events.extend(
            open.into_iter()
                .map(|index| StreamEvent::PartStop { index }),
        );
    }

    /// 唯一的收尾出口：补发缺失的开始事件，再发一次结束事件
    fn finalize(&mut self, stop_reason: StopReason, events: &mut Vec<StreamEvent>) {
        if !self.started {
            events.push(self.message_start(None));
        }
        events.push(StreamEvent::MessageStop {
            stop_reason: Some(stop_reason),
        });
        self.stopped = true;
    }

    /// 切换到 reasoning / 文本块（类型不同则先关闭当前块），返回块序号
    fn switch_block(&mut self, reasoning: bool, events: &mut Vec<StreamEvent>) -> usize {
        match self.open_block {
            Some(OpenBlock::Reasoning(index)) if reasoning => return index,
            Some(OpenBlock::Text(index)) if !reasoning => return index,
            _ => {}
        }
        self.close_open_block(events);
        let index = self.allocate_index();
        let (block, part) = if reasoning {
            (
                OpenBlock::Reasoning(index),
                Part::Reasoning {
                    text: String::new(),
                    signature: None,
                },
            )
        } else {
            (OpenBlock::Text(index), Part::text(""))
        };
        events.push(StreamEvent::PartStart { index, part });
        self.open_block = Some(block);
        index
    }

    fn tool_call(&mut self, call: &DeltaToolCall, events: &mut Vec<StreamEvent>) {
        if !self.tools.contains_key(&call.index) {
            let index = self.allocate_index();
            self.tools.insert(
                call.index,
                ToolState {
                    index,
                    ..ToolState::default()
                },
            );
        }
        let Some(state) = self.tools.get_mut(&call.index) else {
            return;
        };
        // 已因无限空白 bug 中止的工具调用不再处理
        if state.aborted {
            return;
        }
        if let Some(id) = &call.id {
            state.id = id.clone();
        }
        if let Some(name) = call.function.as_ref().and_then(|f| f.name.as_ref()) {
            state.name = name.clone();
        }

        let should_start = !state.started && !state.id.is_empty() && !state.name.is_empty();
        if should_start {
            state.started = true;
        }
        let pending_after_start = (should_start && !state.pending_args.is_empty())
            .then(|| std::mem::take(&mut state.pending_args));
        let immediate = match call.function.as_ref().and_then(|f| f.arguments.clone()) {
            Some(args) => {
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
            }
            None => None,
        };

        let index = state.index;
        if should_start {
            events.push(StreamEvent::PartStart {
                index,
                part: Part::ToolCall {
                    id: state.id.clone(),
                    name: state.name.clone(),
                    arguments: String::new(),
                },
            });
            self.open_tools.insert(index);
        }
        for args in pending_after_start.into_iter().chain(immediate) {
            events.push(StreamEvent::PartDelta {
                index,
                delta: PartDelta::ToolArguments(args),
            });
        }
    }

    /// 处理一行 `data:`
    fn decode_line(&mut self, is_error_event: bool, data: &str, events: &mut Vec<StreamEvent>) {
        // 结束（或报错）之后的任何数据（如重复的 [DONE]）都忽略
        if self.stopped || self.failed {
            log::debug!("[Codec/OpenAiChat] <<< ignoring SSE data after message end");
            return;
        }

        if data.trim() == "[DONE]" {
            self.close_all(events);
            // 未收到 finish_reason 的裸 [DONE] 按正常结束处理
            let stop_reason = self
                .pending_stop
                .take()
                .unwrap_or(if self.any_tool_started() {
                    StopReason::ToolUse
                } else {
                    StopReason::EndTurn
                });
            self.finalize(stop_reason, events);
            return;
        }

        let value = serde_json::from_str::<Value>(data).ok();
        let has_error_field = value
            .as_ref()
            .and_then(|value| value.get("error"))
            .is_some_and(|error| !error.is_null());
        if is_error_event || has_error_field {
            let (message, code) = upstream_error_details(value.as_ref(), data);
            log::error!("[Codec/OpenAiChat] <<< upstream in-band error: {code}: {message}");
            events.push(StreamEvent::Error {
                message,
                code: Some(code),
            });
            self.failed = true;
            return;
        }

        let Some(chunk) = value.and_then(|value| serde_json::from_value::<StreamChunk>(value).ok())
        else {
            return;
        };

        if self.message_id.is_none() && !chunk.id.is_empty() {
            self.message_id = Some(chunk.id.clone());
        }
        if self.model.is_none() && !chunk.model.is_empty() {
            self.model = Some(chunk.model.clone());
        }

        // 非对象 usage（如字符串）视为缺失，避免以零值覆盖已收到的有效 usage
        let usage = chunk
            .usage
            .as_ref()
            .filter(|usage| usage.is_object())
            .map(UsageIR::from_chat_completions);
        if let Some(usage) = usage {
            events.push(StreamEvent::Usage(usage));
        }

        let Some(choice) = chunk.choices.first() else {
            return;
        };
        if !self.started {
            events.push(self.message_start(usage));
        }

        if let Some(reasoning) = &choice.delta.reasoning {
            let index = self.switch_block(true, events);
            events.push(StreamEvent::PartDelta {
                index,
                delta: PartDelta::Reasoning(reasoning.clone()),
            });
        }

        if let Some(content) = choice.delta.content.as_ref().filter(|c| !c.is_empty()) {
            let index = self.switch_block(false, events);
            events.push(StreamEvent::PartDelta {
                index,
                delta: PartDelta::Text(content.clone()),
            });
        }

        if let Some(calls) = choice.delta.tool_calls.as_ref().filter(|c| !c.is_empty()) {
            self.close_open_block(events);
            for call in calls {
                self.tool_call(call, events);
            }
        }

        // 只采用第一个 finish_reason；结束事件推迟到 [DONE] / 流末尾，以便收齐 usage
        if let Some(finish_reason) = &choice.finish_reason {
            if self.pending_stop.is_some() {
                return;
            }
            self.close_all(events);
            // 已发出工具块时 stop 视为 tool_use（Gemini 兼容层 / Ollama / vLLM），与非流式一致
            self.pending_stop = Some(StopReason::from_chat_finish_reason(
                finish_reason,
                self.any_tool_started(),
            ));
        }
    }
}

impl EventDecoder for OpenAiChatEventDecoder {
    fn decode(&mut self, event: &str, data: &str) -> Result<Vec<StreamEvent>, CodecError> {
        let mut events = Vec::new();
        // Chat 的每个 data 行都是一个完整 JSON；多行 data 逐行处理
        for line in data.split('\n') {
            self.decode_line(event == "error", line, &mut events);
        }
        Ok(events)
    }

    fn finish(&mut self) -> Result<Vec<StreamEvent>, CodecError> {
        let mut events = Vec::new();
        if self.stopped || self.failed {
            return Ok(events);
        }
        if let Some(stop_reason) = self.pending_stop.take() {
            self.close_all(&mut events);
            self.finalize(stop_reason, &mut events);
        } else if self.next_index > 0 && self.tools.is_empty() {
            // 未收到 finish_reason 就 EOF：仅有文本 / 思考输出时可安全收尾，
            // 以 max_tokens 标记为不完整的截断回合
            log::warn!(
                "[Codec/OpenAiChat] upstream stream ended before finish_reason; finalizing as truncated"
            );
            self.close_all(&mut events);
            self.finalize(StopReason::MaxTokens, &mut events);
        } else {
            // 无任何输出，或工具参数可能不完整：无法安全收尾，显式报错
            log::warn!(
                "[Codec/OpenAiChat] upstream stream ended before finish_reason without safe partial output"
            );
            events.push(StreamEvent::Error {
                message: "OpenAI Chat upstream stream ended before a finish_reason".to_string(),
                code: Some("stream_truncated".to_string()),
            });
            self.failed = true;
        }
        Ok(events)
    }
}

/// 提取上游 in-band 错误（顶层 `error` 字段或 `event: error` 事件）的 message 与类型
fn upstream_error_details(payload: Option<&Value>, raw: &str) -> (String, String) {
    let error = payload.map(|value| match value.get("error") {
        Some(error) if !error.is_null() => error,
        _ => value,
    });
    let message = error
        .and_then(|error| {
            error
                .get("message")
                .and_then(Value::as_str)
                .or_else(|| error.as_str())
        })
        .map(str::to_string)
        .or_else(|| payload.is_none().then(|| raw.trim().to_string()))
        .filter(|message| !message.trim().is_empty())
        .unwrap_or_else(|| "OpenAI Chat upstream emitted an error event".to_string());
    let code = error
        .and_then(|error| {
            error
                .get("type")
                .and_then(Value::as_str)
                .or_else(|| error.get("code").and_then(Value::as_str))
        })
        .filter(|code| !code.trim().is_empty())
        .unwrap_or("upstream_error")
        .to_string();
    (message, code)
}
