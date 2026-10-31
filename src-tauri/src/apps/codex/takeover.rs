//! 代理接管 Codex `auth.json` + `config.toml` 的读写、清理与凭据回写

use serde_json::{json, Value};

use crate::app_config::AppType;
use crate::apps::adapter::{
    current_provider_for_sync, is_local_proxy_url, proxy_urls_match, save_synced_provider,
};
use crate::config::write_json_file;
use crate::database::Database;
use crate::provider::Provider;
use crate::proxy::inbound_auth::is_proxy_token;

pub(super) fn read_live() -> Result<Value, String> {
    crate::codex_config::read_codex_live_settings()
        .map_err(|e| format!("读取 Codex Live 配置失败: {e}"))
}

/// `auth.json` 的 `OPENAI_API_KEY` 是否为代理 token
pub(crate) fn auth_has_proxy_placeholder(auth: &Value) -> bool {
    auth.get("OPENAI_API_KEY")
        .and_then(|v| v.as_str())
        .is_some_and(is_proxy_token)
}

/// 当前 `model_provider` 的 `base_url`（或顶层 `base_url`）是否满足 `predicate`
pub(crate) fn config_has_base_url_matching(
    config_text: &str,
    predicate: impl Fn(&str) -> bool,
) -> bool {
    let Ok(doc) = toml::from_str::<toml::Value>(config_text) else {
        return false;
    };

    let active_provider = doc
        .get("model_provider")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|id| !id.is_empty());

    if let Some(provider_id) = active_provider {
        if doc
            .get("model_providers")
            .and_then(|value| value.get(provider_id))
            .and_then(|value| value.get("base_url"))
            .and_then(|value| value.as_str())
            .is_some_and(&predicate)
        {
            return true;
        }
    }

    doc.get("base_url")
        .and_then(|value| value.as_str())
        .is_some_and(predicate)
}

pub(super) fn base_url_matches(live: &Value, expected: &str) -> bool {
    live.get("config")
        .and_then(Value::as_str)
        .is_some_and(|text| {
            config_has_base_url_matching(text, |url| proxy_urls_match(url, expected))
        })
}

pub(super) fn strip_placeholders(live: &mut Value) -> Result<bool, String> {
    if let Some(auth) = live.get_mut("auth").and_then(|v| v.as_object_mut()) {
        if auth
            .get("OPENAI_API_KEY")
            .and_then(|v| v.as_str())
            .is_some_and(is_proxy_token)
        {
            auth.remove("OPENAI_API_KEY");
        }
    }

    if let Some(cfg_str) = live.get("config").and_then(|v| v.as_str()) {
        let updated =
            crate::codex_config::remove_codex_toml_base_url_if(cfg_str, is_local_proxy_url);
        let updated = crate::codex_config::remove_codex_experimental_bearer_token_if(
            &updated,
            is_proxy_token,
        )
        .map_err(|e| format!("清理 Codex 接管占位符失败: {e}"))?;
        let updated = crate::codex_config::remove_codex_official_proxy_route(&updated)
            .map_err(|e| format!("清理 Codex 官方接管路由失败: {e}"))?;
        live["config"] = json!(updated);
    }

    Ok(true)
}

pub(super) fn sync_live_credentials(db: &Database, live: &Value) -> Result<(), String> {
    let Some(mut provider) = current_provider_for_sync(db, &AppType::Codex, "Codex")? else {
        return Ok(());
    };
    // The built-in official row is a routing capability, not
    // a credential store. Its auth must remain empty even
    // when the live Codex login uses OPENAI_API_KEY mode.
    if crate::proxy::providers::is_codex_official_provider(&provider) {
        return Ok(());
    }
    let Some(token) = live
        .get("auth")
        .and_then(|v| v.get("OPENAI_API_KEY"))
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty() && !is_proxy_token(s))
    else {
        return Ok(());
    };

    if let Some(auth_obj) = provider
        .settings_config
        .get_mut("auth")
        .and_then(|v| v.as_object_mut())
    {
        auth_obj.insert("OPENAI_API_KEY".to_string(), json!(token));
    } else {
        if provider.settings_config.is_null() {
            provider.settings_config = json!({});
        }

        if let Some(root) = provider.settings_config.as_object_mut() {
            root.insert("auth".to_string(), json!({ "OPENAI_API_KEY": token }));
        } else {
            log::warn!(
                "Codex provider settings_config 格式异常（非对象），跳过写入 Token (provider: {})",
                provider.id
            );
        }
    }

    save_synced_provider(db, &AppType::Codex, "Codex", &provider);
    Ok(())
}

/// 按供应商写入 live（`config` 的 token 已绑定）
pub(crate) fn write_live_for_provider(
    config: &Value,
    provider: Option<&Provider>,
) -> Result<(), String> {
    let Some(provider) = provider else {
        if crate::settings::preserve_codex_official_auth_on_switch() {
            if let (Some(auth), Some(config_str)) = (
                config.get("auth"),
                config.get("config").and_then(|v| v.as_str()),
            ) {
                if auth
                    .get("OPENAI_API_KEY")
                    .and_then(|v| v.as_str())
                    .is_some_and(is_proxy_token)
                {
                    let live_config =
                        crate::codex_config::prepare_codex_provider_live_config(auth, config_str)
                            .map_err(|e| format!("写入 Codex 配置失败: {e}"))?;
                    crate::codex_config::write_codex_live_config_atomic(Some(&live_config))
                        .map_err(|e| format!("写入 Codex 配置失败: {e}"))?;
                    return Ok(());
                }
            }
        }

        return write_live(config);
    };

    let auth = config
        .get("auth")
        .ok_or_else(|| "Codex 配置缺少 auth 字段".to_string())?;
    let config_str = config.get("config").and_then(|v| v.as_str());
    let profile = crate::proxy::providers::resolve_codex_catalog_tool_profile(provider);

    crate::codex_config::write_codex_provider_live_with_catalog(
        config,
        provider.category.as_deref(),
        auth,
        config_str,
        profile,
    )
    .map_err(|e| format!("写入 Codex 配置失败: {e}"))
}

