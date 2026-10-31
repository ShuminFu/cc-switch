//! Codex: `~/.codex/auth.json` + `~/.codex/config.toml`.

use std::path::PathBuf;

use serde_json::Value;

use super::{
    credential_fingerprint, AdapterCapabilities, AppAdapter, CommonConfigShape, ImportCandidate,
    McpProjector, WriteMode,
};
use crate::app_config::{AppType, MultiAppConfig};
use crate::codex_config::{get_codex_auth_path, get_codex_config_path};
use crate::error::AppError;
use crate::provider::Provider;

pub(super) struct CodexAdapter;

impl AppAdapter for CodexAdapter {
    fn app(&self) -> AppType {
        AppType::Codex
    }

    fn capabilities(&self) -> AdapterCapabilities {
        AdapterCapabilities {
            mode: WriteMode::Exclusive,
            generic_live_read: true,
            mcp: true,
            proxy_takeover: true,
            common_config: CommonConfigShape::TomlConfigField,
        }
    }

    fn live_paths(&self) -> Vec<PathBuf> {
        vec![get_codex_auth_path(), get_codex_config_path()]
    }

    fn read_live(&self) -> Result<Value, AppError> {
        // repair a switch interrupted between the auth.json and config.toml
        // writes before the half-applied state is backfilled (CFG-27)
        if let Err(err) = crate::codex_config::recover_codex_live_journal() {
            log::warn!("Codex live journal recovery failed: {err}");
        }
        let mut result = crate::codex_config::read_codex_live_settings()?;
        // `modelCatalog` is a cc-switch private field that lives only in
        // the DB SSOT plus the `cc-switch-model-catalog.json` projection
        // file — it is never inlined into `auth.json` or `config.toml`.
        // Reverse-parse the projection so the edit form for the active
        // Codex provider doesn't see an empty mapping table.
        if let Ok(Some(model_catalog)) =
            crate::codex_config::read_codex_model_catalog_simplified_from_live()
        {
            if let Some(obj) = result.as_object_mut() {
                obj.insert("modelCatalog".to_string(), model_catalog);
            }
        }
        Ok(result)
    }

    fn write_live(&self, provider: &Provider) -> Result<(), AppError> {
        let obj = provider
            .settings_config
            .as_object()
            .ok_or_else(|| AppError::Config("Codex 供应商配置必须是 JSON 对象".to_string()))?;
        let auth = obj
            .get("auth")
            .ok_or_else(|| AppError::Config("Codex 供应商配置缺少 'auth' 字段".to_string()))?;
        let config_str = obj.get("config").and_then(|v| v.as_str());

        // Native (direct) Responses and Anthropic providers must suppress Codex's
        // freeform apply_patch custom tool via the generated catalog; chat/proxy
        // providers keep the default tool set. Uses the same Anthropic detection as
        // the proxy router (apiFormat meta/settings + TOML wire_api).
        let profile = crate::proxy::providers::resolve_codex_catalog_tool_profile(provider);

        crate::config::backup_live_file("codex", &get_codex_auth_path());
        crate::config::backup_live_file("codex", &get_codex_config_path());
        crate::codex_config::write_codex_provider_live_with_catalog(
            &provider.settings_config,
            provider.category.as_deref(),
            auth,
            config_str,
            profile,
        )
    }

