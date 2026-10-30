//! [`ChatRequest`] → OpenAI Responses 请求体（Responses 作为上游）
//!
//! 字段写入顺序与旧转换器（`providers::transform_responses`）一致，序列化结果逐字节相同：
//! - system → `instructions`（去掉开头的 Claude Code 计费标注行，非空块以空行拼接）；
//! - 工具调用 / 工具结果从消息中「提升」为顶层 `function_call` / `function_call_output`；
//! - 由本桥接签发的 reasoning 信封还原为原 `reasoning` 条目；只有 reasoning、没有后续
//!   message / function_call 的助手轮次不回放（上游会拒绝缺少后继条目的 reasoning）。

use serde_json::{json, Map, Value};

use super::{ResponsesUpstreamOptions, TOOL_RESULT_ERROR_MARKER};
use crate::proxy::codec::json_number as number;
use crate::proxy::codec::openai_chat::request::reasoning_effort;
use crate::proxy::json_canonical::canonical_json_string;
use crate::proxy::protocol::ir::{ChatRequest, MediaSource, Message, Part, Role, ToolChoice};
use crate::proxy::providers::reasoning_bridge::decode_openai_reasoning_item;
use crate::proxy::providers::transform::{
    clean_schema, strip_leading_anthropic_billing_header, supports_reasoning_effort,
};

/// Codex OAuth 后端要求的 include 项（无服务端状态时保留多轮 reasoning）
const REASONING_MARKER: &str = "reasoning.encrypted_content";

pub(super) fn encode_request(request: &ChatRequest, options: &ResponsesUpstreamOptions) -> Value {
    let mut result = Map::new();

    // NOTE: 模型映射由上游统一处理（proxy::model_mapper），编解码只做结构转换。
    if !request.model.is_empty() {
        result.insert("model".into(), json!(request.model));
    }

    let instructions = request
        .system
        .iter()
        .filter_map(|part| match part {
            Part::Text { text } => Some(strip_leading_anthropic_billing_header(text)),
            _ => None,
        })
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");
    if !instructions.is_empty() {
        result.insert("instructions".into(), json!(instructions));
    }

    let mut input = Vec::new();
    for message in &request.messages {
        encode_message(message, &mut input);
    }
    result.insert("input".into(), Value::Array(input));

    // Responses API 对所有模型都使用 max_output_tokens
    if let Some(max_tokens) = request.max_tokens {
        result.insert("max_output_tokens".into(), json!(max_tokens));
    }
    if let Some(temperature) = request.temperature {
        result.insert("temperature".into(), number(temperature));
    }
    if let Some(top_p) = request.top_p {
        result.insert("top_p".into(), number(top_p));
    }
    if request.stream {
        result.insert("stream".into(), json!(true));
    }

    if supports_reasoning_effort(&request.model) {
        if let Some(effort) = request.reasoning.as_ref().and_then(reasoning_effort) {
            result.insert("reasoning".into(), json!({ "effort": effort }));
        }
    }

    // stop_sequences 不转发（Responses API 不支持）

    if !request.tools.is_empty() {
        let tools: Vec<Value> = request
            .tools
            .iter()
            .map(|tool| {
                json!({
                    "type": "function",
                    "name": tool.name,
                    "description": tool.description,
                    "parameters": clean_schema(tool.input_schema.clone())
                })
            })
            .collect();
        result.insert("tools".into(), Value::Array(tools));
    }

    if let Some(tool_choice) = &request.tool_choice {
        result.insert("tool_choice".into(), encode_tool_choice(tool_choice));
    }

    if let Some(key) = &options.cache_key {
        result.insert("prompt_cache_key".into(), json!(key));
    }

    if options.codex_oauth {
        apply_codex_oauth_constraints(&mut result, request, options.codex_fast_mode);
    }

    Value::Object(result)
}

