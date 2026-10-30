//! Hyper-based HTTP client for proxy forwarding
//!
//! Uses raw TCP/TLS writes to preserve exact original header name casing and
//! order, on keep-alive connections pooled per (scheme, host, port, proxy).
//! Supports HTTP CONNECT tunneling through upstream proxies.
//! Falls back to hyper-util Client (title-case headers) when raw write is not feasible.

use super::ProxyError;
use bytes::Bytes;
use futures::{stream::Stream, StreamExt};
use http_body_util::BodyExt;
use hyper_rustls::HttpsConnectorBuilder;
use hyper_util::{client::legacy::Client, rt::TokioExecutor};
use std::sync::OnceLock;

/// Our own header case map: maps lowercase header name → original wire-casing bytes.
///
/// This is a backup mechanism independent of hyper's internal `HeaderCaseMap` (which is
/// `pub(crate)` and cannot be directly inspected or constructed from outside hyper).
///
/// Populated in `server.rs` by peeking at raw TCP bytes before hyper parses them.
/// Used in `send_request` to manually write headers with original casing when hyper's
/// own mechanism fails.
#[derive(Clone, Debug, Default)]
pub(crate) struct OriginalHeaderCases {
    /// Ordered list of (lowercase_name, original_wire_bytes) pairs.
    /// Multiple entries with the same name are allowed (for repeated headers).
    pub cases: Vec<(String, Vec<u8>)>,
}

impl OriginalHeaderCases {
    /// Parse raw HTTP request bytes (from TcpStream::peek) to extract original header casings.
    pub fn from_raw_bytes(buf: &[u8]) -> Self {
        let mut headers_buf = [httparse::EMPTY_HEADER; 128];
        let mut req = httparse::Request::new(&mut headers_buf);
        // We don't care if parsing is partial — we just want the header names we can get
        let _ = req.parse(buf);

        let mut cases = Vec::new();
        for header in req.headers.iter() {
            if header.name.is_empty() {
                break;
            }
            cases.push((
                header.name.to_ascii_lowercase(),
                header.name.as_bytes().to_vec(),
            ));
        }

        Self { cases }
    }
}

type HyperClient = Client<
    hyper_rustls::HttpsConnector<hyper_util::client::legacy::connect::HttpConnector>,
    http_body_util::Full<Bytes>,
>;

/// Upper bound for establishing the TCP connection (direct, or to the upstream
/// proxy including the CONNECT tunnel) on the hyper paths.
///
/// Without it an upstream that blackholes SYNs would hold the request for the
/// full non-streaming timeout (600 s by default) before failover could move on;
/// the reqwest path has its own `connect_timeout`.
pub(crate) const CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// Shared rustls client config used by both the raw path and the hyper-util
/// fallback so the two sends trust the same roots (webpki + native certs).
///
/// Native system certs include user-installed proxy MITM CAs (Clash, mitmproxy)
/// so they are trusted through the CONNECT tunnel.
fn global_tls_config() -> &'static rustls::ClientConfig {
    static CONFIG: OnceLock<rustls::ClientConfig> = OnceLock::new();
    CONFIG.get_or_init(|| {
        let mut root_store = rustls::RootCertStore::empty();
        // Baseline: Mozilla/webpki roots
        root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        // Native system certs (includes user-installed proxy CAs)
        let native = rustls_native_certs::load_native_certs();
        let (added, _errors) = root_store.add_parsable_certificates(native.certs);
        log::debug!("[HyperClient] TLS root store: webpki + {added} native certs");
        rustls::ClientConfig::builder()
            .with_root_certificates(root_store)
            .with_no_client_auth()
    })
}

/// Lazily-initialized hyper client with header-case preservation enabled.
fn global_hyper_client() -> &'static HyperClient {
    static CLIENT: OnceLock<HyperClient> = OnceLock::new();
    CLIENT.get_or_init(|| {
        let mut http = hyper_util::client::legacy::connect::HttpConnector::new();
        http.enforce_http(false);
        http.set_connect_timeout(Some(CONNECT_TIMEOUT));
        let connector = HttpsConnectorBuilder::new()
            .with_tls_config(global_tls_config().clone())
            .https_or_http()
            .enable_http1()
            .wrap_connector(http);

        Client::builder(TokioExecutor::new())
            .http1_preserve_header_case(true)
            .http1_title_case_headers(true)
            .build(connector)
    })
}

/// Failure of the raw write path, split by whether request bytes may already
/// have reached the upstream.
#[derive(Debug)]
enum RawSendError {
    /// Nothing was written to the upstream yet (connect / tunnel / TLS failure).
    /// Retrying the same request through another client is safe.
    BeforeWrite(ProxyError),
    /// The request (or part of it) may already have been delivered. Re-sending
    /// it could duplicate a non-idempotent, billed request, so the error must be
    /// surfaced to the caller instead of silently retried.
    AfterWrite(ProxyError),
}

impl RawSendError {
    fn as_inner(&self) -> &ProxyError {
        match self {
            Self::BeforeWrite(e) | Self::AfterWrite(e) => e,
        }
    }

    fn into_inner(self) -> ProxyError {
        match self {
            Self::BeforeWrite(e) | Self::AfterWrite(e) => e,
        }
    }
}

/// Whether a raw-path failure may be retried through the hyper-util fallback.
///
/// - `AfterWrite`: never. The upstream may already have received (and billed)
///   the request; the failover loop decides what happens next.
/// - Upstream proxy configured: never, the fallback would bypass the proxy.
/// - Connect timeout: never, the fallback would just wait on the same
///   unreachable host again; failing fast lets failover proceed.
fn should_fall_back(err: &RawSendError, has_upstream_proxy: bool) -> bool {
    match err {
        RawSendError::AfterWrite(_) => false,
        RawSendError::BeforeWrite(_) if has_upstream_proxy => false,
        RawSendError::BeforeWrite(ProxyError::Timeout(_)) => false,
        RawSendError::BeforeWrite(_) => true,
    }
}

/// Bound a connect-phase future by `limit`, mapping expiry to `ProxyError::Timeout`.
async fn with_connect_timeout<T>(
    limit: std::time::Duration,
    what: &str,
    fut: impl std::future::Future<Output = Result<T, ProxyError>>,
) -> Result<T, ProxyError> {
    match tokio::time::timeout(limit, fut).await {
        Ok(result) => result,
        Err(_) => Err(ProxyError::Timeout(format!(
            "{what} timed out after {:.1}s",
            limit.as_secs_f32()
        ))),
    }
}

