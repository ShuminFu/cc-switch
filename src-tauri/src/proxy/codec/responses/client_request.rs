//! OpenAI Responses 请求体 → [`ChatRequest`]（Responses 作为客户端，Codex）
//!
//! 工具定义经 [`CodexToolContext`] 展开为扁平的函数工具（namespace / custom / tool_search
//! 与 Chat、Anthropic 上游共用同一套名字），历史中的工具调用同样换成扁平名。
//!
//! 两个旧转换器对同一个 Responses 请求的归一化规则不同，迁移期间按
//! [`ResponsesFlavor`] 保留：
//! - Anthropic 上游：`input` 条目按角色合并为消息（相邻同角色的块并入同一条消息，
//!   工具结果排在用户消息的其他块之前，回放的 thinking 排在助手消息开头），
//!   system / developer 条目的文本裁剪后并入 system，空白文本丢弃，
//!   `incomplete` 的历史工具调用丢弃，参数不是 JSON 对象的工具调用是无效请求；
//!   带本桥接信封的 reasoning 还原为签名 thinking，其余 reasoning 丢弃；
//! - Chat 上游：每个条目对应一条 Chat 消息（相邻的工具调用合并为一条助手消息），
//!   system / developer 消息并入 system；reasoning 条目（以及条目自带的
//!   `reasoning_content`）向后附加到下一条助手消息或工具调用消息，遇到用户消息或
//!   输入结束时回溯附加到上一条助手消息；工具输出预先渲染为 Chat tool 消息的文本。

mod chat;

use serde_json::{json, Value};

use super::reasoning::decode_anthropic_thinking_block;
use super::tools::{
    build_codex_tool_context_from_request, CodexToolContext, TOOL_SEARCH_PROXY_NAME,
};
use super::{
    invalid, sanitize_anthropic_tool_use_input, ResponsesFlavor, TOOL_RESULT_ERROR_MARKER,
};
use crate::proxy::codec::CodecError;
use crate::proxy::json_canonical::canonical_json_string;
use crate::proxy::protocol::ir::{
    ChatRequest, MediaSource, Message, Part, ReasoningConfig, Role, ToolChoice, ToolDef,
};

/// 已由 IR 字段建模的顶层字段；其余字段原样进入 [`ChatRequest::extra`]
/// （`reasoning` 对象也保留在其中：Chat 上游需要区分「带了 reasoning 但没有 effort」）
const MODELED_FIELDS: [&str; 9] = [
    "model",
    "instructions",
    "input",
    "max_output_tokens",
    "temperature",
    "top_p",
    "stream",
    "tools",
    "tool_choice",
];

pub(super) fn decode_request(
    body: &Value,
    flavor: ResponsesFlavor,
) -> Result<ChatRequest, CodecError> {
    let tool_context = build_codex_tool_context_from_request(body);
    let mut request = ChatRequest {
        model: body
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        max_tokens: body.get("max_output_tokens").and_then(Value::as_u64),
        temperature: body.get("temperature").and_then(Value::as_f64),
        top_p: body.get("top_p").and_then(Value::as_f64),
        stream: body.get("stream").and_then(Value::as_bool).unwrap_or(false),
        tools: decode_tools(&tool_context),
        reasoning: body
            .pointer("/reasoning/effort")
            .and_then(Value::as_str)
            .map(|effort| ReasoningConfig {
                effort: Some(effort.to_string()),
                ..ReasoningConfig::default()
            }),
        extra: body
            .as_object()
            .map(|object| {
                object
                    .iter()
                    .filter(|(key, _)| !MODELED_FIELDS.contains(&key.as_str()))
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect()
            })
            .unwrap_or_default(),
        ..ChatRequest::default()
    };

    match flavor {
        ResponsesFlavor::AnthropicUpstream => {
            request.system = anthropic_system(body);
            request.messages = anthropic_messages(body.get("input"), &tool_context)?;
            request.tool_choice = body
                .get("tool_choice")
                .map(|tool_choice| anthropic_tool_choice(tool_choice, &tool_context));
        }
        ResponsesFlavor::ChatUpstream => {
            let (system, messages) = chat::messages(body, &tool_context);
            request.system = system;
            request.messages = messages;
            match body
                .get("tool_choice")
                .map(|tool_choice| chat::tool_choice(tool_choice, &tool_context))
            {
                Some(Ok(tool_choice)) => request.tool_choice = Some(tool_choice),
                // 无法用 IR 表达的取值原样转发
                Some(Err(raw)) => {
                    request.extra.insert("tool_choice".into(), raw);
                }
                None => {}
            }
        }
    }
    Ok(request)
}

