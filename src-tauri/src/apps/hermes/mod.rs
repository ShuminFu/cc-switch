//! Hermes 适配器（`~/.hermes/config.yaml`，累加模式）
//!
//! 每个供应商是 `custom_providers[]` 里的一项；切换供应商额外更新顶层 `model:`
//! 指向该供应商（见 [`AppAdapter::apply_switch_defaults`]）。

pub mod config;

use std::path::PathBuf;

use serde_json::Value;

use self::config::{get_hermes_config_path, get_hermes_dir, read_hermes_config, yaml_to_json};
use super::adapter::{str_or_empty, AppAdapter, Credentials, McpProjector, SessionSource};
use crate::app_config::AppType;
use crate::error::AppError;
use crate::provider::Provider;

#[derive(Debug)]
pub struct HermesAdapter;

impl AppAdapter for HermesAdapter {
    fn config_dir(&self) -> Result<PathBuf, AppError> {
        Ok(get_hermes_dir())
    }

    fn prompt_file(&self) -> Option<PathBuf> {
        Some(get_hermes_dir().join("AGENTS.md"))
    }

    fn skills_dir(&self) -> Option<PathBuf> {
        // get_hermes_dir() 已按 settings 覆盖 → HERMES_HOME → 平台默认解析
        Some(get_hermes_dir().join("skills"))
    }

    fn read_live(&self) -> Result<Value, AppError> {
        if !get_hermes_config_path().exists() {
            return Err(AppError::localized(
                "hermes.config.missing",
                "Hermes 配置文件不存在",
                "Hermes configuration file not found",
            ));
        }
        yaml_to_json(&read_hermes_config()?)
    }

    /// 累加模式应用通过 `import_hermes_providers_from_live` 逐个导入，不走默认导入
    fn import_live(&self) -> Result<Value, AppError> {
        Err(AppError::unsupported(&AppType::Hermes, "import_live"))
    }

    fn write_provider(&self, provider: &Provider) -> Result<(), AppError> {
        config::set_provider(&provider.id, provider.settings_config.clone())?;
        log::debug!("Hermes provider '{}' written to live config", provider.id);
        Ok(())
    }

    fn remove_provider(&self, provider_id: &str) -> Result<(), AppError> {
        if !get_hermes_dir().exists() {
            log::debug!(
                "Hermes config directory doesn't exist, skipping removal of '{provider_id}'"
            );
            return Ok(());
        }

        config::remove_provider(provider_id)?;
        log::info!("Hermes provider '{provider_id}' removed from live config");
        Ok(())
    }

    fn live_has_provider(&self, provider_id: &str) -> Result<bool, AppError> {
        config::get_providers().map(|providers| providers.contains_key(provider_id))
    }

    /// 写入 `custom_providers[]` 不会改变 Hermes 正在用的供应商：把顶层 `model:`
    /// 指向该供应商的第一个模型，否则"切换"只是重排了条目
    fn apply_switch_defaults(&self, provider: &Provider) -> Result<(), AppError> {
        config::apply_switch_defaults(&provider.id, &provider.settings_config).map(|_| ())
    }

    fn validate_provider(&self, provider: &Provider) -> Result<(), AppError> {
        // Hermes: accept any JSON object for now
        if !provider.settings_config.is_object() {
            return Err(AppError::localized(
                "provider.hermes.settings.not_object",
                "Hermes 配置必须是 JSON 对象",
                "Hermes configuration must be a JSON object",
            ));
        }
        Ok(())
    }

    /// config.yaml 把凭据平铺在顶层，snake_case
    fn usage_credentials(&self, settings: &Value) -> (String, String) {
        (
            str_or_empty(settings.get("base_url")),
            str_or_empty(settings.get("api_key")),
        )
    }

    fn credentials(&self, provider: &Provider) -> Result<Credentials, AppError> {
        super::openclaw::flat_credentials(provider)
    }

    // Hermes doesn't use common config snippets

    fn mcp(&self) -> Option<&dyn McpProjector> {
        Some(self)
    }

    fn session_source(&self) -> Option<&dyn SessionSource> {
        Some(self)
    }
}

impl McpProjector for HermesAdapter {
    fn upsert(&self, id: &str, spec: &Value) -> Result<(), AppError> {
        crate::mcp::sync_single_server_to_hermes(&Default::default(), id, spec)
    }

    fn remove(&self, id: &str) -> Result<(), AppError> {
        crate::mcp::remove_server_from_hermes(id)
    }
}

impl SessionSource for HermesAdapter {
    fn session_roots(&self) -> Vec<PathBuf> {
        vec![get_hermes_dir().join("sessions")]
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn credentials_share_the_openclaw_shape() {
        let provider = Provider::with_id(
            "p".to_string(),
            "P".to_string(),
            json!({ "apiKey": "sk-test", "baseUrl": "https://example.com/v1" }),
            None,
        );
        let credentials = HermesAdapter.credentials(&provider).unwrap();
        assert_eq!(credentials.api_key, "sk-test");
        assert_eq!(credentials.base_url, "https://example.com/v1");
    }

    #[test]
    fn has_no_common_snippet_or_takeover() {
        assert!(HermesAdapter.common_snippet().is_none());
        assert!(HermesAdapter.takeover().is_none());
        assert!(HermesAdapter.mcp().is_some());
    }
}