/// Unified response wrapper that can hold either a hyper or reqwest response.
///
/// The hyper variant is used for the main (direct) path with header-case preservation.
/// The reqwest variant is the fallback when an upstream HTTP/SOCKS5 proxy is configured.
pub enum ProxyResponse {
    Hyper(hyper::Response<hyper::body::Incoming>),
    Reqwest(reqwest::Response),
    Buffered {
        status: http::StatusCode,
        headers: http::HeaderMap,
        body: Bytes,
    },
    Streamed {
        status: http::StatusCode,
        headers: http::HeaderMap,
        stream: std::pin::Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send>>,
    },
}

impl ProxyResponse {
    pub fn buffered(status: http::StatusCode, headers: http::HeaderMap, body: Bytes) -> Self {
        Self::Buffered {
            status,
            headers,
            body,
        }
    }

    pub fn streamed(
        status: http::StatusCode,
        headers: http::HeaderMap,
        stream: impl Stream<Item = Result<Bytes, std::io::Error>> + Send + 'static,
    ) -> Self {
        Self::Streamed {
            status,
            headers,
            stream: Box::pin(stream),
        }
    }

    pub fn status(&self) -> http::StatusCode {
        match self {
            Self::Hyper(r) => r.status(),
            Self::Reqwest(r) => r.status(),
            Self::Buffered { status, .. } | Self::Streamed { status, .. } => *status,
        }
    }

    pub fn headers(&self) -> &http::HeaderMap {
        match self {
            Self::Hyper(r) => r.headers(),
            Self::Reqwest(r) => r.headers(),
            Self::Buffered { headers, .. } | Self::Streamed { headers, .. } => headers,
        }
    }

    /// Shortcut: extract `content-type` header value as `&str`.
    pub fn content_type(&self) -> Option<&str> {
        self.headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
    }

    /// Check if the response is an SSE stream.
    pub fn is_sse(&self) -> bool {
        self.content_type()
            .map(|ct| ct.contains("text/event-stream"))
            .unwrap_or(false)
    }

    /// Check whether the response explicitly declares a JSON media type.
    pub fn is_json(&self) -> bool {
        self.content_type()
            .map(|content_type| {
                let media_type = content_type
                    .split(';')
                    .next()
                    .unwrap_or("")
                    .trim()
                    .to_ascii_lowercase();
                media_type == "application/json" || media_type.ends_with("+json")
            })
            .unwrap_or(false)
    }

    /// Consume the response and collect the full body into `Bytes`.
    pub async fn bytes(self) -> Result<Bytes, ProxyError> {
        match self {
            Self::Hyper(r) => {
                let collected = r.into_body().collect().await.map_err(|e| {
                    ProxyError::ForwardFailed(format!("Failed to read response body: {e}"))
                })?;
                Ok(collected.to_bytes())
            }
            Self::Reqwest(r) => r.bytes().await.map_err(|e| {
                ProxyError::ForwardFailed(format!("Failed to read response body: {e}"))
            }),
            Self::Buffered { body, .. } => Ok(body),
            Self::Streamed { mut stream, .. } => {
                let mut body = bytes::BytesMut::new();
                while let Some(chunk) = stream.next().await {
                    let chunk = chunk.map_err(|e| {
                        ProxyError::ForwardFailed(format!("Failed to read response body: {e}"))
                    })?;
                    body.extend_from_slice(&chunk);
                }
                Ok(body.freeze())
            }
        }
    }

    /// Consume the response and return a byte-chunk stream (for SSE pass-through).
    pub fn bytes_stream(self) -> impl Stream<Item = Result<Bytes, std::io::Error>> + Send {
        use futures::StreamExt;

        match self {
            Self::Hyper(r) => {
                let body = r.into_body();
                let stream = futures::stream::unfold(body, |mut body| async {
                    match body.frame().await {
                        Some(Ok(frame)) => {
                            if let Ok(data) = frame.into_data() {
                                if data.is_empty() {
                                    Some((Ok(Bytes::new()), body))
                                } else {
                                    Some((Ok(data), body))
                                }
                            } else {
                                Some((Ok(Bytes::new()), body))
                            }
                        }
                        Some(Err(e)) => Some((Err(std::io::Error::other(e.to_string())), body)),
                        None => None,
                    }
                })
                .filter(|result| {
                    futures::future::ready(!matches!(result, Ok(ref b) if b.is_empty()))
                });
                Box::pin(stream)
                    as std::pin::Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send>>
            }
            Self::Reqwest(r) => {
                let stream = r
                    .bytes_stream()
                    .map(|r| r.map_err(|e| std::io::Error::other(e.to_string())));
                Box::pin(stream)
            }
            Self::Buffered { body, .. } => Box::pin(futures::stream::once(async move { Ok(body) }))
                as std::pin::Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send>>,
            Self::Streamed { stream, .. } => stream,
        }
    }
}

