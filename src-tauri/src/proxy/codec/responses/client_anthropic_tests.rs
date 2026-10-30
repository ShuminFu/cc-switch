//! OpenAI Responses（Codex）← Anthropic 桥接：旧转换器与 IR 编解码的差分测试
//!
//! golden 语料之外再用手写输入覆盖旧转换器的各条分支，断言两种引擎输出逐字节相同；
//! 有意的差异集中在文末，逐条断言其行为。

use bytes::Bytes;
use futures::StreamExt;
use serde_json::{json, Value};

use super::reasoning::encode_anthropic_thinking_block;
use super::tools::build_codex_tool_context_from_request;
use crate::proxy::codec::bridge::{
    anthropic_message_to_responses_sse, anthropic_to_responses_response,
    anthropic_to_responses_sse, responses_to_anthropic_request,
};
use crate::proxy::codec::CodecEngine;

fn request(body: &Value, engine: CodecEngine) -> String {
    match responses_to_anthropic_request(body.clone(), 8192, engine) {
        Ok(value) => serde_json::to_string_pretty(&value).unwrap(),
        Err(error) => format!("<err {error}>"),
    }
}

#[track_caller]
fn assert_request_parity(body: Value) {
    assert_eq!(
        request(&body, CodecEngine::Ir),
        request(&body, CodecEngine::Legacy)
    );
}

fn tools_request() -> Value {
    json!({
        "tools": [
            {"type": "function", "name": "exec", "description": "Run", "strict": true, "parameters": {"type": "object", "properties": {"cmd": {"type": "string"}}}},
            {"type": "function", "name": "nop", "parameters": null},
            {"type": "custom", "name": "apply_patch", "description": "Patch"},
            {"type": "tool_search"},
            {"type": "namespace", "name": "mcp_files", "tools": [{"type": "function", "name": "read", "parameters": {"type": "string"}}]},
            {"type": "web_search"}
        ]
    })
}

fn response(body: &Value, context: &Value, engine: CodecEngine) -> String {
    let context = build_codex_tool_context_from_request(context);
    match anthropic_to_responses_response(body.clone(), &context, engine) {
        Ok(value) => serde_json::to_string_pretty(&value).unwrap(),
        Err(error) => format!("<err {error}>"),
    }
}

#[track_caller]
fn assert_response_parity(body: Value) {
    for context in [json!({}), tools_request()] {
        assert_eq!(
            response(&body, &context, CodecEngine::Ir),
            response(&body, &context, CodecEngine::Legacy)
        );
        let context_value = build_codex_tool_context_from_request(&context);
        let frames = |engine| -> String {
            anthropic_message_to_responses_sse(&body, context_value.clone(), engine)
                .into_iter()
                .map(|frame| String::from_utf8(frame.to_vec()).unwrap())
                .collect()
        };
        assert_eq!(frames(CodecEngine::Ir), frames(CodecEngine::Legacy));
    }
}

fn stream(
    chunks: &[&[u8]],
    transport_error: Option<&str>,
    context: &Value,
    engine: CodecEngine,
) -> String {
    let mut items: Vec<Result<Bytes, std::io::Error>> = chunks
        .iter()
        .map(|chunk| Ok(Bytes::copy_from_slice(chunk)))
        .collect();
    if let Some(message) = transport_error {
        items.push(Err(std::io::Error::other(message.to_string())));
    }
    let context = build_codex_tool_context_from_request(context);
    let output: Vec<_> = futures::executor::block_on(
        anthropic_to_responses_sse(futures::stream::iter(items), context, engine).collect(),
    );
    output
        .into_iter()
        .map(|item| match item {
            Ok(bytes) => String::from_utf8(bytes.to_vec()).expect("utf-8 output"),
            Err(error) => format!("<err {error}>"),
        })
        .collect()
}

