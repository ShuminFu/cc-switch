//! [`ChatRequest`] → Anthropic Messages 请求体（Anthropic 作为上游，Codex 客户端）
//!
//! 字段写入顺序与旧转换器（`providers::transform_codex_anthropic`）一致，序列化结果逐字节相同。
//! 除结构转换外还负责 Anthropic 协议对历史的要求：
//! - 不再成对（助手 `tool_use` ↔ 紧随的用户 `tool_result`）的工具轮次整轮丢弃；
//! - 第一条消息必须是 user，空内容的消息与只有空白的结尾 assistant 预填会被拒绝；
//! - 工具结果续接的请求只有在配对的助手轮次带签名 thinking 时才能继续开启 thinking；
//! - thinking 预算 / adaptive 由 `reasoning.effort` 与模型能力决定，开启时不转发采样参数，
//!   强制工具选择与 thinking 冲突时关闭 thinking。

use std::collections::HashSet;

use serde_json::{json, Map, Value};

use super::thinking::{
    codex_effort_to_anthropic, effort_to_thinking_budget, reasoning_explicitly_disabled,
};
use crate::proxy::codec::{json_number, CodecError};
use crate::proxy::protocol::ir::{ChatRequest, MediaSource, Part, Role, ToolChoice};
use crate::proxy::protocol::WireFormat;
use crate::proxy::thinking_optimizer;

fn invalid(message: &str) -> CodecError {
    CodecError::Invalid {
        format: WireFormat::AnthropicMessages,
        message: message.to_string(),
    }
}