/// Send an HTTP request with header-case preservation.
///
/// Uses a two-tier strategy:
/// 1. Primary: raw HTTP/1.1 write via TLS stream with exact original header casing
///    (from `OriginalHeaderCases` captured by peek in server.rs), then hand off to
///    hyper for response parsing.
/// 2. Fallback: hyper-util Client with `title_case_headers(true)` when raw write
///    isn't feasible (e.g., missing original cases).
///
/// The caller is expected to include `Host` in the supplied `headers` at the
/// correct position.
///
/// `proxy_url`: optional upstream HTTP proxy URL (e.g. `http://127.0.0.1:7890`).
/// When set, the raw write path uses HTTP CONNECT tunneling through the proxy,
/// so header-case preservation works even when an upstream proxy is configured.
pub async fn send_request(
    uri: http::Uri,
    method: http::Method,
    headers: http::HeaderMap,
    original_extensions: http::Extensions,
    body: Vec<u8>,
    timeout: std::time::Duration,
    proxy_url: Option<&str>,
) -> Result<ProxyResponse, ProxyError> {
    // Extract our own OriginalHeaderCases if available
    let original_cases = original_extensions.get::<OriginalHeaderCases>().cloned();
    let has_cases = original_cases
        .as_ref()
        .map(|c| !c.cases.is_empty())
        .unwrap_or(false);

    log::debug!(
        "[HyperClient] Sending request: uri={uri}, header_count={}, \
         has_host={}, has_original_cases={has_cases}, proxy={:?}",
        headers.len(),
        headers.contains_key(http::header::HOST),
        proxy_url.map(super::http_client::mask_url),
    );

    let deadline = tokio::time::Instant::now() + timeout;
    let timeout_error = || ProxyError::Timeout(format!("请求超时: {}s", timeout.as_secs()));

    if let Some(original_cases) = original_cases
        .as_ref()
        .filter(|cases| !cases.cases.is_empty())
    {
        // Primary path: use raw write + hyper handshake for exact header casing
        let connect_timeout = CONNECT_TIMEOUT.min(timeout);
        let result = tokio::time::timeout_at(
            deadline,
            send_raw_request(
                &uri,
                &method,
                &headers,
                original_cases,
                &body,
                proxy_url,
                connect_timeout,
            ),
        )
        .await
        .map_err(|_| timeout_error())?;

        match result {
            Ok(resp) => return Ok(resp),
            Err(err) => {
                if !should_fall_back(&err, proxy_url.is_some()) {
                    if let RawSendError::AfterWrite(e) = &err {
                        log::warn!(
                            "[HyperClient] Raw request failed after it was written upstream; \
                             not re-sending: {e}"
                        );
                    }
                    return Err(err.into_inner());
                }
                log::warn!(
                    "[HyperClient] Raw write failed, falling back to hyper-util: {}",
                    err.as_inner()
                );
                // Fall through to hyper-util Client
            }
        }
    }

    // Fallback: hyper-util Client (title-case headers, no proxy support)
    let mut req = http::Request::builder()
        .method(method)
        .uri(&uri)
        .body(http_body_util::Full::new(Bytes::from(body)))
        .map_err(|e| ProxyError::ForwardFailed(format!("Failed to build request: {e}")))?;

    *req.headers_mut() = headers;
    *req.extensions_mut() = original_extensions;

    let client = global_hyper_client();
    // Share one deadline with the raw attempt so a fallback never doubles the budget.
    let resp = tokio::time::timeout_at(deadline, client.request(req))
        .await
        .map_err(|_| timeout_error())?
        .map_err(|e| ProxyError::ForwardFailed(format!("上游请求失败: {e}")))?;

    Ok(ProxyResponse::Hyper(resp))
}

/// TCP or TLS stream returned by `connect_via_proxy`.
///
/// When the proxy URL uses `https://`, the connection to the proxy itself is
/// TLS-wrapped before sending the CONNECT request.  The enum lets
/// `send_raw_request` work with either variant generically.
enum ProxyStream {
    Tcp(tokio::net::TcpStream),
    Tls(Box<tokio_rustls::client::TlsStream<tokio::net::TcpStream>>),
}

impl tokio::io::AsyncRead for ProxyStream {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match self.get_mut() {
            ProxyStream::Tcp(s) => std::pin::Pin::new(s).poll_read(cx, buf),
            ProxyStream::Tls(s) => std::pin::Pin::new(s).poll_read(cx, buf),
        }
    }
}

impl tokio::io::AsyncWrite for ProxyStream {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        match self.get_mut() {
            ProxyStream::Tcp(s) => std::pin::Pin::new(s).poll_write(cx, buf),
            ProxyStream::Tls(s) => std::pin::Pin::new(s).poll_write(cx, buf),
        }
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match self.get_mut() {
            ProxyStream::Tcp(s) => std::pin::Pin::new(s).poll_flush(cx),
            ProxyStream::Tls(s) => std::pin::Pin::new(s).poll_flush(cx),
        }
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match self.get_mut() {
            ProxyStream::Tcp(s) => std::pin::Pin::new(s).poll_shutdown(cx),
            ProxyStream::Tls(s) => std::pin::Pin::new(s).poll_shutdown(cx),
        }
    }
}

/// Send request via raw TCP/TLS with exact original header casing.
///
/// The request bytes are built by `build_raw_request` (original wire casing and
/// order) and written on a pooled HTTP/1.1 keep-alive connection keyed by
/// (scheme, host, port, proxy). An idle connection is reused when one is
/// available; a fresh one (direct, or through an HTTP CONNECT tunnel when
/// `proxy_url` is set) is opened otherwise.
///
/// If a reused connection turns out to be dead before any request byte was
/// written to it, the next idle connection (or a fresh one) is tried. Once a
/// byte may have reached the upstream the failure is `AfterWrite` and the
/// request is never re-sent.
async fn send_raw_request(
    uri: &http::Uri,
    method: &http::Method,
    headers: &http::HeaderMap,
    original_cases: &OriginalHeaderCases,
    body: &[u8],
    proxy_url: Option<&str>,
    connect_timeout: std::time::Duration,
) -> Result<ProxyResponse, RawSendError> {
    let scheme = uri.scheme_str().unwrap_or("https");
    let host = uri.host().ok_or_else(|| {
        RawSendError::BeforeWrite(ProxyError::ForwardFailed("URI has no host".into()))
    })?;
    let port = uri
        .port_u16()
        .unwrap_or(if scheme == "https" { 443 } else { 80 });
    let path_and_query = uri.path_and_query().map(|pq| pq.as_str()).unwrap_or("/");

    // Build raw HTTP request bytes
    let raw = Bytes::from(build_raw_request(
        method,
        path_and_query,
        headers,
        original_cases,
        body,
    ));
    let close_after = request_wants_close(headers);

    let key = PoolKey {
        scheme: scheme.to_ascii_lowercase(),
        host: host.to_ascii_lowercase(),
        port,
        proxy: proxy_url.map(str::to_string),
    };

    // Reuse an idle keep-alive connection when possible.
    while let Some(conn) = raw_pool().checkout(&key) {
        match send_on_conn(conn, &key, raw.clone(), method, close_after).await {
            Err(RawSendError::BeforeWrite(e)) => {
                log::debug!(
                    "[HyperClient] Pooled connection unusable before write, trying another: {e}"
                );
            }
            other => return other,
        }
    }

    let conn = open_raw_conn(scheme, host, port, proxy_url, connect_timeout).await?;
    send_on_conn(conn, &key, raw, method, close_after).await
}

/// Whether the outgoing request asks the upstream to close the connection.
fn request_wants_close(headers: &http::HeaderMap) -> bool {
    headers
        .get_all(http::header::CONNECTION)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .any(|token| token.trim().eq_ignore_ascii_case("close"))
}

