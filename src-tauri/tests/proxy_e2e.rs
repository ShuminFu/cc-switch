//! In-process end-to-end tests of the proxy request path (TEST-01 / PROXY-06).
//!
//! Each test starts the real proxy (`ProxyService::start`, which binds
//! `ProxyServer` on 127.0.0.1 with `listen_port = 0`) against one or two mock upstreams
//! (axum on random ports) and drives real HTTP requests through it with reqwest:
//! listener -> handler -> forwarder (retry/failover/circuit breaker) -> format
//! transform -> streaming -> client. No Tauri runtime or `AppHandle` is needed: the
//! proxy runs with `app_handle = None`, which only disables the Copilot / Codex-OAuth
//! token-injection branches (those managed-auth providers are therefore not covered here;
//! a `ManagedAuthResolver`-style seam would be needed to test them).
//!
//! Upstream request bodies are compared against golden files under
//! `tests/fixtures/proxy/e2e/`; run with `UPDATE_GOLDEN=1` to rewrite them after an
//! intentional change.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::Router;
use cc_switch_lib::{Database, Provider, ProviderMeta, ProxyService};
use futures::StreamExt;
use serde_json::{json, Value};

#[path = "support.rs"]
mod support;

// ---------------------------------------------------------------------------
// Mock upstream
// ---------------------------------------------------------------------------

/// What the mock upstream answers.
#[derive(Clone)]
enum Reply {
    /// JSON body with the given status.
    Json(u16, Value),
    /// `text/event-stream` body sent as-is.
    Sse(String),
    /// `text/event-stream` that sends the given bytes and then drops the connection
    /// without terminating the chunked body.
    SseThenDisconnect(String),
    /// `text/event-stream` that sends the given bytes and then goes silent forever.
    SseThenHang(String),
    /// One reply per request, in order; the last one repeats.
    Sequence(Vec<Reply>),
    /// 200 JSON body gzip-compressed with `content-encoding: gzip`.
    GzipJson(Value),
}

#[derive(Clone, Debug)]
struct Recorded {
    path: String,
    headers: HeaderMap,
    body: Value,
    /// The exact bytes the upstream received.
    raw: Bytes,
}

struct MockState {
    reply: Mutex<Reply>,
    hits: AtomicUsize,
    requests: Mutex<Vec<Recorded>>,
}

struct MockUpstream {
    addr: SocketAddr,
    state: Arc<MockState>,
}

impl MockUpstream {
    async fn start(reply: Reply) -> Self {
        let state = Arc::new(MockState {
            reply: Mutex::new(reply),
            hits: AtomicUsize::new(0),
            requests: Mutex::new(Vec::new()),
        });
        let app = Router::new()
            .fallback(mock_handler)
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock upstream");
        let addr = listener.local_addr().expect("mock addr");
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("mock upstream");
        });
        Self { addr, state }
    }

    fn base_url(&self) -> String {
        format!("http://{}", self.addr)
    }

    fn hits(&self) -> usize {
        self.state.hits.load(Ordering::SeqCst)
    }

    fn set_reply(&self, reply: Reply) {
        *self.state.reply.lock().unwrap() = reply;
    }

    fn last_request(&self) -> Recorded {
        self.state
            .requests
            .lock()
            .unwrap()
            .last()
            .cloned()
            .expect("upstream received no request")
    }
}

async fn mock_handler(
    State(state): State<Arc<MockState>>,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let hit = state.hits.fetch_add(1, Ordering::SeqCst);
    let raw = body.clone();
    let body: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    state.requests.lock().unwrap().push(Recorded {
        path: uri.path().to_string(),
        headers,
        body,
        raw,
    });
    let mut reply = state.reply.lock().unwrap().clone();
    if let Reply::Sequence(replies) = reply {
        reply = replies[hit.min(replies.len() - 1)].clone();
    }
    match reply {
        Reply::Json(status, value) => (
            StatusCode::from_u16(status).unwrap(),
            [("content-type", "application/json")],
            value.to_string(),
        )
            .into_response(),
        Reply::Sse(text) => (
            StatusCode::OK,
            [("content-type", "text/event-stream")],
            text,
        )
            .into_response(),
        Reply::SseThenDisconnect(text) => {
            let chunks: Vec<Result<Bytes, std::io::Error>> = vec![
                Ok(Bytes::from(text)),
                Err(std::io::Error::new(
                    std::io::ErrorKind::ConnectionReset,
                    "mock upstream disconnect",
                )),
            ];
            let stream = futures::stream::iter(chunks).then(|item| async move {
                // Let the first chunk reach the proxy before the connection drops.
                tokio::time::sleep(Duration::from_millis(50)).await;
                item
            });
            Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "text/event-stream")
                .body(Body::from_stream(stream))
                .unwrap()
        }
        Reply::SseThenHang(text) => {
            let stream =
                futures::stream::once(async move { Ok::<_, std::io::Error>(Bytes::from(text)) })
                    .chain(futures::stream::pending());
            Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "text/event-stream")
                .body(Body::from_stream(stream))
                .unwrap()
        }
        Reply::GzipJson(value) => (
            StatusCode::OK,
            [
                ("content-type", "application/json"),
                ("content-encoding", "gzip"),
            ],
            gzip(value.to_string().as_bytes()),
        )
            .into_response(),
        Reply::Sequence(_) => unreachable!("nested sequences are not supported"),
    }
}

