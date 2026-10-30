//! End-to-end proxy scenarios: a real `ProxyServer` (router, forwarder,
//! circuit breakers, rectifiers, usage logger) in front of scripted mock
//! upstreams, plus the latency / allocation / SQL-statement baseline that is
//! committed to `docs/dev/perf-baseline.json`.
//!
//! Everything runs on the `#[tokio::test]` current-thread runtime: the scoped
//! test paths, the statement counter and the allocation counter are all
//! per-thread, so concurrent tests in this binary do not disturb each other.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use cc_switch_lib::proxy_harness::{
    db_read_statement_count, db_statement_count, CircuitState, HarnessCircuit, HarnessHealth,
    MockReply, MockUpstream, ProxyHarness, RawMockUpstream,
};
use cc_switch_lib::{update_settings, AppSettings};
use serde_json::{json, Value};

#[path = "support.rs"]
mod support;
use support::TestEnv;

// ============================================================================
// Allocation counter (per thread)
// ============================================================================

struct CountingAlloc;

thread_local! {
    static ALLOCATIONS: Cell<u64> = const { Cell::new(0) };
    static ALLOCATED_BYTES: Cell<u64> = const { Cell::new(0) };
}

fn note_allocation(size: usize) {
    // const-initialised, destructor-free thread locals never allocate, so this
    // is safe to call from inside the allocator.
    let _ = ALLOCATIONS.try_with(|count| count.set(count.get() + 1));
    let _ = ALLOCATED_BYTES.try_with(|bytes| bytes.set(bytes.get() + size as u64));
}

unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        note_allocation(layout.size());
        System.alloc(layout)
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        note_allocation(layout.size());
        System.alloc_zeroed(layout)
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        note_allocation(new_size);
        System.realloc(ptr, layout, new_size)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout)
    }
}

#[global_allocator]
static GLOBAL: CountingAlloc = CountingAlloc;

fn allocation_snapshot() -> (u64, u64) {
    (ALLOCATIONS.with(Cell::get), ALLOCATED_BYTES.with(Cell::get))
}

// ============================================================================
// Helpers
// ============================================================================

const MODEL: &str = "claude-sonnet-4-5";

fn simple_request() -> Value {
    json!({
        "model": MODEL,
        "max_tokens": 64,
        "messages": [{ "role": "user", "content": "hello" }]
    })
}

fn healthy() -> HarnessHealth {
    HarnessHealth {
        is_healthy: true,
        consecutive_failures: 0,
    }
}

async fn json_body(response: reqwest::Response) -> Value {
    let text = response.text().await.expect("read proxy response");
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("non-JSON body {text:?}: {e}"))
}

fn first_text(body: &Value) -> &str {
    body["content"][0]["text"].as_str().unwrap_or_default()
}

// ============================================================================
// Failover and circuit breaker
// ============================================================================

#[tokio::test]
async fn failover_moves_to_second_provider_when_first_returns_5xx() {
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
    assert_eq!(first_text(&json_body(response).await), "from b");

    assert_eq!(a.hits(), 1, "primary is tried once");
    assert_eq!(b.hits(), 1, "secondary serves the request");
    // Both upstreams receive the provider's own credentials, not the client's.
    assert_eq!(
        a.requests()[0].headers["authorization"],
        "Bearer sk-harness-a"
    );
    assert_eq!(
        b.requests()[0].headers["authorization"],
        "Bearer sk-harness-b"
    );
    assert_eq!(b.requests()[0].path, "/v1/messages");

    let a_health = harness
        .wait_for_health(
            "a",
            &HarnessHealth {
                is_healthy: true,
                consecutive_failures: 1,
            },
        )
        .await;
    assert_eq!(a_health.consecutive_failures, 1);

    let status = harness.status().await;
    assert_eq!(status.total_requests, 1);
    assert_eq!(status.success_requests, 1);
    assert_eq!(status.failover_count, 1);

    let logs = harness.wait_for_request_logs(1).await;
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].provider_id, "b");
    assert_eq!(logs[0].status_code, 200);

    harness.shutdown().await;
}

/// A successful non-streaming OpenAI Responses reply.
fn responses_reply(text: &str) -> MockReply {
    MockReply::json(
        200,
        json!({
            "id": "resp_harness",
            "status": "completed",
            "model": "gpt-5.6",
            "output": [{
                "type": "message",
                "id": "msg_1",
                "role": "assistant",
                "content": [{ "type": "output_text", "text": text }]
            }],
            "usage": { "input_tokens": 12, "output_tokens": 4 }
        }),
    )
}

/// The response is decoded with the route plan of the attempt that succeeded,
/// not with the plan of the provider tried first (CCS-126).
#[tokio::test]
async fn failover_from_copilot_responses_to_anthropic_decodes_with_the_anthropic_plan() {
    let _env = TestEnv::new();
    let copilot = MockUpstream::always(responses_reply("from copilot")).await;
    let b = MockUpstream::always(MockReply::message("from b", 10, 5)).await;
    let harness = ProxyHarness::builder()
        .provider_with_meta(
            "copilot",
            &copilot,
            json!({ "providerType": "github_copilot", "apiFormat": "openai_responses" }),
        )
        .provider("b", &b)
        .failover(true)
        .spawn()
        .await;

    let response = harness.messages(&simple_request()).await;
    assert_eq!(response.status(), 200);
    let body = json_body(response).await;
    // Anthropic passthrough: the upstream message reaches the client untouched
    // instead of being run through the Responses → Anthropic converter.
    assert_eq!(body["id"], "msg_harness");
    assert_eq!(body["type"], "message");
    assert_eq!(first_text(&body), "from b");
    assert_eq!(body["usage"]["input_tokens"], 10);

    // The harness has no Copilot token source, so the Copilot attempt fails
    // before sending and the request fails over to the Anthropic provider.
    assert_eq!(copilot.hits(), 0);
    assert_eq!(b.hits(), 1);
    assert_eq!(b.requests()[0].path, "/v1/messages");

    let logs = harness.wait_for_request_logs(1).await;
    assert_eq!(logs[0].provider_id, "b");
    assert_eq!(logs[0].status_code, 200);

    harness.shutdown().await;
}

#[tokio::test]
async fn failover_from_responses_provider_to_anthropic_passes_the_message_through() {
    let _env = TestEnv::new();
    let a = MockUpstream::always(MockReply::error(503, "overloaded")).await;
    let b = MockUpstream::always(MockReply::message("from b", 10, 5)).await;
    let harness = ProxyHarness::builder()
        .provider_with_meta("a", &a, json!({ "apiFormat": "openai_responses" }))
        .provider("b", &b)
        .failover(true)
        .spawn()
        .await;

    let response = harness.messages(&simple_request()).await;
    assert_eq!(response.status(), 200);
    let body = json_body(response).await;
    assert_eq!(body["id"], "msg_harness");
    assert_eq!(first_text(&body), "from b");

    assert_eq!(a.hits(), 1);
    assert!(
        a.requests()[0].path.ends_with("/responses"),
        "first attempt is sent in the Responses format: {}",
        a.requests()[0].path
    );
    assert!(a.requests()[0].json().get("input").is_some());
    assert_eq!(b.requests()[0].path, "/v1/messages");
    assert!(b.requests()[0].json().get("messages").is_some());

    harness.shutdown().await;
}

