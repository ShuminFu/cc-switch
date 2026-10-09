//! Remote configuration fetching for deep links (`configUrl` parameter).
//!
//! A deep link may point at a hosted config document instead of embedding it
//! as Base64. The document is fetched when the link is opened and folded into
//! the inline `config` field, so the rest of the pipeline
//! (`parse_and_merge_config`, the preview dialog, the import commands) is
//! unchanged. Inline `config` always takes precedence over `configUrl`.

use super::DeepLinkImportRequest;
use crate::error::AppError;
use base64::prelude::*;
use std::time::Duration;
use url::{Host, Url};

/// Upper bound for a remote config body; provider configs are a few KB.
pub const MAX_REMOTE_CONFIG_BYTES: usize = 256 * 1024;
const REMOTE_CONFIG_TIMEOUT_SECS: u64 = 15;

fn is_loopback_host(url: &Url) -> bool {
    match url.host() {
        Some(Host::Domain(domain)) => domain.eq_ignore_ascii_case("localhost"),
        Some(Host::Ipv4(ip)) => ip.is_loopback(),
        Some(Host::Ipv6(ip)) => ip.is_loopback(),
        None => false,
    }
}

/// Validate a `configUrl`.
///
/// Deep links can be triggered by any web page, so the fetch is restricted to
/// HTTPS (plain HTTP only for loopback hosts) and to URLs without embedded
/// credentials.
pub fn validate_remote_config_url(raw: &str) -> Result<Url, AppError> {
    let url = Url::parse(raw.trim())
        .map_err(|e| AppError::InvalidInput(format!("Invalid configUrl: {e}")))?;
    match url.scheme() {
        "https" => {}
        "http" if is_loopback_host(&url) => {}
        "http" => {
            return Err(AppError::InvalidInput(
                "configUrl must use https (plain http is only allowed for localhost)".to_string(),
            ))
        }
        other => {
            return Err(AppError::InvalidInput(format!(
                "Invalid configUrl scheme '{other}': must be https"
            )))
        }
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(AppError::InvalidInput(
            "configUrl must not contain credentials".to_string(),
        ));
    }
    if url.host().is_none() {
        return Err(AppError::InvalidInput(
            "configUrl must include a host".to_string(),
        ));
    }
    Ok(url)
}

/// Decide how to parse the fetched body: an explicit `configFormat` wins, then
/// the URL path extension, then the response Content-Type; JSON otherwise
/// (the same default as `parse_and_merge_config`).
pub fn infer_remote_config_format(
    explicit: Option<&str>,
    url: &Url,
    content_type: Option<&str>,
) -> String {
    if let Some(format) = explicit.map(str::trim).filter(|s| !s.is_empty()) {
        return format.to_ascii_lowercase();
    }
    let path = url.path().to_ascii_lowercase();
    if path.ends_with(".toml") {
        return "toml".to_string();
    }
    if path.ends_with(".json") {
        return "json".to_string();
    }
    if content_type
        .map(str::to_ascii_lowercase)
        .is_some_and(|ct| ct.contains("toml"))
    {
        return "toml".to_string();
    }
    "json".to_string()
}

fn remote_config_client(url: &Url) -> Result<reqwest::Client, AppError> {
    if is_loopback_host(url) {
        // A localhost document must never be routed through the global or
        // system proxy.
        reqwest::Client::builder()
            .no_proxy()
            .build()
            .map_err(|e| AppError::Message(format!("Failed to build HTTP client: {e}")))
    } else {
        Ok(crate::proxy::http_client::get())
    }
}

fn too_large() -> AppError {
    AppError::InvalidInput(format!(
        "configUrl document exceeds {} KB",
        MAX_REMOTE_CONFIG_BYTES / 1024
    ))
}

