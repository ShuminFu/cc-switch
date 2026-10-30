//! Gemini (Google) Provider Adapter
//!
//! 支持 API Key 和 OAuth 两种认证方式
//!
//! ## 认证模式
//! - **Gemini**: API Key 认证 (x-goog-api-key)
//! - **GeminiCli**: OAuth Bearer 认证 (用于 Gemini CLI)
//! - **Anthropic 上游**（CCS-187）：供应商声明 `apiFormat = anthropic` 时，Gemini 客户端的
//!   `generateContent` 请求转换为 Anthropic Messages，Key 以 `x-api-key` 发送
//!   （`meta.apiKeyField = ANTHROPIC_AUTH_TOKEN` 时改为 `Authorization: Bearer`）

use super::{AuthInfo, AuthStrategy, ProviderAdapter, ProviderType};
use crate::provider::Provider;
use crate::proxy::error::ProxyError;
use crate::proxy::protocol::WireFormat;

/// Gemini 供应商的上游是否为 Anthropic Messages
///
/// 只看显式配置（`meta.apiFormat`，其次 `settingsConfig.apiFormat` / `api_format`），
/// 不根据 base_url 猜测；取值与 Codex 的 `apiFormat` 相同（`anthropic` / `claude` / `messages` 等）。
pub fn gemini_provider_uses_anthropic(provider: &Provider) -> bool {
    provider
        .meta
        .as_ref()
        .and_then(|meta| meta.api_format.as_deref())
        .or_else(|| {
            provider
                .settings_config
                .get("apiFormat")
                .or_else(|| provider.settings_config.get("api_format"))
                .and_then(|value| value.as_str())
        })
        .and_then(WireFormat::from_codex_wire_api)
        == Some(WireFormat::AnthropicMessages)
}

/// Gemini 生成端点（`models/{model}:generateContent` / `:streamGenerateContent`）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeminiGenerateEndpoint {
    /// URL 中的模型名（去掉 `models/` 前缀）
    pub model: String,
    /// `:streamGenerateContent`
    pub stream: bool,
}

impl GeminiGenerateEndpoint {
    /// 解析入站端点；`countTokens`、`GET models` 等其他端点返回 `None`
    pub fn parse(endpoint: &str) -> Option<Self> {
        let path = endpoint
            .split_once('?')
            .map_or(endpoint, |(path, _query)| path);
        let (_, resource) = path.rsplit_once("/models/")?;
        let (model, method) = resource.rsplit_once(':')?;
        let stream = match method {
            "generateContent" => false,
            "streamGenerateContent" => true,
            _ => return None,
        };
        let model = percent_decode(model);
        if model.is_empty() || model.contains('/') {
            return None;
        }
        Some(Self { model, stream })
    }
}

/// URL 路径段的百分号解码（模型名中的 `.` 等字符通常不编码，这里只做兜底）
fn percent_decode(segment: &str) -> String {
    let bytes = segment.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hex = |byte: u8| (byte as char).to_digit(16);
            if let (Some(high), Some(low)) = (hex(bytes[index + 1]), hex(bytes[index + 2])) {
                decoded.push((high * 16 + low) as u8);
                index += 3;
                continue;
            }
        }
        decoded.push(bytes[index]);
        index += 1;
    }
    String::from_utf8(decoded).unwrap_or_else(|_| segment.to_string())
}

/// 该请求是否走 Gemini 客户端 → Anthropic 上游的转换
pub fn should_convert_gemini_to_anthropic(provider: &Provider, endpoint: &str) -> bool {
    GeminiGenerateEndpoint::parse(endpoint).is_some() && gemini_provider_uses_anthropic(provider)
}

/// Gemini 适配器
pub struct GeminiAdapter;

/// OAuth 凭证结构
#[derive(Debug, Clone)]
pub struct OAuthCredentials {
    pub access_token: String,
}

impl GeminiAdapter {
    pub fn new() -> Self {
        Self
    }