#[tokio::test]
async fn failover_from_anthropic_to_responses_provider_converts_the_response() {
    let _env = TestEnv::new();
    let a = MockUpstream::always(MockReply::error(503, "overloaded")).await;
    let b = MockUpstream::always(responses_reply("from responses")).await;
    let harness = ProxyHarness::builder()
        .provider("a", &a)
        .provider_with_meta("b", &b, json!({ "apiFormat": "openai_responses" }))
        .failover(true)
        .spawn()
        .await;

    let response = harness.messages(&simple_request()).await;
    assert_eq!(response.status(), 200);
    let body = json_body(response).await;
    // Decoded with the Responses plan of the second attempt.
    assert_eq!(body["id"], "resp_harness");
    assert_eq!(body["type"], "message");
    assert_eq!(first_text(&body), "from responses");
    assert_eq!(body["stop_reason"], "end_turn");

    assert_eq!(a.requests()[0].path, "/v1/messages");
    assert!(b.requests()[0].path.ends_with("/responses"));

    let logs = harness.wait_for_request_logs(1).await;
    assert_eq!(logs[0].provider_id, "b");

    harness.shutdown().await;
}

#[tokio::test]
async fn non_retryable_client_error_does_not_fail_over() {
    let _env = TestEnv::new();
    let a = MockUpstream::always(MockReply::error(400, "messages: field required")).await;
    let b = MockUpstream::always(MockReply::message("from b", 10, 5)).await;
    let harness = ProxyHarness::builder()
        .provider("a", &a)
        .provider("b", &b)
        .failover(true)
        .spawn()
        .await;

    let response = harness.messages(&simple_request()).await;
    assert_eq!(response.status(), 400);
    assert_eq!(a.hits(), 1);
    assert_eq!(b.hits(), 0, "a 400 is the client's fault on every provider");
    // A client error must not count against the provider's health.
    assert_eq!(harness.health("a").await, healthy());

    harness.shutdown().await;
}

#[tokio::test]
async fn breaker_opens_after_threshold_and_skips_the_provider() {
    let _env = TestEnv::new();
    let a = MockUpstream::always(MockReply::error(502, "bad gateway")).await;
    let b = MockUpstream::always(MockReply::message("from b", 10, 5)).await;
    let harness = ProxyHarness::builder()
        .provider("a", &a)
        .provider("b", &b)
        .failover(true)
        .circuit(HarnessCircuit {
            failure_threshold: 2,
            success_threshold: 1,
            timeout_seconds: 600,
        })
        .spawn()
        .await;

    for _ in 0..2 {
        let response = harness.messages(&simple_request()).await;
        assert_eq!(response.status(), 200);
    }
    assert_eq!(a.hits(), 2, "a fails twice and reaches the threshold");

    let a_health = harness
        .wait_for_health(
            "a",
            &HarnessHealth {
                is_healthy: false,
                consecutive_failures: 2,
            },
        )
        .await;
    assert!(
        !a_health.is_healthy,
        "health row follows the breaker threshold"
    );

    // Breaker is open: a is no longer even tried.
    for _ in 0..3 {
        let response = harness.messages(&simple_request()).await;
        assert_eq!(response.status(), 200);
    }
    assert_eq!(a.hits(), 2, "open breaker skips a");
    assert_eq!(b.hits(), 5);

    harness.shutdown().await;
}

#[tokio::test]
async fn retry_after_cools_the_provider_down_without_tripping_the_breaker() {
    let _env = TestEnv::new();
    // a: rate limited once with `Retry-After: 2`, healthy afterwards.
    let a = MockUpstream::spawn(|index, _| {
        if index == 0 {
            MockReply::error(429, "rate limited").with_header("retry-after", "2")
        } else {
            MockReply::message("from a", 10, 5)
        }
    })
    .await;
    let b = MockUpstream::messages("from b", 10, 5).await;
    let harness = ProxyHarness::builder()
        .provider("a", &a)
        .provider("b", &b)
        .failover(true)
        .circuit(HarnessCircuit {
            // One counted failure would open the breaker.
            failure_threshold: 1,
            success_threshold: 1,
            timeout_seconds: 600,
        })
        .spawn()
        .await;

    // 1. a answers 429 + Retry-After: the request fails over to b at once.
    let started = Instant::now();
    let body = json_body(harness.messages(&simple_request()).await).await;
    assert_eq!(first_text(&body), "from b");
    assert_eq!(a.hits(), 1);

    // 2. During the cooldown a is not tried at all.
    for _ in 0..3 {
        let body = json_body(harness.messages(&simple_request()).await).await;
        assert_eq!(first_text(&body), "from b");
    }
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "the cooldown window was missed"
    );
    assert_eq!(a.hits(), 1, "a is cooling down");
    assert_eq!(b.hits(), 4);

    // The rate limit is not a breaker failure, and a's health is untouched.
    let stats = harness.circuit_stats("a").await.expect("a has a breaker");
    assert_eq!(stats.state, CircuitState::Closed, "{stats:?}");
    assert_eq!(stats.consecutive_failures, 0, "{stats:?}");
    assert_eq!(harness.health("a").await, healthy());

    // 3. Once Retry-After has elapsed, traffic goes back to a.
    tokio::time::sleep(
        Duration::from_secs(2).saturating_sub(started.elapsed()) + Duration::from_millis(200),
    )
    .await;
    let body = json_body(harness.messages(&simple_request()).await).await;
    assert_eq!(first_text(&body), "from a");
    assert_eq!(a.hits(), 2);
    assert_eq!(b.hits(), 4);

    let stats = harness.circuit_stats("a").await.expect("a has a breaker");
    assert_eq!(stats.state, CircuitState::Closed, "{stats:?}");
    let status = harness.status().await;
    assert_eq!(status.success_requests, 5);
    assert_eq!(status.failover_count, 1);

    harness.shutdown().await;
}

fn session_request(session_id: &str) -> Value {
    let mut body = simple_request();
    body["metadata"] = json!({ "session_id": session_id });
    body
}

/// a fails once, then is healthy; b is healthy except for its fourth request.
async fn sticky_session_upstreams() -> (MockUpstream, MockUpstream) {
    let a = MockUpstream::spawn(|index, _| {
        if index == 0 {
            MockReply::error(500, "boom")
        } else {
            MockReply::message("from a", 10, 5)
        }
    })
    .await;
    let b = MockUpstream::spawn(|index, _| {
        if index == 3 {
            MockReply::error(500, "b boom")
        } else {
            MockReply::message("from b", 10, 5)
        }
    })
    .await;
    (a, b)
}

async fn sticky_session_harness(a: &MockUpstream, b: &MockUpstream) -> ProxyHarness {
    ProxyHarness::builder()
        .provider("a", a)
        .provider("b", b)
        .failover(true)
        .circuit(HarnessCircuit {
            // One failure does not open a breaker: only the sticky table moves traffic.
            failure_threshold: 5,
            success_threshold: 1,
            timeout_seconds: 600,
        })
        .spawn()
        .await
}