    fn backfill(&self, provider: &Provider, live: Value) -> Value {
        let mut settings = live;
        let restore_provider_token =
            crate::codex_config::should_restore_codex_provider_token_for_backfill(
                provider.category.as_deref(),
                &provider.settings_config,
            );
        if let Err(err) = crate::codex_config::restore_codex_settings_for_backfill(
            &mut settings,
            &provider.settings_config,
            restore_provider_token,
        ) {
            log::warn!(
                "Failed to restore Codex settings while backfilling '{}': {err}",
                provider.id
            );
        }

        // MCP 服务器归 DB mcp_servers 表所有，live 里的 [mcp_servers] 是同步投影；
        // 回填时剥掉，否则已删除的服务器会随供应商快照复活（逐条 reconcile 清不掉孤儿）。
        if let Err(err) = crate::codex_config::strip_codex_mcp_servers_from_settings(&mut settings)
        {
            log::warn!(
                "Failed to strip mcp_servers while backfilling '{}': {err}",
                provider.id
            );
        }

        // 统一会话开关注入的共享 `custom` 路由只属于 live 配置；切换回填时
        // 必须剥掉，否则官方供应商的存储配置被污染，关闭开关后无法还原。
        if provider.category.as_deref() == Some("official") {
            if let Err(err) =
                crate::codex_config::strip_codex_unified_session_bucket_from_settings(&mut settings)
            {
                log::warn!(
                    "Failed to strip unified session bucket while backfilling '{}': {err}",
                    provider.id
                );
            }
        }

        // `modelCatalog` is a cc-switch–private field whose SSOT is the DB. Live's
        // `config.toml` only carries a lossy projection (`model_catalog_json` →
        // generated catalog file) that proxy takeover/restore cycles and Codex.app
        // config rewrites can drop, so `read_live` may reconstruct it as
        // absent. Never let a switch-away backfill from Live erase the stored
        // mapping: prefer the DB provider's `modelCatalog`, falling back to whatever
        // Live reconstructed only when the DB has none.
        if let Some(stored_catalog) = provider.settings_config.get("modelCatalog") {
            if let Some(obj) = settings.as_object_mut() {
                obj.insert("modelCatalog".to_string(), stored_catalog.clone());
            }
        }

        settings
    }

    fn import_live(&self) -> Result<Value, AppError> {
        crate::codex_config::read_codex_live_settings()
    }

    fn import_candidates(&self) -> Result<Vec<ImportCandidate>, AppError> {
        let live = crate::codex_config::read_codex_live_settings()?;
        codex_import_candidates(&live)
    }

    fn mcp(&self) -> Option<&dyn McpProjector> {
        Some(&CodexMcp)
    }
}

/// One candidate per `[model_providers.<id>]` table (CFG-23). Each carries
/// the root settings (model, reasoning effort, …) with `model_provider`
/// pointing at its own table and only that table; the bearer token comes from
/// the table, or from `auth.json` for the active provider. A config without
/// provider tables yields the live snapshot as one candidate.
pub(super) fn codex_import_candidates(live: &Value) -> Result<Vec<ImportCandidate>, AppError> {
    let config_text = live.get("config").and_then(Value::as_str).unwrap_or("");
    let auth = live
        .get("auth")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let doc: toml_edit::DocumentMut = config_text
        .parse()
        .map_err(|e| AppError::Message(format!("Invalid Codex config.toml: {e}")))?;
    let active = doc
        .get("model_provider")
        .and_then(|item| item.as_str())
        .map(str::to_string);
    let live_key = crate::codex_config::extract_codex_auth_api_key(&auth).unwrap_or_default();

    let ids: Vec<String> = doc
        .get("model_providers")
        .and_then(|item| item.as_table_like())
        .map(|providers| providers.iter().map(|(id, _)| id.to_string()).collect())
        .unwrap_or_default();
    if ids.is_empty() {
        return Ok(vec![ImportCandidate {
            id_hint: "default".to_string(),
            name: "default".to_string(),
            settings_config: live.clone(),
            fingerprint: credential_fingerprint("codex:default", &live_key),
        }]);
    }

    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        let mut single = doc.clone();
        if let Some(providers) = single
            .get_mut("model_providers")
            .and_then(|item| item.as_table_like_mut())
        {
            let others: Vec<String> = providers
                .iter()
                .map(|(key, _)| key.to_string())
                .filter(|key| key != &id)
                .collect();
            for other in others {
                providers.remove(&other);
            }
        }
        single["model_provider"] = toml_edit::value(id.as_str());
        let table = single["model_providers"][id.as_str()].clone();
        let field = |key: &str| {
            table
                .get(key)
                .and_then(|item| item.as_str())
                .map(str::to_string)
        };
        let base_url = field("base_url").unwrap_or_default();
        let token = field("experimental_bearer_token").or_else(|| {
            (active.as_deref() == Some(id.as_str()) && !live_key.is_empty())
                .then(|| live_key.clone())
        });
        let auth = match &token {
            Some(token) => serde_json::json!({ "OPENAI_API_KEY": token }),
            None => serde_json::json!({}),
        };
        out.push(ImportCandidate {
            name: field("name").unwrap_or_else(|| id.clone()),
            fingerprint: credential_fingerprint(&base_url, token.as_deref().unwrap_or("")),
            id_hint: id,
            settings_config: serde_json::json!({ "auth": auth, "config": single.to_string() }),
        });
    }
    Ok(out)
}

