//! Hyper-based HTTP client for proxy forwarding
//!
//! Uses raw TCP/TLS writes to preserve exact original header name casing.
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
/// Populated per request in `server.rs` from the raw bytes hyper reads (see
/// `header_case::RecordingIo`).
/// Used in `send_request` to manually write headers with original casing when hyper's
/// own mechanism fails.
#[derive(Clone, Debug, Default)]
pub(crate) struct OriginalHeaderCases {
    /// Ordered list of (lowercase_name, original_wire_bytes) pairs.
    /// Multiple entries with the same name are allowed (for repeated headers).
    pub cases: Vec<(String, Vec<u8>)>,
}

impl OriginalHeaderCases {
    /// Parse raw HTTP request bytes to extract original header casings (test helper;
    /// the server records casing per request through `header_case::RecordingIo`).
    #[cfg(test)]
    pub fn from_raw_bytes(buf: &[u8]) -> Self {
        // Every header occupies at least one line, so this never runs out.
        let line_count = buf.iter().filter(|&&b| b == b'\n').count();
        let mut headers_buf = vec![httparse::EMPTY_HEADER; line_count];
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

/// Lazily-initialized hyper client with header-case preservation enabled.
fn global_hyper_client() -> &'static HyperClient {
    static CLIENT: OnceLock<HyperClient> = OnceLock::new();
    CLIENT.get_or_init(|| {
        let connector = HttpsConnectorBuilder::new()
            .with_webpki_roots()
            .https_or_http()
            .enable_http1()
            .build();

        Client::builder(TokioExecutor::new())
            .http1_preserve_header_case(true)
            .http1_title_case_headers(true)
            .build(connector)
    })
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
///    (from `OriginalHeaderCases` recorded per request in server.rs), then hand off to
///    hyper for response parsing.
/// 2. Fallback: hyper-util Client with `title_case_headers(true)` when raw write
///    isn't feasible (e.g., missing original cases).
///
/// The fallback only runs when the raw path failed before any request bytes
/// were written (see [`RawSendError`]); once the request may have reached the
/// upstream, the error is returned as-is so the request is never sent twice.
/// Both attempts share a single `timeout` deadline.
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
        proxy_url,
    );

    // One deadline shared by the raw attempt and the fallback.
    let deadline = tokio::time::Instant::now() + timeout;