fn gzip(data: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(data).unwrap();
    encoder.finish().unwrap()
}

// ---------------------------------------------------------------------------
// Proxy harness
// ---------------------------------------------------------------------------

struct Harness {
    service: ProxyService,
    port: u16,
    client: reqwest::Client,
}

impl Harness {
    fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.port)
    }

    async fn post(&self, path: &str, body: &Value) -> reqwest::Response {
        self.client
            .post(self.url(path))
            .header("content-type", "application/json")
            .header("anthropic-version", "2023-06-01")
            .header("x-api-key", "client-placeholder-key")
            .json(body)
            .send()
            .await
            .expect("send request through proxy")
    }

    async fn post_raw(&self, path: &str, body: &'static str) -> reqwest::Response {
        self.client
            .post(self.url(path))
            .header("content-type", "application/json")
            .header("anthropic-version", "2023-06-01")
            .header("x-api-key", "client-placeholder-key")
            .body(body)
            .send()
            .await
            .expect("send raw request through proxy")
    }

    async fn stop(self) {
        let _ = self.service.stop().await;
    }
}

fn claude_provider(id: &str, base_url: &str, api_format: Option<&str>) -> Provider {
    let mut provider = Provider::with_id(
        id.to_string(),
        format!("Mock {id}"),
        json!({
            "env": {
                "ANTHROPIC_BASE_URL": base_url,
                "ANTHROPIC_AUTH_TOKEN": format!("sk-test-{id}"),
            }
        }),
        None,
    );
    if let Some(format) = api_format {
        provider.meta = Some(ProviderMeta {
            api_format: Some(format.to_string()),
            ..Default::default()
        });
    }
    provider
}

/// Seed an in-memory database with Claude providers (in failover-queue order), enable
/// failover when there is more than one, and start the proxy on an ephemeral port.
async fn start_proxy(providers: &[Provider], circuit_failure_threshold: u32) -> Harness {
    start_proxy_with(providers, circuit_failure_threshold, Overrides::default()).await
}

/// Per-app proxy config overrides for [`start_proxy_with`].
#[derive(Default)]
struct Overrides {
    /// Enable failover (and queue every provider) even with a single provider.
    failover: bool,
    streaming_first_byte_timeout: Option<u32>,
    streaming_idle_timeout: Option<u32>,
    /// Routing config for the Claude app (PROXY-19), stored in the settings table.
    routing: Option<Value>,
}

/// [`start_proxy`] with per-app config overrides.
async fn start_proxy_with(
    providers: &[Provider],
    circuit_failure_threshold: u32,
    overrides: Overrides,
) -> Harness {
    support::ensure_test_home();
    let db = Arc::new(Database::memory().expect("memory db"));
    for provider in providers {
        db.save_provider("claude", provider).expect("save provider");
    }
    db.set_current_provider("claude", &providers[0].id)
        .expect("set current provider");

    let mut app_config = db
        .get_proxy_config_for_app("claude")
        .await
        .expect("app proxy config");
    app_config.enabled = true;
    app_config.auto_failover_enabled = providers.len() > 1;
    app_config.circuit_failure_threshold = circuit_failure_threshold;
    app_config.circuit_timeout_seconds = 600;
    app_config.auto_failover_enabled |= overrides.failover;
    if let Some(v) = overrides.streaming_first_byte_timeout {
        app_config.streaming_first_byte_timeout = v;
    }
    if let Some(v) = overrides.streaming_idle_timeout {
        app_config.streaming_idle_timeout = v;
    }
    let failover = app_config.auto_failover_enabled;
    db.update_proxy_config_for_app(app_config)
        .await
        .expect("update app proxy config");
    if failover {
        for provider in providers {
            db.add_to_failover_queue("claude", &provider.id)
                .expect("add to failover queue");
        }
    }

    if let Some(routing) = overrides.routing {
        db.set_setting(
            "proxy_routing_config",
            &json!({ "claude": routing }).to_string(),
        )
        .expect("store routing config");
    }

    let mut proxy_config = db.get_proxy_config().await.expect("proxy config");
    proxy_config.listen_address = "127.0.0.1".to_string();
    proxy_config.listen_port = 0;
    db.update_proxy_config(proxy_config)
        .await
        .expect("update proxy config");

    let service = ProxyService::new(db);
    let info = service.start().await.expect("start proxy");
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(30))
        .build()
        .expect("client");
    Harness {
        service,
        port: info.port,
        client,
    }
}

