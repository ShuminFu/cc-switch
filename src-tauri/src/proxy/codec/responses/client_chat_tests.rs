//! OpenAI Responses（Codex）← OpenAI Chat 桥接：旧转换器与 IR 编解码的差分测试
//!
//! golden 语料之外再用手写输入覆盖旧转换器的各条分支，断言两种引擎输出逐字节相同；
//! 有意的差异集中在文末，逐条断言其行为。

use bytes::Bytes;
use futures::StreamExt;
use serde_json::{json, Value};

use super::tools::build_codex_tool_context_from_request;
use crate::provider::CodexChatReasoningConfig;
use crate::proxy::codec::bridge::{
    chat_to_responses_response, chat_to_responses_sse, responses_to_chat_request,
};
use crate::proxy::codec::CodecEngine;

fn request(body: &Value, config: Option<&CodexChatReasoningConfig>, engine: CodecEngine) -> String {
    match responses_to_chat_request(body.clone(), config, engine) {
        Ok(value) => serde_json::to_string_pretty(&value).unwrap(),
        Err(error) => format!("<err {error}>"),
    }
}

#[track_caller]
fn assert_request_parity(body: Value) {
    assert_eq!(
        request(&body, None, CodecEngine::Ir),
        request(&body, None, CodecEngine::Legacy)
    );
}

fn tools_request() -> Value {
    json!({
        "tools": [
            {"type": "function", "name": "exec", "description": "Run", "strict": true, "parameters": {"type": "object", "properties": {"cmd": {"type": "string"}}}},
            {"type": "function", "name": "nop", "parameters": {"type": null}},
            {"type": "function", "name": "bare"},
            {"type": "custom", "name": "apply_patch", "description": "Patch", "format": {"type": "grammar"}},
            "freeform",
            {"type": "tool_search"},
            {"type": "namespace", "name": "mcp_files", "tools": [{"type": "function", "name": "read", "parameters": {"type": "object"}}]},
            {"type": "namespace", "name": "a_very_long_namespace_name_that_needs_hashing_because_it_is_long", "tools": [{"type": "function", "name": "and_a_long_tool_name_too"}]},
            {"type": "web_search"}
        ]
    })
}

fn response(body: &Value, context: &Value, engine: CodecEngine) -> String {
    let context = build_codex_tool_context_from_request(context);
    match chat_to_responses_response(body.clone(), &context, engine) {
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
        chat_to_responses_sse(futures::stream::iter(items), context, engine).collect(),
    );
    output
        .into_iter()
        .map(|item| match item {
            Ok(bytes) => String::from_utf8(bytes.to_vec()).expect("utf-8 output"),
            Err(error) => format!("<err {error}>"),
        })
        .collect()
}

const COMPLETED: &str = "event: response.completed\n";

/// 逐帧比较，失败时只显示第一处不同的帧
#[track_caller]
fn assert_same_frames(ir: &str, legacy: &str, context: &str) {
    let ir_frames: Vec<&str> = ir.split_inclusive("\n\n").collect();
    let legacy_frames: Vec<&str> = legacy.split_inclusive("\n\n").collect();
    for (index, (ir, legacy)) in ir_frames.iter().zip(&legacy_frames).enumerate() {
        assert_eq!(ir, legacy, "{context}: frame {index} differs");
    }
    assert_eq!(
        ir_frames.len(),
        legacy_frames.len(),
        "{context}: frame count differs\n--- ir tail: {:?}\n--- legacy tail: {:?}",
        ir_frames.last(),
        legacy_frames.last()
    );
}

/// 旧转换器在 response.completed 之后仍会为传输错误补发 response.failed
fn strip_after_completed(output: String) -> String {
    match output.find(COMPLETED) {
        Some(start) => {
            let end = output[start..]
                .find("\n\n")
                .map(|end| start + end + 2)
                .unwrap_or(output.len());
            let tail = &output[end..];
            assert!(
                tail.is_empty() || tail.starts_with("event: response.failed\n"),
                "{tail}"
            );
            output[..end].to_string()
        }
        None => output,
    }
}