/// Open a new upstream connection (TCP or CONNECT tunnel, then TLS for https)
/// and set up hyper's HTTP/1.1 client state machine on it.
///
/// Every failure here happens before any request byte is written.
async fn open_raw_conn(
    scheme: &str,
    host: &str,
    port: u16,
    proxy_url: Option<&str>,
    connect_timeout: std::time::Duration,
) -> Result<RawConn, RawSendError> {
    // Establish TCP connection — either direct or through HTTP CONNECT proxy
    let stream = if let Some(proxy) = proxy_url {
        with_connect_timeout(
            connect_timeout,
            "Proxy tunnel setup",
            connect_via_proxy(proxy, host, port),
        )
        .await
        .map_err(RawSendError::BeforeWrite)?
    } else {
        ProxyStream::Tcp(
            with_connect_timeout(connect_timeout, "TCP connect", async {
                tokio::net::TcpStream::connect((host, port))
                    .await
                    .map_err(|e| ProxyError::ForwardFailed(format!("TCP connect failed: {e}")))
            })
            .await
            .map_err(RawSendError::BeforeWrite)?,
        )
    };

    if scheme == "https" {
        let tls_connector = global_tls_connector();
        let server_name =
            rustls::pki_types::ServerName::try_from(host.to_string()).map_err(|e| {
                RawSendError::BeforeWrite(ProxyError::ForwardFailed(format!(
                    "Invalid server name: {e}"
                )))
            })?;
        let tls_stream = tls_connector
            .connect(server_name, stream)
            .await
            .map_err(|e| {
                RawSendError::BeforeWrite(ProxyError::ForwardFailed(format!(
                    "TLS handshake failed: {e}"
                )))
            })?;
        RawConn::handshake(tls_stream).await
    } else {
        RawConn::handshake(stream).await
    }
}

/// Send one raw request on `conn` and parse the response with hyper.
///
/// On success the connection goes back to the pool once the response body has
/// been fully consumed (unless the request asked to close it, or hyper decides
/// the connection cannot be kept alive).
async fn send_on_conn(
    mut conn: RawConn,
    key: &PoolKey,
    raw: Bytes,
    method: &http::Method,
    close_after: bool,
) -> Result<ProxyResponse, RawSendError> {
    if let Err(e) = conn.sender.ready().await {
        return Err(RawSendError::BeforeWrite(ProxyError::ForwardFailed(
            format!("Upstream connection closed: {e}"),
        )));
    }

    conn.slot.arm(raw);

    // hyper encodes this placeholder request, but `WriteFilter` drops those
    // bytes and writes our raw request in their place. Hyper then parses the
    // upstream's response to the raw request with the same method semantics
    // (e.g. no body for HEAD) and tracks keep-alive state for the connection.
    let mut dummy = http::Request::builder().method(method.clone()).uri("/");
    if close_after {
        dummy = dummy.header(http::header::CONNECTION, "close");
    }
    let dummy_req = dummy
        .body(http_body_util::Full::new(Bytes::new()))
        .map_err(|e| {
            RawSendError::BeforeWrite(ProxyError::ForwardFailed(format!(
                "Build dummy request: {e}"
            )))
        })?;

    match conn.sender.send_request(dummy_req).await {
        Ok(resp) => {
            if !close_after {
                raw_pool().return_when_idle(key.clone(), conn);
            }
            Ok(ProxyResponse::Hyper(resp))
        }
        Err(e) => {
            let err = ProxyError::ForwardFailed(format!("Upstream request failed: {e}"));
            if conn.slot.write_started() {
                Err(RawSendError::AfterWrite(err))
            } else {
                Err(RawSendError::BeforeWrite(err))
            }
        }
    }
}

/// How long an idle raw connection is kept for reuse. Kept below the common
/// upstream keep-alive timeouts (nginx 75 s, many CDNs 60 s) so we rarely pick
/// a connection the server is about to close.
const RAW_POOL_IDLE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// Maximum idle raw connections kept per pool key.
const RAW_POOL_MAX_IDLE_PER_KEY: usize = 8;

/// Identity of an upstream connection: same scheme, host, port and upstream
/// proxy means the connection can carry the next request.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct PoolKey {
    scheme: String,
    host: String,
    port: u16,
    proxy: Option<String>,
}

/// One HTTP/1.1 connection used by the raw path.
struct RawConn {
    sender: hyper::client::conn::http1::SendRequest<http_body_util::Full<Bytes>>,
    slot: std::sync::Arc<RawWriteSlot>,
}

impl RawConn {
    /// Run hyper's HTTP/1.1 client handshake (no I/O for HTTP/1) on `stream`
    /// wrapped in a `WriteFilter`, and spawn the connection driver.
    async fn handshake<S>(stream: S) -> Result<Self, RawSendError>
    where
        S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
    {
        let slot = std::sync::Arc::new(RawWriteSlot::default());
        let io = hyper_util::rt::TokioIo::new(WriteFilter {
            inner: stream,
            slot: slot.clone(),
        });

        let (sender, conn) = hyper::client::conn::http1::Builder::new()
            .preserve_header_case(true)
            .handshake::<_, http_body_util::Full<Bytes>>(io)
            .await
            .map_err(|e| {
                RawSendError::BeforeWrite(ProxyError::ForwardFailed(format!(
                    "Handshake failed: {e}"
                )))
            })?;

        // The driver lives as long as the connection: it ends when the
        // upstream closes it or when the last `SendRequest` is dropped.
        tokio::spawn(async move {
            if let Err(e) = conn.await {
                log::debug!("[HyperClient] raw conn driver error: {e}");
            }
        });

        Ok(Self { sender, slot })
    }
}

struct IdleConn {
    id: u64,
    conn: RawConn,
    idle_since: tokio::time::Instant,
}

impl IdleConn {
    fn usable(&self) -> bool {
        !self.conn.sender.is_closed()
            && self.conn.sender.is_ready()
            && self.idle_since.elapsed() < RAW_POOL_IDLE_TIMEOUT
    }
}

type IdleMap = std::collections::HashMap<PoolKey, Vec<IdleConn>>;

/// Keep-alive pool for raw-path connections.
#[derive(Default)]
struct RawConnPool {
    idle: std::sync::Mutex<IdleMap>,
    next_id: std::sync::atomic::AtomicU64,
}

