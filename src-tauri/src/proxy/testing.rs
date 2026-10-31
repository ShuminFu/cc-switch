//! Test-only harness for the protocol bridges (PROV-20 / TEST-02 / PRODUCT-04).
//!
//! * [`golden`] runs every fixture under `src-tauri/tests/fixtures/proxy/` through the
//!   matching conversion function and compares the **whole** converted JSON object (or the
//!   **whole** SSE event sequence) against the checked-in `*.expected.json` file.
//! * [`roundtrip`] holds `proptest` property tests for the request transforms: an
//!   Anthropic request pushed through another protocol (and back, where a reverse
//!   transform exists) keeps its message text, tool ids and ordering.
//!
//! Fixture layout: `tests/fixtures/proxy/<bridge>/<kind>/<case>.input.{json,sse}` and
//! `<case>.expected.json`, where `<kind>` is `request`, `response` or `stream`.
//! Run with `UPDATE_GOLDEN=1 cargo test --lib proxy::testing` to rewrite the expected
//! files after an intentional change, then review the diff. A case whose current output
//! is known to be wrong is listed with `#[ignore = "<bug>"]`; its expected file holds the
//! correct shape so the test turns green once the bug is fixed.

use bytes::Bytes;
use futures::stream::{self, StreamExt};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// Root of the proxy fixture corpus.
pub(crate) fn fixtures_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("proxy")
}

/// Whether `UPDATE_GOLDEN=1` asks the runner to rewrite the expected files.
fn update_golden() -> bool {
    std::env::var("UPDATE_GOLDEN").is_ok_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
}

/// Marker line in an input `.sse` fixture: everything before it is delivered as bytes,
/// then the upstream body stream yields an I/O error (a mid-stream disconnect).
pub(crate) const TRANSPORT_ERROR_MARKER: &str = ": x-fixture-transport-error";

/// Split raw SSE text into `{"event": .., "data": ..}` objects. `data` is parsed as JSON
/// when possible (`[DONE]` and other non-JSON payloads stay strings). Comment-only blocks
/// are kept as `{"comment": ..}` so keep-alives are visible in the expected sequence.
pub(crate) fn parse_sse_events(text: &str) -> Vec<Value> {
    let normalized = text.replace("\r\n", "\n");
    let mut events = Vec::new();
    for block in normalized.split("\n\n") {
        if block.trim().is_empty() {
            continue;
        }
        let mut event_name: Option<String> = None;
        let mut data_lines: Vec<&str> = Vec::new();
        let mut comments: Vec<&str> = Vec::new();
        for line in block.lines() {
            if let Some(rest) = line.strip_prefix("event:") {
                event_name = Some(rest.trim().to_string());
            } else if let Some(rest) = line.strip_prefix("data:") {
                data_lines.push(rest.strip_prefix(' ').unwrap_or(rest));
            } else if let Some(rest) = line.strip_prefix(':') {
                comments.push(rest.trim());
            }
        }
        let mut obj = serde_json::Map::new();
        if let Some(name) = event_name {
            obj.insert("event".into(), Value::String(name));
        }
        if !data_lines.is_empty() {
            let raw = data_lines.join("\n");
            let data = serde_json::from_str::<Value>(&raw).unwrap_or(Value::String(raw));
            obj.insert("data".into(), data);
        }
        if obj.is_empty() && !comments.is_empty() {
            obj.insert("comment".into(), Value::String(comments.join("\n")));
        }
        if !obj.is_empty() {
            events.push(Value::Object(obj));
        }
    }
    events
}

/// Turn an input SSE fixture into upstream body chunks of at most `chunk_size` bytes
/// (0 = one chunk), ending in an I/O error when the fixture carries
/// [`TRANSPORT_ERROR_MARKER`].
pub(crate) fn sse_fixture_chunks(
    text: &str,
    chunk_size: usize,
) -> Vec<Result<Bytes, std::io::Error>> {
    let (body, transport_error) = match text.find(TRANSPORT_ERROR_MARKER) {
        Some(pos) => (&text[..pos], true),
        None => (text, false),
    };
    let bytes = body.as_bytes();
    let mut chunks: Vec<Result<Bytes, std::io::Error>> = if chunk_size == 0 {
        vec![Ok(Bytes::copy_from_slice(bytes))]
    } else {
        bytes
            .chunks(chunk_size)
            .map(|c| Ok(Bytes::copy_from_slice(c)))
            .collect()
    };
    if transport_error {
        chunks.push(Err(std::io::Error::new(
            std::io::ErrorKind::ConnectionReset,
            "fixture: upstream connection reset",
        )));
    }
    chunks
}

/// Drain a converted SSE stream into an event list. A stream error becomes a final
/// `{"stream_error": "<message>"}` element.
pub(crate) async fn collect_stream<S>(stream: S) -> Vec<Value>
where
    S: futures::Stream<Item = Result<Bytes, std::io::Error>>,
{
    futures::pin_mut!(stream);
    let mut text = String::new();
    let mut error = None;
    while let Some(item) = stream.next().await {
        match item {
            Ok(bytes) => text.push_str(&String::from_utf8_lossy(&bytes)),
            Err(e) => {
                error = Some(e.to_string());
                break;
            }
        }
    }
    let mut events = parse_sse_events(&text);
    if let Some(error) = error {
        events.push(json!({ "stream_error": error }));
    }
    events
}

