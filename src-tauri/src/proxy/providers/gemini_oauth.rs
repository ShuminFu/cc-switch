//! Gemini CLI OAuth refresh (PROV-08).
//!
//! A GeminiCli provider stores the content of `~/.gemini/oauth_creds.json`
//! (`access_token`, `refresh_token`, `client_id`, `client_secret`,
//! `expiry_date` in ms). Access tokens live about an hour; before this module
//! users had to re-run the gemini CLI and paste the file again. The manager
//! exchanges the refresh token at Google's token endpoint when the stored
//! access token is missing or expiring, caches the minted token in memory
//! (keyed by refresh token, so it never leaks across credentials) and
//! single-flights concurrent refreshes of the same credential.
//!
//! The provider's stored JSON is not rewritten: Google does not rotate
//! installed-app refresh tokens, and the cache survives for the process
//! lifetime, so every request after the first refresh is served from memory.

use super::gemini::{GeminiAdapter, OAuthCredentials};
use crate::proxy::ProxyError;
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use tokio::sync::{Mutex, RwLock};

const GOOGLE_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
/// Refresh this long before the access token expires.
const REFRESH_BUFFER_MS: i64 = 60_000;
const REFRESH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

#[derive(Debug, Clone)]
struct CachedToken {
    access_token: String,
    expires_at_ms: i64,
}

impl CachedToken {
    fn is_fresh(&self, now_ms: i64) -> bool {
        self.expires_at_ms - now_ms > REFRESH_BUFFER_MS
    }
}

/// Refreshes and caches Gemini CLI access tokens.
pub struct GeminiOAuthManager {
    token_url: String,
    cache: RwLock<HashMap<String, CachedToken>>,
    locks: Mutex<HashMap<String, Arc<Mutex<()>>>>,
}

#[derive(serde::Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    expires_in: Option<i64>,
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// `expiry_date` (ms) from an `oauth_creds.json` blob, if present.
fn expiry_date_ms(raw: &str) -> Option<i64> {
    let raw = raw.trim();
    if !raw.starts_with('{') {
        return None;
    }
    serde_json::from_str::<serde_json::Value>(raw)
        .ok()?
        .get("expiry_date")
        .and_then(|v| v.as_i64())
}

impl GeminiOAuthManager {
    pub fn new(token_url: impl Into<String>) -> Self {
        Self {
            token_url: token_url.into(),
            cache: RwLock::new(HashMap::new()),
            locks: Mutex::new(HashMap::new()),
        }
    }

