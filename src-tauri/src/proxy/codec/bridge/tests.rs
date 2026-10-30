//! Anthropic ← OpenAI Chat 桥接：旧转换器与 IR 编解码的差分测试
//!
//! golden 语料之外再用手写输入覆盖旧转换器的各条分支，断言两种引擎输出逐字节相同；
//! 有意的差异集中在文末，逐条断言其行为。

use super::*;
use futures::StreamExt;
use serde_json::json;

fn request(body: &Value, preserve: bool, engine: CodecEngine) -> Value {
    anthropic_to_openai_chat_request(body.clone(), preserve, engine).expect("convert request")
}

#[track_caller]
fn assert_request_parity(body: Value) {
    for preserve in [false, true] {
        let legacy = request(&body, preserve, CodecEngine::Legacy);
        let ir = request(&body, preserve, CodecEngine::Ir);
        // 比较序列化文本：字段顺序也必须一致
        assert_eq!(
            serde_json::to_string_pretty(&ir).unwrap(),
            serde_json::to_string_pretty(&legacy).unwrap(),
            "preserve_reasoning_content={preserve}"
        );
    }
}

fn response(body: &Value, engine: CodecEngine) -> Result<String, String> {
    openai_chat_to_anthropic_response(body.clone(), engine)
        .map(|value| serde_json::to_string_pretty(&value).unwrap())
        .map_err(|error| error.to_string())
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
        openai_chat_to_anthropic_sse(futures::stream::iter(items), engine).collect(),
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

#[track_caller]
fn assert_stream_parity(chunks: &[&str]) {
    let chunks: Vec<&[u8]> = chunks.iter().map(|chunk| chunk.as_bytes()).collect();
    assert_stream_parity_bytes(&chunks);
}

/// 传输错误追加在上游末尾时同样比较；唯一有意的差异：消息已正常结束
/// （`message_stop` 之后）再出现的传输错误，旧转换器仍会补一个 error 事件，
/// IR 编码端在终止事件之后不再输出任何帧
#[track_caller]
fn assert_stream_parity_bytes(chunks: &[&[u8]]) {
    for transport_error in [None, Some("connection reset")] {
        let mut legacy = stream(chunks, transport_error, CodecEngine::Legacy);
        let ir = stream(chunks, transport_error, CodecEngine::Ir);
        if transport_error.is_some() {
            if let Some(end) = legacy.find(MESSAGE_STOP) {
                let tail = legacy.split_off(end + MESSAGE_STOP.len());
                assert!(
                    tail.is_empty() || tail.starts_with("event: error\n"),
                    "{tail}"
                );
            }
        }
        assert_eq!(ir, legacy, "transport_error={transport_error:?}");
    }
}

fn data(value: Value) -> String {
    format!("data: {value}\n\n")
}

const DONE: &str = "data: [DONE]\n\n";

// ---- 请求 ----

#[test]
fn request_system_variants_match_legacy() {
    assert_request_parity(json!({
        "model": "gpt-4o",
        "system": "x-anthropic-billing-header: cc_version=1; cch=abc\nYou are helpful.",
        "messages": [{"role": "user", "content": "hi"}]
    }));
    assert_request_parity(json!({
        "model": "gpt-4o",
        "system": [
            {"type": "text", "text": "x-anthropic-billing-header: cch=1"},
            {"type": "text", "text": "first", "cache_control": {"type": "ephemeral"}},
            {"type": "image"},
            {"type": "text", "text": "second\r\nline"}
        ],
        "messages": [
            {"role": "system", "content": "from messages"},
            {"role": "user", "content": "hi"}
        ]
    }));
    assert_request_parity(json!({
        "model": "gpt-4o",
        "system": "",
        "messages": [
            {"role": "user", "content": "hi"},
            {"role": "system", "content": "late system"}
        ]
    }));
}

#[test]
fn request_content_blocks_match_legacy() {
    assert_request_parity(json!({
        "model": "gpt-4o",
        "max_tokens": 512,
        "temperature": 0.25,
        "top_p": 0.9,
        "top_k": 40,
        "stop_sequences": ["END", "STOP"],
        "stream": true,
        "metadata": {"user_id": "u"},
        "messages": [
            {"role": "user", "content": [
                {"type": "text", "text": "look"},
                {"type": "image", "source": {"type": "base64", "media_type": "image/jpeg", "data": "AAA"}},
                {"type": "image", "source": {"type": "url", "url": "https://x/y.png"}},
                {"type": "image", "source": {"data": "BBB"}},
                {"type": "image", "source": {"type": "base64", "data": ""}},
                {"type": "image", "source": {"type": "url", "url": ""}},
                {"type": "image", "source": {"type": "file", "file_id": "f"}},
                {"type": "image"},
                {"type": "text"},
                {"type": "server_tool_use", "id": "s"}
            ]},
            {"role": "user", "content": [{"type": "text", "text": ""}]},
            {"role": "user", "content": ""},
            {"role": "user", "content": []}
        ]
    }));
}

#[test]
fn request_documents_match_legacy() {
    let doc =
        |source: Value, title: Value| json!({"type": "document", "source": source, "title": title});
    assert_request_parity(json!({
        "model": "gpt-4o",
        "messages": [{"role": "user", "content": [
            doc(json!({"type": "text", "data": "plain"}), json!("Notes")),
            doc(json!({"type": "text", "data": ""}), json!("Empty")),
            doc(json!({"type": "content", "content": [{"type": "text", "text": "a"}, {"type": "image"}, {"type": "text", "text": "b"}]}), json!(null)),
            doc(json!({"type": "content", "content": "inline"}), json!("")),
            doc(json!({"type": "content"}), json!(null)),
            doc(json!({"type": "base64", "media_type": "application/pdf", "data": "JVBER"}), json!("a.pdf")),
            doc(json!({"type": "base64", "data": "JVBER"}), json!(null)),
            doc(json!({"type": "base64", "media_type": "text/csv", "data": "YQ=="}), json!("data.csv")),
            doc(json!({"type": "base64", "media_type": "application/pdf", "data": ""}), json!(null)),
            doc(json!({"type": "url", "url": "https://x/doc.pdf"}), json!("Remote")),
            doc(json!({"type": "url", "url": "https://x/doc.pdf"}), json!(null)),
            doc(json!({"type": "url", "url": ""}), json!(null)),
            doc(json!({"type": "file", "file_id": "file_1"}), json!(null)),
            json!({"type": "document", "filename": "named.txt", "source": {"type": "text", "data": "via filename"}}),
            json!({"type": "document"})
        ]}]
    }));
}

#[test]
fn request_tool_history_matches_legacy() {
    assert_request_parity(json!({
        "model": "deepseek-chat",
        "messages": [
            {"role": "user", "content": "go"},
            {"role": "assistant", "content": [
                {"type": "thinking", "thinking": "plan", "signature": "sig"},
                {"type": "thinking", "thinking": ""},
                {"type": "redacted_thinking", "data": "opaque"},
                {"type": "text", "text": "calling"},
                {"type": "tool_use", "id": "call_1", "name": "read", "input": {"b": 1, "a": [1, 2]}},
                {"type": "tool_use", "id": "call_2", "name": "noop"}
            ]},
            {"role": "assistant", "content": [
                {"type": "redacted_thinking", "data": "opaque"},
                {"type": "tool_use", "id": "call_3", "name": "x", "input": {}}
            ]},
            {"role": "assistant", "content": [
                {"type": "thinking", "thinking": "no tools here"},
                {"type": "text", "text": "done"}
            ]},
            {"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "call_1", "content": "ok"},
                {"type": "tool_result", "tool_use_id": "call_2", "is_error": true, "content": [
                    {"type": "text", "text": "line 1"},
                    {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "IMG"}},
                    {"type": "image", "source": {"type": "base64", "data": ""}},
                    {"type": "document", "source": {"type": "text", "data": "doc"}},
                    {"type": "search_result", "title": "t", "content": []},
                    {"type": "text"}
                ]},
                {"type": "tool_result", "tool_use_id": "call_3"},
                {"type": "tool_result", "tool_use_id": "call_4", "content": null},
                {"type": "tool_result", "tool_use_id": "call_5", "content": {"k": "v"}},
                {"type": "tool_result", "tool_use_id": "call_6", "content": 42},
                {"type": "tool_result", "tool_use_id": "call_7", "content": []},
                {"type": "text", "text": "and then"}
            ]},
            // assistant 消息里的工具结果图片单独成为一条 user 消息
            {"role": "assistant", "content": [
                {"type": "tool_result", "tool_use_id": "call_8", "content": [
                    {"type": "image", "source": {"type": "url", "url": "https://x/i.png"}}
                ]},
                {"type": "text", "text": "after"}
            ]}
        ]
    }));
}

