//! [`ChatRequest`] → OpenAI Chat Completions 请求体
//!
//! 字段写入顺序与旧转换器（`providers::transform`）一致，序列化结果逐字节相同。

use serde_json::{json, Map, Value};

use crate::proxy::codec::json_number as number;
use crate::proxy::protocol::ir::{
    ChatRequest, MediaSource, Message, Part, ReasoningConfig, Role, ToolChoice,
};
use crate::proxy::providers::transform::{
    clean_schema, is_openai_o_series, strip_leading_anthropic_billing_header,
    supports_reasoning_effort,
};

pub(super) fn encode_request(request: &ChatRequest, preserve_reasoning_content: bool) -> Value {
    let mut result = Map::new();

    // NOTE: 模型映射由上游统一处理（proxy::model_mapper），编解码只做结构转换。
    if !request.model.is_empty() {
        result.insert("model".into(), json!(request.model));
    }

    let mut messages = encode_system(&request.system);
    for message in &request.messages {
        messages.extend(encode_message(message, preserve_reasoning_content));
    }
    result.insert("messages".into(), Value::Array(messages));

    // o-series 模型需要 max_completion_tokens
    if let Some(max_tokens) = request.max_tokens {
        let key = if is_openai_o_series(&request.model) {
            "max_completion_tokens"
        } else {
            "max_tokens"
        };
        result.insert(key.into(), json!(max_tokens));
    }
    if let Some(temperature) = request.temperature {
        result.insert("temperature".into(), number(temperature));
    }
    if let Some(top_p) = request.top_p {
        result.insert("top_p".into(), number(top_p));
    }
    if !request.stop.is_empty() {
        result.insert("stop".into(), json!(request.stop));
    }
    if request.stream {
        result.insert("stream".into(), json!(true));
    }

    if supports_reasoning_effort(&request.model) {
        if let Some(effort) = request.reasoning.as_ref().and_then(reasoning_effort) {
            result.insert("reasoning_effort".into(), json!(effort));
        }
    }

    if !request.tools.is_empty() {
        let tools: Vec<Value> = request
            .tools
            .iter()
            .map(|tool| {
                json!({
                    "type": "function",
                    "function": {
                        "name": tool.name,
                        "description": tool.description,
                        "parameters": clean_schema(tool.input_schema.clone())
                    }
                })
            })
            .collect();
        result.insert("tools".into(), Value::Array(tools));
    }

    if let Some(tool_choice) = &request.tool_choice {
        result.insert("tool_choice".into(), encode_tool_choice(tool_choice));
    }

    Value::Object(result)
}

/// system 提示放在最前面，多个文本块合并为一条 system 消息
///
/// 只去掉开头的 Claude Code 计费标注行：其中轮换的 `cch=` 值会破坏上游前缀缓存（#2350）。
fn encode_system(system: &[Part]) -> Vec<Value> {
    let texts: Vec<&str> = system
        .iter()
        .filter_map(|part| match part {
            Part::Text { text } => Some(strip_leading_anthropic_billing_header(text)),
            _ => None,
        })
        .filter(|text| !text.is_empty())
        .collect();
    if texts.is_empty() {
        Vec::new()
    } else {
        vec![json!({"role": "system", "content": texts.join("\n")})]
    }
}

/// 把 effort / 预算映射为 `reasoning_effort`
///
/// 显式 effort 优先：`low`/`medium`/`high` 原样，`max` → `xhigh`，未知值不注入；
/// 否则 adaptive → `xhigh`，预算 <4 000 → `low`、<16 000 → `medium`、其余（含无预算）→ `high`。
fn reasoning_effort(config: &ReasoningConfig) -> Option<&'static str> {
    if let Some(effort) = &config.effort {
        return match effort.as_str() {
            "low" => Some("low"),
            "medium" => Some("medium"),
            "high" => Some("high"),
            "max" => Some("xhigh"),
            _ => None,
        };
    }
    if config.adaptive {
        return Some("xhigh");
    }
    Some(match config.budget_tokens {
        Some(budget) if budget < 4_000 => "low",
        Some(budget) if budget < 16_000 => "medium",
        _ => "high",
    })
}

/// Chat 的 tool_choice：没有 `any`，对应 `required`；指定工具用 function 选择器
fn encode_tool_choice(tool_choice: &ToolChoice) -> Value {
    match tool_choice {
        ToolChoice::Auto => json!("auto"),
        ToolChoice::None => json!("none"),
        ToolChoice::Any => json!("required"),
        ToolChoice::Tool { name } => json!({
            "type": "function",
            "function": { "name": name }
        }),
    }
}