    /// Process-wide manager used by the proxy auth stage.
    pub fn global() -> &'static GeminiOAuthManager {
        static MANAGER: OnceLock<GeminiOAuthManager> = OnceLock::new();
        MANAGER.get_or_init(|| GeminiOAuthManager::new(GOOGLE_TOKEN_URL))
    }

    /// Return a fresh access token for the stored credential `raw_key`, or
    /// `Ok(None)` when no refresh is needed or possible (the caller keeps the
    /// stored token / existing error path).
    pub async fn resolve(&self, raw_key: &str) -> Result<Option<String>, ProxyError> {
        let Some(creds) = GeminiAdapter::new().parse_oauth_credentials(raw_key) else {
            return Ok(None);
        };
        if !creds.can_refresh() {
            return Ok(None);
        }
        let now = now_ms();
        let stored_expiry = expiry_date_ms(raw_key);
        let stored_fresh = !creds.needs_refresh()
            && stored_expiry.is_none_or(|exp| exp - now > REFRESH_BUFFER_MS);
        if stored_fresh {
            return Ok(None);
        }
        self.refreshed_token(&creds, stored_expiry).await.map(Some)
    }

    async fn refreshed_token(
        &self,
        creds: &OAuthCredentials,
        stored_expiry: Option<i64>,
    ) -> Result<String, ProxyError> {
        let refresh_token = creds.refresh_token.clone().unwrap_or_default();
        if let Some(token) = self.cached(&refresh_token).await {
            return Ok(token);
        }

        let lock = {
            let mut locks = self.locks.lock().await;
            locks
                .entry(refresh_token.clone())
                .or_insert_with(|| Arc::new(Mutex::new(())))
                .clone()
        };
        let _guard = lock.lock().await;
        // Another request may have refreshed while we waited.
        if let Some(token) = self.cached(&refresh_token).await {
            return Ok(token);
        }

        match self.exchange(creds, &refresh_token).await {
            Ok(token) => Ok(token),
            Err(err) => {
                // Keep serving with the stored token while it is still valid.
                let still_valid = !creds.access_token.is_empty()
                    && stored_expiry.is_some_and(|exp| exp > now_ms());
                if still_valid {
                    log::warn!("[Gemini OAuth] refresh failed ({err}); using the stored access token");
                    Ok(creds.access_token.clone())
                } else {
                    Err(err)
                }
            }
        }
    }

    async fn cached(&self, refresh_token: &str) -> Option<String> {
        self.cache
            .read()
            .await
            .get(refresh_token)
            .filter(|t| t.is_fresh(now_ms()))
            .map(|t| t.access_token.clone())
    }

    async fn exchange(
        &self,
        creds: &OAuthCredentials,
        refresh_token: &str,
    ) -> Result<String, ProxyError> {
        let response = crate::proxy::http_client::get()
            .post(&self.token_url)
            .timeout(REFRESH_TIMEOUT)
            .form(&[
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh_token),
                ("client_id", creds.client_id.as_deref().unwrap_or_default()),
                (
                    "client_secret",
                    creds.client_secret.as_deref().unwrap_or_default(),
                ),
            ])
            .send()
            .await
            .map_err(|e| ProxyError::AuthError(format!("Gemini OAuth refresh failed: {e}")))?;

        let status = response.status();
        if status == reqwest::StatusCode::BAD_REQUEST || status == reqwest::StatusCode::UNAUTHORIZED
        {
            return Err(ProxyError::AuthError(
                "Gemini OAuth refresh_token was rejected; run the gemini CLI to log in again \
                 and paste the new ~/.gemini/oauth_creds.json into the provider."
                    .to_string(),
            ));
        }
        if !status.is_success() {
            return Err(ProxyError::AuthError(format!(
                "Gemini OAuth refresh failed: HTTP {status}"
            )));
        }
        let body: TokenResponse = response
            .json()
            .await
            .map_err(|e| ProxyError::AuthError(format!("Gemini OAuth refresh: bad response: {e}")))?;
        if body.access_token.trim().is_empty() {
            return Err(ProxyError::AuthError(
                "Gemini OAuth refresh returned an empty access_token".to_string(),
            ));
        }
        let expires_at_ms = now_ms() + body.expires_in.unwrap_or(3600).max(0) * 1000;
        self.cache.write().await.insert(
            refresh_token.to_string(),
            CachedToken {
                access_token: body.access_token.clone(),
                expires_at_ms,
            },
        );
        log::info!("[Gemini OAuth] access token refreshed");
        Ok(body.access_token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    async fn mock_server(
        status: u16,
        body: &'static str,
        delay: std::time::Duration,
    ) -> (String, Arc<AtomicUsize>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let hits = Arc::new(AtomicUsize::new(0));
        let counter = hits.clone();
        tokio::spawn(async move {
            while let Ok((mut sock, _)) = listener.accept().await {
                counter.fetch_add(1, Ordering::SeqCst);
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 8192];
                    let _ = sock.read(&mut buf).await;
                    tokio::time::sleep(delay).await;
                    let resp = format!(
                        "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = sock.write_all(resp.as_bytes()).await;
                });
            }
        });
        (format!("http://{addr}/token"), hits)
    }

    fn creds_json(access_token: &str, expiry_ms: i64, refresh: &str) -> String {
        serde_json::json!({
            "access_token": access_token,
            "refresh_token": refresh,
            "client_id": "cid",
            "client_secret": "cs",
            "expiry_date": expiry_ms,
        })
        .to_string()
    }

    #[tokio::test]
    async fn fresh_stored_token_needs_no_refresh() {
        let (url, hits) = mock_server(200, "{}", std::time::Duration::ZERO).await;
        let manager = GeminiOAuthManager::new(url);
        let key = creds_json("ya29.ok", now_ms() + 3_600_000, "rt-fresh");
        assert_eq!(manager.resolve(&key).await.unwrap(), None);
        assert_eq!(manager.resolve("ya29.raw").await.unwrap(), None);
        assert_eq!(hits.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn expired_token_is_refreshed_and_cached() {
        let (url, hits) = mock_server(
            200,
            r#"{"access_token":"ya29.new","expires_in":3599}"#,
            std::time::Duration::ZERO,
        )
        .await;
        let manager = GeminiOAuthManager::new(url);
        let key = creds_json("ya29.old", now_ms() - 1_000, "rt-expired");
        assert_eq!(manager.resolve(&key).await.unwrap().as_deref(), Some("ya29.new"));
        assert_eq!(manager.resolve(&key).await.unwrap().as_deref(), Some("ya29.new"));
        assert_eq!(hits.load(Ordering::SeqCst), 1);

        // Refresh-only JSON (no access token yet) is refreshed too.
        let refresh_only = r#"{"refresh_token":"rt-expired","client_id":"cid","client_secret":"cs"}"#;
        assert_eq!(
            manager.resolve(refresh_only).await.unwrap().as_deref(),
            Some("ya29.new")
        );
    }

    #[tokio::test]
    async fn rejected_refresh_token_requires_reauth() {
        let (url, _) = mock_server(400, r#"{"error":"invalid_grant"}"#, std::time::Duration::ZERO).await;
        let manager = GeminiOAuthManager::new(url);
        let key = creds_json("", 0, "rt-revoked");
        let err = manager.resolve(&key).await.unwrap_err();
        assert!(err.to_string().contains("log in again"), "{err}");
    }

    #[tokio::test]
    async fn failed_refresh_keeps_still_valid_stored_token() {
        let (url, _) = mock_server(503, "{}", std::time::Duration::ZERO).await;
        let manager = GeminiOAuthManager::new(url);
        // Inside the refresh buffer but not expired yet.
        let key = creds_json("ya29.soon", now_ms() + 20_000, "rt-soon");
        assert_eq!(manager.resolve(&key).await.unwrap().as_deref(), Some("ya29.soon"));
    }

    #[tokio::test]
    async fn concurrent_refreshes_are_single_flight() {
        let (url, hits) = mock_server(
            200,
            r#"{"access_token":"ya29.one","expires_in":3600}"#,
            std::time::Duration::from_millis(100),
        )
        .await;
        let manager = Arc::new(GeminiOAuthManager::new(url));
        let key = Arc::new(creds_json("", 0, "rt-concurrent"));
        let mut tasks = Vec::new();
        for _ in 0..10 {
            let (m, k) = (manager.clone(), key.clone());
            tasks.push(tokio::spawn(async move { m.resolve(&k).await }));
        }
        for t in tasks {
            assert_eq!(t.await.unwrap().unwrap().as_deref(), Some("ya29.one"));
        }
        assert_eq!(hits.load(Ordering::SeqCst), 1);
    }
}
