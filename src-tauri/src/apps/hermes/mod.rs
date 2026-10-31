//! Hermes 适配器（`~/.hermes/config.yaml`，累加模式）
//!
//! 每个供应商是 `custom_providers[]` 里的一项；切换供应商额外更新顶层 `model:`
//! 指向该供应商（见 [`AppAdapter::apply_switch_defaults`]）。

pub mod config;

use std::path::PathBuf;

use serde_json::Value;

use self::config::{get_hermes_config_path, get_hermes_dir, read_hermes_config, yaml_to_json};
use super::adapter::{
    entries_have_placeholder, str_or_empty, AppAdapter, Credentials, EntryEndpoint, EntryTakeover,
    McpProjector, ProxyTakeover, SessionSource,
};
use crate::app_config::AppType;
use crate::error::AppError;
use crate::provider::Provider;

#[derive(Debug)]
pub struct HermesAdapter;

impl AppAdapter for HermesAdapter {
    fn config_dir(&self) -> Result<PathBuf, AppError> {
        Ok(get_hermes_dir())
    }

    fn prompt_file(&self) -> Option<PathBuf> {
        Some(get_hermes_dir().join("AGENTS.md"))
    }

    fn skills_dir(&self) -> Option<PathBuf> {
        // get_hermes_dir() 已按 settings 覆盖 → HERMES_HOME → 平台默认解析
        Some(get_hermes_dir().join("skills"))
    }

    fn live_files(&self) -> Vec<PathBuf> {
        vec![get_hermes_config_path()]
    }

    fn read_live(&self) -> Result<Value, AppError> {
        if !get_hermes_config_path().exists() {
            return Err(AppError::localized(
                "hermes.config.missing",
                "Hermes 配置文件不存在",
                "Hermes configuration file not found",
            ));
        }
        yaml_to_json(&read_hermes_config()?)
    }

    /// 累加模式应用通过 `import_hermes_providers_from_live` 逐个导入，不走默认导入
    fn import_live(&self) -> Result<Value, AppError> {
        Err(AppError::unsupported(&AppType::Hermes, "import_live"))
    }

    fn write_provider(&self, provider: &Provider) -> Result<(), AppError> {
        config::set_provider(&provider.id, provider.settings_config.clone())?;
        log::debug!("Hermes provider '{}' written to live config", provider.id);
        Ok(())
    }

    fn remove_provider(&self, provider_id: &str) -> Result<(), AppError> {
        if !get_hermes_dir().exists() {
            log::debug!(
                "Hermes config directory doesn't exist, skipping removal of '{provider_id}'"
            );
            return Ok(());
        }

        config::remove_provider(provider_id)?;
        log::info!("Hermes provider '{provider_id}' removed from live config");
        Ok(())
    }

    fn live_has_provider(&self, provider_id: &str) -> Result<bool, AppError> {
        config::get_providers().map(|providers| providers.contains_key(provider_id))
    }

    /// 写入 `custom_providers[]` 不会改变 Hermes 正在用的供应商：把顶层 `model:`
    /// 指向该供应商的第一个模型，否则"切换"只是重排了条目
    fn apply_switch_defaults(&self, provider: &Provider) -> Result<(), AppError> {
        config::apply_switch_defaults(&provider.id, &provider.settings_config).map(|_| ())
    }

    fn validate_provider(&self, provider: &Provider) -> Result<(), AppError> {
        // Hermes: accept any JSON object for now
        if !provider.settings_config.is_object() {
            return Err(AppError::localized(
                "provider.hermes.settings.not_object",
                "Hermes 配置必须是 JSON 对象",
                "Hermes configuration must be a JSON object",
            ));
        }
        Ok(())
    }

    /// config.yaml 把凭据平铺在顶层，snake_case
    fn usage_credentials(&self, settings: &Value) -> (String, String) {
        (
            str_or_empty(settings.get("base_url")),
            str_or_empty(settings.get("api_key")),
        )
    }

    /// 与 [`Self::usage_credentials`] 一致读 snake_case；旧版 DeepLink 导入的
    /// camelCase 记录（写 live 时才会被 `sanitize_hermes_provider_keys` 改名）兜底
    fn credentials(&self, provider: &Provider) -> Result<Credentials, AppError> {
        let settings = &provider.settings_config;
        let field = |snake: &str, camel: &str| {
            [snake, camel]
                .iter()
                .find_map(|key| settings.get(*key).and_then(Value::as_str))
                .map(str::to_string)
        };
        let api_key = field("api_key", "apiKey").ok_or_else(|| {
            AppError::localized(
                "provider.hermes.api_key.missing",
                "缺少 API Key",
                "API key is missing",
            )
        })?;
        let base_url = field("base_url", "baseUrl").unwrap_or_default();
        Ok(Credentials { api_key, base_url })
    }

    // Hermes doesn't use common config snippets

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

/// 本地代理挂载的 OpenAI 兼容协议；缺省 `api_mode` 时 Hermes 按 Chat Completions 处理
const PROXYABLE_API_MODES: [&str; 2] = ["chat_completions", "codex_responses"];

impl ProxyTakeover for HermesAdapter {
    fn has_placeholder(&self, settings: &Value) -> bool {
        entries_have_placeholder(self, settings)
    }

    fn live_files(&self) -> Vec<PathBuf> {
        AppAdapter::live_files(self)
    }

    fn read_live(&self) -> Result<Value, String> {
        AppAdapter::read_live(self).map_err(|e| format!("读取 Hermes 配置失败: {e}"))
    }