/// Codex OAuth（ChatGPT Plus/Pro 反代）协议约束，依据 codex-rs 的 `ResponsesApiRequest`：
/// - `store` 必须为 false，`include` 必须含 `reasoning.encrypted_content`；
/// - 删除该后端不接受的 `max_output_tokens` / `temperature` / `top_p`；
/// - 必填的 `instructions` / `tools` / `parallel_tool_calls` 缺失时补默认值；
/// - `stream` 永远为 true（SSE 解析层只处理流式响应）；FAST 模式写入 `service_tier`。
fn apply_codex_oauth_constraints(
    result: &mut Map<String, Value>,
    request: &ChatRequest,
    fast_mode: bool,
) {
    result.insert("store".into(), json!(false));
    if fast_mode {
        result.insert("service_tier".into(), json!("priority"));
    }

    let mut includes: Vec<Value> = request
        .extra
        .get("include")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if !includes
        .iter()
        .any(|value| value.as_str() == Some(REASONING_MARKER))
    {
        includes.push(json!(REASONING_MARKER));
    }
    result.insert("include".into(), Value::Array(includes));

    result.remove("max_output_tokens");
    result.remove("temperature");
    result.remove("top_p");
    result
        .entry("instructions".to_string())
        .or_insert(json!(""));
    result.entry("tools".to_string()).or_insert(json!([]));
    result
        .entry("parallel_tool_calls".to_string())
        .or_insert(json!(false));
    result.insert("stream".into(), json!(true));
}

/// Responses 的 tool_choice：`any` 对应 `required`，指定工具用扁平的 function 选择器
fn encode_tool_choice(tool_choice: &ToolChoice) -> Value {
    match tool_choice {
        ToolChoice::Auto => json!("auto"),
        ToolChoice::None => json!("none"),
        ToolChoice::Any => json!("required"),
        ToolChoice::Tool { name } => json!({ "type": "function", "name": name }),
    }
}

/// 一条 IR 消息展开为若干 Responses input 条目
fn encode_message(message: &Message, input: &mut Vec<Value>) {
    let (role, text_type) = match message.role {
        Role::User => ("user", "input_text"),
        Role::Assistant => ("assistant", "output_text"),
    };
    let start = input.len();
    let mut content: Vec<Value> = Vec::new();
    let flush = |content: &mut Vec<Value>, input: &mut Vec<Value>| {
        if !content.is_empty() {
            input.push(json!({ "role": role, "content": std::mem::take(content) }));
        }
    };

    for part in &message.parts {
        match part {
            // Responses 不接受 input[].content[] 上的 Anthropic cache_control
            Part::Text { text } => content.push(json!({ "type": text_type, "text": text })),
            Part::Image { source } => match image_part(source) {
                Some(image) => content.push(image),
                None => log::warn!("[Responses] Unsupported or invalid Anthropic image block"),
            },
            Part::Document { source, title } => match document_part(source, title.as_deref()) {
                Some(file) => content.push(file),
                None => log::warn!("[Responses] Unsupported or invalid Anthropic document block"),
            },
            Part::ToolCall {
                id,
                name,
                arguments,
            } => {
                flush(&mut content, input);
                input.push(json!({
                    "type": "function_call",
                    "call_id": id,
                    "name": name,
                    "arguments": arguments
                }));
            }
            Part::ToolResult {
                call_id,
                content: result,
                is_error,
                plain,
            } => {
                flush(&mut content, input);
                input.push(json!({
                    "type": "function_call_output",
                    "call_id": call_id,
                    "output": tool_output(result, *is_error, *plain)
                }));
            }
            Part::Reasoning {
                signature: Some(envelope),
                ..
            }
            | Part::RedactedReasoning { data: envelope } => {
                // 只有本桥接签发的信封能还原；其他来源的 thinking 丢弃
                if let Some(item) = decode_openai_reasoning_item(envelope) {
                    flush(&mut content, input);
                    input.push(item);
                }
            }
            Part::Reasoning {
                signature: None, ..
            } => {}
        }
    }
    flush(&mut content, input);

    // 回放的 reasoning 只有在同一助手轮次里后面还有 message / function_call 时才合法，
    // 否则下一次请求会报 "reasoning item ... without its required following item"
    if message.role == Role::Assistant {
        let mut has_follower = false;
        for index in (start..input.len()).rev() {
            let item_type = input[index].get("type").and_then(Value::as_str);
            let is_assistant_message =
                input[index].get("role").and_then(Value::as_str) == Some("assistant");
            if item_type == Some("reasoning") {
                if !has_follower {
                    input.remove(index);
                }
            } else if item_type == Some("function_call") || is_assistant_message {
                has_follower = true;
            }
        }
    }
}