async fn fetch_remote_config(url: &Url) -> Result<(String, Option<String>), AppError> {
    let client = remote_config_client(url)?;
    let mut response = client
        .get(url.clone())
        .timeout(Duration::from_secs(REMOTE_CONFIG_TIMEOUT_SECS))
        .header(
            reqwest::header::ACCEPT,
            "application/json, application/toml, text/plain;q=0.9, */*;q=0.5",
        )
        .send()
        .await
        .map_err(|e| AppError::Message(format!("Failed to fetch configUrl: {e}")))?;

    // Redirect targets must satisfy the same policy as the original URL.
    validate_remote_config_url(response.url().as_str())?;

    let status = response.status();
    if !status.is_success() {
        return Err(AppError::Message(format!(
            "configUrl returned HTTP {status}"
        )));
    }
    if response
        .content_length()
        .is_some_and(|len| len > MAX_REMOTE_CONFIG_BYTES as u64)
    {
        return Err(too_large());
    }
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);

    let mut body: Vec<u8> = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| AppError::Message(format!("Failed to read configUrl body: {e}")))?
    {
        if body.len() + chunk.len() > MAX_REMOTE_CONFIG_BYTES {
            return Err(too_large());
        }
        body.extend_from_slice(&chunk);
    }
    let text = String::from_utf8(body)
        .map_err(|e| AppError::InvalidInput(format!("configUrl body is not valid UTF-8: {e}")))?;
    if text.trim().is_empty() {
        return Err(AppError::InvalidInput(
            "configUrl returned an empty document".to_string(),
        ));
    }
    Ok((text, content_type))
}