/// The proxy keeps some process-wide state (the bound proxy port used for system-proxy
/// detection, the settings cache), so scenarios run one at a time.
fn serial() -> &'static tokio::sync::Mutex<()> {
    static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/proxy")
}

fn read_fixture(rel: &str) -> String {
    std::fs::read_to_string(fixtures_dir().join(rel))
        .unwrap_or_else(|e| panic!("read fixture {rel}: {e}"))
}

fn read_json_fixture(rel: &str) -> Value {
    serde_json::from_str(&read_fixture(rel)).unwrap_or_else(|e| panic!("parse {rel}: {e}"))
}

/// Compare `actual` with `tests/fixtures/proxy/e2e/<name>.json` (rewritten under
/// `UPDATE_GOLDEN=1`).
fn assert_golden(name: &str, actual: &Value) {
    let path = fixtures_dir().join("e2e").join(format!("{name}.json"));
    let rendered = serde_json::to_string_pretty(actual).unwrap() + "\n";
    if std::env::var("UPDATE_GOLDEN").is_ok_and(|v| v == "1") {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, rendered).unwrap();
        return;
    }
    let expected: Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap_or_else(|e| {
            panic!(
                "missing golden {} ({e}); run with UPDATE_GOLDEN=1",
                path.display()
            )
        }))
        .expect("golden json");
    assert_eq!(
        &expected,
        actual,
        "golden mismatch for {}\n--- actual ---\n{rendered}",
        path.display()
    );
}

/// Parse an SSE body into `(event, data)` pairs (data parsed as JSON when possible).
fn sse_events(text: &str) -> Vec<(Option<String>, Value)> {
    text.replace("\r\n", "\n")
        .split("\n\n")
        .filter(|block| !block.trim().is_empty())
        .filter_map(|block| {
            let mut event = None;
            let mut data = Vec::new();
            for line in block.lines() {
                if let Some(rest) = line.strip_prefix("event:") {
                    event = Some(rest.trim().to_string());
                } else if let Some(rest) = line.strip_prefix("data:") {
                    data.push(rest.trim_start().to_string());
                }
            }
            if data.is_empty() {
                return None;
            }
            let raw = data.join("\n");
            Some((
                event,
                serde_json::from_str(&raw).unwrap_or(Value::String(raw)),
            ))
        })
        .collect()
}

fn event_types(events: &[(Option<String>, Value)]) -> Vec<String> {
    events
        .iter()
        .map(|(event, data)| {
            event
                .clone()
                .or_else(|| data["type"].as_str().map(str::to_string))
                .unwrap_or_default()
        })
        .collect()
}

fn anthropic_message(text: &str) -> Value {
    json!({
        "id": "msg_e2e_1",
        "type": "message",
        "role": "assistant",
        "model": "claude-sonnet-4-5",
        "content": [{"type": "text", "text": text}],
        "stop_reason": "end_turn",
        "stop_sequence": null,
        "usage": {"input_tokens": 12, "output_tokens": 3}
    })
}

fn simple_request(stream: bool) -> Value {
    json!({
        "model": "claude-sonnet-4-5",
        "max_tokens": 256,
        "stream": stream,
        "messages": [{"role": "user", "content": "Say hello"}]
    })
}

fn bearer(recorded: &Recorded) -> Option<String> {
    recorded
        .headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
}

// ---------------------------------------------------------------------------
// Scenarios
// ---------------------------------------------------------------------------

/// Anthropic passthrough, non-streaming: the client gets the upstream body back, the
/// upstream sees the client body, the provider's credentials (not the client's
/// placeholder) and the `/v1/messages` endpoint.
#[tokio::test]
async fn anthropic_passthrough_non_stream_success() {
    let _guard = serial().lock().await;
    let upstream = MockUpstream::start(Reply::Json(200, anthropic_message("Hello!"))).await;
    let harness = start_proxy(&[claude_provider("a", &upstream.base_url(), None)], 3).await;

    let response = harness.post("/v1/messages", &simple_request(false)).await;
    assert_eq!(response.status(), 200);
    let body: Value = response.json().await.expect("json response");
    assert_eq!(body, anthropic_message("Hello!"));

    let recorded = upstream.last_request();
    assert_eq!(recorded.path, "/v1/messages");
    assert_eq!(bearer(&recorded).as_deref(), Some("Bearer sk-test-a"));
    assert_ne!(
        recorded
            .headers
            .get("x-api-key")
            .and_then(|v| v.to_str().ok()),
        Some("client-placeholder-key"),
        "the client's placeholder key must not leak upstream"
    );
    assert_golden("anthropic_passthrough_upstream_body", &recorded.body);
    harness.stop().await;
}

