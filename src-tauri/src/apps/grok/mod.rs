//! Grok Build 适配器（`~/.grok/config.toml`）

pub mod config;

use std::path::PathBuf;

use serde_json::Value;

use self::config::{
    get_grok_config_dir, read_grok_live_settings, strip_grok_mcp_servers_from_settings,
    validate_config_toml, write_grok_provider_live,
};
use super::adapter::{AppAdapter, Credentials, McpProjector, ProxyTakeover, SessionSource};
use crate::app_config::AppType;
use crate::config::get_home_dir;
use crate::error::AppError;
use crate::provider::Provider;

#[derive(Debug)]
pub struct GrokAdapter;

impl AppAdapter for GrokAdapter {
    fn config_dir(&self) -> Result<PathBuf, AppError> {
        Ok(get_grok_config_dir())
    }

    fn prompt_file(&self) -> Option<PathBuf> {
        Some(get_grok_config_dir().join("AGENTS.md"))
    }

    fn skills_dir(&self) -> Option<PathBuf> {
        // 默认路径必须走 get_home_dir()（可被 CC_SWITCH_TEST_HOME 覆盖）
        Some(
            crate::settings::override_dir(&AppType::GrokBuild)
                .unwrap_or_else(|| get_home_dir().join(".grok"))
                .join("skills"),
        )
    }

    fn read_live(&self) -> Result<Value, AppError> {
        read_grok_live_settings()
    }

    fn import_live(&self) -> Result<Value, AppError> {
        let mut settings = read_grok_live_settings()?;
        strip_grok_mcp_servers_from_settings(&mut settings)?;
        Ok(settings)
    }

    fn write_provider(&self, provider: &Provider) -> Result<(), AppError> {
        write_grok_provider_live(provider)
    }

    fn validate_provider(&self, provider: &Provider) -> Result<(), AppError> {
        let config = provider_config_toml(provider)?;
        validate_config_toml(config)
    }

    /// MCP 服务器归 DB 所有，live 里的 `[mcp_servers]` 只是同步投影，回填时剥掉
    fn backfill_live_settings(&self, provider: &Provider, live_settings: Value) -> Value {
        let mut settings = live_settings;
        if let Err(err) = strip_grok_mcp_servers_from_settings(&mut settings) {
            log::warn!(
                "Failed to strip Grok Build mcp_servers while backfilling '{}': {err}",
                provider.id
            );
        }
        settings
    }

    fn usage_credentials(&self, settings: &Value) -> (String, String) {
        settings
            .get("config")
            .and_then(Value::as_str)
            .and_then(config::extract_credentials)
            .unwrap_or_default()
    }

    fn credentials(&self, provider: &Provider) -> Result<Credentials, AppError> {
        let config_toml = provider
            .settings_config
            .get("config")
            .and_then(Value::as_str)
            .ok_or_else(config_missing_error)?;
        let (base_url, api_key) = config::extract_credentials(config_toml).ok_or_else(|| {
            AppError::localized(
                "provider.grokbuild.credentials.missing",
                "Grok Build 配置缺少 Base URL 或 API Key",
                "Grok Build configuration is missing the base URL or API key",
            )
        })?;
        Ok(Credentials { api_key, base_url })
    }

    fn mcp(&self) -> Option<&dyn McpProjector> {
        Some(self)
    }

    fn takeover(&self) -> Option<&dyn ProxyTakeover> {
        Some(self)
    }

    fn session_source(&self) -> Option<&dyn SessionSource> {
        Some(self)
    }
}

impl McpProjector for GrokAdapter {
    fn read_servers(&self) -> Result<Vec<(String, Value)>, AppError> {
        crate::mcp::read_servers_from_grokbuild()
    }

    fn upsert(&self, id: &str, spec: &Value) -> Result<(), AppError> {
        crate::mcp::sync_single_server_to_grokbuild(&Default::default(), id, spec)
    }

    fn remove(&self, id: &str) -> Result<(), AppError> {
        crate::mcp::remove_server_from_grokbuild(id)
    }
}

impl ProxyTakeover for GrokAdapter {
    fn has_placeholder(&self, settings: &Value) -> bool {
        settings
            .get("config")
            .and_then(Value::as_str)
            .is_some_and(config::has_proxy_token)
    }
}

impl SessionSource for GrokAdapter {
    fn session_roots(&self) -> Vec<PathBuf> {
        crate::session_manager::providers::grokbuild::session_roots()
    }
}

fn provider_config_toml(provider: &Provider) -> Result<&str, AppError> {
    let settings = provider.settings_config.as_object().ok_or_else(|| {
        AppError::localized(
            "provider.grokbuild.settings.not_object",
            "Grok Build 配置必须是 JSON 对象",
            "Grok Build configuration must be a JSON object",
        )
    })?;
    settings
        .get("config")
        .and_then(Value::as_str)
        .ok_or_else(config_missing_error)
}

fn config_missing_error() -> AppError {
    AppError::localized(
        "provider.grokbuild.config.missing",
        "Grok Build 配置缺少 config 字段",
        "Grok Build configuration is missing the config field",
    )
}
