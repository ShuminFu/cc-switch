//! Access control for the local proxy listener (SEC-01 / PROXY-03 / PRODUCT-02 / SEC-14 / SEC-15).
//!
//! Policy, applied to every request before any handler reads the body:
//!
//! 1. Browser cross-site requests are refused: an `Origin` header that names a
//!    non-local web origin (or the opaque `null` origin) gets `403`, even with a
//!    valid token. CLIs and SDKs never send `Origin`.
//! 2. `/health` is always reachable.
//! 3. `/claude-desktop/*` keeps its own gateway bearer-token check in the handler.
//! 4. A request is *local* when the TCP peer is the local machine (loopback, or the
//!    same interface address the connection was accepted on) **and** its `Host`
//!    header is `localhost` / an IP literal. Local requests behave as before, so
//!    loopback binds and live-config takeover keep working without any setup.
//!    A non-local hostname in `Host` (DNS rebinding) is refused with `403`.
//! 5. Every other request (another host on the LAN when bound to a non-loopback
//!    address, or a rebinding hostname) must present the per-install access token in
//!    `Authorization: Bearer <token>`, `x-api-key`, `x-goog-api-key`,
//!    `x-cc-switch-token` or Gemini's `?key=` query parameter; otherwise `401`.
//!    The token is compared in constant time. The dedicated `x-cc-switch-token`
//!    header and a matching `?key=` parameter are stripped before forwarding so the
//!    token never reaches an upstream provider through them.
//! 6. Request bodies are capped at [`MAX_REQUEST_BODY_BYTES`].

use axum::{
    body::Body,
    extract::{Request, State},
    http::{header, HeaderMap, HeaderValue, StatusCode, Uri},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

/// Maximum accepted request body size.
///
/// Upstream APIs reject far smaller bodies (Anthropic: 32 MB), so 64 MiB keeps
/// headroom for large image/PDF payloads while bounding memory per request.
pub const MAX_REQUEST_BODY_BYTES: usize = 64 * 1024 * 1024;

/// Dedicated header clients may use to send the access token when their
/// `Authorization` header must carry something else (e.g. Codex ChatGPT login).
pub const ACCESS_TOKEN_HEADER: &str = "x-cc-switch-token";

/// Connection endpoints of the TCP stream a request arrived on.
#[derive(Debug, Clone, Copy)]
pub struct PeerInfo {
    pub remote: SocketAddr,
    pub local: SocketAddr,
}

/// Marker inserted into request extensions describing how the request was admitted.
#[derive(Debug, Clone, Copy, Default)]
pub struct AccessContext {
    /// The request presented a valid access token.
    pub token_authenticated: bool,
}

#[derive(Clone)]
pub struct AccessGuard {
    inner: Arc<Inner>,
}

struct Inner {
    token: String,
    max_body_bytes: usize,
}

impl AccessGuard {
    pub fn new(token: String) -> Self {
        Self::with_body_limit(token, MAX_REQUEST_BODY_BYTES)
    }

    pub fn with_body_limit(token: String, max_body_bytes: usize) -> Self {
        Self {
            inner: Arc::new(Inner {
                token,
                max_body_bytes,
            }),
        }
    }

    fn token_matches(&self, candidate: &str) -> bool {
        !self.inner.token.is_empty()
            && constant_time_eq(candidate.trim().as_bytes(), self.inner.token.as_bytes())
    }
}

/// Constant-time byte comparison (only the length is observable).
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    std::hint::black_box(diff) == 0
}

fn is_local_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_loopback(),
        IpAddr::V6(v6) => {
            v6.is_loopback() || v6.to_ipv4_mapped().is_some_and(|v4| v4.is_loopback())
        }
    }
}

fn canonical_ip(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => v6
            .to_ipv4_mapped()
            .map(IpAddr::V4)
            .unwrap_or(IpAddr::V6(v6)),
        v4 => v4,
    }
}

