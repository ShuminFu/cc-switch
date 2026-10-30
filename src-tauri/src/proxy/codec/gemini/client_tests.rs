//! Gemini 作为客户端（Gemini CLI → Anthropic 上游）的编解码测试

use serde_json::{json, Value};

use super::client_response::{decode_thinking_envelope, gemini_error_from_anthropic};
use super::{GeminiClientCodec, GeminiEventEncoder};
use crate::proxy::codec::anthropic::{AnthropicCodec, AnthropicEventDecoder};
use crate::proxy::codec::{EventDecoder, EventEncoder, RequestCodec, ResponseCodec};
use crate::proxy::protocol::ir::{ChatResponse, Part, Role, StreamEvent, ToolChoice};
use crate::proxy::protocol::StopReason;
use crate::proxy::usage::ir::UsageIR;

fn decode(body: Value) -> crate::proxy::protocol::ir::ChatRequest {
    GeminiClientCodec::new("claude-sonnet-4-5", false)
        .decode_request(&body)
        .expect("decode Gemini request")
}

fn to_anthropic(body: Value) -> Value {
    AnthropicCodec
        .encode_request_with_default_max_tokens(&decode(body), 8192)
        .expect("encode Anthropic request")
}

#[test]
fn decodes_system_generation_config_tools_and_tool_choice() {
    let request = decode(json!({
        "systemInstruction": { "parts": [{ "text": "be brief" }, { "text": "use tools" }] },
        "contents": [{ "role": "user", "parts": [{ "text": "hi" }] }],
        "tools": [
            { "functionDeclarations": [{
                "name": "read_file",
                "description": "Read a file",
                "parameters": {
                    "type": "OBJECT",
                    "properties": {
                        "path": { "type": "STRING" },
                        "limit": { "type": "INTEGER", "nullable": true },
                        "tags": { "type": "ARRAY", "items": { "type": "STRING" } }
                    },
                    "required": ["path"]
                }
            }, {
                "name": "run",
                "parametersJsonSchema": { "type": "object", "properties": { "cmd": { "type": "string" } } }
            }] },
            { "googleSearch": {} }
        ],
        "toolConfig": { "functionCallingConfig": { "mode": "ANY", "allowedFunctionNames": ["run"] } },
        "generationConfig": {
            "temperature": 0.2,
            "topP": 0.9,
            "topK": 40,
            "maxOutputTokens": 1024,
            "stopSequences": ["END", " "],
            "candidateCount": 1
        }
    }));
    assert_eq!(request.model, "claude-sonnet-4-5");
    assert_eq!(
        request.system,
        vec![Part::text("be brief"), Part::text("use tools")]
    );
    assert_eq!(request.max_tokens, Some(1024));
    assert_eq!(request.temperature, Some(0.2));
    assert_eq!(
        request.top_p, None,
        "topP is dropped when temperature is set"
    );
    assert_eq!(request.top_k, Some(40));
    assert_eq!(request.stop, vec!["END".to_string()]);
    assert_eq!(
        request.tool_choice,
        Some(ToolChoice::Tool {
            name: "run".to_string()
        })
    );
    assert_eq!(request.tools.len(), 2, "built-in Gemini tools are dropped");
    assert_eq!(
        request.tools[0].input_schema,
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" },
                "limit": { "type": ["integer", "null"] },
                "tags": { "type": "array", "items": { "type": "string" } }
            },
            "required": ["path"]
        })
    );
    assert_eq!(
        request.tools[1].input_schema,
        json!({ "type": "object", "properties": { "cmd": { "type": "string" } } })
    );
}

