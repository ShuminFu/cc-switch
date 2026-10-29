//! OpenCode 适配器（`~/.config/opencode/opencode.json`，累加模式）
//!
//! 每个供应商是 `provider.<id>` 下的一个条目；OMO / OMO Slim 分类的供应商不进
//! `opencode.json`，而是通过 [`crate::services::OmoService`] 独占写入各自的
//! `oh-my-*.jsonc`（见 [`AppAdapter::omo_variant`]）。

pub mod config;

use std::path::PathBuf;

use serde_json::Value;

use self::config::{get_opencode_config_path, get_opencode_dir, read_opencode_config};
use super::adapter::{
    str_or_empty, AppAdapter, CommonSnippet, Credentials, ExtractOnlySnippet, McpProjector,
    SessionSource,
};
use crate::app_config::AppType;
use crate::config::get_home_dir;
use crate::error::AppError;
use crate::provider::{OpenCodeProviderConfig, Provider};
use crate::services::omo::OmoVariant;

#[derive(Debug)]
pub struct OpenCodeAdapter;

static COMMON_SNIPPET: ExtractOnlySnippet = ExtractOnlySnippet(extract_common_config);

impl AppAdapter for OpenCodeAdapter {
    fn config_dir(&self) -> Result<PathBuf, AppError> {
        Ok(get_opencode_dir())
    }

    fn prompt_file(&self) -> Option<PathBuf> {
        Some(get_opencode_dir().join("AGENTS.md"))
    }

    fn skills_dir(&self) -> Option<PathBuf> {
        // 默认路径必须走 get_home_dir()（可被 CC_SWITCH_TEST_HOME 覆盖）
        Some(
            crate::settings::override_dir(&AppType::OpenCode)
                .unwrap_or_else(|| get_home_dir().join(".config").join("opencode"))
                .join("skills"),
        )
    }

    fn read_live(&self) -> Result<Value, AppError> {
        if !get_opencode_config_path().exists() {
            return Err(AppError::localized(
                "opencode.config.missing",
                "OpenCode 配置文件不存在",
                "OpenCode configuration file not found",
            ));
        }
        read_opencode_config()
    }

    /// 累加模式应用通过 `import_opencode_providers_from_live` 逐个导入，不走默认导入
    fn import_live(&self) -> Result<Value, AppError> {
        Err(AppError::unsupported(&AppType::OpenCode, "import_live"))
    }

    fn write_provider(&self, provider: &Provider) -> Result<(), AppError> {
        // Defensive check: if settings_config is a full config structure, extract provider fragment
        let config_to_write = if let Some(obj) = provider.settings_config.as_object() {
            // Detect full config structure (has $schema or top-level provider field)
            if obj.contains_key("$schema") || obj.contains_key("provider") {
                log::warn!(
                    "OpenCode provider '{}' has full config structure in settings_config, attempting to extract fragment",
                    provider.id
                );
                // Try to extract from provider.{id}
                obj.get("provider")
                    .and_then(|p| p.get(&provider.id))
                    .cloned()
                    .unwrap_or_else(|| provider.settings_config.clone())
            } else {
                provider.settings_config.clone()
            }
        } else {
            provider.settings_config.clone()
        };

        // Convert settings_config to OpenCodeProviderConfig
        match serde_json::from_value::<OpenCodeProviderConfig>(config_to_write.clone()) {
            Ok(typed) => {
                config::set_typed_provider(&provider.id, &typed)?;
                log::info!("OpenCode provider '{}' written to live config", provider.id);
            }
            Err(e) => {
                log::warn!(
                    "Failed to parse OpenCode provider config for '{}': {}",
                    provider.id,
                    e
                );
                // Only write if config looks like a valid provider fragment
                if config_to_write.get("npm").is_some() || config_to_write.get("options").is_some()
                {
                    config::set_provider(&provider.id, config_to_write)?;
                    log::info!(
                        "OpenCode provider '{}' written as raw JSON to live config",
                        provider.id
                    );
                } else {
                    return Err(AppError::Message(format!(
                        "OpenCode provider '{}' has invalid config structure for live config (must contain 'npm' or 'options')",
                        provider.id
                    )));
                }
            }
        }
        Ok(())
    }

    fn remove_provider(&self, provider_id: &str) -> Result<(), AppError> {
        if !get_opencode_dir().exists() {
            log::debug!(
                "OpenCode config directory doesn't exist, skipping removal of '{provider_id}'"
            );
            return Ok(());
        }

        config::remove_provider(provider_id)?;
        log::info!("OpenCode provider '{provider_id}' removed from live config");
        Ok(())
    }

    fn live_has_provider(&self, provider_id: &str) -> Result<bool, AppError> {
        config::get_providers().map(|providers| providers.contains_key(provider_id))
    }