#[tokio::test]
async fn sticky_session_stays_on_the_failover_provider_until_it_fails() {
    let _env = TestEnv::new();
    update_settings(AppSettings {
        sticky_session_routing: true,
        ..AppSettings::default()
    })
    .expect("enable sticky session routing");
    let (a, b) = sticky_session_upstreams().await;
    let harness = sticky_session_harness(&a, &b).await;

    // 1. a fails, s1 fails over to b.
    let body = json_body(harness.messages(&session_request("s1")).await).await;
    assert_eq!(first_text(&body), "from b");
    assert_eq!((a.hits(), b.hits()), (1, 1));
    let stats = harness.circuit_stats("a").await.expect("a has a breaker");
    assert_eq!(stats.state, CircuitState::Closed, "{stats:?}");

    // 2. a is healthy again, but s1 stays on b.
    for _ in 0..2 {
        let body = json_body(harness.messages(&session_request("s1")).await).await;
        assert_eq!(first_text(&body), "from b");
    }
    assert_eq!((a.hits(), b.hits()), (1, 3));

    // 3. Other sessions, and requests without a client session id, start at the head.
    let body = json_body(harness.messages(&session_request("s2")).await).await;
    assert_eq!(first_text(&body), "from a");
    let body = json_body(harness.messages(&simple_request()).await).await;
    assert_eq!(first_text(&body), "from a");
    assert_eq!((a.hits(), b.hits()), (3, 3));

    // 4. b fails: s1 fails over to a and now sticks there.
    let body = json_body(harness.messages(&session_request("s1")).await).await;
    assert_eq!(first_text(&body), "from a");
    assert_eq!((a.hits(), b.hits()), (4, 4));
    let body = json_body(harness.messages(&session_request("s1")).await).await;
    assert_eq!(first_text(&body), "from a");
    assert_eq!((a.hits(), b.hits()), (5, 4));

    harness.shutdown().await;
}

#[tokio::test]
async fn sticky_session_routing_is_off_by_default() {
    let _env = TestEnv::new();
    let (a, b) = sticky_session_upstreams().await;
    let harness = sticky_session_harness(&a, &b).await;

    let body = json_body(harness.messages(&session_request("s1")).await).await;
    assert_eq!(first_text(&body), "from b");
    // Without the setting the session goes back to the head of the queue.
    let body = json_body(harness.messages(&session_request("s1")).await).await;
    assert_eq!(first_text(&body), "from a");
    assert_eq!((a.hits(), b.hits()), (2, 1));

    harness.shutdown().await;
}

#[tokio::test]
async fn half_open_probe_failure_reopens_and_success_closes_the_breaker() {
    let _env = TestEnv::new();
    // a: fails, fails (the half-open probe), then recovers.
    let a = MockUpstream::sequence(vec![
        MockReply::error(500, "boom"),
        MockReply::error(500, "still down"),
        MockReply::message("from a", 10, 5),
    ])
    .await;
    let b = MockUpstream::always(MockReply::message("from b", 10, 5)).await;
    let harness = ProxyHarness::builder()
        .provider("a", &a)
        .provider("b", &b)
        .failover(true)
        .circuit(HarnessCircuit {
            failure_threshold: 1,
            success_threshold: 1,
            timeout_seconds: 1,
        })
        .spawn()
        .await;
    let wait_out_open = || tokio::time::sleep(Duration::from_millis(1100));

    // 1. a fails once → Open; b serves.
    let body = json_body(harness.messages(&simple_request()).await).await;
    assert_eq!(first_text(&body), "from b");
    assert_eq!(a.hits(), 1);

    // 2. Still open: a is skipped.
    let body = json_body(harness.messages(&simple_request()).await).await;
    assert_eq!(first_text(&body), "from b");
    assert_eq!(a.hits(), 1);

    // 3. Timeout elapsed → HalfOpen probe goes to a and fails → Open again.
    wait_out_open().await;
    let body = json_body(harness.messages(&simple_request()).await).await;
    assert_eq!(first_text(&body), "from b");
    assert_eq!(a.hits(), 2, "half-open lets one probe through");

    let body = json_body(harness.messages(&simple_request()).await).await;
    assert_eq!(first_text(&body), "from b");
    assert_eq!(a.hits(), 2, "failed probe re-opens the breaker");

    // 4. Next probe succeeds → Closed; a serves again from then on.
    wait_out_open().await;
    let body = json_body(harness.messages(&simple_request()).await).await;
    assert_eq!(first_text(&body), "from a");
    assert_eq!(a.hits(), 3);

    let body = json_body(harness.messages(&simple_request()).await).await;
    assert_eq!(first_text(&body), "from a");
    assert_eq!(a.hits(), 4, "closed breaker routes to the primary again");
    assert_eq!(harness.wait_for_health("a", &healthy()).await, healthy());

    harness.shutdown().await;
}

