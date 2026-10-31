//! Request-side canonical IR (first increment): Anthropic Messages request →
//! IR → OpenAI Chat Completions request.
//!
//! The response side of every bridge already goes through [`super`]; requests
//! are still converted by hand-written pairwise functions
//! (`transform::anthropic_to_openai_with_reasoning_content` and siblings).
//! This module introduces the canonical request shape and one bridge
//! direction. It covers the text / tool-call subset; anything else (images,
//! documents, thinking blocks, non-text tool results, `system`-role
//! messages) makes [`decode_anthropic_request`] return
//! [`Unsupported`], so a caller can fall back to the legacy converter. It is
//! not wired into the request path yet: the golden fixtures under
//! `testdata/request/` pin that, on the covered subset, the IR path produces
//! exactly what the legacy converter produces.

use serde_json::{json, Map, Value};

use crate::proxy::json_canonical::canonical_json_string;
use crate::proxy::providers::transform::{
    apply_chat_token_limit_and_sampling, clean_schema, map_tool_choice_to_chat,
    resolve_reasoning_effort, strip_leading_anthropic_billing_header, supports_reasoning_effort,
    tool_choice_disables_parallel,
};

/// The request uses something this increment does not model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Unsupported(pub String);

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Role {
    User,
    Assistant,
}

impl Role {
    fn as_str(&self) -> &'static str {
        match self {
            Role::User => "user",
            Role::Assistant => "assistant",
        }
    }
}

