//! Claude Code 适配器（`~/.claude/settings.json`、`~/.claude.json` 与插件配置）

pub mod claude_json;
mod common_config;
mod live_defaults;
pub mod plugin;

use std::path::PathBuf;

use serde_json::Value;

pub(crate) use self::common_config::{extract_common_config, normalize_models_in_value};
use super::adapter::{
    AppAdapter, CommonSnippet, Credentials, McpProjector, ProxyTakeover, SessionSource,
};
use super::{json_deep_merge, json_deep_remove, json_is_subset};
use crate::app_config::AppType;
use crate::config::{
    get_claude_config_dir, get_claude_settings_path, get_home_dir, read_json_file, write_json_file,
};
use crate::error::AppError;
use crate::provider::Provider;

/// 代理接管时会被替换成占位符的凭据键
const TAKEOVER_TOKEN_KEYS: [&str; 4] = [
    "ANTHROPIC_AUTH_TOKEN",
    "ANTHROPIC_API_KEY",
    "OPENROUTER_API_KEY",
    "OPENAI_API_KEY",
];

#[derive(Debug)]
pub struct ClaudeAdapter;

impl AppAdapter for ClaudeAdapter {
    fn config_dir(&self) -> Result<PathBuf, AppError> {
        Ok(get_claude_config_dir())
    }

    fn prompt_file(&self) -> Option<PathBuf> {
        Some(get_claude_config_dir().join("CLAUDE.md"))
    }

    fn skills_dir(&self) -> Option<PathBuf> {
        // 默认路径必须走 get_home_dir()（可被 CC_SWITCH_TEST_HOME 覆盖）
        Some(
            crate::settings::override_dir(&AppType::Claude)
                .unwrap_or_else(|| get_home_dir().join(".claude"))
                .join("skills"),
        )
    }

    fn read_live(&self) -> Result<Value, AppError> {
        read_settings()
    }

    fn import_live(&self) -> Result<Value, AppError> {
        let mut settings = read_settings()?;
        let _ = normalize_models_in_value(&mut settings);
        Ok(settings)
    }

    fn write_provider(&self, provider: &Provider) -> Result<(), AppError> {
        let path = get_claude_settings_path();
        let settings = sanitize_settings_for_live(&provider.settings_config);
        write_json_file(&path, &settings)
    }

    fn apply_live_defaults(&self, provider: &Provider, settings: &mut Value) {
        live_defaults::apply_codex_oauth_claude_context_defaults(settings, provider);
        live_defaults::apply_kimi_for_coding_context_defaults(settings, provider);
    }

    fn normalize_settings(&self, settings: &mut Value) -> bool {
        normalize_models_in_value(settings)
    }

    fn validate_provider(&self, provider: &Provider) -> Result<(), AppError> {
        if !provider.settings_config.is_object() {
            return Err(AppError::localized(
                "provider.claude.settings.not_object",
                "Claude 配置必须是 JSON 对象",
                "Claude configuration must be a JSON object",
            ));
        }
        Ok(())
    }

    fn backfill_live_settings(&self, provider: &Provider, live_settings: Value) -> Value {
        let mut settings = live_settings;
        live_defaults::strip_injected_codex_oauth_context_defaults(&mut settings, provider);
        live_defaults::strip_injected_kimi_for_coding_context_defaults(&mut settings, provider);
        settings
    }

    /// Anthropic 风格的 env，保留 JS 脚本路径依赖的 OpenRouter / Google key 兜底
    fn usage_credentials(&self, settings: &Value) -> (String, String) {
        anthropic_usage_credentials(settings)
    }

    fn credentials(&self, provider: &Provider) -> Result<Credentials, AppError> {
        let env = provider
            .settings_config
            .get("env")
            .and_then(|v| v.as_object())
            .ok_or_else(|| {
                AppError::localized(
                    "provider.claude.env.missing",
                    "配置格式错误: 缺少 env",
                    "Invalid configuration: missing env section",
                )
            })?;

        let api_key = env
            .get("ANTHROPIC_AUTH_TOKEN")
            .or_else(|| env.get("ANTHROPIC_API_KEY"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                AppError::localized(
                    "provider.claude.api_key.missing",
                    "缺少 API Key",
                    "API key is missing",
                )
            })?
            .to_string();

        let base_url = env
            .get("ANTHROPIC_BASE_URL")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                AppError::localized(
                    "provider.claude.base_url.missing",
                    "缺少 ANTHROPIC_BASE_URL 配置",
                    "Missing ANTHROPIC_BASE_URL configuration",
                )
            })?
            .to_string();

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

