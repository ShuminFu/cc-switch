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

use crate::database::Database;
use crate::provider::Provider;
use crate::proxy::server::ProxyServer;
use crate::proxy::types::{AppProxyConfig, ProxyConfig};

/// App type every harness provider is registered under.
pub const HARNESS_APP_TYPE: &str = "claude";

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

    match (state.responder)(index, &recorded) {
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
    }
}

// ============================================================================
// Database statement counter
// ============================================================================

thread_local! {
    static DB_STATEMENTS: Cell<u64> = const { Cell::new(0) };
    static DB_COUNTER_PAUSED: Cell<bool> = const { Cell::new(false) };
}

fn count_statement(sql: &str) {
    // SQLite also traces trigger programs as `-- TRIGGER name`; only count statements.
    if DB_COUNTER_PAUSED.with(Cell::get) || sql.trim_start().starts_with("--") {
        return;
    }
    DB_STATEMENTS.with(|count| count.set(count.get() + 1));
}

/// SQL statements executed on this thread by harness databases, excluding
/// the harness's own inspection queries.
pub fn db_statement_count() -> u64 {
    DB_STATEMENTS.with(Cell::get)
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
    providers: Vec<(String, String)>,
    auto_failover: bool,
    max_retries: u32,
    circuit: HarnessCircuit,
}

impl ProxyHarnessBuilder {
    /// Add a Claude provider pointing at `upstream`.
    pub fn provider(mut self, id: &str, upstream: &MockUpstream) -> Self {
        self.providers.push((id.to_string(), upstream.base_url()));
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
        assert!(!self.providers.is_empty(), "harness needs a provider");
        // Same process-level TLS provider the app installs at startup (lib.rs).
        let _ = rustls::crypto::ring::default_provider().install_default();
        let db = Arc::new(Database::memory().expect("in-memory database"));
        install_statement_counter(&db);

        for (index, (id, base_url)) in self.providers.iter().enumerate() {
            let mut provider = Provider::with_id(
                id.clone(),
                format!("Harness {id}"),
                json!({
                    "env": {
                        "ANTHROPIC_BASE_URL": base_url,
                        "ANTHROPIC_AUTH_TOKEN": format!("sk-harness-{id}"),
                    }
                }),
                None,
            );
            provider.sort_index = Some(index);
            db.save_provider(HARNESS_APP_TYPE, &provider)
                .expect("save harness provider");
            if self.auto_failover {
                db.add_to_failover_queue(HARNESS_APP_TYPE, id)
                    .expect("queue harness provider");
            }
        }
        db.set_current_provider(HARNESS_APP_TYPE, &self.providers[0].0)
            .expect("set current harness provider");

        // Reading the row first creates the per-app defaults.
        let defaults = db
            .get_proxy_config_for_app(HARNESS_APP_TYPE)
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

        let config = ProxyConfig {
            listen_address: "127.0.0.1".to_string(),
            listen_port: 0,
            enable_logging: true,
            ..ProxyConfig::default()
        };
        let server = ProxyServer::new(config, db.clone(), None);
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
            base_url: format!("http://127.0.0.1:{}", info.port),
            port: info.port,
            token,
            client,
        }
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

    pub async fn shutdown(self) {
        let _ = self.server.stop().await;
    }
}
