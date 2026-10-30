//! Anthropic Messages 请求 → [`ChatRequest`]
//!
//! 解码保持宽松：Messages 请求由客户端发来，缺字段或类型不符时按旧转换器的
//! 做法取默认值或跳过该块，而不是拒绝整个请求。

use serde_json::{json, Value};

use crate::proxy::json_canonical::canonical_json_string;
use crate::proxy::protocol::ir::{
    ChatRequest, MediaSource, Message, Part, ReasoningConfig, Role, ToolChoice, ToolDef,
};

/// 已由 IR 字段建模的顶层字段；其余字段原样进入 [`ChatRequest::extra`]
const MODELED_FIELDS: [&str; 12] = [
    "model",
    "system",
    "messages",
    "max_tokens",
    "temperature",
    "top_p",
    "top_k",
    "stop_sequences",
    "stream",
    "tools",
    "tool_choice",
    "thinking",
];

pub(super) fn decode_request(body: &Value) -> ChatRequest {
    let mut request = ChatRequest {
        model: str_field(body, "model").unwrap_or_default().to_string(),
        system: decode_system(body.get("system")),
        max_tokens: body.get("max_tokens").and_then(Value::as_u64),
        temperature: body.get("temperature").and_then(Value::as_f64),
        top_p: body.get("top_p").and_then(Value::as_f64),
        top_k: body.get("top_k").and_then(Value::as_u64),
        stop: body
            .get("stop_sequences")
            .and_then(Value::as_array)
            .map(|stops| {
                stops
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default(),
        stream: body.get("stream").and_then(Value::as_bool).unwrap_or(false),
        tools: decode_tools(body.get("tools")),
        tool_choice: body.get("tool_choice").and_then(decode_tool_choice),
        reasoning: decode_reasoning(body),
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

    for message in body
        .get("messages")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let parts = decode_content(message.get("content"));
        match str_field(message, "role").unwrap_or("user") {
            "assistant" => request.messages.push(Message::new(Role::Assistant, parts)),
            // Messages API 不允许 system 角色出现在 messages 中；兼容旧客户端，
            // 把其中的文本并入 system 提示（与旧转换器合并 system 消息的结果一致）
            "system" => request.system.extend(
                parts
                    .into_iter()
                    .filter(|part| matches!(part, Part::Text { .. })),
            ),
            _ => request.messages.push(Message::new(Role::User, parts)),
        }
    }

    request
}

fn str_field<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

/// `system`：字符串，或带 `text` 的块数组（`cache_control` 等标注不进入 IR）
fn decode_system(system: Option<&Value>) -> Vec<Part> {
    match system {
        Some(Value::String(text)) => vec![Part::text(text.as_str())],
        Some(Value::Array(blocks)) => blocks
            .iter()
            .filter_map(|block| str_field(block, "text"))
            .map(Part::text)
            .collect(),
        _ => Vec::new(),
    }
}

/// 消息内容：字符串即单个文本块，数组逐块解码，其余形式视为空
fn decode_content(content: Option<&Value>) -> Vec<Part> {
    match content {
        Some(Value::String(text)) => vec![Part::text(text.as_str())],
        Some(Value::Array(blocks)) => blocks.iter().filter_map(decode_block).collect(),
        _ => Vec::new(),
    }
}

fn decode_block(block: &Value) -> Option<Part> {
    match str_field(block, "type").unwrap_or_default() {
        "text" => str_field(block, "text").map(Part::text),
        "image" => decode_image(block),
        "document" => Some(decode_document(block)),
        "tool_use" => Some(Part::ToolCall {
            id: str_field(block, "id").unwrap_or_default().to_string(),
            name: str_field(block, "name").unwrap_or_default().to_string(),
            arguments: canonical_json_string(block.get("input").unwrap_or(&json!({}))),
        }),
        "tool_result" => Some(Part::ToolResult {
            call_id: str_field(block, "tool_use_id")
                .unwrap_or_default()
                .to_string(),
            content: decode_tool_result_content(block.get("content")),
            is_error: block.get("is_error").and_then(Value::as_bool) == Some(true),
            plain: matches!(block.get("content"), Some(Value::String(_))),
        }),
        "thinking" => str_field(block, "thinking").map(|text| Part::Reasoning {
            text: text.to_string(),
            signature: str_field(block, "signature").map(str::to_string),
        }),
        "redacted_thinking" => Some(Part::RedactedReasoning {
            data: str_field(block, "data").unwrap_or_default().to_string(),
        }),
        _ => None,
    }
}

/// image 块：`url` 源与 `base64`（或缺省 type）源；其他源类型无法表示，跳过
fn decode_image(block: &Value) -> Option<Part> {
    let source = block.get("source")?;
    let source = match str_field(source, "type") {
        Some("url") => MediaSource::Url {
            url: str_field(source, "url").unwrap_or_default().to_string(),
        },
        Some("base64") | None => MediaSource::Base64 {
            media_type: str_field(source, "media_type")
                .unwrap_or_default()
                .to_string(),
            data: str_field(source, "data").unwrap_or_default().to_string(),
        },
        Some(other) => {
            log::warn!(
                "[Codec/Anthropic] Skipping image block with unsupported source type: {other}"
            );
            return None;
        }
    };
    Some(Part::Image { source })
}

/// document 块：`text` / `content` 源折叠为纯文本，`base64` / `url` 原样保留；
/// 读不出内容的来源（缺失或未知类型）记为空文本，由目标编解码占位
fn decode_document(block: &Value) -> Part {
    let title = str_field(block, "title")
        .or_else(|| str_field(block, "filename"))
        .filter(|title| !title.is_empty())
        .map(str::to_string);
    let source = block.get("source");
    let text = |data: String| MediaSource::Text { data };
    let source = match source.and_then(|s| str_field(s, "type")) {
        Some("text") => text(
            source
                .and_then(|s| str_field(s, "data"))
                .unwrap_or_default()
                .to_string(),
        ),
        Some("content") => text(match source.and_then(|s| s.get("content")) {
            Some(Value::String(content)) => content.clone(),
            Some(Value::Array(blocks)) => blocks
                .iter()
                .filter_map(|block| str_field(block, "text"))
                .collect::<Vec<_>>()
                .join("\n"),
            _ => String::new(),
        }),
        Some("base64") => MediaSource::Base64 {
            media_type: source
                .and_then(|s| str_field(s, "media_type"))
                .unwrap_or_default()
                .to_string(),
            data: source
                .and_then(|s| str_field(s, "data"))
                .unwrap_or_default()
                .to_string(),
        },
        Some("url") => MediaSource::Url {
            url: source
                .and_then(|s| str_field(s, "url"))
                .unwrap_or_default()
                .to_string(),
        },
        _ => text(String::new()),
    };
    Part::Document { source, title }
}

/// tool_result 的 content：字符串即文本；数组中的文本 / 图片 / 文档逐块解码，
/// 未知块以 canonical JSON 文本保留；其他非空值同样转为 JSON 文本
fn decode_tool_result_content(content: Option<&Value>) -> Vec<Part> {
    match content {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::String(text)) => vec![Part::text(text.as_str())],
        Some(Value::Array(blocks)) => blocks
            .iter()
            .filter_map(|block| match str_field(block, "type") {
                Some("text") => str_field(block, "text").map(Part::text),
                Some("image") => decode_image(block),
                Some("document") => Some(decode_document(block)),
                _ => Some(Part::text(canonical_json_string(block))),
            })
            .collect(),
        Some(other) => vec![Part::text(canonical_json_string(other))],
    }
}

/// 工具定义；`BatchTool` 是 Claude Code 的伪工具，不转发
fn decode_tools(tools: Option<&Value>) -> Vec<ToolDef> {
    tools
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|tool| str_field(tool, "type") != Some("BatchTool"))
        .map(|tool| ToolDef {
            name: str_field(tool, "name").unwrap_or_default().to_string(),
            description: str_field(tool, "description").map(str::to_string),
            input_schema: tool
                .get("input_schema")
                .cloned()
                .unwrap_or_else(|| json!({})),
            strict: None,
        })
        .collect()
}