/// 传输错误追加在上游末尾时同样比较；每个字节边界切分也必须得到同样的输出
#[track_caller]
fn assert_stream_parity(chunks: &[&str]) {
    let chunks: Vec<&[u8]> = chunks.iter().map(|chunk| chunk.as_bytes()).collect();
    for context in [json!({}), tools_request()] {
        for transport_error in [None, Some("connection reset")] {
            let ir = stream(&chunks, transport_error, &context, CodecEngine::Ir);
            let mut legacy = stream(&chunks, transport_error, &context, CodecEngine::Legacy);
            if transport_error.is_some() {
                legacy = strip_after_completed(legacy);
            }
            assert_same_frames(
                &ir,
                &legacy,
                &format!("transport_error={transport_error:?}"),
            );
        }
    }
    let joined: Vec<u8> = chunks.concat();
    if joined.len() <= 1536 {
        for split in 1..joined.len() {
            let parts = [&joined[..split], &joined[split..]];
            assert_same_frames(
                &stream(&parts, None, &tools_request(), CodecEngine::Ir),
                &stream(&parts, None, &tools_request(), CodecEngine::Legacy),
                &format!("split at {split}"),
            );
        }
    }
}

fn data(value: Value) -> String {
    format!("data: {value}\n\n")
}

const DONE: &str = "data: [DONE]\n\n";

fn delta(delta: Value) -> String {
    data(
        json!({"id": "chatcmpl_1", "created": 7, "model": "kimi-k2", "choices": [{"index": 0, "delta": delta}]}),
    )
}

fn finish(reason: &str, usage: Option<Value>) -> String {
    let mut chunk = json!({"id": "chatcmpl_1", "model": "kimi-k2", "choices": [{"index": 0, "delta": {}, "finish_reason": reason}]});
    if let Some(usage) = usage {
        chunk["usage"] = usage;
    }
    data(chunk)
}

fn tool_delta(index: u64, id: Option<&str>, name: Option<&str>, arguments: &str) -> String {
    let mut call = json!({"index": index, "function": {"arguments": arguments}});
    if let Some(id) = id {
        call["id"] = json!(id);
    }
    if let Some(name) = name {
        call["function"]["name"] = json!(name);
    }
    delta(json!({"tool_calls": [call]}))
}

// ---- 请求 ----

#[test]
fn request_messages_and_reasoning_match_legacy() {
    let reasoning = |text: &str| json!({"type": "reasoning", "summary": [{"type": "summary_text", "text": text}]});
    assert_request_parity(json!({
        "model": "kimi-k2-thinking",
        "instructions": "Be brief.",
        "input": [
            {"role": "developer", "content": "dev one"},
            {"role": "system", "content": [{"type": "input_text", "text": "a"}, {"type": "input_text", "text": "b"}]},
            {"role": "system", "content": "   "},
            {"role": "user", "content": [{"type": "input_text", "text": "q1"}, {"type": "input_text", "text": ""}, {"type": "input_text", "text": "q2"}]},
            reasoning("think 1"),
            reasoning("  think 2  "),
            {"type": "function_call", "call_id": "c1", "name": "exec", "arguments": "{\"b\":1,\"a\":2}", "reasoning_content": "think 1"},
            {"type": "function_call", "call_id": "c2", "name": "read", "namespace": "mcp_files", "arguments": ""},
            {"type": "custom_tool_call", "call_id": "c3", "name": "apply_patch", "input": "*** Begin"},
            {"type": "tool_search_call", "call_id": "c4", "arguments": {"query": "x"}},
            {"type": "function_call_output", "call_id": "c1", "output": "{\"z\": 1, \"a\": [2]}"},
            {"type": "function_call_output", "call_id": "c2", "output": [{"type": "input_text", "text": "x"}]},
            {"type": "function_call_output", "call_id": "c5"},
            {"type": "custom_tool_call_output", "call_id": "c3", "output": "patched"},
            {"type": "tool_search_output", "call_id": "c4", "tools": [{"type": "function", "name": "loaded"}]},
            reasoning("final thought"),
            {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "Done"}, {"type": "refusal", "refusal": "but no"}], "reasoning_content": "embedded"},
            reasoning("trailing"),
            {"role": "user", "content": "Continue"},
            reasoning("orphan after user"),
            {"type": "input_text", "text": "loose"},
            {"type": "message", "role": "assistant", "content": "second"},
            reasoning("tail")
        ]
    }));
    assert_request_parity(json!({"model": "m", "input": "plain string"}));
    assert_request_parity(
        json!({"model": "m", "input": {"role": "user", "content": "object input"}}),
    );
    assert_request_parity(
        json!({"model": "m", "instructions": [{"text": "a"}, "b", {"text": ""}], "input": []}),
    );
    assert_request_parity(json!({"model": "m", "input": [reasoning("only reasoning")]}));
    assert_request_parity(json!({
        "model": "m",
        "input": [
            {"type": "function_call", "call_id": "c1", "name": "exec", "arguments": "{}"},
            reasoning("between calls"),
            {"type": "function_call", "id": "c2", "name": "exec", "arguments": {"obj": true}}
        ]
    }));
}

