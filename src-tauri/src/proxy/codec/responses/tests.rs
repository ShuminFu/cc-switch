//! Anthropic ← OpenAI Responses 桥接：旧转换器与 IR 编解码的差分测试
//!
//! golden 语料之外再用手写输入覆盖旧转换器的各条分支，断言两种引擎输出逐字节相同；
//! 有意的差异集中在文末，逐条断言其行为。

use bytes::Bytes;
use futures::StreamExt;
use serde_json::{json, Value};

use super::ResponsesUpstreamOptions;
use crate::proxy::codec::bridge::{
    anthropic_to_responses_request, responses_to_anthropic_response, responses_to_anthropic_sse,
};
use crate::proxy::codec::CodecEngine;

fn request(body: &Value, options: &ResponsesUpstreamOptions, engine: CodecEngine) -> String {
    match anthropic_to_responses_request(body.clone(), options.clone(), engine) {
        Ok(value) => serde_json::to_string_pretty(&value).unwrap(),
        Err(error) => format!("<err {error}>"),
    }
}

#[track_caller]
fn assert_request_parity(body: Value) {
    let option_sets = [
        ResponsesUpstreamOptions::default(),
        ResponsesUpstreamOptions {
            cache_key: Some("session-1".into()),
            codex_oauth: true,
            codex_fast_mode: true,
        },
        ResponsesUpstreamOptions {
            cache_key: None,
            codex_oauth: true,
            codex_fast_mode: false,
        },
    ];
    for options in option_sets {
        assert_eq!(
            request(&body, &options, CodecEngine::Ir),
            request(&body, &options, CodecEngine::Legacy),
            "{options:?}"
        );
    }
}

fn response(body: &Value, engine: CodecEngine) -> String {
    match responses_to_anthropic_response(body.clone(), engine) {
        Ok(value) => serde_json::to_string_pretty(&value).unwrap(),
        Err(error) => format!("<err {error}>"),
    }
}

#[track_caller]
fn assert_response_parity(body: Value) {
    assert_eq!(
        response(&body, CodecEngine::Ir),
        response(&body, CodecEngine::Legacy)
    );
}

fn stream(chunks: &[&[u8]], transport_error: Option<&str>, engine: CodecEngine) -> String {
    let mut items: Vec<Result<Bytes, std::io::Error>> = chunks
        .iter()
        .map(|chunk| Ok(Bytes::copy_from_slice(chunk)))
        .collect();
    if let Some(message) = transport_error {
        items.push(Err(std::io::Error::other(message.to_string())));
    }
    let output: Vec<_> = futures::executor::block_on(
        responses_to_anthropic_sse(futures::stream::iter(items), engine).collect(),
    );
    output
        .into_iter()
        .map(|item| match item {
            Ok(bytes) => String::from_utf8(bytes.to_vec()).expect("utf-8 output"),
            Err(error) => format!("<err {error}>"),
        })
        .collect()
}

const MESSAGE_STOP: &str = "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n";

/// 去掉旧转换器在终止事件（`message_stop` 或第一个 `error`）之后补发的 error 事件
fn strip_after_terminal(mut output: String) -> String {
    let terminal_end = output
        .find(MESSAGE_STOP)
        .map(|end| end + MESSAGE_STOP.len())
        .or_else(|| {
            output
                .find("event: error\n")
                .and_then(|start| output[start..].find("\n\n").map(|end| start + end + 2))
        });
    if let Some(end) = terminal_end {
        let tail = output.split_off(end);
        assert!(
            tail.is_empty() || tail.starts_with("event: error\n"),
            "{tail}"
        );
    }
    output
}

/// 传输错误追加在上游末尾时同样比较；有意的差异：终止事件之后再出现的传输错误，
/// 旧转换器仍会补一个 error 事件，IR 编码端不再输出任何帧
#[track_caller]
fn assert_stream_parity(chunks: &[&str]) {
    let chunks: Vec<&[u8]> = chunks.iter().map(|chunk| chunk.as_bytes()).collect();
    for transport_error in [None, Some("connection reset")] {
        let mut legacy = stream(&chunks, transport_error, CodecEngine::Legacy);
        let ir = stream(&chunks, transport_error, CodecEngine::Ir);
        if transport_error.is_some() {
            legacy = strip_after_terminal(legacy);
        }
        assert_eq!(ir, legacy, "transport_error={transport_error:?}");
    }
    // 每个字节边界切分也必须得到同样的输出
    let joined: Vec<u8> = chunks.concat();
    if joined.len() <= 1024 {
        for split in 1..joined.len() {
            let parts = [&joined[..split], &joined[split..]];
            assert_eq!(
                stream(&parts, None, CodecEngine::Ir),
                stream(&parts, None, CodecEngine::Legacy),
                "split at {split}"
            );
        }
    }
}