fn raw_pool() -> &'static RawConnPool {
    static POOL: OnceLock<RawConnPool> = OnceLock::new();
    POOL.get_or_init(RawConnPool::default)
}

impl RawConnPool {
    fn lock(&self) -> std::sync::MutexGuard<'_, IdleMap> {
        self.idle.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Take the most recently used live idle connection for `key`.
    fn checkout(&self, key: &PoolKey) -> Option<RawConn> {
        let mut idle = self.lock();
        let list = idle.get_mut(key)?;
        let mut found = None;
        // Dropping a stale entry closes its connection.
        while let Some(entry) = list.pop() {
            if entry.usable() {
                found = Some(entry.conn);
                break;
            }
        }
        if list.is_empty() {
            idle.remove(key);
        }
        found
    }

    /// Return `conn` to the pool once its current response has been fully read.
    ///
    /// `ready()` resolves when hyper's dispatcher is idle again (response body
    /// consumed, keep-alive allowed) and errors when the connection was closed
    /// (including a body dropped half-read), in which case it is just dropped.
    fn return_when_idle(&'static self, key: PoolKey, mut conn: RawConn) {
        tokio::spawn(async move {
            if conn.sender.ready().await.is_err() {
                return;
            }
            let id = self
                .next_id
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            {
                let mut idle = self.lock();
                let list = idle.entry(key.clone()).or_default();
                list.retain(IdleConn::usable);
                if list.len() >= RAW_POOL_MAX_IDLE_PER_KEY {
                    return;
                }
                list.push(IdleConn {
                    id,
                    conn,
                    idle_since: tokio::time::Instant::now(),
                });
            }
            // Evict after the idle timeout even if no further request for this
            // key arrives, so idle sockets do not linger.
            tokio::time::sleep(RAW_POOL_IDLE_TIMEOUT).await;
            let mut idle = self.lock();
            if let Some(list) = idle.get_mut(&key) {
                list.retain(|entry| entry.id != id);
                if list.is_empty() {
                    idle.remove(&key);
                }
            }
        });
    }

    #[cfg(test)]
    fn idle_count(&self, key: &PoolKey) -> usize {
        self.lock().get(key).map(Vec::len).unwrap_or(0)
    }
}

/// Establish a connection through an HTTP CONNECT proxy tunnel.
///
/// 1. Connect TCP to the proxy server (TLS-wrapped when `https://` proxy)
/// 2. Send `CONNECT host:port HTTP/1.1` with optional `Proxy-Authorization`
/// 3. Read the proxy's 200 response (407 → `AuthError`)
/// 4. Return the tunneled stream (ready for target TLS handshake + raw write)
async fn connect_via_proxy(
    proxy_url: &str,
    target_host: &str,
    target_port: u16,
) -> Result<ProxyStream, ProxyError> {
    use base64::Engine;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let parsed = url::Url::parse(proxy_url)
        .map_err(|e| ProxyError::ForwardFailed(format!("Invalid proxy URL: {e}")))?;

    let proxy_host = parsed
        .host_str()
        .ok_or_else(|| ProxyError::ForwardFailed("Proxy URL has no host".into()))?;
    let proxy_port = parsed
        .port()
        .unwrap_or(if parsed.scheme() == "https" { 443 } else { 80 });

    // Build Proxy-Authorization header if credentials are present
    let proxy_auth = if !parsed.username().is_empty() {
        let password = parsed.password().unwrap_or("");
        let credentials = format!("{}:{}", parsed.username(), password);
        let encoded = base64::engine::general_purpose::STANDARD.encode(credentials);
        Some(format!("Proxy-Authorization: Basic {encoded}\r\n"))
    } else {
        None
    };

    // Connect to the proxy
    let tcp = tokio::net::TcpStream::connect((proxy_host, proxy_port))
        .await
        .map_err(|e| ProxyError::ForwardFailed(format!("Proxy TCP connect failed: {e}")))?;

    // Wrap with TLS if the proxy URL uses https://
    let mut stream: ProxyStream = if parsed.scheme() == "https" {
        let tls_connector = global_tls_connector();
        let server_name = rustls::pki_types::ServerName::try_from(proxy_host.to_string())
            .map_err(|e| ProxyError::ForwardFailed(format!("Invalid proxy server name: {e}")))?;
        let tls_stream = tls_connector
            .connect(server_name, tcp)
            .await
            .map_err(|e| ProxyError::ForwardFailed(format!("Proxy TLS handshake failed: {e}")))?;
        ProxyStream::Tls(Box::new(tls_stream))
    } else {
        ProxyStream::Tcp(tcp)
    };

    // Send CONNECT request
    let mut connect_req = format!(
        "CONNECT {target_host}:{target_port} HTTP/1.1\r\n\
         Host: {target_host}:{target_port}\r\n"
    );
    if let Some(auth) = &proxy_auth {
        connect_req.push_str(auth);
    }
    connect_req.push_str("\r\n");

    stream
        .write_all(connect_req.as_bytes())
        .await
        .map_err(|e| ProxyError::ForwardFailed(format!("CONNECT write failed: {e}")))?;
    stream
        .flush()
        .await
        .map_err(|e| ProxyError::ForwardFailed(format!("CONNECT flush failed: {e}")))?;

    // Read the proxy's response status line
    let mut reader = BufReader::new(&mut stream);
    let mut status_line = String::new();
    reader
        .read_line(&mut status_line)
        .await
        .map_err(|e| ProxyError::ForwardFailed(format!("CONNECT read failed: {e}")))?;

    // Expect "HTTP/1.1 200 ..." or "HTTP/1.0 200 ..."
    if !status_line.contains(" 200 ") {
        if status_line.contains(" 407 ") {
            return Err(ProxyError::AuthError(format!(
                "Proxy authentication required (407): {}",
                status_line.trim()
            )));
        }
        return Err(ProxyError::ForwardFailed(format!(
            "Proxy CONNECT rejected: {}",
            status_line.trim()
        )));
    }

    // Drain remaining response headers (until empty line)
    loop {
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .await
            .map_err(|e| ProxyError::ForwardFailed(format!("CONNECT header read: {e}")))?;
        if line.trim().is_empty() {
            break;
        }
    }
    // BufReader might have buffered data; drop it to get raw stream back.
    // Since CONNECT response is headers-only (no body), and we read until \r\n\r\n,
    // the BufReader buffer should be empty at this point.
    drop(reader);

    log::debug!(
        "[HyperClient] CONNECT tunnel established via {proxy_host}:{proxy_port} -> {target_host}:{target_port}"
    );

    Ok(stream)
}