/// Replace ids the converters generate randomly (only Gemini's synthesized tool-call ids
/// today) with stable ordinal placeholders (`gemini_synth_<1>`, `gemini_synth_<2>`, ...)
/// so whole-object comparison is deterministic while distinct ids stay distinct and a
/// repeated id stays equal. Ids that come from the upstream payload pass through
/// untouched and are pinned by the fixtures.
pub(crate) fn normalize_dynamic(value: &mut Value) {
    fn walk(value: &mut Value, seen: &mut Vec<String>) {
        use crate::proxy::providers::transform_gemini::SYNTHESIZED_ID_PREFIX;
        match value {
            Value::Object(map) => map.values_mut().for_each(|v| walk(v, seen)),
            Value::Array(items) => items.iter_mut().for_each(|v| walk(v, seen)),
            Value::String(s) if s.starts_with(SYNTHESIZED_ID_PREFIX) => {
                let ordinal = match seen.iter().position(|known| known == s) {
                    Some(i) => i + 1,
                    None => {
                        seen.push(s.clone());
                        seen.len()
                    }
                };
                *s = format!("{SYNTHESIZED_ID_PREFIX}<{ordinal}>");
            }
            _ => {}
        }
    }
    walk(value, &mut Vec::new());
}

/// Compare `actual` with the expected fixture at `path`, or rewrite it under
/// `UPDATE_GOLDEN=1`. With `protect_existing` (known-bug cases whose expected file holds
/// the hand-written correct shape) an existing file is never overwritten.
pub(crate) fn assert_golden(path: &Path, actual: &Value, protect_existing: bool) {
    let rendered = serde_json::to_string_pretty(actual).expect("serialize actual") + "\n";
    if update_golden() && !(protect_existing && path.exists()) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create fixture dir");
        }
        std::fs::write(path, &rendered).expect("write golden file");
        return;
    }
    let expected_text = std::fs::read_to_string(path).unwrap_or_else(|e| {
        panic!(
            "missing golden file {} ({e}); run with UPDATE_GOLDEN=1 to create it\n--- actual ---\n{rendered}",
            path.display()
        )
    });
    let expected: Value = serde_json::from_str(&expected_text)
        .unwrap_or_else(|e| panic!("invalid JSON in {}: {e}", path.display()));
    if &expected != actual {
        panic!(
            "golden mismatch for {}\n--- expected ---\n{}\n--- actual ---\n{rendered}\n(run with UPDATE_GOLDEN=1 to accept the new output after reviewing it)",
            path.display(),
            serde_json::to_string_pretty(&expected).unwrap_or_default(),
        );
    }
}

pub(crate) mod golden {
    use super::*;
    use crate::proxy::providers::{
        streaming, streaming_codex_anthropic, streaming_codex_chat, streaming_gemini,
        streaming_responses, transform, transform_codex_anthropic, transform_codex_chat,
        transform_gemini, transform_responses,
    };
    use crate::proxy::ProxyError;

    #[derive(Clone, Copy, Debug)]
    pub(crate) enum Kind {
        Request,
        Response,
        Stream,
    }

