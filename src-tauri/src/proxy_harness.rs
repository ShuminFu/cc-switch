//! End-to-end proxy harness, used by `tests/proxy_e2e.rs`.
//!
//! [`MockUpstream`] is a scripted axum server on `127.0.0.1:0` that records
//! every request it receives. [`ProxyHarness`] starts the real proxy server
//! (router, forwarder, circuit breakers, usage logger) on another ephemeral
//! port, backed by an in-memory database whose providers point at mock
//! upstreams. Nothing here adds behaviour to the proxy: requests go through
//! the same `ProxyServer` the app runs.
//!
//! The database statement counter and the scoped settings/paths used by the
//! proxy are thread-local, so tests must drive the harness from a
//! current-thread runtime (the `#[tokio::test]` default).

use std::cell::Cell;
use std::collections::VecDeque;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::Response;
use axum::Router;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

use crate::app_config::AppType;
use crate::database::Database;
use crate::provider::Provider;
use crate::proxy::capture::CaptureStore;
use crate::proxy::server::ProxyServer;
use crate::proxy::trace::TraceStore;
use crate::proxy::types::{AppProxyConfig, ProxyConfig};
pub use crate::proxy::{CircuitBreakerStats, CircuitProbeResult, CircuitState};
use crate::services::stream_check::{StreamCheckConfig, StreamCheckService};

/// App type every harness provider is registered under (except the Gemini
/// providers added with [`ProxyHarnessBuilder::gemini_provider_with_meta`]).
pub const HARNESS_APP_TYPE: &str = "claude";

/// App type of the Gemini harness providers.
pub const HARNESS_GEMINI_APP_TYPE: &str = "gemini";

// ============================================================================
// Mock upstream
// ============================================================================

/// One request as the mock upstream received it.
#[derive(Debug, Clone)]
pub struct RecordedRequest {
    pub method: Method,
    /// Path plus query, e.g. `/v1/messages?beta=true`.
    pub path: String,
    pub headers: HeaderMap,
    pub body: Vec<u8>,
}

impl RecordedRequest {
    /// Request body parsed as JSON (`Value::Null` when it is not JSON).
    pub fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or(Value::Null)
    }
}

/// A scripted upstream reply.
#[derive(Debug, Clone)]
pub enum MockReply {
    /// A JSON body with the given status.
    Json { status: u16, body: Value },
    /// A `text/event-stream` body sent as one chunk per entry.
    Sse { status: u16, chunks: Vec<String> },
    /// `reply`, sent only after `delay` (simulates a hanging upstream).
    Delayed {
        delay: Duration,
        reply: Box<MockReply>,
    },
    /// `reply` with an extra response header.
    WithHeader {
        name: &'static str,
        value: String,
        reply: Box<MockReply>,
    },
}

impl MockReply {
    pub fn json(status: u16, body: Value) -> Self {
        Self::Json { status, body }
    }

    /// An Anthropic error envelope, as upstreams return it for 4xx/5xx.
    pub fn error(status: u16, message: &str) -> Self {
        Self::json(
            status,
            json!({
                "type": "error",
                "error": { "type": "invalid_request_error", "message": message }
            }),
        )
    }

    /// A successful non-streaming Anthropic Messages response.
    pub fn message(text: &str, input_tokens: u64, output_tokens: u64) -> Self {
        Self::json(
            200,
            json!({
                "id": "msg_harness",
                "type": "message",
                "role": "assistant",
                "model": "claude-sonnet-4-5",
                "content": [{ "type": "text", "text": text }],
                "stop_reason": "end_turn",
                "stop_sequence": null,
                "usage": { "input_tokens": input_tokens, "output_tokens": output_tokens }
            }),
        )
    }

    pub fn sse(chunks: Vec<String>) -> Self {
        Self::Sse {
            status: 200,
            chunks,
        }
    }

    /// This reply, sent only after `delay`.
    pub fn delayed(self, delay: Duration) -> Self {
        Self::Delayed {
            delay,
            reply: Box::new(self),
        }
    }