#[test]
fn request_tools_and_choice_match_legacy() {
    let tools = json!([
        {"name": "search", "description": "Search", "input_schema": {
            "type": "object",
            "properties": {
                "url": {"type": "string", "format": "uri"},
                "list": {"type": "array", "items": {"type": "string", "format": "uri"}}
            }
        }},
        {"name": "bare"},
        {"name": "schema_without_type", "input_schema": {"properties": {"q": {"type": "string"}}}},
        {"type": "BatchTool", "name": "batch"},
        {"type": "bash_20250124", "name": "bash"}
    ]);
    let choices = [
        json!("auto"),
        json!("any"),
        json!("none"),
        json!({"type": "auto", "disable_parallel_tool_use": true}),
        json!({"type": "any"}),
        json!({"type": "none"}),
        json!({"type": "tool", "name": "search"}),
        json!({"type": "tool"}),
    ];
    for tool_choice in choices {
        assert_request_parity(json!({
            "model": "gpt-4o",
            "messages": [{"role": "user", "content": "hi"}],
            "tools": tools,
            "tool_choice": tool_choice
        }));
    }
    // 全部被过滤时不输出 tools
    assert_request_parity(json!({
        "model": "gpt-4o",
        "messages": [{"role": "user", "content": "hi"}],
        "tools": [{"type": "BatchTool", "name": "batch"}]
    }));
}

