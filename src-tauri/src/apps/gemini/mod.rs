//! Gemini CLI 适配器（`~/.gemini/.env` + `~/.gemini/settings.json`）

pub(crate) mod auth;
pub mod config;

use std::path::PathBuf;

use serde_json::{json, Value};

use self::auth::{detect_gemini_auth_type, ensure_google_oauth_security_flag, GeminiAuthType};
use self::config::{
    env_to_json, get_gemini_dir, get_gemini_env_path, get_gemini_settings_path, json_to_env,
    read_gemini_env, validate_gemini_settings, validate_gemini_settings_strict,
    write_gemini_env_atomic,
};
use super::adapter::{
    first_non_empty_str, str_or_empty, AppAdapter, CommonSnippet, Credentials, McpProjector,
    ProxyTakeover, SessionSource,
};
use super::{json_deep_merge, json_deep_remove, json_is_subset};
use crate::app_config::AppType;
use crate::config::{get_home_dir, read_json_file, write_json_file};
use crate::error::AppError;
use crate::provider::Provider;

const DEFAULT_BASE_URL: &str = "https://generativelanguage.googleapis.com";

#[derive(Debug)]
pub struct GeminiAdapter;

impl AppAdapter for GeminiAdapter {
    fn config_dir(&self) -> Result<PathBuf, AppError> {
        Ok(get_gemini_dir())
    }

    fn prompt_file(&self) -> Option<PathBuf> {
        Some(get_gemini_dir().join("GEMINI.md"))
    }

    fn skills_dir(&self) -> Option<PathBuf> {
        // 默认路径必须走 get_home_dir()（可被 CC_SWITCH_TEST_HOME 覆盖）
        Some(
            crate::settings::override_dir(&AppType::Gemini)
                .unwrap_or_else(|| get_home_dir().join(".gemini"))
                .join("skills"),
        )
    }

    fn read_live(&self) -> Result<Value, AppError> {
        read_env_and_settings(|| {
            AppError::localized(
                "gemini.env.missing",
                "Gemini .env 文件不存在",
                "Gemini .env file not found",
            )
        })
    }

    fn import_live(&self) -> Result<Value, AppError> {
        read_env_and_settings(|| {
            AppError::localized(
                "gemini.live.missing",
                "Gemini 配置文件不存在",
                "Gemini configuration file is missing",
            )
        })
    }

    fn write_provider(&self, provider: &Provider) -> Result<(), AppError> {
        write_live(provider)
    }

    fn validate_provider(&self, provider: &Provider) -> Result<(), AppError> {
        validate_gemini_settings(&provider.settings_config)
    }

    /// Google 专用 env 键（含旧版 GOOGLE_API_KEY 兜底）
    fn usage_credentials(&self, settings: &Value) -> (String, String) {
        let env = settings.get("env");
        let base_url = str_or_empty(env.and_then(|e| e.get("GOOGLE_GEMINI_BASE_URL")));
        let api_key = first_non_empty_str(env, &["GEMINI_API_KEY", "GOOGLE_API_KEY"]);
        (base_url, api_key)
    }

    fn credentials(&self, provider: &Provider) -> Result<Credentials, AppError> {
        let env_map = json_to_env(&provider.settings_config)?;

        let api_key = env_map.get("GEMINI_API_KEY").cloned().ok_or_else(|| {
            AppError::localized(
                "gemini.missing_api_key",
                "缺少 GEMINI_API_KEY",
                "Missing GEMINI_API_KEY",
            )
        })?;

        let base_url = env_map
            .get("GOOGLE_GEMINI_BASE_URL")
            .cloned()
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_string());

