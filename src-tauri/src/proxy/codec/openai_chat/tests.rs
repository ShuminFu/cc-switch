use super::*;
use crate::proxy::codec::Codec;
use crate::proxy::protocol::ir::{Message, Part, PartDelta, Role, StreamEvent, ToolDef};
use crate::proxy::protocol::StopReason;
use crate::proxy::usage::ir::UsageIR;
use serde_json::json;

fn decode_all(chunks: &[&str]) -> Vec<StreamEvent> {
    let mut decoder = OpenAiChatEventDecoder::new();
    let mut events = Vec::new();
    for chunk in chunks {
        events.extend(decoder.decode("", chunk).unwrap());
    }
    events.extend(decoder.finish().unwrap());
    events
}

#[test]
fn encode_request_builds_chat_messages() {
    let request = ChatRequest {
        model: "o3".into(),
        system: vec![Part::text("a"), Part::text(""), Part::text("b")],
        messages: vec![
            Message::new(Role::User, vec![Part::text("hi")]),
            Message::new(
                Role::Assistant,
                vec![
                    Part::Reasoning {
                        text: "plan".into(),
                        signature: None,
                    },
                    Part::ToolCall {
                        id: "c1".into(),
                        name: "f".into(),
                        arguments: "{}".into(),
                    },
                ],
            ),
            Message::new(
                Role::User,
                vec![Part::ToolResult {
                    call_id: "c1".into(),
                    content: vec![Part::text("done")],
                    is_error: false,
                    plain: false,
                }],
            ),
        ],
        tools: vec![ToolDef {
            name: "f".into(),
            description: None,
            input_schema: json!({}),
            strict: None,
        }],
        max_tokens: Some(64),
        temperature: Some(0.5),
        stream: true,
        ..ChatRequest::default()
    };
    let body = OpenAiChatCodec::new(true).encode_request(&request).unwrap();
    assert_eq!(
        body,
        json!({
            "model": "o3",
            "messages": [
                {"role": "system", "content": "a\nb"},
                {"role": "user", "content": "hi"},
                {"role": "assistant", "content": null, "tool_calls": [
                    {"id": "c1", "type": "function", "function": {"name": "f", "arguments": "{}"}}
                ], "reasoning_content": "plan"},
                {"role": "tool", "tool_call_id": "c1", "content": "done"}
            ],
            "max_completion_tokens": 64,
            "temperature": 0.5,
            "stream": true,
            "tools": [{"type": "function", "function": {
                "name": "f",
                "description": null,
                "parameters": {"type": "object", "properties": {}}
            }}]
        })
    );
    // 通用上游不发送 reasoning_content
    let generic = OpenAiChatCodec::default().encode_request(&request).unwrap();
    assert!(generic["messages"][2].get("reasoning_content").is_none());
}

#[test]
fn stream_decoder_emits_ir_events_in_order() {
    let events = decode_all(&[
        r#"{"id":"c1","model":"m","choices":[{"delta":{"reasoning":"r"}}],"usage":{"prompt_tokens":5}}"#,
        r#"{"choices":[{"delta":{"content":"t"}}]}"#,
        r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{}"}}]}}]}"#,
        r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call","function":{"name":"f"}}]}}]}"#,
        r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#,
        "[DONE]",
    ]);
    let usage = UsageIR::from_chat_completions(&json!({"prompt_tokens": 5}));
    assert_eq!(
        events,
        vec![
            StreamEvent::Usage(usage),
            StreamEvent::MessageStart {
                id: "c1".into(),
                model: "m".into(),
                usage: Some(usage),
                created: None,
            },
            StreamEvent::PartStart {
                index: 0,
                part: Part::Reasoning {
                    text: String::new(),
                    signature: None
                },
            },
            StreamEvent::PartDelta {
                index: 0,
                delta: PartDelta::Reasoning("r".into()),
            },
            StreamEvent::PartStop { index: 0 },
            StreamEvent::PartStart {
                index: 1,
                part: Part::text(""),
            },
            StreamEvent::PartDelta {
                index: 1,
                delta: PartDelta::Text("t".into()),
            },
            StreamEvent::PartStop { index: 1 },
            // 参数先于 id/name 到达：开始事件推迟，缓存的参数随后补发
            StreamEvent::PartStart {
                index: 2,
                part: Part::ToolCall {
                    id: "call".into(),
                    name: "f".into(),
                    arguments: String::new(),
                },
            },
            StreamEvent::PartDelta {
                index: 2,
                delta: PartDelta::ToolArguments("{}".into()),
            },
            StreamEvent::PartStop { index: 2 },
            // stop + 已有工具调用 → tool_use
            StreamEvent::MessageStop {
                stop_reason: Some(StopReason::ToolUse),
            },
        ]
    );
}

