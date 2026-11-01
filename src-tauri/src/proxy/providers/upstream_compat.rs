//! Native quirks of OpenAI-Chat-compatible upstreams (PROV-23, first increment).
//!
//! Azure OpenAI, Mistral and Ollama all speak OpenAI Chat Completions and are
//! reached through the existing `openai_chat` bridge, but each deviates in a
//! way that turns into a 400/401 or silently wrong behaviour:
//!
//! * **Azure OpenAI** authenticates with an `api-key` header (Entra ID access
//!   tokens still use `Authorization: Bearer`). The deployment URL and the
//!   `api-version` query come from the provider's base URL in "full URL" mode,
//!   e.g. `https://res.openai.azure.com/openai/deployments/gpt-4o/chat/completions?api-version=2024-10-21`.
//! * **Mistral** (`api.mistral.ai`, `codestral.mistral.ai`) rejects unknown
//!   fields (`extra_forbidden`), names the seed `random_seed` and has no
//!   `max_completion_tokens`, `stream_options` or `reasoning_effort`.
//! * **Ollama** (`:11434`) ignores `max_completion_tokens`, so the output cap
//!   the client asked for is lost unless it is sent as `max_tokens`.
//!
//! The flavor is detected from the upstream URL so existing providers need no
//! new configuration. Bedrock (SigV4 + event-stream framing) and Vertex
//! (service-account OAuth) need cloud credential flows and are not covered by
//! this increment.

use http::{HeaderName, HeaderValue};
use serde_json::Value;

/// An OpenAI-compatible upstream with known deviations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UpstreamFlavor {
    AzureOpenAi,
    Mistral,
    Ollama,
    Generic,
}

fn host_and_port(url: &str) -> Option<(String, Option<u16>)> {
    let parsed = url::Url::parse(url).ok()?;
    Some((parsed.host_str()?.to_ascii_lowercase(), parsed.port()))
}

impl UpstreamFlavor {
    pub(crate) fn detect(url: &str) -> Self {
        let Some((host, port)) = host_and_port(url) else {
            return Self::Generic;
        };
        if host.ends_with(".openai.azure.com")
            || host.ends_with(".cognitiveservices.azure.com")
            || host.ends_with(".services.ai.azure.com")
        {
            Self::AzureOpenAi
        } else if host == "api.mistral.ai" || host == "codestral.mistral.ai" {
            Self::Mistral
        } else if port == Some(11434) || host == "ollama" || host.starts_with("ollama.") {
            Self::Ollama
        } else {
            Self::Generic
        }
    }
}

/// Fields Mistral's Chat Completions API accepts; anything else is rejected
/// with `extra_forbidden`.
const MISTRAL_ALLOWED_FIELDS: &[&str] = &[
    "model",
    "messages",
    "temperature",
    "top_p",
    "max_tokens",
    "stream",
    "stop",
    "random_seed",
    "response_format",
    "tools",
    "tool_choice",
    "presence_penalty",
    "frequency_penalty",
    "n",
    "prediction",
    "parallel_tool_calls",
    "safe_prompt",
];

fn rename_if_absent(obj: &mut serde_json::Map<String, Value>, from: &str, to: &str) {
    if let Some(value) = obj.remove(from) {
        obj.entry(to.to_string()).or_insert(value);
    }
}

/// Adjust an outbound OpenAI Chat request body for `flavor`.
pub(crate) fn apply_chat_body_compat(flavor: UpstreamFlavor, body: &mut Value) {
    let Some(obj) = body.as_object_mut() else {
        return;
    };
    match flavor {
        UpstreamFlavor::Mistral => {
            rename_if_absent(obj, "max_completion_tokens", "max_tokens");
            rename_if_absent(obj, "seed", "random_seed");
            let dropped: Vec<String> = obj
                .keys()
                .filter(|k| !MISTRAL_ALLOWED_FIELDS.contains(&k.as_str()))
                .cloned()
                .collect();
            for key in &dropped {
                obj.remove(key);
            }
            if !dropped.is_empty() {
                log::debug!("[Mistral] dropped unsupported fields: {dropped:?}");
            }
        }
        UpstreamFlavor::Ollama => {
            rename_if_absent(obj, "max_completion_tokens", "max_tokens");
        }
        UpstreamFlavor::AzureOpenAi | UpstreamFlavor::Generic => {}
    }
}

