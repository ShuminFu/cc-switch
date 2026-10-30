//! Gemini `GenerateContentResponse` → [`ChatResponse`]
//!
//! 与旧转换器一致：只取第一个候选；思考 parts（`thought: true`）不输出；缺 id 的
//! functionCall 合成 id；提示被拦截（`promptFeedback.blockReason`）时输出一段说明
//! 文本并以 refusal 结束。工具参数原样保留键顺序（不做 canonical 排序）。

use serde_json::{json, Value};

use super::invalid;
use super::tool_calls::{
    extract_tool_call_meta, rectify_tool_call_parts, synthesize_tool_call_id,
    AnthropicToolSchemaHints,
};
use crate::proxy::codec::CodecError;
use crate::proxy::protocol::ir::{ChatResponse, Part};
use crate::proxy::protocol::StopReason;
use crate::proxy::providers::gemini_shadow::GeminiAssistantTurn;
use crate::proxy::usage::ir::UsageIR;

/// 被拦截提示的说明文本（流式与非流式共用）
pub(super) fn blocked_text(reason: &str) -> String {
    format!("Request blocked by Gemini safety filters: {reason}")
}

/// 解码回复，并给出需要记入 shadow 的助手轮次（候选没有 `content` 或提示被拦截时为 `None`）
pub(super) fn decode_response(
    body: &Value,
    tool_schema_hints: Option<&AnthropicToolSchemaHints>,
) -> Result<(ChatResponse, Option<GeminiAssistantTurn>), CodecError> {
    let mut response = ChatResponse {
        id: str_field(body, "responseId").to_string(),
        model: str_field(body, "modelVersion").to_string(),
        // 上游缺 usageMetadata 时 Anthropic 编码端输出全 0 usage
        usage: body.get("usageMetadata").map(UsageIR::from_gemini),
        ..ChatResponse::default()
    };

    if let Some(reason) = body
        .pointer("/promptFeedback/blockReason")
        .and_then(Value::as_str)
    {
        response.parts = vec![Part::text(blocked_text(reason))];
        response.stop_reason = Some(StopReason::Refusal);
        return Ok((response, None));
    }

    let candidate = body
        .get("candidates")
        .and_then(Value::as_array)
        .and_then(|candidates| candidates.first())
        .ok_or_else(|| invalid("No candidates in Gemini response"))?;

    let mut parts = candidate
        .pointer("/content/parts")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    rectify_tool_call_parts(&mut parts, tool_schema_hints);

    // 先给缺 id（或 id 为空串）的 functionCall 写入合成 id：客户端看到的 tool_use、
    // shadow 中的原始 parts 与工具调用元数据必须是同一个 id，否则下一轮的
    // tool_result 无法经 shadow 解析出函数名
    for function_call in parts
        .iter_mut()
        .filter_map(|part| part.get_mut("functionCall").and_then(Value::as_object_mut))
    {
        let missing = function_call
            .get("id")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty);
        if missing {
            function_call.insert("id".into(), json!(synthesize_tool_call_id()));
        }
    }

    for part in &parts {
        if part.get("thought").and_then(Value::as_bool) == Some(true) {
            continue;
        }
        if let Some(text) = part.get("text").and_then(Value::as_str) {
            if !text.is_empty() {
                response.parts.push(Part::text(text));
            }
            continue;
        }
        if let Some(function_call) = part.get("functionCall") {
            let args = function_call
                .get("args")
                .cloned()
                .unwrap_or_else(|| json!({}));
            response.parts.push(Part::ToolCall {
                id: str_field(function_call, "id").to_string(),
                name: str_field(function_call, "name").to_string(),
                arguments: serde_json::to_string(&args).unwrap_or_else(|_| "{}".to_string()),
            });
        }
    }

    response.stop_reason = Some(StopReason::from_gemini_finish_reason(
        candidate.get("finishReason").and_then(Value::as_str),
        response.has_tool_call(),
    ));

    let turn = candidate.get("content").map(|content| {
        let mut content = content.clone();
        let tool_calls = extract_tool_call_meta(&parts);
        if let Some(stored_parts) = content.get_mut("parts") {
            *stored_parts = Value::Array(parts);
        }
        GeminiAssistantTurn::new(content, tool_calls)
    });

    Ok((response, turn))
}

fn str_field<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or_default()
}