/// Anthropic passthrough, streaming: the event sequence reaches the client unchanged.
#[tokio::test]
async fn anthropic_passthrough_stream_success() {
    let _guard = serial().lock().await;
    let sse = read_fixture("codex_anthropic/stream/text_utf8.input.sse");
    let upstream = MockUpstream::start(Reply::Sse(sse.clone())).await;
    let harness = start_proxy(&[claude_provider("a", &upstream.base_url(), None)], 3).await;

    let response = harness.post("/v1/messages", &simple_request(true)).await;
    assert_eq!(response.status(), 200);
    assert!(response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("text/event-stream")));
    let text = response.text().await.expect("stream body");
    let received: Vec<_> = sse_events(&text)
        .into_iter()
        .filter(|(event, _)| event.as_deref() != Some("ping"))
        .collect();
    let sent: Vec<_> = sse_events(&sse)
        .into_iter()
        .filter(|(event, _)| event.as_deref() != Some("ping"))
        .collect();
    assert_eq!(received, sent);
    harness.stop().await;
}

/// Claude client -> OpenAI Chat upstream (non-streaming): the upstream receives the
/// converted Chat request on `/v1/chat/completions` and the client gets an Anthropic
/// message converted from the Chat response.
#[tokio::test]
async fn openai_chat_bridge_non_stream() {
    let _guard = serial().lock().await;
    let chat_response = read_json_fixture("anthropic_chat/response/text.input.json");
    let upstream = MockUpstream::start(Reply::Json(200, chat_response)).await;
    let harness = start_proxy(
        &[claude_provider(
            "a",
            &upstream.base_url(),
            Some("openai_chat"),
        )],
        3,
    )
    .await;

    let request = read_json_fixture("anthropic_chat/request/text_with_system.input.json");
    let response = harness.post("/v1/messages", &request).await;
    assert_eq!(response.status(), 200);
    let body: Value = response.json().await.expect("json response");
    // Same converter as the golden corpus: the HTTP path must not alter the result.
    assert_eq!(
        body,
        read_json_fixture("anthropic_chat/response/text.expected.json")
    );

    let recorded = upstream.last_request();
    assert!(
        recorded.path.ends_with("/chat/completions"),
        "unexpected upstream path {}",
        recorded.path
    );
    assert_golden("openai_chat_bridge_upstream_body", &recorded.body);
    harness.stop().await;
}

/// Claude client -> OpenAI Chat upstream (streaming): the Chat SSE is re-encoded as
/// the Anthropic event sequence pinned by the golden corpus.
#[tokio::test]
async fn openai_chat_bridge_stream() {
    let _guard = serial().lock().await;
    let sse = read_fixture("anthropic_chat/stream/tool_calls_with_index.input.sse");
    let upstream = MockUpstream::start(Reply::Sse(sse)).await;
    let harness = start_proxy(
        &[claude_provider(
            "a",
            &upstream.base_url(),
            Some("openai_chat"),
        )],
        3,
    )
    .await;

    let mut request = read_json_fixture("anthropic_chat/request/tool_use_roundtrip.input.json");
    request["stream"] = json!(true);
    let response = harness.post("/v1/messages", &request).await;
    assert_eq!(response.status(), 200);
    let text = response.text().await.expect("stream body");
    let received: Vec<Value> = sse_events(&text)
        .into_iter()
        .map(|(event, data)| match event {
            Some(event) => json!({"event": event, "data": data}),
            None => json!({"data": data}),
        })
        .collect();
    let expected = read_json_fixture("anthropic_chat/stream/tool_calls_with_index.expected.json");
    assert_eq!(Value::Array(received), expected);

    let recorded = upstream.last_request();
    assert_eq!(recorded.body["stream"], json!(true));
    harness.stop().await;
}

/// Upstream 5xx on the first provider fails over to the second one.
#[tokio::test]
async fn upstream_500_fails_over_to_second_provider() {
    let _guard = serial().lock().await;
    let failing = MockUpstream::start(Reply::Json(
        500,
        json!({"type": "error", "error": {"type": "api_error", "message": "boom"}}),
    ))
    .await;
    let healthy = MockUpstream::start(Reply::Json(200, anthropic_message("from B"))).await;
    let harness = start_proxy(
        &[
            claude_provider("a", &failing.base_url(), None),
            claude_provider("b", &healthy.base_url(), None),
        ],
        5,
    )
    .await;

    let response = harness.post("/v1/messages", &simple_request(false)).await;
    assert_eq!(response.status(), 200);
    let body: Value = response.json().await.expect("json response");
    assert_eq!(body, anthropic_message("from B"));
    assert!(failing.hits() >= 1, "first provider was not tried");
    assert_eq!(healthy.hits(), 1);
    assert_eq!(
        bearer(&healthy.last_request()).as_deref(),
        Some("Bearer sk-test-b"),
        "failover must switch credentials to the second provider"
    );
    harness.stop().await;
}

