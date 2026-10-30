use super::*;
use crate::proxy::codec::Codec;
use crate::proxy::protocol::ir::{
    MediaSource, Message, Part, PartDelta, ReasoningConfig, Role, StreamEvent, ToolChoice,
};
use crate::proxy::protocol::StopReason;
use crate::proxy::usage::ir::{InputSemantics, UsageIR};
use serde_json::json;

fn usage() -> UsageIR {
    UsageIR {
        fresh_input: 10,
        cache_read: 4,
        cache_write: 0,
        output: 7,
        reasoning: 0,
        semantics: InputSemantics::Total,
    }
}

#[test]
fn decode_request_maps_messages_tools_and_reasoning() {
    let body = json!({
        "model": "claude-x",
        "system": [{"type": "text", "text": "sys"}],
        "max_tokens": 100,
        "stream": true,
        "metadata": {"user_id": "u"},
        "thinking": {"type": "enabled", "budget_tokens": 5000},
        "tool_choice": {"type": "tool", "name": "f"},
        "tools": [{"name": "f", "input_schema": {"type": "object"}}, {"type": "BatchTool", "name": "b"}],
        "messages": [
            {"role": "system", "content": "folded"},
            {"role": "user", "content": [
                {"type": "image", "source": {"type": "url", "url": "https://i"}},
                {"type": "document", "title": "T", "source": {"type": "content", "content": [{"type": "text", "text": "a"}, {"type": "text", "text": "b"}]}},
                {"type": "tool_result", "tool_use_id": "c1", "content": [{"type": "unknown", "x": 1}], "is_error": true}
            ]},
            {"role": "assistant", "content": [
                {"type": "thinking", "thinking": "t", "signature": "s"},
                {"type": "tool_use", "id": "c2", "name": "f", "input": {"b": 1, "a": 2}}
            ]}
        ]
    });
    let request = AnthropicCodec.decode_request(&body).unwrap();
    assert_eq!(request.model, "claude-x");
    assert_eq!(
        request.system,
        vec![Part::text("sys"), Part::text("folded")]
    );
    assert_eq!(request.max_tokens, Some(100));
    assert!(request.stream);
    assert_eq!(request.tools.len(), 1);
    assert_eq!(
        request.tool_choice,
        Some(ToolChoice::Tool { name: "f".into() })
    );
    assert_eq!(
        request.reasoning,
        Some(ReasoningConfig {
            budget_tokens: Some(5000),
            effort: None,
            adaptive: false,
        })
    );
    assert_eq!(
        request.extra.get("metadata"),
        Some(&json!({"user_id": "u"}))
    );
    assert!(!request.extra.contains_key("thinking"));
    assert_eq!(
        request.messages,
        vec![
            Message::new(
                Role::User,
                vec![
                    Part::Image {
                        source: MediaSource::Url {
                            url: "https://i".into()
                        }
                    },
                    Part::Document {
                        source: MediaSource::Text {
                            data: "a\nb".into()
                        },
                        title: Some("T".into()),
                    },
                    Part::ToolResult {
                        call_id: "c1".into(),
                        content: vec![Part::text(r#"{"type":"unknown","x":1}"#)],
                        is_error: true,
                    },
                ]
            ),
            Message::new(
                Role::Assistant,
                vec![
                    Part::Reasoning {
                        text: "t".into(),
                        signature: Some("s".into()),
                    },
                    // canonical JSON：键排序
                    Part::ToolCall {
                        id: "c2".into(),
                        name: "f".into(),
                        arguments: r#"{"a":2,"b":1}"#.into(),
                    },
                ]
            ),
        ]
    );
}

#[test]
fn decode_request_reasoning_variants() {
    let reasoning =
        |body: serde_json::Value| AnthropicCodec.decode_request(&body).unwrap().reasoning;
    assert_eq!(reasoning(json!({"thinking": {"type": "disabled"}})), None);
    assert_eq!(
        reasoning(json!({"thinking": {"type": "adaptive"}})),
        Some(ReasoningConfig {
            adaptive: true,
            ..ReasoningConfig::default()
        })
    );
    assert_eq!(
        reasoning(json!({"thinking": {"type": "disabled"}, "output_config": {"effort": "max"}})),
        Some(ReasoningConfig {
            effort: Some("max".into()),
            ..ReasoningConfig::default()
        })
    );
}

#[test]
fn encode_response_writes_blocks_stop_reason_and_usage() {
    let response = ChatResponse {
        id: "msg_1".into(),
        model: "m".into(),
        parts: vec![
            Part::Reasoning {
                text: "t".into(),
                signature: Some("sig".into()),
            },
            Part::RedactedReasoning { data: "x".into() },
            Part::text("hi"),
            Part::ToolCall {
                id: "c".into(),
                name: "f".into(),
                arguments: "{\"a\":1}".into(),
            },
            Part::ToolCall {
                id: "d".into(),
                name: "g".into(),
                arguments: "{broken".into(),
            },
        ],
        stop_reason: Some(StopReason::ToolUse),
        usage: Some(usage()),
    };
    let body = AnthropicCodec.encode_response(&response).unwrap();
    assert_eq!(
        body,
        json!({
            "id": "msg_1",
            "type": "message",
            "role": "assistant",
            "content": [
                {"type": "thinking", "thinking": "t", "signature": "sig"},
                {"type": "redacted_thinking", "data": "x"},
                {"type": "text", "text": "hi"},
                {"type": "tool_use", "id": "c", "name": "f", "input": {"a": 1}},
                {"type": "tool_use", "id": "d", "name": "g", "input": {}}
            ],
            "model": "m",
            "stop_reason": "tool_use",
            "stop_sequence": null,
            "usage": {"input_tokens": 10, "output_tokens": 7, "cache_read_input_tokens": 4}
        })
    );
    let empty = AnthropicCodec
        .encode_response(&ChatResponse::default())
        .unwrap();
    assert_eq!(empty["stop_reason"], json!(null));
    assert_eq!(
        empty["usage"],
        json!({"input_tokens": 0, "output_tokens": 0})
    );
}

fn frames_text(frames: Vec<bytes::Bytes>) -> Vec<String> {
    frames
        .into_iter()
        .map(|frame| String::from_utf8(frame.to_vec()).unwrap())
        .collect()
}

#[test]
fn non_stream_response_encodes_as_anthropic_sse() {
    let response = ChatResponse {
        id: "msg_1".into(),
        model: "m".into(),
        parts: vec![
            Part::Reasoning {
                text: "t".into(),
                signature: Some("sig".into()),
            },
            Part::text("hi"),
        ],
        stop_reason: Some(StopReason::EndTurn),
        usage: Some(usage()),
    };
    let frames = frames_text(AnthropicCodec.encode_response_as_stream(&response).unwrap());
    let expected = [
        r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_1","type":"message","role":"assistant","model":"m","usage":{"input_tokens":0,"output_tokens":0}}}"#,
        r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":""}}"#,
        r#"event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"t"}}"#,
        r#"event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"sig"}}"#,
        r#"event: content_block_stop
data: {"type":"content_block_stop","index":0}"#,
        r#"event: content_block_start
data: {"type":"content_block_start","index":1,"content_block":{"type":"text","text":""}}"#,
        r#"event: content_block_delta
data: {"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"hi"}}"#,
        r#"event: content_block_stop
data: {"type":"content_block_stop","index":1}"#,
        r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"input_tokens":10,"output_tokens":7,"cache_read_input_tokens":4}}"#,
        r#"event: message_stop
data: {"type":"message_stop"}"#,
    ]
    .map(|frame| format!("{frame}\n\n"));
    assert_eq!(frames, expected);
}

#[test]
fn stream_encoder_message_start_usage_and_terminal_events() {
    let mut encoder = AnthropicCodec.stream_encoder().unwrap();
    let start = frames_text(encoder.encode(&StreamEvent::MessageStart {
        id: "m1".into(),
        model: "x".into(),
        usage: Some(usage()),
    }));
    assert!(start[0]
        .contains(r#""usage":{"input_tokens":10,"output_tokens":0,"cache_read_input_tokens":4}"#));
    // 开始事件的 usage 也作为结束时的 usage
    let stop = frames_text(encoder.encode(&StreamEvent::MessageStop { stop_reason: None }));
    assert!(stop[0].contains(r#""stop_reason":null"#));
    assert!(stop[0].contains(r#""output_tokens":7"#));
    assert!(encoder
        .encode(&StreamEvent::PartStop { index: 0 })
        .is_empty());

    // 未发开始事件就输出内容时补一个空的开始事件；错误不补开始事件且终止
    let mut encoder = AnthropicEventEncoder::new();
    let frames = frames_text(encoder.encode(&StreamEvent::PartDelta {
        index: 0,
        delta: PartDelta::Text("x".into()),
    }));
    assert_eq!(frames.len(), 2);
    assert!(frames[0].starts_with("event: message_start\n"));
    let mut encoder = AnthropicEventEncoder::new();
    let frames = frames_text(encoder.encode(&StreamEvent::Error {
        message: "boom".into(),
        code: None,
    }));
    assert_eq!(
        frames,
        vec![
            "event: error\ndata: {\"type\":\"error\",\"error\":{\"type\":\"upstream_error\",\"message\":\"boom\"}}\n\n"
                .to_string()
        ]
    );
    assert!(encoder
        .encode(&StreamEvent::MessageStop { stop_reason: None })
        .is_empty());
}

#[test]
fn upstream_directions_are_unsupported() {
    assert!(matches!(
        AnthropicCodec.encode_request(&ChatRequest::default()),
        Err(CodecError::Unsupported { .. })
    ));
    assert!(matches!(
        AnthropicCodec.decode_response(&json!({})),
        Err(CodecError::Unsupported { .. })
    ));
    assert!(matches!(
        AnthropicCodec.stream_decoder(),
        Err(CodecError::Unsupported { .. })
    ));
}
