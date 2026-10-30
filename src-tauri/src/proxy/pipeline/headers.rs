//! Header policy stage: the single place that decides which client headers reach
//! the upstream, which are replaced in place (host, credentials, beta flags) and
//! which are appended. Pure: inputs in, ordered `HeaderMap` out.

use crate::provider::{LocalProxyRequestOverrides, Provider};
use crate::proxy::ProxyError;
use http::{HeaderMap, HeaderName, HeaderValue};

/// Placeholder credential the proxy writes into client configs it manages.
pub(crate) const PROXY_AUTH_PLACEHOLDER: &str = "PROXY_MANAGED";

/// Claude Code client fingerprint (used for Codex→Anthropic emulation to pass a
/// gateway's "Claude Code only" check).
pub(crate) const CLAUDE_CODE_USER_AGENT: &str = "claude-cli/1.0.119 (external, cli)";
const CLAUDE_CODE_BETA: &str = "claude-code-20250219";
const CONTEXT_1M_BETA: &str = "context-1m-2025-08-07";

/// Headers injected by the Copilot auth adapter; client copies are dropped.
const COPILOT_FINGERPRINT_HEADERS: &[&str] = &[
    "user-agent",
    "editor-version",
    "editor-plugin-version",
    "copilot-integration-id",
    "x-github-api-version",
    "openai-intent",
    "x-initiator",
    "x-interaction-type",
    "x-interaction-id",
    "x-vscode-user-agent-library-version",
    "x-request-id",
    "x-agent-task-id",
];

/// Connection, tracing and CDN headers that describe the client hop only.
const CLIENT_HOP_HEADERS: &[&str] = &[
    "content-length",
    "transfer-encoding",
    "x-forwarded-host",
    "x-forwarded-port",
    "x-forwarded-proto",
    "forwarded",
    "cf-connecting-ip",
    "cf-ipcountry",
    "cf-ray",
    "cf-visitor",
    "true-client-ip",
    "fastly-client-ip",
    "x-azure-clientip",
    "x-azure-fdid",
    "x-azure-ref",
    "akamai-origin-hop",
    "x-akamai-config-log-detail",
    "x-request-id",
    "x-correlation-id",
    "x-trace-id",
    "x-amzn-trace-id",
    "x-b3-traceid",
    "x-b3-spanid",
    "x-b3-parentspanid",
    "x-b3-sampled",
    "traceparent",
    "tracestate",
];

/// Request-side hop-by-hop headers (RFC 7230 §6.1 plus `expect`, which the raw
/// writer cannot honour because it sends the body immediately).
const HOP_BY_HOP_REQUEST_HEADERS: &[&str] = &[
    "connection",
    "keep-alive",
    "proxy-connection",
    "proxy-authorization",
    "te",
    "trailer",
    "upgrade",
    "expect",
];

/// Everything the header policy needs for one attempt.
pub struct HeaderPolicy<'a> {
    pub client_headers: &'a HeaderMap,
    /// Credentials (and Copilot fingerprint headers) from the provider adapter.
    pub auth_headers: &'a [(HeaderName, HeaderValue)],
    /// `host[:port]` of the upstream URL.
    pub upstream_host: Option<String>,
    /// Built-in official Codex route: the client's `authorization` passes through.
    pub passthrough_client_authorization: bool,
    pub codex_responses_to_anthropic: bool,
    pub codex_impersonate_claude_code: bool,
    /// Ask the upstream for an identity-encoded body (transform and SSE paths).
    pub force_identity_encoding: bool,
    pub is_copilot: bool,
    pub custom_user_agent: Option<HeaderValue>,
    /// Rebuilt `anthropic-beta` value; `None` drops the client's header.
    pub anthropic_beta: Option<String>,
    /// Native Anthropic upstream: forward `anthropic-version`.
    pub send_anthropic_headers: bool,
    /// Headers that overwrite whatever the client sent (Codex OAuth session ids).
    pub override_headers: Vec<(HeaderName, HeaderValue)>,
    pub local_overrides: Option<&'a LocalProxyRequestOverrides>,
}