fn event(name: &str, data: Value) -> String {
    format!("event: {name}\ndata: {data}\n\n")
}

fn created() -> String {
    event(
        "response.created",
        json!({"type": "response.created", "response": {"id": "resp_1", "model": "gpt-5", "usage": {"input_tokens": 12, "input_tokens_details": {"cached_tokens": 2}}}}),
    )
}

fn completed(extra: Value) -> String {
    let mut response =
        json!({"status": "completed", "usage": {"input_tokens": 20, "output_tokens": 5}});
    if let Value::Object(extra) = extra {
        for (key, value) in extra {
            response[key] = value;
        }
    }
    event(
        "response.completed",
        json!({"type": "response.completed", "response": response}),
    )
}

// ---- 请求 ----

#[test]
fn request_system_and_sampling_match_legacy() {
    assert_request_parity(json!({
        "model": "gpt-5.4",
        "system": "x-anthropic-billing-header: cc_version=1; cch=abc\nYou are helpful.",
        "max_tokens": 2048,
        "temperature": 0.3,
        "top_p": 0.9,
        "stream": true,
        "stop_sequences": ["END"],
        "messages": [{"role": "user", "content": "hi"}]
    }));
    assert_request_parity(json!({
        "model": "gpt-4o",
        "system": [
            {"type": "text", "text": "x-anthropic-billing-header: cch=1"},
            {"type": "text", "text": "first", "cache_control": {"type": "ephemeral"}},
            {"type": "text", "text": ""},
            {"type": "text", "text": "second"}
        ],
        "include": ["message.output_text.logprobs"],
        "messages": [{"role": "user", "content": [{"type": "text", "text": "hi"}]}]
    }));
    assert_request_parity(json!({
        "model": "o3",
        "system": "x-anthropic-billing-header: cch=1",
        "messages": [{"role": "user", "content": "hi"}]
    }));
}

#[test]
fn request_content_and_media_match_legacy() {
    assert_request_parity(json!({
        "model": "gpt-5",
        "messages": [
            {"role": "user", "content": [
                {"type": "text", "text": "look"},
                {"type": "image", "source": {"type": "base64", "media_type": "image/jpeg", "data": "AAA"}},
                {"type": "image", "source": {"data": "BBB"}},
                {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": ""}},
                {"type": "image", "source": {"type": "url", "url": "https://example.com/a.png"}},
                {"type": "image", "source": {"type": "url", "url": "file:///etc/passwd"}},
                {"type": "document", "title": "a.pdf", "source": {"type": "base64", "media_type": "application/pdf", "data": "JVBE"}},
                {"type": "document", "source": {"type": "base64", "data": "JVBE"}},
                {"type": "document", "filename": "b.pdf", "source": {"type": "url", "url": "https://example.com/b.pdf"}},
                {"type": "document", "source": {"type": "text", "media_type": "text/plain", "data": "notes"}},
                {"type": "server_tool_use", "id": "x"},
                {"type": "text", "text": "after"}
            ]},
            {"role": "assistant", "content": "plain answer"},
            {"role": "assistant", "content": [{"type": "text", "text": "a"}, {"type": "text", "text": "b"}]},
            {"role": "user", "content": []}
        ]
    }));
}

#[test]
fn request_tool_history_matches_legacy() {
    assert_request_parity(json!({
        "model": "gpt-5",
        "messages": [
            {"role": "user", "content": "run"},
            {"role": "assistant", "content": [
                {"type": "text", "text": "calling"},
                {"type": "tool_use", "id": "call_1", "name": "exec", "input": {"z": 1, "a": [2, 1]}},
                {"type": "tool_use", "id": "call_2", "name": "noop"}
            ]},
            {"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "call_1", "content": "ok"},
                {"type": "tool_result", "tool_use_id": "call_2", "content": [{"type": "text", "text": "a"}, {"type": "text", "text": "b"}]},
                {"type": "text", "text": "and then"}
            ]},
            {"role": "assistant", "content": [{"type": "tool_use", "id": "call_3", "name": "exec", "input": {}}]},
            {"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "call_3", "is_error": true, "content": "boom"},
                {"type": "tool_result", "tool_use_id": "call_4", "is_error": true},
                {"type": "tool_result", "tool_use_id": "call_5"},
                {"type": "tool_result", "tool_use_id": "call_6", "content": {"k": "v"}},
                {"type": "tool_result", "tool_use_id": "call_7", "content": [
                    {"type": "text", "text": "t"},
                    {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "IMG"}},
                    {"type": "image", "source": {"type": "url", "url": "ftp://x/y.png"}},
                    {"type": "document", "title": "d.pdf", "source": {"type": "url", "url": "https://example.com/d.pdf"}},
                    {"type": "document", "source": {"type": "base64", "media_type": "application/pdf", "data": ""}},
                    {"type": "search_result", "source": "s", "title": "t", "content": []}
                ]}
            ]}
        ]
    }));
}

