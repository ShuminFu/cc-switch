//! 上游认证
//!
//! 把 adapter 提取的认证信息解析成上游认证头：托管账号（GitHub Copilot、Codex OAuth）
//! 的真实 token 在这里经 AppHandle 从对应的 AuthManager 获取。

use crate::commands::{CodexOAuthState, CopilotAuthState};
use crate::provider::Provider;
use crate::proxy::error::ProxyError;
use crate::proxy::providers::codex_oauth_auth::CodexOAuthManager;
use crate::proxy::providers::copilot_auth::CopilotAuthManager;
use crate::proxy::providers::{AuthInfo, AuthStrategy, ProviderAdapter};
use http::{HeaderName, HeaderValue};
use tauri::Manager;

/// 解析后的上游认证
pub(crate) struct ResolvedAuth {
    /// 认证头（含 Codex OAuth 的 ChatGPT-Account-Id）
    pub headers: Vec<(HeaderName, HeaderValue)>,
    /// 使用了 Codex OAuth token：可附带 Codex 会话路由头
    pub codex_oauth: bool,
}

/// Codex 官方供应商的认证来自 Codex 客户端本身，必须是真实的 ChatGPT 登录凭据
pub(crate) fn validate_codex_official_authorization(
    headers: &http::HeaderMap,
) -> Result<(), ProxyError> {
    let authorization = headers
        .get(http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .map(str::trim);
    match authorization {
        None | Some("") => Err(ProxyError::AuthError(
            "Codex 官方登录不可用，请先在 Codex 中完成 ChatGPT 登录".to_string(),
        )),
        Some(value) if crate::proxy::inbound_auth::contains_proxy_token(value) => {
            Err(ProxyError::AuthError(
                "已切换到 OpenAI 官方供应商，请重启 Codex 或新建会话以加载官方登录配置".to_string(),
            ))
        }
        Some(_) => Ok(()),
    }
}

pub(crate) async fn resolve(
    app_handle: Option<&tauri::AppHandle>,
    adapter: &dyn ProviderAdapter,
    provider: &Provider,
    auth_info: Option<AuthInfo>,
) -> Result<ResolvedAuth, ProxyError> {
    let Some(mut auth) = auth_info else {
        return Ok(ResolvedAuth {
            headers: Vec::new(),
            codex_oauth: false,
        });
    };

    let mut codex_oauth_account_id = None;
    match auth.strategy {
        // GitHub Copilot 特殊处理：从 CopilotAuthManager 获取真实 token
        AuthStrategy::GitHubCopilot => {
            auth = copilot_token(app_handle, provider).await?;
        }
        // Codex OAuth 特殊处理：从 CodexOAuthManager 获取真实 access_token
        AuthStrategy::CodexOAuth => {
            let (token, account_id) = codex_oauth_token(app_handle, provider).await?;
            auth = token;
            codex_oauth_account_id = Some(account_id);
        }
        _ => {}
    }

    let mut headers = adapter.get_auth_headers(&auth)?;

    // 注入 Codex OAuth 的 ChatGPT-Account-Id header（如果有 account_id）
    if let Some(Some(ref account_id)) = codex_oauth_account_id {
        if let Ok(hv) = HeaderValue::from_str(account_id) {
            headers.push((HeaderName::from_static("chatgpt-account-id"), hv));
        }
    }

    Ok(ResolvedAuth {
        headers,
        codex_oauth: codex_oauth_account_id.is_some(),
    })
}

async fn copilot_token(
    app_handle: Option<&tauri::AppHandle>,
    provider: &Provider,
) -> Result<AuthInfo, ProxyError> {
    let Some(app_handle) = app_handle else {
        log::error!("[Copilot] AppHandle 不可用");
        return Err(ProxyError::AuthError(
            "GitHub Copilot 认证不可用（无 AppHandle）".to_string(),
        ));
    };
    let copilot_state = app_handle.state::<CopilotAuthState>();
    // 克隆管理器后立即释放外层 guard，避免联网刷新期间阻塞 UI 写锁
    let copilot_auth: CopilotAuthManager = copilot_state.manager().await;

    // 从 provider.meta 获取关联的 GitHub 账号 ID（多账号支持）
    let account_id = provider
        .meta
        .as_ref()
        .and_then(|m| m.managed_account_id_for("github_copilot"));

    // 根据账号 ID 获取对应 token（向后兼容：无账号 ID 时使用第一个账号）
    let token_result = match &account_id {
        Some(id) => {
            log::debug!("[Copilot] 使用指定账号 {id} 获取 token");
            copilot_auth.get_valid_token_for_account(id).await
        }
        None => {
            log::debug!("[Copilot] 使用默认账号获取 token");
            copilot_auth.get_valid_token().await
        }
    };

    match token_result {
        Ok(token) => {
            log::debug!(
                "[Copilot] 成功获取 Copilot token (account={})",
                account_id.as_deref().unwrap_or("default")
            );
            Ok(AuthInfo::new(token, AuthStrategy::GitHubCopilot))
        }
        Err(e) => {
            log::error!(
                "[Copilot] 获取 Copilot token 失败 (account={}): {e}",
                account_id.as_deref().unwrap_or("default")
            );
            Err(ProxyError::AuthError(format!(
                "GitHub Copilot 认证失败: {e}"
            )))
        }
    }
}

/// 返回 access_token 与实际使用的 ChatGPT 账号 ID（用于注入 ChatGPT-Account-Id header）
async fn codex_oauth_token(
    app_handle: Option<&tauri::AppHandle>,
    provider: &Provider,
) -> Result<(AuthInfo, Option<String>), ProxyError> {
    let Some(app_handle) = app_handle else {
        log::error!("[CodexOAuth] AppHandle 不可用");
        return Err(ProxyError::AuthError(
            "Codex OAuth 认证不可用（无 AppHandle）".to_string(),
        ));
    };
    let codex_state = app_handle.state::<CodexOAuthState>();
    // 克隆管理器后立即释放外层 guard，避免联网刷新期间阻塞 UI 写锁
    let codex_auth: CodexOAuthManager = codex_state.manager().await;

    // 从 provider.meta 获取关联的 ChatGPT 账号 ID
    let account_id = provider
        .meta
        .as_ref()
        .and_then(|m| m.managed_account_id_for("codex_oauth"));

    let token_result = match &account_id {
        Some(id) => {
            log::debug!("[CodexOAuth] 使用指定账号 {id} 获取 token");
            codex_auth.get_valid_token_for_account(id).await
        }
        None => {
            log::debug!("[CodexOAuth] 使用默认账号获取 token");
            codex_auth.get_valid_token().await
        }
    };

    match token_result {
        Ok(token) => {
            let account_id = match account_id {
                Some(id) => Some(id),
                None => codex_auth.default_account_id().await,
            };
            log::debug!(
                "[CodexOAuth] 成功获取 access_token (account={})",
                account_id.as_deref().unwrap_or("default")
            );
            Ok((AuthInfo::new(token, AuthStrategy::CodexOAuth), account_id))
        }
        Err(e) => {
            log::error!("[CodexOAuth] 获取 access_token 失败: {e}");
            Err(ProxyError::AuthError(format!("Codex OAuth 认证失败: {e}")))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use http::HeaderMap;

    #[test]
    fn official_codex_rejects_stale_proxy_placeholder_with_restart_hint() {
        let mut headers = HeaderMap::new();
        headers.insert(
            http::header::AUTHORIZATION,
            http::HeaderValue::from_static("Bearer PROXY_MANAGED"),
        );
        let error = validate_codex_official_authorization(&headers)
            .expect_err("stale placeholder must be rejected");
        assert!(matches!(error, ProxyError::AuthError(message) if message.contains("重启 Codex")));

        let mut headers = HeaderMap::new();
        headers.insert(
            http::header::AUTHORIZATION,
            http::HeaderValue::from_static("Bearer ccs-proxy-0123456789abcdef0123456789abcdef"),
        );
        let error = validate_codex_official_authorization(&headers)
            .expect_err("stale per-install proxy token must be rejected");
        assert!(matches!(error, ProxyError::AuthError(message) if message.contains("重启 Codex")));
    }
}
