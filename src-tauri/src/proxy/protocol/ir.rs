//! 规范协议 IR - 各线协议之间转换的中间表示
//!
//! 每个桥接器曾各自实现 `上游格式 → 客户端格式` 的一对一转换（transform* / streaming*），
//! 同一条规则（工具调用、thinking 签名、停止原因、缓存 token）在多处重复并逐渐漂移。
//! 目标形态是 `decode(upstream) → IR → encode(client)`：每种格式只写一次编解码
//! （见 [`crate::proxy::codec`]），任意两种格式的桥接由两端编解码拼接而成。
//!
//! - [`ChatRequest`] — 一次对话请求（system、消息、工具、采样参数）
//! - [`ChatResponse`] — 一次完整的助手回复
//! - [`Part`] — 消息内容块（文本、图片、工具调用 / 结果、reasoning）
//! - [`StreamEvent`] — 流式回复的事件序列
//!
//! usage 与停止原因直接复用 [`UsageIR`] 与 [`StopReason`]，不另设表示。
//!
//! 非流式即「流式 + 聚合」：[`ChatResponse`] 定义为 [`StreamEvent`] 序列经
//! [`StreamAggregator`] 折叠的结果，[`ChatResponse::to_events`] 是其逆操作。
//! 不变量：`aggregate(response.to_events()) == response`，且与增量如何切分、
//! 不同内容块的增量如何交错无关（见本文件的 proptest）。

// 桥接从 CCS-176 起逐个迁移到编解码；在此之前部分 IR 与编解码项尚无调用方。
#![allow(dead_code)]

use std::collections::HashMap;

use serde_json::{Map, Value};

use crate::proxy::protocol::StopReason;
use crate::proxy::usage::ir::UsageIR;

/// 消息角色
///
/// system 提示单独放在 [`ChatRequest::system`]；工具结果以 [`Part::ToolResult`]
/// 放在 user 消息中（与 Anthropic Messages 一致，OpenAI 的 `tool` 角色由编解码映射）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
}

/// 媒体内容来源
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MediaSource {
    /// 内联 base64 数据
    Base64 { media_type: String, data: String },
    /// 远程 URL
    Url { url: String },
    /// 内联纯文本（Anthropic 文档的 `text` / `content` 源）；无法读取内容的
    /// 文档来源也以空文本表示，由目标编解码决定如何占位
    Text { data: String },
    /// OpenAI 的文件引用：`file_id` 与 / 或原样的 `file_data`（data URL）
    File {
        file_id: Option<String>,
        file_data: Option<String>,
    },
}

/// 消息内容块
#[derive(Debug, Clone, PartialEq)]
pub enum Part {
    /// 纯文本
    Text { text: String },
    /// 图片
    Image { source: MediaSource },
    /// 文档（PDF 等）
    Document {
        source: MediaSource,
        title: Option<String>,
    },
    /// 助手发起的工具调用
    ///
    /// `arguments` 保留原始 JSON 文本：流式增量是参数文本的片段，Chat 协议本身也
    /// 以字符串传递参数；需要对象时由编解码在编码阶段解析。
    ToolCall {
        id: String,
        name: String,
        arguments: String,
    },
    /// 工具执行结果
    ///
    /// `plain` 表示来源把结果作为单个字符串给出（此时 `content` 恰为一个文本块）；
    /// 区分「字符串」与「块数组」的格式（Anthropic `content`、Responses `output`）
    /// 据此还原原有形状，其余格式忽略它。
    ToolResult {
        call_id: String,
        content: Vec<Part>,
        is_error: bool,
        plain: bool,
    },
    /// 可见的 reasoning / thinking 文本，`signature` 为上游的不透明签名
    /// （Anthropic `signature`、Gemini `thoughtSignature`），回传时必须原样带回
    Reasoning {
        text: String,
        signature: Option<String>,
    },
    /// 不可见的加密 reasoning（Anthropic `redacted_thinking`、Responses
    /// `encrypted_content`），只能原样回传
    RedactedReasoning { data: String },
}