#[test]
fn request_reasoning_effort_matches_legacy() {
    let thinking = [
        None,
        Some(json!({"type": "enabled", "budget_tokens": 1024})),
        Some(json!({"type": "enabled", "budget_tokens": 8000})),
        Some(json!({"type": "enabled", "budget_tokens": 32000})),
        Some(json!({"type": "enabled"})),
        Some(json!({"type": "adaptive"})),
        Some(json!({"type": "disabled"})),
        Some(json!({"budget_tokens": 5000})),
    ];
    let efforts = [
        None,
        Some(json!("low")),
        Some(json!("medium")),
        Some(json!("high")),
        Some(json!("max")),
        Some(json!("bogus")),
        Some(json!(3)),
    ];
    for model in ["gpt-5.1", "o3-mini", "gpt-4o", "o1"] {
        for thinking in &thinking {
            for effort in &efforts {
                let mut body = json!({
                    "model": model,
                    "max_tokens": 2048,
                    "messages": [{"role": "user", "content": "hi"}]
                });
                if let Some(thinking) = thinking {
                    body["thinking"] = thinking.clone();
                }
                if let Some(effort) = effort {
                    body["output_config"] = json!({"effort": effort});
                }
                assert_request_parity(body);
            }
        }
    }
}

// ---- 非流式响应 ----