#[test]
fn stream_decoder_reports_errors_and_truncation() {
    let mut decoder = OpenAiChatEventDecoder::new();
    let events = decoder
        .decode(
            "",
            r#"{"error":{"message":"slow down","type":"rate_limit"}}"#,
        )
        .unwrap();
    assert_eq!(
        events,
        vec![StreamEvent::Error {
            message: "slow down".into(),
            code: Some("rate_limit".into()),
        }]
    );
    assert!(decoder.finish().unwrap().is_empty());

    assert_eq!(
        decode_all(&[]),
        vec![StreamEvent::Error {
            message: "OpenAI Chat upstream stream ended before a finish_reason".into(),
            code: Some("stream_truncated".into()),
        }]
    );
    assert_eq!(
        decode_all(&[r#"{"choices":[{"delta":{"content":"cut"}}]}"#]).last(),
        Some(&StreamEvent::MessageStop {
            stop_reason: Some(StopReason::MaxTokens)
        })
    );
}

/// 非流式 = 流式 + 聚合：同一回复的流式与非流式形态解码出相同的 IR
#[test]
fn stream_aggregate_matches_non_stream_decode() {
    let codec = OpenAiChatCodec::default();
    let body = json!({
        "id": "c1",
        "model": "m",
        "choices": [{"message": {
            "reasoning_content": "think",
            "content": "answer",
            "tool_calls": [{"id": "call_1", "function": {"name": "f", "arguments": "{\"a\":1}"}}]
        }, "finish_reason": "tool_calls"}],
        "usage": {"prompt_tokens": 9, "completion_tokens": 3}
    });
    let non_stream = codec.decode_response(&body).unwrap();

    let chunks = [
        r#"{"id":"c1","model":"m","choices":[{"delta":{"reasoning_content":"think"}}]}"#,
        r#"{"choices":[{"delta":{"content":"ans"}}]}"#,
        r#"{"choices":[{"delta":{"content":"wer"}}]}"#,
        r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","function":{"name":"f","arguments":"{\"a\""}}]}}]}"#,
        r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":":1}"}}]}}]}"#,
        r#"{"choices":[{"delta":{},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":9,"completion_tokens":3}}"#,
        "[DONE]",
    ];
    let mut events = chunks.iter().map(|data| ("", *data));
    let streamed = codec.aggregate_stream(&mut events).unwrap();
    assert_eq!(streamed, non_stream);
    assert_eq!(non_stream.stop_reason, Some(StopReason::ToolUse));
}

#[test]
fn decode_response_rejects_bodies_without_a_message() {
    let codec = OpenAiChatCodec::default();
    for (body, message) in [
        (json!({}), "No choices in response"),
        (json!({"choices": []}), "Empty choices array"),
        (json!({"choices": [{}]}), "No message in choice"),
    ] {
        match codec.decode_response(&body) {
            Err(CodecError::Invalid { message: got, .. }) => assert_eq!(got, message),
            other => panic!("unexpected {other:?}"),
        }
    }
}

#[test]
fn client_directions_are_unsupported() {
    let codec = OpenAiChatCodec::default();
    assert!(matches!(
        codec.decode_request(&json!({})),
        Err(CodecError::Unsupported { .. })
    ));
    assert!(matches!(
        codec.encode_response(&ChatResponse::default()),
        Err(CodecError::Unsupported { .. })
    ));
    assert!(matches!(
        codec.stream_encoder(),
        Err(CodecError::Unsupported { .. })
    ));
}
