//! Codex: `~/.codex/auth.json` + `~/.codex/config.toml`.

use std::path::PathBuf;

use serde_json::Value;

use super::{AdapterCapabilities, AppAdapter, CommonConfigShape, McpProjector, WriteMode};
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

    fn mcp(&self) -> Option<&dyn McpProjector> {
        Some(&CodexMcp)
    }
}

struct CodexMcp;

impl McpProjector for CodexMcp {
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
