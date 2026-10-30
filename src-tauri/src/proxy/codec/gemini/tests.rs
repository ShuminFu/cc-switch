//! Gemini 编解码：单元测试与旧转换器的差分测试
//!
//! golden 语料不带 shadow 存储，这里补上 shadow 回放 / 记录、工具结果解析与
//! 各种错误分支，断言两种引擎输出一致。

use std::sync::Arc;

use bytes::Bytes;
use futures::StreamExt;
use serde_json::{json, Value};

use super::*;
use crate::proxy::codec::bridge::{
    anthropic_to_gemini_request, gemini_to_anthropic_response, gemini_to_anthropic_sse,
};
use crate::proxy::codec::{Codec, CodecEngine};
use crate::proxy::protocol::ir::{
    Message, Part, PartDelta, Role, StreamEvent, ToolChoice, ToolDef,
};
use crate::proxy::protocol::StopReason;
use crate::proxy::providers::gemini_shadow::GeminiToolCallMeta;
use crate::proxy::usage::ir::UsageIR;

const ENGINES: [CodecEngine; 2] = [CodecEngine::Legacy, CodecEngine::Ir];
const PROVIDER: &str = "provider-1";
const SESSION: &str = "session-1";

fn sink(store: &Arc<GeminiShadowStore>) -> Option<GeminiShadowSink> {
    GeminiShadowSink::new(
        Some(store.clone()),
        Some(PROVIDER.to_string()),
        Some(SESSION.to_string()),
    )
}

fn request_with(
    body: &Value,
    store: Option<&GeminiShadowStore>,
    engine: CodecEngine,
) -> Result<String, String> {
    anthropic_to_gemini_request(body.clone(), store, Some(PROVIDER), Some(SESSION), engine)
        .map(|value| serde_json::to_string_pretty(&value).unwrap())
        .map_err(|error| error.to_string())
}

/// 两种引擎的请求输出（含字段顺序）与错误文本一致
#[track_caller]
fn assert_request_parity(body: Value, store: Option<&GeminiShadowStore>) -> Result<String, String> {
    let legacy = request_with(&body, store, CodecEngine::Legacy);
    let ir = request_with(&body, store, CodecEngine::Ir);
    assert_eq!(ir, legacy);
    ir
}

/// 把合成的随机 id 换成按出现顺序编号的占位符
fn normalize_ids(text: &str) -> String {
    const PREFIX: &str = "gemini_synth_";
    let mut seen: Vec<String> = Vec::new();
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find(PREFIX) {
        out.push_str(&rest[..at]);
        let id: String = rest[at..].chars().take(PREFIX.len() + 32).collect();
        let number = match seen.iter().position(|known| *known == id) {
            Some(number) => number,
            None => {
                seen.push(id.clone());
                seen.len() - 1
            }
        };
        out.push_str(&format!("<synth {number}>"));
        rest = &rest[at + id.len()..];
    }
    out.push_str(rest);
    out
}

fn response_with(
    body: &Value,
    shadow: Option<GeminiShadowSink>,
    engine: CodecEngine,
) -> Result<String, String> {
    let hints = tool_calls::extract_anthropic_tool_schema_hints(&json!({
        "tools": [{
            "name": "Skill",
            "input_schema": {
                "type": "object",
                "properties": {"skill": {"type": "string"}},
                "required": ["skill"]
            }
        }]
    }));
    gemini_to_anthropic_response(body.clone(), shadow, Some(&hints), engine)
        .map(|value| normalize_ids(&serde_json::to_string_pretty(&value).unwrap()))
        .map_err(|error| error.to_string())
}