impl HeaderPolicy<'_> {
    /// Build the ordered upstream header map. Client order is preserved; replaced
    /// headers keep the client's position.
    pub fn build(&self) -> HeaderMap {
        let mut out = HeaderMap::new();
        let mut saw_auth = false;
        let mut saw_accept_encoding = false;
        let mut saw_accept = false;
        let mut saw_user_agent = false;
        let mut saw_anthropic_beta = false;
        let mut saw_anthropic_version = false;
        let connection_listed = connection_listed_request_headers(self.client_headers);
        let copilot_fingerprint: &[&str] = if self.is_copilot {
            COPILOT_FINGERPRINT_HEADERS
        } else {
            &[]
        };

        for (key, value) in self.client_headers {
            let key_str = key.as_str();

            if is_hop_by_hop_request_header(key_str, &connection_listed) {
                continue;
            }

            if key_str == "host" {
                if let Some(hv) = self
                    .upstream_host
                    .as_deref()
                    .and_then(|host| HeaderValue::from_str(host).ok())
                {
                    out.append(key.clone(), hv);
                }
                continue;
            }

            if CLIENT_HOP_HEADERS.contains(&key_str) {
                continue;
            }

            if matches!(key_str, "authorization" | "x-api-key" | "x-goog-api-key") {
                if self.passthrough_client_authorization && key_str == "authorization" {
                    saw_auth = true;
                    out.append(key.clone(), value.clone());
                    continue;
                }
                if !saw_auth {
                    saw_auth = true;
                    for (name, value) in self.auth_headers {
                        out.append(name.clone(), value.clone());
                    }
                }
                continue;
            }

            // Codex→Anthropic emulation injects `x-app: cli` below.
            if self.codex_impersonate_claude_code && key_str == "x-app" {
                continue;
            }

            // Codex/OpenAI client fingerprints never leak to an Anthropic upstream.
            if self.codex_responses_to_anthropic && is_codex_client_fingerprint_header(key_str) {
                continue;
            }

            // The Codex CLI sends `Accept: text/event-stream`; strict Anthropic
            // gateways answer 406 to that, so normalize to JSON.
            if self.codex_responses_to_anthropic && key_str == "accept" {
                if !saw_accept {
                    saw_accept = true;
                    out.append(
                        http::header::ACCEPT,
                        HeaderValue::from_static("application/json"),
                    );
                }
                continue;
            }

            if key_str == "accept-encoding" {
                if !saw_accept_encoding {
                    saw_accept_encoding = true;
                    if self.force_identity_encoding {
                        out.append(
                            http::header::ACCEPT_ENCODING,
                            HeaderValue::from_static("identity"),
                        );
                    } else {
                        out.append(key.clone(), value.clone());
                    }
                }
                continue;
            }

            if !self.is_copilot && key_str == "user-agent" {
                if !saw_user_agent {
                    saw_user_agent = true;
                    let ua = self.custom_user_agent.as_ref().unwrap_or(value);
                    out.append(http::header::USER_AGENT, ua.clone());
                }
                continue;
            }

            if key_str == "anthropic-beta" {
                if !saw_anthropic_beta {
                    saw_anthropic_beta = true;
                    if let Some(hv) = self.beta_header_value() {
                        out.append("anthropic-beta", hv);
                    }
                }
                continue;
            }

            if key_str == "anthropic-version" {
                if self.send_anthropic_headers {
                    saw_anthropic_version = true;
                    out.append(key.clone(), value.clone());
                }
                continue;
            }

            if copilot_fingerprint.contains(&key_str) {
                continue;
            }

            out.append(key.clone(), value.clone());
        }

        if !saw_auth {
            for (name, value) in self.auth_headers {
                out.append(name.clone(), value.clone());
            }
        }
        if !saw_accept_encoding && self.force_identity_encoding {
            out.append(
                http::header::ACCEPT_ENCODING,
                HeaderValue::from_static("identity"),
            );
        }
        if self.codex_responses_to_anthropic && !saw_accept {
            out.append(
                http::header::ACCEPT,
                HeaderValue::from_static("application/json"),
            );
        }
        if self.codex_impersonate_claude_code {
            out.append("x-app", HeaderValue::from_static("cli"));
        }
        if !saw_user_agent {
            if let Some(ua) = &self.custom_user_agent {
                out.append(http::header::USER_AGENT, ua.clone());
            }
        }
        if !saw_anthropic_beta {
            if let Some(hv) = self.beta_header_value() {
                out.append("anthropic-beta", hv);
            }
        }
        // anthropic-version is required on every Anthropic upstream, including the
        // plain Codex→Anthropic bridge (which sends no Claude Code beta).
        if (self.send_anthropic_headers || self.codex_responses_to_anthropic)
            && !saw_anthropic_version
        {
            out.append("anthropic-version", HeaderValue::from_static("2023-06-01"));
        }
        for (name, value) in &self.override_headers {
            out.insert(name.clone(), value.clone());
        }
        if !out.contains_key(http::header::CONTENT_TYPE) {
            out.insert(
                http::header::CONTENT_TYPE,
                HeaderValue::from_static("application/json"),
            );
        }
        apply_local_proxy_header_overrides(&mut out, self.local_overrides, self.is_copilot);
        out
    }

    fn beta_header_value(&self) -> Option<HeaderValue> {
        self.anthropic_beta
            .as_deref()
            .and_then(|beta| HeaderValue::from_str(beta).ok())
    }
}

