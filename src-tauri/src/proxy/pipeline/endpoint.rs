//! 上游端点与 URL 解析
//!
//! 按本次尝试的路由判定（Codex 桥接方向、Claude API 格式、Copilot、full URL）
//! 把客户端端点改写为上游端点，并拼出最终 URL。

use super::request_builder::RouteFlags;
use crate::provider::Provider;
use crate::proxy::protocol::{ClaudeApiFormat, WireFormat};
use crate::proxy::providers::ProviderAdapter;
use serde_json::Value;

/// 改写后的上游目标
pub(crate) struct UpstreamTarget {
    /// 上游端点（含 query）
    pub endpoint: String,
    /// 最终上游 URL
    pub url: String,
}

impl UpstreamTarget {
    /// `body` 是格式转换前的请求体（Gemini Native 端点需要其中的 model / stream）
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn resolve(
        adapter: &dyn ProviderAdapter,
        provider: &Provider,
        flags: &RouteFlags,
        base_url: &str,
        endpoint: &str,
        api_format: Option<ClaudeApiFormat>,
        needs_transform: bool,
        body: &Value,
    ) -> Self {
        let (effective_endpoint, passthrough_query) = if flags.codex_responses_to_chat {
            rewrite_codex_responses_endpoint_to_chat(endpoint)
        } else if flags.codex_responses_to_anthropic {
            rewrite_codex_responses_endpoint_to_anthropic(endpoint)
        } else if flags.gemini_to_anthropic {
            // Gemini 的 query（`alt=sse`、`key=`）对 Anthropic 上游没有意义，不转发
            ("/v1/messages".to_string(), None)
        } else if needs_transform && adapter.name() == "Claude" {
            let api_format = api_format
                .unwrap_or_else(|| crate::proxy::providers::get_claude_api_format(provider));
            rewrite_claude_transform_endpoint(endpoint, api_format, flags.is_copilot, body)
        } else {
            (
                endpoint.to_string(),
                split_endpoint_and_query(endpoint)
                    .1
                    .map(ToString::to_string),
            )
        };

        let codex_chat_base_is_full_endpoint = flags.codex_responses_to_chat
            && base_url_is_full_endpoint(base_url, "/chat/completions");

        // Defensive fallback mirroring `codex_chat_base_is_full_endpoint`: if a user pastes
        // a base URL already ending in the Anthropic `/v1/messages` endpoint but leaves the
        // "full URL" switch off, treat it as a full endpoint so we don't double-append
        // `/v1/messages` (→ `.../v1/messages/v1/messages`, a non-retryable 400). Matches the
        // exact endpoint suffix, so prefixed gateways like `.../api/v1/messages` are covered.
        let codex_anthropic_base_is_full_endpoint = flags.foreign_client_to_anthropic()
            && base_url_is_full_endpoint(base_url, "/v1/messages");

        let url = if api_format == Some(ClaudeApiFormat::GeminiNative) {
            crate::proxy::gemini_url::resolve_gemini_native_url(
                base_url,
                &effective_endpoint,
                flags.is_full_url,
            )
        } else if flags.is_full_url
            || codex_chat_base_is_full_endpoint
            || codex_anthropic_base_is_full_endpoint
        {
            append_query_to_full_url(base_url, passthrough_query.as_deref())
        } else {
            adapter.build_url(base_url, &effective_endpoint)
        };

        Self {
            endpoint: effective_endpoint,
            url,
        }
    }
}