fn stream_with(
    chunks: &[&str],
    transport_error: Option<&str>,
    shadow: Option<GeminiShadowSink>,
    engine: CodecEngine,
) -> String {
    let mut items: Vec<Result<Bytes, std::io::Error>> = chunks
        .iter()
        .map(|chunk| Ok(Bytes::from(chunk.to_string())))
        .collect();
    if let Some(message) = transport_error {
        items.push(Err(std::io::Error::other(message.to_string())));
    }
    let output: Vec<_> = futures::executor::block_on(
        gemini_to_anthropic_sse(futures::stream::iter(items), shadow, None, engine).collect(),
    );
    let text: String = output
        .into_iter()
        .map(|item| match item {
            Ok(bytes) => String::from_utf8(bytes.to_vec()).expect("utf-8 output"),
            Err(error) => format!("<err {error}>"),
        })
        .collect();
    normalize_ids(&text)
}

/// 两种引擎的客户端 SSE 字节一致（含上游末尾追加传输错误的情形）
#[track_caller]
fn assert_stream_parity(chunks: &[&str]) -> String {
    for transport_error in [None, Some("connection reset")] {
        assert_eq!(
            stream_with(chunks, transport_error, None, CodecEngine::Ir),
            stream_with(chunks, transport_error, None, CodecEngine::Legacy),
            "transport_error={transport_error:?}"
        );
    }
    stream_with(chunks, None, None, CodecEngine::Ir)
}

fn sse(chunk: Value) -> String {
    format!("data: {chunk}\n\n")
}

// ---- 请求 ----

#[test]
fn encode_request_maps_ir_fields_in_legacy_order() {
    let request = ChatRequest {
        model: "gemini-2.5-pro".into(),
        system: vec![Part::text("a"), Part::text(""), Part::text("b")],
        messages: vec![
            Message::new(Role::User, vec![Part::text("hi")]),
            Message::new(
                Role::Assistant,
                vec![
                    Part::Reasoning {
                        text: "plan".into(),
                        signature: Some("sig".into()),
                    },
                    Part::ToolCall {
                        id: "call_1".into(),
                        name: "search".into(),
                        arguments: r#"{"q":"x"}"#.into(),
                    },
                ],
            ),
            Message::new(
                Role::User,
                vec![Part::ToolResult {
                    call_id: "call_1".into(),
                    content: vec![Part::text("r1"), Part::text("r2")],
                    is_error: false,
                    plain: false,
                }],
            ),
        ],
        tools: vec![ToolDef {
            name: "search".into(),
            description: Some("Search".into()),
            input_schema: json!({"type": "object", "properties": {"q": {"type": "string"}}}),
        }],
        tool_choice: Some(ToolChoice::Tool {
            name: "search".into(),
        }),
        max_tokens: Some(64),
        temperature: Some(1.0),
        top_p: Some(0.5),
        top_k: Some(3),
        stop: vec!["END".into()],
        ..ChatRequest::default()
    };
    let body = GeminiCodec::new().encode_request(&request).unwrap();
    let keys: Vec<&str> = body
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        [
            "systemInstruction",
            "contents",
            "generationConfig",
            "tools",
            "toolConfig"
        ]
    );
    assert_eq!(
        body["systemInstruction"],
        json!({"parts": [{"text": "a\n\nb"}]})
    );
    assert_eq!(
        body["contents"],
        json!([
            {"role": "user", "parts": [{"text": "hi"}]},
            {"role": "model", "parts": [
                {"functionCall": {"name": "search", "args": {"q": "x"}, "id": "call_1"}}
            ]},
            {"role": "user", "parts": [
                {"functionResponse": {"name": "search", "response": {"content": "r1\nr2"}, "id": "call_1"}}
            ]}
        ])
    );
    // top_k 与 thinking 不转发；整数值的浮点按整数输出
    assert_eq!(
        body["generationConfig"],
        json!({"maxOutputTokens": 64, "temperature": 1, "topP": 0.5, "stopSequences": ["END"]})
    );
    assert_eq!(
        body["toolConfig"],
        json!({"functionCallingConfig": {"mode": "ANY", "allowedFunctionNames": ["search"]}})
    );
}