/// 一条 IR 消息可能产生多条 Chat 消息：每个工具结果一条 `tool` 消息，
/// 其后是承载文本 / 图片 / 工具调用的主消息
fn encode_message(message: &Message, preserve_reasoning_content: bool) -> Vec<Value> {
    let role = match message.role {
        Role::User => "user",
        Role::Assistant => "assistant",
    };
    let mut result = Vec::new();
    let mut content_parts = Vec::new();
    let mut tool_calls = Vec::new();
    // 仅在兼容 Moonshot/Kimi/DeepSeek thinking tool-call 路径时生成 reasoning_content
    let mut reasoning_parts: Vec<&str> = Vec::new();
    // tool 消息只能承载文本：工具结果中的图片收集到这里，以 user 消息的 image_url 发送
    let mut tool_image_parts = Vec::new();

    for part in &message.parts {
        match part {
            Part::Text { text } => content_parts.push(json!({"type": "text", "text": text})),
            Part::Image { source } => content_parts.extend(image_part(source)),
            Part::Document { source, title } => {
                content_parts.push(document_part(source, title.as_deref()))
            }
            Part::ToolCall {
                id,
                name,
                arguments,
            } => tool_calls.push(json!({
                "id": id,
                "type": "function",
                "function": {
                    "name": name,
                    "arguments": arguments
                }
            })),
            Part::ToolResult {
                call_id,
                content,
                is_error,
            } => {
                let mut text = tool_result_text(content, &mut tool_image_parts);
                if *is_error {
                    text.insert_str(0, "[ERROR] ");
                }
                result.push(json!({
                    "role": "tool",
                    "tool_call_id": call_id,
                    "content": text
                }));
            }
            Part::Reasoning { text, .. } => {
                if !text.is_empty() {
                    reasoning_parts.push(text);
                }
            }
            // 历史 thinking 被加密为 redacted_thinking 时，MiMo/DeepSeek 仍要求带工具调用的
            // assistant 消息有非空 reasoning_content，以占位文本代替
            Part::RedactedReasoning { .. } if preserve_reasoning_content => {
                reasoning_parts.push("[redacted thinking]")
            }
            Part::RedactedReasoning { .. } => {}
        }
    }

    if !tool_image_parts.is_empty() {
        if message.role == Role::User {
            // 合并进本条 user 消息，避免产生连续的 user 消息
            tool_image_parts.append(&mut content_parts);
            content_parts = tool_image_parts;
        } else {
            result.push(json!({"role": "user", "content": tool_image_parts}));
        }
    }

    if !content_parts.is_empty() || !tool_calls.is_empty() {
        let mut msg = json!({"role": role});
        msg["content"] = match content_parts.as_slice() {
            [] => Value::Null,
            // 单个文本块简化为纯字符串
            [single] if single.get("text").is_some() => single["text"].clone(),
            _ => Value::Array(content_parts),
        };
        let has_tool_calls = !tool_calls.is_empty();
        if has_tool_calls {
            msg["tool_calls"] = Value::Array(tool_calls);
        }
        if preserve_reasoning_content && message.role == Role::Assistant && has_tool_calls {
            let reasoning_content = if reasoning_parts.is_empty() {
                "tool call".to_string()
            } else {
                reasoning_parts.join("\n")
            };
            msg["reasoning_content"] = json!(reasoning_content);
        }
        result.push(msg);
    }

    result
}

/// 图片 → `image_url` part；URL 原样，base64 拼 data URL（缺省 image/png）；空数据跳过
fn image_part(source: &MediaSource) -> Option<Value> {
    match source {
        MediaSource::Url { url } => {
            if url.is_empty() {
                log::warn!("[Codec/OpenAiChat] Skipping image block with empty url source");
                return None;
            }
            Some(json!({"type": "image_url", "image_url": {"url": url}}))
        }
        MediaSource::Base64 { media_type, data } => {
            if data.is_empty() {
                log::warn!("[Codec/OpenAiChat] Skipping image block with empty base64 data");
                return None;
            }
            let media_type = if media_type.is_empty() {
                "image/png"
            } else {
                media_type
            };
            Some(json!({
                "type": "image_url",
                "image_url": {"url": format!("data:{media_type};base64,{data}")}
            }))
        }
        MediaSource::Text { .. } => {
            log::warn!("[Codec/OpenAiChat] Skipping image block with a text source");
            None
        }
    }
}

/// 文档 → Chat content part（总是产生非空内容）
///
/// - 文本源转为 text part（有标题时加 `[document: 标题]` 前缀）；
/// - base64 PDF 转为 `file` part；
/// - 其他情况（URL、非 PDF、空内容）插入占位文本，避免文档被静默丢弃。
fn document_part(source: &MediaSource, title: Option<&str>) -> Value {
    match source {
        MediaSource::Text { data } if !data.is_empty() => {
            let text = match title {
                Some(title) => format!("[document: {title}]\n{data}"),
                None => data.clone(),
            };
            return json!({"type": "text", "text": text});
        }
        MediaSource::Base64 { media_type, data } if !data.is_empty() => {
            let media_type = if media_type.is_empty() {
                "application/pdf"
            } else {
                media_type
            };
            if media_type == "application/pdf" {
                return json!({
                    "type": "file",
                    "file": {
                        "filename": title.unwrap_or("document.pdf"),
                        "file_data": format!("data:{media_type};base64,{data}")
                    }
                });
            }
        }
        _ => {}
    }

    let url = match source {
        MediaSource::Url { url } if !url.is_empty() => Some(url.as_str()),
        _ => None,
    };
    log::warn!("[Codec/OpenAiChat] Unsupported document block, inserting placeholder");
    let placeholder = match (title, url) {
        (Some(title), Some(url)) => format!("[document omitted: {title} ({url})]"),
        (Some(title), None) => format!("[document omitted: {title}]"),
        (None, Some(url)) => format!("[document omitted: {url}]"),
        (None, None) => "[document omitted]".to_string(),
    };
    json!({"type": "text", "text": placeholder})
}

/// 工具结果 → tool 消息文本：文本以换行拼接，图片替换为 `[image attached]` 并把
/// image_url part 追加到 `image_parts`，文档替换为占位符
fn tool_result_text(content: &[Part], image_parts: &mut Vec<Value>) -> String {
    let mut texts = Vec::with_capacity(content.len());
    for part in content {
        match part {
            Part::Text { text } => texts.push(text.clone()),
            Part::Image { source } => {
                if let Some(image) = image_part(source) {
                    image_parts.push(image);
                    texts.push("[image attached]".to_string());
                }
            }
            Part::Document { .. } => texts.push("[document omitted]".to_string()),
            // Anthropic 工具结果里不会出现的块
            Part::ToolCall { .. }
            | Part::ToolResult { .. }
            | Part::Reasoning { .. }
            | Part::RedactedReasoning { .. } => {}
        }
    }
    texts.join("\n")
}