/// 工具上下文中的扁平函数工具
fn decode_tools(tool_context: &CodexToolContext) -> Vec<ToolDef> {
    tool_context
        .chat_tools()
        .iter()
        .filter_map(|tool| {
            let function = tool.get("function")?;
            Some(ToolDef {
                name: function
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                description: function
                    .get("description")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                input_schema: function.get("parameters").cloned().unwrap_or(Value::Null),
                strict: function.get("strict").and_then(Value::as_bool),
            })
        })
        .collect()
}

/// Anthropic 拒绝空白文本块
fn is_meaningful(text: &str) -> bool {
    !text.trim().is_empty()
}

/// `instructions` 与历史中的 system / developer 条目 → system 文本（裁剪后）
fn anthropic_system(body: &Value) -> Vec<Part> {
    let mut system = Vec::new();
    if let Some(instructions) = body
        .get("instructions")
        .and_then(Value::as_str)
        .filter(|text| is_meaningful(text))
    {
        system.push(Part::text(instructions.trim()));
    }
    for item in body
        .get("input")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if !matches!(
            item.get("role").and_then(Value::as_str),
            Some("system" | "developer")
        ) {
            continue;
        }
        match item.get("content") {
            Some(Value::String(text)) if is_meaningful(text) => {
                system.push(Part::text(text.trim()))
            }
            Some(Value::Array(parts)) => system.extend(
                parts
                    .iter()
                    .filter(|part| {
                        matches!(
                            part.get("type").and_then(Value::as_str),
                            Some("input_text" | "output_text" | "text")
                        )
                    })
                    .filter_map(|part| part.get("text").and_then(Value::as_str))
                    .filter(|text| is_meaningful(text))
                    .map(|text| Part::text(text.trim())),
            ),
            _ => {}
        }
    }
    system
}

/// Responses → Anthropic 的 tool_choice；无法识别的取值降级为 auto
fn anthropic_tool_choice(tool_choice: &Value, tool_context: &CodexToolContext) -> ToolChoice {
    let str_field = |key: &str| tool_choice.get(key).and_then(Value::as_str);
    match tool_choice {
        Value::String(choice) => match choice.as_str() {
            "required" => ToolChoice::Any,
            "none" => ToolChoice::None,
            _ => ToolChoice::Auto,
        },
        Value::Object(_) => match str_field("type") {
            Some("function") => ToolChoice::Tool {
                name: tool_context.chat_name_for_response_function(
                    str_field("name").unwrap_or_default(),
                    str_field("namespace"),
                ),
            },
            Some("custom") => ToolChoice::Tool {
                name: str_field("name").unwrap_or_default().to_string(),
            },
            Some("tool_search") => ToolChoice::Tool {
                name: TOOL_SEARCH_PROXY_NAME.to_string(),
            },
            // allowed_tools / 托管工具选择器等 Anthropic 不认识的形状
            _ => ToolChoice::Auto,
        },
        _ => ToolChoice::Auto,
    }
}

fn call_id(item: &Value) -> String {
    item.get("call_id")
        .and_then(Value::as_str)
        .or_else(|| item.get("id").and_then(Value::as_str))
        .unwrap_or_default()
        .to_string()
}

/// 追加到最后一条同角色消息，否则新建消息
fn push_part(messages: &mut Vec<Message>, role: Role, part: Part) {
    match messages.last_mut() {
        Some(last) if last.role == role => last.parts.push(part),
        _ => messages.push(Message::new(role, vec![part])),
    }
}

/// 工具结果必须排在用户消息的文本 / 图片块之前
fn push_tool_result(messages: &mut Vec<Message>, part: Part) {
    if let Some(last) = messages.last_mut().filter(|last| last.role == Role::User) {
        let at = last
            .parts
            .iter()
            .position(|part| !matches!(part, Part::ToolResult { .. }))
            .unwrap_or(last.parts.len());
        last.parts.insert(at, part);
        return;
    }
    messages.push(Message::new(Role::User, vec![part]));
}