    /// This reply with an extra response header (e.g. `retry-after`).
    pub fn with_header(self, name: &'static str, value: impl Into<String>) -> Self {
        Self::WithHeader {
            name,
            value: value.into(),
            reply: Box::new(self),
        }
    }
}

type Responder = dyn Fn(usize, &RecordedRequest) -> MockReply + Send + Sync;

#[derive(Clone)]
struct MockState {
    requests: Arc<Mutex<Vec<RecordedRequest>>>,
    responder: Arc<Responder>,
}

/// Scripted HTTP upstream on `127.0.0.1:0`.
///
/// The responder receives the zero-based index of the request and the
/// request itself. The server stops when the value is dropped.
pub struct MockUpstream {
    addr: SocketAddr,
    requests: Arc<Mutex<Vec<RecordedRequest>>>,
    shutdown: Option<oneshot::Sender<()>>,
    handle: Option<JoinHandle<()>>,
}

impl MockUpstream {
    /// Start a mock whose replies come from `responder`.
    pub async fn spawn<F>(responder: F) -> Self
    where
        F: Fn(usize, &RecordedRequest) -> MockReply + Send + Sync + 'static,
    {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let state = MockState {
            requests: requests.clone(),
            responder: Arc::new(responder),
        };
        let app = Router::new().fallback(mock_handler).with_state(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock upstream");
        let addr = listener.local_addr().expect("mock upstream address");
        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
        let handle = tokio::spawn(async move {
            let _ = axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await;
        });
        Self {
            addr,
            requests,
            shutdown: Some(shutdown_tx),
            handle: Some(handle),
        }
    }

    /// Always answer with the same reply.
    pub async fn always(reply: MockReply) -> Self {
        Self::spawn(move |_, _| reply.clone()).await
    }

    /// Answer every request with a successful message whose `id` differs per
    /// request, so usage logging (deduplicated by message id) keeps every row.
    pub async fn messages(text: &str, input_tokens: u64, output_tokens: u64) -> Self {
        let text = text.to_string();
        Self::spawn(move |index, _| {
            let mut reply = MockReply::message(&text, input_tokens, output_tokens);
            if let MockReply::Json { body, .. } = &mut reply {
                body["id"] = json!(format!("msg_harness_{index}"));
            }
            reply
        })
        .await
    }

    /// Answer with `replies` in order, repeating the last one afterwards.
    pub async fn sequence(replies: Vec<MockReply>) -> Self {
        assert!(!replies.is_empty(), "sequence needs at least one reply");
        let replies: VecDeque<MockReply> = replies.into();
        Self::spawn(move |index, _| {
            replies
                .get(index)
                .or_else(|| replies.back())
                .cloned()
                .expect("non-empty replies")
        })
        .await
    }

    /// Base URL to put in a provider's `ANTHROPIC_BASE_URL`.
    pub fn base_url(&self) -> String {
        format!("http://{}", self.addr)
    }

    /// Every request received so far, in arrival order.
    pub fn requests(&self) -> Vec<RecordedRequest> {
        self.requests.lock().expect("mock requests lock").clone()
    }

    pub fn hits(&self) -> usize {
        self.requests.lock().expect("mock requests lock").len()
    }
}

impl Drop for MockUpstream {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        if let Some(handle) = self.handle.take() {
            handle.abort();
        }
    }
}

