//! Route and endpoint stage: which protocol bridge a request takes and which
//! upstream URL it is sent to. Pure functions of (app, provider, endpoint, body).

use crate::app_config::AppType;
use crate::provider::Provider;
use crate::proxy::providers::ProviderAdapter;
use serde_json::Value;

/// Protocol/route decisions taken once per attempt, before the body is touched.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Route {
    /// The provider's base URL is the complete endpoint URL.
    pub is_full_url: bool,
    /// GitHub Copilot (OpenAI-compatible endpoints without the `/v1` prefix).
    pub is_copilot: bool,
    /// Codex Responses request bridged to an OpenAI Chat upstream.
    pub codex_responses_to_chat: bool,
    /// Codex Responses request bridged to an Anthropic Messages upstream.
    pub codex_responses_to_anthropic: bool,
    /// Built-in official Codex provider: the client's own ChatGPT auth passes through.
    pub codex_official_auth_passthrough: bool,
    /// Codex→Anthropic with the "impersonate Claude Code" switch on.
    pub codex_impersonate_claude_code: bool,
}

impl Route {
    pub fn resolve(
        app_type: &AppType,
        provider: &Provider,
        endpoint: &str,
        base_url: &str,
    ) -> Self {
        let meta = provider.meta.as_ref();
        let is_full_url = meta.and_then(|meta| meta.is_full_url).unwrap_or(false);
        let is_copilot = meta.and_then(|m| m.provider_type.as_deref()) == Some("github_copilot")
            || base_url.contains("githubcopilot.com");
        let is_codex_like = matches!(app_type, AppType::Codex | AppType::GrokBuild);
        let codex_responses_to_chat = is_codex_like
            && crate::proxy::providers::should_convert_codex_responses_to_chat(provider, endpoint);
        let codex_responses_to_anthropic = is_codex_like
            && crate::proxy::providers::should_convert_codex_responses_to_anthropic(
                provider, endpoint,
            );
        let codex_official_auth_passthrough = matches!(app_type, AppType::Codex)
            && crate::proxy::providers::is_codex_official_provider(provider);
        // Claude Code emulation is off by default and only enabled when the user
        // explicitly turns it on, so general-purpose gateways never see the Claude
        // Code fingerprint and identity prompt.
        let codex_impersonate_claude_code = codex_responses_to_anthropic
            && meta.and_then(|meta| meta.impersonate_claude_code) == Some(true);
        Self {
            is_full_url,
            is_copilot,
            codex_responses_to_chat,
            codex_responses_to_anthropic,
            codex_official_auth_passthrough,
            codex_impersonate_claude_code,
        }
    }

    /// Request bodies on these routes are rewritten, so the upstream must answer
    /// with an identity-encoded body the converters can read.
    pub fn is_bridged(&self) -> bool {
        self.codex_responses_to_chat || self.codex_responses_to_anthropic
    }
}

/// Where one attempt is sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpstreamTarget {
    pub url: String,
    /// Endpoint (path + query) after protocol rewriting.
    pub effective_endpoint: String,
}

/// Rewrite the client endpoint for the chosen bridge and build the upstream URL.
#[allow(clippy::too_many_arguments)]
pub fn resolve_upstream_target(
    adapter: &dyn ProviderAdapter,
    provider: &Provider,
    base_url: &str,
    endpoint: &str,
    route: &Route,
    claude_api_format: Option<&str>,
    needs_transform: bool,
    body: &Value,
) -> UpstreamTarget {
    let (effective_endpoint, passthrough_query) = if route.codex_responses_to_chat {
        rewrite_codex_responses_endpoint_to_chat(endpoint)
    } else if route.codex_responses_to_anthropic {
        rewrite_codex_responses_endpoint_to_anthropic(endpoint)
    } else if needs_transform && adapter.name() == "Claude" {
        let api_format = claude_api_format
            .unwrap_or_else(|| crate::proxy::providers::get_claude_api_format(provider));
        rewrite_claude_transform_endpoint(endpoint, api_format, route.is_copilot, body)
    } else {
        (
            endpoint.to_string(),
            split_endpoint_and_query(endpoint)
                .1
                .map(ToString::to_string),
        )
    };

    let codex_chat_base_is_full_endpoint =
        route.codex_responses_to_chat && base_url_is_full_endpoint(base_url, "/chat/completions");
    // Defensive fallback mirroring the Chat guard: a base URL already ending in the
    // Anthropic `/v1/messages` endpoint with the "full URL" switch off is treated as a
    // full endpoint so `/v1/messages` is not appended twice (a non-retryable 400).
    let codex_anthropic_base_is_full_endpoint =
        route.codex_responses_to_anthropic && base_url_is_full_endpoint(base_url, "/v1/messages");

    let url = if matches!(claude_api_format, Some("gemini_native")) {
        crate::proxy::gemini_url::resolve_gemini_native_url(
            base_url,
            &effective_endpoint,
            route.is_full_url,
        )
    } else if route.is_full_url
        || codex_chat_base_is_full_endpoint
        || codex_anthropic_base_is_full_endpoint
    {
        append_query_to_full_url(base_url, passthrough_query.as_deref())
    } else {
        adapter.build_url(base_url, &effective_endpoint)
    };

    UpstreamTarget {
        url,
        effective_endpoint,
    }
}