#[test]
fn request_parity_covers_content_forms_and_tool_choices() {
    for tool_choice in [
        json!("auto"),
        json!("none"),
        json!({"type": "auto"}),
        json!({"type": "none"}),
        json!({"type": "any"}),
        json!({"type": "tool", "name": "search"}),
    ] {
        assert_request_parity(
            json!({
                "model": "gemini-2.5-pro",
                "max_tokens": 100,
                "temperature": 0.2,
                "top_p": 0.9,
                "stop_sequences": ["x"],
                "system": [{"type": "text", "text": "S1"}, {"type": "text", "text": ""}],
                "messages": [
                    {"role": "system", "content": "S2"},
                    {"role": "user", "content": [
                        {"type": "text", "text": "look"},
                        {"type": "image", "source": {"type": "base64", "media_type": "image/jpeg", "data": "AAA"}},
                        {"type": "image", "source": {"type": "base64", "data": "BBB"}},
                        {"type": "document", "source": {"type": "base64", "media_type": "application/pdf", "data": "CCC"}},
                        {"type": "document", "source": {"type": "base64", "data": "DDD"}}
                    ]},
                    {"role": "assistant", "content": [
                        {"type": "thinking", "thinking": "hmm", "signature": "s"},
                        {"type": "redacted_thinking", "data": "r"},
                        {"type": "text", "text": "calling"},
                        {"type": "tool_use", "id": "toolu_1", "name": "search", "input": {"q": "x"}},
                        {"type": "tool_use", "id": "gemini_synth_00000000000000000000000000000000", "name": "search", "input": {}}
                    ]},
                    {"role": "user", "content": [
                        {"type": "tool_result", "tool_use_id": "toolu_1", "content": "plain"},
                        {"type": "tool_result", "tool_use_id": "gemini_synth_00000000000000000000000000000000",
                         "content": [{"type": "text", "text": "a"}, {"type": "text", "text": "b"}]},
                        {"type": "tool_result", "tool_use_id": "toolu_1"}
                    ]},
                    {"role": "user", "content": ""},
                    {"role": "assistant"}
                ],
                "tools": [
                    {"name": "search", "description": "Search", "input_schema": {"type": "object", "properties": {"q": {"type": "string"}}}},
                    {"type": "BatchTool", "name": "batch"}
                ],
                "tool_choice": tool_choice
            }),
            None,
        )
        .expect("convert request");
    }
}

#[test]
fn request_parity_covers_error_branches() {
    let cases = [
        (
            json!({"messages": [{"role": "user", "content": [
                {"type": "image", "source": {"type": "url", "url": "https://x/y.png"}}
            ]}]}),
            "Gemini Native only supports base64 image sources, got `url`",
        ),
        (
            json!({"messages": [{"role": "user", "content": [
                {"type": "document", "source": {"type": "url", "url": "https://x/y.pdf"}}
            ]}]}),
            "Gemini Native only supports base64 document sources, got `url`",
        ),
        (
            json!({"messages": [{"role": "user", "content": [
                {"type": "tool_use", "id": "t", "name": "f", "input": {}}
            ]}]}),
            "tool_use blocks are only valid in assistant messages",
        ),
        (
            json!({"messages": [{"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "missing", "content": "x"}
            ]}]}),
            "Unable to resolve Gemini functionResponse.name for tool_use_id `missing`",
        ),
    ];
    for (body, message) in cases {
        let error = assert_request_parity(body, None).unwrap_err();
        assert!(error.contains(message), "{error}");
    }
}

#[test]
fn tool_result_resolves_name_from_tool_use_in_the_same_message() {
    let output = assert_request_parity(
        json!({"messages": [{"role": "assistant", "content": [
            {"type": "tool_result", "tool_use_id": "late", "content": "x"},
            {"type": "tool_use", "id": "late", "name": "lookup", "input": {}}
        ]}]}),
        None,
    )
    .unwrap();
    assert!(output.contains("\"name\": \"lookup\""), "{output}");
}