/// `tool_choice`：字符串或 `{"type": ...}` 对象；无法识别的取值不转发
fn decode_tool_choice(tool_choice: &Value) -> Option<ToolChoice> {
    let (kind, name) = match tool_choice {
        Value::String(kind) => (kind.as_str(), None),
        Value::Object(_) => (
            str_field(tool_choice, "type")?,
            str_field(tool_choice, "name"),
        ),
        _ => return None,
    };
    match kind {
        "auto" => Some(ToolChoice::Auto),
        "none" => Some(ToolChoice::None),
        "any" | "required" => Some(ToolChoice::Any),
        "tool" if tool_choice.is_object() => Some(ToolChoice::Tool {
            name: name.unwrap_or_default().to_string(),
        }),
        other => {
            log::debug!("[Codec/Anthropic] Dropping unknown tool_choice `{other}`");
            None
        }
    }
}

/// 思考配置：显式 `output_config.effort` 与 `thinking`（enabled 带预算 / adaptive）；
/// `thinking` 缺失或为 disabled 且无显式 effort 时不开启
fn decode_reasoning(body: &Value) -> Option<ReasoningConfig> {
    let effort = body
        .pointer("/output_config/effort")
        .and_then(Value::as_str)
        .map(str::to_string);
    let thinking = body.get("thinking");
    let mut config = ReasoningConfig {
        effort,
        ..ReasoningConfig::default()
    };
    let enabled = match thinking.and_then(|t| str_field(t, "type")) {
        Some("enabled") => {
            config.budget_tokens = thinking
                .and_then(|t| t.get("budget_tokens"))
                .and_then(Value::as_u64);
            true
        }
        Some("adaptive") => {
            config.adaptive = true;
            true
        }
        _ => false,
    };
    (enabled || config.effort.is_some()).then_some(config)
}