/// 传输错误追加在上游末尾时同样比较；每个字节边界切分也必须得到同样的输出
#[track_caller]
fn assert_stream_parity(chunks: &[&str]) {
    let chunks: Vec<&[u8]> = chunks.iter().map(|chunk| chunk.as_bytes()).collect();
    for context in [json!({}), tools_request()] {
        for transport_error in [None, Some("connection reset")] {
            assert_eq!(
                stream(&chunks, transport_error, &context, CodecEngine::Ir),
                stream(&chunks, transport_error, &context, CodecEngine::Legacy),
                "transport_error={transport_error:?}"
            );
        }
    }
    let joined: Vec<u8> = chunks.concat();
    if joined.len() <= 1536 {
        for split in 1..joined.len() {
            let parts = [&joined[..split], &joined[split..]];
            assert_eq!(
                stream(&parts, None, &json!({}), CodecEngine::Ir),
                stream(&parts, None, &json!({}), CodecEngine::Legacy),
                "split at {split}"
            );
        }
    }
}

fn event(name: &str, data: Value) -> String {
    format!("event: {name}\ndata: {data}\n\n")
}

fn message_start(usage: Value) -> String {
    event(
        "message_start",
        json!({"type": "message_start", "message": {"id": "msg_1", "model": "claude-sonnet-4-5", "usage": usage}}),
    )
}

fn block_start(index: u64, block: Value) -> String {
    event(
        "content_block_start",
        json!({"type": "content_block_start", "index": index, "content_block": block}),
    )
}

fn block_delta(index: u64, delta: Value) -> String {
    event(
        "content_block_delta",
        json!({"type": "content_block_delta", "index": index, "delta": delta}),
    )
}

fn block_stop(index: u64) -> String {
    event(
        "content_block_stop",
        json!({"type": "content_block_stop", "index": index}),
    )
}

fn message_delta(stop_reason: &str, usage: Value) -> String {
    event(
        "message_delta",
        json!({"type": "message_delta", "delta": {"stop_reason": stop_reason}, "usage": usage}),
    )
}

fn message_stop() -> String {
    event("message_stop", json!({"type": "message_stop"}))
}

fn signed_thinking_envelope() -> String {
    encode_anthropic_thinking_block(
        &json!({"type": "thinking", "thinking": "plan", "signature": "sig"}),
    )
    .unwrap()
}

// ---- 请求 ----

#[test]
fn request_system_messages_and_media_match_legacy() {
    assert_request_parity(json!({
        "model": "claude-sonnet-4-5",
        "instructions": "  You are Codex.  ",
        "input": [
            {"role": "developer", "content": "  dev rules "},
            {"role": "system", "content": [{"type": "input_text", "text": "a"}, {"type": "text", "text": " "}, {"type": "output_text", "text": "b"}]},
            {"type": "message", "role": "user", "content": [
                {"type": "input_text", "text": "look"},
                {"type": "input_text", "text": "   "},
                {"type": "input_image", "image_url": "data:image/jpeg;base64,AAA"},
                {"type": "input_image", "image_url": {"url": "https://example.com/a.png"}},
                {"type": "input_image", "image_url": "file:///x"},
                {"type": "input_file", "filename": "a.pdf", "file_data": "data:application/pdf;base64,JVBE"},
                {"type": "input_file", "file_url": "https://example.com/b.pdf"},
                {"type": "input_file", "file_id": "file_1"},
                {"type": "input_audio", "input_audio": {"data": "x", "format": "wav"}}
            ]},
            {"type": "input_text", "text": "loose text"},
            {"type": "input_image", "image_url": "https://example.com/c.png"},
            {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "ok"}, {"type": "refusal", "refusal": "no"}]},
            {"role": "user", "content": "next"},
            {"role": "assistant", "content": "prefill   "}
        ]
    }));
    assert_request_parity(json!({"model": "m", "input": "just text"}));
    assert_request_parity(json!({"model": "m", "input": "   "}));
    assert_request_parity(json!({"model": "m", "input": [{"type": "reasoning", "summary": []}]}));
    assert_request_parity(
        json!({"model": "m", "input": [{"role": "assistant", "content": "   "}]}),
    );
}

