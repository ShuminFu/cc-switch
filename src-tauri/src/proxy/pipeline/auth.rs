//! Auth stage: turn the provider's credential into upstream auth headers,
//! resolving managed-account tokens (GitHub Copilot, Codex OAuth) through the
//! app's auth managers when an `AppHandle` is available.

use crate::commands::{CodexOAuthState, CopilotAuthState};
use crate::provider::Provider;
use crate::proxy::providers::{AuthInfo, AuthStrategy, ProviderAdapter};
use crate::proxy::ProxyError;
use http::{HeaderName, HeaderValue};
use tauri::Manager;

/// Upstream credentials for one attempt.
#[derive(Debug, Default)]
pub struct ResolvedAuth {
    pub headers: Vec<(HeaderName, HeaderValue)>,
    /// A Codex OAuth token was resolved: the request goes to the ChatGPT backend
    /// and should carry the client's session routing headers.
    pub codex_oauth: bool,
}

/// Resolve the auth headers for `provider`.
///
/// Managed strategies need the auth managers held in Tauri state; without an
/// `AppHandle` (tests, headless runs) they fail with an `AuthError`, which the
/// retry loop treats as provider-specific.
pub async fn resolve_auth(
    adapter: &dyn ProviderAdapter,
    provider: &Provider,
    app_handle: Option<&tauri::AppHandle>,
) -> Result<ResolvedAuth, ProxyError> {
    let Some(mut auth) = adapter.extract_auth(provider) else {
        return Ok(ResolvedAuth::default());
    };
    let mut codex_oauth = false;
    let mut account_header = None;

    match auth.strategy {
        AuthStrategy::GitHubCopilot => {
            let app = app_handle.ok_or_else(|| {
                ProxyError::AuthError("GitHub Copilot auth unavailable (no app handle)".to_string())
            })?;
            let token = copilot_token(app, provider).await?;
            auth = AuthInfo::new(token, AuthStrategy::GitHubCopilot);
        }
        AuthStrategy::CodexOAuth => {
            let app = app_handle.ok_or_else(|| {
                ProxyError::AuthError("Codex OAuth unavailable (no app handle)".to_string())
            })?;
            let (token, account_id) = codex_oauth_token(app, provider).await?;
            auth = AuthInfo::new(token, AuthStrategy::CodexOAuth);
            codex_oauth = true;
            account_header = account_id;
        }
        _ => {}
    }

    let mut headers = adapter.get_auth_headers(&auth)?;
    if let Some(hv) = account_header.and_then(|id| HeaderValue::from_str(&id).ok()) {
        headers.push((HeaderName::from_static("chatgpt-account-id"), hv));
    }
    Ok(ResolvedAuth {
        headers,
        codex_oauth,
    })
}

async fn copilot_token(app: &tauri::AppHandle, provider: &Provider) -> Result<String, ProxyError> {
    let copilot_state = app.state::<CopilotAuthState>();
    let copilot_auth = copilot_state.0.read().await;
    // Multi-account: the provider names its GitHub account; otherwise the first one.
    let account_id = provider
        .meta
        .as_ref()
        .and_then(|m| m.managed_account_id_for("github_copilot"));
    let result = match &account_id {
        Some(id) => copilot_auth.get_valid_token_for_account(id).await,
        None => copilot_auth.get_valid_token().await,
    };
    result.map_err(|e| {
        log::error!(
            "[Copilot] token refresh failed (account={}): {e}",
            account_id.as_deref().unwrap_or("default")
        );
        ProxyError::AuthError(format!("GitHub Copilot authentication failed: {e}"))
    })
}

/// Returns the access token and the ChatGPT account id to send upstream.
async fn codex_oauth_token(
    app: &tauri::AppHandle,
    provider: &Provider,
) -> Result<(String, Option<String>), ProxyError> {
    let codex_state = app.state::<CodexOAuthState>();
    let codex_auth = codex_state.0.read().await;
    let account_id = provider
        .meta
        .as_ref()
        .and_then(|m| m.managed_account_id_for("codex_oauth"));
    let result = match &account_id {
        Some(id) => codex_auth.get_valid_token_for_account(id).await,
        None => codex_auth.get_valid_token().await,
    };
    let token = result.map_err(|e| {
        log::error!("[CodexOAuth] access token refresh failed: {e}");
        ProxyError::AuthError(format!("Codex OAuth authentication failed: {e}"))
    })?;
    let account_id = match account_id {
        Some(id) => Some(id),
        None => codex_auth.default_account_id().await,
    };
    Ok((token, account_id))
}

/// GitHub Copilot may serve an account from a non-default API endpoint
/// (enterprise); use the endpoint the auth manager discovered.
pub async fn copilot_base_url(
    app_handle: Option<&tauri::AppHandle>,
    provider: &Provider,
    base_url: String,
) -> String {
    let Some(app) = app_handle else {
        return base_url;
    };
    let copilot_state = app.state::<CopilotAuthState>();
    let copilot_auth = copilot_state.0.read().await;
    let account_id = provider
        .meta
        .as_ref()
        .and_then(|m| m.managed_account_id_for("github_copilot"));
    let dynamic_endpoint = match &account_id {
        Some(id) => copilot_auth.get_api_endpoint(id).await,
        None => copilot_auth.get_default_api_endpoint().await,
    };
    if dynamic_endpoint != base_url {
        log::debug!("[Copilot] using dynamic API endpoint: {dynamic_endpoint} (was {base_url})");
        return dynamic_endpoint;
    }
    base_url
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_config::AppType;
    use crate::proxy::providers::get_adapter;
    use serde_json::json;

    #[tokio::test]
    async fn api_key_providers_need_no_app_handle() {
        let provider = Provider::with_id(
            "p".into(),
            "P".into(),
            json!({"env": {"ANTHROPIC_BASE_URL": "https://h", "ANTHROPIC_AUTH_TOKEN": "sk-x"}}),
            None,
        );
        let adapter = get_adapter(&AppType::Claude);
        let auth = resolve_auth(adapter.as_ref(), &provider, None)
            .await
            .unwrap();
        assert!(!auth.codex_oauth);
        assert!(auth
            .headers
            .iter()
            .any(|(_, v)| v.to_str().unwrap_or("").contains("sk-x")));
    }

    #[tokio::test]
    async fn copilot_without_app_handle_is_an_auth_error() {
        let mut provider = Provider::with_id("p".into(), "P".into(), json!({}), None);
        provider.meta = Some(crate::provider::ProviderMeta {
            provider_type: Some("github_copilot".into()),
            ..Default::default()
        });
        let adapter = get_adapter(&AppType::Claude);
        if adapter
            .extract_auth(&provider)
            .is_some_and(|a| a.strategy == AuthStrategy::GitHubCopilot)
        {
            let err = resolve_auth(adapter.as_ref(), &provider, None)
                .await
                .unwrap_err();
            assert!(matches!(err, ProxyError::AuthError(_)));
        }
    }
}