#[test]
fn request_reasoning_replay_matches_legacy() {
    let item = json!({"type": "reasoning", "id": "rs_1", "summary": [{"type": "summary_text", "text": "think"}], "encrypted_content": "enc"});
    let envelope =
        crate::proxy::providers::reasoning_bridge::encode_openai_reasoning_item(&item).unwrap();
    assert_request_parity(json!({
        "model": "gpt-5",
        "messages": [
            {"role": "user", "content": "q"},
            {"role": "assistant", "content": [
                {"type": "thinking", "thinking": "think", "signature": envelope},
                {"type": "text", "text": "answer"}
            ]},
            {"role": "assistant", "content": [
                {"type": "redacted_thinking", "data": envelope},
                {"type": "tool_use", "id": "c1", "name": "f", "input": {}}
            ]},
            {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "c1", "content": "r"}]},
            {"role": "assistant", "content": [
                {"type": "thinking", "thinking": "local only", "signature": "anthropic-sig"},
                {"type": "text", "text": "x"},
                {"type": "redacted_thinking", "data": envelope}
            ]},
            {"role": "assistant", "content": [{"type": "redacted_thinking", "data": envelope}]},
            {"role": "user", "content": "go on"}
        ]
    }));
}

#[test]
fn request_tools_choice_and_reasoning_match_legacy() {
    let tools = json!([
        {"name": "search", "description": "Search", "input_schema": {"type": "object", "properties": {"u": {"type": "string", "format": "uri"}}}},
        {"name": "bare"},
        {"type": "BatchTool", "name": "batch"}
    ]);
    for tool_choice in [
        json!({"type": "auto"}),
        json!({"type": "any"}),
        json!({"type": "none"}),
        json!({"type": "tool", "name": "search"}),
        json!("auto"),
        json!("none"),
    ] {
        assert_request_parity(json!({
            "model": "gpt-5",
            "tools": tools,
            "tool_choice": tool_choice,
            "messages": [{"role": "user", "content": "hi"}]
        }));
    }
    assert_request_parity(json!({
        "model": "gpt-5",
        "tools": [{"type": "BatchTool", "name": "batch"}],
        "messages": [{"role": "user", "content": "hi"}]
    }));
    for (model, extra) in [
        (
            "gpt-5.4",
            json!({"thinking": {"type": "enabled", "budget_tokens": 1000}}),
        ),
        (
            "gpt-5.4",
            json!({"thinking": {"type": "enabled", "budget_tokens": 8000}}),
        ),
        (
            "gpt-5.4",
            json!({"thinking": {"type": "enabled", "budget_tokens": 32000}}),
        ),
        ("gpt-5.4", json!({"thinking": {"type": "enabled"}})),
        ("gpt-5.4", json!({"thinking": {"type": "adaptive"}})),
        ("gpt-5.4", json!({"thinking": {"type": "disabled"}})),
        ("gpt-5.4", json!({"output_config": {"effort": "max"}})),
        (
            "gpt-5.4",
            json!({"output_config": {"effort": "bogus"}, "thinking": {"type": "enabled"}}),
        ),
        (
            "o4-mini",
            json!({"thinking": {"type": "enabled", "budget_tokens": 5000}}),
        ),
        (
            "gpt-4o",
            json!({"thinking": {"type": "enabled", "budget_tokens": 5000}}),
        ),
    ] {
        let mut body = json!({"model": model, "messages": [{"role": "user", "content": "hi"}]});
        for (key, value) in extra.as_object().unwrap() {
            body[key] = value.clone();
        }
        assert_request_parity(body);
    }
}