async fn mock_handler(State(state): State<MockState>, request: Request) -> Response {
    let (parts, body) = request.into_parts();
    let body = body
        .collect()
        .await
        .map(|collected| collected.to_bytes().to_vec())
        .unwrap_or_default();
    let recorded = RecordedRequest {
        method: parts.method,
        path: parts
            .uri
            .path_and_query()
            .map(|pq| pq.as_str().to_string())
            .unwrap_or_else(|| parts.uri.path().to_string()),
        headers: parts.headers,
        body,
    };
    let index = {
        let mut requests = state.requests.lock().expect("mock requests lock");
        requests.push(recorded.clone());
        requests.len() - 1
    };

    let mut reply = (state.responder)(index, &recorded);
    let mut extra_headers = Vec::new();
    loop {
        match reply {
            MockReply::Delayed {
                delay,
                reply: inner,
            } => {
                tokio::time::sleep(delay).await;
                reply = *inner;
            }
            MockReply::WithHeader {
                name,
                value,
                reply: inner,
            } => {
                extra_headers.push((name, value));
                reply = *inner;
            }
            _ => break,
        }
    }

    let mut response = match reply {
        MockReply::Delayed { .. } | MockReply::WithHeader { .. } => {
            unreachable!("wrappers are unwrapped above")
        }
        MockReply::Json { status, body } => Response::builder()
            .status(StatusCode::from_u16(status).expect("valid mock status"))
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .expect("mock json response"),
        MockReply::Sse { status, chunks } => {
            let stream = futures::stream::iter(
                chunks
                    .into_iter()
                    .map(|chunk| Ok::<_, Infallible>(bytes::Bytes::from(chunk))),
            );
            Response::builder()
                .status(StatusCode::from_u16(status).expect("valid mock status"))
                .header("content-type", "text/event-stream")
                .header("cache-control", "no-cache")
                .body(Body::from_stream(stream))
                .expect("mock sse response")
        }
    };
    for (name, value) in extra_headers {
        response.headers_mut().insert(
            name,
            axum::http::HeaderValue::from_str(&value).expect("valid mock header value"),
        );
    }
    response
}

// ============================================================================
// Raw-recording mock upstream
// ============================================================================

/// Mock upstream that records each request head exactly as it arrived on the
/// wire (header-name casing and order included), which the axum-based
/// [`MockUpstream`] cannot see. Every request on every connection, keep-alive
/// included, gets a successful Anthropic message reply.
pub struct RawMockUpstream {
    addr: SocketAddr,
    heads: Arc<Mutex<Vec<String>>>,
    handle: JoinHandle<()>,
}

impl RawMockUpstream {
    pub async fn spawn() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind raw mock upstream");
        let addr = listener.local_addr().expect("raw mock upstream address");
        let heads = Arc::new(Mutex::new(Vec::new()));
        let recorded = heads.clone();
        let handle = tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                tokio::spawn(raw_mock_connection(stream, recorded.clone()));
            }
        });
        Self {
            addr,
            heads,
            handle,
        }
    }

    pub fn base_url(&self) -> String {
        format!("http://{}", self.addr)
    }

    /// Raw request heads received so far (request line and header lines,
    /// without the final empty line), in arrival order.
    pub fn heads(&self) -> Vec<String> {
        self.heads.lock().expect("raw mock heads lock").clone()
    }

    /// Header names of request `index` with their wire casing, in order.
    pub fn header_names(&self, index: usize) -> Vec<String> {
        self.heads()[index]
            .lines()
            .skip(1)
            .filter_map(|line| line.split_once(':').map(|(name, _)| name.to_string()))
            .collect()
    }
}

impl Drop for RawMockUpstream {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

async fn raw_mock_connection(mut stream: tokio::net::TcpStream, heads: Arc<Mutex<Vec<String>>>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let mut buf = Vec::new();
    loop {
        let head_end = loop {
            if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                break pos;
            }
            let mut chunk = [0u8; 8192];
            match stream.read(&mut chunk).await {
                Ok(0) | Err(_) => return,
                Ok(n) => buf.extend_from_slice(&chunk[..n]),
            }
        };
        let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
        let content_length = head
            .lines()
            .filter_map(|line| line.split_once(':'))
            .find(|(name, _)| name.trim().eq_ignore_ascii_case("content-length"))
            .and_then(|(_, value)| value.trim().parse::<usize>().ok())
            .unwrap_or(0);
        let request_end = head_end + 4 + content_length;
        while buf.len() < request_end {
            let mut chunk = [0u8; 8192];
            match stream.read(&mut chunk).await {
                Ok(0) | Err(_) => return,
                Ok(n) => buf.extend_from_slice(&chunk[..n]),
            }
        }
        buf.drain(..request_end);

        let index = {
            let mut heads = heads.lock().expect("raw mock heads lock");
            heads.push(head);
            heads.len() - 1
        };
        let mut reply = MockReply::message("raw", 1, 1);
        let MockReply::Json { body, .. } = &mut reply else {
            unreachable!("message replies are JSON")
        };
        body["id"] = json!(format!("msg_raw_{index}"));
        let body = body.to_string();
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        if stream.write_all(response.as_bytes()).await.is_err() {
            return;
        }
    }
}