/// The `anthropic-beta` value sent upstream.
///
/// Native Anthropic upstreams always carry the Claude Code marker (prepended to the
/// client's list when missing); the Codex→Anthropic bridge only sends the markers
/// its emulation / `[1m]` model needs; every other upstream gets none.
pub fn anthropic_beta_value(
    client_headers: &HeaderMap,
    native_anthropic: bool,
    codex_impersonate_claude_code: bool,
    codex_anthropic_one_m: bool,
) -> Option<String> {
    if native_anthropic {
        let client = client_headers
            .get("anthropic-beta")
            .and_then(|beta| beta.to_str().ok());
        return Some(match client {
            Some(beta) if beta.contains(CLAUDE_CODE_BETA) => beta.to_string(),
            Some(beta) => format!("{CLAUDE_CODE_BETA},{beta}"),
            None => CLAUDE_CODE_BETA.to_string(),
        });
    }
    let mut betas = Vec::new();
    if codex_impersonate_claude_code {
        betas.push(CLAUDE_CODE_BETA);
    }
    if codex_anthropic_one_m {
        betas.push(CONTEXT_1M_BETA);
    }
    (!betas.is_empty()).then(|| betas.join(","))
}

/// Header names the client listed in `Connection` (lowercased). Names the proxy
/// rewrites itself (host and credentials) are never dropped this way.
pub(crate) fn connection_listed_request_headers(headers: &HeaderMap) -> Vec<String> {
    headers
        .get_all(http::header::CONNECTION)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .map(|name| name.trim().to_ascii_lowercase())
        .filter(|name| {
            !name.is_empty()
                && !matches!(
                    name.as_str(),
                    "host" | "authorization" | "x-api-key" | "x-goog-api-key"
                )
        })
        .collect()
}

pub(crate) fn is_hop_by_hop_request_header(key_str: &str, connection_listed: &[String]) -> bool {
    HOP_BY_HOP_REQUEST_HEADERS
        .iter()
        .any(|name| key_str.eq_ignore_ascii_case(name))
        || connection_listed
            .iter()
            .any(|name| key_str.eq_ignore_ascii_case(name))
}