#[test]
fn request_media_matches_legacy() {
    assert_request_parity(json!({
        "model": "gpt-5.4",
        "input": [
            {"role": "user", "content": [
                {"type": "input_text", "text": "see"},
                {"type": "input_image", "image_url": "data:image/png;base64,AAA"},
                {"type": "input_image", "image_url": {"url": "https://example.com/a.png"}},
                {"type": "input_file", "file_id": "file_1", "filename": "a.pdf"},
                {"type": "input_file", "file_data": "data:application/pdf;base64,JVBE"},
                {"type": "input_file", "file_url": "https://example.com/only-url.pdf"}
            ]},
            {"type": "input_image", "image_url": "https://example.com/top.png"},
            {"type": "input_file", "file_id": "file_2"},
            {"type": "input_text", "role": "assistant", "text": "assistant text item"}
        ]
    }));
}

#[test]
fn request_tools_limits_and_passthrough_match_legacy() {
    let mut base = tools_request();
    base["model"] = json!("gpt-5.4");
    base["input"] = json!("hi");
    for tool_choice in [
        json!("auto"),
        json!("required"),
        json!("none"),
        json!("bogus"),
        json!({"type": "function", "name": "exec"}),
        json!({"type": "function", "name": "read", "namespace": "mcp_files"}),
        json!({"type": "function", "name": "x", "namespace": "unknown_ns"}),
        json!({"type": "custom", "name": "apply_patch"}),
        json!({"type": "tool_search"}),
        json!({"type": "allowed_tools", "mode": "auto", "tools": []}),
    ] {
        let mut body = base.clone();
        body["tool_choice"] = tool_choice;
        assert_request_parity(body);
    }
    let mut body = base.clone();
    for (key, value) in [
        ("max_output_tokens", json!(512)),
        ("max_tokens", json!(100)),
        ("max_completion_tokens", json!(200)),
        ("temperature", json!(0.4)),
        ("top_p", json!(0.8)),
        ("stream", json!(true)),
        ("frequency_penalty", json!(0.1)),
        ("logit_bias", json!({"1": 2})),
        ("metadata", json!({"k": "v"})),
        ("n", json!(1)),
        ("parallel_tool_calls", json!(false)),
        ("response_format", json!({"type": "json_object"})),
        ("seed", json!(3)),
        ("service_tier", json!("flex")),
        ("stop", json!(["END"])),
        ("stream_options", json!({"include_obfuscation": false})),
        ("user", json!("u")),
        ("store", json!(false)),
        ("include", json!(["reasoning.encrypted_content"])),
        ("previous_response_id", json!("resp_1")),
    ] {
        body[key] = value;
    }
    assert_request_parity(body.clone());
    body["model"] = json!("o3-mini");
    assert_request_parity(body);
    // 没有工具时 tool_choice / parallel_tool_calls 一并删除
    assert_request_parity(json!({
        "model": "m",
        "input": "hi",
        "tools": [{"type": "web_search"}],
        "tool_choice": "required",
        "parallel_tool_calls": true
    }));
}

