//! Gemini `generateContent` 请求 → [`ChatRequest`]（Gemini 作为客户端，如 Gemini CLI）
//!
//! 模型名与是否流式不在请求体中（位于 URL 的 `models/{model}:generateContent` /
//! `:streamGenerateContent`），由调用方传入。解码保持宽松，与 Anthropic 请求解码一致：
//! 缺字段或类型不符时取默认值或跳过该部分，而不是拒绝整个请求。
//!
//! Gemini 与 IR 的结构差异：
//! - 工具调用的 `id` 可省略：没有 id 的 `functionCall` 按（消息, part）位置生成确定性 id，
//!   `functionResponse` 先按 id、再按工具名、最后按顺序与上一个助手轮次中尚未应答的
//!   调用配对，同一历史每次解码得到相同的 id；
//! - id 只保留 Anthropic 允许的字符（`[A-Za-z0-9_-]`），其余替换为 `_`；
//! - `thought: true` 的 part 是 reasoning 摘要，没有签名，不能回放给上游，丢弃；
//!   回复侧装进首个 `functionCall` 的 `thoughtSignature` 的签名 thinking（见
//!   [`super::client_response`]）在这里还原到助手消息开头；
//! - 相邻的同角色 content 合并为一条消息，用户消息中的工具结果移到最前面；
//! - 同时给出 `temperature` 与 `topP` 时只保留 `temperature`（较新的 Claude 模型拒绝两者并存），
//!   `temperature` 收紧到 Anthropic 的 `[0, 1]`；
//! - `responseMimeType: application/json`（Gemini CLI 的结构化输出调用）没有对应字段，
//!   改为在 system 提示末尾追加只输出 JSON 的约束（含 `responseJsonSchema` / `responseSchema`）。

use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde_json::{json, Value};

use super::client_response::decode_thinking_envelope;
use crate::proxy::json_canonical::canonical_json_string;
use crate::proxy::protocol::ir::{
    ChatRequest, MediaSource, Message, Part, ReasoningConfig, Role, ToolChoice, ToolDef,
};

pub(super) fn decode_request(body: &Value, model: &str, stream: bool) -> ChatRequest {
    let generation = field(body, "generationConfig", "generation_config");
    let mut request = ChatRequest {
        model: model.to_string(),
        system: decode_system(field(body, "systemInstruction", "system_instruction")),
        tools: decode_tools(body.get("tools")),
        tool_choice: decode_tool_choice(field(body, "toolConfig", "tool_config")),
        stream,
        ..ChatRequest::default()
    };
    if let Some(generation) = generation {
        request.max_tokens = field(generation, "maxOutputTokens", "max_output_tokens")
            .and_then(Value::as_u64)
            .filter(|value| *value > 0);
        // Gemini 的 temperature 范围是 [0, 2]，Anthropic 只接受 [0, 1]，超出部分以不可重试的
        // 400 拒绝：收紧到上游范围
        request.temperature = generation
            .get("temperature")
            .and_then(Value::as_f64)
            .map(|temperature| temperature.clamp(0.0, 1.0));
        // Gemini CLI 默认同时发送 temperature 与 topP；较新的 Claude 模型拒绝两者同时出现，
        // 有 temperature 时只保留 temperature
        request.top_p = field(generation, "topP", "top_p")
            .and_then(Value::as_f64)
            .filter(|_| request.temperature.is_none());
        request.top_k = field(generation, "topK", "top_k")
            .and_then(Value::as_f64)
            .filter(|value| *value >= 1.0)
            .map(|value| value as u64);
        request.stop = field(generation, "stopSequences", "stop_sequences")
            .and_then(Value::as_array)
            .map(|stops| {
                stops
                    .iter()
                    .filter_map(Value::as_str)
                    .filter(|stop| !stop.trim().is_empty())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        request.reasoning =
            field(generation, "thinkingConfig", "thinking_config").and_then(decode_thinking);
        if let Some(instruction) = json_output_instruction(generation) {
            request.system.push(Part::text(instruction));
        }
    }
    request.messages = decode_contents(body.get("contents"));
    request
}

/// camelCase 字段，缺省时取 snake_case 写法（REST API 两种都接受）
fn field<'a>(value: &'a Value, camel: &str, snake: &str) -> Option<&'a Value> {
    value.get(camel).or_else(|| value.get(snake))
}

fn str_field<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

/// `systemInstruction`：`{parts: [{text}]}`、单个 part 或字符串
fn decode_system(system: Option<&Value>) -> Vec<Part> {
    match system {
        Some(Value::String(text)) if !text.is_empty() => vec![Part::text(text.as_str())],
        Some(system @ Value::Object(_)) => match system.get("parts").and_then(Value::as_array) {
            Some(parts) => parts
                .iter()
                .filter_map(|part| str_field(part, "text"))
                .filter(|text| !text.is_empty())
                .map(Part::text)
                .collect(),
            None => str_field(system, "text")
                .filter(|text| !text.is_empty())
                .map(Part::text)
                .into_iter()
                .collect(),
        },
        _ => Vec::new(),
    }
}