/// Headers a native Claude Code client never sends but the Codex/OpenAI CLI (and
/// its stainless SDK layer) do. `key_str` is already lowercased by the http crate.
pub(crate) fn is_codex_client_fingerprint_header(key_str: &str) -> bool {
    matches!(
        key_str,
        "originator"
            | "session_id"
            | "session-id"
            | "thread-id"
            | "conversation_id"
            | "chatgpt-account-id"
            | "x-openai-subagent"
            | "x-client-request-id"
            | "openai-beta"
            | "openai-organization"
            | "openai-project"
    ) || key_str.starts_with("x-stainless-")
        || key_str.starts_with("x-codex-")
}

/// Session routing headers the official Codex CLI sends to the ChatGPT backend.
/// `session_id` must be the client's own value (PROXY-44), not a proxy-internal key.
pub(crate) fn build_codex_oauth_session_headers(
    session_id: &str,
) -> Vec<(HeaderName, HeaderValue)> {
    let session_id = session_id.trim();
    if session_id.is_empty() {
        return Vec::new();
    }

    let mut headers = Vec::new();
    if let Ok(value) = HeaderValue::from_str(session_id) {
        headers.push((HeaderName::from_static("session_id"), value.clone()));
        headers.push((HeaderName::from_static("x-client-request-id"), value));
    }

    let window_id = format!("{session_id}:0");
    if let Ok(value) = HeaderValue::from_str(&window_id) {
        headers.push((HeaderName::from_static("x-codex-window-id"), value));
    }

    headers
}

/// Copilot optimizer: adjust the adapter's fingerprint headers for this request
/// (initiator, sub-agent interaction type, deterministic request id) and add the
/// session's interaction id.
pub(crate) fn apply_copilot_hint_headers(
    auth_headers: &mut Vec<(HeaderName, HeaderValue)>,
    hints: &crate::proxy::pipeline::body::CopilotHints,
    config: &crate::proxy::types::CopilotOptimizerConfig,
) {
    let classification = &hints.classification;
    for (name, value) in auth_headers.iter_mut() {
        match name.as_str() {
            "x-initiator" if config.request_classification => {
                *value = HeaderValue::from_static(classification.initiator);
            }
            // Sub-agent requests do not count as premium interactions.
            "x-interaction-type" if classification.is_subagent => {
                *value = HeaderValue::from_static("conversation-subagent");
            }
            "x-request-id" | "x-agent-task-id" => {
                if let Some(hv) = hints
                    .deterministic_request_id
                    .as_deref()
                    .and_then(|id| HeaderValue::from_str(id).ok())
                {
                    *value = hv;
                }
            }
            _ => {}
        }
    }
    if let Some(hv) = hints
        .interaction_id
        .as_deref()
        .and_then(|id| HeaderValue::from_str(id).ok())
    {
        auth_headers.push((HeaderName::from_static("x-interaction-id"), hv));
    }
    if classification.is_subagent {
        log::info!(
            "[Copilot] sub-agent request: x-initiator=agent, x-interaction-type=conversation-subagent"
        );
    }
}

/// Custom User-Agent configured on the provider (ignored for Copilot, whose
/// fingerprint UA must not change), or the Claude Code UA during emulation.
pub(crate) fn custom_user_agent(
    provider: &Provider,
    is_copilot: bool,
    codex_impersonate_claude_code: bool,
) -> Option<HeaderValue> {
    if is_copilot {
        return None;
    }
    provider
        .meta
        .as_ref()
        .and_then(|meta| meta.custom_user_agent_header().ok().flatten())
        .or_else(|| {
            codex_impersonate_claude_code.then(|| HeaderValue::from_static(CLAUDE_CODE_USER_AGENT))
        })
}

pub(crate) fn reject_proxy_placeholder_for_managed_account_upstream(
    url: &str,
    headers: &HeaderMap,
) -> Result<(), ProxyError> {
    if !is_managed_account_upstream_url(url) || !headers_contain_proxy_placeholder(headers) {
        return Ok(());
    }

    Err(ProxyError::AuthError(
        "Managed account proxy auth was not resolved; PROXY_MANAGED must not be sent upstream"
            .to_string(),
    ))
}