/// Claude 的通用配置片段是 settings.json 的 JSON 子集，直接作用于整个配置对象。
impl CommonSnippet for ClaudeAdapter {
    fn extract(&self, settings: &Value) -> Result<String, AppError> {
        extract_common_config(settings)
    }

    fn contains(&self, settings: &Value, snippet: &str) -> bool {
        match serde_json::from_str::<Value>(snippet) {
            Ok(source) if source.is_object() => json_is_subset(settings, &source),
            _ => false,
        }
    }

    fn apply(&self, settings: &Value, snippet: &str) -> Result<Value, AppError> {
        let source = parse_snippet(snippet)?;
        let mut result = settings.clone();
        json_deep_merge(&mut result, &source);
        Ok(result)
    }

    fn remove(&self, settings: &Value, snippet: &str) -> Result<Value, AppError> {
        let source = parse_snippet(snippet)?;
        let mut result = settings.clone();
        json_deep_remove(&mut result, &source);
        Ok(result)
    }

    fn sync_from_live(&self) -> bool {
        true
    }
}

impl McpProjector for ClaudeAdapter {
    fn upsert(&self, id: &str, spec: &Value) -> Result<(), AppError> {
        crate::mcp::sync_single_server_to_claude(&Default::default(), id, spec)
    }

    fn remove(&self, id: &str) -> Result<(), AppError> {
        crate::mcp::remove_server_from_claude(id)
    }
}

impl ProxyTakeover for ClaudeAdapter {
    fn has_placeholder(&self, settings: &Value) -> bool {
        let Some(env) = settings.get("env").and_then(Value::as_object) else {
            return false;
        };
        TAKEOVER_TOKEN_KEYS.iter().any(|key| {
            env.get(*key)
                .and_then(Value::as_str)
                .is_some_and(crate::proxy::inbound_auth::is_proxy_token)
        })
    }
}

impl SessionSource for ClaudeAdapter {
    fn session_roots(&self) -> Vec<PathBuf> {
        vec![get_claude_config_dir().join("projects")]
    }
}

/// 去掉只属于 CC Switch 的内部字段，得到可写入 Claude Code settings.json 的配置
pub(crate) fn sanitize_settings_for_live(settings: &Value) -> Value {
    let mut v = settings.clone();
    if let Some(obj) = v.as_object_mut() {
        // Internal-only fields - never write to Claude Code settings.json
        obj.remove("api_format");
        obj.remove("apiFormat");
        obj.remove("openrouter_compat_mode");
        obj.remove("openrouterCompatMode");
    }
    v
}

fn read_settings() -> Result<Value, AppError> {
    let path = get_claude_settings_path();
    if !path.exists() {
        return Err(AppError::localized(
            "claude.live.missing",
            "Claude Code 配置文件不存在",
            "Claude settings file is missing",
        ));
    }
    read_json_file(&path)
}

fn parse_snippet(snippet: &str) -> Result<Value, AppError> {
    serde_json::from_str::<Value>(snippet)
        .map_err(|e| AppError::Message(format!("Invalid Claude common config: {e}")))
}

/// Claude 与 Claude Desktop 共用的 Anthropic 风格用量凭据
pub(super) fn anthropic_usage_credentials(settings: &Value) -> (String, String) {
    let env = settings.get("env");
    let base_url = super::adapter::str_or_empty(env.and_then(|e| e.get("ANTHROPIC_BASE_URL")));
    let api_key = super::adapter::first_non_empty_str(
        env,
        &[
            "ANTHROPIC_AUTH_TOKEN",
            "ANTHROPIC_API_KEY",
            "OPENROUTER_API_KEY",
            "GOOGLE_API_KEY",
        ],
    );
    (base_url, api_key)
}