        Ok(Credentials { api_key, base_url })
    }

    fn common_snippet(&self) -> Option<&dyn CommonSnippet> {
        Some(self)
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

/// Gemini 的通用配置片段是 `.env` 键值（JSON 对象），只作用于 `settings.env`。
impl CommonSnippet for GeminiAdapter {
    /// 提取 `.env` 中除供应商凭据（`GOOGLE_GEMINI_BASE_URL` / `GEMINI_API_KEY`）外的值
    fn extract(&self, settings: &Value) -> Result<String, AppError> {
        let env = settings.get("env").and_then(|v| v.as_object());

        let mut snippet = serde_json::Map::new();
        if let Some(env) = env {
            for (key, value) in env {
                if key == "GOOGLE_GEMINI_BASE_URL" || key == "GEMINI_API_KEY" {
                    continue;
                }
                let Value::String(v) = value else {
                    continue;
                };
                let trimmed = v.trim();
                if !trimmed.is_empty() {
                    snippet.insert(key.to_string(), Value::String(trimmed.to_string()));
                }
            }
        }

        if snippet.is_empty() {
            return Ok("{}".to_string());
        }

        serde_json::to_string_pretty(&Value::Object(snippet))
            .map_err(|e| AppError::Message(format!("Serialization failed: {e}")))
    }

    fn contains(&self, settings: &Value, snippet: &str) -> bool {
        match serde_json::from_str::<Value>(snippet) {
            Ok(Value::Object(source_map)) => {
                let Some(target_map) = settings.get("env").and_then(Value::as_object) else {
                    return false;
                };
                source_map.iter().all(|(key, source_value)| {
                    target_map
                        .get(key)
                        .is_some_and(|target_value| json_is_subset(target_value, source_value))
                })
            }
            _ => false,
        }
    }

    fn apply(&self, settings: &Value, snippet: &str) -> Result<Value, AppError> {
        let source = serde_json::from_str::<Value>(snippet)
            .map_err(|e| AppError::Message(format!("Invalid Gemini common config: {e}")))?;
        let mut result = settings.clone();
        if let Some(env) = result.get_mut("env") {
            json_deep_merge(env, &source);
        } else if let Some(obj) = result.as_object_mut() {
            obj.insert("env".to_string(), source);
        }
        Ok(result)
    }

    fn remove(&self, settings: &Value, snippet: &str) -> Result<Value, AppError> {
        let source = serde_json::from_str::<Value>(snippet)
            .map_err(|e| AppError::Message(format!("Invalid Gemini common config: {e}")))?;
        let mut result = settings.clone();
        if let Some(env) = result.get_mut("env") {
            json_deep_remove(env, &source);
        }
        Ok(result)
    }
}

impl McpProjector for GeminiAdapter {
    fn read_servers(&self) -> Result<Vec<(String, Value)>, AppError> {
        crate::mcp::read_servers_from_gemini()
    }

    fn upsert(&self, id: &str, spec: &Value) -> Result<(), AppError> {
        crate::mcp::sync_single_server_to_gemini(&Default::default(), id, spec)
    }

    fn remove(&self, id: &str) -> Result<(), AppError> {
        crate::mcp::remove_server_from_gemini(id)
    }
}

impl ProxyTakeover for GeminiAdapter {
    fn has_placeholder(&self, settings: &Value) -> bool {
        settings
            .get("env")
            .and_then(Value::as_object)
            .and_then(|env| env.get("GEMINI_API_KEY"))
            .and_then(Value::as_str)
            .is_some_and(crate::proxy::inbound_auth::is_proxy_token)
    }
}

impl SessionSource for GeminiAdapter {
    fn session_roots(&self) -> Vec<PathBuf> {
        vec![get_gemini_dir().join("tmp")]
    }
}

/// 读取 `.env`（必需）与 `settings.json`（可选），组装成
/// `{ "env": {...}, "config": {...} }`。
fn read_env_and_settings(missing_env: impl FnOnce() -> AppError) -> Result<Value, AppError> {
    let env_path = get_gemini_env_path();
    if !env_path.exists() {
        return Err(missing_env());
    }

    let env_map = read_gemini_env()?;
    let env_json = env_to_json(&env_map);
    let env_obj = env_json.get("env").cloned().unwrap_or_else(|| json!({}));

    // settings.json 存放 MCP 等其余配置
    let settings_path = get_gemini_settings_path();
    let config_obj = if settings_path.exists() {
        read_json_file(&settings_path)?
    } else {
        json!({})
    };

    Ok(json!({
        "env": env_obj,
        "config": config_obj
    }))
}

/// Write Gemini live configuration with authentication handling
pub(crate) fn write_live(provider: &Provider) -> Result<(), AppError> {
    // One-time auth type detection to avoid repeated detection
    let auth_type = detect_gemini_auth_type(provider);

    let env_map = json_to_env(&provider.settings_config)?;

    // Prepare config to write to ~/.gemini/settings.json
    // Behavior:
    // - config is object: use it (merge with existing to preserve mcpServers etc.)
    // - config is null or absent: preserve existing file content
    let settings_path = get_gemini_settings_path();
    let mut config_to_write: Option<Value> = None;

    if let Some(config_value) = provider.settings_config.get("config") {
        if config_value.is_object() {
            // Merge with existing settings to preserve mcpServers and other fields
            let mut merged = if settings_path.exists() {
                read_json_file::<Value>(&settings_path).unwrap_or_else(|_| json!({}))
            } else {
                json!({})
            };

            // Merge provider config into existing settings
            if let (Some(merged_obj), Some(config_obj)) =
                (merged.as_object_mut(), config_value.as_object())
            {
                for (k, v) in config_obj {
                    merged_obj.insert(k.clone(), v.clone());
                }
            }
            config_to_write = Some(merged);
        } else if !config_value.is_null() {
            return Err(AppError::localized(
                "gemini.validation.invalid_config",
                "Gemini 配置格式错误: config 必须是对象或 null",
                "Gemini config invalid: config must be an object or null",
            ));
        }
        // config is null: don't modify existing settings.json (preserve mcpServers etc.)
    }

    // If no config specified or config is null, preserve existing file
    if config_to_write.is_none() && settings_path.exists() {
        config_to_write = Some(read_json_file(&settings_path)?);
    }

    match auth_type {
        GeminiAuthType::GoogleOfficial => {
            // Google Official uses OAuth, no API key validation needed.
            // Write user's env vars as-is (e.g. GEMINI_MODEL, custom vars).
            write_gemini_env_atomic(&env_map)?;
        }
        GeminiAuthType::Packycode | GeminiAuthType::Generic => {
            // API Key mode -- require GEMINI_API_KEY
            validate_gemini_settings_strict(&provider.settings_config)?;
            write_gemini_env_atomic(&env_map)?;
        }
    }

    if let Some(config_value) = config_to_write {
        write_json_file(&settings_path, &config_value)?;
    }

    // Set security.auth.selectedType based on auth type
    // - Google Official: OAuth mode
    // - All others: API Key mode
    match auth_type {
        GeminiAuthType::GoogleOfficial => ensure_google_oauth_security_flag(provider)?,
        GeminiAuthType::Packycode | GeminiAuthType::Generic => {
            config::write_packycode_settings()?;
        }
    }

    Ok(())
}
