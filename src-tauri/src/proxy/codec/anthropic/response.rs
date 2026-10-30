//! [`ChatResponse`] → Anthropic Messages 响应体

use serde_json::{json, Value};

use crate::proxy::protocol::ir::{ChatResponse, Part};

pub(super) fn encode_response(response: &ChatResponse) -> Value {
    let content: Vec<Value> = response.parts.iter().filter_map(encode_part).collect();
    json!({
        "id": response.id,
        "type": "message",
        "role": "assistant",
        "content": content,
        "model": response.model,
        "stop_reason": response.stop_reason.map(|reason| reason.as_anthropic_str()),
        "stop_sequence": null,
        // 上游缺 usage 时也输出全 0 usage，客户端依赖该字段存在
        "usage": response.usage.unwrap_or_default().to_anthropic()
    })
}

/// 助手回复中的内容块；回复里不会出现的块（图片、工具结果等）跳过
fn encode_part(part: &Part) -> Option<Value> {
    Some(match part {
        Part::Text { text } => json!({"type": "text", "text": text}),
        Part::Reasoning { text, signature } => {
            let mut block = json!({"type": "thinking", "thinking": text});
            if let Some(signature) = signature {
                block["signature"] = json!(signature);
            }
            block
        }
        Part::RedactedReasoning { data } => json!({"type": "redacted_thinking", "data": data}),
        Part::ToolCall {
            id,
            name,
            arguments,
        } => json!({
            "type": "tool_use",
            "id": id,
            "name": name,
            // 参数不是合法 JSON 时回退为空对象，避免客户端拿到字符串 input
            "input": serde_json::from_str::<Value>(arguments).unwrap_or_else(|_| json!({}))
        }),
        Part::Image { .. } | Part::Document { .. } | Part::ToolResult { .. } => {
            log::warn!("[Codec/Anthropic] Dropping a non-assistant content part from a response");
            return None;
        }
    })
}