    fn entry_takeover(&self) -> Option<&dyn EntryTakeover> {
        Some(self)
    }
}

/// `custom_providers[]` 中按 `name` 匹配的条目，凭据为 `base_url` / `api_key`。
/// `providers:` 字典里的条目由 Hermes 自己管理，CC Switch 不写，故不参与接管。
impl EntryTakeover for HermesAdapter {
    fn entries(&self, live: &Value) -> Vec<(String, Value)> {
        live.get("custom_providers")
            .and_then(Value::as_array)
            .map(|providers| {
                providers
                    .iter()
                    .filter_map(|entry| {
                        let name = entry.get("name").and_then(Value::as_str)?;
                        Some((name.to_string(), entry.clone()))
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// 顶层 `model.provider`（切换供应商时由 `apply_switch_defaults` 写入）
    fn default_entry(&self, live: &Value) -> Option<String> {
        live.get("model")
            .and_then(|model| model.get("provider"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|provider| !provider.is_empty())
            .map(str::to_string)
    }

    fn endpoint(&self, entry: &Value) -> EntryEndpoint {
        let field = |key: &str| entry.get(key).and_then(Value::as_str).map(str::to_string);
        EntryEndpoint {
            base_url: field("base_url"),
            api_key: field("api_key"),
        }
    }

    fn proxyable(&self, entry: &Value) -> bool {
        match entry.get("api_mode") {
            None => true,
            Some(mode) => mode
                .as_str()
                .is_some_and(|mode| PROXYABLE_API_MODES.contains(&mode.trim())),
        }
    }

    fn write_endpoint(&self, id: &str, endpoint: &EntryEndpoint) -> Result<(), AppError> {
        config::set_provider_endpoint(id, endpoint).map(|_| ())
    }
}

impl McpProjector for HermesAdapter {
    fn live_file(&self) -> PathBuf {
        get_hermes_config_path()
    }

    fn read_servers(&self) -> Result<Vec<(String, Value)>, AppError> {
        crate::mcp::read_servers_from_hermes()
    }

    fn upsert(&self, id: &str, spec: &Value) -> Result<(), AppError> {
        crate::mcp::sync_single_server_to_hermes(&Default::default(), id, spec)
    }

    fn remove(&self, id: &str) -> Result<(), AppError> {
        crate::mcp::remove_server_from_hermes(id)
    }
}

impl SessionSource for HermesAdapter {
    fn session_roots(&self) -> Vec<PathBuf> {
        vec![get_hermes_dir().join("sessions")]
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn provider(settings: Value) -> Provider {
        Provider::with_id("p".to_string(), "P".to_string(), settings, None)
    }

    /// config.yaml 的字段是 snake_case；旧版 camelCase 记录仍可读
    #[test]
    fn credentials_read_snake_case_with_camel_case_fallback() {
        let credentials = HermesAdapter
            .credentials(&provider(
                json!({ "api_key": "sk-test", "base_url": "https://example.com/v1" }),
            ))
            .unwrap();
        assert_eq!(credentials.api_key, "sk-test");
        assert_eq!(credentials.base_url, "https://example.com/v1");

        let legacy = HermesAdapter
            .credentials(&provider(
                json!({ "apiKey": "sk-legacy", "baseUrl": "https://legacy.example.com" }),
            ))
            .unwrap();
        assert_eq!(legacy.api_key, "sk-legacy");
        assert_eq!(legacy.base_url, "https://legacy.example.com");

        let both = HermesAdapter
            .credentials(&provider(
                json!({ "api_key": "sk-new", "apiKey": "sk-old" }),
            ))
            .unwrap();
        assert_eq!(both.api_key, "sk-new");
        assert!(HermesAdapter.credentials(&provider(json!({}))).is_err());
    }

    #[test]
    fn has_no_common_snippet_but_entry_level_takeover() {
        assert!(HermesAdapter.common_snippet().is_none());
        assert!(HermesAdapter.mcp().is_some());
        let takeover = HermesAdapter.takeover().expect("takeover");
        assert!(takeover.entry_takeover().is_some());
    }

    #[test]
    fn entry_takeover_reads_custom_providers_and_the_model_section() {
        let entries = HermesAdapter.takeover().unwrap().entry_takeover().unwrap();
        let live = json!({
            "model": { "default": "m", "provider": "b" },
            "custom_providers": [
                { "name": "a", "base_url": "https://a.example.com/v1", "api_key": "sk-a" },
                { "name": "b", "base_url": "https://b.example.com/v1", "api_key": "ccs-proxy-0123456789abcdef0123456789abcdef", "api_mode": "chat_completions" },
                { "name": "c", "api_mode": "anthropic_messages" }
            ]
        });
        let ids: Vec<_> = entries
            .entries(&live)
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        assert_eq!(ids, ["a", "b", "c"]);
        assert_eq!(entries.default_entry(&live).as_deref(), Some("b"));
        let all = entries.entries(&live);
        assert_eq!(
            entries.endpoint(&all[0].1),
            EntryEndpoint {
                base_url: Some("https://a.example.com/v1".to_string()),
                api_key: Some("sk-a".to_string()),
            }
        );
        assert!(entries.proxyable(&all[0].1) && entries.proxyable(&all[1].1));
        assert!(
            !entries.proxyable(&all[2].1),
            "anthropic_messages is not mounted"
        );
        // 整份 live 与单个条目都能识别占位符
        let takeover = HermesAdapter.takeover().unwrap();
        assert!(takeover.has_placeholder(&live));
        assert!(takeover.has_placeholder(&all[1].1));
        assert!(!takeover.has_placeholder(&all[0].1));
    }
}