    impl Kind {
        fn dir(self) -> &'static str {
            match self {
                Kind::Request => "request",
                Kind::Response => "response",
                Kind::Stream => "stream",
            }
        }
    }

    /// Mirrors the forwarder's `DEFAULT_CODEX_ANTHROPIC_MAX_TOKENS`.
    const CODEX_ANTHROPIC_DEFAULT_MAX_TOKENS: u64 = 8192;

    fn json_result(result: Result<Value, ProxyError>) -> Value {
        match result {
            Ok(mut v) => {
                normalize_dynamic(&mut v);
                v
            }
            Err(e) => json!({ "$error": e.to_string() }),
        }
    }

    fn convert_json(bridge: &str, kind: Kind, input: Value) -> Value {
        json_result(match (bridge, kind) {
            ("anthropic_chat", Kind::Request) => transform::anthropic_to_openai(input),
            ("anthropic_chat", Kind::Response) => transform::openai_to_anthropic(input),
            ("anthropic_responses", Kind::Request) => {
                transform_responses::anthropic_to_responses(input, None, false, false)
            }
            ("anthropic_responses", Kind::Response) => {
                transform_responses::responses_to_anthropic(input)
            }
            ("anthropic_gemini", Kind::Request) => transform_gemini::anthropic_to_gemini(input),
            ("anthropic_gemini", Kind::Response) => transform_gemini::gemini_to_anthropic(input),
            ("codex_chat", Kind::Request) => {
                transform_codex_chat::responses_to_chat_completions(input)
            }
            ("codex_chat", Kind::Response) => {
                transform_codex_chat::chat_completion_to_response(input)
            }
            ("codex_anthropic", Kind::Request) => {
                transform_codex_anthropic::responses_request_to_anthropic(
                    input,
                    CODEX_ANTHROPIC_DEFAULT_MAX_TOKENS,
                )
            }
            ("codex_anthropic", Kind::Response) => {
                transform_codex_anthropic::anthropic_response_to_responses(input)
            }
            other => panic!("unknown JSON bridge {other:?}"),
        })
    }

    async fn convert_stream(bridge: &str, input: &str, chunk_size: usize) -> Vec<Value> {
        let upstream = stream::iter(sse_fixture_chunks(input, chunk_size));
        let events = match bridge {
            "anthropic_chat" => {
                collect_stream(streaming::create_anthropic_sse_stream(upstream)).await
            }
            "anthropic_responses" => {
                collect_stream(
                    streaming_responses::create_anthropic_sse_stream_from_responses(upstream),
                )
                .await
            }
            "anthropic_gemini" => {
                collect_stream(streaming_gemini::create_anthropic_sse_stream_from_gemini(
                    upstream, None, None, None, None,
                ))
                .await
            }
            "codex_chat" => {
                collect_stream(streaming_codex_chat::create_responses_sse_stream_from_chat(
                    upstream,
                ))
                .await
            }
            "codex_anthropic" => {
                collect_stream(
                    streaming_codex_anthropic::create_responses_sse_stream_from_anthropic(upstream),
                )
                .await
            }
            other => panic!("unknown stream bridge {other:?}"),
        };
        let mut array = Value::Array(events);
        normalize_dynamic(&mut array);
        match array {
            Value::Array(events) => events,
            _ => unreachable!(),
        }
    }

    fn case_path(bridge: &str, kind: Kind, case: &str, suffix: &str) -> PathBuf {
        fixtures_root()
            .join(bridge)
            .join(kind.dir())
            .join(format!("{case}.{suffix}"))
    }

    /// Run one fixture case and compare against its expected file.
    pub(crate) fn run_case(bridge: &str, kind: Kind, case: &str, known_bug: Option<&str>) {
        let expected_path = case_path(bridge, kind, case, "expected.json");
        let actual = match kind {
            Kind::Request | Kind::Response => {
                let input_path = case_path(bridge, kind, case, "input.json");
                let text = std::fs::read_to_string(&input_path)
                    .unwrap_or_else(|e| panic!("read {}: {e}", input_path.display()));
                let input: Value = serde_json::from_str(&text)
                    .unwrap_or_else(|e| panic!("invalid JSON in {}: {e}", input_path.display()));
                convert_json(bridge, kind, input)
            }
            Kind::Stream => {
                let input_path = case_path(bridge, kind, case, "input.sse");
                let text = std::fs::read_to_string(&input_path)
                    .unwrap_or_else(|e| panic!("read {}: {e}", input_path.display()));
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("runtime");
                let whole = rt.block_on(convert_stream(bridge, &text, 0));
                // The same bytes split at arbitrary boundaries (including inside UTF-8
                // sequences and `data:` lines) must produce the identical event sequence.
                let chunked = rt.block_on(convert_stream(bridge, &text, 7));
                assert_eq!(
                    serde_json::to_string_pretty(&whole).unwrap(),
                    serde_json::to_string_pretty(&chunked).unwrap(),
                    "{bridge}/{case}: output depends on upstream chunk boundaries"
                );
                Value::Array(whole)
            }
        };
        if let Some(bug) = known_bug {
            eprintln!("{bridge}/{}/{case} is a known bug: {bug}", kind.dir());
        }
        assert_golden(&expected_path, &actual, known_bug.is_some());
    }

    /// Every case directory entry must be listed below; this catches fixtures that were
    /// added on disk but never wired into a test.
    pub(crate) fn assert_corpus_is_wired(listed: &[(&str, Kind, &str)]) {
        let mut on_disk = Vec::new();
        for bridge in std::fs::read_dir(fixtures_root()).expect("read fixtures root") {
            let bridge = bridge.expect("entry").path();
            if !bridge.is_dir() {
                continue;
            }
            for kind in std::fs::read_dir(&bridge).expect("read bridge dir") {
                let kind = kind.expect("entry").path();
                if !kind.is_dir() {
                    continue;
                }
                for file in std::fs::read_dir(&kind).expect("read kind dir") {
                    let file = file.expect("entry").path();
                    let name = file.file_name().unwrap().to_string_lossy().to_string();
                    if let Some(case) = name
                        .strip_suffix(".input.json")
                        .or_else(|| name.strip_suffix(".input.sse"))
                    {
                        on_disk.push(format!(
                            "{}/{}/{case}",
                            bridge.file_name().unwrap().to_string_lossy(),
                            kind.file_name().unwrap().to_string_lossy()
                        ));
                    }
                }
            }
        }
        on_disk.sort();
        let mut wired: Vec<String> = listed
            .iter()
            .map(|(b, k, c)| format!("{b}/{}/{c}", k.dir()))
            .collect();
        wired.sort();
        assert_eq!(on_disk, wired, "fixture corpus and golden test list differ");
    }

    /// `name => "bridge", Kind, "case";` declares one golden test. Appending
    /// `known_bug = "<finding id>: <what is wrong>"` marks the test `#[ignore]` with that
    /// reason; its expected file is hand-written with the correct shape and is never
    /// rewritten by `UPDATE_GOLDEN=1`.
    macro_rules! golden_cases {
        ($( $name:ident => $bridge:literal, $kind:ident, $case:literal $(, known_bug = $bug:literal)?; )*) => {
            $(
                #[test]
                $(#[ignore = $bug])?
                fn $name() {
                    let known_bug: Option<&str> = None $(.or(Some($bug)))?;
                    run_case($bridge, Kind::$kind, $case, known_bug);
                }
            )*

            #[test]
            fn corpus_is_fully_wired() {
                assert_corpus_is_wired(&[ $( ($bridge, Kind::$kind, $case), )* ]);
            }
        };
    }

    golden_cases! {
        anthropic_chat_request_image_base64 => "anthropic_chat", Request, "image_base64";
        anthropic_chat_request_text_with_system => "anthropic_chat", Request, "text_with_system";
        anthropic_chat_request_thinking_history => "anthropic_chat", Request, "thinking_history";
        anthropic_chat_request_tool_use_roundtrip => "anthropic_chat", Request, "tool_use_roundtrip";
        anthropic_chat_request_image_url_source => "anthropic_chat", Request, "image_url_source";
        anthropic_chat_response_tool_call_missing_id => "anthropic_chat", Response, "tool_call_missing_id";
        anthropic_chat_response_error_envelope => "anthropic_chat", Response, "error_envelope";
        anthropic_chat_response_length_truncated => "anthropic_chat", Response, "length_truncated";
        anthropic_chat_response_reasoning_content => "anthropic_chat", Response, "reasoning_content";
        anthropic_chat_response_reasoning_field => "anthropic_chat", Response, "reasoning_field";
        anthropic_chat_response_text => "anthropic_chat", Response, "text";
        anthropic_chat_response_tool_calls => "anthropic_chat", Response, "tool_calls";
        anthropic_chat_stream_double_finish_reason => "anthropic_chat", Stream, "double_finish_reason";
        anthropic_chat_stream_error_event => "anthropic_chat", Stream, "error_event";
        anthropic_chat_stream_reasoning_content => "anthropic_chat", Stream, "reasoning_content";
        anthropic_chat_stream_text_utf8 => "anthropic_chat", Stream, "text_utf8";
        anthropic_chat_stream_tool_calls_with_index => "anthropic_chat", Stream, "tool_calls_with_index";
        anthropic_chat_stream_tool_calls_without_index => "anthropic_chat", Stream, "tool_calls_without_index";
        anthropic_chat_stream_transport_error_mid_stream => "anthropic_chat", Stream, "transport_error_mid_stream";
        anthropic_chat_stream_truncated_no_done => "anthropic_chat", Stream, "truncated_no_done";
        anthropic_gemini_request_image_base64 => "anthropic_gemini", Request, "image_base64";
        anthropic_gemini_request_text_with_system => "anthropic_gemini", Request, "text_with_system";
        anthropic_gemini_request_thinking_history => "anthropic_gemini", Request, "thinking_history";
        anthropic_gemini_request_tool_use_roundtrip => "anthropic_gemini", Request, "tool_use_roundtrip";
        anthropic_gemini_response_function_call => "anthropic_gemini", Response, "function_call";
        anthropic_gemini_response_function_call_with_id => "anthropic_gemini", Response, "function_call_with_id";
        anthropic_gemini_response_max_tokens => "anthropic_gemini", Response, "max_tokens";
        anthropic_gemini_response_prompt_blocked => "anthropic_gemini", Response, "prompt_blocked";
        anthropic_gemini_response_text => "anthropic_gemini", Response, "text";
        anthropic_gemini_response_thought => "anthropic_gemini", Response, "thought";
        anthropic_gemini_stream_function_call => "anthropic_gemini", Stream, "function_call";
        anthropic_gemini_stream_parallel_no_id_calls => "anthropic_gemini", Stream, "parallel_no_id_calls";
        anthropic_gemini_stream_text_utf8 => "anthropic_gemini", Stream, "text_utf8";
        anthropic_gemini_stream_thought => "anthropic_gemini", Stream, "thought";
        anthropic_gemini_stream_transport_error_mid_stream => "anthropic_gemini", Stream, "transport_error_mid_stream";
        anthropic_gemini_stream_truncated_no_finish => "anthropic_gemini", Stream, "truncated_no_finish";
        anthropic_responses_request_image_base64 => "anthropic_responses", Request, "image_base64";
        anthropic_responses_request_text_with_system => "anthropic_responses", Request, "text_with_system";
        anthropic_responses_request_thinking_history => "anthropic_responses", Request, "thinking_history";
        anthropic_responses_request_tool_use_roundtrip => "anthropic_responses", Request, "tool_use_roundtrip";
        anthropic_responses_request_image_url_source => "anthropic_responses", Request, "image_url_source";
        anthropic_responses_response_function_call => "anthropic_responses", Response, "function_call";
        anthropic_responses_response_incomplete_max_tokens => "anthropic_responses", Response, "incomplete_max_tokens";
        anthropic_responses_response_reasoning => "anthropic_responses", Response, "reasoning";
        anthropic_responses_response_text => "anthropic_responses", Response, "text";
        anthropic_responses_stream_done_only_text => "anthropic_responses", Stream, "done_only_text";
        anthropic_responses_stream_function_call => "anthropic_responses", Stream, "function_call";
        anthropic_responses_stream_reasoning => "anthropic_responses", Stream, "reasoning";
        anthropic_responses_stream_response_failed => "anthropic_responses", Stream, "response_failed";
        anthropic_responses_stream_text => "anthropic_responses", Stream, "text";
        anthropic_responses_stream_transport_error_mid_stream => "anthropic_responses", Stream, "transport_error_mid_stream";
        anthropic_responses_stream_truncated_no_completed => "anthropic_responses", Stream, "truncated_no_completed";
        codex_anthropic_request_function_call_history => "codex_anthropic", Request, "function_call_history";
        codex_anthropic_request_image_input => "codex_anthropic", Request, "image_input";
        codex_anthropic_request_reasoning_history => "codex_anthropic", Request, "reasoning_history";
        codex_anthropic_request_text_with_instructions => "codex_anthropic", Request, "text_with_instructions";
        codex_anthropic_response_max_tokens => "codex_anthropic", Response, "max_tokens";
        codex_anthropic_response_text => "codex_anthropic", Response, "text";
        codex_anthropic_response_thinking => "codex_anthropic", Response, "thinking";
        codex_anthropic_response_tool_use => "codex_anthropic", Response, "tool_use";
        codex_anthropic_stream_error_event => "codex_anthropic", Stream, "error_event";
        codex_anthropic_stream_text_utf8 => "codex_anthropic", Stream, "text_utf8";
        codex_anthropic_stream_thinking => "codex_anthropic", Stream, "thinking";
        codex_anthropic_stream_tool_use => "codex_anthropic", Stream, "tool_use";
        codex_anthropic_stream_transport_error_mid_stream => "codex_anthropic", Stream, "transport_error_mid_stream";
        codex_anthropic_stream_truncated_no_message_stop => "codex_anthropic", Stream, "truncated_no_message_stop";
        codex_chat_request_function_call_history => "codex_chat", Request, "function_call_history";
        codex_chat_request_image_input => "codex_chat", Request, "image_input";
        codex_chat_request_reasoning_history => "codex_chat", Request, "reasoning_history";
        codex_chat_request_text_with_instructions => "codex_chat", Request, "text_with_instructions";
        codex_chat_response_error_envelope => "codex_chat", Response, "error_envelope";
        codex_chat_response_length_truncated => "codex_chat", Response, "length_truncated";
        codex_chat_response_reasoning_content => "codex_chat", Response, "reasoning_content";
        codex_chat_response_text => "codex_chat", Response, "text";
        codex_chat_response_tool_calls => "codex_chat", Response, "tool_calls";
        codex_chat_stream_double_finish_reason => "codex_chat", Stream, "double_finish_reason";
        codex_chat_stream_error_event => "codex_chat", Stream, "error_event";
        codex_chat_stream_reasoning_content => "codex_chat", Stream, "reasoning_content";
        codex_chat_stream_text_utf8 => "codex_chat", Stream, "text_utf8";
        codex_chat_stream_tool_calls_with_index => "codex_chat", Stream, "tool_calls_with_index";
        codex_chat_stream_tool_calls_without_index => "codex_chat", Stream, "tool_calls_without_index";
        codex_chat_stream_transport_error_mid_stream => "codex_chat", Stream, "transport_error_mid_stream";
        codex_chat_stream_truncated_no_done => "codex_chat", Stream, "truncated_no_done";
    }
}