    /// 获取供应商类型
    ///
    /// 根据 API Key 格式检测：
    /// - GeminiCli: access_token (ya29. 开头) 或 JSON 格式凭证
    /// - Gemini: 普通 API Key
    pub fn provider_type(&self, provider: &Provider) -> ProviderType {
        if let Some(key) = self.extract_key_raw(provider) {
            // OAuth access_token 以 ya29. 开头
            if key.starts_with("ya29.") {
                return ProviderType::GeminiCli;
            }
            // JSON 格式的 OAuth 凭证
            if key.starts_with('{') {
                return ProviderType::GeminiCli;
            }
        }
        ProviderType::Gemini
    }

    /// 检测认证类型
    pub fn detect_auth_type(&self, provider: &Provider) -> AuthStrategy {
        match self.provider_type(provider) {
            ProviderType::GeminiCli => AuthStrategy::GoogleOAuth,
            _ => AuthStrategy::Google,
        }
    }

    /// 解析 OAuth 凭证
    pub fn parse_oauth_credentials(&self, key: &str) -> Option<OAuthCredentials> {
        // 防御性 trim:前端在 input 事件中会 trim,但 JSON 编辑器 / deeplink
        // 导入 / live 回填等路径会绕过。带前导换行的 oauth_creds.json 粘贴
        // 是常见场景,此处统一兜底。
        let key = key.trim();

        // 直接是 access_token
        if key.starts_with("ya29.") {
            return Some(OAuthCredentials {
                access_token: key.to_string(),
            });
        }

        // JSON 格式
        if key.starts_with('{') {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(key) {
                let access_token = json
                    .get("access_token")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_default();
                let has_refresh_token =
                    json.get("refresh_token").and_then(|v| v.as_str()).is_some();

                // 如果有 access_token 或 refresh_token，返回凭证
                if !access_token.is_empty() || has_refresh_token {
                    return Some(OAuthCredentials { access_token });
                }
            }
        }

        None
    }

    /// 从 Provider 配置中提取原始 API Key
    fn extract_key_raw(&self, provider: &Provider) -> Option<String> {
        if let Some(env) = provider.settings_config.get("env") {
            // 使用 GEMINI_API_KEY
            if let Some(key) = env
                .get("GEMINI_API_KEY")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                return Some(key.to_string());
            }
        }

        // 尝试直接获取
        if let Some(key) = provider
            .settings_config
            .get("apiKey")
            .or_else(|| provider.settings_config.get("api_key"))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            return Some(key.to_string());
        }

        None
    }
}