pub(crate) fn split_endpoint_and_query(endpoint: &str) -> (&str, Option<&str>) {
    endpoint
        .split_once('?')
        .map_or((endpoint, None), |(path, query)| (path, Some(query)))
}

fn strip_beta_query(query: Option<&str>) -> Option<String> {
    let filtered = query.map(|query| {
        query
            .split('&')
            .filter(|pair| !pair.is_empty() && !pair.starts_with("beta="))
            .collect::<Vec<_>>()
            .join("&")
    });

    match filtered.as_deref() {
        Some("") | None => None,
        Some(_) => filtered,
    }
}

fn is_claude_messages_path(path: &str) -> bool {
    matches!(path, "/v1/messages" | "/claude/v1/messages")
}

fn with_query(target_path: &str, query: Option<&str>) -> String {
    match query {
        Some(query) if !query.is_empty() => format!("{target_path}?{query}"),
        _ => target_path.to_string(),
    }
}

pub(crate) fn rewrite_codex_responses_endpoint_to_chat(endpoint: &str) -> (String, Option<String>) {
    let (_path, query) = split_endpoint_and_query(endpoint);
    let passthrough_query = query.map(ToString::to_string);
    (
        with_query("/chat/completions", passthrough_query.as_deref()),
        passthrough_query,
    )
}

/// Rewrite Codex's `/responses` (and variants) to Anthropic's `/v1/messages`, preserving the query.
pub(crate) fn rewrite_codex_responses_endpoint_to_anthropic(
    endpoint: &str,
) -> (String, Option<String>) {
    let (_path, query) = split_endpoint_and_query(endpoint);
    let passthrough_query = query.map(ToString::to_string);
    (
        with_query("/v1/messages", passthrough_query.as_deref()),
        passthrough_query,
    )
}

pub(crate) fn rewrite_claude_transform_endpoint(
    endpoint: &str,
    api_format: &str,
    is_copilot: bool,
    body: &Value,
) -> (String, Option<String>) {
    let (path, query) = split_endpoint_and_query(endpoint);
    let passthrough_query = if is_claude_messages_path(path) {
        strip_beta_query(query)
    } else {
        query.map(ToString::to_string)
    };

    if !is_claude_messages_path(path) {
        return (endpoint.to_string(), passthrough_query);
    }

    if api_format == "gemini_native" {
        let model = crate::proxy::providers::transform_gemini::extract_gemini_model(body)
            .unwrap_or("unknown");
        // Accept both bare ids (`gemini-2.5-pro`) and the resource-name form
        // (`models/gemini-2.5-pro`) that Gemini SDKs emit.
        let model = crate::proxy::gemini_url::normalize_gemini_model_id(model);
        let is_stream = body
            .get("stream")
            .and_then(|value| value.as_bool())
            .unwrap_or(false);
        let target_path = if is_stream {
            format!("/v1beta/models/{model}:streamGenerateContent")
        } else {
            format!("/v1beta/models/{model}:generateContent")
        };

        let rewritten_query = merge_query_params(
            passthrough_query.as_deref(),
            if is_stream { Some("alt=sse") } else { None },
        );
        return (
            with_query(&target_path, rewritten_query.as_deref()),
            rewritten_query,
        );
    }

    let target_path = if is_copilot && api_format == "openai_responses" {
        "/v1/responses"
    } else if is_copilot {
        "/chat/completions"
    } else if api_format == "openai_responses" {
        "/v1/responses"
    } else {
        "/v1/chat/completions"
    };

    (
        with_query(target_path, passthrough_query.as_deref()),
        passthrough_query,
    )
}

fn merge_query_params(base_query: Option<&str>, extra_param: Option<&str>) -> Option<String> {
    let mut params: Vec<String> = base_query
        .into_iter()
        .flat_map(|query| query.split('&'))
        .filter(|pair| !pair.is_empty())
        .filter(|pair| !pair.starts_with("alt="))
        .map(ToString::to_string)
        .collect();

    if let Some(extra_param) = extra_param {
        params.push(extra_param.to_string());
    }

    if params.is_empty() {
        None
    } else {
        Some(params.join("&"))
    }
}