/// 写入接管后的 live（`config` 的 token 已绑定）
pub(crate) fn write_takeover_live(
    config: &Value,
    provider: Option<&Provider>,
) -> Result<(), String> {
    let official_passthrough =
        provider.is_some_and(crate::proxy::providers::is_codex_official_provider);
    let placeholder_auth = config.get("auth").is_some_and(auth_has_proxy_placeholder);

    // Takeover must never overwrite Codex's long-lived ChatGPT login. For
    // third-party providers the placeholder is moved into config.toml; for
    // codex-official no placeholder is needed because requires_openai_auth
    // makes Codex supply its native authorization.
    if official_passthrough || placeholder_auth {
        let config_str = config.get("config").and_then(|v| v.as_str()).unwrap_or("");
        let profile = provider
            .map(crate::proxy::providers::resolve_codex_catalog_tool_profile)
            .unwrap_or(crate::codex_config::CodexCatalogToolProfile::ProxyChat);
        let prepared_config =
            crate::codex_config::prepare_codex_live_config_text_with_optional_catalog(
                config, config_str, profile,
            )
            .map_err(|e| format!("写入 Codex 配置失败: {e}"))?;
        let live_config = if official_passthrough {
            prepared_config
        } else {
            crate::codex_config::prepare_codex_provider_live_config(
                config.get("auth").unwrap_or(&Value::Null),
                &prepared_config,
            )
            .map_err(|e| format!("写入 Codex 配置失败: {e}"))?
        };
        crate::codex_config::write_codex_live_config_atomic(Some(&live_config))
            .map_err(|e| format!("写入 Codex 配置失败: {e}"))?;
        return Ok(());
    }

    write_live_for_provider(config, provider)
}

/// 原样写回 live（备份恢复等；`config` 的 token 已绑定）
pub(crate) fn write_live(config: &Value) -> Result<(), String> {
    use crate::codex_config::{get_codex_auth_path, get_codex_config_path};

    let auth = config.get("auth");
    let config_str = config.get("config").and_then(|v| v.as_str());

    // Decide the config.toml text ONCE, before splitting on auth. A stored
    // Codex backup comes in two shapes needing opposite handling:
    //  - snapshot backup (`read_codex_live_settings`): no inline `modelCatalog`;
    //    the config text already carries the live `model_catalog_json` pointer
    //    → keep raw, or projection would strip it.
    //  - provider-rebuilt backup (`update_live_backup_from_provider`): inline
    //    `modelCatalog` (DB SSOT) with a pointer-less config text → project,
    //    or the mapping is lost on restore.
    // The projection decision is orthogonal to auth: a provider-rebuilt backup
    // can pair an inline `modelCatalog` with empty/absent `auth.json` (the key
    // living in the config's `experimental_bearer_token`). Computing it up here
    // keeps every config-writing branch — write-auth, delete-auth, no-auth —
    // consistent instead of letting the empty-auth path skip projection.
    // Verbatim restore has no Provider in hand (we only have the stored
    // backup config), so the catalog tool profile can't be recovered here.
    // Default to ProxyChat: a restored native-direct backup keeps its inline
    // modelCatalog but would not get apply_patch re-stripped until the next
    // provider switch rewrites it via the Codex adapter. Acceptable known
    // limitation (restore-of-deleted-provider-backup only).
    let prepared_cfg = config_str
        .map(|cfg| {
            crate::codex_config::prepare_codex_live_config_text_with_optional_catalog(
                config,
                cfg,
                crate::codex_config::CodexCatalogToolProfile::ProxyChat,
            )
        })
        .transpose()
        .map_err(|e| format!("写入 Codex 配置失败: {e}"))?;

    match (auth, prepared_cfg.as_deref()) {
        (Some(auth), Some(cfg)) => {
            if auth.as_object().is_some_and(|obj| obj.is_empty()) {
                // An empty provider snapshot must not destroy an existing
                // Codex login. This is especially important when takeover
                // switches from an API-key-backed official session to the
                // built-in empty `codex-official` seed.
                let config_path = get_codex_config_path();
                crate::config::write_text_file(&config_path, cfg)
                    .map_err(|e| format!("写入 Codex config 失败: {e}"))?;
            } else {
                crate::codex_config::write_codex_live_atomic(auth, Some(cfg))
                    .map_err(|e| format!("写入 Codex 配置失败: {e}"))?;
            }
        }
        (Some(auth), None) => {
            let auth_path = get_codex_auth_path();
            write_json_file(&auth_path, auth).map_err(|e| format!("写入 Codex auth 失败: {e}"))?;
        }
        (None, Some(cfg)) => {
            let config_path = get_codex_config_path();
            crate::config::write_text_file(&config_path, cfg)
                .map_err(|e| format!("写入 Codex config 失败: {e}"))?;
        }
        (None, None) => {}
    }

    Ok(())
}
