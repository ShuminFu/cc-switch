//! 代理接管 Claude Code `settings.json` 的读写、清理与凭据回写

use serde_json::{json, Value};

use super::TAKEOVER_TOKEN_KEYS;
use crate::app_config::AppType;
use crate::apps::adapter::{current_provider_for_sync, is_local_proxy_url, save_synced_provider};
use crate::config::{get_claude_settings_path, read_json_file, write_json_file};
use crate::database::Database;
use crate::proxy::inbound_auth::is_proxy_token;

pub(super) fn read_live() -> Result<Value, String> {
    let path = get_claude_settings_path();
    if !path.exists() {
        return Err("Claude 配置文件不存在".to_string());
    }

    let mut value: Value =
        read_json_file(&path).map_err(|e| format!("读取 Claude 配置失败: {e}"))?;

    if value.is_null() {
        value = json!({});
    }

    if !value.is_object() {
        let kind = match &value {
            Value::Null => "null",
            Value::Bool(_) => "boolean",
            Value::Number(_) => "number",
            Value::String(_) => "string",
            Value::Array(_) => "array",
            Value::Object(_) => "object",
        };
        return Err(format!(
            "Claude 配置文件格式错误：根节点必须是 JSON 对象（当前为 {kind}），路径: {}",
            path.display()
        ));
    }

    Ok(value)
}

pub(super) fn write_live(config: &Value) -> Result<(), String> {
    let path = get_claude_settings_path();
    let settings = super::sanitize_settings_for_live(config);
    write_json_file(&path, &settings).map_err(|e| format!("写入 Claude 配置失败: {e}"))
}

pub(super) fn strip_placeholders(live: &mut Value) -> bool {
    let Some(env) = live.get_mut("env").and_then(|v| v.as_object_mut()) else {
        return false;
    };

    for key in TAKEOVER_TOKEN_KEYS {
        if env
            .get(key)
            .and_then(|v| v.as_str())
            .is_some_and(is_proxy_token)
        {
            env.remove(key);
        }
    }

    if env
        .get("ANTHROPIC_BASE_URL")
        .and_then(|v| v.as_str())
        .map(is_local_proxy_url)
        .unwrap_or(false)
    {
        env.remove("ANTHROPIC_BASE_URL");
    }
    true
}

pub(super) fn sync_live_credentials(db: &Database, live: &Value) -> Result<(), String> {
    let Some(mut provider) = current_provider_for_sync(db, &AppType::Claude, "Claude")? else {
        return Ok(());
    };
    let Some(env) = live.get("env").and_then(|v| v.as_object()) else {
        return Ok(());
    };
    let token_pair = TAKEOVER_TOKEN_KEYS
        .into_iter()
        .find_map(|key| {
            env.get(key)
                .and_then(|v| v.as_str())
                .map(|s| (key, s.trim()))
        })
        .filter(|(_, token)| !token.is_empty() && !is_proxy_token(token));
    let Some((token_key, token)) = token_pair else {
        return Ok(());
    };

    let env_obj = provider
        .settings_config
        .get_mut("env")
        .and_then(|v| v.as_object_mut());

    match env_obj {
        Some(obj) => {
            if token_key == "ANTHROPIC_AUTH_TOKEN" || token_key == "ANTHROPIC_API_KEY" {
                let mut updated = false;
                if obj.contains_key("ANTHROPIC_AUTH_TOKEN") {
                    obj.insert("ANTHROPIC_AUTH_TOKEN".to_string(), json!(token));
                    updated = true;
                }
                if obj.contains_key("ANTHROPIC_API_KEY") {
                    obj.insert("ANTHROPIC_API_KEY".to_string(), json!(token));
                    updated = true;
                }
                if !updated {
                    obj.insert(token_key.to_string(), json!(token));
                }
            } else {
                obj.insert(token_key.to_string(), json!(token));
            }
        }
        None => {
            // 至少写入一份可用的 Token
            if provider.settings_config.is_null() {
                provider.settings_config = json!({});
            }

            if let Some(root) = provider.settings_config.as_object_mut() {
                root.insert("env".to_string(), json!({ token_key: token }));
            } else {
                log::warn!(
                    "Claude provider settings_config 格式异常（非对象），跳过写入 Token (provider: {})",
                    provider.id
                );
            }
        }
    }

    save_synced_provider(db, &AppType::Claude, "Claude", &provider);
    Ok(())
}