/// shadow 回放：按 id 匹配到的轮次原样回放原始 parts（去掉合成 id），
/// 按位置对齐其余轮次，并补回 functionCall 的 thoughtSignature
#[test]
fn request_replays_shadow_turns_like_legacy() {
    let store = GeminiShadowStore::new();
    store.record_assistant_turn(
        PROVIDER,
        SESSION,
        json!({"role": "model", "parts": [
            {"text": "thinking out loud", "thoughtSignature": "text-sig"},
            {"functionCall": {"id": "real_1", "name": "server_a:search", "args": {"q": "a"}}, "thoughtSignature": "sig-1"}
        ]}),
        vec![GeminiToolCallMeta::new(
            Some("real_1"),
            "server_a:search",
            json!({"q": "a"}),
            Some("sig-1"),
        )],
    );
    store.record_assistant_turn(
        PROVIDER,
        SESSION,
        json!({"parts": [
            {"functionCall": {"id": "gemini_synth_11111111111111111111111111111111", "name": "read", "args": {}}}
        ]}),
        vec![GeminiToolCallMeta::new(
            Some("gemini_synth_11111111111111111111111111111111"),
            "read",
            json!({}),
            None::<String>,
        )],
    );
    store.record_assistant_turn(
        PROVIDER,
        SESSION,
        json!({"parts": [{"text": "done", "thoughtSignature": "final-sig"}]}),
        Vec::new(),
    );

    let body = json!({
        "messages": [
            {"role": "user", "content": "go"},
            {"role": "assistant", "content": [
                {"type": "tool_use", "id": "real_1", "name": "server_a:search", "input": {"q": "a"}}
            ]},
            {"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "real_1", "content": "found"}
            ]},
            {"role": "assistant", "content": [
                {"type": "tool_use", "id": "gemini_synth_11111111111111111111111111111111", "name": "read", "input": {}}
            ]},
            {"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "gemini_synth_11111111111111111111111111111111", "content": "file"}
            ]},
            {"role": "assistant", "content": "done"},
            {"role": "user", "content": "thanks"}
        ]
    });
    let output = assert_request_parity(body, Some(&store)).unwrap();
    let value: Value = serde_json::from_str(&output).unwrap();
    assert_eq!(
        value["contents"][1]["parts"][1],
        json!({"functionCall": {"id": "real_1", "name": "server_a:search", "args": {"q": "a"}}, "thoughtSignature": "sig-1"})
    );
    assert_eq!(
        value["contents"][3]["parts"][0],
        json!({"functionCall": {"name": "read", "args": {}}})
    );
    assert_eq!(
        value["contents"][4]["parts"][0],
        json!({"functionResponse": {"name": "read", "response": {"content": "file"}}})
    );
    assert_eq!(
        value["contents"][5]["parts"][0],
        json!({"text": "done", "thoughtSignature": "final-sig"})
    );
}

/// 已被前一条消息占用的 shadow 轮次不再回放，但其 functionCall 的
/// thoughtSignature 仍补回到按客户端内容重建的 functionCall 上
#[test]
fn request_reattaches_thought_signature_without_replaying_turn() {
    let store = GeminiShadowStore::new();
    store.record_assistant_turn(
        PROVIDER,
        SESSION,
        json!({"parts": [{"text": "only text"}]}),
        vec![GeminiToolCallMeta::new(
            Some("call_x"),
            "f",
            json!({}),
            Some("sig-x"),
        )],
    );
    let body = json!({"messages": [
        {"role": "assistant", "content": [{"type": "tool_use", "id": "call_x", "name": "f", "input": {}}]},
        {"role": "assistant", "content": [{"type": "tool_use", "id": "call_x", "name": "f", "input": {"a": 1}}]},
        {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "call_x", "content": "ok"}]}
    ]});
    let output = assert_request_parity(body, Some(&store)).unwrap();
    let value: Value = serde_json::from_str(&output).unwrap();
    assert_eq!(
        value["contents"][0]["parts"],
        json!([{"text": "only text"}])
    );
    assert_eq!(
        value["contents"][1]["parts"][0],
        json!({"functionCall": {"name": "f", "args": {"a": 1}, "id": "call_x", "thoughtSignature": "sig-x"}})
    );
}