#[test]
fn request_tool_history_matches_legacy() {
    let mut body = tools_request();
    body["model"] = json!("claude-sonnet-4-5");
    body["input"] = json!([
        {"role": "user", "content": "go"},
        {"type": "function_call", "call_id": "c1", "name": "exec", "arguments": "{\"cmd\":\"ls\",\"a\":1}"},
        {"type": "function_call", "call_id": "c2", "name": "Read", "arguments": "{\"file_path\":\"x\",\"pages\":\"\"}"},
        {"type": "function_call", "call_id": "c3", "name": "read", "namespace": "mcp_files", "arguments": ""},
        {"type": "custom_tool_call", "call_id": "c4", "name": "apply_patch", "input": "*** Begin"},
        {"type": "tool_search_call", "call_id": "c5", "arguments": {"query": "x"}},
        {"type": "function_call_output", "call_id": "c1", "output": "done"},
        {"type": "input_text", "text": "between"},
        {"type": "function_call_output", "call_id": "c2", "output": [
            {"type": "input_text", "text": "[cc-switch:tool-result-error]"},
            {"type": "input_text", "text": "failed"},
            {"type": "input_image", "image_url": "data:image/png;base64,IMG"},
            {"type": "input_image", "image_url": "ftp://bad"},
            {"type": "input_file", "file_url": "https://example.com/x.pdf", "filename": "x.pdf"},
            {"type": "input_file", "file_id": "f"},
            {"type": "other", "x": 1}
        ]},
        {"type": "function_call_output", "call_id": "c3", "output": {"k": "v"}},
        {"type": "custom_tool_call_output", "call_id": "c4", "output": "patched"},
        {"type": "tool_search_output", "call_id": "c5", "tools": []},
        {"type": "function_call", "call_id": "c6", "name": "exec", "arguments": "{}", "status": "incomplete"},
        {"type": "function_call_output", "call_id": "c6", "output": "orphan"},
        {"role": "user", "content": "continue"}
    ]);
    for tool_choice in [
        json!("auto"),
        json!("required"),
        json!("none"),
        json!("bogus"),
        json!({"type": "function", "name": "exec"}),
        json!({"type": "function", "name": "read", "namespace": "mcp_files"}),
        json!({"type": "custom", "name": "apply_patch"}),
        json!({"type": "tool_search"}),
        json!({"type": "allowed_tools", "tools": []}),
        json!(3),
    ] {
        let mut body = body.clone();
        body["tool_choice"] = tool_choice;
        assert_request_parity(body.clone());
        body["parallel_tool_calls"] = json!(false);
        assert_request_parity(body);
    }
    // 工具只有托管工具时 tool_choice 一并丢弃
    assert_request_parity(json!({
        "model": "m",
        "tools": [{"type": "web_search"}],
        "tool_choice": "required",
        "parallel_tool_calls": false,
        "input": "hi"
    }));
    // 参数不是 JSON 对象
    for arguments in ["{broken", "[1]", "\"s\""] {
        assert_request_parity(json!({
            "model": "m",
            "input": [
                {"role": "user", "content": "go"},
                {"type": "function_call", "call_id": "c", "name": "f", "arguments": arguments},
                {"type": "function_call_output", "call_id": "c", "output": "x"}
            ]
        }));
    }
}