/// The TCP peer is this machine: loopback, or the address of the interface the
/// connection was accepted on (a remote host cannot complete a handshake with that).
fn peer_is_local(peer: Option<&PeerInfo>) -> bool {
    let Some(peer) = peer else {
        return false;
    };
    let remote = canonical_ip(peer.remote.ip());
    is_local_ip(remote) || remote == canonical_ip(peer.local.ip())
}

/// Strip an optional `:port` and IPv6 brackets from a Host/authority value.
fn host_without_port(authority: &str) -> &str {
    let authority = authority.trim();
    if let Some(rest) = authority.strip_prefix('[') {
        return rest.split(']').next().unwrap_or("");
    }
    match authority.rsplit_once(':') {
        // Exactly one colon → host:port. More colons → bare IPv6 literal.
        Some((host, port)) if !host.contains(':') && port.chars().all(|c| c.is_ascii_digit()) => {
            host
        }
        _ => authority,
    }
}

fn is_localhost_name(host: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    host == "localhost" || host.ends_with(".localhost")
}

/// `Host` values that cannot be the result of DNS rebinding: localhost names and
/// IP literals (a browser only sends an IP literal when the URL itself used it).
fn host_is_safe(host_header: Option<&HeaderValue>) -> bool {
    let Some(value) = host_header else {
        // HTTP/1.0 style clients without Host are not browsers.
        return true;
    };
    let Ok(value) = value.to_str() else {
        return false;
    };
    let host = host_without_port(value);
    if host.is_empty() {
        return false;
    }
    is_localhost_name(host) || host.parse::<IpAddr>().is_ok()
}

/// Returns false when the `Origin` header identifies a web page that is not served
/// from the local machine (the browser CSRF vector), or the opaque `null` origin.
fn origin_is_allowed(headers: &HeaderMap) -> bool {
    let Some(value) = headers.get(header::ORIGIN) else {
        return true;
    };
    let Ok(origin) = value.to_str() else {
        return false;
    };
    let origin = origin.trim();
    if origin.eq_ignore_ascii_case("null") || origin.is_empty() {
        return false;
    }
    let Some((scheme, rest)) = origin.split_once("://") else {
        return false;
    };
    if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
        // Non-web schemes (vscode-webview://, tauri://, chrome-extension://...) come
        // from locally installed apps/extensions, not from arbitrary websites.
        return true;
    }
    let authority = rest.split('/').next().unwrap_or("");
    let host = host_without_port(authority);
    if is_localhost_name(host) {
        return true;
    }
    host.parse::<IpAddr>().is_ok_and(is_local_ip)
}

fn bearer_token(value: &str) -> &str {
    let value = value.trim();
    if value.len() > 7 && value[..7].eq_ignore_ascii_case("bearer ") {
        value[7..].trim()
    } else {
        value
    }
}

/// Look for a token in the headers / query string. Returns whether it matched and
/// whether the `?key=` query parameter carried it (so it can be stripped).
fn find_token(guard: &AccessGuard, headers: &HeaderMap, uri: &Uri) -> (bool, bool) {
    for name in [
        header::AUTHORIZATION.as_str(),
        "x-api-key",
        "x-goog-api-key",
        ACCESS_TOKEN_HEADER,
    ] {
        for value in headers.get_all(name) {
            if let Ok(value) = value.to_str() {
                if guard.token_matches(bearer_token(value)) {
                    return (true, false);
                }
            }
        }
    }
    if let Some(query) = uri.query() {
        for pair in query.split('&') {
            if let Some(value) = pair.strip_prefix("key=") {
                let decoded = percent_decode(value);
                if guard.token_matches(&decoded) {
                    return (true, true);
                }
            }
        }
    }
    (false, false)
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
                if let Some(b) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                    out.push(b);
                    i += 3;
                    continue;
                }
                out.push(b'%');
            }
            b'+' => out.push(b' '),
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn strip_key_query(uri: &Uri) -> Option<Uri> {
    let query = uri.query()?;
    let kept: Vec<&str> = query
        .split('&')
        .filter(|pair| !pair.starts_with("key="))
        .collect();
    let path = uri.path();
    let path_and_query = if kept.is_empty() {
        path.to_string()
    } else {
        format!("{path}?{}", kept.join("&"))
    };
    let mut parts = uri.clone().into_parts();
    parts.path_and_query = path_and_query.parse().ok();
    Uri::from_parts(parts).ok()
}