fn session_request(session: &str) -> Value {
    let mut body = simple_request(false);
    body["metadata"] = json!({ "session_id": session });
    body
}

/// Runs: P1 fails once (request 1 is served by P2), then P1 recovers. Returns
/// (P1 hits, P2 hits) after a second request on the same session and a third
/// on a new session.
async fn run_recovery_scenario(routing: Option<Value>) -> (usize, usize, Value, Value) {
    let p1 = MockUpstream::start(Reply::Json(
        500,
        json!({"type": "error", "error": {"type": "api_error", "message": "boom"}}),
    ))
    .await;
    let p2 = MockUpstream::start(Reply::Json(200, anthropic_message("from P2"))).await;
    let harness = start_proxy_with(
        &[
            claude_provider("p1", &p1.base_url(), None),
            claude_provider("p2", &p2.base_url(), None),
        ],
        // High threshold: P1's breaker stays closed, so it is selectable again.
        50,
        Overrides {
            routing,
            ..Default::default()
        },
    )
    .await;

    let first = harness
        .post("/v1/messages", &session_request("sess-a"))
        .await;
    assert_eq!(first.status(), 200);
    assert_eq!(p2.hits(), 1, "request 1 fails over to P2");

    p1.set_reply(Reply::Json(200, anthropic_message("from P1")));
    let p1_before = p1.hits();
    let second: Value = harness
        .post("/v1/messages", &session_request("sess-a"))
        .await
        .json()
        .await
        .expect("json");
    let third: Value = harness
        .post("/v1/messages", &session_request("sess-b"))
        .await
        .json()
        .await
        .expect("json");
    let result = (p1.hits() - p1_before, p2.hits(), second, third);
    harness.stop().await;
    result
}

/// PROXY-17: a passthrough body nobody changed reaches the upstream with the
/// client's exact bytes (key order and whitespace kept, no re-serialization).
#[tokio::test]
async fn passthrough_body_is_byte_identical_on_the_wire() {
    let _guard = serial().lock().await;
    let upstream = MockUpstream::start(Reply::Json(200, anthropic_message("ok"))).await;
    let harness = start_proxy(&[claude_provider("a", &upstream.base_url(), None)], 5).await;

    const CLIENT_BODY: &str = "{\"stream\": false, \"model\":\"claude-sonnet-4-5\",\n  \"max_tokens\":256,\"messages\":[{\"role\":\"user\",\"content\":\"Say hello\"}]}";
    let response = harness.post_raw("/v1/messages", CLIENT_BODY).await;
    assert_eq!(response.status(), 200);
    let recorded = upstream.last_request();
    assert_eq!(
        std::str::from_utf8(&recorded.raw).unwrap(),
        CLIENT_BODY,
        "unchanged passthrough body must not be re-serialized"
    );

    // A body the proxy does change (private `_` key stripped) is re-serialized.
    let response = harness
        .post_raw(
            "/v1/messages",
            "{\"model\":\"claude-sonnet-4-5\",\"max_tokens\":256,\"_private\":1,\"messages\":[{\"role\":\"user\",\"content\":\"Say hello\"}]}",
        )
        .await;
    assert_eq!(response.status(), 200);
    let recorded = upstream.last_request();
    assert!(recorded.body.get("_private").is_none());
    assert!(!std::str::from_utf8(&recorded.raw)
        .unwrap()
        .contains("_private"));
    harness.stop().await;
}

/// PERF-37: a compressed non-streaming passthrough response reaches the client
/// as the upstream sent it (no proxy-side decompression into a larger body).
#[tokio::test]
async fn compressed_non_stream_response_is_passed_through() {
    let _guard = serial().lock().await;
    let message = anthropic_message("zipped");
    let upstream = MockUpstream::start(Reply::GzipJson(message.clone())).await;
    let harness = start_proxy(&[claude_provider("a", &upstream.base_url(), None)], 5).await;

    let response = harness
        .client
        .post(harness.url("/v1/messages"))
        .header("content-type", "application/json")
        .header("anthropic-version", "2023-06-01")
        .header("x-api-key", "client-placeholder-key")
        .header("accept-encoding", "gzip")
        .json(&simple_request(false))
        .send()
        .await
        .expect("send");
    assert_eq!(response.status(), 200);
    assert_eq!(
        response
            .headers()
            .get("content-encoding")
            .and_then(|v| v.to_str().ok()),
        Some("gzip")
    );
    let raw = response.bytes().await.expect("body");
    assert_eq!(
        raw.as_ref(),
        gzip(message.to_string().as_bytes()).as_slice()
    );
    let mut plain = String::new();
    std::io::Read::read_to_string(&mut flate2::read::GzDecoder::new(raw.as_ref()), &mut plain)
        .unwrap();
    assert_eq!(serde_json::from_str::<Value>(&plain).unwrap(), message);
    harness.stop().await;
}