/// `tools[].functionDeclarations`；`googleSearch` / `codeExecution` 等内置工具上游无法执行，丢弃
fn decode_tools(tools: Option<&Value>) -> Vec<ToolDef> {
    tools
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|tool| field(tool, "functionDeclarations", "function_declarations"))
        .filter_map(Value::as_array)
        .flatten()
        .filter_map(|declaration| {
            let name = str_field(declaration, "name")?.trim();
            if name.is_empty() {
                return None;
            }
            let input_schema = field(
                declaration,
                "parametersJsonSchema",
                "parameters_json_schema",
            )
            .cloned()
            .or_else(|| declaration.get("parameters").map(json_schema_from_gemini))
            .unwrap_or_else(|| json!({ "type": "object", "properties": {} }));
            Some(ToolDef {
                name: name.to_string(),
                description: str_field(declaration, "description").map(str::to_string),
                input_schema,
                strict: None,
            })
        })
        .collect()
}

/// Gemini `Schema`（OpenAPI 子集，`type` 可为大写枚举名）→ JSON Schema：类型名转小写，
/// `nullable: true` 并入类型数组
fn json_schema_from_gemini(schema: &Value) -> Value {
    match schema {
        Value::Object(object) => {
            let mut converted = serde_json::Map::new();
            for (key, value) in object {
                let value = match key.as_str() {
                    "type" => match value.as_str() {
                        Some(kind) => json!(kind.to_ascii_lowercase()),
                        None => value.clone(),
                    },
                    "properties" | "$defs" | "definitions" => match value.as_object() {
                        Some(properties) => Value::Object(
                            properties
                                .iter()
                                .map(|(name, schema)| {
                                    (name.clone(), json_schema_from_gemini(schema))
                                })
                                .collect(),
                        ),
                        None => value.clone(),
                    },
                    "items" | "additionalProperties" | "not" => json_schema_from_gemini(value),
                    "anyOf" | "oneOf" | "allOf" | "prefixItems" => match value.as_array() {
                        Some(items) => {
                            Value::Array(items.iter().map(json_schema_from_gemini).collect())
                        }
                        None => value.clone(),
                    },
                    "nullable" => continue,
                    _ => value.clone(),
                };
                converted.insert(key.clone(), value);
            }
            if object.get("nullable").and_then(Value::as_bool) == Some(true) {
                if let Some(kind) = converted.get("type").and_then(Value::as_str) {
                    let kind = kind.to_string();
                    converted.insert("type".into(), json!([kind, "null"]));
                }
            }
            Value::Object(converted)
        }
        other => other.clone(),
    }
}

/// `toolConfig.functionCallingConfig`：`ANY` 且只允许一个函数时即强制调用该函数
fn decode_tool_choice(tool_config: Option<&Value>) -> Option<ToolChoice> {
    let config = field(
        tool_config?,
        "functionCallingConfig",
        "function_calling_config",
    )?;
    let allowed: Vec<&str> = field(config, "allowedFunctionNames", "allowed_function_names")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    match str_field(config, "mode")?.to_ascii_uppercase().as_str() {
        "ANY" => Some(match allowed.as_slice() {
            [name] => ToolChoice::Tool {
                name: name.to_string(),
            },
            _ => ToolChoice::Any,
        }),
        "NONE" => Some(ToolChoice::None),
        "AUTO" | "VALIDATED" => Some(ToolChoice::Auto),
        _ => None,
    }
}

/// `thinkingConfig` → reasoning 配置
///
/// `thinkingBudget` 0 表示关闭；正数原样作为预算（上游编码端再按 `max_tokens` 收紧），
/// 同时按区间映射到 low / medium / high / max 强度，供只接受强度的 adaptive 模型使用；
/// `-1`（动态）不指定（由模型默认决定）。Gemini 3 的 `thinkingLevel` 直接作为强度。
fn decode_thinking(config: &Value) -> Option<ReasoningConfig> {
    if let Some(level) = field(config, "thinkingLevel", "thinking_level").and_then(Value::as_str) {
        let level = level.trim().to_ascii_lowercase();
        if !level.is_empty() && level != "thinking_level_unspecified" {
            return Some(ReasoningConfig {
                effort: Some(level),
                ..ReasoningConfig::default()
            });
        }
    }
    let budget = field(config, "thinkingBudget", "thinking_budget").and_then(Value::as_i64)?;
    let effort = match budget {
        0 => "none",
        1..=2048 => "low",
        2049..=8192 => "medium",
        8193..=16384 => "high",
        budget if budget > 0 => "max",
        _ => return None,
    };
    Some(ReasoningConfig {
        budget_tokens: u64::try_from(budget).ok().filter(|budget| *budget > 0),
        effort: Some(effort.to_string()),
        adaptive: false,
    })
}