#[test]
fn pairs_function_responses_without_ids_by_name_then_order() {
    let request = decode(json!({
        "contents": [
            { "role": "user", "parts": [{ "text": "list and read" }] },
            { "role": "model", "parts": [
                { "text": "Let me look." },
                { "functionCall": { "name": "ls", "args": { "dir": "." } } },
                { "functionCall": { "name": "read", "args": { "path": "a" } } }
            ] },
            { "role": "user", "parts": [
                { "functionResponse": { "name": "read", "response": { "output": "A" } } },
                { "functionResponse": { "name": "ls", "response": { "error": "denied" } } }
            ] }
        ]
    }));
    let calls: Vec<(&str, &str)> = request.messages[1]
        .parts
        .iter()
        .filter_map(|part| match part {
            Part::ToolCall { id, name, .. } => Some((id.as_str(), name.as_str())),
            _ => None,
        })
        .collect();
    assert_eq!(
        calls,
        vec![("call_gemini_1_1", "ls"), ("call_gemini_1_2", "read")]
    );
    assert_eq!(
        request.messages[2].parts,
        vec![
            Part::ToolResult {
                call_id: "call_gemini_1_2".to_string(),
                content: vec![Part::text("A")],
                is_error: false,
                plain: true,
            },
            Part::ToolResult {
                call_id: "call_gemini_1_1".to_string(),
                content: vec![Part::text("denied")],
                is_error: true,
                plain: true,
            },
        ]
    );
    // The Anthropic encoder keeps the turn: every tool_use is answered.
    let anthropic = AnthropicCodec
        .encode_request_with_default_max_tokens(&request, 8192)
        .unwrap();
    assert_eq!(anthropic["messages"].as_array().unwrap().len(), 3);
    assert_eq!(
        anthropic["messages"][2]["content"][1]["is_error"],
        json!(true)
    );
}

#[test]
fn keeps_and_sanitizes_client_ids_and_matches_by_id() {
    let request = decode(json!({
        "contents": [
            { "role": "user", "parts": [{ "text": "go" }] },
            { "role": "model", "parts": [
                { "functionCall": { "id": "toolu_01", "name": "a", "args": {} } },
                { "functionCall": { "id": "call.2/x", "name": "a", "args": {} } }
            ] },
            { "role": "user", "parts": [
                { "text": "results:" },
                { "functionResponse": { "id": "call.2/x", "name": "a", "response": { "output": 2 } } },
                { "functionResponse": { "id": "toolu_01", "name": "a", "response": { "x": 1 } } }
            ] }
        ]
    }));
    let results: Vec<(&str, &[Part])> = request.messages[2]
        .parts
        .iter()
        .filter_map(|part| match part {
            Part::ToolResult {
                call_id, content, ..
            } => Some((call_id.as_str(), content.as_slice())),
            _ => None,
        })
        .collect();
    assert_eq!(results[0].0, "call_2_x");
    assert_eq!(results[0].1, [Part::text("2")]);
    assert_eq!(results[1].0, "toolu_01");
    assert_eq!(results[1].1, [Part::text("{\"x\":1}")]);
    // Tool results move to the front of the user message.
    assert!(matches!(request.messages[2].parts[2], Part::Text { .. }));
}

#[test]
fn merges_consecutive_roles_and_drops_unsigned_thoughts() {
    let request = decode(json!({
        "contents": [
            { "role": "user", "parts": [{ "text": "a" }] },
            { "role": "user", "parts": [{ "text": "b" }] },
            { "role": "model", "parts": [
                { "text": "secret plan", "thought": true },
                { "text": "answer" }
            ] }
        ]
    }));
    assert_eq!(request.messages.len(), 2);
    assert_eq!(request.messages[0].role, Role::User);
    assert_eq!(
        request.messages[0].parts,
        vec![Part::text("a"), Part::text("b")]
    );
    assert_eq!(request.messages[1].parts, vec![Part::text("answer")]);
}

#[test]
fn maps_media_parts_and_describes_unsupported_ones() {
    let request = decode(json!({
        "contents": [{ "role": "user", "parts": [
            { "inlineData": { "mimeType": "image/png", "data": "iVBORw0KGgo=" } },
            { "inlineData": { "mimeType": "application/pdf", "data": "JVBERi0=" } },
            { "inlineData": { "mimeType": "text/plain", "data": "aGVsbG8=" } },
            { "inlineData": { "mimeType": "audio/mp3", "data": "AAAA" } },
            { "fileData": { "mimeType": "image/jpeg", "fileUri": "https://example.com/cat.jpg" } },
            { "fileData": { "mimeType": "video/mp4", "fileUri": "gs://bucket/v.mp4" } }
        ] }]
    }));
    let anthropic = AnthropicCodec
        .encode_request_with_default_max_tokens(&request, 8192)
        .unwrap();
    assert_eq!(
        anthropic["messages"][0]["content"],
        json!([
            { "type": "image", "source": { "type": "base64", "media_type": "image/png", "data": "iVBORw0KGgo=" } },
            { "type": "document", "source": { "type": "base64", "media_type": "application/pdf", "data": "JVBERi0=" } },
            { "type": "document", "source": { "type": "text", "media_type": "text/plain", "data": "hello" } },
            { "type": "text", "text": "[cc-switch: audio/mp3 attachment omitted, the upstream model cannot read it]" },
            { "type": "image", "source": { "type": "url", "url": "https://example.com/cat.jpg" } },
            { "type": "text", "text": "[cc-switch: video/mp4 attachment omitted, the upstream model cannot read it]" }
        ])
    );
}