impl Part {
    /// 纯文本块
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text { text: text.into() }
    }

    /// 是否为工具调用
    pub fn is_tool_call(&self) -> bool {
        matches!(self, Self::ToolCall { .. })
    }

    /// 流式开始事件携带的初始块：可增量的字段清空，其余字段完整保留
    fn stream_start(&self) -> Self {
        match self {
            Self::Text { .. } => Self::text(""),
            Self::Reasoning { .. } => Self::Reasoning {
                text: String::new(),
                signature: None,
            },
            Self::ToolCall { id, name, .. } => Self::ToolCall {
                id: id.clone(),
                name: name.clone(),
                arguments: String::new(),
            },
            other => other.clone(),
        }
    }

    /// 补齐 [`Part::stream_start`] 清空的字段所需的增量
    fn stream_deltas(&self) -> Vec<PartDelta> {
        match self {
            Self::Text { text } if !text.is_empty() => vec![PartDelta::Text(text.clone())],
            Self::Reasoning { text, signature } => {
                let mut deltas = Vec::new();
                if !text.is_empty() {
                    deltas.push(PartDelta::Reasoning(text.clone()));
                }
                if let Some(signature) = signature {
                    deltas.push(PartDelta::Signature(signature.clone()));
                }
                deltas
            }
            Self::ToolCall { arguments, .. } if !arguments.is_empty() => {
                vec![PartDelta::ToolArguments(arguments.clone())]
            }
            _ => Vec::new(),
        }
    }

    /// 把增量应用到块上；增量类型与块类型不匹配时返回 `false`
    fn apply(&mut self, delta: PartDelta) -> bool {
        match (self, delta) {
            (Self::Text { text }, PartDelta::Text(more)) => text.push_str(&more),
            (Self::Reasoning { text, .. }, PartDelta::Reasoning(more)) => text.push_str(&more),
            (Self::Reasoning { signature, .. }, PartDelta::Signature(more)) => {
                signature.get_or_insert_with(String::new).push_str(&more)
            }
            (Self::ToolCall { arguments, .. }, PartDelta::ToolArguments(more)) => {
                arguments.push_str(&more)
            }
            _ => return false,
        }
        true
    }
}

/// 一条消息
#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    pub role: Role,
    pub parts: Vec<Part>,
}

impl Message {
    pub fn new(role: Role, parts: Vec<Part>) -> Self {
        Self { role, parts }
    }
}

/// 工具定义
#[derive(Debug, Clone, PartialEq)]
pub struct ToolDef {
    pub name: String,
    pub description: Option<String>,
    /// 参数的 JSON Schema
    pub input_schema: Value,
    /// 严格参数校验（OpenAI `strict`、Anthropic `strict`）
    pub strict: Option<bool>,
}

/// 工具选择策略
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolChoice {
    /// 由模型决定
    Auto,
    /// 禁止调用工具
    None,
    /// 必须调用某个工具（Anthropic `any`、OpenAI `required`）
    Any,
    /// 必须调用指定工具
    Tool { name: String },
}

/// reasoning / thinking 配置
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReasoningConfig {
    /// 思考 token 预算（Anthropic `budget_tokens`、Gemini `thinkingBudget`）
    pub budget_tokens: Option<u64>,
    /// 思考强度（OpenAI `reasoning_effort` / `reasoning.effort`、Anthropic
    /// `output_config.effort`），保留原始取值
    pub effort: Option<String>,
    /// 由模型自行决定思考深度（Anthropic `thinking.type = adaptive`）
    pub adaptive: bool,
}

/// 一次对话请求
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ChatRequest {
    pub model: String,
    /// system 提示（通常为文本块）
    pub system: Vec<Part>,
    pub messages: Vec<Message>,
    pub tools: Vec<ToolDef>,
    pub tool_choice: Option<ToolChoice>,
    pub max_tokens: Option<u64>,
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
    pub top_k: Option<u64>,
    pub stop: Vec<String>,
    pub reasoning: Option<ReasoningConfig>,
    pub stream: bool,
    /// IR 未建模的顶层字段，按来源格式原样保留；同格式编码时写回，
    /// 跨格式时由目标编解码决定取舍
    pub extra: Map<String, Value>,
}