/// Azure OpenAI keys go in `api-key`; a JWT (Entra ID token) stays a bearer.
pub(crate) fn apply_auth_header_compat(
    flavor: UpstreamFlavor,
    headers: &mut Vec<(HeaderName, HeaderValue)>,
) {
    if flavor != UpstreamFlavor::AzureOpenAi {
        return;
    }
    let Some(pos) = headers
        .iter()
        .position(|(name, _)| name == http::header::AUTHORIZATION)
    else {
        return;
    };
    let Some(key) = headers[pos]
        .1
        .to_str()
        .ok()
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::trim)
        .map(str::to_string)
    else {
        return;
    };
    if key.starts_with("eyJ") {
        return; // Entra ID access token
    }
    if let Ok(mut value) = HeaderValue::from_str(&key) {
        value.set_sensitive(true);
        headers.remove(pos);
        headers.push((HeaderName::from_static("api-key"), value));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proxy::testing::{assert_golden, fixtures_root};

    #[test]
    fn detects_flavors_from_url() {
        for (url, flavor) in [
            (
                "https://res.openai.azure.com/openai/deployments/gpt/chat/completions?api-version=2024-10-21",
                UpstreamFlavor::AzureOpenAi,
            ),
            ("https://x.cognitiveservices.azure.com/openai/v1", UpstreamFlavor::AzureOpenAi),
            ("https://api.mistral.ai/v1/chat/completions", UpstreamFlavor::Mistral),
            ("http://127.0.0.1:11434/v1/chat/completions", UpstreamFlavor::Ollama),
            ("http://ollama:8080/v1", UpstreamFlavor::Ollama),
            ("https://api.openai.com/v1/chat/completions", UpstreamFlavor::Generic),
            ("https://evil.openai.azure.com.example.org/v1", UpstreamFlavor::Generic),
            ("not a url", UpstreamFlavor::Generic),
        ] {
            assert_eq!(UpstreamFlavor::detect(url), flavor, "{url}");
        }
    }

    #[test]
    fn azure_uses_api_key_header_but_keeps_entra_bearer() {
        let mut headers = vec![(
            http::header::AUTHORIZATION,
            HeaderValue::from_static("Bearer azure-key-123"),
        )];
        apply_auth_header_compat(UpstreamFlavor::AzureOpenAi, &mut headers);
        assert_eq!(headers.len(), 1);
        assert_eq!(headers[0].0.as_str(), "api-key");
        assert_eq!(headers[0].1, "azure-key-123");

        let mut entra = vec![(
            http::header::AUTHORIZATION,
            HeaderValue::from_static("Bearer eyJhbGciOi.payload.sig"),
        )];
        apply_auth_header_compat(UpstreamFlavor::AzureOpenAi, &mut entra);
        assert_eq!(entra[0].0, http::header::AUTHORIZATION);

        let mut generic = vec![(
            http::header::AUTHORIZATION,
            HeaderValue::from_static("Bearer sk"),
        )];
        apply_auth_header_compat(UpstreamFlavor::Generic, &mut generic);
        assert_eq!(generic[0].0, http::header::AUTHORIZATION);
    }

    /// Golden fixtures: an Anthropic request goes through the Claude -> Chat
    /// bridge, then the flavor's compat pass.
    #[test]
    fn golden_bridge_requests_per_flavor() {
        // `*.source.json`, not `*.input.json`: these cases are not part of the
        // generic bridge corpus checked by `corpus_is_fully_wired`.
        let dir = fixtures_root().join("upstream_compat").join("request");
        let input: Value = serde_json::from_str(
            &std::fs::read_to_string(dir.join("anthropic.source.json")).expect("input fixture"),
        )
        .unwrap();
        for (name, flavor) in [
            ("mistral", UpstreamFlavor::Mistral),
            ("ollama", UpstreamFlavor::Ollama),
            ("azure", UpstreamFlavor::AzureOpenAi),
        ] {
            let mut chat = crate::proxy::providers::transform::anthropic_to_openai(input.clone())
                .expect("bridge request");
            // Clients such as Codex send these; the compat pass must handle them.
            chat["max_completion_tokens"] = chat
                .as_object_mut()
                .and_then(|o| o.remove("max_tokens"))
                .unwrap_or(Value::Null);
            chat["seed"] = serde_json::json!(7);
            chat["reasoning_effort"] = serde_json::json!("low");
            apply_chat_body_compat(flavor, &mut chat);
            assert_golden(&dir.join(format!("{name}.expected.json")), &chat, false);
        }
    }
}
