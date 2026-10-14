//! Client authentication for the local proxy when it is exposed to the network.
//!
//! The proxy forwards requests upstream with the user's real API keys. On the
//! default loopback binding only local processes can reach it, so no client
//! credential is required (taken-over CLIs send the `PROXY_MANAGED`
//! placeholder). When the user binds it to a non-loopback address (`0.0.0.0`,
//! `::`, a LAN IP), every other device on the network could otherwise spend
//! those keys.
//!
//! Policy: a request must present the per-install proxy client token iff
//! * the proxy listens on a non-loopback address, **and**
//! * the TCP peer is not this machine (peer IP is loopback, or equals the
//!   local address the connection arrived on), **and**
//! * the route is not exempt (`/health`; `/claude-desktop/*`, which enforces
//!   its own gateway token).
//!
//! Local taken-over CLIs therefore keep working unchanged in every mode, and
//! loopback-only setups behave exactly as before.
//!
//! The token is accepted as `Authorization: Bearer <token>`, `x-api-key`,
//! `x-goog-api-key` or a `key` query parameter — i.e. whatever a client sends
//! as its "API key". Once verified, the credential is scrubbed (headers are
//! rewritten to the `PROXY_MANAGED` placeholder, the query parameter is
//! removed) so it can never be forwarded upstream; the forwarder injects the
//! provider's own key as usual.

use crate::database::Database;
use crate::error::AppError;
use http::{header, HeaderValue};
use std::net::{IpAddr, SocketAddr};

/// DB settings key holding the token.
const TOKEN_SETTING_KEY: &str = "proxy_client_token";
/// Value credentials are rewritten to after verification.
const SCRUBBED_PLACEHOLDER: &str = "PROXY_MANAGED";

/// Peer / local socket addresses of the connection a request arrived on.
/// Inserted into request extensions by the accept loop.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ConnAddrs {
    pub peer: SocketAddr,
    pub local: SocketAddr,
}

fn generate_token() -> String {
    format!("ccsp-{}", uuid::Uuid::new_v4().simple())
}

/// Return the proxy client token, creating and persisting one on first use.
pub fn get_or_create_proxy_client_token(db: &Database) -> Result<String, AppError> {
    if let Some(token) = db.get_setting(TOKEN_SETTING_KEY)? {
        let trimmed = token.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_string());
        }
    }
    let token = generate_token();
    db.set_setting(TOKEN_SETTING_KEY, &token)?;
    Ok(token)
}

/// Replace the proxy client token; previously issued tokens stop working on
/// the next request.
pub fn regenerate_proxy_client_token(db: &Database) -> Result<String, AppError> {
    let token = generate_token();
    db.set_setting(TOKEN_SETTING_KEY, &token)?;
    Ok(token)
}

fn canonical(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => v6
            .to_ipv4_mapped()
            .map(IpAddr::V4)
            .unwrap_or(IpAddr::V6(v6)),
        v4 => v4,
    }
}

fn listen_is_loopback(listen_address: &str) -> bool {
    let addr = listen_address
        .trim()
        .trim_start_matches('[')
        .trim_end_matches(']');
    addr.eq_ignore_ascii_case("localhost")
        || addr
            .parse::<IpAddr>()
            .map(|ip| canonical(ip).is_loopback())
            .unwrap_or(false)
}

fn is_exempt_path(path: &str) -> bool {
    path == "/health" || path.starts_with("/claude-desktop/")
}

/// Whether this request must carry the proxy client token.
///
/// The decision is driven by the actual connection, not only by the
/// configured listen address: a socket bound to loopback only ever sees
/// loopback peers (so loopback setups are unaffected), and a socket still
/// bound to `0.0.0.0` while the saved config already says `127.0.0.1`
/// (restart pending) keeps demanding the token from remote peers.
///
/// `conn` is `None` only when the request did not come through the accept
/// loop; then the configured listen address decides (fail closed when
/// exposed).
pub(crate) fn requires_client_auth(
    listen_address: &str,
    conn: Option<&ConnAddrs>,
    path: &str,
) -> bool {
    if is_exempt_path(path) {
        return false;
    }
    match conn {
        Some(c) => {
            let peer = canonical(c.peer.ip());
            let local = canonical(c.local.ip());
            !(peer.is_loopback() || peer == local)
        }
        None => !listen_is_loopback(listen_address),
    }
}