/// 一次完整的助手回复
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ChatResponse {
    pub id: String,
    pub model: String,
    pub parts: Vec<Part>,
    pub stop_reason: Option<StopReason>,
    pub usage: Option<UsageIR>,
    /// 上游给出的创建时间（Unix 秒），没有时为 `None`
    pub created: Option<u64>,
}

impl ChatResponse {
    /// 是否包含工具调用（用于 [`StopReason`] 的 tool_use 覆盖规则）
    pub fn has_tool_call(&self) -> bool {
        self.parts.iter().any(Part::is_tool_call)
    }

    /// 展开为流式事件序列（非流式上游 → 流式客户端）
    ///
    /// 每个块依次产生 `PartStart` / 若干 `PartDelta` / `PartStop`，块序号即其在
    /// `parts` 中的位置；usage 在 `MessageStop` 之前发出。
    pub fn to_events(&self) -> Vec<StreamEvent> {
        let mut events = vec![StreamEvent::MessageStart {
            id: self.id.clone(),
            model: self.model.clone(),
            usage: None,
            created: self.created,
        }];
        for (index, part) in self.parts.iter().enumerate() {
            events.push(StreamEvent::PartStart {
                index,
                part: part.stream_start(),
            });
            events.extend(
                part.stream_deltas()
                    .into_iter()
                    .map(|delta| StreamEvent::PartDelta { index, delta }),
            );
            events.push(StreamEvent::PartStop { index });
        }
        if let Some(usage) = self.usage {
            events.push(StreamEvent::Usage(usage));
        }
        events.push(StreamEvent::MessageStop {
            stop_reason: self.stop_reason,
        });
        events
    }
}

/// 内容块增量（追加到对应块的可增量字段末尾）
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PartDelta {
    /// 追加到 [`Part::Text`]
    Text(String),
    /// 追加到 [`Part::Reasoning`] 的文本
    Reasoning(String),
    /// 追加到 [`Part::Reasoning`] 的签名
    Signature(String),
    /// 追加到 [`Part::ToolCall`] 的参数 JSON 文本
    ToolArguments(String),
}

/// 流式回复事件
///
/// 块以 `index` 标识（流内唯一，不要求连续）；不同块的增量可以交错
/// （例如 Chat 的并行工具调用），聚合后块按 `PartStart` 的先后排序。
#[derive(Debug, Clone, PartialEq)]
pub enum StreamEvent {
    /// 回复开始；`usage` 为开始时已知的 usage（例如输入 token），之后的
    /// [`StreamEvent::Usage`] 覆盖它
    MessageStart {
        id: String,
        model: String,
        usage: Option<UsageIR>,
        /// 上游给出的创建时间（Unix 秒，Chat `created`），没有时为 `None`
        created: Option<u64>,
    },
    /// 新内容块开始，`part` 为初始内容（可增量字段通常为空）
    PartStart { index: usize, part: Part },
    /// 内容块增量
    PartDelta { index: usize, delta: PartDelta },
    /// 内容块结束
    PartStop { index: usize },
    /// 截至当前的累计 usage；可多次出现，以最后一次为准
    Usage(UsageIR),
    /// 回复结束
    MessageStop { stop_reason: Option<StopReason> },
    /// 上游没有发出终止事件就结束了；解码端随后补发的 `MessageStop` 是截断收尾，
    /// 未完成的块不能当作完整内容（例如工具参数可能不完整）
    UpstreamTruncated,
    /// 上游在流中报告的错误；`code` 为上游给出的错误类型（如 `overloaded_error`）
    Error {
        message: String,
        code: Option<String>,
    },
}

/// 事件序列违反 [`StreamEvent`] 约定（编解码实现错误或上游流损坏）
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AggregateError {
    #[error("content part {0} started twice")]
    DuplicatePart(usize),
    #[error("content part {0} was never started")]
    UnknownPart(usize),
    #[error("content part {0} already stopped")]
    PartClosed(usize),
    #[error("delta does not match the kind of content part {0}")]
    DeltaMismatch(usize),
    #[error("event after message_stop")]
    AfterStop,
    #[error("upstream stream error: {0}")]
    Upstream(String),
}