/// Lazily-initialized TLS connector for raw connections (see `global_tls_config`).
fn global_tls_connector() -> &'static tokio_rustls::TlsConnector {
    static CONNECTOR: OnceLock<tokio_rustls::TlsConnector> = OnceLock::new();
    CONNECTOR.get_or_init(|| {
        tokio_rustls::TlsConnector::from(std::sync::Arc::new(global_tls_config().clone()))
    })
}

/// Build raw HTTP/1.1 request bytes with original header casing.
fn build_raw_request(
    method: &http::Method,
    path_and_query: &str,
    headers: &http::HeaderMap,
    original_cases: &OriginalHeaderCases,
    body: &[u8],
) -> Vec<u8> {
    let mut raw = Vec::with_capacity(4096 + body.len());

    // Request line
    raw.extend_from_slice(method.as_str().as_bytes());
    raw.extend_from_slice(b" ");
    raw.extend_from_slice(path_and_query.as_bytes());
    raw.extend_from_slice(b" HTTP/1.1\r\n");

    // Headers with original casing, emitted in original wire order.
    //
    // Strategy:
    // 1. Walk `original_cases.cases` in order — this preserves the exact
    //    header sequence the client sent.  For each entry, emit the stored
    //    original-casing name plus the current value from `headers` (the
    //    proxy may have rewritten the value, e.g. Authorization).
    //    Repeated headers with the same name are handled by tracking a
    //    per-name value cursor so we step through `get_all()` in order.
    // 2. After the original headers, append any headers that exist in
    //    `headers` but were not present in the original request (i.e. added
    //    by the proxy).  These are emitted in lowercase.
    //
    // This replaces the old `for name in headers.keys()` loop which iterated
    // in hash-map order, destroying the original header sequence.
    let mut emitted: std::collections::HashSet<String> =
        std::collections::HashSet::with_capacity(original_cases.cases.len());
    // Per-name cursor: how many values we have already emitted for each name.
    let mut value_cursor: std::collections::HashMap<String, usize> =
        std::collections::HashMap::with_capacity(original_cases.cases.len());

    for (lower_name, orig_name_bytes) in &original_cases.cases {
        if let Ok(header_name) = http::header::HeaderName::from_bytes(lower_name.as_bytes()) {
            let all_values: Vec<_> = headers.get_all(&header_name).iter().collect();
            let cursor = value_cursor.entry(lower_name.clone()).or_insert(0);
            if let Some(value) = all_values.get(*cursor) {
                raw.extend_from_slice(orig_name_bytes);
                raw.extend_from_slice(b": ");
                raw.extend_from_slice(value.as_bytes());
                raw.extend_from_slice(b"\r\n");
                *cursor += 1;
                emitted.insert(lower_name.clone());
            }
        }
    }

    // Append proxy-added headers (not present in the original request).
    for name in headers.keys() {
        let lower = name.as_str().to_ascii_lowercase();
        if !emitted.contains(&lower) {
            for value in headers.get_all(name) {
                raw.extend_from_slice(name.as_str().as_bytes());
                raw.extend_from_slice(b": ");
                raw.extend_from_slice(value.as_bytes());
                raw.extend_from_slice(b"\r\n");
            }
            emitted.insert(lower);
        }
    }

    // Add Content-Length if not already present
    if !headers.contains_key(http::header::CONTENT_LENGTH) {
        raw.extend_from_slice(b"Content-Length: ");
        raw.extend_from_slice(body.len().to_string().as_bytes());
        raw.extend_from_slice(b"\r\n");
    }

    // End of headers + body
    raw.extend_from_slice(b"\r\n");
    raw.extend_from_slice(body);

    raw
}

/// Raw request bytes waiting to be written in place of hyper's own encoding.
#[derive(Default)]
struct RawWriteSlot {
    state: std::sync::Mutex<RawWriteState>,
}

#[derive(Default)]
struct RawWriteState {
    pending: Bytes,
    started: bool,
}

impl RawWriteSlot {
    fn lock(&self) -> std::sync::MutexGuard<'_, RawWriteState> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Queue the next request's raw bytes.
    fn arm(&self, raw: Bytes) {
        let mut state = self.lock();
        state.pending = raw;
        state.started = false;
    }

    /// Whether any byte of the armed request may have reached the upstream.
    fn write_started(&self) -> bool {
        self.lock().started
    }
}

/// A stream wrapper that replaces hyper's request bytes with our raw request.
///
/// hyper's connection driver thinks it sent its own placeholder request: the
/// bytes it writes are discarded, and its first write of each request instead
/// sends the raw bytes armed in the `RawWriteSlot`. Reads pass through, so
/// hyper parses the upstream's response to the raw request and manages
/// keep-alive for the connection as usual.
struct WriteFilter<S> {
    inner: S,
    slot: std::sync::Arc<RawWriteSlot>,
}

impl<S: tokio::io::AsyncWrite + Unpin> WriteFilter<S> {
    /// Write out any armed raw bytes; `Ready(Ok)` once none remain.
    fn poll_drain_raw(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        use bytes::Buf;
        use std::task::Poll;

        let mut state = self.slot.lock();
        while !state.pending.is_empty() {
            // From the first write attempt on, the upstream may see the request.
            state.started = true;
            match std::pin::Pin::new(&mut self.inner).poll_write(cx, &state.pending) {
                Poll::Ready(Ok(0)) => {
                    return Poll::Ready(Err(std::io::ErrorKind::WriteZero.into()));
                }
                Poll::Ready(Ok(n)) => state.pending.advance(n),
                Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                Poll::Pending => return Poll::Pending,
            }
        }
        Poll::Ready(Ok(()))
    }
}

impl<S: tokio::io::AsyncRead + Unpin> tokio::io::AsyncRead for WriteFilter<S> {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        // Pass reads through to the underlying stream
        let inner = std::pin::Pin::new(&mut self.get_mut().inner);
        inner.poll_read(cx, buf)
    }
}