/// One part of a request message.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum RequestPart {
    Text(String),
    ToolCall {
        id: String,
        name: String,
        input: Value,
    },
    ToolResult {
        tool_use_id: String,
        text: String,
        is_error: bool,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum MessageContent {
    /// `content` was missing.
    Absent,
    /// A plain string.
    Text(String),
    Parts(Vec<RequestPart>),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RequestMessage {
    pub role: Role,
    pub content: MessageContent,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ToolDef {
    pub name: String,
    /// Kept as a JSON value: `null` when absent, as the legacy converter does.
    pub description: Value,
    pub parameters: Value,
}

/// Canonical request.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Request {
    pub model: Option<String>,
    /// System prompt segments (billing header stripped, empty ones dropped).
    pub system: Vec<String>,
    pub messages: Vec<RequestMessage>,
    pub max_tokens: Option<Value>,
    pub temperature: Option<Value>,
    pub top_p: Option<Value>,
    pub stop: Option<Value>,
    pub stream: Option<Value>,
    pub reasoning_effort: Option<&'static str>,
    pub tools: Vec<ToolDef>,
    /// The client's tool choice in Anthropic form.
    pub tool_choice: Option<Value>,
    pub disable_parallel_tool_use: bool,
}

fn unsupported(what: impl Into<String>) -> Unsupported {
    Unsupported(what.into())
}

fn decode_tool_result(block: &Value) -> Result<RequestPart, Unsupported> {
    let text = match block.get("content") {
        None => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(items)) => {
            let mut texts = Vec::new();
            for item in items {
                match item.get("type").and_then(Value::as_str) {
                    Some("text") => {
                        if let Some(t) = item.get("text").and_then(Value::as_str) {
                            texts.push(t.to_string());
                        }
                    }
                    other => {
                        return Err(unsupported(format!(
                            "tool_result item {}",
                            other.unwrap_or("?")
                        )))
                    }
                }
            }
            texts.join("\n")
        }
        Some(other) => canonical_json_string(other),
    };
    Ok(RequestPart::ToolResult {
        tool_use_id: block
            .get("tool_use_id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        text,
        is_error: block.get("is_error").and_then(Value::as_bool) == Some(true),
    })
}

/// Anthropic Messages request → IR.
pub(crate) fn decode_anthropic_request(body: &Value) -> Result<Request, Unsupported> {
    let mut req = Request {
        model: body
            .get("model")
            .and_then(Value::as_str)
            .map(str::to_string),
        ..Default::default()
    };

    match body.get("system") {
        Some(Value::String(text)) => req.system.push(text.clone()),
        Some(Value::Array(items)) => {
            for item in items {
                if let Some(text) = item.get("text").and_then(Value::as_str) {
                    req.system.push(text.to_string());
                }
            }
        }
        _ => {}
    }
    req.system = req
        .system
        .iter()
        .map(|t| strip_leading_anthropic_billing_header(t).to_string())
        .filter(|t| !t.is_empty())
        .collect();

    for msg in body
        .get("messages")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let role = match msg.get("role").and_then(Value::as_str).unwrap_or("user") {
            "user" => Role::User,
            "assistant" => Role::Assistant,
            other => return Err(unsupported(format!("role {other}"))),
        };
        let content = match msg.get("content") {
            None => MessageContent::Absent,
            Some(Value::String(s)) => MessageContent::Text(s.clone()),
            Some(Value::Array(blocks)) => {
                let mut parts = Vec::new();
                for block in blocks {
                    match block.get("type").and_then(Value::as_str).unwrap_or("") {
                        "text" => {
                            if let Some(t) = block.get("text").and_then(Value::as_str) {
                                parts.push(RequestPart::Text(t.to_string()));
                            }
                        }
                        "tool_use" => parts.push(RequestPart::ToolCall {
                            id: block
                                .get("id")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_string(),
                            name: block
                                .get("name")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_string(),
                            input: block.get("input").cloned().unwrap_or(json!({})),
                        }),
                        "tool_result" => parts.push(decode_tool_result(block)?),
                        other => return Err(unsupported(format!("content block {other}"))),
                    }
                }
                MessageContent::Parts(parts)
            }
            Some(_) => return Err(unsupported("non-array message content")),
        };
        req.messages.push(RequestMessage { role, content });
    }

    req.max_tokens = body.get("max_tokens").cloned();
    req.temperature = body.get("temperature").cloned();
    req.top_p = body.get("top_p").cloned();
    req.stop = body.get("stop_sequences").cloned();
    req.stream = body.get("stream").cloned();
    if supports_reasoning_effort(req.model.as_deref().unwrap_or("")) {
        req.reasoning_effort = resolve_reasoning_effort(body);
    }

    for tool in body
        .get("tools")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if tool.get("type").and_then(Value::as_str) == Some("BatchTool") {
            continue;
        }
        req.tools.push(ToolDef {
            name: tool
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            description: tool.get("description").cloned().unwrap_or(Value::Null),
            parameters: clean_schema(tool.get("input_schema").cloned().unwrap_or(json!({}))),
        });
    }
    if let Some(choice) = body.get("tool_choice") {
        req.disable_parallel_tool_use = tool_choice_disables_parallel(choice);
        req.tool_choice = Some(choice.clone());
    }
    Ok(req)
}

fn encode_chat_message(msg: &RequestMessage, out: &mut Vec<Value>) {
    let role = msg.role.as_str();
    let parts = match &msg.content {
        MessageContent::Absent => {
            out.push(json!({"role": role, "content": null}));
            return;
        }
        MessageContent::Text(text) => {
            out.push(json!({"role": role, "content": text}));
            return;
        }
        MessageContent::Parts(parts) => parts,
    };
    let mut texts: Vec<Value> = Vec::new();
    let mut tool_calls: Vec<Value> = Vec::new();
    for part in parts {
        match part {
            RequestPart::Text(text) => texts.push(json!({"type": "text", "text": text})),
            RequestPart::ToolCall { id, name, input } => tool_calls.push(json!({
                "id": id,
                "type": "function",
                "function": { "name": name, "arguments": canonical_json_string(input) }
            })),
            RequestPart::ToolResult {
                tool_use_id,
                text,
                is_error,
            } => {
                let content = match (*is_error, text.is_empty()) {
                    (true, true) => "[error]".to_string(),
                    (true, false) => format!("[error] {text}"),
                    (false, _) => text.clone(),
                };
                out.push(json!({"role": "tool", "tool_call_id": tool_use_id, "content": content}));
            }
        }
    }
    if texts.is_empty() && tool_calls.is_empty() {
        return;
    }
    let mut message = Map::new();
    message.insert("role".into(), json!(role));
    let content = match texts.len() {
        0 => Value::Null,
        1 => texts[0]["text"].clone(),
        _ => Value::Array(texts),
    };
    message.insert("content".into(), content);
    if !tool_calls.is_empty() {
        message.insert("tool_calls".into(), Value::Array(tool_calls));
    }
    out.push(Value::Object(message));
}