#[test]
fn request_reasoning_configs_match_legacy() {
    let configs: Vec<Option<CodexChatReasoningConfig>> = vec![
        None,
        Some(CodexChatReasoningConfig::default()),
        Some(CodexChatReasoningConfig {
            supports_thinking: Some(true),
            ..Default::default()
        }),
        Some(CodexChatReasoningConfig {
            supports_effort: Some(true),
            effort_value_mode: Some("deepseek".into()),
            ..Default::default()
        }),
        Some(CodexChatReasoningConfig {
            supports_thinking: Some(true),
            supports_effort: Some(true),
            thinking_param: Some("enable_thinking".into()),
            effort_param: Some("reasoning.effort".into()),
            effort_value_mode: Some("openrouter".into()),
            ..Default::default()
        }),
        Some(CodexChatReasoningConfig {
            supports_effort: Some(true),
            thinking_param: Some("reasoning_split".into()),
            effort_value_mode: Some("low_high".into()),
            ..Default::default()
        }),
        Some(CodexChatReasoningConfig {
            supports_thinking: Some(true),
            thinking_param: Some("bogus".into()),
            effort_param: Some("bogus".into()),
            ..Default::default()
        }),
    ];
    let reasoning_values = [
        None,
        Some(json!(null)),
        Some(json!({})),
        Some(json!({"summary": "auto"})),
        Some(json!({"effort": "minimal"})),
        Some(json!({"effort": "low"})),
        Some(json!({"effort": "medium"})),
        Some(json!({"effort": "high"})),
        Some(json!({"effort": "xhigh"})),
        Some(json!({"effort": "max"})),
        Some(json!({"effort": "none"})),
        Some(json!({"effort": " OFF "})),
        Some(json!({"effort": "bogus"})),
    ];
    for model in ["gpt-5.4", "deepseek-reasoner", "o3"] {
        for config in &configs {
            for reasoning in &reasoning_values {
                let mut body = json!({"model": model, "input": "hi"});
                if let Some(reasoning) = reasoning {
                    body["reasoning"] = reasoning.clone();
                }
                assert_eq!(
                    request(&body, config.as_ref(), CodecEngine::Ir),
                    request(&body, config.as_ref(), CodecEngine::Legacy),
                    "model={model} config={config:?} reasoning={reasoning:?}"
                );
            }
        }
    }
}

// ---- 非流式响应 ----

#[test]
fn response_variants_match_legacy() {
    let message = |message: Value, finish_reason: Value| {
        json!({
            "id": "chatcmpl_9",
            "created": 99,
            "model": "deepseek-reasoner",
            "choices": [{"index": 0, "message": message, "finish_reason": finish_reason}],
            "usage": {"prompt_tokens": 20, "completion_tokens": 5, "total_tokens": 25, "prompt_tokens_details": {"cached_tokens": 8}, "completion_tokens_details": {"reasoning_tokens": 2}}
        })
    };
    for finish_reason in [
        json!("stop"),
        json!("length"),
        json!("tool_calls"),
        json!("content_filter"),
        json!(null),
    ] {
        assert_response_parity(message(
            json!({
                "role": "assistant",
                "reasoning_content": "I should call tools",
                "content": "Calling now.",
                "tool_calls": [
                    {"id": "call_1", "type": "function", "function": {"name": "exec", "arguments": "{\"b\":1,\"a\":2}"}},
                    {"type": "function", "function": {"name": "apply_patch", "arguments": "{\"input\":\"*** Begin\"}"}},
                    {"id": "", "type": "function", "function": {"name": "tool_search", "arguments": "{\"query\":\"x\"}"}},
                    {"id": "call_4", "type": "function", "function": {"arguments": "{}"}},
                    {"id": "call_5", "type": "function", "function": {"name": "mcp_files__read", "arguments": ""}},
                    {"id": "call_6", "type": "function", "function": {"name": "tool_search", "arguments": "not json"}}
                ]
            }),
            finish_reason,
        ));
    }
    for body in [
        message(
            json!({"content": "<think>\n inner \n</think>\n\nAnswer"}),
            json!("stop"),
        ),
        message(json!({"content": "<think>unterminated"}), json!("stop")),
        message(
            json!({"reasoning": {"content": "obj reasoning"}, "content": "x"}),
            json!("stop"),
        ),
        message(
            json!({"reasoning_details": [{"text": "d1"}, {"parts": [{"text": "d2"}]}], "content": ""}),
            json!("stop"),
        ),
        message(
            json!({"content": [{"type": "text", "text": "a"}, {"type": "output_text", "text": "b"}, {"type": "image_url"}]}),
            json!("stop"),
        ),
        message(
            json!({"content": null, "function_call": {"name": "exec", "arguments": {"cmd": "ls"}}}),
            json!("function_call"),
        ),
        message(
            json!({"content": null, "function_call": {"id": "fc", "name": "", "arguments": "{}"}}),
            json!("stop"),
        ),
        json!({"choices": [{"message": {"content": "no id"}}]}),
        json!({"id": "resp_existing", "choices": [{"message": {"content": "x"}}], "usage": "bad"}),
        json!({"choices": [{"message": {"content": "x"}}], "usage": {"prompt_tokens": 100, "completion_tokens": 1, "prompt_tokens_details": {"cached_tokens": 60, "cache_write_tokens": 30}}}),
        json!({"choices": []}),
        json!({"choices": [{}]}),
        json!({}),
    ] {
        assert_response_parity(body);
    }
}

// ---- 流式 ----

