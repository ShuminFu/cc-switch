use super::*;
use crate::proxy::protocol::{ClaudeApiFormat, WireFormat};
use crate::proxy::providers::AuthStrategy;
use crate::proxy::routing::{SideStores, VendorQuirks};

fn begin(store: &TraceStore) -> TraceHandle {
    store.begin(
        "claude",
        &http::Method::POST,
        &"/v1/messages".parse().unwrap(),
        true,
    )
}

fn plan(provider_id: &str) -> RoutePlan {
    RoutePlan {
        provider_id: provider_id.to_string(),
        client: WireFormat::AnthropicMessages,
        upstream: WireFormat::OpenAiChat,
        claude_api_format: Some(ClaudeApiFormat::OpenAiChat),
        url: "https://api.example.com/v1/chat/completions?key=AIzaSyAbcdefghijklmnopqrstuvwxyz0123456"
            .to_string(),
        auth: Some(AuthStrategy::Bearer),
        quirks: VendorQuirks {
            full_url: true,
            ..VendorQuirks::default()
        },
        side_stores: SideStores::default(),
    }
}

#[test]
fn trace_buffer_is_bounded_and_drops_the_oldest() {
    let store = TraceStore::default();
    let mut handles = Vec::new();
    for index in 0..MAX_TRACES + 7 {
        let handle = begin(&store);
        handle.link_request(&format!("req-{index}"), 1, None);
        handles.push(handle);
    }
    assert_eq!(store.count(), MAX_TRACES);
    // The seven oldest were evicted, the newest are still there.
    for index in 0..7 {
        assert!(store.find(&format!("req-{index}")).is_none(), "req-{index}");
    }
    assert!(store.find("req-7").is_some());
    assert!(store.find(&format!("req-{}", MAX_TRACES + 6)).is_some());
}

#[test]
fn trace_buffer_respects_a_custom_capacity() {
    let store = TraceStore::with_capacity(3);
    for _ in 0..10 {
        begin(&store);
    }
    assert_eq!(store.count(), 3);
    assert_eq!(TraceStore::with_capacity(0).capacity, 1);
}

#[test]
fn trace_attempts_per_request_are_bounded() {
    let store = TraceStore::default();
    let trace = begin(&store);
    for index in 0..MAX_TRACE_ATTEMPTS + 4 {
        trace.begin_attempt(&format!("p{index}"), "Provider");
        trace.fail_attempt("上游错误 (503)", Some(503), &[]);
    }
    let snapshot = trace.snapshot();
    assert_eq!(snapshot.attempts.len(), MAX_TRACE_ATTEMPTS);
    assert_eq!(snapshot.dropped_attempts, 4);
    // Results of dropped attempts must not overwrite the last recorded one.
    let last = snapshot.attempts.last().unwrap();
    assert_eq!(last.provider_id, format!("p{}", MAX_TRACE_ATTEMPTS - 1));
}

#[test]
fn trace_records_a_failover_with_reasons_plans_and_recovery() {
    let store = TraceStore::default();
    let trace = begin(&store);
    let mut client_headers = http::HeaderMap::new();
    client_headers.insert(
        "x-api-key",
        "sk-ant-api03-clientclientclient0123".parse().unwrap(),
    );
    client_headers.insert("anthropic-version", "2023-06-01".parse().unwrap());
    trace.record_client_headers(&client_headers);

    let mut upstream_headers = http::HeaderMap::new();
    upstream_headers.insert(
        "authorization",
        "Bearer sk-upstreamupstreamupstream0123".parse().unwrap(),
    );
    trace.skip_attempt("open", "Open breaker", "熔断器未放行，跳过该供应商");
    trace.begin_attempt("a", "Provider A");
    trace.record_send(&plan("a"), &upstream_headers);
    trace.fail_attempt(
        "上游错误 (503): overloaded, key sk-ant-api03-leakedleakedleaked0123",
        Some(503),
        &["thinking_signature_rectifier"],
    );
    trace.begin_attempt("b", "Provider B");
    trace.record_send(&plan("b"), &upstream_headers);
    trace.succeed_attempt(200, &[]);
    trace.finish(200, None);
    trace.set_usage_source(UsageSource::StreamEvents);
    trace.link_request("req-1", 42, Some(7));

    let snapshot = store.find("req-1").expect("linked trace");
    assert_eq!(snapshot.status_code, Some(200));
    assert_eq!(snapshot.latency_ms, Some(42));
    assert_eq!(snapshot.first_token_ms, Some(7));
    assert!(snapshot.ttfb_ms.is_some());
    assert_eq!(snapshot.usage_source, Some(UsageSource::StreamEvents));

    let statuses: Vec<_> = snapshot.attempts.iter().map(|a| a.status).collect();
    assert_eq!(
        statuses,
        [
            AttemptStatus::Skipped,
            AttemptStatus::Failed,
            AttemptStatus::Succeeded
        ]
    );
    let failed = &snapshot.attempts[1];
    assert_eq!(failed.upstream_status, Some(503));
    assert_eq!(failed.recovery, ["thinking_signature_rectifier"]);
    let reason = failed.reason.as_deref().unwrap();
    assert!(reason.contains("overloaded"), "{reason}");
    assert!(!reason.contains("leakedleaked"), "{reason}");
    let failed_plan = failed.plan.as_ref().unwrap();
    assert_eq!(failed_plan.upstream, "OpenAiChat");
    assert_eq!(failed_plan.api_format.as_deref(), Some("openai_chat"));
    assert_eq!(
        failed_plan.url,
        "https://api.example.com/v1/chat/completions"
    );
    assert_eq!(failed_plan.quirks, ["full_url"]);
    assert_eq!(snapshot.attempts[2].upstream_status, Some(200));

    // Headers are redacted on record.
    let json = serde_json::to_string(&snapshot).unwrap();
    for secret in ["clientclient", "upstreamupstream", "AIzaSy"] {
        assert!(!json.contains(secret), "{secret} leaked: {json}");
    }
    assert!(json.contains("2023-06-01"));
    assert!(json.contains("\"status\":\"skipped\""), "{json}");
    assert!(json.contains("\"usageSource\":\"stream_events\""), "{json}");
}

#[test]
fn trace_find_returns_the_newest_trace_for_a_request_id() {
    let store = TraceStore::default();
    let older = begin(&store);
    older.link_request("dup", 1, None);
    let newer = begin(&store);
    newer.link_request("dup", 2, None);
    assert_eq!(store.find("dup").unwrap().latency_ms, Some(2));
    assert!(store.find("missing").is_none());
}

#[test]
fn trace_truncates_long_reasons_and_header_values() {
    let store = TraceStore::default();
    let trace = begin(&store);
    let mut headers = http::HeaderMap::new();
    headers.insert("x-long", "v".repeat(5000).parse().unwrap());
    trace.record_client_headers(&headers);
    trace.begin_attempt("a", "A");
    trace.fail_attempt(&"错".repeat(5000), None, &[]);
    let snapshot = trace.snapshot();
    assert!(snapshot.client_headers[0].value.chars().count() <= MAX_HEADER_VALUE_CHARS + 1);
    let reason = snapshot.attempts[0].reason.as_deref().unwrap();
    assert!(reason.chars().count() <= MAX_REASON_CHARS + 1);
}