/// PROXY-19: without a routing config, a recovered P1 takes the session back.
#[tokio::test]
async fn default_routing_returns_to_recovered_priority_provider() {
    let _guard = serial().lock().await;
    let (p1_hits, p2_hits, second, third) = run_recovery_scenario(None).await;
    assert_eq!(second, anthropic_message("from P1"));
    assert_eq!(third, anthropic_message("from P1"));
    assert_eq!((p1_hits, p2_hits), (2, 1));
}

/// PROXY-19: with sticky sessions the session stays on P2 after P1 recovers;
/// a new session still starts on P1.
#[tokio::test]
async fn sticky_session_stays_on_failover_provider_after_recovery() {
    let _guard = serial().lock().await;
    let (p1_hits, p2_hits, second, third) = run_recovery_scenario(Some(json!({
        "stickySessions": true,
        "stickyTtlSecs": 600
    })))
    .await;
    assert_eq!(second, anthropic_message("from P2"), "sticky to P2");
    assert_eq!(third, anthropic_message("from P1"), "new session on P1");
    assert_eq!((p1_hits, p2_hits), (1, 2));
}

/// PROXY-19: round-robin spreads consecutive requests over both providers.
#[tokio::test]
async fn round_robin_alternates_providers() {
    let _guard = serial().lock().await;
    let a = MockUpstream::start(Reply::Json(200, anthropic_message("from A"))).await;
    let b = MockUpstream::start(Reply::Json(200, anthropic_message("from B"))).await;
    let harness = start_proxy_with(
        &[
            claude_provider("a", &a.base_url(), None),
            claude_provider("b", &b.base_url(), None),
        ],
        5,
        Overrides {
            routing: Some(json!({ "strategy": "roundRobin" })),
            ..Default::default()
        },
    )
    .await;
    for _ in 0..4 {
        let response = harness.post("/v1/messages", &simple_request(false)).await;
        assert_eq!(response.status(), 200);
    }
    assert_eq!((a.hits(), b.hits()), (2, 2));
    harness.stop().await;
}

/// A 4xx client error is not retried on another provider.
#[tokio::test]
async fn upstream_400_is_not_failed_over() {
    let _guard = serial().lock().await;
    let rejecting = MockUpstream::start(Reply::Json(
        400,
        json!({"type": "error", "error": {"type": "invalid_request_error", "message": "bad"}}),
    ))
    .await;
    let healthy = MockUpstream::start(Reply::Json(200, anthropic_message("from B"))).await;
    let harness = start_proxy(
        &[
            claude_provider("a", &rejecting.base_url(), None),
            claude_provider("b", &healthy.base_url(), None),
        ],
        5,
    )
    .await;

    let response = harness.post("/v1/messages", &simple_request(false)).await;
    assert_eq!(response.status(), 400);
    assert_eq!(healthy.hits(), 0, "a non-retryable 400 must not fail over");
    harness.stop().await;
}

/// Once the first provider's breaker opens, later requests skip it entirely; when every
/// breaker is open the proxy answers without contacting any upstream.
#[tokio::test]
async fn circuit_breaker_opens_and_skips_failing_provider() {
    let _guard = serial().lock().await;
    let error = json!({"type": "error", "error": {"type": "api_error", "message": "boom"}});
    let failing = MockUpstream::start(Reply::Json(500, error.clone())).await;
    let healthy = MockUpstream::start(Reply::Json(200, anthropic_message("from B"))).await;
    let harness = start_proxy(
        &[
            claude_provider("a", &failing.base_url(), None),
            claude_provider("b", &healthy.base_url(), None),
        ],
        1,
    )
    .await;

    // First request: A fails (opening its breaker), B answers.
    let first = harness.post("/v1/messages", &simple_request(false)).await;
    assert_eq!(first.status(), 200);
    let a_hits_after_first = failing.hits();
    assert!(a_hits_after_first >= 1);

    // Second request: A's breaker is open, so only B is contacted.
    let second = harness.post("/v1/messages", &simple_request(false)).await;
    assert_eq!(second.status(), 200);
    assert_eq!(
        failing.hits(),
        a_hits_after_first,
        "open breaker must keep traffic away from provider A"
    );
    assert_eq!(healthy.hits(), 2);

    // Now B fails too: its breaker opens on this request...
    healthy.set_reply(Reply::Json(500, error));
    let third = harness.post("/v1/messages", &simple_request(false)).await;
    assert!(third.status().is_server_error());
    let hits_before = (failing.hits(), healthy.hits());

    // ...and with every breaker open nothing reaches an upstream.
    let fourth = harness.post("/v1/messages", &simple_request(false)).await;
    assert!(
        fourth.status().is_server_error(),
        "all-open must fail, got {}",
        fourth.status()
    );
    assert_eq!((failing.hits(), healthy.hits()), hits_before);
    harness.stop().await;
}

