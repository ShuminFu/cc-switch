//! Browser-origin guard for the local proxy.
//!
//! The proxy forwards requests upstream using the user's stored API keys and
//! has no client authentication of its own (CLI clients send placeholder
//! tokens). Without a guard, any web page open in the user's browser could:
//!
//! * fire a CORS "simple request" (e.g. `POST /v1/messages` with
//!   `Content-Type: text/plain`) — the handlers read the raw body regardless of
//!   content type, so the request is forwarded and billed even though the page
//!   cannot read the response;
//! * use DNS rebinding (`attacker.example` → `127.0.0.1`) to become
//!   same-origin with the proxy and read responses as well.
//!
//! Legitimate clients (Claude Code, Codex, Gemini CLI, SDKs) are not browsers:
//! they send no `Origin` header and address the proxy by a loopback host or an
//! IP literal. This guard therefore rejects requests that carry a non-local
//! web `Origin`, and — when the proxy listens on loopback only — requests whose
//! `Host` is a DNS name other than a local alias.

use super::client_auth::{self, requires_client_auth, ConnAddrs};
use super::server::ProxyState;
use axum::{
    extract::{Request, State},
    http::{header, HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use std::net::IpAddr;

/// Host names (without port) that are always considered local.
const LOCAL_HOST_ALIASES: &[&str] = &[
    "localhost",
    "host.docker.internal",
    "gateway.docker.internal",
    "host.containers.internal",
];

/// Strip an optional `:port` suffix and IPv6 brackets from a Host/authority.
fn host_without_port(authority: &str) -> &str {
    let authority = authority.trim();
    if let Some(rest) = authority.strip_prefix('[') {
        // [::1]:8080 or [::1]
        return rest.split(']').next().unwrap_or(rest);
    }
    // Bare IPv6 without brackets (not valid in Host, but be lenient).
    if authority.matches(':').count() > 1 {
        return authority;
    }
    authority.split(':').next().unwrap_or(authority)
}

fn is_loopback_name(host: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    if host == "localhost" || host.ends_with(".localhost") {
        return true;
    }
    host.parse::<IpAddr>()
        .map(|ip| ip.is_loopback())
        .unwrap_or(false)
}

/// Whether a `Host` header value is acceptable. IP literals can never be the
/// result of DNS rebinding, so any IP literal is accepted; DNS names must be a
/// known local alias.
pub(crate) fn is_allowed_host(host_header: &str) -> bool {
    let host = host_without_port(host_header)
        .trim_end_matches('.')
        .to_ascii_lowercase();
    if host.is_empty() {
        return false;
    }
    if host.parse::<IpAddr>().is_ok() || is_loopback_name(&host) {
        return true;
    }
    LOCAL_HOST_ALIASES.contains(&host.as_str())
}

/// Whether an `Origin` header value is acceptable. Browsers only ever emit
/// `http(s)://…` or `null` origins for web content, so those are restricted
/// to loopback hosts; other schemes (editor webviews, app shells) are allowed
/// to keep non-browser embedders working.
pub(crate) fn is_allowed_origin(origin: &str) -> bool {
    let origin = origin.trim();
    if origin.eq_ignore_ascii_case("null") {
        return false;
    }
    let lower = origin.to_ascii_lowercase();
    let rest = if let Some(r) = lower.strip_prefix("http://") {
        r
    } else if let Some(r) = lower.strip_prefix("https://") {
        r
    } else {
        return true;
    };
    let authority = rest.split('/').next().unwrap_or(rest);
    is_loopback_name(host_without_port(authority))
}

fn listen_is_loopback(listen_address: &str) -> bool {
    is_loopback_name(
        listen_address
            .trim()
            .trim_start_matches('[')
            .trim_end_matches(']'),
    )
}

/// Pure decision function, separated for unit testing.
pub(crate) fn check_request(headers: &HeaderMap, listen_address: &str) -> Result<(), &'static str> {
    if let Some(origin) = headers.get(header::ORIGIN) {
        let ok = origin.to_str().map(is_allowed_origin).unwrap_or(false);
        if !ok {
            return Err("cross-origin browser requests are not allowed");
        }
    }

    // When the user deliberately exposes the proxy on a non-loopback address
    // they may reach it via arbitrary LAN host names, so only enforce the Host
    // allowlist for the (default) loopback binding.
    if listen_is_loopback(listen_address) {
        if let Some(host) = headers.get(header::HOST) {
            let ok = host.to_str().map(is_allowed_host).unwrap_or(false);
            if !ok {
                return Err("unexpected Host header");
            }
        }
    }
    Ok(())
}

pub(crate) async fn guard(State(state): State<ProxyState>, req: Request, next: Next) -> Response {
    let listen_address = state.config.read().await.listen_address.clone();
    if let Err(reason) = check_request(req.headers(), &listen_address) {
        log::warn!(
            "[ProxyGuard] rejected {} {}: {reason}",
            req.method(),
            req.uri().path()
        );
        return (StatusCode::FORBIDDEN, reason).into_response();
    }

    // Exposed (non-loopback) proxy: remote peers must present the proxy client
    // token, which is scrubbed before the request reaches the handlers.
    let conn = req.extensions().get::<ConnAddrs>().copied();
    if requires_client_auth(&listen_address, conn.as_ref(), req.uri().path()) {
        let token = match client_auth::get_or_create_proxy_client_token(&state.db) {
            Ok(token) => token,
            Err(e) => {
                log::error!("[ProxyGuard] failed to load proxy client token: {e}");
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "proxy client token unavailable",
                )
                    .into_response();
            }
        };
        let (mut parts, body) = req.into_parts();
        if !client_auth::verify_and_scrub(&mut parts, &token) {
            log::warn!(
                "[ProxyGuard] rejected unauthenticated {} {} from {}",
                parts.method,
                parts.uri.path(),
                conn.map(|c| c.peer.ip().to_string())
                    .unwrap_or_else(|| "unknown".to_string())
            );
            return (
                StatusCode::UNAUTHORIZED,
                [(header::WWW_AUTHENTICATE, "Bearer")],
                "proxy client token required: this CC Switch proxy is exposed on the network; \
                 send the token shown in CC Switch proxy settings as your API key",
            )
                .into_response();
        }
        return next.run(Request::from_parts(parts, body)).await;
    }

    next.run(req).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn headers(pairs: &[(&'static str, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            h.insert(*k, HeaderValue::from_str(v).unwrap());
        }
        h
    }

    #[test]
    fn allows_cli_style_requests() {
        for host in [
            "127.0.0.1:15721",
            "localhost:15721",
            "LOCALHOST",
            "[::1]:15721",
            "192.168.1.10:15721",
            "host.docker.internal:15721",
            "foo.localhost:1",
        ] {
            assert!(
                check_request(&headers(&[("host", host)]), "127.0.0.1").is_ok(),
                "{host}"
            );
        }
        assert!(check_request(&HeaderMap::new(), "127.0.0.1").is_ok());
    }

    #[test]
    fn rejects_dns_rebinding_hosts_on_loopback_binding() {
        for host in ["attacker.example:15721", "evil.com", "127.0.0.1.nip.io"] {
            assert!(
                check_request(&headers(&[("host", host)]), "127.0.0.1").is_err(),
                "{host}"
            );
        }
    }

    #[test]
    fn host_check_skipped_when_listening_on_all_interfaces() {
        assert!(check_request(&headers(&[("host", "mypc.lan:15721")]), "0.0.0.0").is_ok());
    }

    #[test]
    fn rejects_cross_origin_browser_requests() {
        for origin in ["https://evil.com", "http://attacker.example:8080", "null"] {
            let h = headers(&[("host", "127.0.0.1:15721"), ("origin", origin)]);
            assert!(check_request(&h, "127.0.0.1").is_err(), "{origin}");
            // Origin check also applies to non-loopback bindings.
            assert!(check_request(&h, "0.0.0.0").is_err(), "{origin}");
        }
    }

    #[test]
    fn allows_local_and_non_web_origins() {
        for origin in [
            "http://localhost:3000",
            "http://127.0.0.1:5173",
            "http://[::1]:8080",
            "tauri://localhost",
            "vscode-webview://abc",
        ] {
            let h = headers(&[("host", "127.0.0.1:15721"), ("origin", origin)]);
            assert!(check_request(&h, "127.0.0.1").is_ok(), "{origin}");
        }
    }
}