pub(super) fn encode_request(
    request: &ChatRequest,
    default_max_tokens: u64,
) -> Result<Value, CodecError> {
    let mut result = Map::new();
    let model = request.model.as_str();
    if !model.is_empty() {
        result.insert("model".into(), json!(model));
    }

    let system: Vec<&str> = request
        .system
        .iter()
        .filter_map(|part| match part {
            Part::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    if !system.is_empty() {
        result.insert("system".into(), json!(system.join("\n\n")));
    }

    let mut messages: Vec<Value> = request
        .messages
        .iter()
        .filter(|message| !message.parts.is_empty())
        .map(|message| {
            json!({
                "role": match message.role {
                    Role::User => "user",
                    Role::Assistant => "assistant",
                },
                "content": message.parts.iter().filter_map(encode_block).collect::<Vec<_>>()
            })
        })
        .collect();
    // 压缩 / 恢复的会话可能以 assistant 或工具调用开头，也可能全是被丢弃的 reasoning：
    // 先丢掉不完整的工具轮次（否则 400），再保证以 user 开头
    drop_incomplete_tool_turns(&mut messages);
    drop_empty_messages(&mut messages);
    ensure_leading_user_message(&mut messages);
    if messages.is_empty() {
        return Err(invalid("cannot convert Codex request: empty messages"));
    }
    trim_trailing_assistant_text(&mut messages);
    drop_empty_messages(&mut messages);
    if messages.is_empty() {
        return Err(invalid("cannot convert Codex request: empty messages"));
    }
    let thinking_history_is_valid = trailing_turn_supports_thinking(&messages);
    result.insert("messages".into(), Value::Array(messages));

    let effort = request
        .reasoning
        .as_ref()
        .and_then(|reasoning| reasoning.effort.as_deref());
    let adaptive_model = thinking_optimizer::uses_adaptive_thinking(model);
    let adaptive_by_default = thinking_optimizer::adaptive_thinking_is_default(model);
    let cannot_disable_thinking = thinking_optimizer::thinking_cannot_be_disabled(model);

    // max_output_tokens → max_tokens（必填）
    let max_tokens = request
        .max_tokens
        .filter(|value| *value > 0)
        .unwrap_or(default_max_tokens);
    let mut thinking_enabled = false;
    let mut thinking_budget = effort.and_then(effort_to_thinking_budget).unwrap_or(0);
    let explicitly_disabled = reasoning_explicitly_disabled(effort);
    let adaptive_should_think = adaptive_model
        && (adaptive_by_default || effort.and_then(codex_effort_to_anthropic).is_some());

    if !thinking_history_is_valid {
        if cannot_disable_thinking {
            return Err(invalid(
                "Anthropic model requires thinking, but the tool history has no signed thinking block to replay",
            ));
        }
        if adaptive_should_think {
            result.insert("thinking".into(), json!({ "type": "disabled" }));
        }
    } else if adaptive_should_think && (!explicitly_disabled || cannot_disable_thinking) {
        thinking_enabled = true;
        result.insert("thinking".into(), json!({ "type": "adaptive" }));
        if let Some(effort) = effort.and_then(codex_effort_to_anthropic) {
            result.insert("output_config".into(), json!({ "effort": effort }));
        } else if explicitly_disabled && cannot_disable_thinking {
            // 无法关闭 thinking 的模型：`low` 最接近 Codex 显式的 `none`
            result.insert("output_config".into(), json!({ "effort": "low" }));
        }
    } else if explicitly_disabled {
        result.insert("thinking".into(), json!({ "type": "disabled" }));
    } else if thinking_budget > 0 {
        thinking_enabled = true;
        // Anthropic 要求 max_tokens > budget_tokens 且预算 >= 1024：预算最多占 max_tokens
        // 的一半，给可见回答留余量；剩余预算不足 1024 时关闭 thinking、恢复普通采样
        thinking_budget = thinking_budget.min(max_tokens / 2);
        if thinking_budget < 1024 {
            thinking_enabled = false;
        }
    }
    result.insert("max_tokens".into(), json!(max_tokens));

    if thinking_enabled && !adaptive_model {
        result.insert(
            "thinking".into(),
            json!({ "type": "enabled", "budget_tokens": thinking_budget }),
        );
    }

    if !thinking_enabled {
        insert_sampling(&mut result, request);
    }
    if request.stream {
        result.insert("stream".into(), json!(true));
    }

    let tools: Vec<Value> = request
        .tools
        .iter()
        .filter(|tool| !tool.name.trim().is_empty())
        .map(|tool| {
            let input_schema = if tool
                .input_schema
                .as_object()
                .is_some_and(|object| !object.is_empty())
            {
                tool.input_schema.clone()
            } else {
                json!({ "type": "object", "properties": {} })
            };
            let mut encoded = json!({ "name": tool.name.trim(), "input_schema": input_schema });
            if let Some(description) = &tool.description {
                encoded["description"] = json!(description);
            }
            if let Some(strict) = tool.strict {
                encoded["strict"] = json!(strict);
            }
            encoded
        })
        .collect();
    let has_tools = !tools.is_empty();
    if has_tools {
        result.insert("tools".into(), Value::Array(tools));
    }

    // 只有保留了工具时才转发 tool_choice：没有工具的 tool_choice 会被 Anthropic 以
    // 不可重试的 400 拒绝
    if has_tools {
        if let Some(tool_choice) = &request.tool_choice {
            let mapped = encode_tool_choice(tool_choice);
            let forced = matches!(tool_choice, ToolChoice::Any | ToolChoice::Tool { .. });
            if thinking_enabled && forced {
                if cannot_disable_thinking {
                    return Err(invalid(
                        "Anthropic model requires adaptive thinking and cannot honor a forced tool_choice",
                    ));
                }
                // thinking 开启时 Anthropic 拒绝强制工具：保留调用方的工具约束，本次关闭 thinking
                result.insert("thinking".into(), json!({ "type": "disabled" }));
                result.remove("output_config");
                insert_sampling(&mut result, request);
            }
            result.insert("tool_choice".into(), mapped);
        }

        let parallel_disabled = request
            .extra
            .get("parallel_tool_calls")
            .and_then(Value::as_bool)
            == Some(false);
        if parallel_disabled {
            let tool_choice = result
                .entry("tool_choice".to_string())
                .or_insert_with(|| json!({ "type": "auto" }));
            if let Some(tool_choice) = tool_choice.as_object_mut() {
                tool_choice.insert("disable_parallel_tool_use".into(), json!(true));
            }
        }
    }

    Ok(Value::Object(result))
}

fn insert_sampling(result: &mut Map<String, Value>, request: &ChatRequest) {
    if let Some(temperature) = request.temperature {
        result.insert("temperature".into(), json_number(temperature));
    }
    if let Some(top_p) = request.top_p {
        result.insert("top_p".into(), json_number(top_p));
    }
}

fn encode_tool_choice(tool_choice: &ToolChoice) -> Value {
    match tool_choice {
        ToolChoice::Auto => json!({ "type": "auto" }),
        ToolChoice::None => json!({ "type": "none" }),
        ToolChoice::Any => json!({ "type": "any" }),
        ToolChoice::Tool { name } => json!({ "type": "tool", "name": name }),
    }
}

fn media_source(source: &MediaSource) -> Value {
    match source {
        MediaSource::Base64 { media_type, data } => {
            json!({ "type": "base64", "media_type": media_type, "data": data })
        }
        MediaSource::Url { url } => json!({ "type": "url", "url": url }),
        MediaSource::Text { data } => {
            json!({ "type": "text", "media_type": "text/plain", "data": data })
        }
        MediaSource::File { file_id, .. } => json!({ "type": "file", "file_id": file_id }),
    }
}

/// 单个内容块（工具结果的内容中只会出现文本、图片与文档）
fn encode_block(part: &Part) -> Option<Value> {
    Some(match part {
        Part::Text { text } => json!({ "type": "text", "text": text }),
        Part::Image { source } => json!({ "type": "image", "source": media_source(source) }),
        Part::Document { source, title } => {
            let mut block = json!({ "type": "document", "source": media_source(source) });
            if let Some(title) = title {
                block["title"] = json!(title);
            }
            block
        }
        Part::ToolCall {
            id,
            name,
            arguments,
        } => json!({
            "type": "tool_use",
            "id": id,
            "name": name,
            "input": serde_json::from_str::<Value>(arguments).unwrap_or_else(|_| json!({}))
        }),
        Part::ToolResult {
            call_id,
            content,
            is_error,
            plain,
        } => {
            let content = match content.as_slice() {
                [Part::Text { text }] if *plain => json!(text),
                _ => Value::Array(content.iter().filter_map(encode_block).collect()),
            };
            let mut block = json!({
                "type": "tool_result",
                "tool_use_id": call_id,
                "content": content
            });
            if *is_error {
                block["is_error"] = json!(true);
            }
            block
        }
        Part::Reasoning { text, signature } => {
            let mut block = json!({ "type": "thinking", "thinking": text });
            if let Some(signature) = signature {
                block["signature"] = json!(signature);
            }
            block
        }
        Part::RedactedReasoning { data } => json!({ "type": "redacted_thinking", "data": data }),
    })
}

/// 第一条消息必须是 user，否则补一条续接提示
fn ensure_leading_user_message(messages: &mut Vec<Value>) {
    let leads_with_user = messages
        .first()
        .and_then(|m| m.get("role"))
        .and_then(Value::as_str)
        == Some("user");
    if !messages.is_empty() && !leads_with_user {
        messages.insert(
            0,
            json!({
                "role": "user",
                "content": [{ "type": "text", "text": "(continuing the conversation)" }]
            }),
        );
    }
}

/// 去掉不再构成「助手 `tool_use` → 紧随的用户 `tool_result`」完整配对的工具轮次。
/// Anthropic 要求助手轮次的每个工具调用都在紧随的用户轮次中一并回答；整轮丢弃
/// 也避免改动带签名 thinking 的回复的一部分。
fn drop_incomplete_tool_turns(messages: &mut Vec<Value>) {
    let original = std::mem::take(messages);
    let mut sanitized = Vec::with_capacity(original.len());
    let mut index = 0;

    while index < original.len() {
        let message = &original[index];
        let is_assistant = message.get("role").and_then(Value::as_str) == Some("assistant");
        let tool_use_ids = if is_assistant {
            message_block_ids(message, "tool_use", "id")
        } else {
            Vec::new()
        };

        if !tool_use_ids.is_empty() {
            let paired_user = original
                .get(index + 1)
                .filter(|next| next.get("role").and_then(Value::as_str) == Some("user"));
            let tool_result_ids = paired_user
                .map(|user| message_block_ids(user, "tool_result", "tool_use_id"))
                .unwrap_or_default();
            let unique_tool_uses: HashSet<&str> = tool_use_ids.iter().copied().collect();
            let unique_tool_results: HashSet<&str> = tool_result_ids.iter().copied().collect();
            let complete = tool_use_ids.iter().all(|id| !id.is_empty())
                && tool_result_ids.iter().all(|id| !id.is_empty())
                && unique_tool_uses.len() == tool_use_ids.len()
                && unique_tool_results.len() == tool_result_ids.len()
                && unique_tool_uses == unique_tool_results;

            match paired_user {
                Some(user) if complete => {
                    sanitized.push(message.clone());
                    sanitized.push(user.clone());
                }
                Some(user) => {
                    let mut user = user.clone();
                    drop_tool_result_blocks(&mut user);
                    if message_has_content(&user) {
                        sanitized.push(user);
                    }
                }
                None => {}
            }

            index += if paired_user.is_some() { 2 } else { 1 };
            continue;
        }

        let mut message = message.clone();
        if message.get("role").and_then(Value::as_str) == Some("user") {
            // 没有作为完整工具配对一半的用户消息不能保留 tool_result 块
            drop_tool_result_blocks(&mut message);
        }
        if message_has_content(&message) {
            sanitized.push(message);
        }
        index += 1;
    }

    *messages = sanitized;
}

fn message_block_ids<'a>(message: &'a Value, block_type: &str, id_field: &str) -> Vec<&'a str> {
    message
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|block| block.get("type").and_then(Value::as_str) == Some(block_type))
        .map(|block| block.get(id_field).and_then(Value::as_str).unwrap_or(""))
        .collect()
}

