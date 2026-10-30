//! Responses 请求 → IR：Chat 上游的消息归一化（旧 `transform_codex_chat` 的规则）
//!
//! 先按旧转换器的算法生成与 Chat 消息一一对应的中间列表（含 reasoning 的归属），
//! 再转换为 IR 消息：助手消息的 reasoning 作为开头的 [`Part::Reasoning`]，工具调用消息
//! 是只含工具调用的助手消息，tool 消息是只含一个工具结果的用户消息。

use serde_json::{json, Value};

use crate::proxy::codec::responses::tools::{
    CodexToolContext, CUSTOM_TOOL_INPUT_FIELD, TOOL_SEARCH_PROXY_NAME,
};
use crate::proxy::json_canonical::{
    canonical_json_string, canonicalize_json_string_if_parseable, canonicalize_tool_arguments,
};
use crate::proxy::protocol::ir::{MediaSource, Message, Part, Role, ToolChoice};
use crate::proxy::providers::codex_chat_common::{
    extract_reasoning_field_text, extract_reasoning_summary_text,
};

/// Chat 消息的内容
#[derive(Debug)]
enum Content {
    Null,
    Text(String),
    Parts(Vec<Part>),
}

/// 与一条 Chat 消息对应的中间条目
#[derive(Debug)]
enum ChatItem {
    System(String),
    Message {
        role: Role,
        content: Content,
        reasoning: Option<String>,
    },
    ToolCalls {
        calls: Vec<Part>,
        reasoning: Option<String>,
    },
    Tool {
        call_id: String,
        content: String,
    },
}

impl ChatItem {
    fn is_assistant(&self) -> bool {
        matches!(
            self,
            ChatItem::Message {
                role: Role::Assistant,
                ..
            } | ChatItem::ToolCalls { .. }
        )
    }

    fn reasoning_mut(&mut self) -> Option<&mut Option<String>> {
        match self {
            ChatItem::Message {
                role: Role::Assistant,
                reasoning,
                ..
            }
            | ChatItem::ToolCalls { reasoning, .. } => Some(reasoning),
            _ => None,
        }
    }
}

/// `reasoning_content` 追加（已有内容时以空行分隔），空白不追加
fn append_reasoning(target: &mut Option<String>, reasoning: &str) {
    let reasoning = reasoning.trim();
    if reasoning.is_empty() {
        return;
    }
    match target {
        Some(existing) if !existing.is_empty() => {
            existing.push_str("\n\n");
            existing.push_str(reasoning);
        }
        _ => *target = Some(reasoning.to_string()),
    }
}

#[derive(Default)]
struct State {
    items: Vec<ChatItem>,
    pending_calls: Vec<Part>,
    pending_reasoning: Option<String>,
    last_assistant: Option<usize>,
}

impl State {
    fn append_pending(&mut self, reasoning: Option<String>) {
        if let Some(reasoning) = reasoning {
            append_reasoning(&mut self.pending_reasoning, &reasoning);
        }
    }

    /// 工具调用条目自带的 reasoning：已包含时不重复追加
    fn append_pending_unique(&mut self, reasoning: Option<String>) {
        let Some(reasoning) = reasoning else {
            return;
        };
        let reasoning = reasoning.trim();
        if reasoning.is_empty() {
            return;
        }
        match &mut self.pending_reasoning {
            Some(existing) if existing.contains(reasoning) => {}
            Some(existing) if !existing.is_empty() => {
                existing.push_str("\n\n");
                existing.push_str(reasoning);
            }
            _ => self.pending_reasoning = Some(reasoning.to_string()),
        }
    }

    fn take_pending_into(&mut self, target: &mut Option<String>) {
        if let Some(reasoning) = self.pending_reasoning.take() {
            if !reasoning.trim().is_empty() {
                append_reasoning(target, &reasoning);
            }
        }
    }

    /// 尾部 / 回合边界：未消费的 reasoning 回溯附加到上一条助手消息，然后丢弃
    fn attach_pending_to_previous_assistant(&mut self) {
        let Some(reasoning) = self.pending_reasoning.take() else {
            return;
        };
        if reasoning.trim().is_empty() {
            return;
        }
        if let Some(target) = self
            .last_assistant
            .and_then(|index| self.items.get_mut(index))
            .and_then(ChatItem::reasoning_mut)
        {
            append_reasoning(target, &reasoning);
        }
    }

    fn flush_calls(&mut self) {
        if self.pending_calls.is_empty() {
            return;
        }
        let mut reasoning = None;
        self.take_pending_into(&mut reasoning);
        self.last_assistant = Some(self.items.len());
        self.items.push(ChatItem::ToolCalls {
            calls: std::mem::take(&mut self.pending_calls),
            reasoning,
        });
    }

    /// 追加一条消息并更新「上一条助手消息」；tool 消息不改变它
    fn push(&mut self, item: ChatItem) {
        match &item {
            item if item.is_assistant() => self.last_assistant = Some(self.items.len()),
            ChatItem::Tool { .. } => {}
            _ => self.last_assistant = None,
        }
        self.items.push(item);
    }