#[tokio::test]
async fn client_abort_during_half_open_probe_releases_the_probe_slot() {
    let _env = TestEnv::new();
    // a: fails (→ Open), hangs on the half-open probe, then recovers.
    let a = MockUpstream::sequence(vec![
        MockReply::error(500, "boom"),
        MockReply::message("too late", 10, 5).delayed(Duration::from_secs(10)),
        MockReply::message("from a", 10, 5),
    ])
    .await;
    let b = MockUpstream::always(MockReply::message("from b", 10, 5)).await;
    let harness = ProxyHarness::builder()
        .provider("a", &a)
        .provider("b", &b)
        .failover(true)
        .circuit(HarnessCircuit {
            failure_threshold: 1,
            success_threshold: 1,
            timeout_seconds: 0,
        })
        .spawn()
        .await;

    // 1. a fails once → Open; b serves.
    let body = json_body(harness.messages(&simple_request()).await).await;
    assert_eq!(first_text(&body), "from b");
    assert_eq!(a.hits(), 1);

    // 2. timeout 0 → the next request is a's half-open probe; the upstream
    //    hangs and the client gives up, which cancels the proxy's request.
    let aborted = tokio::time::timeout(
        Duration::from_millis(300),
        harness.messages(&simple_request()),
    )
    .await;
    assert!(aborted.is_err(), "the probe hangs until the client aborts");
    assert_eq!(a.hits(), 2, "the probe reached a");

    // 3. The aborted probe must not keep the half-open slot: a is probed
    //    again (instead of being skipped forever) and closes the breaker.
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let body = json_body(harness.messages(&simple_request()).await).await;
        if first_text(&body) == "from a" {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "a stayed half-open with a leaked probe slot"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(a.hits(), 3);

    let body = json_body(harness.messages(&simple_request()).await).await;
    assert_eq!(first_text(&body), "from a", "closed breaker serves a again");

    harness.shutdown().await;
}

#[tokio::test]
async fn probe_now_closes_an_open_breaker_against_a_healthy_upstream() {
    let _env = TestEnv::new();
    // a: fails once (→ Open), healthy from then on.
    let a = MockUpstream::sequence(vec![
        MockReply::error(500, "boom"),
        MockReply::message("from a", 10, 5),
    ])
    .await;
    let b = MockUpstream::always(MockReply::message("from b", 10, 5)).await;
    let harness = ProxyHarness::builder()
        .provider("a", &a)
        .provider("b", &b)
        .failover(true)
        .circuit(HarnessCircuit {
            failure_threshold: 1,
            // A manual probe closes the breaker without waiting for the
            // success threshold or the open timeout.
            success_threshold: 3,
            timeout_seconds: 600,
        })
        .spawn()
        .await;

    // 1. Force Open: a fails once, b serves.
    let body = json_body(harness.messages(&simple_request()).await).await;
    assert_eq!(first_text(&body), "from b");
    let stats = harness.circuit_stats("a").await.expect("a has a breaker");
    assert_eq!(stats.state, CircuitState::Open);
    assert!(stats.open_remaining_secs.unwrap() > 500, "{stats:?}");

    // 2. Probe now: one minimal request to a through the half-open path.
    let probe = harness.probe_now("a").await;
    assert!(probe.probed && probe.success, "{probe:?}");
    assert_eq!(probe.stats.state, CircuitState::Closed);
    assert_eq!(probe.stats.open_remaining_secs, None);
    assert_eq!(a.hits(), 2, "the probe reached a");
    assert_eq!(a.requests()[1].method, "GET");
    assert_eq!(harness.health("a").await, healthy());

    // 3. Closed: a serves real traffic again, well before the open timeout.
    let body = json_body(harness.messages(&simple_request()).await).await;
    assert_eq!(first_text(&body), "from a");

    // 4. A closed breaker is not probed again.
    let probe = harness.probe_now("a").await;
    assert!(!probe.probed, "{probe:?}");
    assert_eq!(a.hits(), 3);

    harness.shutdown().await;
}

#[tokio::test]
async fn probe_now_against_a_failing_upstream_keeps_the_breaker_open() {
    let _env = TestEnv::new();
    let a = MockUpstream::always(MockReply::error(503, "still down")).await;
    let b = MockUpstream::always(MockReply::message("from b", 10, 5)).await;
    let harness = ProxyHarness::builder()
        .provider("a", &a)
        .provider("b", &b)
        .failover(true)
        .circuit(HarnessCircuit {
            failure_threshold: 1,
            success_threshold: 1,
            timeout_seconds: 600,
        })
        .spawn()
        .await;

    let body = json_body(harness.messages(&simple_request()).await).await;
    assert_eq!(first_text(&body), "from b");

    let probe = harness.probe_now("a").await;
    assert!(probe.probed && !probe.success, "{probe:?}");
    assert_eq!(probe.stats.state, CircuitState::Open);
    assert_eq!(a.hits(), 2);

    // Still open: real traffic keeps skipping a.
    let body = json_body(harness.messages(&simple_request()).await).await;
    assert_eq!(first_text(&body), "from b");
    assert_eq!(a.hits(), 2);

    harness.shutdown().await;
}

// ============================================================================
// Rectifiers
// ============================================================================

#[tokio::test]
async fn signature_rectifier_strips_thinking_blocks_and_retries_same_provider() {
    let _env = TestEnv::new();
    let upstream = MockUpstream::sequence(vec![
        MockReply::error(
            400,
            "messages.1.content.0: Invalid `signature` in `thinking` block",
        ),
        MockReply::message("rectified", 10, 5),
    ])
    .await;
    let harness = ProxyHarness::spawn(&upstream).await;

    let request = json!({
        "model": MODEL,
        "max_tokens": 64,
        "messages": [
            { "role": "user", "content": "first" },
            {
                "role": "assistant",
                "content": [
                    { "type": "thinking", "thinking": "hmm", "signature": "forged" },
                    { "type": "text", "text": "answer" }
                ]
            },
            { "role": "user", "content": "second" }
        ]
    });
    let response = harness.messages(&request).await;
    assert_eq!(response.status(), 200);
    assert_eq!(first_text(&json_body(response).await), "rectified");

    let requests = upstream.requests();
    assert_eq!(requests.len(), 2, "one rejected attempt plus one retry");
    let original = requests[0].json();
    assert_eq!(original["messages"][1]["content"][0]["type"], "thinking");
    let retried = requests[1].json();
    assert_eq!(
        retried["messages"][1]["content"],
        json!([{ "type": "text", "text": "answer" }]),
        "thinking block removed on retry"
    );
    // Rectified retries are neutral for provider health.
    assert_eq!(harness.wait_for_health("p1", &healthy()).await, healthy());

    harness.shutdown().await;
}

#[tokio::test]
async fn budget_rectifier_raises_thinking_budget_and_retries() {
    let _env = TestEnv::new();
    let upstream = MockUpstream::sequence(vec![
        MockReply::error(
            400,
            "thinking.enabled.budget_tokens: Input should be greater than or equal to 1024",
        ),
        MockReply::message("budget ok", 10, 5),
    ])
    .await;
    let harness = ProxyHarness::spawn(&upstream).await;

    let request = json!({
        "model": MODEL,
        "max_tokens": 512,
        "thinking": { "type": "enabled", "budget_tokens": 256 },
        "messages": [{ "role": "user", "content": "think" }]
    });
    let response = harness.messages(&request).await;
    assert_eq!(response.status(), 200);
    assert_eq!(first_text(&json_body(response).await), "budget ok");

    let requests = upstream.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].json()["thinking"]["budget_tokens"], 256);
    let retried = requests[1].json();
    assert_eq!(retried["thinking"]["type"], "enabled");
    assert_eq!(retried["thinking"]["budget_tokens"], 32000);
    assert_eq!(retried["max_tokens"], 64000);

    harness.shutdown().await;
}

#[tokio::test]
async fn media_rectifier_replaces_images_when_upstream_rejects_them() {
    let _env = TestEnv::new();
    let upstream = MockUpstream::sequence(vec![
        MockReply::error(400, "This model does not support image input"),
        MockReply::message("text only", 10, 5),
    ])
    .await;
    let harness = ProxyHarness::spawn(&upstream).await;

    let request = json!({
        "model": MODEL,
        "max_tokens": 64,
        "messages": [{
            "role": "user",
            "content": [
                { "type": "text", "text": "what is this?" },
                {
                    "type": "image",
                    "source": { "type": "base64", "media_type": "image/png", "data": "iVBORw0KGgo=" }
                }
            ]
        }]
    });
    let response = harness.messages(&request).await;
    assert_eq!(response.status(), 200);
    assert_eq!(first_text(&json_body(response).await), "text only");

    let requests = upstream.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(
        requests[0].json()["messages"][0]["content"][1]["type"],
        "image"
    );
    let retried = requests[1].json();
    let retried_content = retried["messages"][0]["content"].to_string();
    assert!(
        !retried_content.contains("\"image\""),
        "image block removed on retry: {retried_content}"
    );
    assert!(
        retried_content.contains("[Unsupported Image]"),
        "image replaced by the marker: {retried_content}"
    );

    harness.shutdown().await;
}