// ============================================================================
// Database statement counter
// ============================================================================

thread_local! {
    static DB_STATEMENTS: Cell<u64> = const { Cell::new(0) };
    static DB_READ_STATEMENTS: Cell<u64> = const { Cell::new(0) };
    static DB_COUNTER_PAUSED: Cell<bool> = const { Cell::new(false) };
}

fn is_read_statement(sql: &str) -> bool {
    let keyword = sql.split_whitespace().next().unwrap_or_default();
    keyword.eq_ignore_ascii_case("SELECT") || keyword.eq_ignore_ascii_case("WITH")
}

fn count_statement(sql: &str) {
    // SQLite also traces trigger programs as `-- TRIGGER name`; only count statements.
    let sql = sql.trim_start();
    if DB_COUNTER_PAUSED.with(Cell::get) || sql.starts_with("--") {
        return;
    }
    DB_STATEMENTS.with(|count| count.set(count.get() + 1));
    if is_read_statement(sql) {
        DB_READ_STATEMENTS.with(|count| count.set(count.get() + 1));
    }
}

/// SQL statements executed on this thread by harness databases, excluding
/// the harness's own inspection queries.
pub fn db_statement_count() -> u64 {
    DB_STATEMENTS.with(Cell::get)
}

/// The read (`SELECT` / `WITH`) subset of [`db_statement_count`].
pub fn db_read_statement_count() -> u64 {
    DB_READ_STATEMENTS.with(Cell::get)
}

/// Run a harness inspection query without counting its statements.
fn uncounted<T>(f: impl FnOnce() -> T) -> T {
    let previous = DB_COUNTER_PAUSED.with(|paused| paused.replace(true));
    let result = f();
    DB_COUNTER_PAUSED.with(|paused| paused.set(previous));
    result
}

// ============================================================================
// Proxy harness
// ============================================================================

/// Circuit breaker and retry settings written to `proxy_config` for the app.
#[derive(Debug, Clone, Copy)]
pub struct HarnessCircuit {
    pub failure_threshold: u32,
    pub success_threshold: u32,
    pub timeout_seconds: u32,
}

impl Default for HarnessCircuit {
    fn default() -> Self {
        Self {
            failure_threshold: 4,
            success_threshold: 1,
            timeout_seconds: 60,
        }
    }
}

/// Builder for [`ProxyHarness`]; providers are tried in the order added.
pub struct ProxyHarnessBuilder {
    /// `(id, base_url, meta)`; `meta` is the provider's JSON `meta` object.
    providers: Vec<(String, String, Option<Value>)>,
    /// Gemini providers, same shape as `providers`.
    gemini_providers: Vec<(String, String, Option<Value>)>,
    auto_failover: bool,
    max_retries: u32,
    circuit: HarnessCircuit,
}

impl ProxyHarnessBuilder {
    /// Add a Claude provider pointing at `upstream`.
    pub fn provider(mut self, id: &str, upstream: &MockUpstream) -> Self {
        self.providers
            .push((id.to_string(), upstream.base_url(), None));
        self
    }

    /// Add a Claude provider pointing at `upstream` with the given JSON `meta`
    /// (e.g. `{"apiFormat": "openai_responses", "providerType": "github_copilot"}`).
    pub fn provider_with_meta(mut self, id: &str, upstream: &MockUpstream, meta: Value) -> Self {
        self.providers
            .push((id.to_string(), upstream.base_url(), Some(meta)));
        self
    }

    /// Add a Gemini provider pointing at `upstream` with the given JSON `meta`
    /// (e.g. `{"apiFormat": "anthropic"}` for an Anthropic upstream). Its key is
    /// `sk-harness-<id>`, sent as configured for the upstream format.
    pub fn gemini_provider_with_meta(
        mut self,
        id: &str,
        upstream: &MockUpstream,
        meta: Value,
    ) -> Self {
        self.gemini_providers
            .push((id.to_string(), upstream.base_url(), Some(meta)));
        self
    }