/// 把 [`StreamEvent`] 序列折叠为 [`ChatResponse`]（流式上游 → 非流式客户端）
#[derive(Debug, Default)]
pub struct StreamAggregator {
    response: ChatResponse,
    /// 块序号 → (`parts` 中的位置, 是否仍可接收增量)
    slots: HashMap<usize, (usize, bool)>,
    stopped: bool,
}

impl StreamAggregator {
    pub fn new() -> Self {
        Self::default()
    }

    /// 是否已收到 `MessageStop`
    pub fn is_finished(&self) -> bool {
        self.stopped
    }

    /// 应用一个事件
    pub fn push(&mut self, event: StreamEvent) -> Result<(), AggregateError> {
        if self.stopped {
            return Err(AggregateError::AfterStop);
        }
        match event {
            StreamEvent::MessageStart {
                id,
                model,
                usage,
                created,
            } => {
                self.response.id = id;
                self.response.model = model;
                self.response.created = created;
                if usage.is_some() {
                    self.response.usage = usage;
                }
            }
            StreamEvent::PartStart { index, part } => {
                if self.slots.contains_key(&index) {
                    return Err(AggregateError::DuplicatePart(index));
                }
                self.slots.insert(index, (self.response.parts.len(), true));
                self.response.parts.push(part);
            }
            StreamEvent::PartDelta { index, delta } => {
                let position = self.open_slot(index)?;
                if !self.response.parts[position].apply(delta) {
                    return Err(AggregateError::DeltaMismatch(index));
                }
            }
            StreamEvent::PartStop { index } => {
                self.open_slot(index)?;
                if let Some(slot) = self.slots.get_mut(&index) {
                    slot.1 = false;
                }
            }
            StreamEvent::Usage(usage) => self.response.usage = Some(usage),
            StreamEvent::UpstreamTruncated => {}
            StreamEvent::MessageStop { stop_reason } => {
                self.response.stop_reason = stop_reason;
                self.stopped = true;
            }
            StreamEvent::Error { message, .. } => return Err(AggregateError::Upstream(message)),
        }
        Ok(())
    }

    fn open_slot(&self, index: usize) -> Result<usize, AggregateError> {
        match self.slots.get(&index) {
            Some((position, true)) => Ok(*position),
            Some((_, false)) => Err(AggregateError::PartClosed(index)),
            None => Err(AggregateError::UnknownPart(index)),
        }
    }

    /// 结束聚合；未收到 `MessageStop`（流被截断）时 `stop_reason` 保持为 `None`
    pub fn finish(self) -> ChatResponse {
        self.response
    }
}

