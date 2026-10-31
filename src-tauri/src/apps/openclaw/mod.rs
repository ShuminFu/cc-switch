//! OpenClaw 适配器（`~/.openclaw/openclaw.json`，累加模式）
//!
//! 每个供应商是 `models.providers.<id>` 下的一个条目。

pub mod config;

use std::path::PathBuf;

use serde_json::Value;

use self::config::{
    get_openclaw_config_path, get_openclaw_dir, read_openclaw_config, OpenClawProviderConfig,
};
use super::adapter::{
    entries_have_placeholder, str_or_empty, AppAdapter, CommonSnippet, Credentials, EntryEndpoint,
    EntryTakeover, ExtractOnlySnippet, McpProjector, ProxyTakeover, SessionSource,
};
use crate::app_config::AppType;
use crate::config::get_home_dir;
use crate::error::AppError;
use crate::provider::Provider;

#[derive(Debug)]
pub struct OpenClawAdapter;

static COMMON_SNIPPET: ExtractOnlySnippet = ExtractOnlySnippet(extract_common_config);

impl AppAdapter for OpenClawAdapter {
    fn config_dir(&self) -> Result<PathBuf, AppError> {
        Ok(get_openclaw_dir())
    }

    fn prompt_file(&self) -> Option<PathBuf> {
        Some(get_openclaw_dir().join("AGENTS.md"))
    }

    fn skills_dir(&self) -> Option<PathBuf> {
        // 默认路径必须走 get_home_dir()（可被 CC_SWITCH_TEST_HOME 覆盖）
        Some(
            crate::settings::override_dir(&AppType::OpenClaw)
                .unwrap_or_else(|| get_home_dir().join(".openclaw"))
                .join("skills"),
        )
    }

    fn live_files(&self) -> Vec<PathBuf> {
        vec![crate::openclaw_config::get_openclaw_config_path()]
    }

    fn read_live(&self) -> Result<Value, AppError> {
        if !get_openclaw_config_path().exists() {
            return Err(AppError::localized(
                "openclaw.config.missing",
                "OpenClaw 配置文件不存在",
                "OpenClaw configuration file not found",
            ));
        }
        read_openclaw_config()
    }

    /// 累加模式应用通过 `import_openclaw_providers_from_live` 逐个导入，不走默认导入
    fn import_live(&self) -> Result<Value, AppError> {
        Err(AppError::unsupported(&AppType::OpenClaw, "import_live"))
    }