#[test]
fn response_variants_match_legacy() {
    let cases = [
        json!({
            "id": "chatcmpl-1",
            "model": "gpt-4o",
            "choices": [{"message": {"role": "assistant", "content": "hello"}, "finish_reason": "stop"}],
            "usage": {"prompt_tokens": 10, "completion_tokens": 2, "prompt_tokens_details": {"cached_tokens": 4}}
        }),
        json!({
            "choices": [{"message": {
                "reasoning_content": "think",
                "content": [
                    {"type": "text", "text": "a"},
                    {"type": "output_text", "text": "b"},
                    {"type": "text", "text": ""},
                    {"type": "refusal", "refusal": "no"},
                    {"type": "image_url"}
                ],
                "refusal": "top-level refusal"
            }, "finish_reason": "content_filter"}]
        }),
        json!({
            "choices": [{"message": {"content": null, "tool_calls": [
                {"id": "call_1", "type": "function", "function": {"name": "f", "arguments": "{\"z\":1,\"a\":2}"}},
                {"id": "call_2", "function": {"name": "g", "arguments": "not json"}},
                {"function": {"arguments": {"obj": true}}},
                {"id": "call_4"}
            ]}, "finish_reason": "stop"}],
            "usage": null
        }),
        json!({"choices": [{"message": {"tool_calls": [{"id": "c", "function": {"name": "f"}}]}}]}),
        json!({"choices": [{"message": {"tool_calls": [], "function_call": {"name": "legacy", "arguments": "{\"x\":1}"}}, "finish_reason": "function_call"}]}),
        json!({"choices": [{"message": {"function_call": {"id": "fc", "arguments": {"b": 2, "a": 1}}}}]}),
        json!({"choices": [{"message": {"function_call": {"arguments": [1, 2]}}}]}),
        json!({"choices": [{"message": {"function_call": {"arguments": 7}}}]}),
        json!({"choices": [{"message": {"function_call": {}}}]}),
        json!({"choices": [{"message": {"content": "cut"}, "finish_reason": "length"}], "usage": {"input_tokens": 3, "output_tokens": 9}}),
        json!({"choices": [{"message": {"content": "x"}, "finish_reason": "mystery"}]}),
        json!({"choices": [{"message": {"content": "x"}}]}),
        json!({"choices": [{"message": {"tool_calls": [{"id": "n", "function": {"name": "f", "arguments": "null"}}]}}]}),
        // 错误路径：消息文本与旧转换器一致
        json!({"error": {"message": "boom"}}),
        json!({"choices": []}),
        json!({"choices": [{"finish_reason": "stop"}]}),
    ];
    for case in cases {
        assert_response_parity(case);
    }
}

// ---- 流式 ----

#[test]
fn stream_text_reasoning_and_usage_match_legacy() {
    assert_stream_parity(&[
        // usage-only chunk before the first choice: not part of message_start
        &data(json!({"id": "", "choices": [], "usage": {"prompt_tokens": 1}})),
        &data(json!({"id": "c1", "model": "m", "choices": [{"delta": {"role": "assistant"}}]})),
        &data(json!({"id": "c2", "choices": [{"delta": {"reasoning_content": ""}}]})),
        &data(json!({"choices": [{"delta": {"reasoning": "step"}}]})),
        &data(json!({"choices": [{"delta": {"content": "Hello"}}]})),
        &data(json!({"choices": [{"delta": {"content": ""}}]})),
        &data(json!({"choices": [{"delta": {"reasoning": "again"}}]})),
        &data(json!({"choices": [{"delta": {"content": " world", "reasoning": "both"}}]})),
        &data(json!({"choices": [{"delta": null, "finish_reason": "stop"}], "usage": "bad"})),
        &data(
            json!({"choices": [], "usage": {"prompt_tokens": 20, "completion_tokens": 5, "prompt_tokens_details": {"cached_tokens": 8}}}),
        ),
        DONE,
        DONE,
        &data(json!({"choices": [{"delta": {"content": "after done"}}]})),
    ]);
    // message_start 带首个 chunk 的 usage
    assert_stream_parity(&[
        &data(
            json!({"id": "c1", "model": "m", "choices": [{"delta": {"content": "hi"}}], "usage": {"prompt_tokens": 7, "cache_read_input_tokens": 2}}),
        ),
        &data(json!({"choices": [{"delta": {}, "finish_reason": "length"}]})),
    ]);
}