/// 结构化输出调用（`responseMimeType: application/json`）的 JSON 约束提示
fn json_output_instruction(generation: &Value) -> Option<String> {
    let mime = field(generation, "responseMimeType", "response_mime_type")?.as_str()?;
    if !mime.eq_ignore_ascii_case("application/json") {
        return None;
    }
    let schema = field(generation, "responseJsonSchema", "response_json_schema")
        .cloned()
        .or_else(|| {
            field(generation, "responseSchema", "response_schema").map(json_schema_from_gemini)
        });
    let mut instruction = String::from(
        "Respond with a single valid JSON value only: no prose, no explanations and no Markdown code fences.",
    );
    if let Some(schema) = schema {
        instruction.push_str(" The JSON must conform to this JSON Schema: ");
        instruction.push_str(&canonical_json_string(&schema));
    }
    Some(instruction)
}

/// 历史中尚未得到 `functionResponse` 的工具调用
struct PendingCall {
    id: String,
    name: String,
}

fn decode_contents(contents: Option<&Value>) -> Vec<Message> {
    let mut messages: Vec<Message> = Vec::new();
    let mut pending: Vec<PendingCall> = Vec::new();
    for (message_index, content) in contents
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .enumerate()
    {
        let role = match str_field(content, "role") {
            Some("model") | Some("assistant") => Role::Assistant,
            // `user`、缺省，以及旧版 SDK 承载函数结果的 `function` / `tool`
            _ => Role::User,
        };
        let raw_parts = content
            .get("parts")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let parts = match role {
            Role::Assistant => {
                // 新的助手轮次开始，上一轮没有应答的调用不再配对
                if messages.last().map(|message| message.role) != Some(Role::Assistant) {
                    pending.clear();
                }
                decode_model_parts(raw_parts, message_index, &mut pending)
            }
            Role::User => decode_user_parts(raw_parts, message_index, &mut pending),
        };
        if parts.is_empty() {
            continue;
        }
        match messages.last_mut() {
            Some(last) if last.role == role => last.parts.extend(parts),
            _ => messages.push(Message::new(role, parts)),
        }
    }
    for message in &mut messages {
        if message.role == Role::User {
            // Anthropic 要求工具结果位于用户消息开头（稳定排序，保持相对顺序）
            message
                .parts
                .sort_by_key(|part| !matches!(part, Part::ToolResult { .. }));
        }
    }
    messages
}

fn decode_model_parts(
    raw_parts: &[Value],
    message_index: usize,
    pending: &mut Vec<PendingCall>,
) -> Vec<Part> {
    let mut reasoning = Vec::new();
    let mut parts = Vec::new();
    for (part_index, part) in raw_parts.iter().enumerate() {
        if let Some(call) = field(part, "functionCall", "function_call") {
            let name = str_field(call, "name").unwrap_or_default().to_string();
            let id = match str_field(call, "id").map(sanitize_tool_call_id) {
                Some(id) if !id.is_empty() => id,
                _ => positional_call_id(message_index, part_index),
            };
            if let Some(signature) =
                field(part, "thoughtSignature", "thought_signature").and_then(Value::as_str)
            {
                reasoning.extend(decode_thinking_envelope(signature));
            }
            let arguments = call
                .get("args")
                .filter(|args| args.is_object())
                .map(canonical_json_string)
                .unwrap_or_else(|| "{}".to_string());
            pending.push(PendingCall {
                id: id.clone(),
                name: name.clone(),
            });
            parts.push(Part::ToolCall {
                id,
                name,
                arguments,
            });
        } else if part.get("thought").and_then(Value::as_bool) == Some(true) {
            // 无签名的 reasoning 摘要无法回放给上游
        } else if let Some(decoded) = decode_media_or_text(part) {
            parts.push(decoded);
        }
    }
    // thinking 块必须位于助手消息开头
    reasoning.extend(parts);
    reasoning
}

fn decode_user_parts(
    raw_parts: &[Value],
    message_index: usize,
    pending: &mut Vec<PendingCall>,
) -> Vec<Part> {
    let mut parts = Vec::new();
    for (part_index, part) in raw_parts.iter().enumerate() {
        if let Some(response) = field(part, "functionResponse", "function_response") {
            let call_id = take_pending_call(pending, response)
                .unwrap_or_else(|| positional_call_id(message_index, part_index));
            parts.push(decode_function_response(response, call_id));
        } else if part.get("thought").and_then(Value::as_bool) == Some(true) {
            // 用户侧不应出现 reasoning；保守起见同样丢弃
        } else if let Some(decoded) = decode_media_or_text(part) {
            parts.push(decoded);
        }
    }
    parts
}