#[test]
fn request_thinking_matrix_matches_legacy() {
    let envelope = signed_thinking_envelope();
    let redacted =
        encode_anthropic_thinking_block(&json!({"type": "redacted_thinking", "data": "opaque"}))
            .unwrap();
    let histories = [
        json!([{"role": "user", "content": "hi"}]),
        // 带签名 thinking 的工具续接
        json!([
            {"role": "user", "content": "hi"},
            {"type": "reasoning", "encrypted_content": envelope, "summary": []},
            {"type": "reasoning", "encrypted_content": redacted, "summary": []},
            {"type": "function_call", "call_id": "c1", "name": "exec", "arguments": "{}"},
            {"type": "function_call_output", "call_id": "c1", "output": "ok"}
        ]),
        // 没有签名 thinking 的工具续接
        json!([
            {"role": "user", "content": "hi"},
            {"type": "reasoning", "encrypted_content": "gAAAA-openai", "summary": []},
            {"type": "message", "role": "assistant", "content": "text first"},
            {"type": "reasoning", "encrypted_content": envelope, "summary": []},
            {"type": "function_call", "call_id": "c1", "name": "exec", "arguments": "{}"},
            {"type": "function_call_output", "call_id": "c1", "output": "ok"}
        ]),
        // 以助手开头、工具结果没有 id
        json!([
            {"role": "assistant", "content": "earlier"},
            {"type": "function_call", "call_id": "", "name": "exec", "arguments": "{}"},
            {"type": "function_call_output", "call_id": "", "output": "ok"}
        ]),
    ];
    let models = [
        "claude-sonnet-4-5",
        "claude-opus-4-6",
        "claude-sonnet-4-6",
        "claude-haiku-4-5",
        "claude-opus-4-7",
        "claude-fable-1",
        "claude-mythos-1",
        "gpt-4o",
    ];
    let efforts = [
        None,
        Some("minimal"),
        Some("low"),
        Some("medium"),
        Some("high"),
        Some("xhigh"),
        Some("max"),
        Some("none"),
        Some("bogus"),
    ];
    for history in &histories {
        for model in models {
            for effort in efforts {
                for max_output_tokens in [None, Some(1500), Some(32000)] {
                    let mut body = tools_request();
                    body["model"] = json!(model);
                    body["input"] = history.clone();
                    body["temperature"] = json!(0.2);
                    body["top_p"] = json!(0.9);
                    body["stream"] = json!(true);
                    if let Some(effort) = effort {
                        body["reasoning"] = json!({"effort": effort});
                    }
                    if let Some(max) = max_output_tokens {
                        body["max_output_tokens"] = json!(max);
                    }
                    assert_request_parity(body.clone());
                    body["tool_choice"] = json!("required");
                    assert_request_parity(body.clone());
                    body["tool_choice"] = json!({"type": "function", "name": "exec"});
                    body["parallel_tool_calls"] = json!(false);
                    assert_request_parity(body);
                }
            }
        }
    }
}

// ---- 非流式响应 ----

#[test]
fn response_variants_match_legacy() {
    assert_response_parity(json!({
        "id": "msg_1",
        "model": "claude",
        "content": [
            {"type": "thinking", "thinking": "plan", "signature": "sig"},
            {"type": "thinking", "thinking": "unsigned"},
            {"type": "redacted_thinking", "data": "opaque"},
            {"type": "text", "text": "a"},
            {"type": "text", "text": ""},
            {"type": "tool_use", "id": "t1", "name": "exec", "input": {"z": 1, "a": {"y": 2, "b": 3}}},
            {"type": "tool_use", "id": "t2", "name": "Read", "input": {"file_path": "x", "pages": ""}},
            {"type": "tool_use", "id": "t3", "name": "apply_patch", "input": {"input": "patch"}},
            {"type": "tool_use", "id": "t4", "name": "tool_search", "input": {"query": "q"}},
            {"type": "tool_use", "id": "t5", "name": "mcp_files__read", "input": {}},
            {"type": "tool_use", "id": "t6", "name": "nop"},
            {"type": "text", "text": "after"},
            {"type": "server_tool_use", "id": "s"}
        ],
        "stop_reason": "tool_use",
        "usage": {"input_tokens": 10, "output_tokens": 5, "cache_read_input_tokens": 3, "cache_creation_input_tokens": 2}
    }));
    for stop_reason in [
        json!("end_turn"),
        json!("max_tokens"),
        json!("refusal"),
        json!("model_context_window_exceeded"),
        json!("pause_turn"),
        json!("stop_sequence"),
        json!(null),
    ] {
        assert_response_parity(json!({
            "id": "resp_already",
            "content": [{"type": "text", "text": "x"}],
            "stop_reason": stop_reason
        }));
    }
    for body in [
        json!({"content": []}),
        json!({"content": [], "usage": "bad"}),
        json!({"type": "error", "error": {"type": "overloaded_error", "message": "busy"}}),
        json!({"error": "plain"}),
        json!({"error": {}}),
    ] {
        assert_response_parity(body);
    }
}

// ---- 流式 ----