// ---- 非流式响应 ----

#[test]
fn response_variants_match_legacy() {
    let encrypted = json!({"type": "reasoning", "id": "rs_1", "summary": [{"type": "summary_text", "text": "hmm"}], "encrypted_content": "enc"});
    assert_response_parity(json!({
        "id": "resp_1",
        "model": "gpt-5",
        "status": "completed",
        "output": [
            encrypted,
            {"type": "reasoning", "id": "rs_2", "summary": [], "encrypted_content": "only"},
            {"type": "reasoning", "id": "rs_3", "summary": [{"type": "summary_text", "text": "plain"}, {"type": "reasoning_text", "text": " more"}]},
            {"type": "reasoning", "id": "rs_4", "summary": []},
            {"type": "message", "content": [
                {"type": "output_text", "text": "hello"},
                {"type": "output_text", "text": ""},
                {"type": "refusal", "refusal": "no"},
                {"type": "image"}
            ]},
            {"type": "function_call", "call_id": "c1", "name": "Read", "arguments": "{\"file_path\":\"a\",\"pages\":\"\"}"},
            {"type": "function_call", "call_id": "c2", "name": "f", "arguments": ""},
            {"type": "function_call", "call_id": "c3", "name": "g"},
            {"type": "web_search_call", "id": "ws"}
        ],
        "usage": {"input_tokens": 100, "output_tokens": 7, "input_tokens_details": {"cached_tokens": 40, "cache_write_tokens": 10}, "output_tokens_details": {"reasoning_tokens": 3}}
    }));
    for (status, reason) in [
        (Some("incomplete"), Some("max_output_tokens")),
        (Some("incomplete"), Some("content_filter")),
        (Some("incomplete"), None),
        (Some("in_progress"), None),
        (None, None),
    ] {
        let mut body = json!({
            "id": "resp_2",
            "output": [
                {"type": "message", "content": [{"type": "output_text", "text": "partial"}]},
                {"type": "function_call", "call_id": "c1", "name": "f", "arguments": "{\"a\":"},
                {"type": "function_call", "call_id": "c2", "name": "f", "arguments": "[1]"}
            ]
        });
        if let Some(status) = status {
            body["status"] = json!(status);
        }
        if let Some(reason) = reason {
            body["incomplete_details"] = json!({ "reason": reason });
        }
        assert_response_parity(body);
    }
    for body in [
        json!({"status": "completed", "output": [{"type": "function_call", "call_id": "c", "name": "f", "arguments": "{bad"}]}),
        json!({"status": "completed", "output": [{"type": "function_call", "call_id": "c", "name": "f", "arguments": "\"str\""}]}),
        json!({"status": "failed", "error": {"message": "quota"}, "output": []}),
        json!({"status": "failed", "output": []}),
        json!({"status": "cancelled", "message": "user stop", "output": []}),
        json!({"error": "bad gateway"}),
        json!({"error": {"message": "  "}, "output": []}),
        json!({"error": null, "status": "completed", "output": []}),
        json!({"status": "completed"}),
        json!({"status": "completed", "output": [], "usage": {}}),
        json!({"status": "completed", "output": [], "usage": {"prompt_tokens": 9, "completion_tokens": 2, "prompt_tokens_details": {"cached_tokens": 3}}}),
    ] {
        assert_response_parity(body);
    }
}

// ---- 流式 ----

