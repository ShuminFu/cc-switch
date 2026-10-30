//! [`ChatRequest`] → Gemini `generateContent` 请求体
//!
//! 字段写入顺序与旧转换器（`providers::transform_gemini`）一致。模型名在 URL 中，
//! 不写入请求体；thinking 配置与 `top_k` 旧转换器不转发，这里同样不转发。
//!
//! assistant 历史优先回放 shadow 中记录的原始 Gemini parts（带 `thoughtSignature`
//! 等客户端不会带回的状态）：先按工具调用 id / 名称匹配，再按位置对齐到最近的
//! 若干轮。工具结果的 `functionResponse.name` 依次从 shadow、请求里的工具调用与
//! 同一条消息中的工具调用解析。

use std::collections::{HashMap, HashSet};

use serde_json::{json, Map, Value};

use super::invalid;
use super::tool_calls::{is_synthesized_tool_call_id, shadow_parts};
use crate::proxy::codec::{json_number, CodecError};
use crate::proxy::protocol::ir::{ChatRequest, MediaSource, Message, Part, Role, ToolChoice};
use crate::proxy::providers::gemini_schema::build_gemini_function_declaration;
use crate::proxy::providers::gemini_shadow::GeminiAssistantTurn;

pub(super) fn encode_request(
    request: &ChatRequest,
    shadow_turns: &[GeminiAssistantTurn],
) -> Result<Value, CodecError> {
    let mut result = Map::new();

    if let Some(system) = encode_system(&request.system) {
        result.insert("systemInstruction".into(), system);
    }

    result.insert(
        "contents".into(),
        Value::Array(encode_contents(&request.messages, shadow_turns)?),
    );

    if let Some(config) = encode_generation_config(request) {
        result.insert("generationConfig".into(), config);
    }

    let declarations: Vec<Value> = request
        .tools
        .iter()
        .map(|tool| {
            build_gemini_function_declaration(
                &tool.name,
                tool.description.as_deref(),
                tool.input_schema.clone(),
            )
        })
        .collect();
    if !declarations.is_empty() {
        result.insert(
            "tools".into(),
            json!([{ "functionDeclarations": declarations }]),
        );
    }

    if let Some(tool_choice) = &request.tool_choice {
        result.insert("toolConfig".into(), encode_tool_choice(tool_choice));
    }

    Ok(Value::Object(result))
}

/// 非空文本块以空行连接为一个 system 文本
fn encode_system(system: &[Part]) -> Option<Value> {
    let texts: Vec<&str> = system
        .iter()
        .filter_map(|part| match part {
            Part::Text { text } if !text.is_empty() => Some(text.as_str()),
            _ => None,
        })
        .collect();
    (!texts.is_empty()).then(|| json!({ "parts": [{ "text": texts.join("\n\n") }] }))
}

fn encode_generation_config(request: &ChatRequest) -> Option<Value> {
    let mut config = Map::new();
    if let Some(max_tokens) = request.max_tokens {
        config.insert("maxOutputTokens".into(), json!(max_tokens));
    }
    if let Some(temperature) = request.temperature {
        config.insert("temperature".into(), json_number(temperature));
    }
    if let Some(top_p) = request.top_p {
        config.insert("topP".into(), json_number(top_p));
    }
    if !request.stop.is_empty() {
        config.insert("stopSequences".into(), json!(request.stop));
    }
    (!config.is_empty()).then_some(Value::Object(config))
}

fn encode_tool_choice(tool_choice: &ToolChoice) -> Value {
    let config = match tool_choice {
        ToolChoice::Auto => json!({ "mode": "AUTO" }),
        ToolChoice::None => json!({ "mode": "NONE" }),
        ToolChoice::Any => json!({ "mode": "ANY" }),
        ToolChoice::Tool { name } => json!({
            "mode": "ANY",
            "allowedFunctionNames": [name]
        }),
    };
    json!({ "functionCallingConfig": config })
}

/// 回放与工具结果解析所需的、跨消息累积的状态
struct ReplayState {
    /// 工具调用 id → 函数名（`functionResponse.name` 必填）
    tool_name_by_id: HashMap<String, String>,
    /// 工具调用 id → Gemini `thoughtSignature`
    thought_signature_by_id: HashMap<String, String>,
}

impl ReplayState {
    fn from_shadow(shadow_turns: &[GeminiAssistantTurn]) -> Self {
        let mut state = Self {
            tool_name_by_id: HashMap::new(),
            thought_signature_by_id: HashMap::new(),
        };
        for turn in shadow_turns {
            state.merge_shadow_turn(turn);
        }
        state
    }

    fn merge_shadow_turn(&mut self, turn: &GeminiAssistantTurn) {
        for tool_call in &turn.tool_calls {
            if let Some(id) = &tool_call.id {
                self.tool_name_by_id
                    .insert(id.clone(), tool_call.name.clone());
                if let Some(signature) = &tool_call.thought_signature {
                    self.thought_signature_by_id
                        .insert(id.clone(), signature.clone());
                }
            }
        }
        if let Some(parts) = shadow_parts(&turn.assistant_content) {
            self.merge_tool_names_from_parts(&parts);
        }
    }

