//! Codex 适配器（`~/.codex/auth.json` + `~/.codex/config.toml`）
//!
//! 会话历史迁移（[`history_migration`]）会把 Codex 自己的 state DB 中
//! `threads.model_provider` 改写为 [`config::CC_SWITCH_CODEX_MODEL_PROVIDER_ID`]，并把完成
//! 标记写进 settings.json 的 `localMigrations`。这两者都是已落盘的外部契约：
//! 迁移代码换位置不能改变 provider id 或标记键名（见本模块的一致性测试）。

mod common_config;
pub mod config;
pub mod history_migration;
pub mod state_db;
pub(crate) mod takeover;

use std::path::PathBuf;

use regex::Regex;
use serde_json::Value;

pub use self::common_config::update_toml_common_config_snippet;
use self::config::{
    codex_auth_has_login_material, codex_config_has_official_proxy_route, extract_codex_api_key,
    extract_codex_experimental_bearer_token, get_codex_config_dir, read_codex_live_settings,
    read_codex_model_catalog_simplified_from_live, restore_codex_settings_for_backfill,
    should_restore_codex_provider_token_for_backfill, strip_codex_mcp_servers_from_settings,
    strip_codex_unified_session_bucket_from_settings, validate_config_toml,
    write_codex_provider_live_with_catalog,
};
use super::adapter::{
    AppAdapter, CommonSnippet, Credentials, McpProjector, ProxyTakeover, SessionSource,
    TakeoverProviderNeed,
};
use crate::app_config::AppType;
use crate::config::get_home_dir;
use crate::database::Database;
use crate::error::AppError;
use crate::provider::Provider;
use crate::proxy::inbound_auth::is_proxy_token;

#[derive(Debug)]
pub struct CodexAdapter;

impl AppAdapter for CodexAdapter {
    fn config_dir(&self) -> Result<PathBuf, AppError> {
        Ok(get_codex_config_dir())
    }

    fn prompt_file(&self) -> Option<PathBuf> {
        Some(get_codex_config_dir().join("AGENTS.md"))
    }

    fn skills_dir(&self) -> Option<PathBuf> {
        // 默认路径必须走 get_home_dir()（可被 CC_SWITCH_TEST_HOME 覆盖）
        Some(
            crate::settings::override_dir(&AppType::Codex)
                .unwrap_or_else(|| get_home_dir().join(".codex"))
                .join("skills"),
        )
    }

    fn live_files(&self) -> Vec<PathBuf> {
        vec![
            crate::codex_config::get_codex_auth_path(),
            crate::codex_config::get_codex_config_path(),
        ]
    }

    fn read_live(&self) -> Result<Value, AppError> {
        let mut result = read_codex_live_settings()?;
        // `modelCatalog` is a cc-switch private field that lives only in
        // the DB SSOT plus the `cc-switch-model-catalog.json` projection
        // file — it is never inlined into `auth.json` or `config.toml`.
        // Reverse-parse the projection so the edit form for the active
        // Codex provider doesn't see an empty mapping table.
        if let Ok(Some(model_catalog)) = read_codex_model_catalog_simplified_from_live() {
            if let Some(obj) = result.as_object_mut() {
                obj.insert("modelCatalog".to_string(), model_catalog);
            }
        }
        Ok(result)
    }

    fn import_live(&self) -> Result<Value, AppError> {
        read_codex_live_settings()
    }

    /// 只有登录材料、没有供应商密钥的 live 配置视为官方订阅
    fn import_category(&self, settings: &Value) -> &'static str {
        let config_text = settings.get("config").and_then(Value::as_str);
        let has_provider_key = extract_codex_api_key(settings.get("auth"), config_text).is_some();
        let has_login_material = settings
            .get("auth")
            .is_some_and(codex_auth_has_login_material);

