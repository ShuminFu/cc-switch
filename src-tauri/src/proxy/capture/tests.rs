use std::io::Read;
use std::time::Duration;

use bytes::Bytes;
use futures::StreamExt;
use http::{HeaderMap, HeaderValue, Method, Uri};
use regex::Regex;
use serde_json::{json, Value};

use super::redact::{redact_json, redact_text, REDACTED};
use super::*;
use crate::proxy::providers::testkit::{self, Bridge};

// ---------------------------------------------------------------------------
// Key corpus
// ---------------------------------------------------------------------------

const ANTHROPIC_KEY: &str = "sk-ant-api03-Zx9Qw7Er5Ty3Ui1Op8As6Df4Gh2Jk0LzXcVbNm-AbCdEf_GhIjKlMn";
const OPENAI_KEY: &str = "sk-proj-4fG7hJ2kL9mN3pQ6rS8tU1vW5xY0zA";
const PLAIN_SK_KEY: &str = "sk-1234567890abcdefghijklmnopqrstuv";
const GOOGLE_KEY: &str = "AIzaSyD-9tSrke72PouQMnMX-a7eZSW0jkFMBWY";
const GITHUB_USER_TOKEN: &str = concat!("ghu", "_16C7e42F292c6912E7710c838347Ae178B4a");
const GITHUB_PAT: &str = concat!("ghp", "_wWPw5k4aXcaT4fNP0UcnZwJUVFk6LO0pINUx");
const BEARER_TOKEN: &str = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJjYy1zd2l0Y2gifQ.c2lnbmF0dXJl";
const OPAQUE_API_KEY: &str = "opaque-gateway-key-0f3c9a7d";
const SESSION_COOKIE: &str = "session=8f2b6c1e9d0a; cf_clearance=Zq81kLmN0pQr";

/// Every literal secret seeded into the capture.
const SECRETS: &[&str] = &[
    ANTHROPIC_KEY,
    OPENAI_KEY,
    PLAIN_SK_KEY,
    GOOGLE_KEY,
    GITHUB_USER_TOKEN,
    GITHUB_PAT,
    BEARER_TOKEN,
    OPAQUE_API_KEY,
    "8f2b6c1e9d0a",
    "Zq81kLmN0pQr",
];

/// Shapes that must not survive the export, whatever the literal value.
fn leak_patterns() -> Vec<Regex> {
    [
        r"sk-[A-Za-z0-9_\-]{8,}",
        r"sk-ant-[A-Za-z0-9_\-]+",
        r"AIza[0-9A-Za-z_\-]{20,}",
        r"gh[pousr]_[A-Za-z0-9]{16,}",
        r"(?i)bearer\s+[A-Za-z0-9._~+/=\-]{8,}",
        r#"(?i)"x-api-key"\s*:\s*"[^"\[]"#,
        r#"(?i)"(set-)?cookie"\s*:\s*"[^"\[]"#,
        r"(?i)[?&]key=[^\[&\s]",
    ]
    .iter()
    .map(|pattern| Regex::new(pattern).expect("valid leak pattern"))
    .collect()
}

fn assert_no_leaks(label: &str, text: &str) {
    for secret in SECRETS {
        assert!(
            !text.contains(secret),
            "{label}: secret {secret:?} survived redaction:\n{text}"
        );
    }
    for pattern in leak_patterns() {
        assert!(
            pattern.find(text).is_none(),
            "{label}: pattern {} matched {:?}",
            pattern.as_str(),
            pattern.find(text).map(|m| m.as_str())
        );
    }
}