fn split_endpoint_and_query(endpoint: &str) -> (&str, Option<&str>) {
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

fn rewrite_codex_responses_endpoint_to_chat(endpoint: &str) -> (String, Option<String>) {
    let (_path, query) = split_endpoint_and_query(endpoint);
    let passthrough_query = query.map(ToString::to_string);
    let target_path = "/chat/completions";
    let rewritten = match passthrough_query.as_deref() {
        Some(query) if !query.is_empty() => format!("{target_path}?{query}"),
        _ => target_path.to_string(),
    };

    (rewritten, passthrough_query)
}

/// Whether `base_url` already ends in `endpoint_suffix` (e.g. `/v1/messages` or
/// `/chat/completions`), ignoring surrounding whitespace, any `?query`/`#fragment`, and a
/// trailing slash. Used to avoid double-appending the endpoint when a user pastes a full
/// URL but leaves the "full URL" switch off (`.../v1/messages` → `.../v1/messages/v1/messages`,
/// a non-retryable 400). `endpoint_suffix` must be lowercase.
fn base_url_is_full_endpoint(base_url: &str, endpoint_suffix: &str) -> bool {
    let trimmed = base_url.trim();
    // Match against the path only: a `?query`/`#fragment` on a full endpoint URL must not
    // hide the suffix (`.../v1/messages?beta=true` still ends in the endpoint).
    let path = match trimmed.split_once(['?', '#']) {
        Some((head, _)) => head,
        None => trimmed,
    };
    path.trim_end_matches('/')
        .to_ascii_lowercase()
        .ends_with(endpoint_suffix)
}

/// Rewrite Codex's `/responses` (and variants) to Anthropic's `/v1/messages`, preserving the query.
fn rewrite_codex_responses_endpoint_to_anthropic(endpoint: &str) -> (String, Option<String>) {
    let (_path, query) = split_endpoint_and_query(endpoint);
    let passthrough_query = query.map(ToString::to_string);
    let target_path = "/v1/messages";
    let rewritten = match passthrough_query.as_deref() {
        Some(query) if !query.is_empty() => format!("{target_path}?{query}"),
        _ => target_path.to_string(),
    };

    (rewritten, passthrough_query)
}

fn rewrite_claude_transform_endpoint(
    endpoint: &str,
    api_format: ClaudeApiFormat,
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

    if api_format == ClaudeApiFormat::GeminiNative {
        let model = crate::proxy::codec::gemini::extract_gemini_model(body).unwrap_or("unknown");
        // Accept both bare ids (`gemini-2.5-pro`) and the resource-name
        // form (`models/gemini-2.5-pro`) that Gemini SDKs emit. See
        // `normalize_gemini_model_id` for rationale.
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

        let rewritten = match rewritten_query.as_deref() {
            Some(query) if !query.is_empty() => format!("{target_path}?{query}"),
            _ => target_path,
        };

        return (rewritten, rewritten_query);
    }

    let target_path = match (api_format.wire_format(), is_copilot) {
        (WireFormat::OpenAiResponses, _) => "/v1/responses",
        (_, true) => "/chat/completions",
        _ => "/v1/chat/completions",
    };

    let rewritten = match passthrough_query.as_deref() {
        Some(query) if !query.is_empty() => format!("{target_path}?{query}"),
        _ => target_path.to_string(),
    };

    (rewritten, passthrough_query)
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

fn append_query_to_full_url(base_url: &str, query: Option<&str>) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn rewrite_claude_transform_endpoint_strips_beta_for_chat_completions() {
        let (endpoint, passthrough_query) = rewrite_claude_transform_endpoint(
            "/v1/messages?beta=true&foo=bar",
            ClaudeApiFormat::OpenAiChat,
            false,
            &json!({ "model": "gpt-5.4" }),
        );

        assert_eq!(endpoint, "/v1/chat/completions?foo=bar");
        assert_eq!(passthrough_query.as_deref(), Some("foo=bar"));
    }

    #[test]
    fn rewrite_claude_transform_endpoint_strips_beta_for_responses() {
        let (endpoint, passthrough_query) = rewrite_claude_transform_endpoint(
            "/claude/v1/messages?beta=true&x-id=1",
            ClaudeApiFormat::OpenAiResponses,
            false,
            &json!({ "model": "gpt-5.4" }),
        );

        assert_eq!(endpoint, "/v1/responses?x-id=1");
        assert_eq!(passthrough_query.as_deref(), Some("x-id=1"));
    }

    #[test]
    fn rewrite_codex_responses_endpoint_to_chat_preserves_query() {
        let (endpoint, passthrough_query) =
            rewrite_codex_responses_endpoint_to_chat("/v1/responses?foo=bar");

        assert_eq!(endpoint, "/chat/completions?foo=bar");
        assert_eq!(passthrough_query.as_deref(), Some("foo=bar"));
    }

    #[test]
    fn rewrite_codex_responses_compact_endpoint_to_chat_preserves_query() {
        let (endpoint, passthrough_query) =
            rewrite_codex_responses_endpoint_to_chat("/v1/responses/compact?foo=bar");

        assert_eq!(endpoint, "/chat/completions?foo=bar");
        assert_eq!(passthrough_query.as_deref(), Some("foo=bar"));
    }

    #[test]
    fn rewrite_codex_responses_endpoint_to_anthropic_preserves_query() {
        let (endpoint, passthrough_query) =
            rewrite_codex_responses_endpoint_to_anthropic("/responses?x=1");
        assert_eq!(endpoint, "/v1/messages?x=1");
        assert_eq!(passthrough_query.as_deref(), Some("x=1"));

        let (endpoint, _) = rewrite_codex_responses_endpoint_to_anthropic("/v1/responses");
        assert_eq!(endpoint, "/v1/messages");
    }

    #[test]
    fn codex_anthropic_full_endpoint_guard_avoids_double_messages() {
        // On the Codex→Anthropic path a base URL already ending in `/v1/messages` (switch
        // off) must be treated as a full endpoint by the real `base_url_is_full_endpoint`.

        // Without the guard, build_url would concatenate the pasted endpoint with the
        // rewritten `/v1/messages` target, producing a broken double suffix.
        use crate::proxy::providers::ProviderAdapter;
        let doubled = crate::proxy::providers::CodexAdapter::new()
            .build_url("https://host.example/v1/messages", "/v1/messages");
        assert_eq!(doubled, "https://host.example/v1/messages/v1/messages");

        // With the guard, the pasted URL is used verbatim (plus preserved query). Includes
        // query/fragment/whitespace suffixes, which must not hide the endpoint (fix: a base
        // like `.../v1/messages?beta=true` previously evaded the suffix check).
        for base in [
            "https://host.example/v1/messages",
            "https://host.example/v1/messages/",
            "https://host.example/api/v1/messages", // prefixed gateway
            "https://host.example/v1/messages?beta=true",
            "https://host.example/v1/messages/?beta=true",
            "https://host.example/v1/messages#frag",
            "  https://host.example/v1/messages  ",
        ] {
            assert!(
                base_url_is_full_endpoint(base, "/v1/messages"),
                "expected full-endpoint match: {base:?}"
            );
        }
        assert_eq!(
            append_query_to_full_url("https://host.example/v1/messages", Some("x=1")),
            "https://host.example/v1/messages?x=1"
        );
        // A base URL that already carries its own query is preserved verbatim (no double
        // `/v1/messages`, query kept).
        assert_eq!(
            append_query_to_full_url("https://host.example/v1/messages?beta=true", None),
            "https://host.example/v1/messages?beta=true"
        );

        // A non-endpoint base (origin/prefix) must NOT match, so build_url still appends.
        assert!(!base_url_is_full_endpoint(
            "https://host.example",
            "/v1/messages"
        ));
        assert!(!base_url_is_full_endpoint(
            "https://host.example/v1",
            "/v1/messages"
        ));
        // The shared helper also backs the Chat path's `/chat/completions` guard.
        assert!(base_url_is_full_endpoint(
            "https://host.example/v1/chat/completions?api-version=2024",
            "/chat/completions"
        ));
    }

    #[test]
    fn rewrite_claude_transform_endpoint_uses_copilot_path() {
        let (endpoint, passthrough_query) = rewrite_claude_transform_endpoint(
            "/v1/messages?beta=true&x-id=1",
            ClaudeApiFormat::Anthropic,
            true,
            &json!({ "model": "claude-sonnet-4-6" }),
        );

        assert_eq!(endpoint, "/chat/completions?x-id=1");
        assert_eq!(passthrough_query.as_deref(), Some("x-id=1"));
    }

    #[test]
    fn rewrite_claude_transform_endpoint_uses_copilot_responses_path() {
        let (endpoint, passthrough_query) = rewrite_claude_transform_endpoint(
            "/v1/messages?beta=true&x-id=1",
            ClaudeApiFormat::OpenAiResponses,
            true,
            &json!({ "model": "gpt-5.4" }),
        );

        assert_eq!(endpoint, "/v1/responses?x-id=1");
        assert_eq!(passthrough_query.as_deref(), Some("x-id=1"));
    }

    #[test]
    fn rewrite_claude_transform_endpoint_maps_gemini_generate_content() {
        let (endpoint, passthrough_query) = rewrite_claude_transform_endpoint(
            "/v1/messages?beta=true&x-id=1",
            ClaudeApiFormat::GeminiNative,
            false,
            &json!({ "model": "gemini-2.5-pro" }),
        );

        assert_eq!(
            endpoint,
            "/v1beta/models/gemini-2.5-pro:generateContent?x-id=1"
        );
        assert_eq!(passthrough_query.as_deref(), Some("x-id=1"));
    }

    /// Regression: body.model arriving as the resource-name form
    /// `models/gemini-2.5-pro` must not produce a doubled
    /// `/v1beta/models/models/...` path.
    #[test]
    fn rewrite_claude_transform_endpoint_strips_gemini_model_resource_prefix() {
        let (endpoint, _) = rewrite_claude_transform_endpoint(
            "/v1/messages",
            ClaudeApiFormat::GeminiNative,
            false,
            &json!({ "model": "models/gemini-2.5-pro" }),
        );

        assert_eq!(endpoint, "/v1beta/models/gemini-2.5-pro:generateContent");
    }

    #[test]
    fn rewrite_claude_transform_endpoint_maps_gemini_streaming() {
        let (endpoint, passthrough_query) = rewrite_claude_transform_endpoint(
            "/v1/messages?beta=true",
            ClaudeApiFormat::GeminiNative,
            false,
            &json!({ "model": "gemini-2.5-flash", "stream": true }),
        );

        assert_eq!(
            endpoint,
            "/v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse"
        );
        assert_eq!(passthrough_query.as_deref(), Some("alt=sse"));
    }

    #[test]
    fn append_query_to_full_url_preserves_existing_query_string() {
        let url = append_query_to_full_url("https://relay.example/api?foo=bar", Some("x-id=1"));

        assert_eq!(url, "https://relay.example/api?foo=bar&x-id=1");
    }

    #[test]
    fn build_gemini_native_url_uses_origin_when_base_ends_with_v1beta() {
        let url = crate::proxy::gemini_url::build_gemini_native_url(
            "https://generativelanguage.googleapis.com/v1beta",
            "/v1beta/models/gemini-2.5-pro:generateContent",
        );

        assert_eq!(
            url,
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-pro:generateContent"
        );
    }

    #[test]
    fn build_gemini_native_url_uses_origin_when_base_already_contains_models_prefix() {
        let url = crate::proxy::gemini_url::build_gemini_native_url(
            "https://generativelanguage.googleapis.com/v1beta/models",
            "/v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse",
        );

        assert_eq!(
            url,
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse"
        );
    }

    #[test]
    fn resolve_gemini_native_url_keeps_opaque_full_url_as_is() {
        let url = crate::proxy::gemini_url::resolve_gemini_native_url(
            "https://relay.example/custom/generate-content",
            "/v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse",
            true,
        );

        assert_eq!(url, "https://relay.example/custom/generate-content?alt=sse");
    }
}