    fn write_provider(&self, provider: &Provider) -> Result<(), AppError> {
        // Convert settings_config to OpenClawProviderConfig
        match serde_json::from_value::<OpenClawProviderConfig>(provider.settings_config.clone()) {
            Ok(typed) => {
                config::set_typed_provider(&provider.id, &typed)?;
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
                    config::set_provider(&provider.id, provider.settings_config.clone())?;
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

    fn remove_provider(&self, provider_id: &str) -> Result<(), AppError> {
        if !get_openclaw_dir().exists() {
            log::debug!(
                "OpenClaw config directory doesn't exist, skipping removal of '{provider_id}'"
            );
            return Ok(());
        }

        config::remove_provider(provider_id)?;
        log::info!("OpenClaw provider '{provider_id}' removed from live config");
        Ok(())
    }

    fn live_has_provider(&self, provider_id: &str) -> Result<bool, AppError> {
        config::get_providers().map(|providers| providers.contains_key(provider_id))
    }

    fn validate_provider(&self, provider: &Provider) -> Result<(), AppError> {
        // OpenClaw uses config structure: { baseUrl, apiKey, api, models }
        if !provider.settings_config.is_object() {
            return Err(AppError::localized(
                "provider.openclaw.settings.not_object",
                "OpenClaw 配置必须是 JSON 对象",
                "OpenClaw configuration must be a JSON object",
            ));
        }
        Ok(())
    }

    /// openclaw.json 把凭据平铺在顶层，camelCase
    fn usage_credentials(&self, settings: &Value) -> (String, String) {
        (
            str_or_empty(settings.get("baseUrl")),
            str_or_empty(settings.get("apiKey")),
        )
    }

    /// OpenClaw: `{ baseUrl, apiKey, api, ... }`（camelCase）
    fn probe_base_url(&self, provider: &Provider) -> Option<Result<String, AppError>> {
        let base_url = provider
            .settings_config
            .get("baseUrl")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                AppError::localized(
                    "openclaw_base_url_missing",
                    "OpenClaw 供应商缺少 baseUrl",
                    "OpenClaw provider is missing `baseUrl`",
                )
            });
        Some(base_url)
    }

    fn credentials(&self, provider: &Provider) -> Result<Credentials, AppError> {
        flat_credentials(provider)
    }

    fn common_snippet(&self) -> Option<&dyn CommonSnippet> {
        Some(&COMMON_SNIPPET)
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

/// 本地代理挂载的 OpenAI 兼容协议；缺省 `api` 时 OpenClaw 按 Chat Completions 处理
const PROXYABLE_APIS: [&str; 2] = ["openai-completions", "openai-responses"];

impl ProxyTakeover for OpenClawAdapter {
    fn has_placeholder(&self, settings: &Value) -> bool {
        entries_have_placeholder(self, settings)
    }

    fn live_files(&self) -> Vec<PathBuf> {
        AppAdapter::live_files(self)
    }

    fn read_live(&self) -> Result<Value, String> {
        AppAdapter::read_live(self).map_err(|e| format!("读取 OpenClaw 配置失败: {e}"))
    }

    fn entry_takeover(&self) -> Option<&dyn EntryTakeover> {
        Some(self)
    }
}

/// `models.providers.<id>` 条目，凭据平铺在 `baseUrl` / `apiKey`
impl EntryTakeover for OpenClawAdapter {
    fn entries(&self, live: &Value) -> Vec<(String, Value)> {
        live.get("models")
            .and_then(|models| models.get("providers"))
            .and_then(Value::as_object)
            .map(|providers| {
                providers
                    .iter()
                    .map(|(id, entry)| (id.clone(), entry.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// `agents.defaults.model` 为 `<provider>/<model>` 字符串或 `{ primary }` 对象
    fn default_entry(&self, live: &Value) -> Option<String> {
        let model = live
            .get("agents")
            .and_then(|agents| agents.get("defaults"))
            .and_then(|defaults| defaults.get("model"))?;
        crate::apps::opencode::model_ref_provider(model.get("primary").or(Some(model)))
    }

    fn endpoint(&self, entry: &Value) -> EntryEndpoint {
        let field = |key: &str| entry.get(key).and_then(Value::as_str).map(str::to_string);
        EntryEndpoint {
            base_url: field("baseUrl"),
            api_key: field("apiKey"),
        }
    }

    fn proxyable(&self, entry: &Value) -> bool {
        match entry.get("api") {
            None => true,
            Some(api) => api
                .as_str()
                .is_some_and(|api| PROXYABLE_APIS.contains(&api.trim())),
        }
    }

    fn write_endpoint(&self, id: &str, endpoint: &EntryEndpoint) -> Result<(), AppError> {
        config::set_provider_endpoint(id, endpoint).map(|_| ())
    }
}

/// MCP 服务器投影到 openclaw.json 的 `mcp.servers` 段（Issue #4834）
impl McpProjector for OpenClawAdapter {
    fn live_file(&self) -> PathBuf {
        crate::openclaw_config::get_openclaw_config_path()
    }

    fn read_servers(&self) -> Result<Vec<(String, Value)>, AppError> {
        crate::mcp::read_servers_from_openclaw()
    }

    fn upsert(&self, id: &str, spec: &Value) -> Result<(), AppError> {
        crate::mcp::sync_single_server_to_openclaw(id, spec)
    }

    fn remove(&self, id: &str) -> Result<(), AppError> {
        crate::mcp::remove_server_from_openclaw(id)
    }
}

impl SessionSource for OpenClawAdapter {
    fn session_roots(&self) -> Vec<PathBuf> {
        vec![get_openclaw_dir().join("agents")]
    }
}

/// OpenClaw 的直连凭据：对象顶层的 `apiKey` 与 `baseUrl`
fn flat_credentials(provider: &Provider) -> Result<Credentials, AppError> {
    let api_key = provider
        .settings_config
        .get("apiKey")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            AppError::localized(
                "provider.openclaw.api_key.missing",
                "缺少 API Key",
                "API key is missing",
            )
        })?
        .to_string();

    let base_url = provider
        .settings_config
        .get("baseUrl")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    Ok(Credentials { api_key, base_url })
}

/// Extract common config for OpenClaw (JSON format)
fn extract_common_config(settings: &Value) -> Result<String, AppError> {
    // OpenClaw uses a different config structure with baseUrl, apiKey, api, models
    // For common config, we exclude provider-specific fields like apiKey
    let mut config = settings.clone();

    // Remove provider-specific fields
    if let Some(obj) = config.as_object_mut() {
        obj.remove("apiKey");
        obj.remove("baseUrl");
        // Keep api and models as they might be common
    }

    if config.is_null() || config.as_object().is_some_and(|obj| obj.is_empty()) {
        return Ok("{}".to_string());
    }

    serde_json::to_string_pretty(&config)
        .map_err(|e| AppError::Message(format!("Serialization failed: {e}")))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn credentials_come_from_top_level_fields() {
        let provider = Provider::with_id(
            "p".to_string(),
            "P".to_string(),
            json!({ "apiKey": "sk-test", "baseUrl": "https://example.com/v1", "models": [] }),
            None,
        );
        let credentials = OpenClawAdapter.credentials(&provider).unwrap();
        assert_eq!(credentials.api_key, "sk-test");
        assert_eq!(credentials.base_url, "https://example.com/v1");

        let missing = Provider::with_id("p".to_string(), "P".to_string(), json!({}), None);
        assert!(OpenClawAdapter.credentials(&missing).is_err());
    }

    #[test]
    fn common_snippet_strips_endpoint_and_key() {
        let common = OpenClawAdapter.common_snippet().expect("snippet");
        let settings = json!({ "apiKey": "sk", "baseUrl": "https://example.com", "api": "openai-completions" });
        let snippet = common.extract(&settings).unwrap();
        assert!(!snippet.contains("apiKey"));
        assert!(!snippet.contains("baseUrl"));
        assert!(snippet.contains("openai-completions"));
        assert!(!common.contains(&settings, &snippet));
        assert_eq!(common.apply(&settings, &snippet).unwrap(), settings);
    }

    #[test]
    fn has_mcp_and_entry_level_takeover() {
        assert!(OpenClawAdapter.mcp().is_some());
        assert!(OpenClawAdapter.omo_variant(Some("omo")).is_none());
        let takeover = OpenClawAdapter.takeover().expect("takeover");
        assert!(takeover.entry_takeover().is_some());
    }

    #[test]
    fn entry_takeover_reads_model_providers_and_the_default_model() {
        let entries = OpenClawAdapter
            .takeover()
            .unwrap()
            .entry_takeover()
            .unwrap();
        let live = json!({
            "models": { "providers": {
                "a": { "baseUrl": "https://a.example.com/v1", "apiKey": "sk-a", "api": "openai-responses" },
                "b": { "baseUrl": "https://b.example.com", "api": "anthropic-messages" }
            } },
            "agents": { "defaults": { "model": { "primary": "a/gpt-5" } } }
        });
        assert_eq!(entries.default_entry(&live).as_deref(), Some("a"));
        let as_string = json!({ "agents": { "defaults": { "model": "b/claude" } } });
        assert_eq!(entries.default_entry(&as_string).as_deref(), Some("b"));
        let all = entries.entries(&live);
        assert_eq!(all.len(), 2);
        assert_eq!(entries.endpoint(&all[0].1).api_key.as_deref(), Some("sk-a"));
        assert!(entries.proxyable(&all[0].1));
        assert!(
            !entries.proxyable(&all[1].1),
            "anthropic-messages is not mounted"
        );
        assert!(!OpenClawAdapter.takeover().unwrap().has_placeholder(&live));
    }
}