#[test]
fn stream_text_thinking_and_usage_match_legacy() {
    assert_stream_parity(&[
        &message_start(json!({"input_tokens": 10, "cache_read_input_tokens": 4})),
        &block_start(
            0,
            json!({"type": "thinking", "thinking": "", "signature": ""}),
        ),
        &block_delta(0, json!({"type": "thinking_delta", "thinking": "plan"})),
        &block_delta(0, json!({"type": "signature_delta", "signature": "sig"})),
        &block_stop(0),
        &block_start(1, json!({"type": "thinking", "thinking": "no sig"})),
        &block_stop(1),
        &block_start(2, json!({"type": "redacted_thinking", "data": "opaque"})),
        &block_stop(2),
        &block_start(3, json!({"type": "text", "text": "pre"})),
        &block_delta(3, json!({"type": "text_delta", "text": "fix"})),
        &block_delta(3, json!({"type": "unknown_delta"})),
        &block_delta(9, json!({"type": "text_delta", "text": "ghost"})),
        &block_stop(3),
        &block_stop(3),
        &block_start(4, json!({"type": "server_tool_use", "id": "s"})),
        &message_delta(
            "end_turn",
            json!({"output_tokens": 7, "cache_creation_input_tokens": 2}),
        ),
        &message_stop(),
    ]);
    for stop_reason in [
        "max_tokens",
        "refusal",
        "model_context_window_exceeded",
        "pause_turn",
    ] {
        assert_stream_parity(&[
            &message_start(json!({})),
            &block_start(0, json!({"type": "text", "text": ""})),
            &block_delta(0, json!({"type": "text_delta", "text": "x"})),
            &message_delta(stop_reason, json!({"output_tokens": 1})),
            &message_stop(),
        ]);
    }
    // 没有 message_start、没有 event: 行
    assert_stream_parity(&[
        "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"hi\"}}\n\n",
        "event: ping\ndata: {\"type\":\"ping\"}\n\n",
        "data: [DONE]\n\n",
        "data: {\"type\":\"message_stop\"}\n\n",
        &message_start(json!({"input_tokens": 1})),
    ]);
}

#[test]
fn stream_tool_calls_match_legacy() {
    let deltas = |index: u64, parts: &[&str]| -> Vec<String> {
        parts
            .iter()
            .map(|part| {
                block_delta(
                    index,
                    json!({"type": "input_json_delta", "partial_json": part}),
                )
            })
            .collect()
    };
    let mut chunks = vec![
        message_start(json!({"input_tokens": 3})),
        block_start(
            0,
            json!({"type": "tool_use", "id": "t1", "name": "exec", "input": {}}),
        ),
    ];
    chunks.extend(deltas(0, &["{\"cmd\":", "\"ls\"}", ""]));
    chunks.push(block_stop(0));
    chunks.push(block_start(
        1,
        json!({"type": "tool_use", "id": "t2", "name": "Read"}),
    ));
    chunks.extend(deltas(1, &["{\"file_path\":\"a\",", "\"pages\":\"\"}"]));
    chunks.push(block_stop(1));
    chunks.push(block_start(
        2,
        json!({"type": "tool_use", "id": "t3", "name": "apply_patch"}),
    ));
    chunks.extend(deltas(2, &["{\"input\":\"*** Begin\"}"]));
    chunks.push(block_stop(2));
    chunks.push(block_start(
        3,
        json!({"type": "tool_use", "id": "t4", "name": "tool_search", "input": {"query": "files"}}),
    ));
    chunks.push(block_stop(3));
    chunks.push(block_start(4, json!({"type": "tool_use", "id": "t5", "name": "mcp_files__read", "input": {"b": 1, "a": 2}})));
    chunks.extend(deltas(4, &["   "]));
    chunks.push(block_stop(4));
    chunks.push(block_start(
        5,
        json!({"type": "tool_use", "id": "t6", "name": "nop"}),
    ));
    chunks.push(message_delta("tool_use", json!({"output_tokens": 9})));
    chunks.push(message_stop());
    let refs: Vec<&str> = chunks.iter().map(String::as_str).collect();
    assert_stream_parity(&refs);
}