/// Fold a remote `configUrl` into the inline `config` field.
///
/// Returns the request unchanged when it already carries an inline config or
/// has no `configUrl`. The resolved request can be passed to
/// `parse_and_merge_config` and the import commands like any inline-config
/// request.
pub async fn resolve_remote_config(
    request: &DeepLinkImportRequest,
) -> Result<DeepLinkImportRequest, AppError> {
    if request
        .config
        .as_deref()
        .is_some_and(|config| !config.trim().is_empty())
    {
        return Ok(request.clone());
    }
    let Some(config_url) = request
        .config_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(request.clone());
    };

    let url = validate_remote_config_url(config_url)?;
    log::info!(
        "Fetching remote deep link config from {}",
        url.host_str().unwrap_or("<unknown host>")
    );
    let (body, content_type) = fetch_remote_config(&url).await?;
    let format = infer_remote_config_format(
        request.config_format.as_deref(),
        &url,
        content_type.as_deref(),
    );

    let mut resolved = request.clone();
    resolved.config = Some(BASE64_STANDARD.encode(body.as_bytes()));
    resolved.config_format = Some(format);
    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::super::provider::parse_and_merge_config;
    use super::*;
    use axum::http::header::CONTENT_TYPE;
    use axum::routing::get;
    use axum::Router;

    async fn serve(router: Router) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind loopback listener");
        let addr = listener.local_addr().expect("local addr");
        tokio::spawn(async move {
            axum::serve(listener, router)
                .await
                .expect("serve test router");
        });
        format!("http://{addr}")
    }

    fn provider_request(app: &str, config_url: String) -> DeepLinkImportRequest {
        DeepLinkImportRequest {
            version: "v1".to_string(),
            resource: "provider".to_string(),
            app: Some(app.to_string()),
            name: Some("Remote".to_string()),
            config_url: Some(config_url),
            ..Default::default()
        }
    }

    #[test]
    fn validates_scheme_host_and_credentials() {
        assert!(validate_remote_config_url("https://relay.example/cfg.json").is_ok());
        assert!(validate_remote_config_url("http://localhost:8080/cfg.json").is_ok());
        assert!(validate_remote_config_url("http://127.0.0.1:8080/cfg.json").is_ok());
        assert!(validate_remote_config_url("http://[::1]:8080/cfg.json").is_ok());

        let plain_http = validate_remote_config_url("http://relay.example/cfg.json")
            .expect_err("plain http on a remote host must be rejected");
        assert!(plain_http.to_string().contains("https"));
        assert!(validate_remote_config_url("ftp://relay.example/cfg.json").is_err());
        assert!(validate_remote_config_url("https://user:pw@relay.example/cfg.json").is_err());
        assert!(validate_remote_config_url("not a url").is_err());
    }

    #[test]
    fn infers_format_from_param_extension_then_content_type() {
        let toml_url = Url::parse("https://relay.example/provider.TOML").unwrap();
        let json_url = Url::parse("https://relay.example/provider.json").unwrap();
        let bare_url = Url::parse("https://relay.example/provider").unwrap();

        assert_eq!(
            infer_remote_config_format(Some("JSON"), &toml_url, None),
            "json"
        );
        assert_eq!(infer_remote_config_format(None, &toml_url, None), "toml");
        assert_eq!(
            infer_remote_config_format(None, &json_url, Some("application/toml")),
            "json"
        );
        assert_eq!(
            infer_remote_config_format(None, &bare_url, Some("application/toml; charset=utf-8")),
            "toml"
        );
        assert_eq!(infer_remote_config_format(None, &bare_url, None), "json");
    }

    #[tokio::test]
    async fn leaves_requests_without_remote_config_untouched() {
        let inline = DeepLinkImportRequest {
            config: Some("eyJlbnYiOnt9fQ==".to_string()),
            config_url: Some("https://relay.example/never-fetched.json".to_string()),
            ..Default::default()
        };
        let resolved = resolve_remote_config(&inline).await.expect("inline wins");
        assert_eq!(resolved.config, inline.config);

        let none = DeepLinkImportRequest::default();
        let resolved = resolve_remote_config(&none).await.expect("nothing to do");
        assert!(resolved.config.is_none());
    }

    #[tokio::test]
    async fn resolves_json_document_and_merges_claude_fields() {
        let router = Router::new().route(
            "/claude.json",
            get(|| async {
                (
                    [(CONTENT_TYPE, "application/json")],
                    r#"{"env":{"ANTHROPIC_AUTH_TOKEN":"sk-remote","ANTHROPIC_BASE_URL":"https://relay.example/v1"}}"#,
                )
            }),
        );
        let base = serve(router).await;

        let request = provider_request("claude", format!("{base}/claude.json"));
        let resolved = resolve_remote_config(&request)
            .await
            .expect("resolve remote config");
        assert_eq!(resolved.config_format.as_deref(), Some("json"));
        assert!(resolved.config.is_some());

        let merged = parse_and_merge_config(&resolved).expect("merge resolved config");
        assert_eq!(merged.api_key.as_deref(), Some("sk-remote"));
        assert_eq!(merged.endpoint.as_deref(), Some("https://relay.example/v1"));
    }

    #[tokio::test]
    async fn infers_toml_from_extension_and_merges_codex_fields() {
        let document = r#"config = """
model = "gpt-5-remote"
model_provider = "relay"

[model_providers.relay]
name = "Relay"
base_url = "https://codex-relay.example/v1"
wire_api = "responses"
"""

[auth]
OPENAI_API_KEY = "sk-remote-codex"
"#;
        let router = Router::new().route(
            "/codex.toml",
            get(move || async move { ([(CONTENT_TYPE, "text/plain")], document) }),
        );
        let base = serve(router).await;

        let request = provider_request("codex", format!("{base}/codex.toml"));
        let resolved = resolve_remote_config(&request)
            .await
            .expect("resolve remote config");
        assert_eq!(resolved.config_format.as_deref(), Some("toml"));

        let merged = parse_and_merge_config(&resolved).expect("merge resolved config");
        assert_eq!(merged.api_key.as_deref(), Some("sk-remote-codex"));
        assert_eq!(
            merged.endpoint.as_deref(),
            Some("https://codex-relay.example/v1")
        );
        assert_eq!(merged.model.as_deref(), Some("gpt-5-remote"));
    }

    #[tokio::test]
    async fn rejects_oversized_and_failed_documents() {
        let router = Router::new()
            .route(
                "/huge.json",
                get(|| async { "x".repeat(MAX_REMOTE_CONFIG_BYTES + 1) }),
            )
            .route(
                "/missing.json",
                get(|| async { (axum::http::StatusCode::NOT_FOUND, "nope") }),
            )
            .route("/empty.json", get(|| async { "   " }));
        let base = serve(router).await;

        let huge = resolve_remote_config(&provider_request("claude", format!("{base}/huge.json")))
            .await
            .expect_err("oversized document must be rejected");
        assert!(huge.to_string().contains("exceeds"));

        let missing =
            resolve_remote_config(&provider_request("claude", format!("{base}/missing.json")))
                .await
                .expect_err("404 must be rejected");
        assert!(missing.to_string().contains("404"));

        let empty =
            resolve_remote_config(&provider_request("claude", format!("{base}/empty.json")))
                .await
                .expect_err("empty document must be rejected");
        assert!(empty.to_string().contains("empty"));
    }
}
