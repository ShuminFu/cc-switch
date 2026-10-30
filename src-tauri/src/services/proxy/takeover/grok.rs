//! Grok Build Live 配置接管字段与读写

use super::*;
use serde_json::{json, Value};

impl ProxyService {
    pub(in crate::services::proxy) fn apply_grok_takeover_fields(
        config: &mut Value,
        proxy_base_url: &str,
    ) -> Result<(), String> {
        let config_toml = config
            .get("config")
            .and_then(Value::as_str)
            .ok_or_else(|| "Grok Build 配置缺少 config 字段".to_string())?;
        let updated = crate::grok_config::apply_proxy_takeover(
            config_toml,
            proxy_base_url,
            PROXY_TOKEN_PLACEHOLDER,
        )
        .map_err(|e| format!("更新 Grok Build 接管配置失败: {e}"))?;
        config["config"] = json!(updated);
        Ok(())
    }

    pub(in crate::services::proxy) fn cleanup_grok_takeover_placeholders_in_live(
        &self,
    ) -> Result<(), String> {
        let config = self.read_grok_live()?;
        let Some(config_toml) = config.get("config").and_then(Value::as_str) else {
            return Ok(());
        };
        if !crate::grok_config::has_proxy_token(config_toml) {
            return Ok(());
        }

        // A valid provider snapshot should normally restore before this fallback.
        // Clearing the token prevents a stale local route from looking usable.
        let updated = crate::grok_config::update_api_key(config_toml, "")
            .map_err(|e| format!("清理 Grok Build 接管占位符失败: {e}"))?;
        crate::config::write_text_file(&crate::grok_config::get_grok_config_path(), &updated)
            .map_err(|e| format!("写入 Grok Build 配置失败: {e}"))
    }

    pub(in crate::services::proxy) fn read_grok_live(&self) -> Result<Value, String> {
        crate::grok_config::read_grok_live_settings()
            .map_err(|e| format!("读取 Grok Build 配置失败: {e}"))
    }

    pub(in crate::services::proxy) fn write_grok_live(&self, config: &Value) -> Result<(), String> {
        let config = self.bind_proxy_token(config)?;
        crate::grok_config::write_grok_live_settings(&config)
            .map_err(|e| format!("写入 Grok Build 配置失败: {e}"))
    }
}