    /// 已编码的 Gemini parts 中带 id 的 functionCall
    fn merge_tool_names_from_parts(&mut self, parts: &[Value]) {
        for function_call in parts.iter().filter_map(|part| part.get("functionCall")) {
            let id = function_call.get("id").and_then(Value::as_str);
            let name = function_call.get("name").and_then(Value::as_str);
            if let (Some(id), Some(name)) = (id, name) {
                if !id.is_empty() && !name.is_empty() {
                    self.tool_name_by_id
                        .insert(id.to_string(), name.to_string());
                }
            }
        }
    }
}

fn tool_calls(message: &Message) -> impl Iterator<Item = (&str, &str)> {
    message.parts.iter().filter_map(|part| match part {
        Part::ToolCall { id, name, .. } => Some((id.as_str(), name.as_str())),
        _ => None,
    })
}

fn encode_contents(
    messages: &[Message],
    shadow_turns: &[GeminiAssistantTurn],
) -> Result<Vec<Value>, CodecError> {
    let total_assistant_messages = messages
        .iter()
        .filter(|message| message.role == Role::Assistant)
        .count();
    // 只有最近的 total_assistant_messages 轮参与按位置对齐
    let effective_shadow_turns =
        &shadow_turns[shadow_turns.len().saturating_sub(total_assistant_messages)..];

    let mut state = ReplayState::from_shadow(shadow_turns);

    // 先登记请求历史中所有工具调用的名称：shadow 已淘汰对应轮次（长会话、
    // 重启、会话淘汰）时，工具结果仍能解析出函数名
    for message in messages.iter().filter(|m| m.role == Role::Assistant) {
        for (id, name) in tool_calls(message) {
            if !id.is_empty() && !name.is_empty() {
                state
                    .tool_name_by_id
                    .entry(id.to_string())
                    .or_insert_with(|| name.to_string());
            }
        }
    }

    let shadow_start_index = total_assistant_messages - effective_shadow_turns.len();
    let mut assistant_seen_index = 0usize;
    let mut used_shadow_indices = HashSet::new();
    let mut contents = Vec::with_capacity(messages.len());

    for message in messages {
        let parts = match message.role {
            Role::User => encode_parts(message, &mut state)?,
            Role::Assistant => {
                let positional_index = assistant_seen_index
                    .checked_sub(shadow_start_index)
                    .filter(|index| *index < effective_shadow_turns.len())
                    .filter(|index| !used_shadow_indices.contains(index));
                let matched_index = find_matching_shadow_turn(message, effective_shadow_turns)
                    .filter(|index| !used_shadow_indices.contains(index));
                assistant_seen_index += 1;

                let replayed = match matched_index.or(positional_index) {
                    Some(index) => {
                        used_shadow_indices.insert(index);
                        let turn = &effective_shadow_turns[index];
                        state.merge_shadow_turn(turn);
                        shadow_parts(&turn.assistant_content)
                    }
                    None => None,
                };
                let parts = match replayed {
                    Some(parts) => parts,
                    None => encode_parts(message, &mut state)?,
                };
                state.merge_tool_names_from_parts(&parts);
                parts
            }
        };

        let role = match message.role {
            Role::Assistant => "model",
            Role::User => "user",
        };
        contents.push(json!({ "role": role, "parts": parts }));
    }

    Ok(contents)
}

/// 按工具调用找对应的 shadow 轮次：优先精确匹配 id（不同 MCP 服务器下同名工具
/// 只靠名称会匹配到错误的轮次），消息不带 id 或 id 未命中时再按名称（含去掉
/// `server:` 前缀后的名称）匹配
fn find_matching_shadow_turn(
    message: &Message,
    shadow_turns: &[GeminiAssistantTurn],
) -> Option<usize> {
    let mut ids = HashSet::new();
    let mut names = HashSet::new();
    for (id, name) in tool_calls(message) {
        if !id.is_empty() {
            ids.insert(id);
        }
        if !name.is_empty() {
            names.insert(name);
            names.insert(normalize_tool_name(name));
        }
    }
    if ids.is_empty() && names.is_empty() {
        return None;
    }

    if !ids.is_empty() {
        let by_id = shadow_turns.iter().position(|turn| {
            turn.tool_calls
                .iter()
                .any(|call| call.id.as_deref().is_some_and(|id| ids.contains(id)))
        });
        if by_id.is_some() {
            return by_id;
        }
    }

    shadow_turns.iter().position(|turn| {
        turn.tool_calls.iter().any(|call| {
            names.contains(call.name.as_str()) || names.contains(normalize_tool_name(&call.name))
        })
    })
}

fn normalize_tool_name(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}