/// The upstream drops the connection in the middle of an Anthropic stream: the client
/// receives the events sent so far and the stream then ends (no hang), and the proxy
/// keeps serving the next request.
#[tokio::test]
async fn mid_stream_upstream_disconnect_passthrough() {
    let _guard = serial().lock().await;
    let full = read_fixture("codex_anthropic/stream/text_utf8.input.sse");
    let partial: String = full.split_inclusive("\n\n").take(4).collect();
    let upstream = MockUpstream::start(Reply::SseThenDisconnect(partial.clone())).await;
    let harness = start_proxy(&[claude_provider("a", &upstream.base_url(), None)], 3).await;

    let response = harness.post("/v1/messages", &simple_request(true)).await;
    assert_eq!(response.status(), 200);
    let mut body = response.bytes_stream();
    let mut text = String::new();
    let outcome = tokio::time::timeout(Duration::from_secs(15), async {
        while let Some(chunk) = body.next().await {
            match chunk {
                Ok(bytes) => text.push_str(&String::from_utf8_lossy(&bytes)),
                Err(_) => break,
            }
        }
    })
    .await;
    assert!(
        outcome.is_ok(),
        "client stream hung after upstream disconnect"
    );
    let received = event_types(&sse_events(&text));
    assert!(
        received.starts_with(&[
            "message_start".to_string(),
            "content_block_start".to_string()
        ]),
        "partial events must reach the client: {received:?}"
    );
    assert!(
        !received.contains(&"message_stop".to_string()),
        "a cut stream must not be presented as complete: {received:?}"
    );

    upstream.set_reply(Reply::Json(200, anthropic_message("still alive")));
    let next = harness.post("/v1/messages", &simple_request(false)).await;
    assert_eq!(next.status(), 200);
    harness.stop().await;
}

/// Mid-stream disconnect on the Chat bridge: the converted Anthropic stream ends with
/// an `error` event instead of a fabricated `message_stop`.
#[tokio::test]
async fn mid_stream_upstream_disconnect_chat_bridge() {
    let _guard = serial().lock().await;
    let full = read_fixture("anthropic_chat/stream/text_utf8.input.sse");
    let partial: String = full.split_inclusive("\n\n").take(2).collect();
    let upstream = MockUpstream::start(Reply::SseThenDisconnect(partial)).await;
    let harness = start_proxy(
        &[claude_provider(
            "a",
            &upstream.base_url(),
            Some("openai_chat"),
        )],
        3,
    )
    .await;

    let response = harness.post("/v1/messages", &simple_request(true)).await;
    assert_eq!(response.status(), 200);
    let text = tokio::time::timeout(Duration::from_secs(15), response.text())
        .await
        .expect("client stream hung after upstream disconnect")
        .unwrap_or_default();
    let events = sse_events(&text);
    let types = event_types(&events);
    assert_eq!(types.first().map(String::as_str), Some("message_start"));
    assert_eq!(
        types.last().map(String::as_str),
        Some("error"),
        "expected a terminal error event: {types:?}"
    );
    assert!(!types.contains(&"message_stop".to_string()));
    harness.stop().await;
}

/// A 429 (rate limit) on the first provider is provider-specific: fail over.
#[tokio::test]
async fn upstream_429_fails_over_to_second_provider() {
    let _guard = serial().lock().await;
    let limited = MockUpstream::start(Reply::Json(
        429,
        json!({"type": "error", "error": {"type": "rate_limit_error", "message": "slow down"}}),
    ))
    .await;
    let healthy = MockUpstream::start(Reply::Json(200, anthropic_message("from B"))).await;
    let harness = start_proxy(
        &[
            claude_provider("a", &limited.base_url(), None),
            claude_provider("b", &healthy.base_url(), None),
        ],
        5,
    )
    .await;

    let response = harness.post("/v1/messages", &simple_request(false)).await;
    assert_eq!(response.status(), 200);
    let body: Value = response.json().await.expect("json response");
    assert_eq!(body, anthropic_message("from B"));
    assert_eq!(
        limited.hits(),
        1,
        "a 429 is not retried on the same provider"
    );
    assert_eq!(healthy.hits(), 1);
    harness.stop().await;
}