/// 为 `functionResponse` 找到对应的调用：先按 id，再按工具名，最后取最早的未应答调用
fn take_pending_call(pending: &mut Vec<PendingCall>, response: &Value) -> Option<String> {
    let id = str_field(response, "id")
        .map(sanitize_tool_call_id)
        .filter(|id| !id.is_empty());
    let name = str_field(response, "name").unwrap_or_default();
    let position = id
        .as_deref()
        .and_then(|id| pending.iter().position(|call| call.id == id))
        .or_else(|| pending.iter().position(|call| call.name == name))
        .or_else(|| (!pending.is_empty()).then_some(0));
    match position {
        Some(position) => Some(pending.remove(position).id),
        // 没有可配对的调用：保留客户端的 id（孤立的结果由上游编码端丢弃）
        None => id,
    }
}

/// `functionResponse.response`：Gemini CLI 以 `{output}` 返回结果、以 `{error}` 返回失败；
/// 其余形状按 JSON 文本传递
fn decode_function_response(response: &Value, call_id: String) -> Part {
    let payload = response.get("response").unwrap_or(&Value::Null);
    let (text, is_error) = match payload {
        Value::String(text) => (text.clone(), false),
        Value::Object(object) if object.len() == 1 && object.contains_key("output") => {
            (json_text(&object["output"]), false)
        }
        Value::Object(object) if object.contains_key("error") && !object.contains_key("output") => {
            (json_text(&object["error"]), true)
        }
        Value::Null => (String::new(), false),
        other => (canonical_json_string(other), false),
    };
    let mut content = vec![Part::text(text)];
    // 多模态函数结果（`functionResponse.parts`）的图片 / 文档随结果一起返回
    content.extend(
        response
            .get("parts")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(decode_media_or_text),
    );
    let plain = content.len() == 1;
    Part::ToolResult {
        call_id,
        content,
        is_error,
        plain,
    }
}

fn json_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => canonical_json_string(other),
    }
}

/// 文本、`inlineData` 与 `fileData`；上游无法表示的媒体类型以文本说明代替
fn decode_media_or_text(part: &Value) -> Option<Part> {
    if let Some(text) = str_field(part, "text") {
        return (!text.is_empty()).then(|| Part::text(text));
    }
    if let Some(inline) = field(part, "inlineData", "inline_data") {
        let mime = field(inline, "mimeType", "mime_type")
            .and_then(Value::as_str)
            .unwrap_or("application/octet-stream")
            .to_ascii_lowercase();
        let data = str_field(inline, "data").unwrap_or_default().to_string();
        return Some(if mime.starts_with("image/") {
            Part::Image {
                source: MediaSource::Base64 {
                    media_type: mime,
                    data,
                },
            }
        } else if mime == "application/pdf" {
            Part::Document {
                source: MediaSource::Base64 {
                    media_type: mime,
                    data,
                },
                title: None,
            }
        } else if mime.starts_with("text/") {
            match STANDARD
                .decode(data.as_bytes())
                .ok()
                .and_then(|bytes| String::from_utf8(bytes).ok())
            {
                Some(text) => Part::Document {
                    source: MediaSource::Text { data: text },
                    title: None,
                },
                None => unsupported_media(&mime),
            }
        } else {
            unsupported_media(&mime)
        });
    }
    if let Some(file) = field(part, "fileData", "file_data") {
        let mime = field(file, "mimeType", "mime_type")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_ascii_lowercase();
        let uri = field(file, "fileUri", "file_uri")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let is_http = uri.starts_with("https://") || uri.starts_with("http://");
        return Some(if is_http && mime.starts_with("image/") {
            Part::Image {
                source: MediaSource::Url {
                    url: uri.to_string(),
                },
            }
        } else if is_http && mime == "application/pdf" {
            Part::Document {
                source: MediaSource::Url {
                    url: uri.to_string(),
                },
                title: None,
            }
        } else {
            unsupported_media(if mime.is_empty() { uri } else { &mime })
        });
    }
    None
}

fn unsupported_media(kind: &str) -> Part {
    Part::text(format!(
        "[cc-switch: {kind} attachment omitted, the upstream model cannot read it]"
    ))
}

/// 客户端没有给出 id 的工具调用：按位置生成，同一历史每次解码结果相同
fn positional_call_id(message_index: usize, part_index: usize) -> String {
    format!("call_gemini_{message_index}_{part_index}")
}

/// Anthropic 的工具调用 id 只允许 `[A-Za-z0-9_-]`
fn sanitize_tool_call_id(id: &str) -> String {
    id.trim()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
                ch
            } else {
                '_'
            }
        })
        .collect()
}
