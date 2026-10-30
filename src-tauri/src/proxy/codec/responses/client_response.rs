//! [`ChatResponse`] → OpenAI Responses 响应体（Responses 作为客户端，Codex）
//!
//! 条目布局与对应的旧转换器一致（见 [`ResponsesFlavor`]）：
//! - Anthropic 上游：相邻文本块合并为一个 `message` 条目，thinking 只有带签名时才成为
//!   条目，条目 id 带其在 `output` 中的位置；
//! - Chat 上游：一个 reasoning 条目、一个 `message` 条目，之后是工具调用条目（附带 reasoning）。

use serde_json::{json, Value};

use super::client_stream::{response_id, status, usage_json};
use super::reasoning::responses_reasoning_item_from_anthropic_block;
use super::tools::{
    response_tool_call_item_from_chat_name, response_tool_call_item_id_from_chat_name,
    CodexToolContext,
};
use super::{sanitize_anthropic_tool_use_input_json, ResponsesFlavor};
use crate::proxy::json_canonical::canonicalize_tool_arguments_str;
use crate::proxy::protocol::ir::{ChatResponse, Part};

pub(super) fn encode_response(
    response: &ChatResponse,
    flavor: ResponsesFlavor,
    tool_context: &CodexToolContext,
) -> Value {
    let response_id = response_id(&response.id);
    let output = match flavor {
        ResponsesFlavor::AnthropicUpstream => {
            anthropic_output(response, &response_id, tool_context)
        }
        ResponsesFlavor::ChatUpstream => chat_output(response, &response_id, tool_context),
    };
    let (status, incomplete_reason) = status(response.stop_reason);
    let mut result = json!({
        "id": response_id,
        "object": "response",
        "created_at": response.created.unwrap_or(0),
        "status": status,
        "model": response.model,
        "output": output,
        "usage": usage_json(response.usage, flavor)
    });
    if let Some(reason) = incomplete_reason {
        result["incomplete_details"] = json!({ "reason": reason });
    }
    result
}

fn anthropic_output(
    response: &ChatResponse,
    response_id: &str,
    tool_context: &CodexToolContext,
) -> Vec<Value> {
    let mut output: Vec<Value> = Vec::new();
    let mut text_parts: Vec<Value> = Vec::new();
    let flush_text = |output: &mut Vec<Value>, text_parts: &mut Vec<Value>| {
        if !text_parts.is_empty() {
            let index = output.len();
            output.push(json!({
                "id": format!("{response_id}_msg_{index}"),
                "type": "message",
                "status": "completed",
                "role": "assistant",
                "content": std::mem::take(text_parts)
            }));
        }
    };

    for part in &response.parts {
        match part {
            Part::Text { text } => text_parts.push(json!({
                "type": "output_text",
                "text": text,
                "annotations": []
            })),
            Part::ToolCall {
                id,
                name,
                arguments,
            } => {
                flush_text(&mut output, &mut text_parts);
                let arguments = if name == "Read" {
                    sanitize_anthropic_tool_use_input_json(name, arguments)
                } else {
                    arguments.clone()
                };
                let item_id = response_tool_call_item_id_from_chat_name(id, name, tool_context);
                output.push(response_tool_call_item_from_chat_name(
                    &item_id,
                    "completed",
                    id,
                    name,
                    &canonicalize_tool_arguments_str(&arguments),
                    None,
                    tool_context,
                ));
            }
            Part::Reasoning { .. } | Part::RedactedReasoning { .. } => {
                flush_text(&mut output, &mut text_parts);
                let block = match part {
                    Part::Reasoning { text, signature } => {
                        let mut block = json!({ "type": "thinking", "thinking": text });
                        if let Some(signature) = signature {
                            block["signature"] = json!(signature);
                        }
                        block
                    }
                    Part::RedactedReasoning { data } => {
                        json!({ "type": "redacted_thinking", "data": data })
                    }
                    _ => unreachable!(),
                };
                let item_id = format!("rs_{response_id}_{}", output.len());
                if let Some(item) = responses_reasoning_item_from_anthropic_block(&item_id, &block)
                {
                    output.push(item);
                }
            }
            Part::Image { .. } | Part::Document { .. } | Part::ToolResult { .. } => {}
        }
    }
    flush_text(&mut output, &mut text_parts);
    output
}

fn chat_output(
    response: &ChatResponse,
    response_id: &str,
    tool_context: &CodexToolContext,
) -> Vec<Value> {
    let reasoning: Option<&str> = response.parts.iter().find_map(|part| match part {
        Part::Reasoning { text, .. } if !text.is_empty() => Some(text.as_str()),
        _ => None,
    });
    let mut output = Vec::new();
    if let Some(reasoning) = reasoning {
        output.push(json!({
            "id": format!("rs_{response_id}"),
            "type": "reasoning",
            "summary": [{ "type": "summary_text", "text": reasoning }]
        }));
    }
    let content: Vec<Value> = response
        .parts
        .iter()
        .filter_map(|part| match part {
            Part::Text { text } if !text.is_empty() => Some(json!({
                "type": "output_text",
                "text": text,
                "annotations": []
            })),
            _ => None,
        })
        .collect();
    if !content.is_empty() {
        output.push(json!({
            "id": format!("{response_id}_msg"),
            "type": "message",
            "status": "completed",
            "role": "assistant",
            "content": content
        }));
    }
    for part in &response.parts {
        if let Part::ToolCall {
            id,
            name,
            arguments,
        } = part
        {
            let item_id = response_tool_call_item_id_from_chat_name(id, name, tool_context);
            output.push(response_tool_call_item_from_chat_name(
                &item_id,
                "completed",
                id,
                name,
                &canonicalize_tool_arguments_str(arguments),
                reasoning,
                tool_context,
            ));
        }
    }
    output
}