    /// Add a Claude provider whose base URL is `base_url` (e.g. a
    /// [`RawMockUpstream`]).
    pub fn provider_url(mut self, id: &str, base_url: String) -> Self {
        self.providers.push((id.to_string(), base_url, None));
        self
    }

    /// Enable auto failover across the providers (queue order = add order).
    pub fn failover(mut self, enabled: bool) -> Self {
        self.auto_failover = enabled;
        self
    }

    pub fn max_retries(mut self, max_retries: u32) -> Self {
        self.max_retries = max_retries;
        self
    }

    pub fn circuit(mut self, circuit: HarnessCircuit) -> Self {
        self.circuit = circuit;
        self
    }

    pub async fn spawn(self) -> ProxyHarness {
        assert!(
            !self.providers.is_empty() || !self.gemini_providers.is_empty(),
            "harness needs a provider"
        );
        // Same process-level TLS provider the app installs at startup (lib.rs).
        let _ = rustls::crypto::ring::default_provider().install_default();
        let db = Arc::new(Database::memory().expect("in-memory database"));
        install_statement_counter(&db);

        for (app_type, providers) in [
            (HARNESS_APP_TYPE, &self.providers),
            (HARNESS_GEMINI_APP_TYPE, &self.gemini_providers),
        ] {
            if !providers.is_empty() {
                self.register_providers(&db, app_type, providers).await;
            }
        }

        let config = ProxyConfig {
            listen_address: "127.0.0.1".to_string(),
            listen_port: 0,
            enable_logging: true,
            ..ProxyConfig::default()
        };
        let mut server = ProxyServer::new(config, db.clone(), None);
        // Private capture store: arming capture in one scenario must not record
        // the requests of concurrent scenarios.
        let capture = Arc::new(CaptureStore::default());
        server.set_capture_store(capture.clone());
        let traces = Arc::new(TraceStore::default());
        server.set_trace_store(traces.clone());
        let info = server.start().await.expect("start proxy server");
        let token = crate::proxy::inbound_auth::get_or_create_inbound_token(&db)
            .expect("read proxy inbound token");
        let client = reqwest::Client::builder()
            .no_proxy()
            .build()
            .expect("harness http client");

        ProxyHarness {
            server,
            db,
            capture,
            traces,
            base_url: format!("http://127.0.0.1:{}", info.port),
            port: info.port,
            token,
            client,
        }
    }

    /// Save `providers` under `app_type` (queue order = add order), make the
    /// first one current and enable the proxy for the app.
    async fn register_providers(
        &self,
        db: &Database,
        app_type: &str,
        providers: &[(String, String, Option<Value>)],
    ) {
        for (index, (id, base_url, meta)) in providers.iter().enumerate() {
            let settings = if app_type == HARNESS_GEMINI_APP_TYPE {
                json!({
                    "env": {
                        "GOOGLE_GEMINI_BASE_URL": base_url,
                        "GEMINI_API_KEY": format!("sk-harness-{id}"),
                    }
                })
            } else {
                json!({
                    "env": {
                        "ANTHROPIC_BASE_URL": base_url,
                        "ANTHROPIC_AUTH_TOKEN": format!("sk-harness-{id}"),
                    }
                })
            };
            let mut provider =
                Provider::with_id(id.clone(), format!("Harness {id}"), settings, None);
            provider.sort_index = Some(index);
            if let Some(meta) = meta {
                provider.meta = Some(
                    serde_json::from_value(meta.clone()).expect("valid harness provider meta"),
                );
            }
            db.save_provider(app_type, &provider)
                .expect("save harness provider");
            if self.auto_failover {
                db.add_to_failover_queue(app_type, id)
                    .expect("queue harness provider");
            }
        }
        db.set_current_provider(app_type, &providers[0].0)
            .expect("set current harness provider");

        // Reading the row first creates the per-app defaults.
        let defaults = db
            .get_proxy_config_for_app(app_type)
            .await
            .expect("read app proxy config");
        db.update_proxy_config_for_app(AppProxyConfig {
            enabled: true,
            auto_failover_enabled: self.auto_failover,
            max_retries: self.max_retries,
            // Keep scenario failures fast and deterministic.
            non_streaming_timeout: 30,
            streaming_first_byte_timeout: 30,
            streaming_idle_timeout: 30,
            circuit_failure_threshold: self.circuit.failure_threshold,
            circuit_success_threshold: self.circuit.success_threshold,
            circuit_timeout_seconds: self.circuit.timeout_seconds,
            // Only consecutive failures open the breaker in harness scenarios.
            circuit_error_rate_threshold: 1.0,
            circuit_min_requests: u32::MAX / 2,
            ..defaults
        })
        .await
        .expect("write app proxy config");
    }
}

