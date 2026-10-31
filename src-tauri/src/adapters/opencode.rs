//! OpenCode (additive): `~/.config/opencode/opencode.json` — `provider.<id>`
//! and `mcp.<id>` entries; the rest of the JSONC file is preserved.

use std::path::PathBuf;

use serde_json::Value;

use super::{AdapterCapabilities, AppAdapter, CommonConfigShape, McpProjector, WriteMode};
use crate::app_config::{AppType, MultiAppConfig};
use crate::error::AppError;
use crate::opencode_config;
use crate::provider::{OpenCodeProviderConfig, Provider};

pub(super) struct OpenCodeAdapter;

impl AppAdapter for OpenCodeAdapter {
    fn app(&self) -> AppType {
        AppType::OpenCode
    }

    fn capabilities(&self) -> AdapterCapabilities {
        AdapterCapabilities {
            mode: WriteMode::Additive,
            generic_live_read: true,
            mcp: true,
            proxy_takeover: false,
            common_config: CommonConfigShape::None,
        }
    }

    fn live_paths(&self) -> Vec<PathBuf> {
        vec![opencode_config::get_opencode_config_path()]
    }

    fn read_live(&self) -> Result<Value, AppError> {
        if !opencode_config::get_opencode_config_path().exists() {
            return Err(super::missing_live_error(
                "opencode.config.missing",
                "OpenCode 配置文件不存在",
                "OpenCode configuration file not found",
            ));
        }
        opencode_config::read_opencode_config()
    }

    fn write_live(&self, provider: &Provider) -> Result<(), AppError> {
        // Defensive check: if settings_config is a full config structure, extract provider fragment
        let config_to_write = if let Some(obj) = provider.settings_config.as_object() {
            // Detect full config structure (has $schema or top-level provider field)
            if obj.contains_key("$schema") || obj.contains_key("provider") {
                log::warn!(
                    "OpenCode provider '{}' has full config structure in settings_config, attempting to extract fragment",
                    provider.id
                );
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

        match serde_json::from_value::<OpenCodeProviderConfig>(config_to_write.clone()) {
            Ok(config) => {
                opencode_config::set_typed_provider(&provider.id, &config)?;
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
                    opencode_config::set_provider(&provider.id, config_to_write)?;
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

    fn live_has_provider(&self, provider_id: &str) -> Result<bool, AppError> {
        opencode_config::get_providers().map(|providers| providers.contains_key(provider_id))
    }

    fn remove_provider_from_live(&self, provider_id: &str) -> Result<(), AppError> {
        if !opencode_config::get_opencode_dir().exists() {
            log::debug!(
                "OpenCode config directory doesn't exist, skipping removal of '{provider_id}'"
            );
            return Ok(());
        }
        opencode_config::remove_provider(provider_id)?;
        log::info!("OpenCode provider '{provider_id}' removed from live config");
        Ok(())
    }

    fn mcp(&self) -> Option<&dyn McpProjector> {
        Some(&OpenCodeMcp)
    }
}

struct OpenCodeMcp;

impl McpProjector for OpenCodeMcp {
    fn missing_config_dir(&self) -> Option<std::path::PathBuf> {
        let dir = crate::opencode_config::get_opencode_dir();
        (!dir.exists()).then_some(dir)
    }

    fn upsert_server(&self, id: &str, spec: &Value) -> Result<(), AppError> {
        crate::mcp::sync_single_server_to_opencode(&Default::default(), id, spec)
    }

    fn remove_server(&self, id: &str) -> Result<(), AppError> {
        crate::mcp::remove_server_from_opencode(id)
    }

    fn import_servers(&self, config: &mut MultiAppConfig) -> Result<usize, AppError> {
        crate::mcp::import_from_opencode(config)
    }
}