/// 折叠完整的事件序列
pub fn aggregate(
    events: impl IntoIterator<Item = StreamEvent>,
) -> Result<ChatResponse, AggregateError> {
    let mut aggregator = StreamAggregator::new();
    for event in events {
        aggregator.push(event)?;
    }
    Ok(aggregator.finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proxy::usage::ir::InputSemantics;
    use proptest::prelude::*;

    fn usage() -> UsageIR {
        UsageIR {
            fresh_input: 10,
            cache_read: 4,
            cache_write: 2,
            output: 7,
            reasoning: 3,
            semantics: InputSemantics::Fresh,
        }
    }

    fn sample_response() -> ChatResponse {
        ChatResponse {
            id: "msg_1".into(),
            model: "claude".into(),
            parts: vec![
                Part::Reasoning {
                    text: "think".into(),
                    signature: Some("sig".into()),
                },
                Part::RedactedReasoning {
                    data: "opaque".into(),
                },
                Part::text("hello"),
                Part::ToolCall {
                    id: "call_1".into(),
                    name: "search".into(),
                    arguments: r#"{"q":"x"}"#.into(),
                },
            ],
            stop_reason: Some(StopReason::ToolUse),
            usage: Some(usage()),
            created: None,
        }
    }

    #[test]
    fn to_events_emits_start_deltas_stop_per_part() {
        let events = sample_response().to_events();
        assert_eq!(
            events.first(),
            Some(&StreamEvent::MessageStart {
                id: "msg_1".into(),
                model: "claude".into(),
                usage: None,
                created: None,
            })
        );
        assert_eq!(
            events.last(),
            Some(&StreamEvent::MessageStop {
                stop_reason: Some(StopReason::ToolUse)
            })
        );
        assert!(events.contains(&StreamEvent::PartStart {
            index: 3,
            part: Part::ToolCall {
                id: "call_1".into(),
                name: "search".into(),
                arguments: String::new(),
            },
        }));
        assert!(events.contains(&StreamEvent::PartDelta {
            index: 0,
            delta: PartDelta::Signature("sig".into()),
        }));
        // 不可增量的块在开始事件里完整给出，无增量
        assert!(events.contains(&StreamEvent::PartStart {
            index: 1,
            part: Part::RedactedReasoning {
                data: "opaque".into()
            },
        }));
        assert!(!events
            .iter()
            .any(|e| matches!(e, StreamEvent::PartDelta { index: 1, .. })));
    }

    #[test]
    fn aggregate_inverts_to_events() {
        let response = sample_response();
        assert_eq!(aggregate(response.to_events()), Ok(response.clone()));
        assert!(response.has_tool_call());
    }

    #[test]
    fn interleaved_parallel_tool_calls_aggregate_in_start_order() {
        let call = |id: &str| Part::ToolCall {
            id: id.into(),
            name: "f".into(),
            arguments: String::new(),
        };
        let args = |index, s: &str| StreamEvent::PartDelta {
            index,
            delta: PartDelta::ToolArguments(s.into()),
        };
        let events = vec![
            StreamEvent::PartStart {
                index: 5,
                part: call("b"),
            },
            StreamEvent::PartStart {
                index: 2,
                part: call("a"),
            },
            args(2, "{\"x\":"),
            args(5, "{}"),
            args(2, "1}"),
            StreamEvent::PartStop { index: 2 },
            StreamEvent::PartStop { index: 5 },
            StreamEvent::MessageStop {
                stop_reason: Some(StopReason::ToolUse),
            },
        ];
        let response = aggregate(events).unwrap();
        let ids_and_args: Vec<_> = response
            .parts
            .iter()
            .map(|p| match p {
                Part::ToolCall { id, arguments, .. } => (id.as_str(), arguments.as_str()),
                other => panic!("unexpected part {other:?}"),
            })
            .collect();
        assert_eq!(ids_and_args, vec![("b", "{}"), ("a", "{\"x\":1}")]);
    }

    #[test]
    fn usage_keeps_last_snapshot() {
        let mut later = usage();
        later.output = 99;
        let response = aggregate([StreamEvent::Usage(usage()), StreamEvent::Usage(later)]).unwrap();
        assert_eq!(response.usage, Some(later));
    }

    #[test]
    fn message_start_usage_is_overridden_by_later_usage() {
        let start = |usage| StreamEvent::MessageStart {
            id: "msg_1".into(),
            model: "m".into(),
            usage,
            created: None,
        };
        let response = aggregate([start(Some(usage()))]).unwrap();
        assert_eq!(response.usage, Some(usage()));

        let mut later = usage();
        later.output = 42;
        let response = aggregate([start(Some(usage())), StreamEvent::Usage(later)]).unwrap();
        assert_eq!(response.usage, Some(later));

        // 开始事件不带 usage 时不清除之前的 usage
        let response = aggregate([StreamEvent::Usage(later), start(None)]).unwrap();
        assert_eq!(response.usage, Some(later));
    }

    #[test]
    fn truncated_stream_has_no_stop_reason() {
        let mut aggregator = StreamAggregator::new();
        aggregator
            .push(StreamEvent::PartStart {
                index: 0,
                part: Part::text("partial"),
            })
            .unwrap();
        assert!(!aggregator.is_finished());
        let response = aggregator.finish();
        assert_eq!(response.stop_reason, None);
        assert_eq!(response.parts, vec![Part::text("partial")]);
    }

    #[test]
    fn contract_violations_are_reported() {
        let start = |index| StreamEvent::PartStart {
            index,
            part: Part::text(""),
        };
        let text = |index| StreamEvent::PartDelta {
            index,
            delta: PartDelta::Text("x".into()),
        };
        let stop = StreamEvent::MessageStop { stop_reason: None };
        let cases: Vec<(Vec<StreamEvent>, AggregateError)> = vec![
            (vec![start(0), start(0)], AggregateError::DuplicatePart(0)),
            (vec![text(1)], AggregateError::UnknownPart(1)),
            (
                vec![start(0), StreamEvent::PartStop { index: 0 }, text(0)],
                AggregateError::PartClosed(0),
            ),
            (
                vec![
                    start(0),
                    StreamEvent::PartDelta {
                        index: 0,
                        delta: PartDelta::ToolArguments("{}".into()),
                    },
                ],
                AggregateError::DeltaMismatch(0),
            ),
            (vec![stop.clone(), start(0)], AggregateError::AfterStop),
            (
                vec![StreamEvent::Error {
                    message: "overloaded".into(),
                    code: None,
                }],
                AggregateError::Upstream("overloaded".into()),
            ),
        ];
        for (events, expected) in cases {
            assert_eq!(aggregate(events.clone()), Err(expected), "{events:?}");
        }
    }

    #[test]
    fn signature_delta_without_text_creates_signature() {
        let response = aggregate([
            StreamEvent::PartStart {
                index: 0,
                part: Part::Reasoning {
                    text: String::new(),
                    signature: None,
                },
            },
            StreamEvent::PartDelta {
                index: 0,
                delta: PartDelta::Signature(String::new()),
            },
        ])
        .unwrap();
        assert_eq!(
            response.parts,
            vec![Part::Reasoning {
                text: String::new(),
                signature: Some(String::new()),
            }]
        );
    }

    // ---- proptest：流式 ⇄ 聚合往返无损 ----

    fn arb_text() -> impl Strategy<Value = String> {
        "\\PC{0,24}"
    }

    fn arb_media() -> impl Strategy<Value = MediaSource> {
        prop_oneof![
            (arb_text(), arb_text())
                .prop_map(|(media_type, data)| MediaSource::Base64 { media_type, data }),
            arb_text().prop_map(|url| MediaSource::Url { url }),
            arb_text().prop_map(|data| MediaSource::Text { data }),
        ]
    }

    fn arb_part() -> impl Strategy<Value = Part> {
        let leaf = prop_oneof![
            arb_text().prop_map(|text| Part::Text { text }),
            arb_media().prop_map(|source| Part::Image { source }),
            (arb_media(), proptest::option::of(arb_text()))
                .prop_map(|(source, title)| Part::Document { source, title }),
            (arb_text(), arb_text(), arb_text()).prop_map(|(id, name, arguments)| {
                Part::ToolCall {
                    id,
                    name,
                    arguments,
                }
            }),
            (arb_text(), proptest::option::of(arb_text()))
                .prop_map(|(text, signature)| Part::Reasoning { text, signature }),
            arb_text().prop_map(|data| Part::RedactedReasoning { data }),
        ];
        leaf.prop_recursive(2, 8, 3, |inner| {
            (
                arb_text(),
                proptest::collection::vec(inner, 0..3),
                any::<bool>(),
            )
                .prop_map(|(call_id, content, is_error)| Part::ToolResult {
                    call_id,
                    content,
                    is_error,
                    plain: false,
                })
        })
    }

    fn arb_stop_reason() -> impl Strategy<Value = Option<StopReason>> {
        proptest::option::of(prop_oneof![
            Just(StopReason::EndTurn),
            Just(StopReason::MaxTokens),
            Just(StopReason::ToolUse),
            Just(StopReason::Refusal),
        ])
    }

    fn arb_usage() -> impl Strategy<Value = Option<UsageIR>> {
        proptest::option::of(
            (
                0u64..1_000_000,
                0u64..1_000_000,
                0u64..1_000_000,
                0u64..1_000_000,
            )
                .prop_map(|(fresh_input, cache_read, cache_write, output)| UsageIR {
                    fresh_input,
                    cache_read,
                    cache_write,
                    output,
                    reasoning: output / 2,
                    semantics: InputSemantics::Total,
                }),
        )
    }

    fn arb_response() -> impl Strategy<Value = ChatResponse> {
        (
            arb_text(),
            arb_text(),
            proptest::collection::vec(arb_part(), 0..6),
            arb_stop_reason(),
            arb_usage(),
        )
            .prop_map(|(id, model, parts, stop_reason, usage)| ChatResponse {
                id,
                model,
                parts,
                stop_reason,
                usage,
                created: None,
            })
    }

    /// 在字符边界把字符串切成若干片（`cuts` 为切点种子）
    fn split_chars(text: &str, cuts: &[usize]) -> Vec<String> {
        let chars: Vec<char> = text.chars().collect();
        if chars.is_empty() {
            return vec![String::new()];
        }
        let mut points: Vec<usize> = cuts.iter().map(|c| c % chars.len()).collect();
        points.push(chars.len());
        points.sort_unstable();
        points.dedup();
        let mut pieces = Vec::new();
        let mut from = 0;
        for to in points {
            if to > from {
                pieces.push(chars[from..to].iter().collect());
                from = to;
            }
        }
        pieces
    }

    fn split_delta(delta: &PartDelta, cuts: &[usize]) -> Vec<PartDelta> {
        let (text, make): (&str, fn(String) -> PartDelta) = match delta {
            PartDelta::Text(t) => (t, PartDelta::Text),
            PartDelta::Reasoning(t) => (t, PartDelta::Reasoning),
            PartDelta::Signature(t) => (t, PartDelta::Signature),
            PartDelta::ToolArguments(t) => (t, PartDelta::ToolArguments),
        };
        split_chars(text, cuts).into_iter().map(make).collect()
    }

    /// 重新切分增量，并按 `order` 种子把各块的事件交错（每块内部顺序不变，
    /// 所有块先全部开始，保证聚合后的块顺序与 `PartStart` 顺序一致）
    fn rechunk_and_interleave(
        events: Vec<StreamEvent>,
        cuts: &[usize],
        order: &[usize],
    ) -> Vec<StreamEvent> {
        let mut head = Vec::new();
        let mut tail = Vec::new();
        let mut starts = Vec::new();
        let mut per_part: Vec<Vec<StreamEvent>> = Vec::new();
        for event in events {
            match event {
                StreamEvent::MessageStart { .. } => head.push(event),
                StreamEvent::PartStart { .. } => {
                    starts.push(event);
                    per_part.push(Vec::new());
                }
                StreamEvent::PartDelta { index, ref delta } => {
                    per_part[index].extend(
                        split_delta(delta, cuts)
                            .into_iter()
                            .map(|delta| StreamEvent::PartDelta { index, delta }),
                    );
                }
                StreamEvent::PartStop { index } => per_part[index].push(event),
                _ => tail.push(event),
            }
        }
        let mut out = head;
        out.extend(starts);
        let mut queues: Vec<std::collections::VecDeque<StreamEvent>> =
            per_part.into_iter().map(Into::into).collect();
        let mut seeds = order.iter().cycle();
        while queues.iter().any(|q| !q.is_empty()) {
            let live: Vec<usize> = (0..queues.len())
                .filter(|&i| !queues[i].is_empty())
                .collect();
            let pick = live[seeds.next().copied().unwrap_or(0) % live.len()];
            out.extend(queues[pick].pop_front());
        }
        out.extend(tail);
        out
    }

    proptest! {
        /// 非流式 = 流式 + 聚合：展开后再折叠得到原回复
        #[test]
        fn response_round_trips_through_events(response in arb_response()) {
            prop_assert_eq!(aggregate(response.to_events()), Ok(response));
        }

        /// 增量的切分方式与块之间的交错不影响聚合结果
        #[test]
        fn aggregation_is_independent_of_chunking_and_interleaving(
            response in arb_response(),
            cuts in proptest::collection::vec(any::<usize>(), 0..6),
            order in proptest::collection::vec(any::<usize>(), 1..8),
        ) {
            let events = rechunk_and_interleave(response.to_events(), &cuts, &order);
            prop_assert_eq!(aggregate(events), Ok(response));
        }
    }
}