#[tokio::test]
async fn recovery_strategies_chain_on_the_same_provider() {
    let _env = TestEnv::new();
    let upstream = MockUpstream::sequence(vec![
        MockReply::error(400, "This model does not support image input"),
        MockReply::error(
            400,
            "messages.0.content.0: Invalid `signature` in `thinking` block",
        ),
        MockReply::message("recovered", 10, 5),
    ])
    .await;
    let harness = ProxyHarness::spawn(&upstream).await;

    let request = json!({
        "model": MODEL,
        "max_tokens": 64,
        "messages": [{
            "role": "assistant",
            "content": [
                { "type": "thinking", "thinking": "hmm", "signature": "forged" },
                {
                    "type": "image",
                    "source": { "type": "base64", "media_type": "image/png", "data": "iVBORw0KGgo=" }
                }
            ]
        }]
    });
    let response = harness.messages(&request).await;
    assert_eq!(response.status(), 200);
    assert_eq!(first_text(&json_body(response).await), "recovered");

    let requests = upstream.requests();
    assert_eq!(
        requests.len(),
        3,
        "media downgrade, then signature rectifier"
    );
    let downgraded = requests[1].json()["messages"][0]["content"].to_string();
    assert!(downgraded.contains("[Unsupported Image]"), "{downgraded}");
    assert!(downgraded.contains("\"thinking\""), "{downgraded}");
    let rectified = requests[2].json()["messages"][0]["content"].to_string();
    assert!(rectified.contains("[Unsupported Image]"), "{rectified}");
    assert!(!rectified.contains("\"thinking\""), "{rectified}");
    assert_eq!(harness.wait_for_health("p1", &healthy()).await, healthy());

    harness.shutdown().await;
}

#[tokio::test]
async fn recovery_strategy_is_applied_at_most_once() {
    let _env = TestEnv::new();
    let upstream = MockUpstream::always(MockReply::error(
        400,
        "thinking.enabled.budget_tokens: Input should be greater than or equal to 1024",
    ))
    .await;
    let harness = ProxyHarness::spawn(&upstream).await;

    let request = json!({
        "model": MODEL,
        "max_tokens": 512,
        "thinking": { "type": "enabled", "budget_tokens": 256 },
        "messages": [{ "role": "user", "content": "think" }]
    });
    let response = harness.messages(&request).await;
    assert_eq!(response.status(), 400);
    assert_eq!(
        upstream.hits(),
        2,
        "the original request plus a single budget-rectified retry"
    );
    // Rectifier give-ups are client errors and leave provider health untouched.
    assert_eq!(harness.health("p1").await, healthy());

    harness.shutdown().await;
}

// ============================================================================
// Streaming and usage logging
// ============================================================================

