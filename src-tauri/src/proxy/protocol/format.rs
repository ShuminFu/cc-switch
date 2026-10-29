//! 协议格式与路由键类型
//!
//! - `WireFormat` — 代理两端实际说的线协议
//! - `ClaudeApiFormat` — Claude 供应商配置里的 `apiFormat`（与既有字符串互转）
//! - `AppRef` — 代理配置 / 熔断器使用的应用标识
//! - `BreakerKey` — 熔断器键（应用 + 供应商），替代 `"app_type:provider_id"` 字符串拼接
//!
//! 配置中的格式字符串只在本文件解析与序列化，其余代码一律使用枚举。

use crate::app_config::AppType;
use crate::proxy::types::AppProxyConfig;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::str::FromStr;

/// 线协议
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WireFormat {
    /// Anthropic Messages API (`/v1/messages`)
    AnthropicMessages,
    /// OpenAI Chat Completions API (`/chat/completions`)
    OpenAiChat,
    /// OpenAI Responses API (`/v1/responses`)
    OpenAiResponses,
    /// Google Gemini Native (`models/*:generateContent`)
    GeminiNative,
}

impl WireFormat {
    /// 解析 Codex 供应商的 `apiFormat` / TOML `wire_api` 值（大小写、空白不敏感，接受常见别名）
    pub fn from_codex_wire_api(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "chat"
            | "chat_completions"
            | "chat-completions"
            | "openai_chat"
            | "openai-chat"
            | "openai_chat_completions" => Some(Self::OpenAiChat),
            "anthropic" | "anthropic_messages" | "anthropic-messages" | "claude" | "messages" => {
                Some(Self::AnthropicMessages)
            }
            "responses" | "openai_responses" | "openai-responses" => Some(Self::OpenAiResponses),
            _ => None,
        }
    }
}

/// Claude 供应商的上游 API 格式
///
/// 序列化为既有配置字符串（`anthropic` / `openai_chat` / `openai_responses` /
/// `gemini_native`），反序列化严格匹配这些值。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ClaudeApiFormat {
    /// Anthropic Messages API，直接透传（默认）
    #[default]
    Anthropic,
    /// OpenAI Chat Completions，需要 Anthropic ↔ OpenAI 转换
    OpenAiChat,
    /// OpenAI Responses API，需要 Anthropic ↔ Responses 转换
    OpenAiResponses,
    /// Gemini Native generateContent，需要 Anthropic ↔ Gemini 转换
    GeminiNative,
}

impl ClaudeApiFormat {
    /// 配置中使用的字符串形式
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Anthropic => "anthropic",
            Self::OpenAiChat => "openai_chat",
            Self::OpenAiResponses => "openai_responses",
            Self::GeminiNative => "gemini_native",
        }
    }

    /// 宽松解析：未知值回退为 [`ClaudeApiFormat::Anthropic`]（与历史行为一致）
    pub fn from_config(value: &str) -> Self {
        value.parse().unwrap_or_default()
    }

    /// 是否需要在 Anthropic 与上游格式之间转换
    pub const fn needs_transform(self) -> bool {
        !matches!(self, Self::Anthropic)
    }

    /// 上游实际使用的线协议
    pub const fn wire_format(self) -> WireFormat {
        match self {
            Self::Anthropic => WireFormat::AnthropicMessages,
            Self::OpenAiChat => WireFormat::OpenAiChat,
            Self::OpenAiResponses => WireFormat::OpenAiResponses,
            Self::GeminiNative => WireFormat::GeminiNative,
        }
    }
}

/// 无法识别的 Claude API 格式字符串
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownClaudeApiFormat(pub String);

impl fmt::Display for UnknownClaudeApiFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown Claude API format: {}", self.0)
    }
}

impl std::error::Error for UnknownClaudeApiFormat {}

impl FromStr for ClaudeApiFormat {
    type Err = UnknownClaudeApiFormat;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        [
            Self::Anthropic,
            Self::OpenAiChat,
            Self::OpenAiResponses,
            Self::GeminiNative,
        ]
        .into_iter()
        .find(|format| format.as_str() == s)
        .ok_or_else(|| UnknownClaudeApiFormat(s.to_string()))
    }
}

impl fmt::Display for ClaudeApiFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for ClaudeApiFormat {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ClaudeApiFormat {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(serde::de::Error::custom)
    }
}