fn is_managed_account_upstream_url(url: &str) -> bool {
    let Ok(uri) = url.parse::<http::Uri>() else {
        return false;
    };

    let Some(host) = uri.host().map(str::to_ascii_lowercase) else {
        return false;
    };

    host == "githubcopilot.com"
        || host.ends_with(".githubcopilot.com")
        || (host == "chatgpt.com" && uri.path().starts_with("/backend-api/codex"))
}

fn headers_contain_proxy_placeholder(headers: &HeaderMap) -> bool {
    headers.values().any(|value| {
        value
            .to_str()
            .map(|value| value.contains(PROXY_AUTH_PLACEHOLDER))
            .unwrap_or(false)
    })
}

pub(crate) fn apply_local_proxy_header_overrides(
    headers: &mut HeaderMap,
    overrides: Option<&LocalProxyRequestOverrides>,
    is_copilot: bool,
) {
    if is_copilot {
        return;
    }

    let Some(header_overrides) = overrides.map(|overrides| &overrides.headers) else {
        return;
    };

    for (raw_name, raw_value) in header_overrides {
        let header_name = raw_name.trim().to_ascii_lowercase();
        if header_name.is_empty() {
            log::warn!("[LocalProxyOverrides] Ignoring header override with empty name");
            continue;
        }

        let Ok(name) = HeaderName::from_bytes(header_name.as_bytes()) else {
            log::warn!("[LocalProxyOverrides] Ignoring invalid header override name: {raw_name}");
            continue;
        };

        if is_protected_local_proxy_override_header(&name) {
            log::debug!(
                "[LocalProxyOverrides] Ignoring protected header override: {}",
                name.as_str()
            );
            continue;
        }

        let Ok(value) = HeaderValue::from_str(raw_value) else {
            log::warn!(
                "[LocalProxyOverrides] Ignoring invalid header override value for {}",
                name.as_str()
            );
            continue;
        };

        headers.insert(name, value);
    }
}