        if has_login_material && !has_provider_key {
            "official"
        } else {
            "custom"
        }
    }

    /// 写 `auth.json` + `config.toml`；`config.toml` 写失败时回滚 `auth.json`
    fn write_provider(&self, provider: &Provider) -> Result<(), AppError> {
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

        write_codex_provider_live_with_catalog(
            &provider.settings_config,
            provider.category.as_deref(),
            auth,
            config_str,
            profile,
        )
    }

    fn validate_provider(&self, provider: &Provider) -> Result<(), AppError> {
        let settings = provider.settings_config.as_object().ok_or_else(|| {
            AppError::localized(
                "provider.codex.settings.not_object",
                "Codex 配置必须是 JSON 对象",
                "Codex configuration must be a JSON object",
            )
        })?;

        let auth = settings.get("auth").ok_or_else(|| {
            AppError::localized(
                "provider.codex.auth.missing",
                format!("供应商 {} 缺少 auth 配置", provider.id),
                format!("Provider {} is missing auth configuration", provider.id),
            )
        })?;
        if !auth.is_object() {
            return Err(AppError::localized(
                "provider.codex.auth.not_object",
                format!("供应商 {} 的 auth 配置必须是 JSON 对象", provider.id),
                format!(
                    "Provider {} auth configuration must be a JSON object",
                    provider.id
                ),
            ));
        }

        if let Some(config_value) = settings.get("config") {
            if !(config_value.is_string() || config_value.is_null()) {
                return Err(AppError::localized(
                    "provider.codex.config.invalid_type",
                    "Codex config 字段必须是字符串",
                    "Codex config field must be a string",
                ));
            }
            if let Some(cfg_text) = config_value.as_str() {
                validate_config_toml(cfg_text)?;
            }
        }
        Ok(())
    }

    fn backfill_live_settings(&self, provider: &Provider, live_settings: Value) -> Value {
        let mut settings = live_settings;
        let restore_provider_token = should_restore_codex_provider_token_for_backfill(
            provider.category.as_deref(),
            &provider.settings_config,
        );
        if let Err(err) = restore_codex_settings_for_backfill(
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
        if let Err(err) = strip_codex_mcp_servers_from_settings(&mut settings) {
            log::warn!(
                "Failed to strip mcp_servers while backfilling '{}': {err}",
                provider.id
            );
        }

        // 统一会话开关注入的共享 `custom` 路由只属于 live 配置；切换回填时
        // 必须剥掉，否则官方供应商的存储配置被污染，关闭开关后无法还原。
        if provider.category.as_deref() == Some("official") {
            if let Err(err) = strip_codex_unified_session_bucket_from_settings(&mut settings) {
                log::warn!(
                    "Failed to strip unified session bucket while backfilling '{}': {err}",
                    provider.id
                );
            }
        }

        // `modelCatalog` is a cc-switch–private field whose SSOT is the DB. Live's
        // `config.toml` only carries a lossy projection (`model_catalog_json` →
        // generated catalog file) that proxy takeover/restore cycles and Codex.app
        // config rewrites can drop, so `read_live_settings` may reconstruct it as
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

    /// key 在 `auth.OPENAI_API_KEY`，base URL 在 TOML `config` 字符串里
    fn usage_credentials(&self, settings: &Value) -> (String, String) {
        let auth = settings.get("auth");
        let config_text = settings.get("config").and_then(|v| v.as_str());
        let api_key = config::extract_codex_api_key(auth, config_text).unwrap_or_default();
        let base_url = config_text
            .and_then(config::extract_codex_base_url)
            .unwrap_or_default();
        (base_url, api_key)
    }

    fn credentials(&self, provider: &Provider) -> Result<Credentials, AppError> {
        let _auth = provider
            .settings_config
            .get("auth")
            .and_then(|v| v.as_object())
            .ok_or_else(|| {
                AppError::localized(
                    "provider.codex.auth.missing",
                    "配置格式错误: 缺少 auth",
                    "Invalid configuration: missing auth section",
                )
            })?;

        let config_toml = provider
            .settings_config
            .get("config")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let api_key =
            extract_codex_api_key(provider.settings_config.get("auth"), Some(config_toml))
                .ok_or_else(|| {
                    AppError::localized(
                        "provider.codex.api_key.missing",
                        "缺少 API Key",
                        "API key is missing",
                    )
                })?;

        let base_url = if config_toml.contains("base_url") {
            let re = Regex::new(r#"base_url\s*=\s*["']([^"']+)["']"#).map_err(|e| {
                AppError::localized(
                    "provider.regex_init_failed",
                    format!("正则初始化失败: {e}"),
                    format!("Failed to initialize regex: {e}"),
                )
            })?;
            re.captures(config_toml)
                .and_then(|caps| caps.get(1))
                .map(|m| m.as_str().to_string())
                .ok_or_else(|| {
                    AppError::localized(
                        "provider.codex.base_url.invalid",
                        "config.toml 中 base_url 格式错误",
                        "base_url in config.toml has invalid format",
                    )
                })?
        } else {
            return Err(AppError::localized(
                "provider.codex.base_url.missing",
                "config.toml 中缺少 base_url 配置",
                "base_url is missing from config.toml",
            ));
        };

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

/// Codex 的通用配置片段是 `config.toml` 的 TOML 子集，只作用于 `settings.config`。
impl CommonSnippet for CodexAdapter {
    fn extract(&self, settings: &Value) -> Result<String, AppError> {
        common_config::extract_common_config(settings)
    }

    fn contains(&self, settings: &Value, snippet: &str) -> bool {
        common_config::contains(settings, snippet)
    }

    fn apply(&self, settings: &Value, snippet: &str) -> Result<Value, AppError> {
        common_config::apply(settings, snippet)
    }

    fn remove(&self, settings: &Value, snippet: &str) -> Result<Value, AppError> {
        common_config::remove(settings, snippet)
    }

    fn sync_from_live(&self) -> bool {
        true
    }
}

impl McpProjector for CodexAdapter {
    fn live_file(&self) -> PathBuf {
        crate::codex_config::get_codex_config_path()
    }

    fn read_servers(&self) -> Result<Vec<(String, Value)>, AppError> {
        crate::mcp::read_servers_from_codex()
    }

    fn upsert(&self, id: &str, spec: &Value) -> Result<(), AppError> {
        // Codex uses TOML format, must use the correct function
        crate::mcp::sync_single_server_to_codex(&Default::default(), id, spec)
    }

    fn remove(&self, id: &str) -> Result<(), AppError> {
        crate::mcp::remove_server_from_codex(id)
    }
}

impl ProxyTakeover for CodexAdapter {
    /// 沿用无前缀的 `/v1`：Codex CLI 还会探测 `/v1/models`，该路径没有 `/codex` 前缀版本
    fn base_url(&self, proxy_origin: &str, _proxy_route: &str) -> String {
        format!("{proxy_origin}/v1")
    }

    /// `auth.json` 的 `OPENAI_API_KEY` 或 `config.toml` 的 bearer token 是代理占位符，
    /// 或 `config.toml` 带有官方供应商的代理路由
    fn has_placeholder(&self, settings: &Value) -> bool {
        let auth_token_is_placeholder = settings
            .get("auth")
            .and_then(|v| v.as_object())
            .and_then(|auth| auth.get("OPENAI_API_KEY"))
            .and_then(|v| v.as_str())
            .is_some_and(is_proxy_token);
        if auth_token_is_placeholder {
            return true;
        }

        let Some(config_text) = settings.get("config").and_then(|v| v.as_str()) else {
            return false;
        };
        extract_codex_experimental_bearer_token(config_text)
            .is_some_and(|token| is_proxy_token(&token))
            || codex_config_has_official_proxy_route(config_text)
    }

    /// 内置 Codex 官方供应商在接管期间可选：Codex 自己持有 ChatGPT 登录，代理只转发
    fn official_provider_supported(&self, provider: &Provider) -> bool {
        crate::proxy::providers::is_codex_official_provider(provider)
    }

    fn live_files(&self) -> Vec<PathBuf> {
        vec![
            crate::codex_config::get_codex_auth_path(),
            crate::codex_config::get_codex_config_path(),
        ]
    }

    fn read_live(&self) -> Result<Value, String> {
        takeover::read_live()
    }

    fn write_live(&self, config: &Value) -> Result<(), String> {
        takeover::write_live(config)
    }

    fn write_takeover_live(
        &self,
        config: &Value,
        provider: Option<&Provider>,
    ) -> Result<(), String> {
        takeover::write_takeover_live(config, provider)
    }

    fn strip_placeholders(&self, live: &mut Value) -> Result<bool, String> {
        takeover::strip_placeholders(live)
    }

    fn base_url_matches(&self, live: &Value, expected: &str) -> bool {
        takeover::base_url_matches(live, expected)
    }

    fn provider_need(&self) -> TakeoverProviderNeed {
        TakeoverProviderNeed::Required
    }

    fn sync_live_credentials(&self, db: &Database, live: &Value) -> Result<(), String> {
        takeover::sync_live_credentials(db, live)
    }

    /// 只在 live 已被接管时刷新：模型映射投影在生成的 model_catalog_json 中，
    /// 不刷新会让旧的目录指针与能力继续生效
    fn refresh_after_save(&self, live_taken_over: bool) -> bool {
        live_taken_over
    }

    fn preserves_live_toml_mcp(&self) -> bool {
        true
    }
}

impl SessionSource for CodexAdapter {
    fn session_roots(&self) -> Vec<PathBuf> {
        let config_dir = get_codex_config_dir();
        vec![
            config_dir.join("sessions"),
            config_dir.join("archived_sessions"),
        ]
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::history_migration::{
        LEGACY_CC_SWITCH_CODEX_MODEL_PROVIDER_ID, MIGRATION_NAME,
        OFFICIAL_OPENAI_CODEX_MODEL_PROVIDER_ID, OFFICIAL_UNIFY_MIGRATION_NAME,
        OFFICIAL_UNIFY_RESTORE_BACKUP_NAME,
    };
    use super::*;
    use crate::settings::{
        AppSettings, CodexOfficialHistoryUnifyMigration, CodexProviderTemplateMigration,
        CodexThirdPartyHistoryProviderBucketMigration, LocalMigrations,
    };

    /// 历史迁移已经把这些 id 写进用户的 Codex state DB（`threads.model_provider`）
    /// 和会话 jsonl；改动任何一个都会让已迁移的会话落到另一个桶里。
    #[test]
    fn history_migration_provider_ids_are_stable() {
        assert_eq!(config::CC_SWITCH_CODEX_MODEL_PROVIDER_ID, "custom");
        assert_eq!(LEGACY_CC_SWITCH_CODEX_MODEL_PROVIDER_ID, "ccswitch");
        assert_eq!(OFFICIAL_OPENAI_CODEX_MODEL_PROVIDER_ID, "openai");
        assert_eq!(
            config::CC_SWITCH_CODEX_OFFICIAL_PROXY_PROVIDER_ID,
            "cc-switch-official"
        );
    }

    /// 备份目录名是迁移账本：还原路径按这些名字去找备份。
    #[test]
    fn history_migration_backup_names_are_stable() {
        assert_eq!(MIGRATION_NAME, "codex-history-provider-migration-v1");
        assert_eq!(
            OFFICIAL_UNIFY_MIGRATION_NAME,
            "codex-official-history-unify-v1"
        );
        assert_eq!(
            OFFICIAL_UNIFY_RESTORE_BACKUP_NAME,
            "codex-official-history-unify-restore-v1"
        );
        assert_eq!(state_db::CODEX_STATE_DB_FILENAME, "state_5.sqlite");
    }

    /// settings.json 里的迁移完成标记键名：改名会让已完成的迁移在下次启动时重跑。
    #[test]
    fn history_migration_marker_keys_are_stable() {
        let settings = AppSettings {
            local_migrations: Some(LocalMigrations {
                codex_third_party_history_provider_bucket_v1: Some(
                    CodexThirdPartyHistoryProviderBucketMigration {
                        completed_at: "t".to_string(),
                        target_provider_id: config::CC_SWITCH_CODEX_MODEL_PROVIDER_ID.to_string(),
                        source_provider_ids: vec!["aihubmix".to_string()],
                        migrated_jsonl_files: 1,
                        migrated_state_rows: 2,
                        scanned_history_files: true,
                    },
                ),
                codex_provider_template_v1: Some(CodexProviderTemplateMigration {
                    completed_at: "t".to_string(),
                    migrated_provider_ids: vec!["p".to_string()],
                }),
                codex_official_history_unify_v1: Some(CodexOfficialHistoryUnifyMigration {
                    completed_at: "t".to_string(),
                    target_provider_id: config::CC_SWITCH_CODEX_MODEL_PROVIDER_ID.to_string(),
                    migrated_jsonl_files: 3,
                    migrated_state_rows: 4,
                    codex_config_dir: Some("/home/u/.codex".to_string()),
                }),
                deleted_builtin_model_pricing: Vec::new(),
            }),
            ..AppSettings::default()
        };
        let value = serde_json::to_value(&settings).expect("serialize settings");
        assert_eq!(
            value["localMigrations"],
            json!({
                "codexThirdPartyHistoryProviderBucketV1": {
                    "completedAt": "t",
                    "targetProviderId": "custom",
                    "sourceProviderIds": ["aihubmix"],
                    "migratedJsonlFiles": 1,
                    "migratedStateRows": 2,
                    "scannedHistoryFiles": true
                },
                "codexProviderTemplateV1": {
                    "completedAt": "t",
                    "migratedProviderIds": ["p"]
                },
                "codexOfficialHistoryUnifyV1": {
                    "completedAt": "t",
                    "targetProviderId": "custom",
                    "migratedJsonlFiles": 3,
                    "migratedStateRows": 4,
                    "codexConfigDir": "/home/u/.codex"
                }
            })
        );
    }

    fn provider(category: Option<&str>, settings: Value) -> Provider {
        let mut provider = Provider::with_id("p".to_string(), "P".to_string(), settings, None);
        provider.category = category.map(str::to_string);
        provider
    }

    #[test]
    fn import_category_detects_official_login() {
        let adapter = CodexAdapter;
        let official = json!({
            "auth": { "tokens": { "access_token": "a", "refresh_token": "r" } },
            "config": "model = \"gpt-5\"\n"
        });
        assert_eq!(adapter.import_category(&official), "official");

        let custom = json!({
            "auth": { "OPENAI_API_KEY": "sk-relay" },
            "config": "model_provider = \"custom\"\n"
        });
        assert_eq!(adapter.import_category(&custom), "custom");
        assert_eq!(adapter.import_category(&json!({})), "custom");
    }

    #[test]
    fn credentials_read_auth_and_config_base_url() {
        let adapter = CodexAdapter;
        let creds = adapter
            .credentials(&provider(
                None,
                json!({
                    "auth": { "OPENAI_API_KEY": "sk-relay" },
                    "config": "model_provider = \"custom\"\n\n[model_providers.custom]\nbase_url = \"https://relay.example.com/v1\"\n"
                }),
            ))
            .expect("credentials");
        assert_eq!(
            creds,
            Credentials {
                api_key: "sk-relay".to_string(),
                base_url: "https://relay.example.com/v1".to_string(),
            }
        );

        assert!(adapter
            .credentials(&provider(
                None,
                json!({ "auth": { "OPENAI_API_KEY": "sk" }, "config": "" })
            ))
            .is_err());
    }

    #[test]
    fn validate_rejects_non_string_config() {
        let adapter = CodexAdapter;
        assert!(adapter
            .validate_provider(&provider(None, json!({ "auth": {}, "config": "" })))
            .is_ok());
        assert!(adapter
            .validate_provider(&provider(None, json!({ "config": "" })))
            .is_err());
        assert!(adapter
            .validate_provider(&provider(None, json!({ "auth": {}, "config": 1 })))
            .is_err());
        assert!(adapter
            .validate_provider(&provider(None, json!({ "auth": {}, "config": "[" })))
            .is_err());
    }

    #[test]
    fn common_snippet_round_trips_config_toml() {
        let common = CodexAdapter.common_snippet().expect("codex common snippet");
        assert!(common.sync_from_live());

        let settings = json!({
            "auth": { "OPENAI_API_KEY": "sk" },
            "config": "# keep me\nmodel = \"gpt-5\"\n"
        });
        let snippet = "[tui]\nnotifications = true\n";
        assert!(!common.contains(&settings, snippet));

        let applied = common.apply(&settings, snippet).unwrap();
        assert!(common.contains(&applied, snippet));
        assert_eq!(applied["auth"], settings["auth"]);

        let removed = common.remove(&applied, snippet).unwrap();
        assert!(!common.contains(&removed, snippet));
        assert_eq!(removed["config"], settings["config"]);

        let extracted = common
            .extract(&json!({
                "config": "model = \"gpt-5\"\nmodel_provider = \"custom\"\n\n[tui]\nnotifications = true\n\n[model_providers.custom]\nbase_url = \"https://relay.example.com/v1\"\n"
            }))
            .unwrap();
        assert_eq!(extracted, "[tui]\nnotifications = true");
    }

    #[test]
    fn takeover_detects_placeholders_and_official_route() {
        let token = crate::proxy::inbound_auth::LEGACY_PROXY_TOKEN;
        let takeover = CodexAdapter.takeover().expect("codex takeover");

        assert!(takeover.has_placeholder(&json!({ "auth": { "OPENAI_API_KEY": token } })));
        assert!(takeover.has_placeholder(&json!({
            "auth": {},
            "config": format!(
                "model_provider = \"custom\"\n\n[model_providers.custom]\nexperimental_bearer_token = \"{token}\"\n"
            )
        })));
        assert!(takeover.has_placeholder(&json!({
            "auth": {},
            "config": "model_provider = \"cc-switch-official\"\n\n[model_providers.cc-switch-official]\nbase_url = \"http://127.0.0.1:15721/v1\"\n"
        })));
        assert!(!takeover.has_placeholder(&json!({
            "auth": { "OPENAI_API_KEY": "sk-real" },
            "config": "model_provider = \"custom\"\n"
        })));
        assert!(!takeover.has_placeholder(&json!({})));

        let mut official = provider(Some("official"), json!({ "auth": {}, "config": "" }));
        official.id = crate::database::CODEX_OFFICIAL_PROVIDER_ID.to_string();
        assert!(takeover.official_provider_supported(&official));
        assert!(!takeover.official_provider_supported(&provider(
            Some("custom"),
            json!({ "auth": {}, "config": "" })
        )));
    }

    #[test]
    fn backfill_keeps_stored_model_catalog_and_strips_mcp_servers() {
        let stored = provider(
            Some("custom"),
            json!({
                "auth": { "OPENAI_API_KEY": "sk" },
                "config": "model = \"gpt-5\"\n",
                "modelCatalog": { "models": ["a"] }
            }),
        );
        let live = json!({
            "auth": { "OPENAI_API_KEY": "sk" },
            "config": "model = \"gpt-5\"\n\n[mcp_servers.fs]\ncommand = \"npx\"\n"
        });
        let backfilled = CodexAdapter.backfill_live_settings(&stored, live);
        assert_eq!(backfilled["modelCatalog"], json!({ "models": ["a"] }));
        assert!(!backfilled["config"]
            .as_str()
            .unwrap_or_default()
            .contains("mcp_servers"));
    }
}
