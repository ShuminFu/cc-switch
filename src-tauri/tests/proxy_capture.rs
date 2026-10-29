//! Bug report capture end to end (CCS-138): a real proxy with capture armed
//! in front of a scripted bridge upstream. The exported zip must unpack into
//! the golden fixture layout, replay green through the testkit that drives
//! `tests/proxy_golden.rs`, reproduce the client stream the proxy sent, and
//! carry none of the keys that crossed the proxy.

use std::io::Read;
use std::path::Path;
use std::time::{Duration, Instant};

use cc_switch_lib::proxy_harness::{MockReply, MockUpstream, ProxyHarness};
use cc_switch_lib::proxy_testkit::{self as testkit, Bridge};
use serde_json::{json, Value};

#[path = "support.rs"]
mod support;
use support::TestEnv;

const MODEL: &str = "claude-sonnet-4-5";
/// A key the user pasted into the conversation.
const PASTED_KEY: &str = "sk-ant-api03-pastedPastedPasted0123456789abcdef";

fn chat_sse_chunks() -> Vec<String> {
    let chunks = [
        json!({ "id": "chatcmpl_capture", "model": "gpt-4o",
                "choices": [{ "index": 0, "delta": { "role": "assistant", "content": "Hello" } }] }),
        json!({ "id": "chatcmpl_capture", "model": "gpt-4o",
                "choices": [{ "index": 0, "delta": { "content": " world" } }] }),
        json!({ "id": "chatcmpl_capture", "model": "gpt-4o",
                "choices": [{ "index": 0, "delta": {}, "finish_reason": "stop" }] }),
        json!({ "id": "chatcmpl_capture", "model": "gpt-4o", "choices": [],
                "usage": { "prompt_tokens": 11, "completion_tokens": 2, "total_tokens": 13 } }),
    ];
    let mut out: Vec<String> = chunks
        .iter()
        .map(|chunk| format!("data: {chunk}\n\n"))
        .collect();
    out.push("data: [DONE]\n\n".to_string());
    out
}

fn stream_request() -> Value {
    json!({
        "model": MODEL,
        "max_tokens": 64,
        "stream": true,
        "messages": [{ "role": "user", "content": format!("say hello, my key is {PASTED_KEY}") }]
    })
}

fn unzip_into(zip: &[u8], dir: &Path) -> Vec<String> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(zip)).expect("valid zip");
    let mut names = Vec::new();
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).expect("zip entry");
        let mut contents = Vec::new();
        file.read_to_end(&mut contents).expect("read zip entry");
        let path = dir.join(file.name());
        std::fs::create_dir_all(path.parent().expect("entry parent")).expect("mkdir");
        std::fs::write(&path, contents).expect("write entry");
        names.push(file.name().to_string());
    }
    names
}

/// The request log row is written first and linked to its capture right
/// after, from the same background task.
async fn export_when_linked(harness: &ProxyHarness, request_id: &str) -> Vec<u8> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match harness.capture().export_zip(request_id) {
            Ok(zip) => return zip,
            Err(error) if Instant::now() > deadline => {
                panic!("capture for {request_id} never became exportable: {error}")
            }
            Err(_) => tokio::time::sleep(Duration::from_millis(20)).await,
        }
    }
}