fn request_with_thinking_history() -> Value {
    json!({
        "model": "claude-sonnet-4-5",
        "max_tokens": 256,
        "stream": false,
        "messages": [
            {"role": "user", "content": "hi"},
            {"role": "assistant", "content": [
                {"type": "thinking", "thinking": "hmm", "signature": "sig-from-another-provider"},
                {"type": "text", "text": "hello"}
            ]},
            {"role": "user", "content": "again"}
        ]
    })
}

fn has_thinking_block(body: &Value) -> bool {
    body["messages"].as_array().is_some_and(|messages| {
        messages.iter().any(|m| {
            m["content"].as_array().is_some_and(|blocks| {
                blocks
                    .iter()
                    .any(|b| b["type"] == "thinking" || b.get("signature").is_some())
            })
        })
    })
}

fn signature_error() -> Reply {
    Reply::Json(
        400,
        json!({"type": "error", "error": {
            "type": "invalid_request_error",
            "message": "messages.1.content.0: Invalid `signature` in `thinking` block"
        }}),
    )
}

/// The thinking-signature rectifier rewrites the body and retries the same
/// provider once; the client sees the retry's success.
#[tokio::test]
async fn signature_rectifier_retries_the_same_provider() {
    let _guard = serial().lock().await;
    let upstream = MockUpstream::start(Reply::Sequence(vec![
        signature_error(),
        Reply::Json(200, anthropic_message("rectified")),
    ]))
    .await;
    let harness = start_proxy(&[claude_provider("a", &upstream.base_url(), None)], 3).await;

    let response = harness
        .post("/v1/messages", &request_with_thinking_history())
        .await;
    assert_eq!(response.status(), 200);
    let body: Value = response.json().await.expect("json response");
    assert_eq!(body, anthropic_message("rectified"));
    assert_eq!(upstream.hits(), 2, "original + one rectified retry");
    let requests = upstream.state.requests.lock().unwrap().clone();
    assert!(has_thinking_block(&requests[0].body));
    assert!(!has_thinking_block(&requests[1].body));
    harness.stop().await;
}

/// PROXY-42 + PROXY-24: after the rectifier fired, a 429 on the retry still fails
/// over, and the next provider receives the rectified body on its first call.
#[tokio::test]
async fn rectifier_retry_rate_limit_fails_over_with_the_rectified_body() {
    let _guard = serial().lock().await;
    let first = MockUpstream::start(Reply::Sequence(vec![
        signature_error(),
        Reply::Json(
            429,
            json!({"type": "error", "error": {"type": "rate_limit_error", "message": "slow down"}}),
        ),
    ]))
    .await;
    let second = MockUpstream::start(Reply::Json(200, anthropic_message("from B"))).await;
    let harness = start_proxy(
        &[
            claude_provider("a", &first.base_url(), None),
            claude_provider("b", &second.base_url(), None),
        ],
        5,
    )
    .await;

    let response = harness
        .post("/v1/messages", &request_with_thinking_history())
        .await;
    assert_eq!(response.status(), 200);
    assert_eq!(first.hits(), 2);
    assert_eq!(second.hits(), 1, "B must succeed on its first call");
    assert!(!has_thinking_block(&second.last_request().body));
    harness.stop().await;
}

/// A stream that sends `message_start` and then goes silent is ended by the idle
/// timeout with a protocol error event instead of hanging the client.
#[tokio::test]
async fn stream_idle_timeout_ends_the_stream_with_an_error_event() {
    let _guard = serial().lock().await;
    let upstream = MockUpstream::start(Reply::SseThenHang(
        "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"model\":\"claude-sonnet-4-5\",\"content\":[],\"usage\":{\"input_tokens\":1,\"output_tokens\":0}}}\n\n"
            .to_string(),
    ))
    .await;
    let harness = start_proxy_with(
        &[claude_provider("a", &upstream.base_url(), None)],
        3,
        Overrides {
            failover: true,
            streaming_first_byte_timeout: Some(5),
            streaming_idle_timeout: Some(1),
            ..Default::default()
        },
    )
    .await;

    let started = std::time::Instant::now();
    let response = harness.post("/v1/messages", &simple_request(true)).await;
    assert_eq!(response.status(), 200);
    let text = tokio::time::timeout(Duration::from_secs(15), response.text())
        .await
        .expect("client stream hung despite the idle timeout")
        .unwrap_or_default();
    assert!(started.elapsed() < Duration::from_secs(10));
    let types = event_types(&sse_events(&text));
    assert_eq!(types.first().map(String::as_str), Some("message_start"));
    assert_eq!(
        types.last().map(String::as_str),
        Some("error"),
        "expected a terminal error event: {types:?}"
    );
    harness.stop().await;
}