/// IR → OpenAI Chat Completions request.
pub(crate) fn encode_chat_request(req: &Request) -> Value {
    let mut result = json!({});
    if let Some(model) = &req.model {
        result["model"] = json!(model);
    }
    let mut messages = Vec::new();
    match req.system.len() {
        0 => {}
        1 => messages.push(json!({"role": "system", "content": req.system[0]})),
        _ => messages.push(json!({"role": "system", "content": req.system.join("\n")})),
    }
    for msg in &req.messages {
        encode_chat_message(msg, &mut messages);
    }
    result["messages"] = Value::Array(messages);

    let model = req.model.as_deref().unwrap_or("");
    apply_chat_token_limit_and_sampling(
        &mut result,
        model,
        req.max_tokens.as_ref(),
        req.temperature.as_ref(),
        req.top_p.as_ref(),
    );
    if let Some(stop) = &req.stop {
        result["stop"] = stop.clone();
    }
    if let Some(stream) = &req.stream {
        result["stream"] = stream.clone();
    }
    if let Some(effort) = req.reasoning_effort {
        result["reasoning_effort"] = json!(effort);
    }
    if !req.tools.is_empty() {
        result["tools"] = Value::Array(
            req.tools
                .iter()
                .map(|t| {
                    json!({
                        "type": "function",
                        "function": {
                            "name": t.name,
                            "description": t.description,
                            "parameters": t.parameters
                        }
                    })
                })
                .collect(),
        );
    }
    if let Some(choice) = &req.tool_choice {
        result["tool_choice"] = map_tool_choice_to_chat(choice);
        if req.disable_parallel_tool_use && result.get("tools").is_some() {
            result["parallel_tool_calls"] = json!(false);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proxy::providers::transform::anthropic_to_openai;

    const FIXTURES: &[(&str, &str)] = &[
        (
            "text_and_system",
            include_str!("testdata/request/text_and_system.json"),
        ),
        (
            "tool_round_trip",
            include_str!("testdata/request/tool_round_trip.json"),
        ),
        (
            "reasoning_model_params",
            include_str!("testdata/request/reasoning_model_params.json"),
        ),
    ];

    #[test]
    fn golden_fixtures_match_ir_and_legacy_converter() {
        for (name, text) in FIXTURES {
            let fixture: Value = serde_json::from_str(text).expect(name);
            let anthropic = &fixture["anthropic"];
            let golden = &fixture["chat"];
            let ir =
                decode_anthropic_request(anthropic).unwrap_or_else(|e| panic!("{name}: {e:?}"));
            assert_eq!(&encode_chat_request(&ir), golden, "{name}: IR path");
            assert_eq!(
                &anthropic_to_openai(anthropic.clone()).unwrap(),
                golden,
                "{name}: legacy converter"
            );
        }
    }

    #[test]
    fn unsupported_content_is_reported_for_fallback() {
        let image = json!({"messages": [{"role": "user", "content": [
            {"type": "image", "source": {"type": "url", "url": "https://x/a.png"}}
        ]}]});
        assert_eq!(
            decode_anthropic_request(&image).unwrap_err(),
            Unsupported("content block image".into())
        );
        let thinking = json!({"messages": [{"role": "assistant", "content": [
            {"type": "thinking", "thinking": "hm", "signature": "s"}
        ]}]});
        assert!(decode_anthropic_request(&thinking).is_err());
        let system_role = json!({"messages": [{"role": "system", "content": "x"}]});
        assert!(decode_anthropic_request(&system_role).is_err());
    }
}
