//! Grok Build: `~/.grok/config.toml`.

use std::path::PathBuf;

use serde_json::Value;

use super::{AdapterCapabilities, AppAdapter, CommonConfigShape, McpProjector, WriteMode};
use crate::app_config::{AppType, MultiAppConfig};
use crate::error::AppError;
use crate::provider::Provider;

pub(super) struct GrokAdapter;

impl AppAdapter for GrokAdapter {
    fn app(&self) -> AppType {
        AppType::GrokBuild
    }

    fn capabilities(&self) -> AdapterCapabilities {
        AdapterCapabilities {
            mode: WriteMode::Exclusive,
            generic_live_read: true,
            mcp: true,
            proxy_takeover: true,
            common_config: CommonConfigShape::None,
        }
    }

    fn live_paths(&self) -> Vec<PathBuf> {
        vec![crate::grok_config::get_grok_config_path()]
    }

    fn read_live(&self) -> Result<Value, AppError> {
        crate::grok_config::read_grok_live_settings()
    }

    fn write_live(&self, provider: &Provider) -> Result<(), AppError> {
        crate::grok_config::write_grok_provider_live(provider)
    }

    fn backfill(&self, provider: &Provider, live: Value) -> Value {
        let mut settings = live;
        if let Err(err) = crate::grok_config::strip_grok_mcp_servers_from_settings(&mut settings) {
            log::warn!(
                "Failed to strip Grok Build mcp_servers while backfilling '{}': {err}",
                provider.id
            );
        }
        settings
    }

    fn import_live(&self) -> Result<Value, AppError> {
        let mut settings = self.read_live()?;
        crate::grok_config::strip_grok_mcp_servers_from_settings(&mut settings)?;
        Ok(settings)
    }

    fn mcp(&self) -> Option<&dyn McpProjector> {
        Some(&GrokMcp)
    }
}

struct GrokMcp;

impl McpProjector for GrokMcp {
    fn upsert_server(&self, id: &str, spec: &Value) -> Result<(), AppError> {
        crate::mcp::sync_single_server_to_grokbuild(&Default::default(), id, spec)
    }

    fn remove_server(&self, id: &str) -> Result<(), AppError> {
        crate::mcp::remove_server_from_grokbuild(id)
    }

    fn import_servers(&self, config: &mut MultiAppConfig) -> Result<usize, AppError> {
        crate::mcp::import_from_grokbuild(config)
    }
}