fn is_protected_local_proxy_override_header(name: &HeaderName) -> bool {
    let name = name.as_str();
    CLIENT_HOP_HEADERS.contains(&name)
        || matches!(
            name,
            "host"
                | "connection"
                | "proxy-authorization"
                | "proxy-authenticate"
                | "te"
                | "trailer"
                | "upgrade"
                | "accept-encoding"
                | "content-type"
                | "authorization"
                | "x-api-key"
                | "x-goog-api-key"
                | "chatgpt-account-id"
                | "session_id"
                | "x-client-request-id"
                | "x-codex-window-id"
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn client_headers() -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.append("host", HeaderValue::from_static("127.0.0.1:15721"));
        headers.append("x-api-key", HeaderValue::from_static("PROXY_MANAGED"));
        headers.append("user-agent", HeaderValue::from_static("claude-cli/2.0"));
        headers.append("anthropic-version", HeaderValue::from_static("2023-06-01"));
        headers.append("x-request-id", HeaderValue::from_static("trace"));
        headers.append("connection", HeaderValue::from_static("keep-alive"));
        headers.append("accept-encoding", HeaderValue::from_static("gzip"));
        headers
    }

    fn policy<'a>(
        client: &'a HeaderMap,
        auth: &'a [(HeaderName, HeaderValue)],
    ) -> HeaderPolicy<'a> {
        HeaderPolicy {
            client_headers: client,
            auth_headers: auth,
            upstream_host: Some("relay.example".into()),
            passthrough_client_authorization: false,
            codex_responses_to_anthropic: false,
            codex_impersonate_claude_code: false,
            force_identity_encoding: false,
            is_copilot: false,
            custom_user_agent: None,
            anthropic_beta: anthropic_beta_value(client, true, false, false),
            send_anthropic_headers: true,
            override_headers: Vec::new(),
            local_overrides: None,
        }
    }

    fn names(headers: &HeaderMap) -> Vec<&str> {
        headers.keys().map(|k| k.as_str()).collect()
    }

    #[test]
    fn native_anthropic_replaces_host_and_credentials_in_place() {
        let client = client_headers();
        let auth = [(
            HeaderName::from_static("authorization"),
            HeaderValue::from_static("Bearer sk-real"),
        )];
        let out = policy(&client, &auth).build();
        assert_eq!(
            names(&out),
            vec![
                "host",
                "authorization",
                "user-agent",
                "anthropic-version",
                "accept-encoding",
                "anthropic-beta",
                "content-type"
            ]
        );
        assert_eq!(out["host"], "relay.example");
        assert_eq!(out["authorization"], "Bearer sk-real");
        assert_eq!(out["anthropic-beta"], CLAUDE_CODE_BETA);
        assert_eq!(out["accept-encoding"], "gzip");
        assert!(!out.contains_key("x-api-key"));
        assert!(!out.contains_key("x-request-id"));
    }

    #[test]
    fn identity_encoding_and_custom_user_agent_are_applied() {
        let client = client_headers();
        let mut p = policy(&client, &[]);
        p.force_identity_encoding = true;
        p.custom_user_agent = Some(HeaderValue::from_static("my-ua"));
        let out = p.build();
        assert_eq!(out["accept-encoding"], "identity");
        assert_eq!(out["user-agent"], "my-ua");
    }

    #[test]
    fn codex_to_anthropic_drops_codex_fingerprints_and_normalizes_accept() {
        let mut client = HeaderMap::new();
        client.append("originator", HeaderValue::from_static("codex_cli_rs"));
        client.append("x-stainless-lang", HeaderValue::from_static("rust"));
        client.append("accept", HeaderValue::from_static("text/event-stream"));
        client.append("x-app", HeaderValue::from_static("codex"));
        let mut p = policy(&client, &[]);
        p.send_anthropic_headers = false;
        p.codex_responses_to_anthropic = true;
        p.codex_impersonate_claude_code = true;
        p.anthropic_beta = anthropic_beta_value(&client, false, true, true);
        let out = p.build();
        assert!(!out.contains_key("originator"));
        assert!(!out.contains_key("x-stainless-lang"));
        assert_eq!(out["accept"], "application/json");
        assert_eq!(out["x-app"], "cli");
        assert_eq!(out["anthropic-version"], "2023-06-01");
        assert_eq!(
            out["anthropic-beta"],
            "claude-code-20250219,context-1m-2025-08-07"
        );
    }

    #[test]
    fn non_anthropic_upstreams_get_no_beta_or_version() {
        let client = client_headers();
        let mut p = policy(&client, &[]);
        p.send_anthropic_headers = false;
        p.anthropic_beta = anthropic_beta_value(&client, false, false, false);
        let out = p.build();
        assert!(!out.contains_key("anthropic-beta"));
        assert!(!out.contains_key("anthropic-version"));
    }

    #[test]
    fn override_headers_replace_client_values() {
        let mut client = HeaderMap::new();
        client.append("session_id", HeaderValue::from_static("client"));
        let mut p = policy(&client, &[]);
        p.override_headers = build_codex_oauth_session_headers("abc");
        let out = p.build();
        assert_eq!(out["session_id"], "abc");
        assert_eq!(out["x-codex-window-id"], "abc:0");
        assert_eq!(out.get_all("session_id").iter().count(), 1);
    }

    #[test]
    fn beta_marker_is_prepended_once() {
        let mut client = HeaderMap::new();
        client.insert("anthropic-beta", HeaderValue::from_static("foo"));
        assert_eq!(
            anthropic_beta_value(&client, true, false, false).as_deref(),
            Some("claude-code-20250219,foo")
        );
        client.insert(
            "anthropic-beta",
            HeaderValue::from_static("claude-code-20250219,foo"),
        );
        assert_eq!(
            anthropic_beta_value(&client, true, false, false).as_deref(),
            Some("claude-code-20250219,foo")
        );
    }
}