    /// 用户等回合边界消息：pending reasoning 不跨回合泄漏，回溯附加到上一条助手消息
    fn push_message(&mut self, role: ChatRole, content: Content, own_reasoning: Option<String>) {
        match role {
            ChatRole::Assistant => {
                self.append_pending(own_reasoning);
                let mut reasoning = None;
                self.take_pending_into(&mut reasoning);
                self.push(ChatItem::Message {
                    role: Role::Assistant,
                    content,
                    reasoning,
                });
            }
            other => {
                self.attach_pending_to_previous_assistant();
                let item = match other {
                    ChatRole::System => ChatItem::System(match content {
                        Content::Text(text) => text,
                        Content::Parts(parts) => parts
                            .iter()
                            .filter_map(|part| match part {
                                Part::Text { text } => Some(text.as_str()),
                                _ => None,
                            })
                            .collect::<Vec<_>>()
                            .join("\n"),
                        Content::Null => String::new(),
                    }),
                    _ => ChatItem::Message {
                        role: Role::User,
                        content,
                        reasoning: None,
                    },
                };
                self.push(item);
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChatRole {
    System,
    User,
    Assistant,
}

fn chat_role(role: &str) -> ChatRole {
    match role {
        "system" | "developer" => ChatRole::System,
        "assistant" => ChatRole::Assistant,
        // tool 角色的消息没有 tool_call_id，按用户消息发送
        _ => ChatRole::User,
    }
}

/// `instructions`：字符串，或以空行拼接的文本块
fn instruction_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|part| {
                part.get("text")
                    .and_then(Value::as_str)
                    .or_else(|| part.as_str())
            })
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n"),
        _ => String::new(),
    }
}

/// Responses content → Chat content：只有文本时以换行拼接为字符串，否则为 part 列表
fn content(value: &Value) -> Content {
    let parts = match value {
        Value::Null => return Content::Null,
        Value::String(text) => return Content::Text(text.clone()),
        Value::Array(parts) => parts,
        _ => return Content::Null,
    };
    let mut converted = Vec::new();
    let mut has_non_text = false;
    for part in parts {
        let text_field = |key: &str| {
            part.get(key)
                .and_then(Value::as_str)
                .filter(|text| !text.is_empty())
                .map(Part::text)
        };
        match part.get("type").and_then(Value::as_str).unwrap_or_default() {
            "input_text" | "output_text" | "text" => converted.extend(text_field("text")),
            "refusal" => converted.extend(text_field("refusal")),
            "input_image" => {
                if let Some(image_url) = part.get("image_url") {
                    let url = image_url
                        .as_str()
                        .or_else(|| image_url.get("url").and_then(Value::as_str))
                        .unwrap_or_default();
                    converted.push(Part::Image {
                        source: MediaSource::Url {
                            url: url.to_string(),
                        },
                    });
                    has_non_text = true;
                }
            }
            "input_file" => {
                let file_id = part.get("file_id").and_then(Value::as_str);
                let file_data = part.get("file_data").and_then(Value::as_str);
                if file_id.is_some() || file_data.is_some() {
                    converted.push(Part::Document {
                        source: MediaSource::File {
                            file_id: file_id.map(str::to_string),
                            file_data: file_data.map(str::to_string),
                        },
                        title: part
                            .get("filename")
                            .and_then(Value::as_str)
                            .map(str::to_string),
                    });
                    has_non_text = true;
                }
            }
            _ => {}
        }
    }
    if has_non_text {
        return Content::Parts(converted);
    }
    Content::Text(
        converted
            .iter()
            .filter_map(|part| match part {
                Part::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

fn call_id(item: &Value) -> String {
    item.get("call_id")
        .or_else(|| item.get("id"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn tool_call(item: &Value, item_type: &str, tool_context: &CodexToolContext) -> Part {
    let (name, arguments) = match item_type {
        "function_call" => (
            tool_context.chat_name_for_response_function(
                item.get("name").and_then(Value::as_str).unwrap_or_default(),
                item.get("namespace").and_then(Value::as_str),
            ),
            canonicalize_tool_arguments(item.get("arguments")),
        ),
        "custom_tool_call" => (
            item.get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            canonical_json_string(&json!({
                CUSTOM_TOOL_INPUT_FIELD: item.get("input").cloned().unwrap_or_else(|| json!(""))
            })),
        ),
        _ => (
            TOOL_SEARCH_PROXY_NAME.to_string(),
            item.get("arguments")
                .map(canonical_json_string)
                .unwrap_or_else(|| "{}".to_string()),
        ),
    };
    Part::ToolCall {
        id: call_id(item),
        name,
        arguments,
    }
}

/// 生成 (system, messages)
pub(super) fn messages(body: &Value, tool_context: &CodexToolContext) -> (Vec<Part>, Vec<Message>) {
    let mut state = State::default();
    if let Some(instructions) = body.get("instructions") {
        let instructions = instruction_text(instructions);
        if !instructions.is_empty() {
            state.push(ChatItem::System(instructions));
        }
    }

    match body.get("input") {
        Some(Value::String(text)) => state.push(ChatItem::Message {
            role: Role::User,
            content: Content::Text(text.clone()),
            reasoning: None,
        }),
        Some(Value::Array(items)) => {
            for item in items {
                append_item(&mut state, item, tool_context);
            }
        }
        Some(item @ Value::Object(_)) => append_item(&mut state, item, tool_context),
        _ => {}
    }
    state.flush_calls();
    // 输入结束后仍未消费的 reasoning 属于真正的尾部，回溯附加到最后一条助手消息
    state.attach_pending_to_previous_assistant();

    let mut system = Vec::new();
    let mut messages = Vec::new();
    for item in state.items {
        match item {
            ChatItem::System(text) => system.push(Part::text(text)),
            ChatItem::Message {
                role,
                content,
                reasoning,
            } => {
                let mut parts: Vec<Part> = reasoning
                    .map(|text| Part::Reasoning {
                        text,
                        signature: None,
                    })
                    .into_iter()
                    .collect();
                match content {
                    Content::Null => {}
                    Content::Text(text) => parts.push(Part::text(text)),
                    Content::Parts(more) => parts.extend(more),
                }
                messages.push(Message::new(role, parts));
            }
            ChatItem::ToolCalls { calls, reasoning } => {
                let mut parts: Vec<Part> = reasoning
                    .map(|text| Part::Reasoning {
                        text,
                        signature: None,
                    })
                    .into_iter()
                    .collect();
                parts.extend(calls);
                messages.push(Message::new(Role::Assistant, parts));
            }
            ChatItem::Tool { call_id, content } => messages.push(Message::new(
                Role::User,
                vec![Part::ToolResult {
                    call_id,
                    content: vec![Part::text(content)],
                    is_error: false,
                    plain: true,
                }],
            )),
        }
    }
    (system, messages)
}

fn append_item(state: &mut State, item: &Value, tool_context: &CodexToolContext) {
    let item_type = item.get("type").and_then(Value::as_str);
    match item_type {
        Some(kind @ ("function_call" | "custom_tool_call" | "tool_search_call")) => {
            state.append_pending_unique(extract_reasoning_field_text(item));
            state
                .pending_calls
                .push(tool_call(item, kind, tool_context));
        }
        Some("function_call_output") => {
            state.flush_calls();
            let content = match item.get("output") {
                Some(Value::String(text)) => canonicalize_json_string_if_parseable(text),
                Some(value) => canonical_json_string(value),
                None => String::new(),
            };
            state.push(ChatItem::Tool {
                call_id: item
                    .get("call_id")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                content,
            });
        }
        Some("custom_tool_call_output" | "tool_search_output") => {
            state.flush_calls();
            state.push(ChatItem::Tool {
                call_id: item
                    .get("call_id")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                content: canonical_json_string(item),
            });
        }
        // reasoning 先进入 pending，向后附加到之后的消息 / 工具调用
        Some("reasoning") => state.append_pending(extract_reasoning_summary_text(item)),
        Some("input_text" | "input_image" | "input_file" | "input_audio") => {
            state.flush_calls();
            let role = chat_role(item.get("role").and_then(Value::as_str).unwrap_or("user"));
            state.push_message(role, content(&Value::Array(vec![item.clone()])), None);
        }
        _ => {
            state.flush_calls();
            if item.get("role").is_some() || item.get("content").is_some() {
                let role = chat_role(item.get("role").and_then(Value::as_str).unwrap_or("user"));
                let content = item.get("content").map(content).unwrap_or(Content::Null);
                let own_reasoning = (role == ChatRole::Assistant)
                    .then(|| extract_reasoning_field_text(item))
                    .flatten();
                state.push_message(role, content, own_reasoning);
            }
        }
    }
}

/// Responses → Chat 的 tool_choice；IR 无法表达的取值以 `Err(原值)` 返回，原样转发
pub(super) fn tool_choice(
    tool_choice: &Value,
    tool_context: &CodexToolContext,
) -> Result<ToolChoice, Value> {
    let str_field = |key: &str| tool_choice.get(key).and_then(Value::as_str);
    match tool_choice {
        Value::String(choice) => match choice.as_str() {
            "auto" => Ok(ToolChoice::Auto),
            "none" => Ok(ToolChoice::None),
            "required" => Ok(ToolChoice::Any),
            _ => Err(tool_choice.clone()),
        },
        Value::Object(_) => match str_field("type") {
            Some("function") => Ok(ToolChoice::Tool {
                name: tool_context.chat_name_for_response_function(
                    str_field("name").unwrap_or_default(),
                    str_field("namespace"),
                ),
            }),
            Some("tool_search") => Ok(ToolChoice::Tool {
                name: TOOL_SEARCH_PROXY_NAME.to_string(),
            }),
            Some("custom") => Ok(ToolChoice::Tool {
                name: str_field("name").unwrap_or_default().to_string(),
            }),
            _ => Err(tool_choice.clone()),
        },
        _ => Err(tool_choice.clone()),
    }
}
