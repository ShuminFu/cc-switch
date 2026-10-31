//! Claude Code: `~/.claude/settings.json` (provider) and `~/.claude.json` (MCP).

use std::path::PathBuf;

use serde_json::Value;

use super::{AdapterCapabilities, AppAdapter, CommonConfigShape, McpProjector, WriteMode};
use crate::app_config::{AppType, MultiAppConfig};
use crate::config::{get_claude_mcp_path, get_claude_settings_path, read_json_file};
use crate::error::AppError;
use crate::provider::Provider;

pub(super) struct ClaudeAdapter;

impl AppAdapter for ClaudeAdapter {
    fn app(&self) -> AppType {
        AppType::Claude
    }

    fn capabilities(&self) -> AdapterCapabilities {
        AdapterCapabilities {
            mode: WriteMode::Exclusive,
            generic_live_read: true,
            mcp: true,
            proxy_takeover: true,
            common_config: CommonConfigShape::JsonRoot,
        }
    }

    fn live_paths(&self) -> Vec<PathBuf> {
        vec![get_claude_settings_path(), get_claude_mcp_path()]
    }

    fn read_live(&self) -> Result<Value, AppError> {
        let path = get_claude_settings_path();
        if !path.exists() {
            return Err(super::missing_live_error(
                "claude.live.missing",
                "Claude Code 配置文件不存在",
                "Claude settings file is missing",
            ));
        }
        read_json_file(&path)
    }

    fn write_live(&self, provider: &Provider) -> Result<(), AppError> {
        let path = get_claude_settings_path();
        let settings =
            crate::services::provider::sanitize_claude_settings_for_live(&provider.settings_config);
        crate::config::backup_live_file("claude", &path);
        // settings.json carries ANTHROPIC_AUTH_TOKEN: create it 0600 when new
        crate::config::write_json_file_secret(&path, &settings)
    }

    fn backfill(&self, provider: &Provider, live: Value) -> Value {
        crate::services::provider::strip_injected_claude_defaults(live, provider)
    }

    fn import_live(&self) -> Result<Value, AppError> {
        let mut value = self.read_live()?;
        let _ = crate::services::provider::normalize_claude_models_in_value(&mut value);
        Ok(value)
    }

    fn mcp(&self) -> Option<&dyn McpProjector> {
        Some(&ClaudeMcp)
    }
}

struct ClaudeMcp;

impl McpProjector for ClaudeMcp {
    fn upsert_server(&self, id: &str, spec: &Value) -> Result<(), AppError> {
        self.project(&[(id, spec)], &[])
    }

    fn remove_server(&self, id: &str) -> Result<(), AppError> {
        self.project(&[], &[id])
    }

    fn import_servers(&self, config: &mut MultiAppConfig) -> Result<usize, AppError> {
        crate::mcp::import_from_claude(config)
    }

    /// One read-modify-write of the whole file per projection (CFG-41).
    fn project(&self, enabled: &[(&str, &Value)], disabled: &[&str]) -> Result<(), AppError> {
        crate::mcp::project_to_claude(enabled, disabled)
    }
}