fn install_statement_counter(db: &Database) {
    db.conn
        .lock()
        .expect("db lock")
        .trace(Some(count_statement));
}

/// One `proxy_request_logs` row.
#[derive(Debug, Clone)]
pub struct RequestLogRow {
    pub request_id: String,
    pub provider_id: String,
    pub app_type: String,
    pub model: String,
    pub request_model: Option<String>,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub total_cost_usd: String,
    pub status_code: i64,
    pub error_message: Option<String>,
    pub session_id: Option<String>,
    pub is_streaming: bool,
}

/// Request counters from the proxy status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HarnessStatus {
    pub total_requests: u64,
    pub success_requests: u64,
    pub failed_requests: u64,
    pub failover_count: u64,
}

/// Health row the router keeps per provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HarnessHealth {
    pub is_healthy: bool,
    pub consecutive_failures: u32,
}

/// The real proxy server on `127.0.0.1:0`, wired to mock upstreams.
pub struct ProxyHarness {
    server: ProxyServer,
    db: Arc<Database>,
    capture: Arc<CaptureStore>,
    traces: Arc<TraceStore>,
    base_url: String,
    port: u16,
    token: String,
    client: reqwest::Client,
}

impl ProxyHarness {
    /// A single-provider proxy (failover off) in front of `upstream`.
    pub async fn spawn(upstream: &MockUpstream) -> Self {
        Self::builder().provider("p1", upstream).spawn().await
    }

    pub fn builder() -> ProxyHarnessBuilder {
        ProxyHarnessBuilder {
            providers: Vec::new(),
            gemini_providers: Vec::new(),
            auto_failover: false,
            max_retries: 3,
            circuit: HarnessCircuit::default(),
        }
    }