/// IR 引擎有意与旧转换器不同的地方（均为默认值等价或不规范的输入）
#[test]
fn request_intentional_differences_from_legacy() {
    let ir = |body: Value| {
        anthropic_to_gemini_request(body, None, None, None, CodecEngine::Ir)
            .map_err(|e| e.to_string())
    };
    let legacy = |body: Value| {
        anthropic_to_gemini_request(body, None, None, None, CodecEngine::Legacy)
            .map_err(|e| e.to_string())
    };

    // 工具参数经 IR 后按键排序（canonical JSON）
    let body = json!({"messages": [{"role": "assistant", "content": [
        {"type": "tool_use", "id": "c", "name": "f", "input": {"z": 1, "a": 2}}
    ]}]});
    let args = &ir(body.clone()).unwrap()["contents"][0]["parts"][0]["functionCall"]["args"];
    assert_eq!(serde_json::to_string(args).unwrap(), r#"{"a":2,"z":1}"#);
    assert_eq!(
        serde_json::to_string(
            &legacy(body).unwrap()["contents"][0]["parts"][0]["functionCall"]["args"]
        )
        .unwrap(),
        r#"{"z":1,"a":2}"#
    );

    // 空的 stop_sequences 不再转发
    let body = json!({"stop_sequences": [], "messages": []});
    assert!(ir(body.clone()).unwrap().get("generationConfig").is_none());
    assert_eq!(
        legacy(body).unwrap()["generationConfig"]["stopSequences"],
        json!([])
    );

    // 空数组形式的 tool_result 内容与缺省内容一样发空串
    let body = json!({"messages": [
        {"role": "assistant", "content": [{"type": "tool_use", "id": "c", "name": "f", "input": {}}]},
        {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "c", "content": []}]}
    ]});
    let response =
        |value: Value| value["contents"][1]["parts"][0]["functionResponse"]["response"].clone();
    assert_eq!(response(ir(body.clone()).unwrap()), json!({"content": ""}));
    assert_eq!(response(legacy(body).unwrap()), json!({"content": []}));

    // 字符串形式的 `any` / `required` 与对象形式同样映射为 ANY，未知取值不转发
    let body = json!({"messages": [], "tool_choice": "any"});
    assert_eq!(
        ir(body.clone()).unwrap()["toolConfig"],
        json!({"functionCallingConfig": {"mode": "ANY"}})
    );
    assert!(legacy(body).is_err());
    let body = json!({"messages": [], "tool_choice": {"type": "bogus"}});
    assert!(ir(body.clone()).unwrap().get("toolConfig").is_none());
    assert!(legacy(body).is_err());
}

// ---- 非流式回复 ----