fn encode_parts(message: &Message, state: &mut ReplayState) -> Result<Vec<Value>, CodecError> {
    let mut parts = Vec::with_capacity(message.parts.len());
    for part in &message.parts {
        match part {
            Part::Text { text } => parts.push(json!({ "text": text })),
            Part::Image { source } => parts.push(inline_data(source, "image", "image/png")?),
            Part::Document { source, .. } => {
                parts.push(inline_data(source, "document", "application/pdf")?)
            }
            Part::ToolCall {
                id,
                name,
                arguments,
            } => {
                if message.role != Role::Assistant {
                    return Err(invalid(
                        "tool_use blocks are only valid in assistant messages",
                    ));
                }
                if !id.is_empty() && !name.is_empty() {
                    state.tool_name_by_id.insert(id.clone(), name.clone());
                }
                let args = serde_json::from_str::<Value>(arguments).unwrap_or_else(|_| json!({}));
                let mut function_call = json!({ "name": name, "args": args });
                // 合成的 id 只在代理与客户端之间使用，不发给 Gemini（Gemini 按调用顺序对齐）
                if !id.is_empty() && !is_synthesized_tool_call_id(id) {
                    function_call["id"] = json!(id);
                }
                // 多轮工具调用中 Gemini 要求每个 functionCall 带回原 thoughtSignature
                if let Some(signature) = state.thought_signature_by_id.get(id) {
                    function_call["thoughtSignature"] = json!(signature);
                }
                parts.push(json!({ "functionCall": function_call }));
            }
            Part::ToolResult {
                call_id, content, ..
            } => {
                let name = state
                    .tool_name_by_id
                    .get(call_id)
                    .cloned()
                    .or_else(|| {
                        // 兜底：同一条消息里的工具调用（非标准客户端）
                        tool_calls(message)
                            .find(|(id, _)| id == call_id)
                            .map(|(_, name)| name.to_string())
                    })
                    .ok_or_else(|| {
                        invalid(format!(
                            "Unable to resolve Gemini functionResponse.name for tool_use_id `{call_id}`"
                        ))
                    })?;
                let mut function_response = json!({
                    "name": name,
                    "response": tool_result_response(content)
                });
                if !call_id.is_empty() && !is_synthesized_tool_call_id(call_id) {
                    function_response["id"] = json!(call_id);
                }
                parts.push(json!({ "functionResponse": function_response }));
            }
            // Gemini 的思考由 shadow 回放，客户端的 thinking 块不转发
            Part::Reasoning { .. } | Part::RedactedReasoning { .. } => {}
        }
    }
    Ok(parts)
}

/// 图片 / 文档：Gemini 只接受内联 base64
fn inline_data(
    source: &MediaSource,
    kind: &str,
    default_mime_type: &str,
) -> Result<Value, CodecError> {
    match source {
        MediaSource::Base64 { media_type, data } => Ok(json!({
            "inlineData": {
                "mimeType": if media_type.is_empty() { default_mime_type } else { media_type },
                "data": data
            }
        })),
        MediaSource::Url { .. } => Err(unsupported_source(kind, "url")),
        MediaSource::Text { .. } => Err(unsupported_source(kind, "text")),
        MediaSource::File { .. } => Err(unsupported_source(kind, "file")),
    }
}

fn unsupported_source(kind: &str, source_type: &str) -> CodecError {
    invalid(format!(
        "Gemini Native only supports base64 {kind} sources, got `{source_type}`"
    ))
}

/// `functionResponse.response`：文本块以换行连接；没有文本时保留其余块
/// （按 Anthropic 块的形状）；没有内容时为空串
fn tool_result_response(content: &[Part]) -> Value {
    let texts: Vec<&str> = content
        .iter()
        .filter_map(|part| match part {
            Part::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    if !texts.is_empty() {
        return json!({ "content": texts.join("\n") });
    }
    if content.is_empty() {
        return json!({ "content": "" });
    }
    let blocks: Vec<Value> = content.iter().filter_map(anthropic_block).collect();
    json!({ "content": blocks })
}

fn anthropic_block(part: &Part) -> Option<Value> {
    let source = |source: &MediaSource| match source {
        MediaSource::Base64 { media_type, data } => {
            json!({ "type": "base64", "media_type": media_type, "data": data })
        }
        MediaSource::Url { url } => json!({ "type": "url", "url": url }),
        MediaSource::Text { data } => {
            json!({ "type": "text", "media_type": "text/plain", "data": data })
        }
        MediaSource::File { file_id, .. } => json!({ "type": "file", "file_id": file_id }),
    };
    match part {
        Part::Image { source: media } => Some(json!({ "type": "image", "source": source(media) })),
        Part::Document {
            source: media,
            title,
        } => {
            let mut block = json!({ "type": "document", "source": source(media) });
            if let Some(title) = title {
                block["title"] = json!(title);
            }
            Some(block)
        }
        _ => None,
    }
}