/// Length-checked constant-time comparison.
fn token_eq(candidate: &str, token: &str) -> bool {
    let (a, b) = (candidate.as_bytes(), token.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn bearer_value(raw: &str) -> &str {
    let raw = raw.trim();
    match raw.split_once(' ') {
        Some((scheme, rest)) if scheme.eq_ignore_ascii_case("bearer") => rest.trim(),
        _ => raw,
    }
}

/// Verify the proxy client token on `parts` and scrub every place it was
/// presented. Returns whether a matching token was found.
pub(crate) fn verify_and_scrub(parts: &mut http::request::Parts, token: &str) -> bool {
    if token.is_empty() {
        return false;
    }
    let mut ok = false;

    if let Some(value) = parts.headers.get(header::AUTHORIZATION) {
        if value
            .to_str()
            .map(|v| token_eq(bearer_value(v), token))
            .unwrap_or(false)
        {
            ok = true;
            parts.headers.insert(
                header::AUTHORIZATION,
                HeaderValue::from_static("Bearer PROXY_MANAGED"),
            );
        }
    }

    for name in ["x-api-key", "x-goog-api-key"] {
        let matches = parts
            .headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(|v| token_eq(v.trim(), token))
            .unwrap_or(false);
        if matches {
            ok = true;
            parts
                .headers
                .insert(name, HeaderValue::from_static(SCRUBBED_PLACEHOLDER));
        }
    }

    if let Some(query) = parts.uri.query() {
        let mut removed = false;
        let kept: Vec<&str> = query
            .split('&')
            .filter(|pair| {
                let is_token = url::form_urlencoded::parse(pair.as_bytes())
                    .next()
                    .map(|(k, v)| k == "key" && token_eq(&v, token))
                    .unwrap_or(false);
                removed |= is_token;
                !is_token
            })
            .collect();
        if removed {
            ok = true;
            let path = parts.uri.path();
            let path_and_query = if kept.is_empty() {
                path.to_string()
            } else {
                format!("{path}?{}", kept.join("&"))
            };
            let mut uri_parts = parts.uri.clone().into_parts();
            if let Ok(pq) = path_and_query.parse() {
                uri_parts.path_and_query = Some(pq);
                if let Ok(uri) = http::Uri::from_parts(uri_parts) {
                    parts.uri = uri;
                }
            }
        }
    }

    ok
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKEN: &str = "ccsp-0123456789abcdef0123456789abcdef";

    fn conn(peer: &str, local: &str) -> ConnAddrs {
        ConnAddrs {
            peer: peer.parse().unwrap(),
            local: local.parse().unwrap(),
        }
    }

    fn parts(uri: &str, headers: &[(&'static str, &str)]) -> http::request::Parts {
        let mut builder = http::Request::builder().uri(uri);
        for (k, v) in headers {
            builder = builder.header(*k, *v);
        }
        builder.body(()).unwrap().into_parts().0
    }

    #[test]
    fn loopback_listen_never_requires_auth() {
        // A loopback-bound socket only ever sees loopback peers.
        let v4 = conn("127.0.0.1:5000", "127.0.0.1:15721");
        let v6 = conn("[::1]:5000", "[::1]:15721");
        for listen in ["127.0.0.1", "localhost", "::1", "[::1]"] {
            assert!(!requires_client_auth(listen, Some(&v4), "/v1/messages"));
            assert!(!requires_client_auth(listen, Some(&v6), "/v1/messages"));
            assert!(!requires_client_auth(listen, None, "/v1/messages"));
        }
    }

    #[test]
    fn remote_peer_needs_auth_even_if_config_already_says_loopback() {
        // Socket still bound to 0.0.0.0, config saved as 127.0.0.1 (restart pending).
        let remote = conn("192.168.1.20:5000", "192.168.1.10:15721");
        assert!(requires_client_auth(
            "127.0.0.1",
            Some(&remote),
            "/v1/messages"
        ));
    }

    #[test]
    fn exposed_listen_requires_auth_only_for_remote_peers() {
        let path = "/v1/messages";
        // Remote LAN client.
        assert!(requires_client_auth(
            "0.0.0.0",
            Some(&conn("192.168.1.20:5000", "192.168.1.10:15721")),
            path
        ));
        // Local CLI via loopback (takeover writes 127.0.0.1 for 0.0.0.0).
        assert!(!requires_client_auth(
            "0.0.0.0",
            Some(&conn("127.0.0.1:5000", "127.0.0.1:15721")),
            path
        ));
        // Local CLI via the machine's own LAN IP (listen on a specific IP).
        assert!(!requires_client_auth(
            "192.168.1.10",
            Some(&conn("192.168.1.10:5000", "192.168.1.10:15721")),
            path
        ));
        // IPv4-mapped loopback on a dual-stack `::` socket.
        assert!(!requires_client_auth(
            "::",
            Some(&conn("[::ffff:127.0.0.1]:5000", "[::ffff:127.0.0.1]:15721")),
            path
        ));
        // Missing connection info fails closed.
        assert!(requires_client_auth("0.0.0.0", None, path));
    }

    #[test]
    fn exempt_paths_skip_auth() {
        let remote = conn("10.0.0.5:5000", "10.0.0.1:15721");
        assert!(!requires_client_auth("0.0.0.0", Some(&remote), "/health"));
        assert!(!requires_client_auth(
            "0.0.0.0",
            Some(&remote),
            "/claude-desktop/v1/messages"
        ));
        assert!(requires_client_auth("0.0.0.0", Some(&remote), "/status"));
    }

    #[test]
    fn accepts_bearer_and_scrubs_it() {
        let mut p = parts(
            "/v1/messages",
            &[("authorization", &format!("Bearer {TOKEN}"))],
        );
        assert!(verify_and_scrub(&mut p, TOKEN));
        assert_eq!(p.headers["authorization"], "Bearer PROXY_MANAGED");
    }

    #[test]
    fn accepts_api_key_headers_and_scrubs_them() {
        for name in ["x-api-key", "x-goog-api-key"] {
            let mut p = parts("/v1/messages", &[(name, TOKEN)]);
            assert!(verify_and_scrub(&mut p, TOKEN), "{name}");
            assert_eq!(p.headers[name], "PROXY_MANAGED");
        }
    }

    #[test]
    fn accepts_query_key_and_removes_it() {
        let mut p = parts(
            &format!("/v1beta/models/g:generateContent?alt=sse&key={TOKEN}"),
            &[],
        );
        assert!(verify_and_scrub(&mut p, TOKEN));
        assert_eq!(p.uri.path(), "/v1beta/models/g:generateContent");
        assert_eq!(p.uri.query(), Some("alt=sse"));

        let mut p = parts(&format!("/v1beta/models?key={TOKEN}"), &[]);
        assert!(verify_and_scrub(&mut p, TOKEN));
        assert_eq!(p.uri.query(), None);
    }

    #[test]
    fn rejects_missing_or_wrong_token_without_touching_request() {
        let mut p = parts(
            "/v1beta/models?key=AIzaUSER",
            &[
                ("authorization", "Bearer PROXY_MANAGED"),
                ("x-api-key", "sk-ant-user"),
            ],
        );
        assert!(!verify_and_scrub(&mut p, TOKEN));
        assert_eq!(p.headers["authorization"], "Bearer PROXY_MANAGED");
        assert_eq!(p.headers["x-api-key"], "sk-ant-user");
        assert_eq!(p.uri.query(), Some("key=AIzaUSER"));

        let mut p = parts("/v1/messages", &[("x-api-key", &TOKEN[..TOKEN.len() - 1])]);
        assert!(!verify_and_scrub(&mut p, TOKEN));
        let mut p = parts("/v1/messages", &[("x-api-key", "")]);
        assert!(!verify_and_scrub(&mut p, ""));
    }

    #[test]
    fn token_store_creates_once_and_regenerates() {
        let db = Database::memory().unwrap();
        let first = get_or_create_proxy_client_token(&db).unwrap();
        assert!(first.starts_with("ccsp-") && first.len() > 30);
        assert_eq!(get_or_create_proxy_client_token(&db).unwrap(), first);
        let second = regenerate_proxy_client_token(&db).unwrap();
        assert_ne!(first, second);
        assert_eq!(get_or_create_proxy_client_token(&db).unwrap(), second);
    }
}