#[test]
fn stream_errors_and_truncation_match_legacy() {
    let text = [
        block_start(0, json!({"type": "text", "text": ""})),
        block_delta(0, json!({"type": "text_delta", "text": "partial"})),
    ]
    .concat();
    let tool = [
        block_start(0, json!({"type": "tool_use", "id": "t", "name": "exec"})),
        block_delta(
            0,
            json!({"type": "input_json_delta", "partial_json": "{\"cmd\":"}),
        ),
    ]
    .concat();
    let custom = [
        block_start(
            0,
            json!({"type": "tool_use", "id": "t", "name": "apply_patch"}),
        ),
        block_delta(
            0,
            json!({"type": "input_json_delta", "partial_json": "{\"input\":\"x\"}"}),
        ),
    ]
    .concat();
    let start = message_start(json!({"input_tokens": 2}));
    // EOF 截断：有部分输出 / 已有 stop_reason / 什么都没有
    assert_stream_parity(&[&start, &text]);
    assert_stream_parity(&[&start, &tool]);
    assert_stream_parity(&[&start, &custom]);
    assert_stream_parity(&[&start, &text, &message_delta("end_turn", json!({}))]);
    assert_stream_parity(&[&start]);
    assert_stream_parity(&[]);
    assert_stream_parity(&[
        &start,
        &block_start(0, json!({"type": "thinking", "thinking": "  "})),
    ]);
    // 错误事件
    for error in [
        event(
            "error",
            json!({"type": "error", "error": {"type": "overloaded_error", "message": "busy"}}),
        ),
        event("error", json!({"type": "error", "error": "plain"})),
        event(
            "error",
            json!({"type": "error", "error": {"message": "no type"}}),
        ),
        event("error", json!({"type": "error", "error": {"type": ""}})),
    ] {
        assert_stream_parity(&[&start, &text, &error, &message_stop()]);
        assert_stream_parity(&[&start, &text, &message_stop(), &error]);
    }
}

#[test]
fn stream_framing_and_json_bodies_match_legacy() {
    // 最后一个事件没有空行、CRLF 行尾
    assert_stream_parity(&[
        "event: message_start\r\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"m\"}}\r\n\r\n",
        &block_start(0, json!({"type": "text", "text": ""})),
        &block_delta(0, json!({"type": "text_delta", "text": "x"})),
        "event: message_stop\ndata: {\"type\":\"message_stop\"}",
    ]);
    // 整个 JSON 消息体（含空行与前导空白）
    let body = json!({
        "id": "msg_json",
        "model": "claude",
        "content": [
            {"type": "thinking", "thinking": "t", "signature": "s"},
            {"type": "text", "text": "hi"},
            {"type": "tool_use", "id": "t1", "name": "Read", "input": {"z": 1, "pages": "", "a": 2}},
            {"type": "tool_use", "id": "t2", "name": "exec", "input": {}}
        ],
        "stop_reason": "tool_use",
        "usage": {"input_tokens": 10, "output_tokens": 4}
    });
    let pretty = serde_json::to_string_pretty(&body).unwrap();
    assert_stream_parity(&["\n  ", &pretty]);
    assert_stream_parity(&[&pretty[..10], "\n\n", &pretty[10..]]);
    assert_stream_parity(&[
        "{\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\",\"message\":\"busy\"}}",
    ]);
    assert_stream_parity(&["{\"content\": [}"]);
}

// ---- 有意的差异 ----

#[test]
fn ir_engine_documented_request_differences() {
    let body = json!({
        "model": "claude-haiku-4-5",
        "temperature": 1.0,
        "stream": false,
        "input": "hi"
    });
    let legacy = responses_to_anthropic_request(body.clone(), 8192, CodecEngine::Legacy).unwrap();
    let ir = responses_to_anthropic_request(body, 8192, CodecEngine::Ir).unwrap();
    // 整数值的浮点写成整数；`stream: false` 不转发（与缺省相同）
    assert_eq!(legacy["temperature"], json!(1.0));
    assert_eq!(ir["temperature"], json!(1));
    assert_eq!(legacy["stream"], json!(false));
    assert!(ir.get("stream").is_none());
}

#[test]
fn ir_engine_uses_the_default_id_for_an_empty_message_id() {
    // 旧流式转换器把空的 message.id 变成 `resp_`（非流式路径为 `resp_ccswitch`）；
    // IR 编码端两条路径都使用 `resp_ccswitch`
    let body = json!({"id": "", "content": []});
    let context = build_codex_tool_context_from_request(&json!({}));
    let frames: String = anthropic_message_to_responses_sse(&body, context, CodecEngine::Ir)
        .into_iter()
        .map(|frame| String::from_utf8(frame.to_vec()).unwrap())
        .collect();
    assert!(frames.contains("\"id\":\"resp_ccswitch\""), "{frames}");
    assert!(response(&body, &json!({}), CodecEngine::Ir).contains("\"resp_ccswitch\""));
}
