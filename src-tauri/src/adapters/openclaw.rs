//! OpenClaw (additive): `~/.openclaw/openclaw.json` (JSON5) —
//! `models.providers.<id>` entries.

use std::path::PathBuf;

use serde_json::Value;

use super::{AdapterCapabilities, AppAdapter, CommonConfigShape, WriteMode};
use crate::app_config::AppType;
use crate::error::AppError;
use crate::openclaw_config::{self, OpenClawProviderConfig};
use crate::provider::Provider;

pub(super) struct OpenClawAdapter;

impl AppAdapter for OpenClawAdapter {
    fn app(&self) -> AppType {
        AppType::OpenClaw
    }

    fn capabilities(&self) -> AdapterCapabilities {
        AdapterCapabilities {
            mode: WriteMode::Additive,
            generic_live_read: true,
            // OpenClaw MCP support is still in development (Issue #4834)
            mcp: false,
            proxy_takeover: false,
            common_config: CommonConfigShape::None,
        }
    }

    fn live_paths(&self) -> Vec<PathBuf> {
        vec![openclaw_config::get_openclaw_config_path()]
    }

    fn read_live(&self) -> Result<Value, AppError> {
        if !openclaw_config::get_openclaw_config_path().exists() {
            return Err(super::missing_live_error(
                "openclaw.config.missing",
                "OpenClaw 配置文件不存在",
                "OpenClaw configuration file not found",
            ));
        }
        openclaw_config::read_openclaw_config()
    }

    fn write_live(&self, provider: &Provider) -> Result<(), AppError> {
        match serde_json::from_value::<OpenClawProviderConfig>(provider.settings_config.clone()) {
            Ok(config) => {
                openclaw_config::set_typed_provider(&provider.id, &config)?;
                log::info!("OpenClaw provider '{}' written to live config", provider.id);
            }
            Err(e) => {
                log::warn!(
                    "Failed to parse OpenClaw provider config for '{}': {}",
                    provider.id,
                    e
                );
                // Try to write as raw JSON if it looks valid
                if provider.settings_config.get("baseUrl").is_some()
                    || provider.settings_config.get("api").is_some()
                    || provider.settings_config.get("models").is_some()
                {
                    openclaw_config::set_provider(&provider.id, provider.settings_config.clone())?;
                    log::info!(
                        "OpenClaw provider '{}' written as raw JSON to live config",
                        provider.id
                    );
                } else {
                    return Err(AppError::Message(format!(
                        "OpenClaw provider '{}' has invalid config structure for live config (must contain 'baseUrl', 'api', or 'models')",
                        provider.id
                    )));
                }
            }
        }
        Ok(())
    }

    fn live_has_provider(&self, provider_id: &str) -> Result<bool, AppError> {
        openclaw_config::get_providers().map(|providers| providers.contains_key(provider_id))
    }

    fn remove_provider_from_live(&self, provider_id: &str) -> Result<(), AppError> {
        if !openclaw_config::get_openclaw_dir().exists() {
            log::debug!(
                "OpenClaw config directory doesn't exist, skipping removal of '{provider_id}'"
            );
            return Ok(());
        }
        openclaw_config::remove_provider(provider_id)?;
        log::info!("OpenClaw provider '{provider_id}' removed from live config");
        Ok(())
    }
}