#[test]
fn stream_text_and_reasoning_match_legacy() {
    assert_stream_parity(&[
        &delta(json!({"role": "assistant", "content": ""})),
        &delta(json!({"reasoning_content": "Let me think"})),
        &delta(json!({"reasoning_content": " more"})),
        &delta(json!({"content": "Hello"})),
        &delta(json!({"content": " world"})),
        &finish(
            "stop",
            Some(
                json!({"prompt_tokens": 9, "completion_tokens": 3, "total_tokens": 12, "completion_tokens_details": {"reasoning_tokens": 1}}),
            ),
        ),
        DONE,
        DONE,
    ]);
    // reasoning 字段的其他形式
    assert_stream_parity(&[
        &delta(json!({"reasoning": "plain"})),
        &delta(json!({"reasoning": {"text": "object"}})),
        &delta(json!({"reasoning_details": [{"text": "details"}]})),
        &delta(json!({"content": "x"})),
        &finish("length", None),
        DONE,
    ]);
    // 只有 usage 的收尾 chunk，没有 [DONE]
    assert_stream_parity(&[
        &delta(json!({"content": "x"})),
        &finish("stop", None),
        &data(
            json!({"id": "chatcmpl_1", "choices": [], "usage": {"prompt_tokens": 100, "completion_tokens": 2, "prompt_tokens_details": {"cached_tokens": 50, "cache_write_tokens": 25}}}),
        ),
    ]);
}

#[test]
fn stream_inline_think_tags_match_legacy() {
    let content = |text: &str| delta(json!({"content": text}));
    // 开头的 <think> 被拆成 reasoning；标签跨 chunk
    assert_stream_parity(&[
        &content("  <th"),
        &content("ink>step one"),
        &content(" and two</thi"),
        &content("nk>\n\nThe answer"),
        &content(" is 4"),
        &finish("stop", None),
        DONE,
    ]);
    // 不是 <think> 开头
    assert_stream_parity(&[
        &content("<"),
        &content("b>bold"),
        &finish("stop", None),
        DONE,
    ]);
    assert_stream_parity(&[
        &content("   "),
        &content("plain"),
        &finish("stop", None),
        DONE,
    ]);
    // think 未闭合就遇到工具调用 / 结束
    assert_stream_parity(&[
        &content("<think>unfinished thought"),
        &tool_delta(0, Some("call_1"), Some("exec"), "{}"),
        &finish("tool_calls", None),
        DONE,
    ]);
    assert_stream_parity(&[&content("<think>never closed"), &finish("stop", None), DONE]);
    assert_stream_parity(&[
        &content("<think></think>"),
        &content("after"),
        &finish("stop", None),
        DONE,
    ]);
    assert_stream_parity(&[&content(" <thi"), &finish("stop", None), DONE]);
}

#[test]
fn stream_tool_calls_match_legacy() {
    // 并行工具调用：身份迟到、index 稀疏、参数交错
    assert_stream_parity(&[
        &delta(json!({"reasoning_content": "plan the calls"})),
        &tool_delta(1, Some("call_b"), None, "{\"x\":"),
        &tool_delta(0, None, Some("exec"), "{\"cmd\":"),
        &tool_delta(1, None, Some("read"), "1}"),
        &tool_delta(0, Some("call_a"), None, "\"ls\"}"),
        &delta(json!({"reasoning_content": " and more reasoning"})),
        &tool_delta(
            3,
            Some("call_d"),
            Some("apply_patch"),
            "{\"input\":\"*** Begin\"}",
        ),
        &tool_delta(2, None, None, "{\"lost\":true}"),
        &tool_delta(4, None, Some("tool_search"), "{\"query\":\"files\"}"),
        &tool_delta(5, Some("call_ns"), Some("mcp_files__read"), ""),
        &finish(
            "tool_calls",
            Some(json!({"prompt_tokens": 9, "completion_tokens": 4, "total_tokens": 13})),
        ),
        DONE,
    ]);
    // 文本 → 工具 → 继续文本与 reasoning
    assert_stream_parity(&[
        &delta(json!({"content": "Let me check."})),
        &tool_delta(0, Some("call_1"), Some("exec"), "{\"cmd\":\"ls\"}"),
        &delta(json!({"reasoning_content": "after the call"})),
        &delta(json!({"content": " More text."})),
        &finish("stop", None),
        DONE,
    ]);
    // 空 id / 空 name 的增量不覆盖已有身份
    assert_stream_parity(&[
        &tool_delta(0, Some("call_1"), Some("exec"), ""),
        &tool_delta(0, Some(""), Some(""), "{\"a\":1}"),
        &tool_delta(0, None, None, ""),
        &finish("tool_calls", None),
        DONE,
    ]);
}

