//! Hermes (additive): `~/.hermes/config.yaml` — `custom_providers`, `model`
//! and `mcp_servers` sections; everything else is spliced through verbatim.

use std::path::PathBuf;

use serde_json::Value;

use super::{AdapterCapabilities, AppAdapter, CommonConfigShape, McpProjector, WriteMode};
use crate::app_config::{AppType, MultiAppConfig};
use crate::error::AppError;
use crate::hermes_config;
use crate::provider::Provider;

pub(super) struct HermesAdapter;

impl AppAdapter for HermesAdapter {
    fn app(&self) -> AppType {
        AppType::Hermes
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
        vec![hermes_config::get_hermes_config_path()]
    }

    fn read_live(&self) -> Result<Value, AppError> {
        if !hermes_config::get_hermes_config_path().exists() {
            return Err(super::missing_live_error(
                "hermes.config.missing",
                "Hermes 配置文件不存在",
                "Hermes configuration file not found",
            ));
        }
        let yaml_config = hermes_config::read_hermes_config()?;
        hermes_config::yaml_to_json(&yaml_config)
    }

    fn write_live(&self, provider: &Provider) -> Result<(), AppError> {
        hermes_config::set_provider(&provider.id, provider.settings_config.clone())?;
        log::debug!("Hermes provider '{}' written to live config", provider.id);
        Ok(())
    }

    fn live_has_provider(&self, provider_id: &str) -> Result<bool, AppError> {
        hermes_config::get_providers().map(|providers| providers.contains_key(provider_id))
    }

    fn remove_provider_from_live(&self, provider_id: &str) -> Result<(), AppError> {
        if !hermes_config::get_hermes_dir().exists() {
            log::debug!(
                "Hermes config directory doesn't exist, skipping removal of '{provider_id}'"
            );
            return Ok(());
        }
        hermes_config::remove_provider(provider_id)?;
        log::info!("Hermes provider '{provider_id}' removed from live config");
        Ok(())
    }

    fn mcp(&self) -> Option<&dyn McpProjector> {
        Some(&HermesMcp)
    }
}

struct HermesMcp;

impl McpProjector for HermesMcp {
    fn upsert_server(&self, id: &str, spec: &Value) -> Result<(), AppError> {
        crate::mcp::sync_single_server_to_hermes(&Default::default(), id, spec)
    }

    fn remove_server(&self, id: &str) -> Result<(), AppError> {
        crate::mcp::remove_server_from_hermes(id)
    }

    fn import_servers(&self, config: &mut MultiAppConfig) -> Result<usize, AppError> {
        crate::mcp::import_from_hermes(config)
    }
}