#[test]
fn stream_tool_calls_match_legacy() {
    let call = |index: usize, id: Option<&str>, name: Option<&str>, args: Option<&str>| {
        let mut function = json!({});
        if let Some(name) = name {
            function["name"] = json!(name);
        }
        if let Some(args) = args {
            function["arguments"] = json!(args);
        }
        let mut call = json!({"index": index, "function": function});
        if let Some(id) = id {
            call["id"] = json!(id);
        }
        data(json!({"choices": [{"delta": {"tool_calls": [call]}}]}))
    };
    // 交错的并行调用、推迟到拿到 id+name 才开始、收尾时补发开始
    assert_stream_parity(&[
        &data(json!({"id": "c", "model": "m", "choices": [{"delta": {"content": "let me"}}]})),
        &call(1, None, None, Some("{\"late\":")),
        &call(0, Some("call_a"), Some("alpha"), Some("")),
        &call(1, None, None, Some("true}")),
        &call(0, None, None, Some("{\"x\":1}")),
        &call(2, Some(""), None, None),
        &call(3, Some("call_d"), None, Some("{}")),
        &call(4, None, None, None),
        &data(json!({"choices": [{"delta": {"content": "text after tools"}}]})),
        &data(json!({"choices": [{"finish_reason": "stop"}]})),
        &data(
            json!({"choices": [{"finish_reason": "tool_calls"}], "usage": {"prompt_tokens": 3, "completion_tokens": 4}}),
        ),
        DONE,
    ]);
    // 裸 [DONE]（无 finish_reason）且已有工具调用 → tool_use
    assert_stream_parity(&[&call(0, Some("call_a"), Some("alpha"), Some("{}")), DONE]);
    // 无 [DONE]：在流末尾收尾
    assert_stream_parity(&[
        &call(0, Some("call_a"), Some("alpha"), Some("{}")),
        &data(json!({"choices": [{"finish_reason": "tool_calls"}]})),
    ]);
    // 无 finish_reason 且有工具调用就 EOF → stream_truncated 错误
    assert_stream_parity(&[&call(0, Some("call_a"), Some("alpha"), Some("{\"a\":"))]);
}

#[test]
fn stream_infinite_whitespace_abort_matches_legacy() {
    let spaces = " ".repeat(300);
    let chunk = |args: &str| {
        data(json!({"choices": [{"delta": {"tool_calls": [
            {"index": 0, "id": "call_ws", "function": {"name": "ws", "arguments": args}}
        ]}}]}))
    };
    assert_stream_parity(&[
        &data(json!({"id": "c", "choices": [{"delta": {"content": "x"}}]})),
        &chunk("{\"a\":"),
        &chunk(&spaces),
        &chunk(&spaces),
        &chunk("1}"),
        &data(json!({"choices": [{"finish_reason": "tool_calls"}]})),
        DONE,
    ]);
}

#[test]
fn stream_errors_and_truncation_match_legacy() {
    // in-band error 字段
    assert_stream_parity(&[
        &data(json!({"id": "c", "choices": [{"delta": {"content": "partial"}}]})),
        &data(json!({"error": {"message": "rate limited", "type": "rate_limit_error"}})),
        &data(json!({"choices": [{"delta": {"content": "ignored"}}]})),
    ]);
    // event: error，各种 payload 形态
    for payload in [
        "{\"error\":{\"code\":\"overloaded\",\"message\":\"\"}}",
        "{\"message\":\"plain\"}",
        "\"just a string\"",
        "not json at all",
        "{}",
    ] {
        assert_stream_parity(&[&format!("event: error\ndata: {payload}\n\n")]);
    }
    // error: null 不是错误
    assert_stream_parity(&[
        &data(
            json!({"id": "c", "error": null, "choices": [{"delta": {"content": "ok"}, "finish_reason": "stop"}]}),
        ),
        DONE,
    ]);
    // 只有文本就 EOF → max_tokens；什么都没有就 EOF → stream_truncated
    assert_stream_parity(&[&data(json!({"choices": [{"delta": {"reasoning": "hmm"}}]}))]);
    assert_stream_parity(&[&data(
        json!({"id": "c", "choices": [{"delta": {"role": "assistant"}}]}),
    )]);
    assert_stream_parity(&[]);
    // 无效 JSON、无 choices 的 chunk 被忽略
    assert_stream_parity(&[
        "data: {not json\n\n",
        &data(json!({"object": "chat.completion.chunk"})),
        ": keep-alive comment\n\n",
        &data(json!({"choices": [{"delta": {"content": "x"}, "finish_reason": "stop"}]})),
        DONE,
    ]);
}