#[test]
fn stream_text_and_usage_match_legacy() {
    let text_added = event(
        "response.content_part.added",
        json!({"type": "response.content_part.added", "item_id": "msg_1", "output_index": 0, "content_index": 0, "part": {"type": "output_text", "text": ""}}),
    );
    let delta = |text: &str| {
        event(
            "response.output_text.delta",
            json!({"type": "response.output_text.delta", "item_id": "msg_1", "output_index": 0, "content_index": 0, "delta": text}),
        )
    };
    let text_done = event(
        "response.output_text.done",
        json!({"type": "response.output_text.done", "item_id": "msg_1", "output_index": 0, "content_index": 0, "text": "Hi there"}),
    );
    assert_stream_parity(&[
        &created(),
        &text_added,
        &delta("Hi"),
        &delta(" there"),
        &text_done,
        &completed(json!({})),
    ]);
    // 没有 response.created、usage 缺失、带 cache 字段
    assert_stream_parity(&[&delta("x"), &completed(json!({"usage": null}))]);
    assert_stream_parity(&[
        &delta("x"),
        &completed(
            json!({"usage": {"input_tokens": 50, "output_tokens": 1, "input_tokens_details": {"cached_tokens": 30, "cache_write_tokens": 5}}}),
        ),
    ]);
    // refusal 与 keyless 文本
    assert_stream_parity(&[
        &created(),
        &event(
            "response.refusal.delta",
            json!({"type": "response.refusal.delta", "delta": "no"}),
        ),
        &event(
            "response.refusal.done",
            json!({"type": "response.refusal.done"}),
        ),
        &event(
            "response.output_text.delta",
            json!({"type": "response.output_text.delta", "delta": "keyless"}),
        ),
        &completed(
            json!({"status": "incomplete", "incomplete_details": {"reason": "max_output_tokens"}}),
        ),
    ]);
    // 没有 event: 行，只靠 payload type
    assert_stream_parity(&[
        "data: {\"type\":\"response.created\",\"response\":{\"id\":\"r\",\"model\":\"m\"}}\n\n",
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"a\"}\n\n",
        "data: not json\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\"}}\n\n",
    ]);
}

#[test]
fn stream_tool_calls_match_legacy() {
    let added = |id: &str, call_id: &str, name: &str, output_index: u64| {
        event(
            "response.output_item.added",
            json!({"type": "response.output_item.added", "output_index": output_index, "item": {"type": "function_call", "id": id, "call_id": call_id, "name": name, "arguments": ""}}),
        )
    };
    let args = |id: &str, delta: &str| {
        event(
            "response.function_call_arguments.delta",
            json!({"type": "response.function_call_arguments.delta", "item_id": id, "delta": delta}),
        )
    };
    let args_done = |id: &str, arguments: &str| {
        event(
            "response.function_call_arguments.done",
            json!({"type": "response.function_call_arguments.done", "item_id": id, "arguments": arguments}),
        )
    };
    let item_done = |id: &str, call_id: &str, name: &str, arguments: &str| {
        event(
            "response.output_item.done",
            json!({"type": "response.output_item.done", "item": {"type": "function_call", "id": id, "call_id": call_id, "name": name, "arguments": arguments}}),
        )
    };
    assert_stream_parity(&[
        &created(),
        &event(
            "response.output_text.delta",
            json!({"type": "response.output_text.delta", "item_id": "m", "content_index": 0, "delta": "let me"}),
        ),
        &added("fc_1", "call_1", "exec", 1),
        &added("fc_2", "call_2", "Read", 2),
        &args("fc_1", "{\"cmd\":"),
        &args("fc_2", "{\"file_path\":\"a\",\"pages\":\"\"}"),
        &args("fc_1", "\"ls\"}"),
        &args_done("fc_1", "{\"cmd\":\"ls\"}"),
        &item_done(
            "fc_2",
            "call_2",
            "Read",
            "{\"file_path\":\"a\",\"pages\":\"\"}",
        ),
        &item_done("fc_1", "call_1", "exec", "{\"cmd\":\"ls\"}"),
        &completed(json!({})),
    ]);
    // 只有 done 事件带参数、未知 item_id 的增量、按 output_index 定位
    assert_stream_parity(&[
        &added("fc_1", "call_1", "f", 0),
        &args_done("fc_1", "{\"a\":1}"),
        &event(
            "response.function_call_arguments.delta",
            json!({"type": "response.function_call_arguments.delta", "item_id": "ghost", "call_id": "call_g", "name": "g", "delta": "{}"}),
        ),
        &event(
            "response.output_item.added",
            json!({"type": "response.output_item.added", "output_index": 4, "item": {"type": "function_call", "call_id": "call_3", "name": "h"}}),
        ),
        &event(
            "response.function_call_arguments.delta",
            json!({"type": "response.function_call_arguments.delta", "output_index": 4, "delta": "{\"x\":2}"}),
        ),
        &event(
            "response.output_item.done",
            json!({"type": "response.output_item.done", "output_index": 4, "item": {"type": "function_call", "call_id": "call_3", "name": "h", "arguments": "{\"x\":2}"}}),
        ),
        &item_done("ghost", "call_g", "g", ""),
        &completed(json!({"status": "completed"})),
    ]);
    // 只在 output_item.done 带参数，以及重复的 added
    assert_stream_parity(&[
        &added("fc_1", "call_1", "f", 0),
        &added("fc_1", "call_1", "f", 0),
        &item_done("fc_1", "call_1", "f", "{\"y\":true}"),
        &completed(json!({})),
    ]);
}