#[tokio::test]
async fn bug_report_capture_of_a_bridged_stream_replays_green() {
    let _env = TestEnv::new();
    let upstream = MockUpstream::always(MockReply::sse(chat_sse_chunks())).await;
    let harness = ProxyHarness::builder()
        .provider_with_meta("chat", &upstream, json!({ "apiFormat": "openai_chat" }))
        .spawn()
        .await;
    harness.capture().arm(Duration::from_secs(60));

    let response = harness.messages(&stream_request()).await;
    assert_eq!(response.status(), 200);
    let client_body = response.text().await.expect("read client stream");
    assert!(client_body.contains("message_stop"), "{client_body}");
    assert!(upstream.requests()[0].path.ends_with("/chat/completions"));

    let logs = harness.wait_for_request_logs(1).await;
    let request_id = logs[0].request_id.clone();
    let zip = export_when_linked(&harness, &request_id).await;
    assert!(harness
        .capture()
        .status()
        .captured_request_ids
        .contains(&request_id));

    // Zero key hits: the inbound proxy token, the upstream provider key and
    // the key pasted into the prompt.
    let dir = tempfile::tempdir().expect("tempdir");
    let names = unzip_into(&zip, dir.path());
    for name in &names {
        let text = std::fs::read_to_string(dir.path().join(name)).expect("utf-8 entry");
        for secret in [harness.token(), "sk-harness-chat", PASTED_KEY] {
            assert!(!text.contains(secret), "{name} leaks {secret}:\n{text}");
        }
    }

    // Golden layout: anthropic__openai_chat/<case>/...
    let case_dir = names
        .iter()
        .find_map(|name| name.strip_suffix("/meta.toml"))
        .expect("meta.toml in the export");
    let (bridge_dir, _case) = case_dir.split_once('/').expect("<bridge>/<case>");
    assert_eq!(bridge_dir, "anthropic__openai_chat");
    let bridge = Bridge::from_dir_name(bridge_dir).expect("known bridge");

    let fixture = testkit::load_fixture(bridge, &dir.path().join(case_dir)).expect("export loads");
    assert_eq!(
        fixture.upstream_sse.as_deref(),
        Some(chat_sse_chunks().concat().as_bytes()),
        "upstream SSE is kept byte for byte"
    );
    assert_eq!(
        fixture.client_request.as_ref().unwrap()["model"],
        MODEL,
        "client request is the one the client sent"
    );

    // Replays green: every invariant and split check of proxy_golden passes.
    let outcome = testkit::run_fixture(&fixture);
    assert!(outcome.failures.is_empty(), "{:#?}", outcome.failures);
    assert!(outcome.upstream_request.is_some());
    assert!(outcome.client_response.is_some());

    // And the replayed stream is the stream the proxy actually sent.
    let sse = fixture.upstream_sse.clone().unwrap();
    let replayed = testkit::run_stream(&fixture, &[&sse]).expect("replay stream");
    let captured = std::fs::read(
        dir.path()
            .join(case_dir)
            .join("captured_client_response.sse"),
    )
    .expect("captured client stream");
    assert_eq!(captured, client_body.as_bytes());
    let captured_events = testkit::parse_client_stream(vec![Ok(captured.into())])
        .expect("parse captured client stream");
    assert_eq!(replayed, captured_events);

    // Timings and the upstream request ride along in capture.json.
    let capture: Value = serde_json::from_slice(
        &std::fs::read(dir.path().join(case_dir).join("capture.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(capture["request_id"], request_id);
    assert_eq!(capture["upstream_status"], 200);
    assert!(capture["upstream_request"]["url"]
        .as_str()
        .unwrap()
        .ends_with("/chat/completions"));
    assert_eq!(capture["upstream_request"]["body"]["stream"], true);
    assert!(!capture["upstream_response"]["chunks"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(capture["client_response"]["first_chunk_ms"].is_u64());

    harness.shutdown().await;
}

#[tokio::test]
async fn bug_report_capture_is_opt_in_and_skips_passthrough() {
    let _env = TestEnv::new();
    // Distinct message ids, so the two request log rows are not deduplicated.
    let upstream = MockUpstream::messages("plain", 10, 5).await;
    let harness = ProxyHarness::spawn(&upstream).await;

    // Disarmed: nothing is recorded.
    let response = harness
        .messages(&json!({
            "model": MODEL, "max_tokens": 16,
            "messages": [{ "role": "user", "content": "hi" }]
        }))
        .await;
    assert_eq!(response.status(), 200);
    let logs = harness.wait_for_request_logs(1).await;
    assert!(harness.capture().export_zip(&logs[0].request_id).is_err());

    // Armed, but Anthropic → Anthropic is a passthrough, not a bridge.
    harness.capture().arm(Duration::from_secs(60));
    let response = harness
        .messages(&json!({
            "model": MODEL, "max_tokens": 16,
            "messages": [{ "role": "user", "content": "again" }]
        }))
        .await;
    assert_eq!(response.status(), 200);
    let _ = response.text().await;
    let logs = harness.wait_for_request_logs(2).await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    for row in &logs {
        assert!(harness.capture().export_zip(&row.request_id).is_err());
    }
    assert!(harness.capture().status().captured_request_ids.is_empty());
    assert!(!harness.capture().disarm().active);

    harness.shutdown().await;
}

#[tokio::test]
async fn bug_report_capture_of_a_non_stream_bridge_exports_json_bodies() {
    let _env = TestEnv::new();
    let upstream = MockUpstream::always(MockReply::json(
        200,
        json!({
            "id": "resp_capture",
            "status": "completed",
            "model": "gpt-5.6",
            "output": [{
                "type": "message", "id": "msg_1", "role": "assistant",
                "content": [{ "type": "output_text", "text": "from responses" }]
            }],
            "usage": { "input_tokens": 12, "output_tokens": 4 }
        }),
    ))
    .await;
    let harness = ProxyHarness::builder()
        .provider_with_meta(
            "resp",
            &upstream,
            json!({ "apiFormat": "openai_responses" }),
        )
        .spawn()
        .await;
    harness.capture().arm(Duration::from_secs(60));

    let response = harness
        .messages(&json!({
            "model": MODEL, "max_tokens": 16,
            "messages": [{ "role": "user", "content": "hi" }]
        }))
        .await;
    assert_eq!(response.status(), 200);
    let client_json: Value = response.json().await.expect("client json");

    let logs = harness.wait_for_request_logs(1).await;
    let zip = export_when_linked(&harness, &logs[0].request_id).await;
    let dir = tempfile::tempdir().expect("tempdir");
    let names = unzip_into(&zip, dir.path());
    let case_dir = names
        .iter()
        .find_map(|name| name.strip_suffix("/meta.toml"))
        .expect("meta.toml");
    assert!(case_dir.starts_with("anthropic__responses/"), "{case_dir}");
    assert!(names
        .iter()
        .any(|name| name.ends_with("/upstream_response.json")));

    let fixture = testkit::load_fixture(Bridge::AnthropicResponses, &dir.path().join(case_dir))
        .expect("export loads");
    let outcome = testkit::run_fixture(&fixture);
    assert!(outcome.failures.is_empty(), "{:#?}", outcome.failures);
    let replayed = testkit::convert_non_stream(&fixture)
        .expect("non-stream body")
        .expect("converts");
    let captured: Value = serde_json::from_slice(
        &std::fs::read(
            dir.path()
                .join(case_dir)
                .join("captured_client_response.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(captured, client_json);
    assert_eq!(replayed, client_json);

    harness.shutdown().await;
}