    if let Some(original_cases) = original_cases
        .as_ref()
        .filter(|cases| !cases.cases.is_empty())
    {
        // Primary path: use raw write + hyper handshake for exact header casing
        let result = tokio::time::timeout_at(
            deadline,
            send_raw_request(&uri, &method, &headers, original_cases, &body, proxy_url),
        )
        .await
        .map_err(|_| ProxyError::Timeout(format!("请求超时: {}s", timeout.as_secs())))?;

        match result {
            Ok(resp) => return Ok(resp),
            Err(RawSendError::PostWrite(e)) => {
                // The request may already have reached the upstream; resending it
                // could bill the same LLM call twice. Let the forwarder decide.
                return Err(e);
            }
            Err(RawSendError::PreWrite(e)) => {
                if proxy_url.is_some() {
                    // Don't bypass configured proxy with direct connect fallback
                    return Err(e);
                }
                log::warn!("[HyperClient] Raw write failed, falling back to hyper-util: {e}");
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
    let resp = tokio::time::timeout_at(deadline, client.request(req))
        .await
        .map_err(|_| ProxyError::Timeout(format!("请求超时: {}s", timeout.as_secs())))?
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

/// Failure of the raw header-case path, split by whether any request bytes
/// may have been written to the upstream.
#[derive(Debug)]
enum RawSendError {
    /// Nothing was sent (URI/host, connect, server name, TLS handshake); a
    /// fallback attempt cannot duplicate the request.
    PreWrite(ProxyError),
    /// The request may have been (partially) sent; it must not be resent here.
    PostWrite(ProxyError),
}

/// Send request via raw TCP/TLS with exact original header casing.
///
/// When `proxy_url` is provided, establishes an HTTP CONNECT tunnel through
/// the proxy first, then performs TLS + raw write through the tunnel.
/// This preserves header casing even when an upstream proxy is configured.
async fn send_raw_request(
    uri: &http::Uri,
    method: &http::Method,
    headers: &http::HeaderMap,
    original_cases: &OriginalHeaderCases,
    body: &[u8],
    proxy_url: Option<&str>,
) -> Result<ProxyResponse, RawSendError> {
    use tokio::io::AsyncWriteExt;

    let pre = |msg: String| RawSendError::PreWrite(ProxyError::ForwardFailed(msg));
    let post = |msg: String| RawSendError::PostWrite(ProxyError::ForwardFailed(msg));

    let scheme = uri.scheme_str().unwrap_or("https");
    let host = uri.host().ok_or_else(|| pre("URI has no host".into()))?;
    let port = uri
        .port_u16()
        .unwrap_or(if scheme == "https" { 443 } else { 80 });
    let path_and_query = uri.path_and_query().map(|pq| pq.as_str()).unwrap_or("/");

    // Build raw HTTP request bytes
    let raw = build_raw_request(method, path_and_query, headers, original_cases, body);

    // Establish TCP connection — either direct or through HTTP CONNECT proxy
    let stream = if let Some(proxy) = proxy_url {
        connect_via_proxy(proxy, host, port)
            .await
            .map_err(RawSendError::PreWrite)?
    } else {
        ProxyStream::Tcp(
            tcp_connect(host, port)
                .await
                .map_err(|e| pre(format!("TCP connect failed: {e}")))?,
        )
    };

    if scheme == "https" {
        let tls_connector = global_tls_connector();
        let server_name =
            tls_server_name(host).map_err(|e| pre(format!("Invalid server name: {e}")))?;
        let mut tls_stream = tls_connector
            .connect(server_name, stream)
            .await
            .map_err(|e| pre(format!("TLS handshake failed: {e}")))?;

        tls_stream
            .write_all(&raw)
            .await
            .map_err(|e| post(format!("Write failed: {e}")))?;
        tls_stream
            .flush()
            .await
            .map_err(|e| post(format!("Flush failed: {e}")))?;

        let filtered = WriteFilter::new(tls_stream);
        do_hyper_response(filtered, method.clone())
            .await
            .map_err(RawSendError::PostWrite)
    } else {
        let mut stream = stream;
        stream
            .write_all(&raw)
            .await
            .map_err(|e| post(format!("Write failed: {e}")))?;
        stream
            .flush()
            .await
            .map_err(|e| post(format!("Flush failed: {e}")))?;

        let filtered = WriteFilter::new(stream);
        do_hyper_response(filtered, method.clone())
            .await
            .map_err(RawSendError::PostWrite)
    }
}

/// Strip the `[...]` brackets that URIs put around IPv6 literals.
///
/// `http::Uri::host` and `url::Url::host_str` both return `"[::1]"`, which
/// neither the socket resolver nor `ServerName` accepts.
fn unbracket_host(host: &str) -> &str {
    host.strip_prefix('[')
        .and_then(|h| h.strip_suffix(']'))
        .unwrap_or(host)
}

/// TCP connect to a URI host, connecting IP literals (bracketed IPv6
/// included) by `SocketAddr` and resolving everything else by name.
async fn tcp_connect(host: &str, port: u16) -> std::io::Result<tokio::net::TcpStream> {
    let host = unbracket_host(host);
    match host.parse::<std::net::IpAddr>() {
        Ok(ip) => tokio::net::TcpStream::connect(std::net::SocketAddr::new(ip, port)).await,
        Err(_) => tokio::net::TcpStream::connect((host, port)).await,
    }
}

/// TLS server name for a URI host: `IpAddress` for IP literals (bracketed
/// IPv6 included), `DnsName` otherwise.
fn tls_server_name(
    host: &str,
) -> Result<rustls::pki_types::ServerName<'static>, rustls::pki_types::InvalidDnsNameError> {
    let host = unbracket_host(host);
    match host.parse::<std::net::IpAddr>() {
        Ok(ip) => Ok(rustls::pki_types::ServerName::IpAddress(ip.into())),
        Err(_) => rustls::pki_types::ServerName::try_from(host.to_string()),
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
    let proxy_auth =
        proxy_auth_header(&parsed).map(|value| format!("Proxy-Authorization: {value}\r\n"));

    // Connect to the proxy
    let tcp = tcp_connect(proxy_host, proxy_port)
        .await
        .map_err(|e| ProxyError::ForwardFailed(format!("Proxy TCP connect failed: {e}")))?;

    // Wrap with TLS if the proxy URL uses https://
    let mut stream: ProxyStream = if parsed.scheme() == "https" {
        let tls_connector = global_tls_connector();
        let server_name = tls_server_name(proxy_host)
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

    // Expect "HTTP/1.x 2xx ..."; the reason phrase is optional.
    match parse_connect_status(&status_line) {
        Some(code) if (200..300).contains(&code) => {}
        Some(407) => {
            return Err(ProxyError::AuthError(format!(
                "Proxy authentication required (407): {}",
                status_line.trim()
            )));
        }
        _ => {
            return Err(ProxyError::ForwardFailed(format!(
                "Proxy CONNECT rejected: {}",
                status_line.trim()
            )));
        }
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

/// Build the `Proxy-Authorization` header value (`Basic ...`) from the proxy
/// URL's userinfo, or `None` when the URL carries no username.
///
/// `Url::username()` / `Url::password()` return the percent-encoded form, so
/// both parts are decoded first; otherwise credentials containing characters
/// such as `@ : # % / ?` or non-ASCII would be sent still encoded.
fn proxy_auth_header(url: &url::Url) -> Option<String> {
    use base64::Engine;
    use percent_encoding::percent_decode_str;

    if url.username().is_empty() {
        return None;
    }
    let mut credentials: Vec<u8> = percent_decode_str(url.username()).collect();
    credentials.push(b':');
    credentials.extend(percent_decode_str(url.password().unwrap_or("")));
    let encoded = base64::engine::general_purpose::STANDARD.encode(credentials);
    Some(format!("Basic {encoded}"))
}

/// Parse the status code from a CONNECT response status line.
///
/// Requires an `HTTP/` version prefix and a 3-digit status code as the
/// second token; the reason phrase is optional. Returns `None` for anything
/// that is not a well-formed status line.
fn parse_connect_status(line: &str) -> Option<u16> {
    let mut parts = line.trim_end_matches(['\r', '\n']).splitn(3, ' ');
    let version = parts.next()?;
    if !version.starts_with("HTTP/") {
        return None;
    }
    let code = parts.next()?;
    if code.len() != 3 || !code.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    code.parse().ok()
}

/// Lazily-initialized TLS connector for raw connections.
///
/// Loads both webpki roots AND native system certificates so that
/// proxy MITM CAs (e.g. Clash, mitmproxy) installed in the system
/// keychain are trusted through the CONNECT tunnel.
fn global_tls_connector() -> &'static tokio_rustls::TlsConnector {
    static CONNECTOR: OnceLock<tokio_rustls::TlsConnector> = OnceLock::new();
    CONNECTOR.get_or_init(|| {
        let mut root_store = rustls::RootCertStore::empty();
        // Baseline: Mozilla/webpki roots
        root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        // Native system certs (includes user-installed proxy CAs)
        let native = rustls_native_certs::load_native_certs();
        let (added, _errors) = root_store.add_parsable_certificates(native.certs);
        log::debug!("[HyperClient] TLS root store: webpki + {added} native certs");
        let config = rustls::ClientConfig::builder()
            .with_root_certificates(root_store)
            .with_no_client_auth();
        tokio_rustls::TlsConnector::from(std::sync::Arc::new(config))
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

/// Use hyper's low-level client to parse the response on a stream where we've
/// already written the request.
///
/// `WriteFilter` discards any writes from hyper (it would try to send its own
/// request encoding), while passing reads through transparently.
async fn do_hyper_response<S>(
    stream: WriteFilter<S>,
    method: http::Method,
) -> Result<ProxyResponse, ProxyError>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let io = hyper_util::rt::TokioIo::new(stream);

    let (mut sender, conn) = hyper::client::conn::http1::Builder::new()
        .preserve_header_case(true)
        .handshake::<_, http_body_util::Full<Bytes>>(io)
        .await
        .map_err(|e| ProxyError::ForwardFailed(format!("Handshake failed: {e}")))?;

    // Spawn the connection driver (reads responses from the stream)
    tokio::spawn(async move {
        if let Err(e) = conn.await {
            log::debug!("[HyperClient] raw conn driver error: {e}");
        }
    });

    // Send a dummy request through hyper — hyper will encode this and try to write it,
    // but WriteFilter discards all writes. Hyper will then read the response from the stream.
    let dummy_req = http::Request::builder()
        .method(method)
        .uri("/")
        .body(http_body_util::Full::new(Bytes::new()))
        .map_err(|e| ProxyError::ForwardFailed(format!("Build dummy request: {e}")))?;

    let resp = sender
        .send_request(dummy_req)
        .await
        .map_err(|e| ProxyError::ForwardFailed(format!("Response parse failed: {e}")))?;

    Ok(ProxyResponse::Hyper(resp))
}

/// A stream wrapper that discards all writes but passes reads through.
///
/// This lets hyper's connection driver think it sent a request (its encoded bytes
/// go to /dev/null), while correctly parsing the response that the upstream server
/// sends in reply to our raw-written request.
struct WriteFilter<S> {
    inner: S,
}

impl<S> WriteFilter<S> {
    fn new(inner: S) -> Self {
        Self { inner }
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

impl<S: Unpin> tokio::io::AsyncWrite for WriteFilter<S> {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        // Discard all writes — pretend they succeeded
        std::task::Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::task::Poll::Ready(Ok(()))
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::task::Poll::Ready(Ok(()))
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

    #[test]
    fn json_content_type_detection_accepts_json_suffixes() {
        assert!(buffered_with_content_type(Some("application/json; charset=utf-8")).is_json());
        assert!(buffered_with_content_type(Some("application/problem+json")).is_json());
        assert!(!buffered_with_content_type(Some("text/event-stream")).is_json());
        assert!(!buffered_with_content_type(None).is_json());
    }

    fn extensions_with_cases() -> http::Extensions {
        // The hyper-util fallback client needs a process-level rustls provider.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let mut ext = http::Extensions::new();
        ext.insert(OriginalHeaderCases::from_raw_bytes(
            b"POST /v1/messages HTTP/1.1\r\nHost: x\r\nX-Custom-Case: 1\r\n\r\n",
        ));
        ext
    }

    fn request_headers(port: u16) -> http::HeaderMap {
        let mut headers = http::HeaderMap::new();
        headers.insert(
            http::header::HOST,
            http::HeaderValue::from_str(&format!("127.0.0.1:{port}")).unwrap(),
        );
        headers.insert("x-custom-case", http::HeaderValue::from_static("1"));
        headers
    }

    /// Read one full HTTP/1.1 request (headers + Content-Length body).
    async fn read_full_request(stream: &mut tokio::net::TcpStream) {
        use tokio::io::AsyncReadExt;
        let mut buf = Vec::new();
        let mut chunk = [0u8; 1024];
        loop {
            let n = stream.read(&mut chunk).await.unwrap_or(0);
            if n == 0 {
                return;
            }
            buf.extend_from_slice(&chunk[..n]);
            if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                let head = String::from_utf8_lossy(&buf[..pos]).to_ascii_lowercase();
                let len = head
                    .lines()
                    .find_map(|l| l.strip_prefix("content-length:"))
                    .and_then(|v| v.trim().parse::<usize>().ok())
                    .unwrap_or(0);
                if buf.len() >= pos + 4 + len {
                    return;
                }
            }
        }
    }

    #[tokio::test]
    async fn raw_path_does_not_resend_after_request_was_written() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;
        use tokio::io::AsyncWriteExt;

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let connections = Arc::new(AtomicUsize::new(0));
        let counter = connections.clone();
        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                counter.fetch_add(1, Ordering::SeqCst);
                tokio::spawn(async move {
                    read_full_request(&mut stream).await;
                    let _ = stream.write_all(b"this is not http\r\n\r\n").await;
                    let _ = stream.shutdown().await;
                });
            }
        });

        let uri: http::Uri = format!("http://127.0.0.1:{port}/v1/messages")
            .parse()
            .unwrap();
        let result = send_request(
            uri,
            http::Method::POST,
            request_headers(port),
            extensions_with_cases(),
            b"{\"model\":\"m\"}".to_vec(),
            std::time::Duration::from_secs(10),
            None,
        )
        .await;

        assert!(
            matches!(result, Err(ProxyError::ForwardFailed(_))),
            "garbage response must surface as ForwardFailed"
        );
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        assert_eq!(
            connections.load(Ordering::SeqCst),
            1,
            "request must not be resent after it was written"
        );
    }

    #[tokio::test]
    async fn raw_path_connect_refused_still_falls_back() {
        // Reserve a port, then free it so connecting is refused.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);

        let uri: http::Uri = format!("http://127.0.0.1:{port}/v1/messages")
            .parse()
            .unwrap();
        let result = send_request(
            uri,
            http::Method::POST,
            request_headers(port),
            extensions_with_cases(),
            Vec::new(),
            std::time::Duration::from_secs(10),
            None,
        )
        .await;

        match result {
            // The hyper-util fallback reports its own error prefix.
            Err(ProxyError::ForwardFailed(msg)) => {
                assert!(
                    msg.contains("上游请求失败"),
                    "expected fallback error, got: {msg}"
                )
            }
            Err(e) => panic!("expected fallback ForwardFailed, got: {e}"),
            Ok(_) => panic!("expected connection error"),
        }
    }

    #[test]
    fn parse_connect_status_reads_second_token() {
        for line in [
            "HTTP/1.1 200 Connection established\r\n",
            "HTTP/1.0 200 OK\r\n",
            "HTTP/1.1 200\r\n",
            "HTTP/1.1 200 \r\n",
        ] {
            assert_eq!(parse_connect_status(line), Some(200), "line: {line:?}");
        }
        assert_eq!(
            parse_connect_status("HTTP/1.1 407 Proxy Authentication Required\r\n"),
            Some(407)
        );
        assert_eq!(parse_connect_status("HTTP/1.1 407\r\n"), Some(407));
        assert_eq!(
            parse_connect_status("HTTP/1.1 502 Upstream 200 failed\r\n"),
            Some(502)
        );
    }

    #[test]
    fn parse_connect_status_rejects_malformed_lines() {
        assert_eq!(parse_connect_status(""), None);
        assert_eq!(parse_connect_status("\r\n"), None);
        assert_eq!(parse_connect_status("garbage 200 ok\r\n"), None);
        assert_eq!(parse_connect_status("HTTP/1.1\r\n"), None);
        assert_eq!(parse_connect_status("HTTP/1.1 20 short\r\n"), None);
        assert_eq!(parse_connect_status("HTTP/1.1 abc\r\n"), None);
    }

    fn auth_header_for(raw: &str) -> Option<String> {
        proxy_auth_header(&url::Url::parse(raw).unwrap())
    }

    fn basic(credentials: &[u8]) -> String {
        use base64::Engine;
        format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode(credentials)
        )
    }

    #[test]
    fn proxy_auth_header_percent_decodes_userinfo() {
        assert_eq!(
            auth_header_for("http://us%40er:p%40ss%3Aw0rd@127.0.0.1:8080"),
            Some(basic(b"us@er:p@ss:w0rd"))
        );
        assert_eq!(
            auth_header_for("http://user:a%23b%25c%2Fd%3Fe@127.0.0.1:8080"),
            Some(basic(b"user:a#b%c/d?e"))
        );
        assert_eq!(
            auth_header_for("http://%E7%94%A8%E6%88%B7:%C3%A9t%C3%A9@127.0.0.1:8080"),
            Some(basic("用户:été".as_bytes()))
        );
    }

    #[test]
    fn proxy_auth_header_handles_missing_parts() {
        assert_eq!(auth_header_for("http://127.0.0.1:8080"), None);
        assert_eq!(
            auth_header_for("http://user@127.0.0.1:8080"),
            Some(basic(b"user:"))
        );
        assert_eq!(
            auth_header_for("http://user:plain@127.0.0.1:8080"),
            Some(basic(b"user:plain"))
        );
    }

    /// Spawn a fake CONNECT proxy that answers with `response` and return its URL.
    async fn fake_connect_proxy(response: &'static str) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 1024];
            let _ = sock.read(&mut buf).await;
            let _ = sock.write_all(response.as_bytes()).await;
            let _ = sock.flush().await;
            // Keep the socket open until the client is done.
            let _ = sock.read(&mut buf).await;
        });
        format!("http://{addr}")
    }

    #[tokio::test]
    async fn connect_via_proxy_accepts_status_without_reason_phrase() {
        let proxy = fake_connect_proxy("HTTP/1.1 200\r\n\r\n").await;
        let result = connect_via_proxy(&proxy, "example.com", 443).await;
        assert!(result.is_ok(), "bare 200 must establish the tunnel");
    }

    #[tokio::test]
    async fn connect_via_proxy_rejects_502_mentioning_200() {
        let proxy = fake_connect_proxy("HTTP/1.1 502 Upstream 200 failed\r\n\r\n").await;
        match connect_via_proxy(&proxy, "example.com", 443).await {
            Err(ProxyError::ForwardFailed(msg)) => assert!(msg.contains("502"), "got: {msg}"),
            Err(e) => panic!("expected ForwardFailed, got: {e}"),
            Ok(_) => panic!("502 must not establish the tunnel"),
        }
    }

    #[tokio::test]
    async fn connect_via_proxy_maps_bare_407_to_auth_error() {
        let proxy = fake_connect_proxy("HTTP/1.1 407\r\n\r\n").await;
        match connect_via_proxy(&proxy, "example.com", 443).await {
            Err(ProxyError::AuthError(_)) => {}
            Err(e) => panic!("expected AuthError, got: {e}"),
            Ok(_) => panic!("407 must not establish the tunnel"),
        }
    }

    /// Bind a listener on `[::1]:0`, or `None` when the host has no IPv6.
    async fn bind_ipv6_loopback() -> Option<tokio::net::TcpListener> {
        match tokio::net::TcpListener::bind("[::1]:0").await {
            Ok(listener) => Some(listener),
            Err(e) => {
                eprintln!("skipping: IPv6 loopback unavailable: {e}");
                None
            }
        }
    }

    #[test]
    fn unbracket_host_strips_only_ipv6_brackets() {
        assert_eq!(unbracket_host("[::1]"), "::1");
        assert_eq!(unbracket_host("[2001:db8::1]"), "2001:db8::1");
        assert_eq!(unbracket_host("127.0.0.1"), "127.0.0.1");
        assert_eq!(unbracket_host("example.com"), "example.com");
        assert_eq!(unbracket_host("[::1"), "[::1");
    }

    #[test]
    fn tls_server_name_uses_ip_address_for_literals() {
        use rustls::pki_types::ServerName;
        let v6: std::net::IpAddr = "::1".parse().unwrap();
        assert_eq!(
            tls_server_name("[::1]").unwrap(),
            ServerName::IpAddress(v6.into())
        );
        let v4: std::net::IpAddr = "127.0.0.1".parse().unwrap();
        assert_eq!(
            tls_server_name("127.0.0.1").unwrap(),
            ServerName::IpAddress(v4.into())
        );
        assert!(matches!(
            tls_server_name("example.com").unwrap(),
            ServerName::DnsName(_)
        ));
        assert!(tls_server_name("not a host").is_err());
    }

    #[tokio::test]
    async fn connect_via_proxy_reaches_ipv6_literal_proxy() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let Some(listener) = bind_ipv6_loopback().await else {
            return;
        };
        let port = listener.local_addr().unwrap().port();
        let (tx, rx) = tokio::sync::oneshot::channel::<String>();
        tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 1024];
            let n = sock.read(&mut buf).await.unwrap_or(0);
            let _ = tx.send(String::from_utf8_lossy(&buf[..n]).into_owned());
            let _ = sock.write_all(b"HTTP/1.1 200 OK\r\n\r\n").await;
            let _ = sock.flush().await;
            let _ = sock.read(&mut buf).await;
        });

        let result = connect_via_proxy(&format!("http://[::1]:{port}"), "[::1]", 8443).await;
        assert!(result.is_ok(), "IPv6-literal proxy must connect");
        let request = rx.await.unwrap();
        assert!(
            request.starts_with("CONNECT [::1]:8443 HTTP/1.1\r\nHost: [::1]:8443\r\n"),
            "CONNECT line and Host header must keep brackets, got: {request:?}"
        );
    }

    #[tokio::test]
    async fn send_raw_request_reaches_ipv6_literal_upstream() {
        use tokio::io::AsyncWriteExt;
        let Some(listener) = bind_ipv6_loopback().await else {
            return;
        };
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            read_full_request(&mut stream).await;
            let _ = stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok")
                .await;
            let _ = stream.flush().await;
        });

        let uri: http::Uri = format!("http://[::1]:{port}/v1/messages").parse().unwrap();
        let mut headers = http::HeaderMap::new();
        headers.insert(
            http::header::HOST,
            http::HeaderValue::from_str(&format!("[::1]:{port}")).unwrap(),
        );
        let cases =
            OriginalHeaderCases::from_raw_bytes(b"POST /v1/messages HTTP/1.1\r\nHost: x\r\n\r\n");
        // Call the raw path directly: success here cannot come from the fallback.
        let result =
            send_raw_request(&uri, &http::Method::POST, &headers, &cases, b"{}", None).await;
        match result {
            Ok(resp) => {
                assert_eq!(resp.status(), http::StatusCode::OK);
                assert_eq!(&resp.bytes().await.unwrap()[..], b"ok");
            }
            Err(e) => panic!("raw path to [::1] must succeed, got: {e:?}"),
        }
    }

    #[tokio::test]
    async fn send_raw_request_through_proxy_builds_tls_name_for_ipv6_target() {
        // IPv4 proxy, IPv6 https target: needs no IPv6 on the host. The fake
        // proxy drops the tunnel after the ClientHello, so the handshake fails,
        // but it must get that far instead of rejecting "[::1]" as a name.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let proxy = fake_connect_proxy("HTTP/1.1 200 OK\r\n\r\n").await;
        let uri: http::Uri = "https://[::1]:8443/v1/messages".parse().unwrap();
        let cases =
            OriginalHeaderCases::from_raw_bytes(b"POST /v1/messages HTTP/1.1\r\nHost: x\r\n\r\n");
        let result = send_raw_request(
            &uri,
            &http::Method::POST,
            &http::HeaderMap::new(),
            &cases,
            b"{}",
            Some(&proxy),
        )
        .await;
        match result {
            Err(RawSendError::PreWrite(ProxyError::ForwardFailed(msg))) => assert!(
                msg.contains("TLS handshake failed"),
                "expected a TLS handshake attempt, got: {msg}"
            ),
            Err(e) => panic!("expected PreWrite TLS handshake failure, got: {e:?}"),
            Ok(_) => panic!("fake proxy cannot complete a TLS handshake"),
        }
    }
}