#[test]
fn stream_reasoning_matches_legacy() {
    let reasoning_added = |id: &str| {
        event(
            "response.output_item.added",
            json!({"type": "response.output_item.added", "output_index": 0, "item": {"type": "reasoning", "id": id, "summary": []}}),
        )
    };
    let summary_delta = |id: &str, delta: &str| {
        event(
            "response.reasoning_summary_text.delta",
            json!({"type": "response.reasoning_summary_text.delta", "item_id": id, "delta": delta}),
        )
    };
    let reasoning_done = |id: &str, summary: &str, encrypted: Option<&str>| {
        let mut item = json!({"type": "reasoning", "id": id, "summary": [{"type": "summary_text", "text": summary}]});
        if let Some(encrypted) = encrypted {
            item["encrypted_content"] = json!(encrypted);
        }
        event(
            "response.output_item.done",
            json!({"type": "response.output_item.done", "item": item}),
        )
    };
    assert_stream_parity(&[
        &created(),
        &reasoning_added("rs_1"),
        &summary_delta("rs_1", "Think"),
        &summary_delta("rs_1", "ing"),
        &reasoning_done("rs_1", "Thinking", Some("enc1")),
        &reasoning_added("rs_2"),
        &event(
            "response.reasoning_summary_text.done",
            json!({"type": "response.reasoning_summary_text.done", "item_id": "rs_2", "text": "from done"}),
        ),
        &reasoning_done("rs_2", "from done", None),
        &reasoning_added("rs_3"),
        &reasoning_done("rs_3", "", Some("enc3")),
        &reasoning_done("rs_4", "only at done", Some("enc4")),
        &event(
            "response.output_text.delta",
            json!({"type": "response.output_text.delta", "delta": "answer"}),
        ),
        &completed(json!({})),
    ]);
    // 旧网关的 keyless reasoning 事件
    assert_stream_parity(&[
        &event(
            "response.reasoning.delta",
            json!({"type": "response.reasoning.delta", "delta": "a"}),
        ),
        &event(
            "response.reasoning.delta",
            json!({"type": "response.reasoning.delta", "text": "b"}),
        ),
        &event(
            "response.reasoning.done",
            json!({"type": "response.reasoning.done"}),
        ),
        &event(
            "response.reasoning_text.delta",
            json!({"type": "response.reasoning_text.delta", "output_index": 3, "delta": "c"}),
        ),
        &event(
            "response.output_text.delta",
            json!({"type": "response.output_text.delta", "delta": "answer"}),
        ),
        &completed(json!({})),
    ]);
}

#[test]
fn stream_errors_and_truncation_match_legacy() {
    let text = event(
        "response.output_text.delta",
        json!({"type": "response.output_text.delta", "delta": "partial"}),
    );
    let tool = event(
        "response.output_item.added",
        json!({"type": "response.output_item.added", "item": {"type": "function_call", "id": "fc", "call_id": "c", "name": "f"}}),
    );
    let reasoning = event(
        "response.reasoning_summary_text.delta",
        json!({"type": "response.reasoning_summary_text.delta", "item_id": "rs", "delta": "hm"}),
    );
    // EOF 截断：只有文本 / 有工具 / 有 reasoning / 什么都没有
    assert_stream_parity(&[&created(), &text]);
    assert_stream_parity(&[&created(), &tool]);
    assert_stream_parity(&[&created(), &reasoning]);
    assert_stream_parity(&[&created()]);
    assert_stream_parity(&[]);
    assert_stream_parity(&["   \n\n"]);
    // 失败事件
    for failure in [
        event(
            "response.failed",
            json!({"type": "response.failed", "response": {"status": "failed", "error": {"type": "server_error", "message": "boom"}}}),
        ),
        event("response.failed", json!({"type": "response.failed"})),
        event(
            "error",
            json!({"type": "error", "code": "rate_limit", "message": "slow down"}),
        ),
        event("error", json!({"type": "error", "error": "plain string"})),
        completed(json!({"status": "failed", "error": {"code": "x", "message": "bad"}})),
        completed(json!({"status": "cancelled"})),
        completed(json!({"error": {"message": "  "}})),
    ] {
        assert_stream_parity(&[&created(), &text, &failure, &text, &completed(json!({}))]);
    }
}

