//! Provider records as serialized by the backend (`src-tauri/src/provider.rs`).
//!
//! Field names are the wire names: the backend uses explicit `rename`s and the
//! React types mirror them in camelCase. Unknown fields are preserved in
//! `extra` so older/newer backends never lose data through the frontend.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Provider {
    pub id: String,
    pub name: String,
    #[serde(rename = "settingsConfig", default)]
    pub settings_config: Value,
    #[serde(
        rename = "websiteUrl",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub website_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(rename = "createdAt", default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<i64>,
    #[serde(rename = "sortIndex", default, skip_serializing_if = "Option::is_none")]
    pub sort_index: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<ProviderMeta>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(rename = "iconColor", default, skip_serializing_if = "Option::is_none")]
    pub icon_color: Option<String>,
    #[serde(rename = "inFailoverQueue", default)]
    pub in_failover_queue: bool,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomEndpoint {
    pub url: String,
    pub added_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_used: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AuthBindingSource {
    #[default]
    Manual,
    Managed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct AuthBinding {
    #[serde(default)]
    pub source: AuthBindingSource,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// Provider metadata kept only in cc-switch's own config, never written to
/// the tool's live config. Only the fields the UI reads are typed; the rest
/// travel in `extra`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ProviderMeta {
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub custom_endpoints: HashMap<String, CustomEndpoint>,
    #[serde(
        rename = "commonConfigEnabled",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub common_config_enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage_script: Option<Value>,
    #[serde(
        rename = "endpointAutoSelect",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub endpoint_auto_select: Option<bool>,
    #[serde(rename = "isPartner", default, skip_serializing_if = "Option::is_none")]
    pub is_partner: Option<bool>,
    #[serde(
        rename = "partnerPromotionKey",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub partner_promotion_key: Option<String>,
    #[serde(
        rename = "costMultiplier",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub cost_multiplier: Option<String>,
    #[serde(
        rename = "limitDailyUsd",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub limit_daily_usd: Option<String>,
    #[serde(
        rename = "limitMonthlyUsd",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub limit_monthly_usd: Option<String>,
    #[serde(rename = "apiFormat", default, skip_serializing_if = "Option::is_none")]
    pub api_format: Option<String>,
    #[serde(
        rename = "authBinding",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub auth_binding: Option<AuthBinding>,
    #[serde(
        rename = "apiKeyField",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub api_key_field: Option<String>,
    #[serde(rename = "isFullUrl", default, skip_serializing_if = "Option::is_none")]
    pub is_full_url: Option<bool>,
    #[serde(
        rename = "customUserAgent",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub custom_user_agent: Option<String>,
    #[serde(
        rename = "liveConfigManaged",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub live_config_managed: Option<bool>,
    #[serde(
        rename = "providerType",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub provider_type: Option<String>,
    #[serde(
        rename = "maxOutputTokens",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub max_output_tokens: Option<u64>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r##"{
      "id": "p1",
      "name": "OpenAI",
      "settingsConfig": {"env": {"ANTHROPIC_BASE_URL": "https://x"}},
      "websiteUrl": "https://openai.com",
      "category": "official",
      "createdAt": 1700000000000,
      "sortIndex": 2,
      "icon": "openai",
      "iconColor": "#00A67E",
      "inFailoverQueue": true,
      "meta": {
        "custom_endpoints": {"https://a": {"url": "https://a", "addedAt": 1, "lastUsed": 2}},
        "commonConfigEnabled": true,
        "apiFormat": "anthropic",
        "authBinding": {"source": "managed", "accountId": "acc"},
        "codexFastMode": true
      },
      "futureField": {"nested": [1, 2]}
    }"##;

    #[test]
    fn round_trips_and_preserves_unknown_fields() {
        let provider: Provider = serde_json::from_str(SAMPLE).unwrap();
        assert_eq!(provider.name, "OpenAI");
        assert_eq!(provider.sort_index, Some(2));
        assert!(provider.in_failover_queue);
        let meta = provider.meta.as_ref().unwrap();
        assert_eq!(meta.custom_endpoints["https://a"].last_used, Some(2));
        assert_eq!(meta.api_format.as_deref(), Some("anthropic"));
        assert_eq!(
            meta.auth_binding.as_ref().unwrap().source,
            AuthBindingSource::Managed
        );
        assert_eq!(meta.extra["codexFastMode"], Value::Bool(true));
        assert_eq!(provider.extra["futureField"]["nested"][1], 2);

        let json: Value = serde_json::to_value(&provider).unwrap();
        let expected: Value = serde_json::from_str(SAMPLE).unwrap();
        assert_eq!(json, expected);
    }

    #[test]
    fn minimal_provider_gets_defaults() {
        let provider: Provider = serde_json::from_str(r#"{"id":"a","name":"b"}"#).unwrap();
        assert!(!provider.in_failover_queue);
        assert_eq!(provider.settings_config, Value::Null);
        assert_eq!(
            serde_json::to_string(&provider).unwrap(),
            r#"{"id":"a","name":"b","settingsConfig":null,"inFailoverQueue":false}"#
        );
    }
}