struct CodexMcp;

impl McpProjector for CodexMcp {
    fn missing_config_dir(&self) -> Option<std::path::PathBuf> {
        let dir = crate::codex_config::get_codex_config_dir();
        (!dir.exists()).then_some(dir)
    }

    fn upsert_server(&self, id: &str, spec: &Value) -> Result<(), AppError> {
        crate::mcp::sync_single_server_to_codex(&Default::default(), id, spec)
    }

    fn remove_server(&self, id: &str) -> Result<(), AppError> {
        crate::mcp::remove_server_from_codex(id)
    }

    fn import_servers(&self, config: &mut MultiAppConfig) -> Result<usize, AppError> {
        crate::mcp::import_from_codex(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const THREE: &str = r#"model_provider = "b"
model = "gpt-5"

[model_providers.a]
name = "Alpha"
base_url = "https://a.example.com/v1"
experimental_bearer_token = "sk-a"

[model_providers.b]
name = "Beta"
base_url = "https://b.example.com/v1"

[model_providers.c]
base_url = "https://c.example.com/v1"
experimental_bearer_token = "sk-c"
"#;

    #[test]
    fn three_provider_tables_yield_three_candidates() {
        let live = json!({ "auth": { "OPENAI_API_KEY": "sk-b" }, "config": THREE });
        let candidates = codex_import_candidates(&live).unwrap();
        let ids: Vec<_> = candidates.iter().map(|c| c.id_hint.as_str()).collect();
        assert_eq!(ids, ["a", "b", "c"]);
        assert_eq!(candidates[0].name, "Alpha");
        assert_eq!(candidates[2].name, "c");
        // the active provider takes its key from auth.json
        assert_eq!(
            candidates[1].settings_config["auth"]["OPENAI_API_KEY"],
            "sk-b"
        );
        assert_eq!(
            candidates[0].settings_config["auth"]["OPENAI_API_KEY"],
            "sk-a"
        );

        let config: toml::Table =
            toml::from_str(candidates[0].settings_config["config"].as_str().unwrap()).unwrap();
        assert_eq!(config["model_provider"].as_str(), Some("a"));
        assert_eq!(config["model"].as_str(), Some("gpt-5"));
        assert_eq!(config["model_providers"].as_table().unwrap().len(), 1);

        // fingerprints identify endpoint + key without exposing the key
        assert!(!candidates[0].fingerprint.contains("sk-a"));
        assert_ne!(candidates[0].fingerprint, candidates[2].fingerprint);
        // repeatable: the same live config gives the same candidates
        assert_eq!(codex_import_candidates(&live).unwrap(), candidates);
    }

    #[test]
    fn config_without_provider_tables_is_one_candidate() {
        let live = json!({ "auth": {}, "config": "model = \"gpt-5\"\n" });
        let candidates = codex_import_candidates(&live).unwrap();
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].settings_config, live);
    }
}