/// Property tests: an Anthropic conversation pushed through another protocol keeps its
/// text, tool ids, tool names/arguments and ordering.
///
/// Every target format is reduced to the same protocol-neutral projection (a flat list
/// of [`Atom`]s), so message re-grouping (e.g. Responses lifting tool calls into
/// top-level items, Chat splitting tool results into `role: tool` messages) does not
/// matter, but any lost, duplicated, reordered or rewritten atom does.
pub(crate) mod roundtrip {
    use crate::proxy::providers::{
        transform, transform_codex_anthropic, transform_codex_chat, transform_gemini,
        transform_responses,
    };
    use proptest::prelude::*;
    use serde_json::{json, Value};

    #[derive(Clone, Debug, PartialEq)]
    pub(crate) enum Atom {
        System(String),
        Text {
            role: &'static str,
            text: String,
        },
        ToolUse {
            id: String,
            name: String,
            input: Value,
        },
        ToolResult {
            id: String,
            text: String,
        },
    }

    fn arguments_value(raw: &Value) -> Value {
        match raw {
            Value::String(s) => serde_json::from_str(s).unwrap_or(Value::String(s.clone())),
            other => other.clone(),
        }
    }

    fn role_name(role: &str) -> &'static str {
        match role {
            "assistant" | "model" => "assistant",
            _ => "user",
        }
    }

    fn text_of_parts(content: &Value) -> Vec<String> {
        match content {
            Value::String(s) => vec![s.clone()],
            Value::Array(parts) => parts
                .iter()
                .filter_map(|p| p.get("text").and_then(Value::as_str).map(str::to_string))
                .collect(),
            _ => Vec::new(),
        }
    }

    /// Projection of an Anthropic Messages request.
    pub(crate) fn project_anthropic(body: &Value) -> Vec<Atom> {
        let mut atoms = Vec::new();
        match body.get("system") {
            Some(Value::String(s)) => atoms.push(Atom::System(s.clone())),
            Some(Value::Array(blocks)) => {
                let joined: Vec<String> = blocks
                    .iter()
                    .filter_map(|b| b.get("text").and_then(Value::as_str).map(str::to_string))
                    .collect();
                if !joined.is_empty() {
                    atoms.push(Atom::System(joined.join("\n")));
                }
            }
            _ => {}
        }
        for msg in body["messages"].as_array().into_iter().flatten() {
            let role = role_name(msg["role"].as_str().unwrap_or("user"));
            match &msg["content"] {
                Value::String(s) => atoms.push(Atom::Text {
                    role,
                    text: s.clone(),
                }),
                Value::Array(blocks) => {
                    for block in blocks {
                        match block["type"].as_str() {
                            Some("text") => atoms.push(Atom::Text {
                                role,
                                text: block["text"].as_str().unwrap_or("").to_string(),
                            }),
                            Some("tool_use") => atoms.push(Atom::ToolUse {
                                id: block["id"].as_str().unwrap_or("").to_string(),
                                name: block["name"].as_str().unwrap_or("").to_string(),
                                input: block["input"].clone(),
                            }),
                            Some("tool_result") => atoms.push(Atom::ToolResult {
                                id: block["tool_use_id"].as_str().unwrap_or("").to_string(),
                                text: text_of_parts(&block["content"]).join(""),
                            }),
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }
        atoms
    }

    /// Projection of an OpenAI Chat Completions request.
    pub(crate) fn project_chat(body: &Value) -> Vec<Atom> {
        let mut atoms = Vec::new();
        for msg in body["messages"].as_array().into_iter().flatten() {
            match msg["role"].as_str() {
                Some("system") | Some("developer") => {
                    atoms.push(Atom::System(text_of_parts(&msg["content"]).join("\n")))
                }
                Some("tool") => atoms.push(Atom::ToolResult {
                    id: msg["tool_call_id"].as_str().unwrap_or("").to_string(),
                    text: text_of_parts(&msg["content"]).join(""),
                }),
                Some(role) => {
                    let role = role_name(role);
                    for text in text_of_parts(&msg["content"]) {
                        atoms.push(Atom::Text { role, text });
                    }
                    for call in msg["tool_calls"].as_array().into_iter().flatten() {
                        atoms.push(Atom::ToolUse {
                            id: call["id"].as_str().unwrap_or("").to_string(),
                            name: call["function"]["name"].as_str().unwrap_or("").to_string(),
                            input: arguments_value(&call["function"]["arguments"]),
                        });
                    }
                }
                None => {}
            }
        }
        atoms
    }

    /// Projection of an OpenAI Responses request.
    pub(crate) fn project_responses(body: &Value) -> Vec<Atom> {
        let mut atoms = Vec::new();
        if let Some(instructions) = body.get("instructions").and_then(Value::as_str) {
            atoms.push(Atom::System(instructions.to_string()));
        }
        for item in body["input"].as_array().into_iter().flatten() {
            match item
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("message")
            {
                "message" => {
                    let raw_role = item["role"].as_str().unwrap_or("user");
                    if matches!(raw_role, "system" | "developer") {
                        atoms.push(Atom::System(text_of_parts(&item["content"]).join("\n")));
                        continue;
                    }
                    let role = role_name(raw_role);
                    for text in text_of_parts(&item["content"]) {
                        atoms.push(Atom::Text { role, text });
                    }
                }
                "function_call" => atoms.push(Atom::ToolUse {
                    id: item["call_id"].as_str().unwrap_or("").to_string(),
                    name: item["name"].as_str().unwrap_or("").to_string(),
                    input: arguments_value(&item["arguments"]),
                }),
                "function_call_output" => atoms.push(Atom::ToolResult {
                    id: item["call_id"].as_str().unwrap_or("").to_string(),
                    text: text_of_parts(&item["output"]).join(""),
                }),
                _ => {}
            }
        }
        atoms
    }

    /// Projection of a Gemini generateContent request.
    pub(crate) fn project_gemini(body: &Value) -> Vec<Atom> {
        let mut atoms = Vec::new();
        if let Some(parts) = body["systemInstruction"]["parts"].as_array() {
            let joined: Vec<String> = parts
                .iter()
                .filter_map(|p| p.get("text").and_then(Value::as_str).map(str::to_string))
                .collect();
            if !joined.is_empty() {
                atoms.push(Atom::System(joined.join("\n")));
            }
        }
        for content in body["contents"].as_array().into_iter().flatten() {
            let role = role_name(content["role"].as_str().unwrap_or("user"));
            for part in content["parts"].as_array().into_iter().flatten() {
                if let Some(text) = part.get("text").and_then(Value::as_str) {
                    atoms.push(Atom::Text {
                        role,
                        text: text.to_string(),
                    });
                } else if let Some(call) = part.get("functionCall") {
                    atoms.push(Atom::ToolUse {
                        id: call["id"].as_str().unwrap_or("").to_string(),
                        name: call["name"].as_str().unwrap_or("").to_string(),
                        input: call["args"].clone(),
                    });
                } else if let Some(resp) = part.get("functionResponse") {
                    let text = match &resp["response"]["content"] {
                        Value::String(s) => s.clone(),
                        other => text_of_parts(other).join(""),
                    };
                    atoms.push(Atom::ToolResult {
                        id: resp["id"].as_str().unwrap_or("").to_string(),
                        text,
                    });
                }
            }
        }
        atoms
    }

    // -----------------------------------------------------------------------
    // Generators
    // -----------------------------------------------------------------------

    /// Non-blank text without leading/trailing whitespace (transforms may trim).
    fn text() -> impl Strategy<Value = String> {
        "[A-Za-z0-9\u{4e00}-\u{4e20}][A-Za-z0-9 .,!?\u{4e00}-\u{4e20}]{0,24}[A-Za-z0-9.!?]"
    }

    fn tool_name() -> impl Strategy<Value = String> {
        prop_oneof![
            Just("get_weather".to_string()),
            Just("read_file".to_string()),
            Just("run_shell".to_string()),
        ]
    }

    #[derive(Clone, Debug)]
    struct Turn {
        user_text: Option<String>,
        assistant_text: Option<String>,
        /// (name, argument, result text)
        tool_calls: Vec<(String, String, String)>,
    }

    fn turn() -> impl Strategy<Value = Turn> {
        (
            proptest::option::of(text()),
            proptest::option::of(text()),
            proptest::collection::vec((tool_name(), text(), text()), 0..3),
        )
            .prop_map(|(user_text, assistant_text, tool_calls)| Turn {
                user_text,
                assistant_text,
                tool_calls,
            })
    }

    /// Build a valid Anthropic request: user/assistant alternate, every tool_use is
    /// answered by a tool_result (same order) in the next user message, and the
    /// conversation ends on a user message.
    fn anthropic_request() -> impl Strategy<Value = Value> {
        (
            proptest::option::of(text()),
            text(),
            proptest::collection::vec(turn(), 0..4),
            any::<bool>(),
        )
            .prop_map(|(system, first, turns, with_tools)| {
                let mut messages = vec![json!({"role": "user", "content": first})];
                let mut next_id = 0u32;
                let mut pending_results: Vec<Value> = Vec::new();
                for t in &turns {
                    let mut assistant = Vec::new();
                    if let Some(text) = &t.assistant_text {
                        assistant.push(json!({"type": "text", "text": text}));
                    }
                    if with_tools {
                        for (name, arg, result) in &t.tool_calls {
                            next_id += 1;
                            let id = format!("toolu_{next_id:04}");
                            assistant.push(json!({
                                "type": "tool_use", "id": id, "name": name,
                                "input": {"arg": arg}
                            }));
                            pending_results.push(json!({
                                "type": "tool_result", "tool_use_id": id, "content": result
                            }));
                        }
                    }
                    if assistant.is_empty() {
                        assistant.push(json!({"type": "text", "text": "ok"}));
                    }
                    messages.push(json!({"role": "assistant", "content": assistant}));
                    let mut user: Vec<Value> = std::mem::take(&mut pending_results);
                    match &t.user_text {
                        Some(text) => user.push(json!({"type": "text", "text": text})),
                        None if user.is_empty() => {
                            user.push(json!({"type": "text", "text": "continue"}))
                        }
                        None => {}
                    }
                    messages.push(json!({"role": "user", "content": user}));
                }
                let mut body = json!({
                    "model": "claude-sonnet-4-5",
                    "max_tokens": 1024,
                    "messages": messages,
                });
                if let Some(system) = system {
                    body["system"] = json!([{"type": "text", "text": system}]);
                }
                if with_tools {
                    body["tools"] = json!(["get_weather", "read_file", "run_shell"]
                        .iter()
                        .map(|name| json!({
                            "name": name,
                            "description": "test tool",
                            "input_schema": {
                                "type": "object",
                                "properties": {"arg": {"type": "string"}},
                                "required": ["arg"]
                            }
                        }))
                        .collect::<Vec<_>>());
                }
                body
            })
    }

    /// Anthropic Messages response with text and tool_use blocks.
    fn anthropic_response() -> impl Strategy<Value = Value> {
        (
            proptest::collection::vec(text(), 0..3),
            proptest::collection::vec((tool_name(), text()), 0..3),
            prop_oneof![Just("end_turn"), Just("max_tokens")],
        )
            .prop_map(|(texts, calls, stop)| {
                let mut content: Vec<Value> = texts
                    .iter()
                    .map(|t| json!({"type": "text", "text": t}))
                    .collect();
                for (i, (name, arg)) in calls.iter().enumerate() {
                    content.push(json!({
                        "type": "tool_use", "id": format!("toolu_r{i}"), "name": name,
                        "input": {"arg": arg}
                    }));
                }
                if content.is_empty() {
                    content.push(json!({"type": "text", "text": "ok"}));
                }
                let stop = if calls.is_empty() { stop } else { "tool_use" };
                json!({
                    "id": "msg_prop", "type": "message", "role": "assistant",
                    "model": "claude-sonnet-4-5", "content": content,
                    "stop_reason": stop, "stop_sequence": null,
                    "usage": {"input_tokens": 10, "output_tokens": 5}
                })
            })
    }

    fn response_content_atoms(body: &Value) -> Vec<Atom> {
        body["content"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|b| match b["type"].as_str() {
                Some("text") => Some(Atom::Text {
                    role: "assistant",
                    text: b["text"].as_str().unwrap_or("").to_string(),
                }),
                Some("tool_use") => Some(Atom::ToolUse {
                    id: b["id"].as_str().unwrap_or("").to_string(),
                    name: b["name"].as_str().unwrap_or("").to_string(),
                    input: b["input"].clone(),
                }),
                _ => None,
            })
            .collect()
    }

    /// Adjacent text atoms of one role may legitimately be merged by a transform
    /// (e.g. a Chat assistant message carries a single `content` string). Compare
    /// modulo that merge by concatenating runs of same-role text.
    fn merge_text_runs(atoms: Vec<Atom>, sep: &str) -> Vec<Atom> {
        let mut out: Vec<Atom> = Vec::new();
        for atom in atoms {
            if let (
                Some(Atom::Text {
                    role: prev_role,
                    text: prev,
                }),
                Atom::Text { role, text },
            ) = (out.last_mut(), &atom)
            {
                if *prev_role == *role {
                    prev.push_str(sep);
                    prev.push_str(text);
                    continue;
                }
            }
            out.push(atom);
        }
        out
    }

    fn config() -> ProptestConfig {
        ProptestConfig {
            cases: 128,
            failure_persistence: None,
            ..ProptestConfig::default()
        }
    }

    proptest! {
        #![proptest_config(config())]

        /// Anthropic -> Responses -> Anthropic is the identity on the projection.
        #[test]
        fn anthropic_responses_anthropic_roundtrip(body in anthropic_request()) {
            let expected = project_anthropic(&body);
            let responses =
                transform_responses::anthropic_to_responses(body, None, false, false).unwrap();
            prop_assert_eq!(&project_responses(&responses), &expected);
            let back =
                transform_codex_anthropic::responses_request_to_anthropic(responses, 8192).unwrap();
            prop_assert_eq!(project_anthropic(&back), expected);
        }

        /// Anthropic -> Chat keeps text, tool ids and order.
        #[test]
        fn anthropic_to_chat_preserves_projection(body in anthropic_request()) {
            let expected = merge_text_runs(project_anthropic(&body), "");
            let chat = transform::anthropic_to_openai(body).unwrap();
            prop_assert_eq!(merge_text_runs(project_chat(&chat), ""), expected);
        }

        /// Anthropic -> Responses -> Chat (the Codex bridge) agrees with the direct
        /// Anthropic -> Chat transform on the projection.
        #[test]
        fn anthropic_responses_chat_matches_direct_chat(body in anthropic_request()) {
            let direct = transform::anthropic_to_openai(body.clone()).unwrap();
            let responses =
                transform_responses::anthropic_to_responses(body, None, false, false).unwrap();
            let via = transform_codex_chat::responses_to_chat_completions(responses).unwrap();
            prop_assert_eq!(
                merge_text_runs(project_chat(&via), ""),
                merge_text_runs(project_chat(&direct), "")
            );
        }

        /// Anthropic -> Gemini keeps text, tool ids (functionCall.id /
        /// functionResponse.id), names, arguments and order.
        #[test]
        fn anthropic_to_gemini_preserves_projection(body in anthropic_request()) {
            let expected = project_anthropic(&body);
            let gemini = transform_gemini::anthropic_to_gemini(body).unwrap();
            prop_assert_eq!(project_gemini(&gemini), expected);
        }

        /// Anthropic response -> Responses response -> Anthropic response keeps the
        /// content blocks and stop reason.
        #[test]
        fn anthropic_response_responses_roundtrip(body in anthropic_response()) {
            let expected = response_content_atoms(&body);
            let stop = body["stop_reason"].clone();
            let responses =
                transform_codex_anthropic::anthropic_response_to_responses(body).unwrap();
            let back = transform_responses::responses_to_anthropic(responses).unwrap();
            prop_assert_eq!(response_content_atoms(&back), expected);
            prop_assert_eq!(&back["stop_reason"], &stop);
        }
    }

    /// PROV-35: converters must never hand the client (or the usage logger) an empty
    /// message id when the upstream omits one.
    #[test]
    fn converted_responses_never_have_empty_ids() {
        let chat = transform::openai_to_anthropic(json!({
            "object": "chat.completion", "model": "m",
            "choices": [{"index": 0, "message": {"role": "assistant", "content": "hi"},
                         "finish_reason": "stop"}]
        }))
        .unwrap();
        assert_ne!(chat["id"].as_str().unwrap_or(""), "", "chat -> anthropic");
        let gemini = transform_gemini::gemini_to_anthropic(json!({
            "candidates": [{"content": {"role": "model", "parts": [{"text": "hi"}]},
                            "finishReason": "STOP"}]
        }))
        .unwrap();
        assert_ne!(
            gemini["id"].as_str().unwrap_or(""),
            "",
            "gemini -> anthropic"
        );
    }
}