fn anthropic_sse_chunks() -> Vec<String> {
    let events = [
        (
            "message_start",
            json!({
                "type": "message_start",
                "message": {
                    "id": "msg_sse", "type": "message", "role": "assistant", "model": MODEL,
                    "content": [], "stop_reason": null,
                    "usage": { "input_tokens": 21, "output_tokens": 1 }
                }
            }),
        ),
        (
            "content_block_start",
            json!({ "type": "content_block_start", "index": 0,
                    "content_block": { "type": "text", "text": "" } }),
        ),
        (
            "content_block_delta",
            json!({ "type": "content_block_delta", "index": 0,
                    "delta": { "type": "text_delta", "text": "Hello" } }),
        ),
        (
            "content_block_delta",
            json!({ "type": "content_block_delta", "index": 0,
                    "delta": { "type": "text_delta", "text": " world" } }),
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
async fn sse_stream_passes_through_unchanged_and_logs_usage() {
    let _env = TestEnv::new();
    let chunks = anthropic_sse_chunks();
    let upstream = MockUpstream::always(MockReply::sse(chunks.clone())).await;
    let harness = ProxyHarness::spawn(&upstream).await;

    let mut request = simple_request();
    request["stream"] = json!(true);
    let response = harness.messages(&request).await;
    assert_eq!(response.status(), 200);
    let content_type = response.headers()["content-type"]
        .to_str()
        .unwrap()
        .to_string();
    assert!(
        content_type.starts_with("text/event-stream"),
        "{content_type}"
    );
    let body = response.text().await.expect("read stream");
    assert_eq!(body, chunks.concat(), "SSE bytes are passed through as-is");

    let logs = harness.wait_for_request_logs(1).await;
    assert_eq!(logs.len(), 1);
    let row = &logs[0];
    assert!(row.is_streaming);
    assert_eq!(row.status_code, 200);
    assert_eq!(row.input_tokens, 21);
    assert_eq!(row.output_tokens, 9);
    assert_eq!(row.provider_id, "p1");

    harness.shutdown().await;
}

#[tokio::test]
async fn request_logs_record_success_and_failure_rows() {
    let _env = TestEnv::new();
    let upstream = MockUpstream::sequence(vec![
        MockReply::message("ok", 1200, 300),
        MockReply::error(500, "upstream exploded"),
    ])
    .await;
    let harness = ProxyHarness::spawn(&upstream).await;

    let response = harness.messages(&simple_request()).await;
    assert_eq!(response.status(), 200);
    let success = harness.wait_for_request_logs(1).await;
    assert_eq!(success.len(), 1);

    let response = harness.messages(&simple_request()).await;
    assert!(response.status().is_server_error(), "{}", response.status());
    let logs = harness.wait_for_request_logs(2).await;
    assert_eq!(logs.len(), 2);

    let ok = logs
        .iter()
        .find(|row| row.status_code == 200)
        .expect("success row");
    assert_eq!(ok.app_type, "claude");
    assert_eq!(ok.provider_id, "p1");
    assert_eq!(ok.model, MODEL);
    assert_eq!(ok.request_model.as_deref(), Some(MODEL));
    assert_eq!(ok.input_tokens, 1200);
    assert_eq!(ok.output_tokens, 300);
    assert!(!ok.is_streaming);
    assert!(ok.error_message.is_none());
    assert!(ok.session_id.is_some());
    let cost: f64 = ok.total_cost_usd.parse().expect("numeric cost");
    assert!(cost > 0.0, "seeded pricing gives a non-zero cost: {cost}");

    let failed = logs
        .iter()
        .find(|row| row.status_code != 200)
        .expect("failure row");
    assert_eq!(failed.provider_id, "p1");
    assert!(failed.status_code >= 500, "{}", failed.status_code);
    assert_eq!(failed.input_tokens, 0);
    let message = failed.error_message.as_deref().unwrap_or_default();
    assert!(!message.is_empty(), "failure row carries the error message");
    assert_ne!(ok.request_id, failed.request_id);

    harness.shutdown().await;
}

// ============================================================================
// Inbound auth (CCS-049)
// ============================================================================

fn router_request(
    host: &str,
    content_type: &str,
    extra_headers: &[(&str, &str)],
) -> axum::http::Request<axum::body::Body> {
    let mut builder = axum::http::Request::builder()
        .method("POST")
        .uri("/v1/messages")
        .header("host", host)
        .header("content-type", content_type)
        .header("anthropic-version", "2023-06-01");
    for (name, value) in extra_headers {
        builder = builder.header(*name, *value);
    }
    builder
        .body(axum::body::Body::from(simple_request().to_string()))
        .expect("build router request")
}

#[tokio::test]
async fn router_rejects_untrusted_requests_before_the_forwarder() {
    use tower::ServiceExt;

    let _env = TestEnv::new();
    let upstream = MockUpstream::always(MockReply::message("leaked", 10, 5)).await;
    let harness = ProxyHarness::spawn(&upstream).await;
    let loopback_host = format!("127.0.0.1:{}", harness.port());
    let evil_host = format!("evil.example:{}", harness.port());
    let token = harness.token().to_string();

    let cases: Vec<(&str, axum::http::Request<axum::body::Body>, u16)> = vec![
        (
            "text/plain POST with Origin evil.example",
            router_request(
                &loopback_host,
                "text/plain",
                &[("origin", "https://evil.example"), ("x-api-key", &token)],
            ),
            403,
        ),
        (
            "Host evil.example:PORT",
            router_request(&evil_host, "application/json", &[("x-api-key", &token)]),
            403,
        ),
        (
            "missing token",
            router_request(&loopback_host, "application/json", &[]),
            401,
        ),
        (
            "wrong token",
            router_request(
                &loopback_host,
                "application/json",
                &[("x-api-key", "ccs-proxy-ffffffffffffffffffffffffffffffff")],
            ),
            401,
        ),
        (
            "real-looking upstream key instead of the proxy token",
            router_request(
                &loopback_host,
                "application/json",
                &[("authorization", "Bearer sk-ant-not-the-proxy-token")],
            ),
            401,
        ),
    ];

    for (name, request, expected) in cases {
        let response = harness
            .router()
            .oneshot(request)
            .await
            .expect("router response");
        assert_eq!(response.status().as_u16(), expected, "{name}");
    }
    assert_eq!(upstream.hits(), 0, "no rejected request reaches upstream");
    assert!(
        harness.request_logs().is_empty(),
        "rejected requests are not logged as proxied traffic"
    );

    // A loopback Host with the valid token reaches the mock upstream.
    let response = harness
        .router()
        .oneshot(router_request(
            &loopback_host,
            "application/json",
            &[("x-api-key", &token)],
        ))
        .await
        .expect("router response");
    assert_eq!(response.status(), 200);
    assert_eq!(upstream.hits(), 1);
    assert_eq!(
        upstream.requests()[0].headers["authorization"],
        "Bearer sk-harness-p1",
        "the proxy token is replaced by the provider credential"
    );
    assert!(
        !upstream.requests()[0].headers.contains_key("x-api-key"),
        "the proxy token never reaches upstream"
    );

    harness.shutdown().await;
}

#[tokio::test]
async fn router_returns_400_for_malformed_json_on_every_body_reading_route() {
    use tower::ServiceExt;

    let _env = TestEnv::new();
    let upstream = MockUpstream::always(MockReply::message("leaked", 10, 5)).await;
    let harness = ProxyHarness::spawn(&upstream).await;
    let host = format!("127.0.0.1:{}", harness.port());
    let token = harness.token().to_string();

    // 覆盖 ClientEndpoint 表生成的全部 POST 路由别名（Claude Desktop 需 gateway token，单独鉴权）
    for uri in [
        "/v1/messages",
        "/claude/v1/messages",
        "/chat/completions",
        "/v1/chat/completions",
        "/v1/v1/chat/completions",
        "/codex/v1/chat/completions",
        "/responses",
        "/v1/responses",
        "/v1/v1/responses",
        "/codex/v1/responses",
        "/responses/compact",
        "/v1/responses/compact",
        "/v1/v1/responses/compact",
        "/codex/v1/responses/compact",
        "/grokbuild/v1/responses",
        "/grokbuild/v1/responses/compact",
        "/v1beta/models/gemini-pro:generateContent",
        "/gemini/v1beta/models/gemini-pro:generateContent",
        "/gemini/v1/models/gemini-pro:generateContent",
    ] {
        let request = axum::http::Request::builder()
            .method("POST")
            .uri(uri)
            .header("host", &host)
            .header("content-type", "application/json")
            .header("x-api-key", &token)
            .header("x-goog-api-key", &token)
            .body(axum::body::Body::from("{not json"))
            .expect("build router request");
        let response = harness
            .router()
            .oneshot(request)
            .await
            .expect("router response");
        assert_eq!(response.status().as_u16(), 400, "{uri}");
    }
    assert_eq!(upstream.hits(), 0, "malformed bodies never reach upstream");

    harness.shutdown().await;
}

#[tokio::test]
async fn router_applies_per_endpoint_flags_from_the_endpoint_table() {
    use tower::ServiceExt;

    let _env = TestEnv::new();
    let upstream = MockUpstream::always(MockReply::message("leaked", 10, 5)).await;
    let harness = ProxyHarness::spawn(&upstream).await;
    let host = format!("127.0.0.1:{}", harness.port());
    let token = harness.token().to_string();

    // Claude Desktop 端点需 gateway token：缺少 Authorization 时在读取请求体之前就返回 401
    let request = axum::http::Request::builder()
        .method("POST")
        .uri("/claude-desktop/v1/messages")
        .header("host", &host)
        .header("content-type", "application/json")
        .header("x-api-key", &token)
        .body(axum::body::Body::from("{not json"))
        .expect("build router request");
    let response = harness
        .router()
        .oneshot(request)
        .await
        .expect("router response");
    assert_eq!(response.status().as_u16(), 401);

    // Gemini 端点挂载为 any(..) 且允许空请求体：GET 只读端点不会在路由层 404/405，
    // 也不会因空 body 被当成畸形 JSON 拒绝
    for uri in [
        "/v1beta/models",
        "/gemini/v1beta/models",
        "/gemini/v1/models",
    ] {
        let request = axum::http::Request::builder()
            .method("GET")
            .uri(uri)
            .header("host", &host)
            .header("x-goog-api-key", &token)
            .body(axum::body::Body::empty())
            .expect("build router request");
        let response = harness
            .router()
            .oneshot(request)
            .await
            .expect("router response");
        let status = response.status().as_u16();
        assert!(
            !matches!(status, 400 | 404 | 405),
            "{uri} returned {status}"
        );
    }

    // 其余端点只挂 POST
    let request = axum::http::Request::builder()
        .method("GET")
        .uri("/v1/messages")
        .header("host", &host)
        .header("x-api-key", &token)
        .body(axum::body::Body::empty())
        .expect("build router request");
    let response = harness
        .router()
        .oneshot(request)
        .await
        .expect("router response");
    assert_eq!(response.status().as_u16(), 405);
    assert_eq!(upstream.hits(), 0);

    harness.shutdown().await;
}

#[tokio::test]
async fn foreign_host_text_plain_post_is_rejected_on_the_wire() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let _env = TestEnv::new();
    let upstream = MockUpstream::always(MockReply::message("leaked", 10, 5)).await;
    let harness = ProxyHarness::spawn(&upstream).await;

    // curl -H 'Host: attacker.test' -H 'Content-Type: text/plain' -d '{}' http://127.0.0.1:PORT/v1/messages
    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", harness.port()))
        .await
        .expect("connect to proxy");
    stream
        .write_all(
            b"POST /v1/messages HTTP/1.1\r\nHost: attacker.test\r\nContent-Type: text/plain\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}",
        )
        .await
        .expect("write raw request");
    let mut raw = Vec::new();
    stream
        .read_to_end(&mut raw)
        .await
        .expect("read raw response");
    let raw = String::from_utf8_lossy(&raw);
    assert!(
        raw.starts_with("HTTP/1.1 403"),
        "unexpected response: {raw}"
    );
    assert_eq!(upstream.hits(), 0);

    // The same request from a real client with the token goes through.
    let response = harness.messages(&simple_request()).await;
    assert_eq!(response.status(), 200);
    assert_eq!(upstream.hits(), 1);

    harness.shutdown().await;
}

// ============================================================================
// Header casing on the wire
// ============================================================================

/// Head of a raw `/v1/messages` request to the proxy; `extra` header lines go
/// before the body framing headers.
fn raw_messages_head(harness: &ProxyHarness, extra: &str, body: &str) -> String {
    format!(
        "POST /v1/messages HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nX-Api-Key: {}\r\n\
         Anthropic-Version: 2023-06-01\r\n{extra}Content-Type: application/json\r\n\
         Content-Length: {}\r\n\r\n",
        harness.port(),
        harness.token(),
        body.len()
    )
}

/// Read one HTTP/1.1 response (Content-Length or chunked) and return its status.
async fn read_raw_response(stream: &mut tokio::net::TcpStream, buf: &mut Vec<u8>) -> u16 {
    use tokio::io::AsyncReadExt;

    async fn fill(stream: &mut tokio::net::TcpStream, buf: &mut Vec<u8>) {
        let mut chunk = [0u8; 8192];
        let n = stream.read(&mut chunk).await.expect("read proxy response");
        assert!(n > 0, "proxy closed the connection");
        buf.extend_from_slice(&chunk[..n]);
    }
    fn find(buf: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
        buf.get(from..)?
            .windows(needle.len())
            .position(|w| w == needle)
            .map(|pos| pos + from)
    }

    let head_end = loop {
        if let Some(pos) = find(buf, 0, b"\r\n\r\n") {
            break pos + 4;
        }
        fill(stream, buf).await;
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).to_ascii_lowercase();
    let status: u16 = head[9..12].parse().expect("status code");
    let header = |name: &str| {
        head.lines()
            .find_map(|line| line.strip_prefix(name))
            .map(|value| value.trim().to_string())
    };
    let end = if let Some(length) = header("content-length:") {
        let end = head_end + length.parse::<usize>().expect("content length");
        while buf.len() < end {
            fill(stream, buf).await;
        }
        end
    } else {
        assert_eq!(header("transfer-encoding:").as_deref(), Some("chunked"));
        let mut pos = head_end;
        loop {
            let line_end = loop {
                if let Some(at) = find(buf, pos, b"\r\n") {
                    break at;
                }
                fill(stream, buf).await;
            };
            let size_line = String::from_utf8_lossy(&buf[pos..line_end]).into_owned();
            let size = usize::from_str_radix(size_line.split(';').next().unwrap().trim(), 16)
                .expect("chunk size");
            pos = line_end + 2;
            if size == 0 {
                // No trailers from the proxy: just the final CRLF.
                while buf.len() < pos + 2 {
                    fill(stream, buf).await;
                }
                break pos + 2;
            }
            while buf.len() < pos + size + 2 {
                fill(stream, buf).await;
            }
            pos += size + 2;
        }
    };
    buf.drain(..end);
    status
}

#[tokio::test]
async fn original_header_casing_is_captured_per_request_on_keep_alive_connections() {
    use tokio::io::AsyncWriteExt;

    let _env = TestEnv::new();
    let upstream = RawMockUpstream::spawn().await;
    let harness = ProxyHarness::builder()
        .provider_url("raw", upstream.base_url())
        .spawn()
        .await;
    let body = simple_request().to_string();
    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", harness.port()))
        .await
        .expect("connect to proxy");
    stream.set_nodelay(true).expect("disable nagle");
    let mut buf = Vec::new();

    // A: the first request on the connection.
    let a = raw_messages_head(&harness, "X-Foo-A: 1\r\n", &body) + &body;
    stream.write_all(a.as_bytes()).await.expect("write A");
    assert_eq!(read_raw_response(&mut stream, &mut buf).await, 200);

    // B: a later keep-alive request whose head is larger than 8 KB.
    let padding = format!("X-Padding: {}\r\n", "p".repeat(9_000));
    let b = raw_messages_head(
        &harness,
        &format!("X-Bar-B: 1\r\n{padding}X-After-Eight-K: 1\r\n"),
        &body,
    ) + &body;
    stream.write_all(b.as_bytes()).await.expect("write B");
    assert_eq!(read_raw_response(&mut stream, &mut buf).await, 200);

    // C: a head split across two writes, in the middle of a header name.
    let c = raw_messages_head(&harness, "X-Split-Header: 1\r\n", &body) + &body;
    let split = c.find("X-Split-He").expect("split marker") + "X-Split-He".len();
    stream
        .write_all(&c.as_bytes()[..split])
        .await
        .expect("write C part 1");
    stream.flush().await.expect("flush C part 1");
    tokio::time::sleep(Duration::from_millis(50)).await;
    stream
        .write_all(&c.as_bytes()[split..])
        .await
        .expect("write C part 2");
    assert_eq!(read_raw_response(&mut stream, &mut buf).await, 200);

    let heads = upstream.heads();
    assert_eq!(heads.len(), 3, "upstream heads: {heads:#?}");
    let a_names = upstream.header_names(0);
    assert!(a_names.iter().any(|n| n == "X-Foo-A"), "A: {a_names:?}");
    assert!(
        a_names.iter().any(|n| n == "Anthropic-Version"),
        "A: {a_names:?}"
    );

    let b_names = upstream.header_names(1);
    assert!(b_names.iter().any(|n| n == "X-Bar-B"), "B: {b_names:?}");
    assert!(
        b_names.iter().any(|n| n == "X-After-Eight-K"),
        "B: {b_names:?}"
    );
    assert!(!b_names.iter().any(|n| n == "X-Foo-A"), "B: {b_names:?}");

    let c_names = upstream.header_names(2);
    assert!(
        c_names.iter().any(|n| n == "X-Split-Header"),
        "C: {c_names:?}"
    );

    harness.shutdown().await;
}

// ============================================================================
// Runtime config snapshot
// ============================================================================

#[tokio::test]
async fn config_and_provider_changes_reach_the_next_request_without_restart() {
    let _env = TestEnv::new();
    let a = MockUpstream::messages("from a", 10, 5).await;
    let b = MockUpstream::messages("from b", 10, 5).await;
    let harness = ProxyHarness::builder()
        .provider("a", &a)
        .provider("b", &b)
        .spawn()
        .await;

    // Warm the snapshot on the current provider.
    let response = harness.messages(&simple_request()).await;
    assert_eq!(first_text(&json_body(response).await), "from a");

    // Switching the current provider is a plain DB write; the cached snapshot
    // must notice it on the very next request.
    harness
        .db()
        .set_current_provider("claude", "b")
        .expect("switch current provider");
    let response = harness.messages(&simple_request()).await;
    assert_eq!(first_text(&json_body(response).await), "from b");

    // Same for per-app proxy_config: enable failover with only `a` queued.
    harness
        .db()
        .add_to_failover_queue("claude", "a")
        .expect("queue a");
    let mut config = harness
        .db()
        .get_proxy_config_for_app("claude")
        .await
        .expect("read proxy config");
    config.auto_failover_enabled = true;
    harness
        .db()
        .update_proxy_config_for_app(config)
        .await
        .expect("enable failover");
    let response = harness.messages(&simple_request()).await;
    assert_eq!(first_text(&json_body(response).await), "from a");

    assert_eq!(a.hits(), 2);
    assert_eq!(b.hits(), 1);
    harness.shutdown().await;
}

// ============================================================================
// Baseline: latency, allocations and SQL statements per request
// ============================================================================

const WARMUP_REQUESTS: usize = 5;
const MEASURED_REQUESTS: usize = 50;

fn baseline_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("docs")
        .join("dev")
        .join("perf-baseline.json")
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    let rank = ((p / 100.0) * (sorted.len() - 1) as f64).round() as usize;
    sorted[rank.min(sorted.len() - 1)]
}

fn round2(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

#[tokio::test]
async fn perf_baseline_non_streaming_request() {
    let _env = TestEnv::new();
    let upstream = MockUpstream::messages("ok", 100, 20).await;
    let harness = ProxyHarness::spawn(&upstream).await;
    let request = simple_request();

    for _ in 0..WARMUP_REQUESTS {
        let response = harness.messages(&request).await;
        assert_eq!(response.status(), 200);
        response.bytes().await.expect("read warm-up body");
    }
    harness.wait_for_request_logs(WARMUP_REQUESTS).await;
    tokio::time::sleep(Duration::from_millis(100)).await;

    let statements_before = db_statement_count();
    let reads_before = db_read_statement_count();
    let (allocs_before, bytes_before) = allocation_snapshot();
    let mut latencies_ms = Vec::with_capacity(MEASURED_REQUESTS);
    for _ in 0..MEASURED_REQUESTS {
        let started = Instant::now();
        let response = harness.messages(&request).await;
        assert_eq!(response.status(), 200);
        response.bytes().await.expect("read body");
        latencies_ms.push(started.elapsed().as_secs_f64() * 1000.0);
    }
    // Usage rows and health updates land in spawned tasks; let them finish
    // before reading the counters (the harness's own reads are not counted).
    let logs = harness
        .wait_for_request_logs(WARMUP_REQUESTS + MEASURED_REQUESTS)
        .await;
    assert_eq!(logs.len(), WARMUP_REQUESTS + MEASURED_REQUESTS);
    tokio::time::sleep(Duration::from_millis(100)).await;
    let statements = db_statement_count() - statements_before;
    let reads = db_read_statement_count() - reads_before;
    let (allocs_after, bytes_after) = allocation_snapshot();

    latencies_ms.sort_by(|a, b| a.partial_cmp(b).expect("finite latency"));
    let n = MEASURED_REQUESTS as f64;
    let statements_per_request = statements as f64 / n;
    let reads_per_request = reads as f64 / n;
    let allocations_per_request = (allocs_after - allocs_before) as f64 / n;
    let measured = json!({
        "scenario": "non-streaming Claude /v1/messages, single provider, usage logging on",
        "requests": MEASURED_REQUESTS,
        "profile": if cfg!(debug_assertions) { "debug" } else { "release" },
        "platform": format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
        "latency_ms": {
            "p50": round2(percentile(&latencies_ms, 50.0)),
            "p95": round2(percentile(&latencies_ms, 95.0)),
            "max": round2(*latencies_ms.last().expect("latencies")),
        },
        "db_statements_per_request": round2(statements_per_request),
        "db_reads_per_request": round2(reads_per_request),
        "allocations_per_request": allocations_per_request.round(),
        "allocated_bytes_per_request": ((bytes_after - bytes_before) as f64 / n).round(),
    });

    // JSON artifact for CI upload / local comparison.
    let artifact = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("proxy-e2e-perf.json");
    std::fs::write(
        &artifact,
        serde_json::to_string_pretty(&measured).expect("serialize measurement"),
    )
    .expect("write perf artifact");
    println!(
        "proxy e2e perf: {measured}\n(written to {})",
        artifact.display()
    );

    let path = baseline_path();
    let mut baseline: Value =
        serde_json::from_str(&std::fs::read_to_string(&path).expect("read perf baseline"))
            .expect("parse perf baseline");

    if std::env::var_os("CC_SWITCH_UPDATE_PERF_BASELINE").is_some() {
        let mut entry = measured.clone();
        entry["_note"] = json!(
            "Proxy e2e baseline (CCS-014), measured by tests/proxy_e2e.rs::perf_baseline_non_streaming_request. \
             Refresh with `CC_SWITCH_UPDATE_PERF_BASELINE=1 cargo test --manifest-path src-tauri/Cargo.toml --test proxy_e2e perf_baseline`, then `npx prettier --write docs/dev/perf-baseline.json`. \
             Gates: db_reads_per_request must be 0 (runtime config snapshot, CCS-124); db_statements_per_request must not grow; allocations_per_request may not exceed 3x. Latency is informational."
        );
        baseline["proxy_e2e"] = entry;
        std::fs::write(
            &path,
            serde_json::to_string_pretty(&baseline).expect("serialize baseline") + "\n",
        )
        .expect("write perf baseline");
        return;
    }

    // Steady-state requests are served from the runtime config snapshot (CCS-124):
    // the only statements left are the request-log and provider-health writes.
    assert_eq!(
        reads, 0,
        "proxied requests must not read the database once the runtime config snapshot is warm \
         ({reads} SELECTs over {MEASURED_REQUESTS} requests)"
    );

    let recorded = &baseline["proxy_e2e"];
    let max_statements = recorded["db_statements_per_request"]
        .as_f64()
        .expect("baseline db_statements_per_request");
    assert!(
        statements_per_request <= max_statements,
        "SQL statements per proxied request grew: {statements_per_request} > baseline {max_statements}. \
         If intended, refresh docs/dev/perf-baseline.json (see its proxy_e2e._note)."
    );
    let baseline_allocations = recorded["allocations_per_request"]
        .as_f64()
        .expect("baseline allocations_per_request");
    assert!(
        allocations_per_request <= baseline_allocations * 3.0,
        "allocations per proxied request grew: {allocations_per_request} > 3x baseline {baseline_allocations}"
    );

    harness.shutdown().await;
}
