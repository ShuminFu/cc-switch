//! 累加模式应用（OpenCode / OpenClaw / Hermes）的上游适配器
//!
//! 这些应用的供应商 `settings_config` 是各自 live 条目的形状（OpenCode 的
//! `options.baseURL`、OpenClaw 的 `baseUrl`、Hermes 的 `base_url`），凭据统一经
//! [`crate::apps::AppAdapter::usage_credentials`] 提取；条目说 OpenAI 兼容协议，
//! URL 拼接与 Bearer 认证沿用 [`CodexAdapter`]。

use super::{AuthInfo, AuthStrategy, CodexAdapter, ProviderAdapter};
use crate::app_config::AppType;
use crate::provider::Provider;
use crate::proxy::error::ProxyError;

pub struct AdditiveAppAdapter {
    app: AppType,
    openai: CodexAdapter,
}

impl AdditiveAppAdapter {
    pub fn new(app: AppType) -> Self {
        Self {
            app,
            openai: CodexAdapter::new(),
        }
    }

    /// `(base_url, api_key)`，缺失的字段为空串
    fn credentials(&self, provider: &Provider) -> (String, String) {
        crate::apps::adapter(&self.app).usage_credentials(&provider.settings_config)
    }
}

impl ProviderAdapter for AdditiveAppAdapter {
    fn name(&self) -> &'static str {
        crate::apps::registry::descriptor(&self.app).display_name
    }

    fn extract_base_url(&self, provider: &Provider) -> Result<String, ProxyError> {
        let (base_url, _) = self.credentials(provider);
        let base_url = base_url.trim().trim_end_matches('/');
        if base_url.is_empty() {
            return Err(ProxyError::ConfigError(format!(
                "{} Provider 缺少 base_url 配置",
                self.name()
            )));
        }
        Ok(base_url.to_string())
    }

    fn extract_auth(&self, provider: &Provider) -> Option<AuthInfo> {
        let (_, api_key) = self.credentials(provider);
        let api_key = api_key.trim();
        (!api_key.is_empty()).then(|| AuthInfo::new(api_key.to_string(), AuthStrategy::Bearer))
    }

    fn build_url(&self, base_url: &str, endpoint: &str) -> String {
        self.openai.build_url(base_url, endpoint)
    }

    fn get_auth_headers(
        &self,
        auth: &AuthInfo,
    ) -> Result<Vec<(http::HeaderName, http::HeaderValue)>, ProxyError> {
        self.openai.get_auth_headers(auth)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn provider(settings: Value) -> Provider {
        Provider::with_id("p".to_string(), "P".to_string(), settings, None)
    }

    #[test]
    fn credentials_follow_each_app_entry_shape() {
        let cases = [
            (
                AppType::OpenCode,
                json!({ "npm": "@ai-sdk/openai-compatible", "options": { "baseURL": "https://up.example.com/v1/", "apiKey": "sk-oc" } }),
                "sk-oc",
            ),
            (
                AppType::OpenClaw,
                json!({ "baseUrl": "https://up.example.com/v1", "apiKey": "sk-ocl", "api": "openai-completions" }),
                "sk-ocl",
            ),
            (
                AppType::Hermes,
                json!({ "name": "p", "base_url": "https://up.example.com/v1", "api_key": "sk-h" }),
                "sk-h",
            ),
        ];
        for (app, settings, key) in cases {
            let adapter = AdditiveAppAdapter::new(app.clone());
            let provider = provider(settings);
            assert_eq!(
                adapter.extract_base_url(&provider).unwrap(),
                "https://up.example.com/v1",
                "{app:?}"
            );
            let auth = adapter.extract_auth(&provider).expect("auth");
            assert_eq!(auth.api_key, key, "{app:?}");
            assert_eq!(auth.strategy, AuthStrategy::Bearer, "{app:?}");
            assert_eq!(
                adapter.build_url("https://up.example.com/v1", "/chat/completions"),
                "https://up.example.com/v1/chat/completions"
            );
        }
    }

    #[test]
    fn missing_base_url_or_key_is_reported() {
        let adapter = AdditiveAppAdapter::new(AppType::OpenClaw);
        let empty = provider(json!({}));
        assert!(adapter.extract_base_url(&empty).is_err());
        assert!(adapter.extract_auth(&empty).is_none());
    }
}