fn header_map(pairs: &[(&'static str, &str)]) -> HeaderMap {
    let mut headers = HeaderMap::new();
    for (name, value) in pairs {
        headers.append(*name, HeaderValue::from_str(value).expect("header value"));
    }
    headers
}

fn unzip(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("valid zip");
    (0..archive.len())
        .map(|index| {
            let mut file = archive.by_index(index).expect("zip entry");
            let mut contents = Vec::new();
            file.read_to_end(&mut contents).expect("read entry");
            (
                file.name().expect("utf-8 entry name").into_owned(),
                contents,
            )
        })
        .collect()
}

async fn feed(capture: &CaptureHandle, side: CaptureSide, chunks: &[&str]) {
    let items: Vec<Result<Bytes, std::io::Error>> = chunks
        .iter()
        .map(|chunk| Ok(Bytes::from(chunk.to_string())))
        .collect();
    let tapped = tap_stream(Some(capture.clone()), side, futures::stream::iter(items));
    let _: Vec<_> = tapped.collect().await;
}

/// A Gemini-bridge capture with the whole key corpus in headers, URLs and bodies.
async fn leaky_capture(store: &CaptureStore) -> CaptureHandle {
    store.arm(Duration::from_secs(60));
    let capture = store.begin("claude", "p1").expect("capture is armed");
    let client_headers = header_map(&[
        ("x-api-key", ANTHROPIC_KEY),
        ("authorization", &format!("Bearer {BEARER_TOKEN}")),
        ("cookie", SESSION_COOKIE),
        ("x-custom-auth-token", OPAQUE_API_KEY),
        ("anthropic-version", "2023-06-01"),
        ("x-debug-echo", &format!("upstream said key {OPENAI_KEY}")),
    ]);
    let client_body = json!({
        "model": "claude-sonnet-4-5",
        "max_tokens": 64,
        "stream": true,
        "metadata": { "api_key": OPAQUE_API_KEY, "user_id": "u-1" },
        "system": format!("Use {GITHUB_PAT} for GitHub and {GOOGLE_KEY} for maps."),
        "messages": [{
            "role": "user",
            "content": format!(
                "My keys: {PLAIN_SK_KEY}, {GITHUB_USER_TOKEN}. Header was \
                 'Authorization: Bearer {BEARER_TOKEN}' and Cookie: {SESSION_COOKIE}"
            )
        }]
    });
    capture.record_client_request(
        &Method::POST,
        &Uri::from_static("/v1/messages?beta=true"),
        &client_headers,
        &client_body,
    );
    capture.record_upstream_request(
        &Method::POST,
        &format!(
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-pro:streamGenerateContent?alt=sse&key={GOOGLE_KEY}"
        ),
        &header_map(&[
            ("x-goog-api-key", GOOGLE_KEY),
            ("content-type", "application/json"),
            ("set-cookie", SESSION_COOKIE),
        ]),
        &json!({
            "contents": [{ "role": "user", "parts": [{ "text": format!("key {ANTHROPIC_KEY}") }] }]
        }),
    );
    capture.set_route("p1", CaptureBridge::AnthropicGemini, false);
    capture.set_upstream_status(200);

    // The secret is split across two upstream chunks.
    let (head, tail) = OPENAI_KEY.split_at(10);
    let first = format!(
        "data: {{\"candidates\":[{{\"content\":{{\"role\":\"model\",\"parts\":[{{\"text\":\"leaked {head}"
    );
    let second = format!(
        "{tail} and {GITHUB_PAT} \\\"x-api-key\\\": \\\"{OPAQUE_API_KEY}\\\"\"}}]}}}}],\"usageMetadata\":{{\"promptTokenCount\":5,\"candidatesTokenCount\":3}},\"modelVersion\":\"gemini-2.5-pro\"}}\n\n"
    );
    let finish = "data: {\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"text\":\"\"}]},\"finishReason\":\"STOP\"}],\"usageMetadata\":{\"promptTokenCount\":5,\"candidatesTokenCount\":3,\"totalTokenCount\":8},\"modelVersion\":\"gemini-2.5-pro\"}\n\n";
    feed(&capture, CaptureSide::Upstream, &[&first, &second, finish]).await;
    feed(
        &capture,
        CaptureSide::Client,
        &[&format!(
            "event: content_block_delta\ndata: {{\"type\":\"content_block_delta\",\"index\":0,\"delta\":{{\"type\":\"text_delta\",\"text\":\"leaked {OPENAI_KEY} {GITHUB_USER_TOKEN}\"}}}}\n\nevent: message_stop\ndata: {{\"type\":\"message_stop\"}}\n\n"
        )],
    )
    .await;
    capture.set_request_id("session:msg_leaky");
    capture
}

// ---------------------------------------------------------------------------
// Redaction
// ---------------------------------------------------------------------------

#[test]
fn capture_redact_text_covers_every_key_shape() {
    let cases = [
        format!("anthropic {ANTHROPIC_KEY} end"),
        format!("openai {OPENAI_KEY}"),
        format!("plain {PLAIN_SK_KEY}"),
        format!("google {GOOGLE_KEY}"),
        format!("copilot {GITHUB_USER_TOKEN} / {GITHUB_PAT}"),
        format!("Authorization: Bearer {BEARER_TOKEN}"),
        format!("x-api-key: {OPAQUE_API_KEY}"),
        format!("Cookie: {SESSION_COOKIE}"),
        format!("Set-Cookie: {SESSION_COOKIE}"),
        format!(r#"{{"x-api-key":"{OPAQUE_API_KEY}","cookie":"{SESSION_COOKIE}"}}"#),
        format!("https://host/v1?alt=sse&key={GOOGLE_KEY}"),
    ];
    for case in &cases {
        let redacted = redact_text(case);
        assert!(redacted.contains(REDACTED), "{case} -> {redacted}");
        assert_no_leaks(case, &redacted);
    }
    // Ordinary text and token counts are left alone.
    let plain = r#"{"usage":{"input_tokens":12},"text":"ask-me risk-taking task-runner"}"#;
    assert_eq!(redact_text(plain), plain);
}

/// Raw SSE bytes keep JSON escapes, so a key at the start of a line follows
/// `\n` / `\t` with no word boundary in between.
#[test]
fn capture_redact_text_catches_keys_after_json_escapes() {
    for (escape, key) in [
        (r"\n", ANTHROPIC_KEY),
        (r"\n", PLAIN_SK_KEY),
        (r"\r\n", GOOGLE_KEY),
        (r"\t", GITHUB_PAT),
        (r"\n", GITHUB_USER_TOKEN),
        (r"\n", &format!("Bearer {BEARER_TOKEN}")),
        (r"\n", &format!("x-api-key: {OPAQUE_API_KEY}")),
        (r"\n", &format!("Cookie: {SESSION_COOKIE}")),
    ] {
        let raw = format!(r#"data: {{"text":"code:{escape}{key}{escape}done"}}"#);
        let redacted = redact_text(&raw);
        assert_no_leaks(&raw, &redacted);
        let payload = redacted.strip_prefix("data: ").expect("data line kept");
        let value: Value = serde_json::from_str(payload).expect("still JSON");
        assert!(
            value["text"].as_str().unwrap().contains(REDACTED),
            "{raw} -> {redacted}"
        );
    }
}

#[test]
fn capture_redaction_keeps_sse_and_json_parseable() {
    let line = format!(
        "data: {{\"type\":\"content_block_delta\",\"delta\":{{\"text\":\"key {ANTHROPIC_KEY} and Bearer {BEARER_TOKEN}\"}}}}\n\n"
    );
    let redacted = redact_text(&line);
    let payload = redacted
        .trim()
        .strip_prefix("data: ")
        .expect("data line kept");
    let value: Value = serde_json::from_str(payload).expect("still JSON");
    assert_eq!(
        value["delta"]["text"],
        format!("key {REDACTED} and Bearer {REDACTED}")
    );

    let body = json!({
        "api_key": OPAQUE_API_KEY,
        "nested": { "Authorization": BEARER_TOKEN, "count": 3, "token": { "kept": true } },
        "list": [format!("x {GOOGLE_KEY}")],
    });
    let redacted = redact_json(&body);
    assert_eq!(redacted["api_key"], REDACTED);
    assert_eq!(redacted["nested"]["Authorization"], REDACTED);
    assert_eq!(redacted["nested"]["count"], 3);
    // Only string values of sensitive keys are dropped: schemas stay intact.
    assert_eq!(redacted["nested"]["token"]["kept"], true);
    assert_eq!(redacted["list"][0], format!("x {REDACTED}"));
}

/// Acceptance: zero hits for the key corpus anywhere in the exported zip.
#[tokio::test]
async fn capture_export_has_zero_key_hits() {
    let store = CaptureStore::default();
    let capture = leaky_capture(&store).await;
    // Sanity check: the raw stream chunks still hold the secret before export.
    capture.with_record(|record| {
        let raw = String::from_utf8(record.upstream.concat()).expect("utf-8");
        assert!(raw.contains(OPENAI_KEY));
    });

    let zip = store.export_zip("session:msg_leaky").expect("export");
    let entries = unzip(&zip);
    assert!(
        entries.len() >= 5,
        "{:?}",
        entries.iter().map(|e| &e.0).collect::<Vec<_>>()
    );
    for (name, contents) in &entries {
        let text = String::from_utf8_lossy(contents);
        assert_no_leaks(name, &text);
    }
    // The zip as a whole (deflated) must not carry the secrets either.
    let raw = String::from_utf8_lossy(&zip);
    for secret in SECRETS {
        assert!(!raw.contains(secret));
    }
}

// ---------------------------------------------------------------------------
// Export layout and replay
// ---------------------------------------------------------------------------

#[tokio::test]
async fn capture_export_uses_the_golden_fixture_layout() {
    let store = CaptureStore::default();
    let capture = leaky_capture(&store).await;
    let entries = unzip(&store.export_zip("session:msg_leaky").expect("export"));
    let case = capture.with_record(export::case_name);
    let prefix = format!("anthropic__gemini/{case}/");
    let names: Vec<&str> = entries
        .iter()
        .map(|(name, _)| name.strip_prefix(&prefix).expect("case dir prefix"))
        .collect();
    for expected in [
        "meta.toml",
        "client_request.json",
        "upstream_response.sse",
        "captured_client_response.sse",
        "capture.json",
    ] {
        assert!(
            names.contains(&expected),
            "{expected} missing from {names:?}"
        );
    }

    let dir = tempfile::tempdir().expect("tempdir");
    for (name, contents) in &entries {
        let path = dir.path().join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }
    let fixture = testkit::load_fixture(
        Bridge::AnthropicGemini,
        &dir.path().join("anthropic__gemini").join(&case),
    )
    .expect("exported capture loads as a golden fixture");
    assert!(fixture.has_stream());
    assert!(fixture.client_request.is_some());
    assert!(!fixture.meta.source_commit.is_empty());

    let capture_json: Value = serde_json::from_slice(
        &entries
            .iter()
            .find(|(name, _)| name.ends_with("capture.json"))
            .unwrap()
            .1,
    )
    .unwrap();
    assert_eq!(capture_json["request_id"], "session:msg_leaky");
    assert_eq!(capture_json["upstream_status"], 200);
    assert_eq!(
        capture_json["upstream_response"]["chunks"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert_eq!(
        capture_json["upstream_request"]["headers"]["x-goog-api-key"],
        REDACTED
    );
}

/// Replays an existing golden case through a capture: the export must load
/// and pass every testkit check, i.e. run green through proxy_golden.
#[tokio::test]
async fn capture_of_a_golden_case_replays_green() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/proxy");
    let original = testkit::load_fixture(
        Bridge::AnthropicOpenAiChat,
        &root.join("anthropic__openai_chat/parity_text_stop"),
    )
    .expect("golden fixture");
    let sse = original.upstream_sse.clone().expect("stream body");

    let store = CaptureStore::default();
    store.arm(Duration::from_secs(60));
    let capture = store.begin("claude", "p1").expect("armed");
    capture.record_client_request(
        &Method::POST,
        &Uri::from_static("/v1/messages"),
        &HeaderMap::new(),
        &json!({
            "model": "claude-sonnet-4-5",
            "max_tokens": 32,
            "stream": true,
            "messages": [{ "role": "user", "content": "hi" }]
        }),
    );
    capture.set_route("p1", CaptureBridge::AnthropicOpenAiChat, false);
    // Split the body at an arbitrary offset, as the network would.
    let (head, tail) = sse.split_at(sse.len() / 3);
    feed(
        &capture,
        CaptureSide::Upstream,
        &[
            std::str::from_utf8(head).unwrap(),
            std::str::from_utf8(tail).unwrap(),
        ],
    )
    .await;
    capture.set_request_id("req-1");

    let entries = unzip(&store.export_zip("req-1").expect("export"));
    let dir = tempfile::tempdir().expect("tempdir");
    for (name, contents) in &entries {
        let path = dir.path().join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }
    let case = capture.with_record(export::case_name);
    let fixture = testkit::load_fixture(
        Bridge::AnthropicOpenAiChat,
        &dir.path().join("anthropic__openai_chat").join(case),
    )
    .expect("load export");
    assert_eq!(fixture.upstream_sse.as_deref(), Some(sse.as_slice()));
    let outcome = testkit::run_fixture(&fixture);
    assert!(outcome.failures.is_empty(), "{:#?}", outcome.failures);
    assert!(outcome.upstream_request.is_some());
    assert!(outcome.client_response.is_some());
}

/// A Gemini client capture keeps the model and stream flag from the request
/// URL (the body has neither) and replays through the gemini__anthropic bridge.
#[tokio::test]
async fn capture_export_of_a_gemini_client_round_trips_through_the_testkit() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/proxy");
    let original = testkit::load_fixture(
        Bridge::GeminiAnthropic,
        &root.join("gemini__anthropic/parity_text_stop"),
    )
    .expect("golden fixture");
    let sse = original.upstream_sse.clone().expect("stream body");

    let store = CaptureStore::default();
    store.arm(Duration::from_secs(60));
    let capture = store.begin("gemini", "g1").expect("armed");
    capture.record_client_request(
        &Method::POST,
        &Uri::from_static("/v1beta/models/claude-haiku-4-5:streamGenerateContent?alt=sse"),
        &HeaderMap::new(),
        original.client_request.as_ref().expect("client request"),
    );
    capture.set_route("g1", CaptureBridge::GeminiAnthropic, false);
    feed(
        &capture,
        CaptureSide::Upstream,
        &[std::str::from_utf8(&sse).unwrap()],
    )
    .await;
    capture.set_request_id("req-gemini");

    let entries = unzip(&store.export_zip("req-gemini").expect("export"));
    let dir = tempfile::tempdir().expect("tempdir");
    for (name, contents) in &entries {
        let path = dir.path().join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }
    let case = capture.with_record(export::case_name);
    let fixture = testkit::load_fixture(
        Bridge::GeminiAnthropic,
        &dir.path().join("gemini__anthropic").join(case),
    )
    .expect("load export");
    assert_eq!(
        fixture.meta.options.client_model.as_deref(),
        Some("claude-haiku-4-5")
    );
    assert_eq!(fixture.meta.options.client_stream, Some(true));
    let outcome = testkit::run_fixture(&fixture);
    assert!(outcome.failures.is_empty(), "{:#?}", outcome.failures);
    let request = outcome.upstream_request.expect("upstream request");
    assert!(
        request.contains("\"model\": \"claude-haiku-4-5\""),
        "{request}"
    );
    assert!(request.contains("\"stream\": true"), "{request}");
}

#[tokio::test]
async fn capture_export_records_terminal_and_upstream_error() {
    let store = CaptureStore::default();
    store.arm(Duration::from_secs(60));
    let capture = store.begin("codex", "p1").expect("armed");
    capture.set_route("p1", CaptureBridge::ResponsesChat, false);
    let items: Vec<Result<Bytes, std::io::Error>> = vec![
        Ok(Bytes::from_static(
            b"data: {\"id\":\"c1\",\"choices\":[{\"delta\":{\"content\":\"Hi\"}}]}\n\n",
        )),
        Err(std::io::Error::other(format!(
            "reset by peer, key {PLAIN_SK_KEY}"
        ))),
    ];
    let _: Vec<_> = tap_stream(
        Some(capture.clone()),
        CaptureSide::Upstream,
        futures::stream::iter(items),
    )
    .collect()
    .await;
    feed(
        &capture,
        CaptureSide::Client,
        &["event: response.created\ndata: {\"type\":\"response.created\"}\n\n"],
    )
    .await;
    capture.set_request_id("req-err");

    let entries = unzip(&store.export_zip("req-err").expect("export"));
    let meta = entries
        .iter()
        .find(|(name, _)| name.ends_with("meta.toml"))
        .map(|(_, contents)| String::from_utf8(contents.clone()).unwrap())
        .unwrap();
    let parsed: toml::Value = toml::from_str(&meta).expect("meta.toml parses");
    assert_eq!(parsed["invariants"]["terminal"].as_str(), Some("none"));
    assert_eq!(
        parsed["options"]["upstream_error"].as_str(),
        Some(format!("reset by peer, key {REDACTED}").as_str())
    );
    assert!(meta.starts_with("# Bug report captured by CC Switch"));
}

#[test]
fn capture_export_refuses_unbridged_and_unknown_requests() {
    let store = CaptureStore::default();
    assert!(store.export_zip("missing").is_err());

    store.arm(Duration::from_secs(60));
    let capture = store.begin("claude", "p1").expect("armed");
    capture.record_body(CaptureSide::Upstream, br#"{"id":"msg"}"#);
    capture.set_request_id("req-passthrough");
    let error = store.export_zip("req-passthrough").unwrap_err();
    assert!(error.to_string().contains("bridge") || error.to_string().contains("协议"));
}

// ---------------------------------------------------------------------------
// Store: opt-in, time box, bounds
// ---------------------------------------------------------------------------

#[test]
fn capture_is_opt_in_and_time_boxed() {
    let store = CaptureStore::default();
    assert!(store.begin("claude", "p1").is_none(), "disarmed by default");
    assert!(!store.status().active);

    let status = store.arm(Duration::from_millis(30));
    assert!(status.active);
    assert!(status.expires_at.is_some());
    assert!(store.begin("claude", "p1").is_some());

    std::thread::sleep(Duration::from_millis(60));
    assert!(store.begin("claude", "p1").is_none(), "expired window");
    let status = store.status();
    assert!(!status.active);
    assert_eq!(status.expires_at, None);

    // The arm duration is capped.
    let status = store.arm(Duration::from_secs(24 * 3600));
    assert!(status.remaining_secs <= u64::from(MAX_CAPTURE_MINUTES) * 60);
    assert!(!store.disarm().active);
    assert!(store.begin("claude", "p1").is_none());
}

#[test]
fn capture_store_keeps_the_latest_captures_and_drops_discarded_ones() {
    let store = CaptureStore::default();
    store.arm(Duration::from_secs(60));
    let mut handles = Vec::new();
    for index in 0..MAX_CAPTURES + 3 {
        let handle = store.begin("claude", "p1").expect("armed");
        handle.set_request_id(&format!("req-{index}"));
        handles.push(handle);
    }
    let ids = store.status().captured_request_ids;
    assert_eq!(ids.len(), MAX_CAPTURES);
    assert_eq!(ids[0], format!("req-{}", MAX_CAPTURES + 2), "newest first");
    assert!(store.find("req-0").is_none(), "oldest evicted");

    let last = handles.last().unwrap();
    store.discard(last);
    assert!(store.find(&format!("req-{}", MAX_CAPTURES + 2)).is_none());
}

#[test]
fn capture_body_is_bounded() {
    let mut body = CapturedBody::default();
    let chunk = Bytes::from(vec![b'a'; MAX_STREAM_BYTES / 2 + 1]);
    body.push(0, &chunk);
    body.push(1, &chunk);
    body.push(2, &chunk);
    assert!(body.truncated);
    assert_eq!(body.bytes, MAX_STREAM_BYTES);
    assert_eq!(body.concat().len(), MAX_STREAM_BYTES);
}

#[test]
fn capture_bridges_match_the_golden_testkit() {
    for bridge in CaptureBridge::ALL {
        assert!(
            Bridge::from_dir_name(bridge.dir_name()).is_some(),
            "{} is not a golden bridge directory",
            bridge.dir_name()
        );
    }
    assert_eq!(CaptureBridge::ALL.len(), Bridge::ALL.len());
    assert_eq!(
        CaptureBridge::from_decode(ResponseDecode::Passthrough),
        None
    );
    assert_eq!(
        CaptureBridge::from_decode(ResponseDecode::AnthropicFrom(ClaudeApiFormat::Anthropic)),
        None
    );
    assert_eq!(
        CaptureBridge::from_decode(ResponseDecode::ResponsesFromChat),
        Some(CaptureBridge::ResponsesChat)
    );
    assert_eq!(
        CaptureBridge::from_decode(ResponseDecode::GeminiFromAnthropic),
        Some(CaptureBridge::GeminiAnthropic)
    );
}