#[test]
fn stream_framing_matches_legacy() {
    // CRLF 分隔、一个块里多个 data 行、跨 chunk 切开的多字节字符
    let text = data(json!({"id": "c", "choices": [{"delta": {"content": "你好🌍"}}]}));
    // 切点落在 🌍（4 字节）中间
    let cut = text.find('🌍').unwrap() + 2;
    let (head, tail) = text.as_bytes().split_at(cut);
    assert_stream_parity_bytes(&[
        head,
        tail,
        b"data: {\"choices\":[{\"delta\":{\"content\":\"a\"}}]}\r\n\r\n",
        b"data: {\"choices\":[{\"delta\":{\"content\":\"b\"}}]}\ndata: {\"choices\":[{\"delta\":{\"content\":\"c\"}}]}\n\n",
        b"data:{\"choices\":[{\"finish_reason\":\"stop\"}]}\n\n",
        b"data:  [DONE]  \n\n",
    ]);
}

// ---- 有意的差异 ----

#[test]
fn ir_engine_documented_request_differences() {
    let body = json!({
        "model": "gpt-4o",
        "temperature": 1.0,
        "stream": false,
        "stop_sequences": [],
        "messages": [
            {"role": "developer", "content": "treated as user"},
            {"role": "user"}
        ]
    });
    let ir = request(&body, false, CodecEngine::Ir);
    let legacy = request(&body, false, CodecEngine::Legacy);
    // 整数值的浮点写成整数；stream=false 与空 stop 与缺省等价，不再转发
    assert_eq!(ir.get("temperature"), Some(&json!(1)));
    assert_eq!(legacy.get("temperature"), Some(&json!(1.0)));
    assert!(ir.get("stream").is_none() && ir.get("stop").is_none());
    assert_eq!(legacy["stream"], json!(false));
    // 未知角色按 user 处理；缺 content 的消息不转发
    assert_eq!(
        ir["messages"],
        json!([{"role": "user", "content": "treated as user"}])
    );
    assert_eq!(
        legacy["messages"],
        json!([
            {"role": "developer", "content": "treated as user"},
            {"role": "user", "content": null}
        ])
    );
}

#[test]
fn ir_engine_emits_nothing_after_a_completed_message() {
    let chunks: [&[u8]; 2] = [
        b"data: {\"choices\":[{\"delta\":{\"content\":\"x\"},\"finish_reason\":\"stop\"}]}\n\n",
        b"data: [DONE]\n\n",
    ];
    let ir = stream(&chunks, Some("reset after done"), CodecEngine::Ir);
    assert!(ir.ends_with(MESSAGE_STOP), "{ir}");
    let legacy = stream(&chunks, Some("reset after done"), CodecEngine::Legacy);
    assert!(legacy.ends_with("\"message\":\"Stream error: reset after done\"}}\n\n"));
}

#[test]
fn ir_engine_is_the_default_and_legacy_stays_a_fallback() {
    assert_eq!(CodecEngine::default(), CodecEngine::Ir);
    let body = json!({"model": "gpt-4o", "temperature": 1.0, "messages": []});
    assert_eq!(
        request(&body, false, CodecEngine::default()),
        request(&body, false, CodecEngine::Ir)
    );
    assert_eq!(
        request(&body, false, CodecEngine::Legacy),
        transform::anthropic_to_openai_with_reasoning_content(body, false).unwrap()
    );
}
