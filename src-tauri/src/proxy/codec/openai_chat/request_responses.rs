//! [`ChatRequest`] → OpenAI Chat Completions 请求体（Codex Responses 客户端）
//!
//! 与 Anthropic 客户端的 Chat 请求编码不同，这里沿用旧 `transform_codex_chat` 的规则，
//! 字段写入顺序一致、序列化结果逐字节相同：
//! - 所有 system 文本合并为开头的一条 system 消息（MiniMax 只允许首条为 system）；
//! - 每条 IR 消息对应一条 Chat 消息：助手消息带 `reasoning_content`，只有工具调用的
//!   助手消息内容为 null 且缺 reasoning 时补 `tool call` 占位（Kimi / DeepSeek 要求）；
//! - 工具定义、tool_choice 与 reasoning 参数按供应商声明的
//!   [`CodexChatReasoningConfig`] 生成；Chat 专用的透传字段原样转发；
//! - 没有工具时删除 tool_choice / parallel_tool_calls，流式请求声明 `include_usage`。

use serde_json::{json, Map, Value};

use super::request::{document_part, image_part};
use crate::provider::CodexChatReasoningConfig;
use crate::proxy::codec::json_number as number;
use crate::proxy::codec::openai_chat::support::{
    inject_openai_stream_include_usage, is_openai_o_series, supports_reasoning_effort,
};
use crate::proxy::protocol::ir::{ChatRequest, Message, Part, Role, ToolChoice};

/// Responses 请求中原样转发给 Chat 上游的字段
const PASSTHROUGH_FIELDS: &[&str] = &[
    "frequency_penalty",
    "logit_bias",
    "logprobs",
    "metadata",
    "n",
    "parallel_tool_calls",
    "presence_penalty",
    "response_format",
    "seed",
    "service_tier",
    "stop",
    "stream_options",
    "top_logprobs",
    "user",
];

