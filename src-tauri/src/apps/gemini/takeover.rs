//! 代理接管 Gemini CLI `.env` 的读写、清理与凭据回写
//!
//! 接管只改写 `.env`；`settings.json`（含 mcpServers）保持原样。

use serde_json::{json, Value};

use super::config::{env_to_json, get_gemini_env_path, json_to_env, read_gemini_env};
use crate::app_config::AppType;
use crate::apps::adapter::{current_provider_for_sync, is_local_proxy_url, save_synced_provider};
use crate::database::Database;
use crate::proxy::inbound_auth::is_proxy_token;

pub(super) fn read_live() -> Result<Value, String> {
    if !get_gemini_env_path().exists() {
        return Err("Gemini .env 文件不存在".to_string());
    }

    let env_map = read_gemini_env().map_err(|e| format!("读取 Gemini env 失败: {e}"))?;
    Ok(env_to_json(&env_map))
}

pub(super) fn write_live(config: &Value) -> Result<(), String> {
    let env_map = json_to_env(config).map_err(|e| format!("转换 Gemini 配置失败: {e}"))?;
    super::config::write_gemini_env_atomic(&env_map)
        .map_err(|e| format!("写入 Gemini env 失败: {e}"))
}

pub(super) fn strip_placeholders(live: &mut Value) -> bool {
    let Some(env) = live.get_mut("env").and_then(|v| v.as_object_mut()) else {
        return false;
    };

    if env
        .get("GEMINI_API_KEY")
        .and_then(|v| v.as_str())
        .is_some_and(is_proxy_token)
    {
        env.remove("GEMINI_API_KEY");
    }

    if env
        .get("GOOGLE_GEMINI_BASE_URL")
        .and_then(|v| v.as_str())
        .map(is_local_proxy_url)
        .unwrap_or(false)
    {
        env.remove("GOOGLE_GEMINI_BASE_URL");
    }
    true
}

pub(super) fn sync_live_credentials(db: &Database, live: &Value) -> Result<(), String> {
    let Some(mut provider) = current_provider_for_sync(db, &AppType::Gemini, "Gemini")? else {
        return Ok(());
    };
    let Some(token) = live
        .get("env")
        .and_then(|v| v.get("GEMINI_API_KEY"))
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty() && !is_proxy_token(s))
    else {
        return Ok(());
    };

    if let Some(env_obj) = provider
        .settings_config
        .get_mut("env")
        .and_then(|v| v.as_object_mut())
    {
        env_obj.insert("GEMINI_API_KEY".to_string(), json!(token));
    } else {
        if provider.settings_config.is_null() {
            provider.settings_config = json!({});
        }

        if let Some(root) = provider.settings_config.as_object_mut() {
            root.insert("env".to_string(), json!({ "GEMINI_API_KEY": token }));
        } else {
            log::warn!(
                "Gemini provider settings_config 格式异常（非对象），跳过写入 Token (provider: {})",
                provider.id
            );
        }
    }

    save_synced_provider(db, &AppType::Gemini, "Gemini", &provider);
    Ok(())
}

/// 接管备份只保存 `.env`
pub(super) fn backup_projection(settings: Value) -> Value {
    match settings.get("env") {
        Some(env) => json!({ "env": env }),
        None => json!({ "env": {} }),
    }
}