/// 应用标识（与 proxy_config / provider 表中的 app_type 字符串一致）
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AppRef(String);

impl AppRef {
    pub fn new(app_type: impl Into<String>) -> Self {
        Self(app_type.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for AppRef {
    fn from(app_type: &str) -> Self {
        Self::new(app_type)
    }
}

impl From<&AppType> for AppRef {
    fn from(app_type: &AppType) -> Self {
        Self::new(app_type.as_str())
    }
}

impl From<&AppProxyConfig> for AppRef {
    fn from(config: &AppProxyConfig) -> Self {
        Self::new(config.app_type.as_str())
    }
}

impl fmt::Display for AppRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// 熔断器键：每个（应用, 供应商）组合独立一个熔断器
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BreakerKey {
    app: AppRef,
    provider_id: String,
}

impl BreakerKey {
    pub fn new(app: impl Into<AppRef>, provider_id: impl Into<String>) -> Self {
        Self {
            app: app.into(),
            provider_id: provider_id.into(),
        }
    }

    pub fn app(&self) -> &AppRef {
        &self.app
    }

    pub fn provider_id(&self) -> &str {
        &self.provider_id
    }
}

impl fmt::Display for BreakerKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.app, self.provider_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [ClaudeApiFormat; 4] = [
        ClaudeApiFormat::Anthropic,
        ClaudeApiFormat::OpenAiChat,
        ClaudeApiFormat::OpenAiResponses,
        ClaudeApiFormat::GeminiNative,
    ];

    #[test]
    fn claude_api_format_round_trips_through_config_strings() {
        for format in ALL {
            assert_eq!(format.as_str().parse::<ClaudeApiFormat>(), Ok(format));
            let json = serde_json::to_value(format).unwrap();
            assert_eq!(json, serde_json::Value::String(format.as_str().into()));
            let back: ClaudeApiFormat = serde_json::from_value(json).unwrap();
            assert_eq!(back, format);
        }
    }

    #[test]
    fn claude_api_format_strict_parse_rejects_unknown_values() {
        assert!("openai".parse::<ClaudeApiFormat>().is_err());
        assert!("".parse::<ClaudeApiFormat>().is_err());
        assert!(serde_json::from_str::<ClaudeApiFormat>("\"OpenAI_Chat\"").is_err());
    }

    #[test]
    fn claude_api_format_lenient_parse_falls_back_to_anthropic() {
        assert_eq!(
            ClaudeApiFormat::from_config("something_else"),
            ClaudeApiFormat::Anthropic
        );
        assert_eq!(
            ClaudeApiFormat::from_config(ClaudeApiFormat::GeminiNative.as_str()),
            ClaudeApiFormat::GeminiNative
        );
    }

    #[test]
    fn only_anthropic_skips_transform() {
        for format in ALL {
            assert_eq!(
                format.needs_transform(),
                format != ClaudeApiFormat::Anthropic
            );
        }
        assert_eq!(
            ClaudeApiFormat::Anthropic.wire_format(),
            WireFormat::AnthropicMessages
        );
    }

    #[test]
    fn codex_wire_api_aliases() {
        for alias in ["chat", " Chat_Completions ", "openai-chat"] {
            assert_eq!(
                WireFormat::from_codex_wire_api(alias),
                Some(WireFormat::OpenAiChat)
            );
        }
        for alias in ["anthropic", "Messages", "claude"] {
            assert_eq!(
                WireFormat::from_codex_wire_api(alias),
                Some(WireFormat::AnthropicMessages)
            );
        }
        assert_eq!(
            WireFormat::from_codex_wire_api("responses"),
            Some(WireFormat::OpenAiResponses)
        );
        assert_eq!(WireFormat::from_codex_wire_api("grpc"), None);
    }

    #[test]
    fn breaker_key_keeps_provider_id_with_colons_intact() {
        let key = BreakerKey::new("claude", "custom:a:b");
        assert_eq!(key.app().as_str(), "claude");
        assert_eq!(key.provider_id(), "custom:a:b");
        assert_eq!(key.to_string(), "claude:custom:a:b");
        assert_ne!(key, BreakerKey::new("claude:custom", "a:b"));
        assert_eq!(
            BreakerKey::new(&AppType::Codex, "p").app().as_str(),
            "codex"
        );
    }
}