    fn omo_variant(&self, category: Option<&str>) -> Option<&'static OmoVariant> {
        OmoVariant::for_category(category)
    }

    fn validate_provider(&self, provider: &Provider) -> Result<(), AppError> {
        // OpenCode uses a different config structure: { npm, options, models }
        if !provider.settings_config.is_object() {
            return Err(AppError::localized(
                "provider.opencode.settings.not_object",
                "OpenCode 配置必须是 JSON 对象",
                "OpenCode configuration must be a JSON object",
            ));
        }
        Ok(())
    }

    /// 凭据嵌在 `options`（SDK options 对象）下
    fn usage_credentials(&self, settings: &Value) -> (String, String) {
        let options = settings.get("options");
        (
            str_or_empty(options.and_then(|o| o.get("baseURL"))),
            str_or_empty(options.and_then(|o| o.get("apiKey"))),
        )
    }

    fn credentials(&self, provider: &Provider) -> Result<Credentials, AppError> {
        // OpenCode uses options.apiKey and options.baseURL
        let options = provider
            .settings_config
            .get("options")
            .and_then(|v| v.as_object())
            .ok_or_else(|| {
                AppError::localized(
                    "provider.opencode.options.missing",
                    "配置格式错误: 缺少 options",
                    "Invalid configuration: missing options section",
                )
            })?;

        let api_key = options
            .get("apiKey")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                AppError::localized(
                    "provider.opencode.api_key.missing",
                    "缺少 API Key",
                    "API key is missing",
                )
            })?
            .to_string();

        let base_url = options
            .get("baseURL")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        Ok(Credentials { api_key, base_url })
    }

    fn common_snippet(&self) -> Option<&dyn CommonSnippet> {
        Some(&COMMON_SNIPPET)
    }

    fn mcp(&self) -> Option<&dyn McpProjector> {
        Some(self)
    }

    fn session_source(&self) -> Option<&dyn SessionSource> {
        Some(self)
    }
}

impl McpProjector for OpenCodeAdapter {
    fn upsert(&self, id: &str, spec: &Value) -> Result<(), AppError> {
        crate::mcp::sync_single_server_to_opencode(&Default::default(), id, spec)
    }

    fn remove(&self, id: &str) -> Result<(), AppError> {
        crate::mcp::remove_server_from_opencode(id)
    }
}

impl SessionSource for OpenCodeAdapter {
    fn session_roots(&self) -> Vec<PathBuf> {
        vec![crate::session_manager::providers::opencode::get_opencode_data_dir()]
    }
}

/// Extract common config for OpenCode (JSON format)
fn extract_common_config(settings: &Value) -> Result<String, AppError> {
    // OpenCode uses a different config structure with npm, options, models
    // For common config, we exclude provider-specific fields like apiKey
    let mut config = settings.clone();

    // Remove provider-specific fields
    if let Some(obj) = config.as_object_mut() {
        if let Some(options) = obj.get_mut("options").and_then(|v| v.as_object_mut()) {
            options.remove("apiKey");
            options.remove("baseURL");
        }
        // Keep npm and models as they might be common
    }

    if config.is_null() || config.as_object().is_some_and(|obj| obj.is_empty()) {
        return Ok("{}".to_string());
    }

    serde_json::to_string_pretty(&config)
        .map_err(|e| AppError::Message(format!("Serialization failed: {e}")))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn credentials_come_from_options() {
        let provider = Provider::with_id(
            "p".to_string(),
            "P".to_string(),
            json!({ "options": { "apiKey": "sk-test", "baseURL": "https://example.com/v1" } }),
            None,
        );
        let credentials = OpenCodeAdapter.credentials(&provider).unwrap();
        assert_eq!(credentials.api_key, "sk-test");
        assert_eq!(credentials.base_url, "https://example.com/v1");

        let missing = Provider::with_id("p".to_string(), "P".to_string(), json!({}), None);
        assert!(OpenCodeAdapter.credentials(&missing).is_err());
    }

    #[test]
    fn omo_variants_follow_provider_category() {
        let omo = OpenCodeAdapter.omo_variant(Some("omo")).expect("omo");
        assert_eq!(omo.category, "omo");
        assert_eq!(omo.counterpart().category, "omo-slim");

        let slim = OpenCodeAdapter.omo_variant(Some("omo-slim")).expect("slim");
        assert_eq!(slim.category, "omo-slim");
        assert_eq!(slim.counterpart().category, "omo");

        assert!(OpenCodeAdapter.omo_variant(Some("custom")).is_none());
        assert!(OpenCodeAdapter.omo_variant(None).is_none());
    }

    #[test]
    fn common_snippet_is_extract_only() {
        let common = OpenCodeAdapter.common_snippet().expect("snippet");
        let settings = json!({
            "npm": "@ai-sdk/openai-compatible",
            "options": { "apiKey": "sk", "baseURL": "https://example.com", "timeout": 5 }
        });
        let snippet = common.extract(&settings).unwrap();
        assert!(!snippet.contains("apiKey"));
        assert!(!snippet.contains("baseURL"));
        assert!(snippet.contains("timeout"));
        assert_eq!(common.extract(&json!({})).unwrap(), "{}");

        // 累加模式应用不把片段合并进供应商配置
        assert!(!common.contains(&settings, &snippet));
        assert_eq!(common.apply(&settings, &snippet).unwrap(), settings);
        assert_eq!(common.remove(&settings, &snippet).unwrap(), settings);
        assert!(!common.sync_from_live());
    }
}