pub(super) fn encode_request(
    request: &ChatRequest,
    reasoning_config: Option<&CodexChatReasoningConfig>,
) -> Value {
    let mut result = Map::new();
    let model = request.model.as_str();
    if !model.is_empty() {
        result.insert("model".into(), json!(model));
    }

    let mut messages = Vec::new();
    let system: Vec<&str> = request
        .system
        .iter()
        .filter_map(|part| match part {
            Part::Text { text } if !text.trim().is_empty() => Some(text.as_str()),
            _ => None,
        })
        .collect();
    if !system.is_empty() {
        messages.push(json!({ "role": "system", "content": system.join("\n\n") }));
    }
    messages.extend(request.messages.iter().map(encode_message));
    result.insert("messages".into(), Value::Array(messages));

    if let Some(max_tokens) = request.max_tokens {
        let key = if is_openai_o_series(model) {
            "max_completion_tokens"
        } else {
            "max_tokens"
        };
        result.insert(key.into(), json!(max_tokens));
    }
    for key in ["max_tokens", "max_completion_tokens"] {
        if let Some(value) = request.extra.get(key) {
            result.insert(key.into(), value.clone());
        }
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

    apply_reasoning_options(
        &mut result,
        request.extra.get("reasoning"),
        model,
        reasoning_config,
    );

    if !request.tools.is_empty() {
        let tools: Vec<Value> = request
            .tools
            .iter()
            .map(|tool| {
                let mut function = json!({
                    "name": tool.name,
                    "description": tool.description,
                    "parameters": tool.input_schema
                });
                if let Some(strict) = tool.strict {
                    function["strict"] = json!(strict);
                }
                json!({ "type": "function", "function": function })
            })
            .collect();
        result.insert("tools".into(), Value::Array(tools));
    }

    match &request.tool_choice {
        Some(tool_choice) => {
            result.insert("tool_choice".into(), encode_tool_choice(tool_choice));
        }
        None => {
            if let Some(raw) = request.extra.get("tool_choice") {
                result.insert("tool_choice".into(), raw.clone());
            }
        }
    }

    for key in PASSTHROUGH_FIELDS {
        if let Some(value) = request.extra.get(*key) {
            result.insert((*key).into(), value.clone());
        }
    }

    // 严格的 OpenAI 兼容上游（vLLM、企业网关）拒绝没有工具却带 tool_choice /
    // parallel_tool_calls 的请求
    if request.tools.is_empty() {
        result.remove("tool_choice");
        result.remove("parallel_tool_calls");
    }

    let mut result = Value::Object(result);
    // OpenAI 兼容上游流式下默认不返回 usage，必须显式声明 include_usage
    inject_openai_stream_include_usage(&mut result);
    result
}

fn encode_tool_choice(tool_choice: &ToolChoice) -> Value {
    match tool_choice {
        ToolChoice::Auto => json!("auto"),
        ToolChoice::None => json!("none"),
        ToolChoice::Any => json!("required"),
        ToolChoice::Tool { name } => json!({ "type": "function", "function": { "name": name } }),
    }
}

fn encode_message(message: &Message) -> Value {
    let mut reasoning: Vec<&str> = Vec::new();
    let mut content: Vec<Value> = Vec::new();
    let mut tool_calls: Vec<Value> = Vec::new();
    for part in &message.parts {
        match part {
            Part::Text { text } => content.push(json!({ "type": "text", "text": text })),
            Part::Image { source } => content.extend(image_part(source)),
            Part::Document { source, title } => {
                content.push(document_part(source, title.as_deref()))
            }
            Part::ToolCall {
                id,
                name,
                arguments,
            } => tool_calls.push(json!({
                "id": id,
                "type": "function",
                "function": { "name": name, "arguments": arguments }
            })),
            Part::ToolResult {
                call_id, content, ..
            } => {
                // 解码端把每个工具结果放在单独的消息里，内容已渲染为文本
                let text: Vec<&str> = content
                    .iter()
                    .filter_map(|part| match part {
                        Part::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect();
                return json!({
                    "role": "tool",
                    "tool_call_id": call_id,
                    "content": text.join("\n")
                });
            }
            Part::Reasoning { text, .. } if !text.is_empty() => reasoning.push(text),
            Part::Reasoning { .. } | Part::RedactedReasoning { .. } => {}
        }
    }

    let role = match message.role {
        Role::User => "user",
        Role::Assistant => "assistant",
    };
    let mut result = json!({
        "role": role,
        "content": match content.as_slice() {
            [] => Value::Null,
            [single] if single.get("type") == Some(&json!("text")) => single["text"].clone(),
            _ => Value::Array(content.clone()),
        }
    });
    if !tool_calls.is_empty() {
        result["tool_calls"] = Value::Array(tool_calls);
    }
    if message.role == Role::Assistant {
        let reasoning = reasoning.join("\n\n");
        if !reasoning.trim().is_empty() {
            result["reasoning_content"] = json!(reasoning.trim());
        } else if result.get("tool_calls").is_some() {
            // 思考型模型要求每条带 tool_calls 的助手消息都有非空 reasoning_content
            result["reasoning_content"] = json!("tool call");
        }
    }
    result
}

/// 按供应商声明的 thinking / effort 能力生成 reasoning 参数
///
/// 没有声明时：支持 `reasoning_effort` 的 OpenAI 模型原样转发 `reasoning.effort`。
fn apply_reasoning_options(
    result: &mut Map<String, Value>,
    reasoning: Option<&Value>,
    model: &str,
    config: Option<&CodexChatReasoningConfig>,
) {
    let effort = reasoning.and_then(|reasoning| reasoning.get("effort"));
    let Some(config) = config else {
        if supports_reasoning_effort(model) {
            if let Some(effort) = effort {
                result.insert("reasoning_effort".into(), effort.clone());
            }
        }
        return;
    };

    let supports_effort = config.supports_effort.unwrap_or(false);
    let supports_thinking = config.supports_thinking.unwrap_or(false) || supports_effort;
    let Some(enabled) = reasoning_requested(reasoning) else {
        return;
    };

    if supports_thinking {
        match config
            .thinking_param
            .as_deref()
            .unwrap_or("thinking")
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "thinking" => {
                result.insert(
                    "thinking".into(),
                    json!({ "type": if enabled { "enabled" } else { "disabled" } }),
                );
            }
            "enable_thinking" => {
                result.insert("enable_thinking".into(), json!(enabled));
            }
            "reasoning_split" => {
                result.insert("reasoning_split".into(), json!(enabled));
            }
            _ => {}
        }
    }

    let effort_param = config
        .effort_param
        .as_deref()
        .unwrap_or("reasoning_effort")
        .trim()
        .to_ascii_lowercase();

    if !enabled {
        // OpenRouter 的 reasoning.effort 支持显式 "none"（彻底关闭推理）：上游显式关闭时
        // 忠实转发关闭意图；顶层 reasoning_effort 的枚举不含 none，不发 effort
        if effort_param == "reasoning.effort" {
            result.insert("reasoning".into(), json!({ "effort": "none" }));
        }
        return;
    }
    if !supports_effort {
        return;
    }
    let Some(mapped) = effort
        .and_then(Value::as_str)
        .and_then(|effort| map_reasoning_effort(effort, config.effort_value_mode.as_deref()))
    else {
        return;
    };
    match effort_param.as_str() {
        "reasoning_effort" => {
            result.insert("reasoning_effort".into(), json!(mapped));
        }
        "reasoning.effort" => {
            result.insert("reasoning".into(), json!({ "effort": mapped }));
        }
        _ => {}
    }
}

/// 请求是否开启 reasoning：显式 effort 为 none / off / disabled 时为关闭；
/// 没有 effort 时，带非 null 的 `reasoning` 对象即开启；完全没有 reasoning 为 `None`
fn reasoning_requested(reasoning: Option<&Value>) -> Option<bool> {
    if let Some(effort) = reasoning
        .and_then(|reasoning| reasoning.get("effort"))
        .and_then(Value::as_str)
    {
        return Some(!matches!(
            effort.trim().to_ascii_lowercase().as_str(),
            "none" | "off" | "disabled"
        ));
    }
    reasoning.map(|value| !value.is_null())
}

/// effort 按供应商的取值模式映射；未知值不转发，避免被上游拒绝
fn map_reasoning_effort(effort: &str, mode: Option<&str>) -> Option<&'static str> {
    let effort = effort.trim().to_ascii_lowercase();
    if matches!(effort.as_str(), "none" | "off" | "disabled") {
        return None;
    }
    match mode.unwrap_or("passthrough") {
        "deepseek" => match effort.as_str() {
            "max" | "xhigh" => Some("max"),
            _ => Some("high"),
        },
        "low_high" => match effort.as_str() {
            "minimal" | "low" => Some("low"),
            _ => Some("high"),
        },
        // OpenRouter 的 effort 枚举没有 max：钳到最高的合法档 xhigh
        "openrouter" => match effort.as_str() {
            "max" | "xhigh" => Some("xhigh"),
            "high" => Some("high"),
            "medium" => Some("medium"),
            "low" => Some("low"),
            "minimal" => Some("minimal"),
            _ => None,
        },
        _ => match effort.as_str() {
            "minimal" => Some("minimal"),
            "low" => Some("low"),
            "medium" => Some("medium"),
            "high" => Some("high"),
            "xhigh" => Some("xhigh"),
            "max" => Some("max"),
            _ => None,
        },
    }
}