impl<S: tokio::io::AsyncWrite + Unpin> tokio::io::AsyncWrite for WriteFilter<S> {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        // Send our raw request first, then report hyper's bytes as written
        // without sending them.
        match self.get_mut().poll_drain_raw(cx) {
            std::task::Poll::Ready(Ok(())) => std::task::Poll::Ready(Ok(buf.len())),
            std::task::Poll::Ready(Err(e)) => std::task::Poll::Ready(Err(e)),
            std::task::Poll::Pending => std::task::Poll::Pending,
        }
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        let this = self.get_mut();
        match this.poll_drain_raw(cx) {
            std::task::Poll::Ready(Ok(())) => std::pin::Pin::new(&mut this.inner).poll_flush(cx),
            other => other,
        }
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.get_mut().inner).poll_shutdown(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buffered_with_content_type(content_type: Option<&str>) -> ProxyResponse {
        let mut headers = http::HeaderMap::new();
        if let Some(content_type) = content_type {
            headers.insert(
                http::header::CONTENT_TYPE,
                http::HeaderValue::from_str(content_type).unwrap(),
            );
        }
        ProxyResponse::buffered(http::StatusCode::OK, headers, Bytes::new())
    }

    fn sample_cases() -> OriginalHeaderCases {
        OriginalHeaderCases {
            cases: vec![
                ("host".into(), b"Host".to_vec()),
                ("content-type".into(), b"Content-Type".to_vec()),
            ],
        }
    }

    fn sample_headers(host: &str) -> http::HeaderMap {
        let mut headers = http::HeaderMap::new();
        headers.insert(http::header::HOST, host.parse().unwrap());
        headers.insert(
            http::header::CONTENT_TYPE,
            http::HeaderValue::from_static("application/json"),
        );
        headers
    }

    /// Fake upstream: accepts connections, reads one full request, then closes
    /// the socket without answering. Returns the address and a counter of
    /// complete requests received.
    async fn spawn_reset_after_read_upstream() -> (
        std::net::SocketAddr,
        std::sync::Arc<std::sync::atomic::AtomicUsize>,
    ) {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use tokio::io::AsyncReadExt;

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let count = std::sync::Arc::new(AtomicUsize::new(0));
        let counter = count.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    return;
                };
                let counter = counter.clone();
                tokio::spawn(async move {
                    let mut buf = Vec::new();
                    let mut chunk = [0u8; 4096];
                    loop {
                        match sock.read(&mut chunk).await {
                            Ok(0) | Err(_) => return,
                            Ok(n) => buf.extend_from_slice(&chunk[..n]),
                        }
                        // Request is complete once the body (after the blank line) is in.
                        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                            if buf.len() >= pos + 4 + 2 {
                                counter.fetch_add(1, Ordering::SeqCst);
                                return; // drop socket without a response
                            }
                        }
                    }
                });
            }
        });
        (addr, count)
    }

    #[tokio::test]
    async fn raw_path_does_not_resend_after_request_was_written() {
        use std::sync::atomic::Ordering;

        let (addr, count) = spawn_reset_after_read_upstream().await;
        let uri: http::Uri = format!("http://{addr}/v1/messages").parse().unwrap();
        let mut ext = http::Extensions::new();
        ext.insert(sample_cases());

        let result = send_request(
            uri,
            http::Method::POST,
            sample_headers(&addr.to_string()),
            ext,
            b"{}".to_vec(),
            std::time::Duration::from_secs(5),
            None,
        )
        .await;

        assert!(result.is_err(), "upstream closed without a response");
        // Give a would-be fallback request time to arrive.
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        assert_eq!(
            count.load(Ordering::SeqCst),
            1,
            "request must reach the upstream exactly once"
        );
    }

    #[tokio::test]
    async fn raw_path_classifies_post_write_failure_as_after_write() {
        let (addr, _count) = spawn_reset_after_read_upstream().await;
        let uri: http::Uri = format!("http://{addr}/").parse().unwrap();
        let err = send_raw_request(
            &uri,
            &http::Method::POST,
            &sample_headers(&addr.to_string()),
            &sample_cases(),
            b"{}",
            None,
            CONNECT_TIMEOUT,
        )
        .await
        .err()
        .expect("must fail");
        assert!(matches!(err, RawSendError::AfterWrite(_)), "{err:?}");
        assert!(!should_fall_back(&err, false));
    }

    /// Keep-alive fake upstream: answers every request with `response` and
    /// records the exact bytes of each request together with the index of the
    /// TCP connection it arrived on.
    async fn spawn_keepalive_upstream(
        response: &'static str,
    ) -> (
        std::net::SocketAddr,
        std::sync::Arc<std::sync::Mutex<Vec<(usize, Vec<u8>)>>>,
        std::sync::Arc<std::sync::atomic::AtomicUsize>,
    ) {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let requests = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let accepted = std::sync::Arc::new(AtomicUsize::new(0));
        let (requests_tx, accepted_tx) = (requests.clone(), accepted.clone());
        tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    return;
                };
                let conn_idx = accepted_tx.fetch_add(1, Ordering::SeqCst);
                let requests = requests_tx.clone();
                tokio::spawn(async move {
                    let mut buf = Vec::new();
                    let mut chunk = [0u8; 4096];
                    loop {
                        // Parse one complete request (head + Content-Length body).
                        let complete = loop {
                            if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                                let mut hdrs = [httparse::EMPTY_HEADER; 64];
                                let mut req = httparse::Request::new(&mut hdrs);
                                req.parse(&buf[..pos + 4]).unwrap();
                                let len = req
                                    .headers
                                    .iter()
                                    .find(|h| h.name.eq_ignore_ascii_case("content-length"))
                                    .map(|h| std::str::from_utf8(h.value).unwrap().parse().unwrap())
                                    .unwrap_or(0usize);
                                if buf.len() >= pos + 4 + len {
                                    break pos + 4 + len;
                                }
                            }
                            match sock.read(&mut chunk).await {
                                Ok(0) | Err(_) => return,
                                Ok(n) => buf.extend_from_slice(&chunk[..n]),
                            }
                        };
                        let request: Vec<u8> = buf.drain(..complete).collect();
                        requests.lock().unwrap().push((conn_idx, request));
                        if sock.write_all(response.as_bytes()).await.is_err() {
                            return;
                        }
                    }
                });
            }
        });
        (addr, requests, accepted)
    }

    fn test_key(addr: std::net::SocketAddr) -> PoolKey {
        PoolKey {
            scheme: "http".into(),
            host: addr.ip().to_string(),
            port: addr.port(),
            proxy: None,
        }
    }

    async fn wait_until_pooled(key: &PoolKey) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while raw_pool().idle_count(key) == 0 {
            assert!(
                std::time::Instant::now() < deadline,
                "connection was never returned to the pool"
            );
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    }

    #[tokio::test]
    async fn raw_path_preserves_wire_case_and_order_and_reuses_one_connection() {
        let (addr, requests, accepted) =
            spawn_keepalive_upstream("HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok").await;
        let host = addr.to_string();

        // Original client order and casing, as captured by server.rs.
        let cases = OriginalHeaderCases {
            cases: vec![
                ("host".into(), b"Host".to_vec()),
                ("x-stainless-lang".into(), b"X-Stainless-Lang".to_vec()),
                ("anthropic-version".into(), b"anthropic-version".to_vec()),
                ("content-type".into(), b"Content-Type".to_vec()),
            ],
        };
        // Header map in a different order, plus a proxy-added credential.
        let mut headers = http::HeaderMap::new();
        headers.insert("content-type", "application/json".parse().unwrap());
        headers.insert("anthropic-version", "2023-06-01".parse().unwrap());
        headers.insert(http::header::HOST, host.parse().unwrap());
        headers.insert("x-stainless-lang", "js".parse().unwrap());
        headers.insert("x-api-key", "sk-test".parse().unwrap());

        let uri: http::Uri = format!("http://{addr}/v1/messages").parse().unwrap();
        for _ in 0..2 {
            let mut ext = http::Extensions::new();
            ext.insert(cases.clone());
            let resp = send_request(
                uri.clone(),
                http::Method::POST,
                headers.clone(),
                ext,
                b"{}".to_vec(),
                std::time::Duration::from_secs(5),
                None,
            )
            .await
            .expect("request succeeds");
            assert_eq!(resp.status(), http::StatusCode::OK);
            assert_eq!(resp.bytes().await.unwrap().as_ref(), b"ok");
            wait_until_pooled(&test_key(addr)).await;
        }

        let expected = format!(
            "POST /v1/messages HTTP/1.1\r\n\
             Host: {host}\r\n\
             X-Stainless-Lang: js\r\n\
             anthropic-version: 2023-06-01\r\n\
             Content-Type: application/json\r\n\
             x-api-key: sk-test\r\n\
             Content-Length: 2\r\n\
             \r\n\
             {{}}"
        );
        let requests = requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 2, "hyper's placeholder must never be sent");
        for (conn_idx, bytes) in &requests {
            assert_eq!(*conn_idx, 0, "both requests must use the first connection");
            assert_eq!(String::from_utf8_lossy(bytes), expected);
        }
        assert_eq!(
            accepted.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "two sequential requests must share one TCP connection"
        );
    }

    #[tokio::test]
    async fn connection_close_response_is_not_reused() {
        let (addr, requests, accepted) = spawn_keepalive_upstream(
            "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok",
        )
        .await;
        let uri: http::Uri = format!("http://{addr}/v1/messages").parse().unwrap();
        for _ in 0..2 {
            let mut ext = http::Extensions::new();
            ext.insert(sample_cases());
            let resp = send_request(
                uri.clone(),
                http::Method::POST,
                sample_headers(&addr.to_string()),
                ext,
                b"{}".to_vec(),
                std::time::Duration::from_secs(5),
                None,
            )
            .await
            .expect("request succeeds");
            assert_eq!(resp.bytes().await.unwrap().as_ref(), b"ok");
        }
        assert_eq!(requests.lock().unwrap().len(), 2);
        assert_eq!(accepted.load(std::sync::atomic::Ordering::SeqCst), 2);
        assert_eq!(raw_pool().idle_count(&test_key(addr)), 0);
    }

    #[tokio::test]
    async fn connect_failure_is_before_write_and_falls_back() {
        // Bind then drop to get a port with nothing listening.
        let addr = {
            let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            l.local_addr().unwrap()
        };
        let uri: http::Uri = format!("http://{addr}/").parse().unwrap();
        let err = send_raw_request(
            &uri,
            &http::Method::POST,
            &sample_headers(&addr.to_string()),
            &sample_cases(),
            b"{}",
            None,
            CONNECT_TIMEOUT,
        )
        .await
        .err()
        .expect("connect must fail");
        assert!(
            matches!(err, RawSendError::BeforeWrite(ProxyError::ForwardFailed(_))),
            "{err:?}"
        );
        assert!(should_fall_back(&err, false));
        // A configured upstream proxy must never be bypassed by the fallback.
        assert!(!should_fall_back(&err, true));
    }

    #[test]
    fn connect_timeout_does_not_fall_back() {
        let err = RawSendError::BeforeWrite(ProxyError::Timeout("TCP connect".into()));
        assert!(!should_fall_back(&err, false));
    }

    #[tokio::test]
    async fn connect_timeout_bounds_a_hanging_connect() {
        let started = std::time::Instant::now();
        let res: Result<(), ProxyError> = with_connect_timeout(
            std::time::Duration::from_millis(50),
            "TCP connect",
            futures::future::pending(),
        )
        .await;
        assert!(matches!(res, Err(ProxyError::Timeout(_))), "{res:?}");
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
    }

    #[tokio::test]
    async fn unroutable_upstream_fails_within_connect_timeout() {
        // 10.255.255.1 normally blackholes SYNs; in sandboxes it may instead fail
        // immediately. Either way the raw path must give up well before the
        // overall request timeout and without falling back.
        let uri: http::Uri = "http://10.255.255.1:81/".parse().unwrap();
        let mut ext = http::Extensions::new();
        ext.insert(sample_cases());
        let started = std::time::Instant::now();
        let res = send_request(
            uri,
            http::Method::POST,
            sample_headers("10.255.255.1:81"),
            ext,
            b"{}".to_vec(),
            std::time::Duration::from_secs(60),
            None,
        )
        .await;
        assert!(res.is_err());
        assert!(
            started.elapsed() < CONNECT_TIMEOUT * 2 + std::time::Duration::from_secs(2),
            "took {:?}",
            started.elapsed()
        );
    }

    #[test]
    fn json_content_type_detection_accepts_json_suffixes() {
        assert!(buffered_with_content_type(Some("application/json; charset=utf-8")).is_json());
        assert!(buffered_with_content_type(Some("application/problem+json")).is_json());
        assert!(!buffered_with_content_type(Some("text/event-stream")).is_json());
        assert!(!buffered_with_content_type(None).is_json());
    }
}