#[test]
fn thinking_config_maps_to_anthropic_thinking() {
    let body = |config: Value| {
        json!({
            "contents": [{ "role": "user", "parts": [{ "text": "think" }] }],
            "generationConfig": { "maxOutputTokens": 32000, "temperature": 1, "thinkingConfig": config }
        })
    };
    let high = to_anthropic(body(
        json!({ "thinkingBudget": 12000, "includeThoughts": true }),
    ));
    assert_eq!(
        high["thinking"],
        json!({ "type": "enabled", "budget_tokens": 12000 })
    );
    assert!(
        high.get("temperature").is_none(),
        "sampling is off with thinking"
    );

    let off = to_anthropic(body(json!({ "thinkingBudget": 0 })));
    assert_eq!(off["thinking"], json!({ "type": "disabled" }));

    let dynamic = to_anthropic(body(json!({ "thinkingBudget": -1 })));
    assert!(dynamic.get("thinking").is_none());
    assert_eq!(dynamic["temperature"], json!(1));

    let level = to_anthropic(body(json!({ "thinkingLevel": "LOW" })));
    assert_eq!(
        level["thinking"],
        json!({ "type": "enabled", "budget_tokens": 2048 })
    );
}

#[test]
fn json_response_mime_type_becomes_a_system_instruction() {
    let anthropic = to_anthropic(json!({
        "systemInstruction": { "parts": [{ "text": "judge" }] },
        "contents": [{ "role": "user", "parts": [{ "text": "next speaker?" }] }],
        "generationConfig": {
            "responseMimeType": "application/json",
            "responseJsonSchema": { "type": "object", "properties": { "next_speaker": { "type": "string" } } }
        }
    }));
    let system = anthropic["system"].as_str().unwrap();
    assert!(system.starts_with("judge\n\nRespond with a single valid JSON value only"));
    assert!(
        system.ends_with(r#"{"properties":{"next_speaker":{"type":"string"}},"type":"object"}"#)
    );
}

#[test]
fn top_p_is_kept_without_temperature() {
    let request = decode(json!({
        "contents": [{ "role": "user", "parts": [{ "text": "hi" }] }],
        "generationConfig": { "topP": 0.5 }
    }));
    assert_eq!(request.top_p, Some(0.5));
}

#[test]
fn non_object_body_is_an_invalid_request() {
    assert!(GeminiClientCodec::new("m", false)
        .decode_request(&json!([1, 2]))
        .is_err());
}

fn signed_response() -> ChatResponse {
    ChatResponse {
        id: "msg_1".to_string(),
        model: "claude-sonnet-4-5".to_string(),
        parts: vec![
            Part::Reasoning {
                text: "plan".to_string(),
                signature: Some("sig-1".to_string()),
            },
            Part::RedactedReasoning {
                data: "opaque".to_string(),
            },
            Part::text("Reading."),
            Part::ToolCall {
                id: "toolu_1".to_string(),
                name: "read".to_string(),
                arguments: r#"{"path":"a"}"#.to_string(),
            },
            Part::ToolCall {
                id: "toolu_2".to_string(),
                name: "ls".to_string(),
                arguments: String::new(),
            },
        ],
        stop_reason: Some(StopReason::ToolUse),
        usage: Some(UsageIR::from_anthropic(&json!({
            "input_tokens": 10,
            "cache_read_input_tokens": 90,
            "output_tokens": 5
        }))),
        created: None,
    }
}

#[test]
fn encodes_response_with_thinking_envelope_on_first_call() {
    let encoded = GeminiClientCodec::default()
        .encode_response(&signed_response())
        .unwrap();
    let parts = encoded["candidates"][0]["content"]["parts"]
        .as_array()
        .unwrap();
    assert_eq!(parts[0], json!({ "text": "plan", "thought": true }));
    assert_eq!(parts[1], json!({ "text": "Reading." }));
    assert_eq!(
        parts[2]["functionCall"],
        json!({ "id": "toolu_1", "name": "read", "args": { "path": "a" } })
    );
    let envelope = parts[2]["thoughtSignature"].as_str().unwrap();
    assert!(parts[3].get("thoughtSignature").is_none());
    assert_eq!(parts[3]["functionCall"]["args"], json!({}));
    assert_eq!(encoded["candidates"][0]["finishReason"], json!("STOP"));
    assert_eq!(
        encoded["usageMetadata"],
        json!({
            "promptTokenCount": 100,
            "candidatesTokenCount": 5,
            "totalTokenCount": 105,
            "cachedContentTokenCount": 90
        })
    );
    assert_eq!(encoded["modelVersion"], json!("claude-sonnet-4-5"));
    assert_eq!(encoded["responseId"], json!("msg_1"));

    // The envelope restores both signed blocks on the next request.
    assert_eq!(
        decode_thinking_envelope(envelope),
        vec![
            Part::Reasoning {
                text: "plan".to_string(),
                signature: Some("sig-1".to_string()),
            },
            Part::RedactedReasoning {
                data: "opaque".to_string(),
            },
        ]
    );
    assert!(decode_thinking_envelope("skip_thought_signature_validator").is_empty());
}

#[test]
fn thinking_envelope_round_trips_into_the_tool_result_request() {
    let encoded = GeminiClientCodec::default()
        .encode_response(&signed_response())
        .unwrap();
    let model_turn = encoded["candidates"][0]["content"].clone();
    let anthropic = to_anthropic(json!({
        "contents": [
            { "role": "user", "parts": [{ "text": "read a" }] },
            model_turn,
            { "role": "user", "parts": [
                { "functionResponse": { "id": "toolu_1", "name": "read", "response": { "output": "A" } } },
                { "functionResponse": { "id": "toolu_2", "name": "ls", "response": { "output": "a b" } } }
            ] }
        ],
        "generationConfig": { "maxOutputTokens": 32000, "thinkingConfig": { "thinkingBudget": 4096 } }
    }));
    assert_eq!(
        anthropic["messages"][1]["content"],
        json!([
            { "type": "thinking", "thinking": "plan", "signature": "sig-1" },
            { "type": "redacted_thinking", "data": "opaque" },
            { "type": "text", "text": "Reading." },
            { "type": "tool_use", "id": "toolu_1", "name": "read", "input": { "path": "a" } },
            { "type": "tool_use", "id": "toolu_2", "name": "ls", "input": {} }
        ])
    );
    // Signed thinking is replayed, so the tool-result continuation keeps thinking on.
    assert_eq!(
        anthropic["thinking"],
        json!({ "type": "enabled", "budget_tokens": 4096 })
    );
}

fn encode_stream(events: &[StreamEvent]) -> Vec<Value> {
    let mut encoder = GeminiEventEncoder::new();
    let mut frames: Vec<bytes::Bytes> = events.iter().flat_map(|e| encoder.encode(e)).collect();
    frames.extend(encoder.finish());
    frames
        .iter()
        .map(|frame| {
            let text = std::str::from_utf8(frame).unwrap();
            let data = text
                .strip_prefix("data: ")
                .and_then(|rest| rest.strip_suffix("\n\n"))
                .unwrap_or_else(|| panic!("bad frame {text:?}"));
            serde_json::from_str(data).unwrap()
        })
        .collect()
}

#[test]
fn stream_encoder_attaches_finish_reason_and_usage_to_the_last_frame() {
    let frames = encode_stream(&signed_response().to_events());
    let parts: Vec<Value> = frames
        .iter()
        .map(|frame| frame["candidates"][0]["content"]["parts"].clone())
        .collect();
    assert_eq!(
        parts,
        vec![
            json!([{ "text": "plan", "thought": true }]),
            json!([{ "text": "Reading." }]),
            json!([{
                "functionCall": { "id": "toolu_1", "name": "read", "args": { "path": "a" } },
                "thoughtSignature": frames[2]["candidates"][0]["content"]["parts"][0]["thoughtSignature"]
            }]),
            json!([{ "functionCall": { "id": "toolu_2", "name": "ls", "args": {} } }]),
        ]
    );
    assert!(frames[..3]
        .iter()
        .all(|frame| frame["candidates"][0].get("finishReason").is_none()));
    assert_eq!(frames[3]["candidates"][0]["finishReason"], json!("STOP"));
    assert_eq!(frames[3]["usageMetadata"]["totalTokenCount"], json!(105));
    assert_eq!(
        GeminiClientCodec::default()
            .encode_response(&signed_response())
            .unwrap()["candidates"][0]["content"]["parts"][2]["thoughtSignature"],
        frames[2]["candidates"][0]["content"]["parts"][0]["thoughtSignature"]
    );
}

#[test]
fn stream_encoder_reports_errors_and_empty_replies() {
    let frames = encode_stream(&[
        StreamEvent::MessageStart {
            id: "msg_1".to_string(),
            model: "m".to_string(),
            usage: None,
            created: None,
        },
        StreamEvent::PartStart {
            index: 0,
            part: Part::text(""),
        },
        StreamEvent::PartDelta {
            index: 0,
            delta: crate::proxy::protocol::ir::PartDelta::Text("partial".to_string()),
        },
        StreamEvent::Error {
            message: "Overloaded".to_string(),
            code: Some("overloaded_error".to_string()),
        },
        StreamEvent::MessageStop { stop_reason: None },
    ]);
    assert_eq!(frames.len(), 2);
    assert_eq!(
        frames[0]["candidates"][0]["content"]["parts"],
        json!([{ "text": "partial" }])
    );
    assert_eq!(
        frames[1],
        json!({ "error": { "code": 503, "message": "Overloaded", "status": "UNAVAILABLE" } })
    );

    let empty = encode_stream(
        &ChatResponse {
            stop_reason: Some(StopReason::MaxTokens),
            ..ChatResponse::default()
        }
        .to_events(),
    );
    assert_eq!(empty.len(), 1);
    assert_eq!(
        empty[0]["candidates"][0]["finishReason"],
        json!("MAX_TOKENS")
    );
    assert_eq!(empty[0]["candidates"][0]["content"]["parts"], json!([]));
}

#[test]
fn anthropic_stream_transcodes_to_gemini_frames() {
    let anthropic = [
        (
            "message_start",
            json!({"type":"message_start","message":{"id":"msg_9","model":"claude-x","usage":{"input_tokens":7,"output_tokens":1}}}),
        ),
        (
            "content_block_start",
            json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
        ),
        (
            "content_block_delta",
            json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hel"}}),
        ),
        (
            "content_block_delta",
            json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"lo"}}),
        ),
        (
            "content_block_stop",
            json!({"type":"content_block_stop","index":0}),
        ),
        (
            "message_delta",
            json!({"type":"message_delta","delta":{"stop_reason":"max_tokens"},"usage":{"output_tokens":3}}),
        ),
        ("message_stop", json!({"type":"message_stop"})),
    ];
    let mut decoder = AnthropicEventDecoder::new();
    let mut events = Vec::new();
    for (name, data) in &anthropic {
        events.extend(decoder.decode(name, &data.to_string()).unwrap());
    }
    events.extend(decoder.finish().unwrap());
    let frames = encode_stream(&events);
    assert_eq!(frames.len(), 2);
    assert_eq!(
        frames[1],
        json!({
            "candidates": [{
                "content": { "role": "model", "parts": [{ "text": "lo" }] },
                "index": 0,
                "finishReason": "MAX_TOKENS"
            }],
            "usageMetadata": { "promptTokenCount": 7, "candidatesTokenCount": 3, "totalTokenCount": 10 },
            "modelVersion": "claude-x",
            "responseId": "msg_9"
        })
    );
}

#[test]
fn anthropic_error_envelope_becomes_a_gemini_error() {
    assert_eq!(
        gemini_error_from_anthropic(
            &json!({"type":"error","error":{"type":"rate_limit_error","message":"slow down"}}),
            429
        ),
        Some(
            json!({ "error": { "code": 429, "message": "slow down", "status": "RESOURCE_EXHAUSTED" } })
        )
    );
    assert_eq!(
        gemini_error_from_anthropic(
            &json!({"error":{"code":400,"message":"x","status":"INVALID_ARGUMENT"}}),
            400
        ),
        None,
        "Gemini-shaped errors are left alone"
    );
}

#[test]
fn client_codec_rejects_upstream_directions() {
    let codec = GeminiClientCodec::default();
    assert!(codec.encode_request(&Default::default()).is_err());
    assert!(codec.decode_response(&json!({})).is_err());
    assert!(crate::proxy::codec::StreamCodec::stream_decoder(&codec).is_err());
}
