//! 代理接管 Grok Build `config.toml` 的读写、清理与凭据回写

use serde_json::{json, Value};

use super::config::{get_grok_config_path, read_grok_live_settings, write_grok_live_settings};
use crate::app_config::AppType;
use crate::apps::adapter::{current_provider_for_sync, proxy_urls_match};
use crate::database::Database;
use crate::proxy::inbound_auth::is_proxy_token;

pub(super) fn read_live() -> Result<Value, String> {
    read_grok_live_settings().map_err(|e| format!("读取 Grok Build 配置失败: {e}"))
}

pub(super) fn write_live(config: &Value) -> Result<(), String> {
    write_grok_live_settings(config).map_err(|e| format!("写入 Grok Build 配置失败: {e}"))
}

/// 清空 config.toml 中的代理 token。
///
/// 正常情况下会先用供应商快照恢复；这里清掉 token，避免残留的本地路由看起来仍可用。
pub(super) fn strip_placeholders(live: &mut Value) -> Result<bool, String> {
    let Some(config_toml) = live.get("config").and_then(Value::as_str) else {
        return Ok(false);
    };
    if !super::config::has_proxy_token(config_toml) {
        return Ok(false);
    }

    let updated = super::config::update_api_key(config_toml, "")
        .map_err(|e| format!("清理 Grok Build 接管占位符失败: {e}"))?;
    live["config"] = json!(updated);
    Ok(true)
}

/// 清空 token 后的配置不再满足 `write_grok_live_settings` 的校验（缺少 api_key），
/// 按原文写入
pub(super) fn write_stripped_live(live: &Value) -> Result<(), String> {
    let config_toml = live
        .get("config")
        .and_then(Value::as_str)
        .unwrap_or_default();
    crate::config::write_text_file(&get_grok_config_path(), config_toml)
        .map_err(|e| format!("写入 Grok Build 配置失败: {e}"))
}

pub(super) fn base_url_matches(live: &Value, expected: &str) -> bool {
    live.get("config")
        .and_then(Value::as_str)
        .is_some_and(|text| {
            super::config::base_url_matches(text, |url| proxy_urls_match(url, expected))
        })
}

/// Grok Build 的回写失败会中止接管：清空 live token 后 config.toml 里不再有可用凭据
pub(super) fn sync_live_credentials(db: &Database, live: &Value) -> Result<(), String> {
    let Some(mut provider) = current_provider_for_sync(db, &AppType::GrokBuild, "Grok Build")?
    else {
        return Ok(());
    };
    let live_config_toml = live
        .get("config")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let Some(token) = super::config::extract_inline_api_key(live_config_toml) else {
        return Ok(());
    };
    if token.is_empty() || is_proxy_token(&token) {
        return Ok(());
    }
    let Some(provider_config) = provider
        .settings_config
        .get("config")
        .and_then(Value::as_str)
    else {
        return Ok(());
    };

    let updated = super::config::update_api_key(provider_config, &token)
        .map_err(|e| format!("更新 Grok Build API Key 失败: {e}"))?;
    provider.settings_config["config"] = json!(updated);
    db.update_provider_settings_config(
        AppType::GrokBuild.as_str(),
        &provider.id,
        &provider.settings_config,
    )
    .map_err(|e| format!("同步 Grok Build Token 到数据库失败: {e}"))
}