#[test]
fn stream_errors_and_truncation_match_legacy() {
    // 没有 finish_reason：有输出按 length 收尾，没有输出报错
    assert_stream_parity(&[&delta(json!({"content": "partial"}))]);
    assert_stream_parity(&[&tool_delta(0, Some("call_1"), Some("exec"), "{\"a\":")]);
    assert_stream_parity(&[&tool_delta(0, None, None, "   ")]);
    assert_stream_parity(&[&delta(json!({"content": "<thin"}))]);
    assert_stream_parity(&[&delta(json!({"role": "assistant"}))]);
    assert_stream_parity(&[]);
    assert_stream_parity(&[DONE]);
    // 上游错误
    for error in [
        "event: error\ndata: {\"error\":{\"message\":\"rate limited\",\"type\":\"rate_limit\"}}\n\n".to_string(),
        data(json!({"error": {"message": "bad", "code": "server_error"}})),
        data(json!({"error": "plain string"})),
        data(json!({"error": {"detail": "detail only"}})),
        data(json!({"error": {"weird": 1}})),
        data(json!({"error": null})),
    ] {
        assert_stream_parity(&[&delta(json!({"content": "x"})), &error, &delta(json!({"content": "late"})), DONE]);
    }
    // 不是 JSON 的 data、注释与没有 data 的事件
    assert_stream_parity(&[
        ": comment\n\n",
        "event: ping\n\n",
        "data: not json\n\n",
        &delta(json!({"content": "ok"})),
        &finish("stop", None),
        DONE,
    ]);
}

// ---- 有意的差异 ----

#[test]
fn ir_engine_documented_differences() {
    // 拒绝内容在 IR 中是文本：Responses 输出为 output_text 而不是 refusal part
    let body = json!({"id": "c", "choices": [{"message": {"content": "hi", "refusal": "no"}, "finish_reason": "stop"}]});
    let ir =
        chat_to_responses_response(body.clone(), &Default::default(), CodecEngine::Ir).unwrap();
    let legacy =
        chat_to_responses_response(body, &Default::default(), CodecEngine::Legacy).unwrap();
    assert_eq!(legacy["output"][0]["content"][1]["type"], json!("refusal"));
    assert_eq!(
        ir["output"][0]["content"][1],
        json!({"type": "output_text", "text": "no", "annotations": []})
    );

    // [DONE] 之后的数据一律忽略（旧转换器会在 response.completed 之后继续输出增量）
    let chunks: Vec<&[u8]> = vec![
        b"data: {\"choices\":[{\"delta\":{\"content\":\"a\"},\"finish_reason\":\"stop\"}]}\n\n",
        b"data: [DONE]\n\n",
        b"data: {\"choices\":[{\"delta\":{\"content\":\"late\"}}]}\n\n",
    ];
    let ir = stream(&chunks, None, &json!({}), CodecEngine::Ir);
    let legacy = stream(&chunks, None, &json!({}), CodecEngine::Legacy);
    assert!(ir.trim_end().ends_with('}') && !ir.contains("late"), "{ir}");
    assert!(legacy.contains("late"));

    // usage：上游的 total_tokens / completion_tokens_details 其余字段与顶层
    // cache_read_input_tokens 别名不再原样透传，token 数相同
    let usage_body = json!({"choices": [{"message": {"content": "x"}}], "usage": {"prompt_tokens": 10, "completion_tokens": 2, "total_tokens": 99, "cache_read_input_tokens": 4, "completion_tokens_details": {"audio_tokens": 1}}});
    let ir = chat_to_responses_response(usage_body.clone(), &Default::default(), CodecEngine::Ir)
        .unwrap();
    let legacy =
        chat_to_responses_response(usage_body, &Default::default(), CodecEngine::Legacy).unwrap();
    assert_eq!(legacy["usage"]["total_tokens"], json!(99));
    assert_eq!(ir["usage"]["total_tokens"], json!(12));
    assert_eq!(ir["usage"]["input_tokens"], legacy["usage"]["input_tokens"]);
    assert_eq!(
        ir["usage"]["input_tokens_details"],
        legacy["usage"]["input_tokens_details"]
    );
}