fn reject(status: StatusCode, error_type: &str, message: &str) -> Response {
    (
        status,
        Json(json!({
            "error": {
                "type": error_type,
                "message": message,
            }
        })),
    )
        .into_response()
}

/// Axum middleware enforcing the policy described in the module docs.
pub async fn enforce(State(guard): State<AccessGuard>, mut req: Request, next: Next) -> Response {
    let path = req.uri().path().to_string();

    if !origin_is_allowed(req.headers()) {
        log::warn!("[ProxyAccess] rejected cross-site request to {path} (Origin not local)");
        return reject(
            StatusCode::FORBIDDEN,
            "permission_error",
            "Cross-origin browser requests are not allowed by the CC Switch proxy",
        );
    }

    // SEC-14: refuse oversized bodies before anything buffers them.
    let max = guard.inner.max_body_bytes;
    if let Some(len) = req
        .headers()
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse::<u64>().ok())
    {
        if len > max as u64 {
            return reject(
                StatusCode::PAYLOAD_TOO_LARGE,
                "request_too_large",
                "Request body exceeds the CC Switch proxy limit",
            );
        }
    }

    let (token_ok, token_in_query) = find_token(&guard, req.headers(), req.uri());

    let exempt = path == "/health" || path.starts_with("/claude-desktop/");
    if !exempt && !token_ok {
        let peer = req.extensions().get::<PeerInfo>().copied();
        let local_peer = peer_is_local(peer.as_ref());
        let host_ok = host_is_safe(req.headers().get(header::HOST));
        if !(local_peer && host_ok) {
            if local_peer {
                log::warn!("[ProxyAccess] rejected request to {path}: Host header is not local");
                return reject(
                    StatusCode::FORBIDDEN,
                    "permission_error",
                    "Host header is not allowed; use 127.0.0.1 or localhost, or send the CC Switch access token",
                );
            }
            log::warn!(
                "[ProxyAccess] rejected unauthenticated request to {path} from {}",
                peer.map(|p| p.remote.to_string())
                    .unwrap_or_else(|| "unknown".to_string())
            );
            return reject(
                StatusCode::UNAUTHORIZED,
                "authentication_error",
                "CC Switch proxy access token required (Authorization: Bearer <token>)",
            );
        }
    }

    if token_ok {
        req.headers_mut().remove(ACCESS_TOKEN_HEADER);
        if token_in_query {
            if let Some(uri) = strip_key_query(req.uri()) {
                *req.uri_mut() = uri;
            }
        }
    }
    req.extensions_mut().insert(AccessContext {
        token_authenticated: token_ok,
    });

    let (parts, body) = req.into_parts();
    let body = Body::new(http_body_util::Limited::new(body, max));
    next.run(Request::from_parts(parts, body)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{routing::get, routing::post, Extension, Router};
    use http_body_util::BodyExt;

    /// Drive the router directly (axum's `Router` is always ready).
    async fn call(mut router: Router, req: Request) -> Response {
        match <Router as tower::Service<Request>>::call(&mut router, req).await {
            Ok(resp) => resp,
            Err(never) => match never {},
        }
    }

    const TOKEN: &str = "ccsp-test-token-0123456789abcdef";

    fn app_with_limit(limit: usize) -> Router {
        let guard = AccessGuard::with_body_limit(TOKEN.to_string(), limit);
        Router::new()
            .route("/health", get(|| async { "ok" }))
            .route(
                "/v1/messages",
                post(|req: Request| async move {
                    let seen_header = req.headers().contains_key(ACCESS_TOKEN_HEADER);
                    let query = req.uri().query().unwrap_or("").to_string();
                    let ctx = req.extensions().get::<AccessContext>().copied();
                    match req.into_body().collect().await {
                        Ok(b) => format!(
                            "len={} hdr={} q={} auth={}",
                            b.to_bytes().len(),
                            seen_header,
                            query,
                            ctx.map(|c| c.token_authenticated).unwrap_or(false)
                        )
                        .into_response(),
                        Err(_) => StatusCode::PAYLOAD_TOO_LARGE.into_response(),
                    }
                }),
            )
            .route(
                "/status",
                get(|Extension(ctx): Extension<AccessContext>| async move {
                    format!("auth={}", ctx.token_authenticated)
                }),
            )
            .layer(axum::middleware::from_fn_with_state(guard, enforce))
    }

    fn app() -> Router {
        app_with_limit(1024)
    }

    fn peer(remote: &str, local: &str) -> PeerInfo {
        PeerInfo {
            remote: remote.parse().unwrap(),
            local: local.parse().unwrap(),
        }
    }

    fn loopback() -> PeerInfo {
        peer("127.0.0.1:50000", "127.0.0.1:15721")
    }

    fn lan() -> PeerInfo {
        peer("192.168.1.20:50000", "192.168.1.5:15721")
    }

    fn request(method: &str, uri: &str, peer: PeerInfo, headers: &[(&str, &str)]) -> Request {
        let mut builder = http::Request::builder().method(method).uri(uri);
        for (k, v) in headers {
            builder = builder.header(*k, *v);
        }
        let mut req = builder.body(Body::from("{}")).unwrap();
        req.extensions_mut().insert(peer);
        req
    }

    async fn send(req: Request) -> (StatusCode, String) {
        let resp = call(app(), req).await;
        let status = resp.status();
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        (status, String::from_utf8_lossy(&body).into_owned())
    }

    #[test]
    fn constant_time_eq_works() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"abcd"));
    }

    #[test]
    fn host_parsing() {
        assert_eq!(host_without_port("127.0.0.1:15721"), "127.0.0.1");
        assert_eq!(host_without_port("[::1]:15721"), "::1");
        assert_eq!(host_without_port("::1"), "::1");
        assert_eq!(host_without_port("localhost"), "localhost");
    }

    #[tokio::test]
    async fn loopback_request_without_token_is_allowed() {
        let (status, _) = send(request(
            "POST",
            "/v1/messages",
            loopback(),
            &[("host", "127.0.0.1:15721"), ("x-api-key", "PROXY_MANAGED")],
        ))
        .await;
        assert_eq!(status, StatusCode::OK);

        let (status, _) = send(request(
            "POST",
            "/v1/messages",
            loopback(),
            &[("host", "localhost:15721")],
        ))
        .await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn ipv4_mapped_loopback_peer_is_local() {
        let (status, _) = send(request(
            "POST",
            "/v1/messages",
            peer("[::ffff:127.0.0.1]:50000", "[::]:15721"),
            &[("host", "127.0.0.1:15721")],
        ))
        .await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn same_machine_via_lan_interface_is_local() {
        let (status, _) = send(request(
            "POST",
            "/v1/messages",
            peer("192.168.1.5:50000", "192.168.1.5:15721"),
            &[("host", "192.168.1.5:15721")],
        ))
        .await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn dns_rebinding_host_is_rejected() {
        let (status, _) = send(request(
            "POST",
            "/v1/messages",
            loopback(),
            &[("host", "evil.example:15721")],
        ))
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);

        // A valid token overrides the Host check (e.g. local reverse proxies).
        let auth = format!("Bearer {TOKEN}");
        let (status, _) = send(request(
            "POST",
            "/v1/messages",
            loopback(),
            &[("host", "evil.example:15721"), ("authorization", &auth)],
        ))
        .await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn cross_site_origin_is_rejected_even_with_token() {
        let auth = format!("Bearer {TOKEN}");
        for origin in ["https://evil.example", "null", "http://192.168.1.66:3000"] {
            let (status, _) = send(request(
                "POST",
                "/v1/messages",
                loopback(),
                &[
                    ("host", "127.0.0.1:15721"),
                    ("origin", origin),
                    ("authorization", &auth),
                ],
            ))
            .await;
            assert_eq!(status, StatusCode::FORBIDDEN, "origin {origin}");
        }
    }

    #[tokio::test]
    async fn local_origins_are_allowed() {
        for origin in [
            "http://localhost:5173",
            "http://127.0.0.1:3000",
            "http://[::1]:8080",
            "vscode-webview://abc",
        ] {
            let (status, _) = send(request(
                "POST",
                "/v1/messages",
                loopback(),
                &[("host", "127.0.0.1:15721"), ("origin", origin)],
            ))
            .await;
            assert_eq!(status, StatusCode::OK, "origin {origin}");
        }
    }

    #[tokio::test]
    async fn remote_peer_requires_token() {
        let (status, body) = send(request(
            "POST",
            "/v1/messages",
            lan(),
            &[
                ("host", "192.168.1.5:15721"),
                ("x-api-key", "PROXY_MANAGED"),
            ],
        ))
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert!(body.contains("authentication_error"));

        let (status, _) = send(request(
            "POST",
            "/v1/messages",
            lan(),
            &[
                ("host", "192.168.1.5:15721"),
                ("authorization", "Bearer wrong"),
            ],
        ))
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn missing_peer_info_fails_closed() {
        let mut req = request("POST", "/v1/messages", loopback(), &[]);
        req.extensions_mut().remove::<PeerInfo>();
        let (status, _) = send(req).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn remote_peer_with_token_in_any_supported_place_is_allowed() {
        let bearer = format!("Bearer {TOKEN}");
        let cases: Vec<(&str, &str)> = vec![
            ("authorization", bearer.as_str()),
            ("x-api-key", TOKEN),
            ("x-goog-api-key", TOKEN),
            (ACCESS_TOKEN_HEADER, TOKEN),
        ];
        for (name, value) in cases {
            let (status, body) = send(request(
                "POST",
                "/v1/messages",
                lan(),
                &[("host", "192.168.1.5:15721"), (name, value)],
            ))
            .await;
            assert_eq!(status, StatusCode::OK, "header {name}");
            assert!(body.contains("hdr=false"), "token header must be stripped");
            assert!(body.contains("auth=true"));
        }

        let uri = format!("/v1/messages?alt=sse&key={TOKEN}");
        let (status, body) = send(request("POST", &uri, lan(), &[])).await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("q=alt=sse "), "key must be stripped: {body}");
    }

    #[tokio::test]
    async fn health_is_always_reachable() {
        let (status, _) = send(request("GET", "/health", lan(), &[])).await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn status_marks_token_authentication() {
        let (status, body) = send(request(
            "GET",
            "/status",
            loopback(),
            &[("host", "127.0.0.1:15721")],
        ))
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, "auth=false");

        let (_, body) = send(request(
            "GET",
            "/status",
            loopback(),
            &[("authorization", &format!("Bearer {TOKEN}"))],
        ))
        .await;
        assert_eq!(body, "auth=true");
    }

    #[tokio::test]
    async fn oversized_body_is_rejected() {
        let mut req = http::Request::builder()
            .method("POST")
            .uri("/v1/messages")
            .header("content-length", "4096")
            .body(Body::from(vec![b'a'; 4096]))
            .unwrap();
        req.extensions_mut().insert(loopback());
        let resp = call(app_with_limit(1024), req).await;
        assert_eq!(resp.status(), StatusCode::PAYLOAD_TOO_LARGE);

        // Without Content-Length (chunked) the limited body errors while reading.
        let stream = futures_util_stream(vec![b'a'; 4096]);
        let mut req = http::Request::builder()
            .method("POST")
            .uri("/v1/messages")
            .body(stream)
            .unwrap();
        req.extensions_mut().insert(loopback());
        let resp = call(app_with_limit(1024), req).await;
        assert_eq!(resp.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }

    /// Body without a known size (no Content-Length).
    fn futures_util_stream(data: Vec<u8>) -> Body {
        let stream = futures::stream::iter(vec![Ok::<_, std::io::Error>(bytes::Bytes::from(data))]);
        Body::from_stream(stream)
    }
}
