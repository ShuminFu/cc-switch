//! Claude Code: `~/.claude/settings.json` (provider) and `~/.claude.json` (MCP).

use std::path::PathBuf;

use serde_json::Value;

use super::plan::PlannedWrite;
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

    fn plan_live(&self, provider: &Provider) -> Result<Option<Vec<PlannedWrite>>, AppError> {
        // same sanitizer and renderer as `write_live`
        let path = get_claude_settings_path();
        let settings =
            crate::services::provider::sanitize_claude_settings_for_live(&provider.settings_config);
        let content = crate::config::render_json_file(&path, &settings)?;
        Ok(Some(vec![PlannedWrite { path, content }]))
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
    fn missing_config_dir(&self) -> Option<std::path::PathBuf> {
        let dir = crate::config::get_claude_config_dir();
        (!dir.exists() && !crate::config::get_claude_mcp_path().exists()).then_some(dir)
    }

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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The preview is byte-for-byte what the switch writes (CFG-22).
    #[test]
    fn plan_live_matches_write_live() {
        let _home = crate::test_support::isolated_home();
        let path = get_claude_settings_path();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "{\n    \"permissions\": {},\n    \"env\": {}\n}\n").unwrap();
        let provider = Provider::with_id(
            "p1".into(),
            "P1".into(),
            json!({ "env": { "ANTHROPIC_BASE_URL": "https://example.test" }, "model": "m" }),
            None,
        );

        let plan = ClaudeAdapter.plan_live(&provider).unwrap().unwrap();
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].path, path);
        let before = std::fs::read_to_string(&path).unwrap();
        assert_ne!(plan[0].content, before);
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            before,
            "plan must not write"
        );

        ClaudeAdapter.write_live(&provider).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), plan[0].content);
    }
}