impl Default for GeminiAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl ProviderAdapter for GeminiAdapter {
    fn name(&self) -> &'static str {
        "Gemini"
    }

    fn extract_base_url(&self, provider: &Provider) -> Result<String, ProxyError> {
        // 从 env 中获取
        if let Some(env) = provider.settings_config.get("env") {
            if let Some(url) = env.get("GOOGLE_GEMINI_BASE_URL").and_then(|v| v.as_str()) {
                return Ok(url.trim_end_matches('/').to_string());
            }
        }

        // 尝试直接获取
        if let Some(url) = provider
            .settings_config
            .get("base_url")
            .and_then(|v| v.as_str())
        {
            return Ok(url.trim_end_matches('/').to_string());
        }

        if let Some(url) = provider
            .settings_config
            .get("baseURL")
            .and_then(|v| v.as_str())
        {
            return Ok(url.trim_end_matches('/').to_string());
        }

        Err(ProxyError::ConfigError(
            "Gemini Provider 缺少 base_url 配置".to_string(),
        ))
    }

    fn extract_auth(&self, provider: &Provider) -> Option<AuthInfo> {
        let key = self.extract_key_raw(provider)?;
        if gemini_provider_uses_anthropic(provider) {
            let uses_bearer = provider
                .meta
                .as_ref()
                .and_then(|meta| meta.api_key_field.as_deref())
                .is_some_and(|field| field.eq_ignore_ascii_case("ANTHROPIC_AUTH_TOKEN"));
            let strategy = if uses_bearer {
                AuthStrategy::ClaudeAuth
            } else {
                AuthStrategy::Anthropic
            };
            return Some(AuthInfo::new(key, strategy));
        }
        let strategy = self.detect_auth_type(provider);

        match strategy {
            AuthStrategy::GoogleOAuth => {
                // 解析 OAuth 凭证
                if let Some(creds) = self.parse_oauth_credentials(&key) {
                    Some(AuthInfo::with_access_token(key, creds.access_token))
                } else {
                    // 回退到普通 API Key
                    Some(AuthInfo::new(key, AuthStrategy::Google))
                }
            }
            _ => Some(AuthInfo::new(key, AuthStrategy::Google)),
        }
    }

    fn build_url(&self, base_url: &str, endpoint: &str) -> String {
        let base_trimmed = base_url.trim_end_matches('/');
        let endpoint_trimmed = endpoint.trim_start_matches('/');

        let mut url = format!("{base_trimmed}/{endpoint_trimmed}");

        // 处理 /v1beta 路径去重
        let version_patterns = ["/v1beta", "/v1"];
        for pattern in &version_patterns {
            let duplicate = format!("{pattern}{pattern}");
            if url.contains(&duplicate) {
                url = url.replace(&duplicate, pattern);
            }
        }

        url
    }

    fn get_auth_headers(
        &self,
        auth: &AuthInfo,
    ) -> Result<Vec<(http::HeaderName, http::HeaderValue)>, ProxyError> {
        use super::adapter::auth_header_value as hv;
        use http::{HeaderName, HeaderValue};
        Ok(match auth.strategy {
            // Anthropic 上游：只发一种认证头（anthropic-version 由出站头策略补齐）
            AuthStrategy::Anthropic => {
                vec![(HeaderName::from_static("x-api-key"), hv(&auth.api_key)?)]
            }
            AuthStrategy::ClaudeAuth => vec![(
                HeaderName::from_static("authorization"),
                hv(&format!("Bearer {}", auth.api_key))?,
            )],
            AuthStrategy::GoogleOAuth => {
                let token = auth.access_token.as_ref().unwrap_or(&auth.api_key);
                vec![
                    (
                        HeaderName::from_static("authorization"),
                        hv(&format!("Bearer {token}"))?,
                    ),
                    (
                        HeaderName::from_static("x-goog-api-client"),
                        HeaderValue::from_static("GeminiCLI/1.0"),
                    ),
                ]
            }
            _ => vec![(
                HeaderName::from_static("x-goog-api-key"),
                hv(&auth.api_key)?,
            )],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn create_provider(config: serde_json::Value) -> Provider {
        Provider {
            id: "test".to_string(),
            name: "Test Gemini".to_string(),
            settings_config: config,
            website_url: None,
            category: Some("gemini".to_string()),
            created_at: None,
            sort_index: None,
            notes: None,
            meta: None,
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        }
    }

    #[test]
    fn test_extract_base_url_from_env() {
        let adapter = GeminiAdapter::new();
        let provider = create_provider(json!({
            "env": {
                "GOOGLE_GEMINI_BASE_URL": "https://generativelanguage.googleapis.com/v1beta"
            }
        }));

        let url = adapter.extract_base_url(&provider).unwrap();
        assert_eq!(url, "https://generativelanguage.googleapis.com/v1beta");
    }

    #[test]
    fn test_extract_auth_api_key() {
        let adapter = GeminiAdapter::new();
        let provider = create_provider(json!({
            "env": {
                "GEMINI_API_KEY": "AIza-test-key-12345678"
            }
        }));

        let auth = adapter.extract_auth(&provider).unwrap();
        assert_eq!(auth.api_key, "AIza-test-key-12345678");
        assert_eq!(auth.strategy, AuthStrategy::Google);
        assert!(auth.access_token.is_none());
    }

    #[test]
    fn test_extract_auth_oauth_access_token() {
        let adapter = GeminiAdapter::new();
        let provider = create_provider(json!({
            "env": {
                "GEMINI_API_KEY": "ya29.test-access-token-12345"
            }
        }));

        let auth = adapter.extract_auth(&provider).unwrap();
        assert_eq!(auth.strategy, AuthStrategy::GoogleOAuth);
        assert_eq!(
            auth.access_token,
            Some("ya29.test-access-token-12345".to_string())
        );
    }

    #[test]
    fn test_extract_auth_oauth_json() {
        let adapter = GeminiAdapter::new();
        let provider = create_provider(json!({
            "env": {
                "GEMINI_API_KEY": "{\"access_token\":\"ya29.test-token\",\"refresh_token\":\"1//refresh\"}"
            }
        }));

        let auth = adapter.extract_auth(&provider).unwrap();
        assert_eq!(auth.strategy, AuthStrategy::GoogleOAuth);
        assert_eq!(auth.access_token, Some("ya29.test-token".to_string()));
    }

    #[test]
    fn test_provider_type_detection() {
        let adapter = GeminiAdapter::new();

        // API Key
        let api_key_provider = create_provider(json!({
            "env": {
                "GEMINI_API_KEY": "AIza-test-key"
            }
        }));
        assert_eq!(
            adapter.provider_type(&api_key_provider),
            ProviderType::Gemini
        );

        // OAuth access_token
        let oauth_provider = create_provider(json!({
            "env": {
                "GEMINI_API_KEY": "ya29.test-token"
            }
        }));
        assert_eq!(
            adapter.provider_type(&oauth_provider),
            ProviderType::GeminiCli
        );

        // OAuth JSON
        let oauth_json_provider = create_provider(json!({
            "env": {
                "GEMINI_API_KEY": "{\"access_token\":\"ya29.test\"}"
            }
        }));
        assert_eq!(
            adapter.provider_type(&oauth_json_provider),
            ProviderType::GeminiCli
        );
    }

    #[test]
    fn test_extract_auth_fallback() {
        let adapter = GeminiAdapter::new();
        let provider = create_provider(json!({
            "env": {
                "GEMINI_API_KEY": "AIza-fallback-key"
            }
        }));

        let auth = adapter.extract_auth(&provider).unwrap();
        assert_eq!(auth.api_key, "AIza-fallback-key");
    }

    #[test]
    fn test_build_url_dedup() {
        let adapter = GeminiAdapter::new();
        // 模拟 base_url 已包含 /v1beta，endpoint 也包含 /v1beta
        let url = adapter.build_url(
            "https://generativelanguage.googleapis.com/v1beta",
            "/v1beta/models/gemini-pro:generateContent",
        );
        assert_eq!(
            url,
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-pro:generateContent"
        );
    }

    #[test]
    fn test_build_url_normal() {
        let adapter = GeminiAdapter::new();
        let url = adapter.build_url(
            "https://generativelanguage.googleapis.com/v1beta",
            "/models/gemini-pro:generateContent",
        );
        assert_eq!(
            url,
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-pro:generateContent"
        );
    }

    #[test]
    fn test_parse_oauth_credentials_direct_token() {
        let adapter = GeminiAdapter::new();
        let creds = adapter
            .parse_oauth_credentials("ya29.test-access-token")
            .unwrap();
        assert_eq!(creds.access_token, "ya29.test-access-token");
    }

    #[test]
    fn test_parse_oauth_credentials_json() {
        let adapter = GeminiAdapter::new();
        let creds = adapter
            .parse_oauth_credentials(
                "{\"access_token\":\"ya29.test\",\"refresh_token\":\"1//refresh\"}",
            )
            .unwrap();
        assert_eq!(creds.access_token, "ya29.test");
    }

    #[test]
    fn test_parse_oauth_credentials_json_refresh_token_only() {
        let adapter = GeminiAdapter::new();
        let creds = adapter
            .parse_oauth_credentials("{\"refresh_token\":\"1//refresh\"}")
            .unwrap();
        assert!(creds.access_token.is_empty());
    }

    #[test]
    fn test_parse_oauth_credentials_invalid() {
        let adapter = GeminiAdapter::new();
        assert!(adapter.parse_oauth_credentials("AIza-api-key").is_none());
        assert!(adapter.parse_oauth_credentials("invalid-json{").is_none());
    }

    fn anthropic_provider(meta: serde_json::Value) -> Provider {
        let mut provider = create_provider(json!({
            "env": {
                "GOOGLE_GEMINI_BASE_URL": "https://api.anthropic.com",
                "GEMINI_API_KEY": "sk-ant-test"
            }
        }));
        provider.meta = Some(serde_json::from_value(meta).unwrap());
        provider
    }

    #[test]
    fn anthropic_upstream_is_declared_only_by_api_format() {
        assert!(gemini_provider_uses_anthropic(&anthropic_provider(
            json!({ "apiFormat": "anthropic" })
        )));
        assert!(gemini_provider_uses_anthropic(&anthropic_provider(
            json!({ "apiFormat": " Claude " })
        )));
        assert!(!gemini_provider_uses_anthropic(&anthropic_provider(json!(
            {}
        ))));
        assert!(gemini_provider_uses_anthropic(&create_provider(json!({
            "apiFormat": "anthropic",
            "env": { "GEMINI_API_KEY": "k" }
        }))));
        // base_url 不参与判断
        assert!(!gemini_provider_uses_anthropic(&create_provider(json!({
            "env": { "GOOGLE_GEMINI_BASE_URL": "https://api.anthropic.com" }
        }))));
    }

    #[test]
    fn generate_endpoints_are_parsed_from_the_inbound_path() {
        assert_eq!(
            GeminiGenerateEndpoint::parse(
                "/v1beta/models/claude-sonnet-4-5:streamGenerateContent?alt=sse"
            ),
            Some(GeminiGenerateEndpoint {
                model: "claude-sonnet-4-5".to_string(),
                stream: true,
            })
        );
        assert_eq!(
            GeminiGenerateEndpoint::parse("/gemini/v1/models/claude%2Dhaiku:generateContent"),
            Some(GeminiGenerateEndpoint {
                model: "claude-haiku".to_string(),
                stream: false,
            })
        );
        for other in [
            "/v1beta/models/m:countTokens",
            "/v1beta/models",
            "/v1beta/models/m",
            "/v1beta/models/:generateContent",
        ] {
            assert_eq!(GeminiGenerateEndpoint::parse(other), None, "{other}");
        }
        let provider = anthropic_provider(json!({ "apiFormat": "anthropic" }));
        assert!(should_convert_gemini_to_anthropic(
            &provider,
            "/v1beta/models/m:generateContent"
        ));
        assert!(!should_convert_gemini_to_anthropic(
            &provider,
            "/v1beta/models/m:countTokens"
        ));
    }

    #[test]
    fn anthropic_upstream_uses_x_api_key_or_bearer() {
        let adapter = GeminiAdapter::new();
        let provider = anthropic_provider(json!({ "apiFormat": "anthropic" }));
        let auth = adapter.extract_auth(&provider).unwrap();
        assert_eq!(auth.strategy, AuthStrategy::Anthropic);
        let headers = adapter.get_auth_headers(&auth).unwrap();
        assert_eq!(headers.len(), 1);
        assert_eq!(headers[0].0.as_str(), "x-api-key");
        assert_eq!(headers[0].1, "sk-ant-test");

        let provider = anthropic_provider(
            json!({ "apiFormat": "anthropic", "apiKeyField": "ANTHROPIC_AUTH_TOKEN" }),
        );
        let auth = adapter.extract_auth(&provider).unwrap();
        assert_eq!(auth.strategy, AuthStrategy::ClaudeAuth);
        let headers = adapter.get_auth_headers(&auth).unwrap();
        assert_eq!(headers[0].0.as_str(), "authorization");
        assert_eq!(headers[0].1, "Bearer sk-ant-test");
    }
}