fn drop_tool_result_blocks(message: &mut Value) {
    if let Some(content) = message.get_mut("content").and_then(Value::as_array_mut) {
        content.retain(|block| block.get("type").and_then(Value::as_str) != Some("tool_result"));
    }
}

fn message_has_content(message: &Value) -> bool {
    message
        .get("content")
        .and_then(Value::as_array)
        .map(|content| !content.is_empty())
        .unwrap_or(true)
}

/// 新的用户提问可以开启新的 thinking 轮次；工具结果续接只有在配对的助手轮次带有
/// Anthropic 要求回放的签名 thinking / redacted_thinking 块时才能保持 thinking
fn trailing_turn_supports_thinking(messages: &[Value]) -> bool {
    let Some(last) = messages.last() else {
        return false;
    };
    if last.get("role").and_then(Value::as_str) != Some("user") {
        return false;
    }
    let mut tool_result_ids = Vec::new();
    for block in last
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if block.get("type").and_then(Value::as_str) != Some("tool_result") {
            continue;
        }
        let Some(id) = block
            .get("tool_use_id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
        else {
            return false;
        };
        tool_result_ids.push(id);
    }
    if tool_result_ids.is_empty() {
        return true;
    }

    // 工具结果只回答紧邻的上一个助手轮次；再往前找可能拿到无关的签名 thinking
    let Some(paired_assistant) = messages.get(messages.len().saturating_sub(2)) else {
        return false;
    };
    if paired_assistant.get("role").and_then(Value::as_str) != Some("assistant") {
        return false;
    }
    let Some(blocks) = paired_assistant.get("content").and_then(Value::as_array) else {
        return false;
    };
    let has_signed_thinking = blocks.iter().any(|block| {
        matches!(
            block.get("type").and_then(Value::as_str),
            Some("thinking" | "redacted_thinking")
        )
    });
    if !has_signed_thinking {
        return false;
    }
    let paired_tool_use_ids: HashSet<&str> = blocks
        .iter()
        .filter(|block| block.get("type").and_then(Value::as_str) == Some("tool_use"))
        .filter_map(|block| block.get("id").and_then(Value::as_str))
        .collect();
    tool_result_ids
        .iter()
        .all(|id| paired_tool_use_ids.contains(id))
}

/// 去掉只有空白的结尾 assistant 预填，并裁掉真实预填末尾的空白（Anthropic 拒绝
/// 以空白结尾的预填）
fn trim_trailing_assistant_text(messages: &mut [Value]) {
    let Some(last) = messages.last_mut() else {
        return;
    };
    if last.get("role").and_then(Value::as_str) != Some("assistant") {
        return;
    }
    let Some(blocks) = last.get_mut("content").and_then(Value::as_array_mut) else {
        return;
    };
    let Some(block) = blocks.last_mut() else {
        return;
    };
    if block.get("type").and_then(Value::as_str) != Some("text") {
        return;
    }
    let Some(text) = block.get("text").and_then(Value::as_str) else {
        return;
    };
    let trimmed = text.trim_end();
    if trimmed.is_empty() {
        blocks.pop();
    } else if trimmed.len() != text.len() {
        block["text"] = json!(trimmed);
    }
}

/// Anthropic 拒绝空 content 的消息
fn drop_empty_messages(messages: &mut Vec<Value>) {
    messages.retain(|message| {
        message
            .get("content")
            .and_then(Value::as_array)
            .map(|content| !content.is_empty())
            .unwrap_or(true)
    });
}