/// 回放的 thinking 排在助手消息已有的 thinking 之后、其他块之前
fn push_thinking(messages: &mut Vec<Message>, part: Part) {
    if let Some(last) = messages
        .last_mut()
        .filter(|last| last.role == Role::Assistant)
    {
        let at = last
            .parts
            .iter()
            .take_while(|part| {
                matches!(
                    part,
                    Part::Reasoning { .. } | Part::RedactedReasoning { .. }
                )
            })
            .count();
        last.parts.insert(at, part);
        return;
    }
    push_part(messages, Role::Assistant, part);
}

fn anthropic_messages(
    input: Option<&Value>,
    tool_context: &CodexToolContext,
) -> Result<Vec<Message>, CodecError> {
    let items = match input {
        Some(Value::Array(items)) => items,
        Some(Value::String(text)) if is_meaningful(text) => {
            return Ok(vec![Message::new(
                Role::User,
                vec![Part::text(text.as_str())],
            )]);
        }
        _ => return Ok(Vec::new()),
    };

    let mut messages = Vec::new();
    for item in items {
        let item_type = item.get("type").and_then(Value::as_str);
        if matches!(
            item_type,
            Some("function_call" | "custom_tool_call" | "tool_search_call")
        ) && item.get("status").and_then(Value::as_str) == Some("incomplete")
        {
            log::warn!(
                "[Codex/Anthropic] Dropping incomplete historical tool call: type={}, call_id={}",
                item_type.unwrap_or("unknown"),
                call_id(item)
            );
            continue;
        }

        match item_type {
            Some("function_call") => {
                let name = item.get("name").and_then(Value::as_str).unwrap_or_default();
                let upstream_name = tool_context.chat_name_for_response_function(
                    name,
                    item.get("namespace").and_then(Value::as_str),
                );
                let raw = item
                    .get("arguments")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let input: Value = if raw.trim().is_empty() {
                    json!({})
                } else {
                    serde_json::from_str(raw).map_err(|error| {
                        invalid(format!(
                            "Invalid function_call arguments for '{name}': {error}"
                        ))
                    })?
                };
                if !input.is_object() {
                    return Err(invalid(format!(
                        "Function call arguments for '{name}' must be a JSON object"
                    )));
                }
                let input = sanitize_anthropic_tool_use_input(name, input);
                push_part(
                    &mut messages,
                    Role::Assistant,
                    Part::ToolCall {
                        id: call_id(item),
                        name: upstream_name,
                        arguments: input.to_string(),
                    },
                );
            }
            Some("custom_tool_call") => {
                let input = item.get("input").cloned().unwrap_or_else(|| json!(""));
                push_part(
                    &mut messages,
                    Role::Assistant,
                    Part::ToolCall {
                        id: call_id(item),
                        name: item
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        arguments: json!({ "input": input }).to_string(),
                    },
                );
            }
            Some("tool_search_call") => {
                let input = item
                    .get("arguments")
                    .cloned()
                    .filter(Value::is_object)
                    .unwrap_or_else(|| json!({}));
                push_part(
                    &mut messages,
                    Role::Assistant,
                    Part::ToolCall {
                        id: call_id(item),
                        name: TOOL_SEARCH_PROXY_NAME.to_string(),
                        arguments: input.to_string(),
                    },
                );
            }
            Some("function_call_output" | "custom_tool_call_output" | "tool_search_output") => {
                push_tool_result(&mut messages, tool_result(item));
            }
            Some("input_text") => {
                if let Some(text) = item
                    .get("text")
                    .and_then(Value::as_str)
                    .filter(|text| is_meaningful(text))
                {
                    push_part(&mut messages, Role::User, Part::text(text));
                }
            }
            Some("input_image") => {
                if let Some(image) = image_part(item) {
                    push_part(&mut messages, Role::User, image);
                }
            }
            Some("reasoning") => {
                if let Some(part) = item
                    .get("encrypted_content")
                    .and_then(Value::as_str)
                    .and_then(decode_anthropic_thinking_block)
                    .and_then(|block| thinking_part(&block))
                {
                    push_thinking(&mut messages, part);
                }
            }
            // message 条目或带 role 的条目
            _ => {
                let role = item.get("role").and_then(Value::as_str).unwrap_or("user");
                if matches!(role, "system" | "developer") {
                    continue;
                }
                let role = if role == "assistant" {
                    Role::Assistant
                } else {
                    Role::User
                };
                match item.get("content") {
                    Some(Value::String(text)) if is_meaningful(text) => {
                        push_part(&mut messages, role, Part::text(text.as_str()))
                    }
                    Some(Value::Array(parts)) => {
                        for part in parts {
                            let text = |key: &str| {
                                part.get(key)
                                    .and_then(Value::as_str)
                                    .filter(|text| is_meaningful(text))
                                    .map(Part::text)
                            };
                            let decoded = match part.get("type").and_then(Value::as_str) {
                                Some("input_text" | "output_text") => text("text"),
                                Some("refusal") => text("refusal"),
                                Some("input_image") => image_part(part),
                                Some("input_file") => document_part(part),
                                _ => None,
                            };
                            if let Some(decoded) = decoded {
                                push_part(&mut messages, role, decoded);
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    Ok(messages)
}

/// 信封里还原出的 Anthropic thinking 块
fn thinking_part(block: &Value) -> Option<Part> {
    let str_field = |key: &str| block.get(key).and_then(Value::as_str);
    match str_field("type")? {
        "thinking" => Some(Part::Reasoning {
            text: str_field("thinking").unwrap_or_default().to_string(),
            signature: str_field("signature").map(str::to_string),
        }),
        "redacted_thinking" => Some(Part::RedactedReasoning {
            data: str_field("data").unwrap_or_default().to_string(),
        }),
        _ => None,
    }
}

/// `input_image` → 图片：data URL 拆成 base64，http(s) URL 原样，其余丢弃
fn image_part(part: &Value) -> Option<Part> {
    let url = part.get("image_url").and_then(|value| {
        value
            .as_str()
            .or_else(|| value.get("url").and_then(Value::as_str))
    })?;
    let source = if let Some(rest) = url.strip_prefix("data:") {
        let (meta, data) = rest.split_once(',')?;
        MediaSource::Base64 {
            media_type: meta.split(';').next().unwrap_or("image/png").to_string(),
            data: data.to_string(),
        }
    } else if url.starts_with("http://") || url.starts_with("https://") {
        MediaSource::Url {
            url: url.to_string(),
        }
    } else {
        return None;
    };
    Some(Part::Image { source })
}

/// `input_file` → 文档：http(s) `file_url` 或非空的 data URL `file_data`
fn document_part(part: &Value) -> Option<Part> {
    let title = part
        .get("filename")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let source = if let Some(url) = part
        .get("file_url")
        .and_then(Value::as_str)
        .filter(|url| url.starts_with("http://") || url.starts_with("https://"))
    {
        MediaSource::Url {
            url: url.to_string(),
        }
    } else {
        let rest = part
            .get("file_data")
            .and_then(Value::as_str)?
            .strip_prefix("data:")?;
        let (meta, data) = rest.split_once(',')?;
        if data.is_empty() {
            return None;
        }
        MediaSource::Base64 {
            media_type: meta
                .split(';')
                .next()
                .unwrap_or("application/pdf")
                .to_string(),
            data: data.to_string(),
        }
    };
    Some(Part::Document { source, title })
}

/// 工具输出 → 工具结果：字符串原样；数组逐个转换，[`TOOL_RESULT_ERROR_MARKER`] 标记
/// 错误，无法转换的部分以 canonical JSON 文本保留；其他值（以及没有 output 的条目本身）
/// 转为 canonical JSON 字符串
fn tool_result(item: &Value) -> Part {
    let (content, is_error, plain) = match item.get("output") {
        Some(Value::String(text)) => (vec![Part::text(text.as_str())], false, true),
        Some(Value::Array(parts)) => {
            let mut content = Vec::new();
            let mut is_error = false;
            for part in parts {
                let fallback = || Part::text(canonical_json_string(part));
                match part.get("type").and_then(Value::as_str) {
                    Some("input_text" | "output_text") => {
                        if let Some(text) = part.get("text").and_then(Value::as_str) {
                            if text == TOOL_RESULT_ERROR_MARKER {
                                is_error = true;
                            } else {
                                content.push(Part::text(text));
                            }
                        }
                    }
                    Some("input_image") => content.push(image_part(part).unwrap_or_else(fallback)),
                    Some("input_file") => {
                        content.push(document_part(part).unwrap_or_else(fallback))
                    }
                    _ => content.push(fallback()),
                }
            }
            (content, is_error, false)
        }
        Some(value) => (vec![Part::text(canonical_json_string(value))], false, true),
        None => (vec![Part::text(canonical_json_string(item))], false, true),
    };
    Part::ToolResult {
        call_id: item
            .get("call_id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        content,
        is_error,
        plain,
    }
}