#[test]
fn stream_framing_and_json_bodies_match_legacy() {
    // 最后一个事件没有空行、CRLF 行尾、注释与 BOM
    assert_stream_parity(&[
        "\u{feff}: keep-alive\r\n\r\n",
        "event: response.output_text.delta\r\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"crlf\"}\r\n\r\n",
        "event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\"}}",
    ]);
    // 整个 JSON 响应体
    let body = json!({
        "id": "resp_json",
        "model": "gpt-5",
        "status": "completed",
        "output": [
            {"type": "reasoning", "id": "rs", "summary": [{"type": "summary_text", "text": "t"}], "encrypted_content": "e"},
            {"type": "reasoning", "id": "rs2", "summary": [], "encrypted_content": "e2"},
            {"type": "message", "content": [{"type": "output_text", "text": "hi"}]},
            {"type": "function_call", "call_id": "c", "name": "Read", "arguments": "{\"z\":1,\"pages\":\"\",\"a\":2}"}
        ],
        "usage": {"input_tokens": 10, "output_tokens": 4, "input_tokens_details": {"cached_tokens": 6}}
    });
    let pretty = serde_json::to_string_pretty(&body).unwrap();
    assert_stream_parity(&["\n  ", &pretty]);
    assert_stream_parity(&[&pretty[..10], "\n\n", &pretty[10..]]);
    assert_stream_parity(&[&serde_json::to_string(
        &json!({"status": "failed", "error": {"message": "nope"}}),
    )
    .unwrap()]);
    assert_stream_parity(&["{\"status\": \"completed\", \"output\": [}"]);
    assert_stream_parity(&["[1, 2]"]);
}

// ---- 有意的差异 ----

#[test]
fn ir_engine_documented_request_differences() {
    let body = json!({
        "model": "gpt-5",
        "messages": [
            {"role": "system", "content": "folded"},
            {"role": "user"},
            {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "c", "content": null}]}
        ],
        "tool_choice": "any",
        "stream": false
    });
    let ir =
        anthropic_to_responses_request(body, ResponsesUpstreamOptions::default(), CodecEngine::Ir)
            .unwrap();
    // role: system 的消息并入 instructions；没有 content 的消息被丢弃；
    // null 工具结果为空数组；字符串 tool_choice "any" 规范化为 required；stream: false 不转发
    assert_eq!(ir["instructions"], json!("folded"));
    assert_eq!(
        ir["input"],
        json!([{"type": "function_call_output", "call_id": "c", "output": []}])
    );
    assert_eq!(ir["tool_choice"], json!("required"));
    assert!(ir.get("stream").is_none());
}

#[test]
fn ir_engine_emits_nothing_after_a_terminal_event() {
    let chunks: Vec<&[u8]> = vec![b"event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\"}}\n\n"];
    let ir = stream(&chunks, Some("reset"), CodecEngine::Ir);
    assert!(ir.ends_with(MESSAGE_STOP), "{ir}");
    let legacy = stream(&chunks, Some("reset"), CodecEngine::Legacy);
    assert!(legacy.contains("event: error\n"), "{legacy}");
}

#[test]
fn ir_engine_drops_the_anthropic_cache_ttl_breakdown() {
    // 上游原样带回的 Anthropic `cache_creation` TTL 分桶不参与计费算术，IR 的 usage
    // 不携带它；token 数与旧转换器一致
    let body = json!({
        "status": "completed",
        "output": [],
        "usage": {"input_tokens": 10, "output_tokens": 1, "cache_creation": {"ephemeral_5m_input_tokens": 3}}
    });
    let legacy = responses_to_anthropic_response(body.clone(), CodecEngine::Legacy).unwrap();
    let ir = responses_to_anthropic_response(body, CodecEngine::Ir).unwrap();
    assert!(legacy["usage"].get("cache_creation").is_some());
    assert!(ir["usage"].get("cache_creation").is_none());
    let mut legacy_usage = legacy["usage"].clone();
    legacy_usage
        .as_object_mut()
        .unwrap()
        .remove("cache_creation");
    assert_eq!(ir["usage"], legacy_usage);
}