fn is_http_url(url: &str) -> bool {
    url.starts_with("http://") || url.starts_with("https://")
}

/// 图片 → `input_image`：只接受 http(s) URL 与非空 base64（缺省 image/png）
fn image_part(source: &MediaSource) -> Option<Value> {
    match source {
        MediaSource::Url { url } if is_http_url(url) => {
            Some(json!({ "type": "input_image", "image_url": url }))
        }
        MediaSource::Base64 { media_type, data } if !data.is_empty() => {
            let media_type = if media_type.is_empty() {
                "image/png"
            } else {
                media_type
            };
            Some(json!({
                "type": "input_image",
                "image_url": format!("data:{media_type};base64,{data}")
            }))
        }
        _ => None,
    }
}

/// 文档 → `input_file`：只接受 http(s) URL 与非空 base64（缺省 application/pdf）
fn document_part(source: &MediaSource, title: Option<&str>) -> Option<Value> {
    let filename = title.unwrap_or("document.pdf");
    match source {
        MediaSource::Url { url } if is_http_url(url) => {
            Some(json!({ "type": "input_file", "file_url": url, "filename": filename }))
        }
        MediaSource::Base64 { media_type, data } if !data.is_empty() => {
            let media_type = if media_type.is_empty() {
                "application/pdf"
            } else {
                media_type
            };
            Some(json!({
                "type": "input_file",
                "file_data": format!("data:{media_type};base64,{data}"),
                "filename": filename
            }))
        }
        _ => None,
    }
}

/// 无法转换的块按原 Anthropic 形状序列化为 canonical JSON 文本
fn media_block_json(kind: &str, source: &MediaSource, title: Option<&str>) -> String {
    let source = match source {
        MediaSource::Url { url } => json!({ "type": "url", "url": url }),
        MediaSource::Base64 { media_type, data } => {
            json!({ "type": "base64", "media_type": media_type, "data": data })
        }
        MediaSource::Text { data } => json!({ "type": "text", "data": data }),
        MediaSource::File { file_id, .. } => json!({ "type": "file", "file_id": file_id }),
    };
    let mut block = json!({ "type": kind, "source": source });
    if let Some(title) = title {
        block["title"] = json!(title);
    }
    canonical_json_string(&block)
}

/// 工具结果 → `function_call_output.output`
///
/// 非错误的字符串结果原样作为字符串；其余情况为 content part 数组，错误结果以
/// [`TOOL_RESULT_ERROR_MARKER`] 开头（Responses 没有 `is_error`）。
fn tool_output(content: &[Part], is_error: bool, plain: bool) -> Value {
    if plain && !is_error {
        if let [Part::Text { text }] = content {
            return json!(text);
        }
    }

    let mut output = Vec::new();
    if is_error {
        output.push(json!({ "type": "input_text", "text": TOOL_RESULT_ERROR_MARKER }));
    }
    for part in content {
        match part {
            Part::Text { text } => output.push(json!({ "type": "input_text", "text": text })),
            Part::Image { source } => output.push(image_part(source).unwrap_or_else(
                || json!({ "type": "input_text", "text": media_block_json("image", source, None) }),
            )),
            Part::Document { source, title } => {
                output.push(document_part(source, title.as_deref()).unwrap_or_else(|| {
                    json!({
                        "type": "input_text",
                        "text": media_block_json("document", source, title.as_deref())
                    })
                }))
            }
            // Anthropic 工具结果里不会出现的块
            Part::ToolCall { .. }
            | Part::ToolResult { .. }
            | Part::Reasoning { .. }
            | Part::RedactedReasoning { .. } => {}
        }
    }
    Value::Array(output)
}
