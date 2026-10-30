//! Request trace inspector end to end (CCS-139): a real proxy with failover
//! in front of scripted upstreams. The trace linked to the request log row
//! must explain why the request ended up on the provider that served it.

use std::time::{Duration, Instant};

use cc_switch_lib::proxy_harness::{MockReply, MockUpstream, ProxyHarness};
use serde_json::{json, Value};

#[path = "support.rs"]
mod support;
use support::TestEnv;

fn simple_request() -> Value {
    json!({
        "model": "claude-sonnet-4-5",
        "max_tokens": 64,
        "messages": [{ "role": "user", "content": "hello" }]
    })
}

/// The request log row is written first and linked to its trace right after,
/// from the same background task.
async fn trace_when_linked(harness: &ProxyHarness, request_id: &str) -> Value {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(trace) = harness.traces().find(request_id) {
            if trace.latency_ms.is_some() {
                return serde_json::to_value(trace).expect("serialize trace");
            }
        }
        if Instant::now() > deadline {
            panic!("trace for {request_id} was never linked");
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn trace_of_a_failover_request_shows_two_attempts_with_reasons() {
    let _env = TestEnv::new();
    let a = MockUpstream::always(MockReply::error(503, "overloaded")).await;
    let b = MockUpstream::always(MockReply::message("from b", 10, 5)).await;
    let harness = ProxyHarness::builder()
        .provider("a", &a)
        .provider("b", &b)
        .failover(true)
        .spawn()
        .await;

    let response = harness.messages(&simple_request()).await;
    assert_eq!(response.status(), 200);
    let _ = response.text().await;

    let logs = harness.wait_for_request_logs(1).await;
    assert_eq!(logs[0].provider_id, "b");
    let trace = trace_when_linked(&harness, &logs[0].request_id).await;

    let attempts = trace["attempts"].as_array().expect("attempts");
    assert_eq!(attempts.len(), 2, "{trace:#}");

    let first = &attempts[0];
    assert_eq!(first["providerId"], "a");
    assert_eq!(first["status"], "failed");
    assert_eq!(first["upstreamStatus"], 503);
    let reason = first["reason"].as_str().expect("failure reason");
    assert!(reason.contains("503"), "{reason}");
    assert!(reason.contains("overloaded"), "{reason}");
    assert_eq!(first["plan"]["upstream"], "AnthropicMessages");
    assert!(first["plan"]["url"]
        .as_str()
        .unwrap()
        .ends_with("/v1/messages"));

    let second = &attempts[1];
    assert_eq!(second["providerId"], "b");
    assert_eq!(second["status"], "succeeded");
    assert_eq!(second["upstreamStatus"], 200);
    assert!(second["reason"].is_null());

    assert_eq!(trace["statusCode"], 200);
    assert_eq!(trace["usageSource"], "response_body");
    assert!(trace["ttfbMs"].is_u64(), "{trace:#}");
    assert!(trace["error"].is_null());

    // Headers are redacted: neither the inbound proxy token nor the
    // provider keys appear anywhere in the trace.
    let text = trace.to_string();
    for secret in [harness.token(), "sk-harness-a", "sk-harness-b"] {
        assert!(!text.contains(secret), "trace leaks {secret}: {text}");
    }
    let client_headers = trace["clientHeaders"].as_array().unwrap();
    assert!(client_headers
        .iter()
        .any(|h| h["name"] == "anthropic-version" && h["value"] == "2023-06-01"));
    assert!(client_headers
        .iter()
        .any(|h| h["name"] == "x-api-key" && h["value"] == "[REDACTED]"));
    assert!(!first["upstreamHeaders"].as_array().unwrap().is_empty());

    harness.shutdown().await;
}

#[tokio::test]
async fn trace_of_a_request_where_every_provider_fails_is_linked_to_the_error_row() {
    let _env = TestEnv::new();
    let a = MockUpstream::always(MockReply::error(503, "a down")).await;
    let b = MockUpstream::always(MockReply::error(502, "b down")).await;
    let harness = ProxyHarness::builder()
        .provider("a", &a)
        .provider("b", &b)
        .failover(true)
        .spawn()
        .await;

    let response = harness.messages(&simple_request()).await;
    assert!(response.status().is_server_error());
    let _ = response.text().await;

    let logs = harness.wait_for_request_logs(1).await;
    let trace = trace_when_linked(&harness, &logs[0].request_id).await;
    let attempts = trace["attempts"].as_array().expect("attempts");
    let summary: Vec<(&str, &str, u64)> = attempts
        .iter()
        .map(|attempt| {
            (
                attempt["providerId"].as_str().unwrap(),
                attempt["status"].as_str().unwrap(),
                attempt["upstreamStatus"].as_u64().unwrap(),
            )
        })
        .collect();
    assert_eq!(summary, [("a", "failed", 503), ("b", "failed", 502)]);
    assert!(attempts[1]["reason"].as_str().unwrap().contains("b down"));
    assert!(trace["error"].as_str().is_some(), "{trace:#}");
    assert_eq!(trace["statusCode"], logs[0].status_code);

    harness.shutdown().await;
}

fn anthropic_sse_chunks() -> Vec<String> {
    let events = [
        (
            "message_start",
            json!({ "type": "message_start", "message": {
                "id": "msg_1", "type": "message", "role": "assistant",
                "model": "claude-sonnet-4-5", "content": [],
                "usage": { "input_tokens": 21, "output_tokens": 0 } } }),
        ),
        (
            "content_block_start",
            json!({ "type": "content_block_start", "index": 0,
                    "content_block": { "type": "text", "text": "" } }),
        ),
        (
            "content_block_delta",
            json!({ "type": "content_block_delta", "index": 0,
                    "delta": { "type": "text_delta", "text": "hi" } }),
        ),
        (
            "content_block_stop",
            json!({ "type": "content_block_stop", "index": 0 }),
        ),
        (
            "message_delta",
            json!({ "type": "message_delta", "delta": { "stop_reason": "end_turn" },
                    "usage": { "output_tokens": 9 } }),
        ),
        ("message_stop", json!({ "type": "message_stop" })),
    ];
    events
        .iter()
        .map(|(event, data)| format!("event: {event}\ndata: {data}\n\n"))
        .collect()
}

#[tokio::test]
async fn trace_of_a_streaming_request_is_linked_with_stream_usage() {
    let _env = TestEnv::new();
    let upstream = MockUpstream::always(MockReply::sse(anthropic_sse_chunks())).await;
    let harness = ProxyHarness::spawn(&upstream).await;

    let mut request = simple_request();
    request["stream"] = json!(true);
    let response = harness.messages(&request).await;
    assert_eq!(response.status(), 200);
    let _ = response.text().await;

    let logs = harness.wait_for_request_logs(1).await;
    let trace = trace_when_linked(&harness, &logs[0].request_id).await;
    assert_eq!(trace["isStream"], true);
    assert_eq!(trace["usageSource"], "stream_events", "{trace:#}");
    assert!(trace["firstTokenMs"].is_u64(), "{trace:#}");
    let attempts = trace["attempts"].as_array().expect("attempts");
    assert_eq!(attempts.len(), 1, "{trace:#}");
    assert_eq!(attempts[0]["status"], "succeeded");

    harness.shutdown().await;
}