#[test]
fn response_parity_covers_text_tools_thoughts_and_blocking() {
    let cases = [
        json!({
            "responseId": "r1",
            "modelVersion": "gemini-2.5-pro",
            "candidates": [{"finishReason": "STOP", "content": {"role": "model", "parts": [
                {"text": "hidden", "thought": true},
                {"text": ""},
                {"text": "visible"},
                {"functionCall": {"name": "Skill", "args": {"name": "pdf"}}},
                {"functionCall": {"id": "", "name": "b", "args": {"z": 1, "a": 2}}},
                {"functionCall": {"id": "real", "name": "c"}}
            ]}}],
            "usageMetadata": {"promptTokenCount": 10, "cachedContentTokenCount": 4, "totalTokenCount": 15, "thoughtsTokenCount": 2}
        }),
        json!({"candidates": [{"finishReason": "MAX_TOKENS", "content": {"parts": [{"text": "cut"}]}}]}),
        json!({"candidates": [{"finishReason": "SAFETY"}]}),
        json!({"responseId": "r2", "promptFeedback": {"blockReason": "OTHER"}, "usageMetadata": {"promptTokenCount": 3}}),
        json!({"candidates": []}),
        json!({}),
    ];
    for body in cases {
        assert_eq!(
            response_with(&body, None, CodecEngine::Ir),
            response_with(&body, None, CodecEngine::Legacy),
            "{body}"
        );
    }
    let error = response_with(&json!({}), None, CodecEngine::Ir).unwrap_err();
    assert!(
        error.contains("No candidates in Gemini response"),
        "{error}"
    );
}

/// 非流式回复记入 shadow 的轮次与客户端看到的 tool_use 使用同一个合成 id
#[test]
fn response_records_shadow_turn_with_client_visible_ids() {
    let body = json!({
        "candidates": [{"content": {"role": "model", "parts": [
            {"functionCall": {"name": "Skill", "args": {"name": "pdf"}}, "thoughtSignature": "sig"}
        ]}}]
    });
    for engine in ENGINES {
        let store = Arc::new(GeminiShadowStore::new());
        let response =
            gemini_to_anthropic_response(body.clone(), sink(&store), None, engine).unwrap();
        let client_id = response["content"][0]["id"].as_str().unwrap().to_string();
        assert!(client_id.starts_with("gemini_synth_"), "{engine:?}");
        let content = store.latest_assistant_content(PROVIDER, SESSION).unwrap();
        assert_eq!(content["role"], "model");
        assert_eq!(
            content["parts"][0]["functionCall"]["id"],
            client_id.as_str()
        );
        let calls = store.latest_tool_calls(PROVIDER, SESSION).unwrap();
        assert_eq!(
            calls[0].id.as_deref(),
            Some(client_id.as_str()),
            "{engine:?}"
        );
        assert_eq!(calls[0].thought_signature.as_deref(), Some("sig"));
    }
    // 被拦截的回复不记录
    for engine in ENGINES {
        let store = Arc::new(GeminiShadowStore::new());
        let blocked = json!({"promptFeedback": {"blockReason": "SAFETY"}});
        gemini_to_anthropic_response(blocked, sink(&store), None, engine).unwrap();
        assert_eq!(store.session_count(), 0, "{engine:?}");
    }
}

#[test]
fn decode_response_preserves_tool_argument_key_order() {
    let body = json!({"candidates": [{"content": {"parts": [
        {"functionCall": {"id": "c", "name": "f", "args": {"z": 1, "a": 2}}}
    ]}}]});
    let response = GeminiCodec::new().decode_response(&body).unwrap();
    assert_eq!(
        response.parts,
        vec![Part::ToolCall {
            id: "c".into(),
            name: "f".into(),
            arguments: r#"{"z":1,"a":2}"#.into(),
        }]
    );
    assert_eq!(response.stop_reason, Some(StopReason::ToolUse));
}

// ---- 流式 ----

fn decode_all(chunks: &[Value]) -> Vec<StreamEvent> {
    let mut decoder = GeminiEventDecoder::new(None, None);
    let mut events = Vec::new();
    for chunk in chunks {
        events.extend(decoder.decode("", &chunk.to_string()).unwrap());
    }
    events.extend(decoder.finish().unwrap());
    events
}