    pub fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base_url)
    }

    pub fn db(&self) -> &Database {
        &self.db
    }

    /// Bug report capture store of this proxy (disarmed until a test arms it).
    pub fn capture(&self) -> &CaptureStore {
        &self.capture
    }

    /// Request trace ring buffer of this proxy.
    pub fn traces(&self) -> &TraceStore {
        &self.traces
    }

    /// Port the proxy listens on.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// The per-install inbound token the proxy expects from clients.
    pub fn token(&self) -> &str {
        &self.token
    }

    /// The proxy's full router (inbound auth included), for in-process
    /// `tower::ServiceExt::oneshot` requests against the same state.
    pub fn router(&self) -> Router {
        self.server
            .router_for_port(self.port)
            .expect("build proxy router")
    }

    /// POST a Claude Messages request to the proxy.
    pub async fn messages(&self, body: &Value) -> reqwest::Response {
        self.client
            .post(self.url("/v1/messages"))
            .header("content-type", "application/json")
            .header("anthropic-version", "2023-06-01")
            .header("x-api-key", &self.token)
            .json(body)
            .send()
            .await
            .expect("send request to proxy")
    }

    /// POST a JSON body to `path` as a Gemini client does (`x-goog-api-key`
    /// carries the proxy token).
    pub async fn gemini(&self, path: &str, body: &Value) -> reqwest::Response {
        self.client
            .post(self.url(path))
            .header("content-type", "application/json")
            .header("x-goog-api-key", &self.token)
            .header("x-goog-api-client", "google-genai-sdk/1.0.0 gl-node/v22")
            .json(body)
            .send()
            .await
            .expect("send request to proxy")
    }

    /// All `proxy_request_logs` rows, oldest first.
    pub fn request_logs(&self) -> Vec<RequestLogRow> {
        uncounted(|| self.query_request_logs())
    }

    fn query_request_logs(&self) -> Vec<RequestLogRow> {
        let conn = self.db.conn.lock().expect("db lock");
        let mut stmt = conn
            .prepare(
                "SELECT request_id, provider_id, app_type, model, request_model,
                        input_tokens, output_tokens, total_cost_usd, status_code,
                        error_message, session_id, is_streaming
                 FROM proxy_request_logs ORDER BY created_at, rowid",
            )
            .expect("prepare request log query");
        let rows = stmt
            .query_map([], |row| {
                Ok(RequestLogRow {
                    request_id: row.get(0)?,
                    provider_id: row.get(1)?,
                    app_type: row.get(2)?,
                    model: row.get(3)?,
                    request_model: row.get(4)?,
                    input_tokens: row.get(5)?,
                    output_tokens: row.get(6)?,
                    total_cost_usd: row.get(7)?,
                    status_code: row.get(8)?,
                    error_message: row.get(9)?,
                    session_id: row.get(10)?,
                    is_streaming: row.get::<_, i64>(11)? != 0,
                })
            })
            .expect("query request logs");
        rows.collect::<Result<Vec<_>, _>>()
            .expect("read request log rows")
    }

    /// Wait until at least `count` log rows exist; usage logs are written
    /// by a task spawned after the response is returned.
    pub async fn wait_for_request_logs(&self, count: usize) -> Vec<RequestLogRow> {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        loop {
            let rows = self.request_logs();
            if rows.len() >= count || tokio::time::Instant::now() >= deadline {
                return rows;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    pub async fn health(&self, provider_id: &str) -> HarnessHealth {
        // get_provider_health has no await points, so no other task runs while paused.
        let previous = DB_COUNTER_PAUSED.with(|paused| paused.replace(true));
        let health = self
            .db
            .get_provider_health(provider_id, HARNESS_APP_TYPE)
            .await;
        DB_COUNTER_PAUSED.with(|paused| paused.set(previous));
        let health = health.expect("read provider health");
        HarnessHealth {
            is_healthy: health.is_healthy,
            consecutive_failures: health.consecutive_failures,
        }
    }

    /// Wait until the provider's health row matches `expected`; closed-breaker
    /// successes are recorded asynchronously.
    pub async fn wait_for_health(
        &self,
        provider_id: &str,
        expected: &HarnessHealth,
    ) -> HarnessHealth {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        loop {
            let health = self.health(provider_id).await;
            if &health == expected || tokio::time::Instant::now() >= deadline {
                return health;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    pub async fn status(&self) -> HarnessStatus {
        let status = self.server.get_status().await;
        HarnessStatus {
            total_requests: status.total_requests,
            success_requests: status.success_requests,
            failed_requests: status.failed_requests,
            failover_count: status.failover_count,
        }
    }

    /// In-memory breaker state of a provider (`None` before its first request).
    pub async fn circuit_stats(&self, provider_id: &str) -> Option<CircuitBreakerStats> {
        self.server
            .provider_router()
            .get_circuit_breaker_stats(provider_id, HARNESS_APP_TYPE)
            .await
    }

    /// "Probe now": the same minimal request the `probe_circuit_breaker`
    /// command sends, recorded through the breaker's half-open path.
    pub async fn probe_now(&self, provider_id: &str) -> CircuitProbeResult {
        let provider = self
            .db
            .get_all_providers(HARNESS_APP_TYPE)
            .expect("read harness providers")
            .get(provider_id)
            .cloned()
            .expect("harness provider exists");
        let config = StreamCheckConfig::default();
        self.server
            .provider_router()
            .probe_provider(provider_id, HARNESS_APP_TYPE, || {
                StreamCheckService::probe_circuit_target(&AppType::Claude, &provider, &config, None)
            })
            .await
            .expect("probe provider")
    }

    pub async fn shutdown(self) {
        let _ = self.server.stop().await;
    }
}
