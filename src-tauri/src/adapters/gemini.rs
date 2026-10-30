//! Gemini CLI: `~/.gemini/.env` (credentials) + `~/.gemini/settings.json`.

use std::path::PathBuf;

use serde_json::{json, Value};

use super::{AdapterCapabilities, AppAdapter, CommonConfigShape, McpProjector, WriteMode};
use crate::app_config::{AppType, MultiAppConfig};
use crate::error::AppError;
use crate::gemini_config::{
    env_to_json, get_gemini_env_path, get_gemini_settings_path, read_gemini_env,
};
use crate::provider::Provider;

pub(super) struct GeminiAdapter;

/// `{ "env": {...}, "config": {...} }` from the two live files.
fn read_snapshot(
    missing_key: &'static str,
    zh: &'static str,
    en: &'static str,
) -> Result<Value, AppError> {
    let env_path = get_gemini_env_path();
    if !env_path.exists() {
        return Err(super::missing_live_error(missing_key, zh, en));
    }

    let env_map = read_gemini_env()?;
    let env_json = env_to_json(&env_map);
    let env_obj = env_json.get("env").cloned().unwrap_or_else(|| json!({}));

    // settings.json (MCP config etc.); an unparsable file is an error
    let config_obj = crate::gemini_config::read_gemini_settings_value(&get_gemini_settings_path())?;

    Ok(json!({
        "env": env_obj,
        "config": config_obj
    }))
}

impl AppAdapter for GeminiAdapter {
    fn app(&self) -> AppType {
        AppType::Gemini
    }

    fn capabilities(&self) -> AdapterCapabilities {
        AdapterCapabilities {
            mode: WriteMode::Exclusive,
            generic_live_read: true,
            mcp: true,
            proxy_takeover: true,
            common_config: CommonConfigShape::JsonEnvField,
        }
    }

    fn live_paths(&self) -> Vec<PathBuf> {
        vec![get_gemini_env_path(), get_gemini_settings_path()]
    }

    fn read_live(&self) -> Result<Value, AppError> {
        read_snapshot(
            "gemini.env.missing",
            "Gemini .env 文件不存在",
            "Gemini .env file not found",
        )
    }

    fn write_live(&self, provider: &Provider) -> Result<(), AppError> {
        crate::services::provider::write_gemini_live(provider)
    }

    fn import_live(&self) -> Result<Value, AppError> {
        read_snapshot(
            "gemini.live.missing",
            "Gemini 配置文件不存在",
            "Gemini configuration file is missing",
        )
    }

    fn mcp(&self) -> Option<&dyn McpProjector> {
        Some(&GeminiMcp)
    }
}

struct GeminiMcp;

impl McpProjector for GeminiMcp {
    fn upsert_server(&self, id: &str, spec: &Value) -> Result<(), AppError> {
        crate::mcp::sync_single_server_to_gemini(&Default::default(), id, spec)
    }

    fn remove_server(&self, id: &str) -> Result<(), AppError> {
        crate::mcp::remove_server_from_gemini(id)
    }

    fn import_servers(&self, config: &mut MultiAppConfig) -> Result<usize, AppError> {
        crate::mcp::import_from_gemini(config)
    }
}