pub(crate) fn append_query_to_full_url(base_url: &str, query: Option<&str>) -> String {
    match query {
        Some(query) if !query.is_empty() => {
            if base_url.contains('?') {
                format!("{base_url}&{query}")
            } else {
                format!("{base_url}?{query}")
            }
        }
        _ => base_url.to_string(),
    }
}

/// Whether `base_url` already ends in `endpoint_suffix` (e.g. `/v1/messages` or
/// `/chat/completions`), ignoring surrounding whitespace, any `?query`/`#fragment`, and a
/// trailing slash. Used to avoid double-appending the endpoint when a user pastes a full
/// URL but leaves the "full URL" switch off. `endpoint_suffix` must be lowercase.
pub(crate) fn base_url_is_full_endpoint(base_url: &str, endpoint_suffix: &str) -> bool {
    let trimmed = base_url.trim();
    let path = match trimmed.split_once(['?', '#']) {
        Some((head, _)) => head,
        None => trimmed,
    };
    path.trim_end_matches('/')
        .to_ascii_lowercase()
        .ends_with(endpoint_suffix)
}

/// Whether the client asked for a streamed response.
pub(crate) fn is_streaming_request(
    endpoint: &str,
    body: &Value,
    headers: &axum::http::HeaderMap,
) -> bool {
    if body
        .get("stream")
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
    {
        return true;
    }

    if endpoint.contains("streamGenerateContent") || endpoint.contains("alt=sse") {
        return true;
    }

    headers
        .get(axum::http::header::ACCEPT)
        .and_then(|value| value.to_str().ok())
        .map(|accept| accept.contains("text/event-stream"))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proxy::providers::get_adapter;
    use serde_json::json;

    fn provider(settings: Value, meta: Option<crate::provider::ProviderMeta>) -> Provider {
        let mut provider = Provider::with_id("p".into(), "P".into(), settings, None);
        provider.meta = meta;
        provider
    }

    #[test]
    fn native_claude_route_is_plain_passthrough() {
        let p = provider(
            json!({"env": {"ANTHROPIC_BASE_URL": "https://relay.example"}}),
            None,
        );
        let route = Route::resolve(
            &AppType::Claude,
            &p,
            "/v1/messages",
            "https://relay.example",
        );
        assert_eq!(route, Route::default());
        let adapter = get_adapter(&AppType::Claude);
        let target = resolve_upstream_target(
            adapter.as_ref(),
            &p,
            "https://relay.example",
            "/v1/messages?beta=true",
            &route,
            Some("anthropic"),
            false,
            &json!({}),
        );
        assert_eq!(target.url, "https://relay.example/v1/messages?beta=true");
        assert_eq!(target.effective_endpoint, "/v1/messages?beta=true");
    }

    #[test]
    fn copilot_is_detected_from_the_base_url() {
        let p = provider(json!({}), None);
        let route = Route::resolve(
            &AppType::Claude,
            &p,
            "/v1/messages",
            "https://api.githubcopilot.com",
        );
        assert!(route.is_copilot);
        assert!(!route.is_bridged());
    }

    #[test]
    fn full_url_provider_keeps_its_url_and_appends_the_query() {
        let p = provider(
            json!({}),
            Some(crate::provider::ProviderMeta {
                is_full_url: Some(true),
                ..Default::default()
            }),
        );
        let route = Route::resolve(&AppType::Claude, &p, "/v1/messages", "https://h/x");
        assert!(route.is_full_url);
        let adapter = get_adapter(&AppType::Claude);
        let target = resolve_upstream_target(
            adapter.as_ref(),
            &p,
            "https://h/custom/messages",
            "/v1/messages?a=1",
            &route,
            Some("anthropic"),
            false,
            &json!({}),
        );
        assert_eq!(target.url, "https://h/custom/messages?a=1");
    }

    #[test]
    fn claude_transform_rewrites_to_chat_completions() {
        let p = provider(json!({}), None);
        let adapter = get_adapter(&AppType::Claude);
        let target = resolve_upstream_target(
            adapter.as_ref(),
            &p,
            "https://h",
            "/v1/messages?beta=true",
            &Route::default(),
            Some("openai_chat"),
            true,
            &json!({}),
        );
        assert_eq!(target.effective_endpoint, "/v1/chat/completions");
        assert_eq!(target.url, "https://h/v1/chat/completions");
    }
}