#[test]
fn stream_decoder_emits_text_live_and_tools_at_end() {
    let events = decode_all(&[
        json!({"responseId": "r", "modelVersion": "m", "candidates": [{"content": {"parts": [{"text": "He"}]}}],
               "usageMetadata": {"promptTokenCount": 5, "totalTokenCount": 6}}),
        json!({"candidates": [{"finishReason": "STOP", "content": {"parts": [
            {"text": "llo"},
            {"functionCall": {"id": "c1", "name": "f", "args": {"b": 1, "a": 2}}}
        ]}}], "usageMetadata": {"promptTokenCount": 5, "totalTokenCount": 9}}),
    ]);
    let usage =
        |total| UsageIR::from_gemini(&json!({"promptTokenCount": 5, "totalTokenCount": total}));
    assert_eq!(
        events,
        vec![
            StreamEvent::MessageStart {
                id: "r".into(),
                model: "m".into(),
                usage: Some(usage(6)),
            },
            StreamEvent::PartStart {
                index: 0,
                part: Part::text(""),
            },
            StreamEvent::PartDelta {
                index: 0,
                delta: PartDelta::Text("He".into()),
            },
            StreamEvent::PartDelta {
                index: 0,
                delta: PartDelta::Text("llo".into()),
            },
            StreamEvent::PartStop { index: 0 },
            StreamEvent::PartStart {
                index: 1,
                part: Part::ToolCall {
                    id: "c1".into(),
                    name: "f".into(),
                    arguments: String::new(),
                },
            },
            StreamEvent::PartDelta {
                index: 1,
                delta: PartDelta::ToolArguments(r#"{"b":1,"a":2}"#.into()),
            },
            StreamEvent::PartStop { index: 1 },
            StreamEvent::Usage(usage(9)),
            StreamEvent::MessageStop {
                stop_reason: Some(StopReason::ToolUse),
            },
        ]
    );
}

/// 流式 + 聚合与非流式解码得到同样的回复
#[test]
fn aggregated_stream_matches_non_stream_decode() {
    let chunk = json!({
        "responseId": "r",
        "modelVersion": "m",
        "candidates": [{"finishReason": "STOP", "content": {"parts": [
            {"text": "hi"},
            {"functionCall": {"id": "c1", "name": "f", "args": {"a": 1}}}
        ]}}],
        "usageMetadata": {"promptTokenCount": 5, "totalTokenCount": 9}
    });
    let data = chunk.to_string();
    let codec = GeminiCodec::new();
    let aggregated = codec
        .aggregate_stream(&mut std::iter::once(("", data.as_str())))
        .unwrap();
    assert_eq!(aggregated, codec.decode_response(&chunk).unwrap());
}

/// `[DONE]` 之后的块一律忽略（旧转换器只忽略同一个网络块内其后的事件，
/// 结果取决于分块方式）
#[test]
fn stream_decoder_ignores_events_after_done() {
    let text = |text: &str| json!({"candidates": [{"content": {"parts": [{"text": text}]}}]});
    let mut decoder = GeminiEventDecoder::new(None, None);
    decoder.decode("", &text("a").to_string()).unwrap();
    assert!(decoder.decode("", " [DONE] ").unwrap().is_empty());
    assert!(decoder
        .decode("", &text("b").to_string())
        .unwrap()
        .is_empty());
}

#[test]
fn stream_parity_covers_blocking_empty_and_garbage() {
    let blocked = sse(json!({"responseId": "rb", "promptFeedback": {"blockReason": "SAFETY"}}));
    let output = assert_stream_parity(&[&blocked]);
    assert!(output.contains("Request blocked by Gemini safety filters: SAFETY"));
    assert!(output.contains("\"stop_reason\":\"refusal\""));

    // 没有任何可解析的块：仍输出完整的空消息
    let output = assert_stream_parity(&["data: not json\n\n", ": comment\n\n"]);
    assert!(output.starts_with("event: message_start"), "{output}");
    assert!(
        output.ends_with("event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n"),
        "{output}"
    );

    // 被拦截但已有文本：不再补说明文本，仍以 refusal 结束
    let text = sse(json!({"candidates": [{"content": {"parts": [{"text": "partial"}]}}]}));
    let output = assert_stream_parity(&[&text, &blocked]);
    assert!(!output.contains("Request blocked"), "{output}");
}

#[test]
fn stream_parity_covers_tools_signatures_and_transport_errors() {
    let first = sse(json!({
        "responseId": "r",
        "modelVersion": "gemini-2.5-pro",
        "candidates": [{"content": {"parts": [
            {"text": "Let me check", "thoughtSignature": "t-sig"},
            {"functionCall": {"name": "search", "args": {"q": "a"}}, "thoughtSignature": "f-sig"}
        ]}}],
        "usageMetadata": {"promptTokenCount": 8, "cachedContentTokenCount": 3, "totalTokenCount": 10}
    }));
    let second = sse(json!({
        "candidates": [{"finishReason": "STOP", "content": {"parts": [
            {"text": "Let me check"},
            {"functionCall": {"id": "real", "name": "search", "args": {"q": "a"}}},
            {"functionCall": {"name": "search", "args": {"q": "b"}}}
        ]}}],
        "usageMetadata": {"promptTokenCount": 8, "cachedContentTokenCount": 3, "totalTokenCount": 20}
    }));
    let output = assert_stream_parity(&[&first, &second]);
    assert!(output.contains("\"id\":\"real\""), "{output}");
    // message_start 带首块的完整 usage（含输出 token）
    assert!(
        output.contains(
            "\"usage\":{\"input_tokens\":5,\"output_tokens\":2,\"cache_read_input_tokens\":3}"
        ),
        "{output}"
    );
    // 传输错误：以 Err 项结束，不补 message_stop
    let errored = stream_with(&[&first], Some("reset"), None, CodecEngine::Ir);
    assert!(errored.ends_with("<err reset>"), "{errored}");
    assert!(!errored.contains("message_stop"), "{errored}");
}

/// 流式回复正常结束时记录 shadow，两种引擎记录的内容一致；传输错误时不记录
#[test]
fn stream_records_shadow_turn_like_legacy() {
    let chunks = [
        sse(json!({"candidates": [{"content": {"parts": [
            {"text": "Hi", "thoughtSignature": "t-sig"},
            {"functionCall": {"name": "f", "args": {}}, "thoughtSignature": "f-sig"}
        ]}}]})),
        sse(
            json!({"candidates": [{"finishReason": "STOP", "content": {"parts": [
                {"text": "Hi there"},
                {"functionCall": {"name": "f", "args": {"x": 1}}}
            ]}}]}),
        ),
    ];
    let chunks: Vec<&str> = chunks.iter().map(String::as_str).collect();
    let mut recorded = Vec::new();
    for engine in ENGINES {
        let store = Arc::new(GeminiShadowStore::new());
        stream_with(&chunks, None, sink(&store), engine);
        let content = store.latest_assistant_content(PROVIDER, SESSION).unwrap();
        let calls = store.latest_tool_calls(PROVIDER, SESSION).unwrap();
        assert_eq!(calls.len(), 1, "{engine:?}");
        assert_eq!(calls[0].thought_signature.as_deref(), Some("f-sig"));
        recorded.push(normalize_ids(&content.to_string()));

        let store = Arc::new(GeminiShadowStore::new());
        stream_with(&chunks, Some("reset"), sink(&store), engine);
        assert_eq!(store.session_count(), 0, "{engine:?}");
    }
    assert_eq!(recorded[0], recorded[1]);
    assert!(
        recorded[0].contains("\"text\":\"Hi there\""),
        "{}",
        recorded[0]
    );
    assert!(
        recorded[0].contains("\"thoughtSignature\":\"t-sig\""),
        "{}",
        recorded[0]
    );
}

#[test]
fn client_side_directions_are_unsupported() {
    let codec = GeminiCodec::new();
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
    assert!(GeminiShadowSink::new(None, Some("p".into()), Some("s".into())).is_none());
}
