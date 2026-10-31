//! Grok Build Live 配置接管字段
//!
//! Live 的读写、清理与凭据回写位于 [`crate::apps::grok`] 的 `ProxyTakeover` 实现。

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

    pub(in crate::services::proxy) fn read_grok_live(&self) -> Result<Value, String> {
        self.read_live_for_takeover(&AppType::GrokBuild)
    }

    pub(in crate::services::proxy) fn write_grok_live(&self, config: &Value) -> Result<(), String> {
        self.write_live_config_for_app(&AppType::GrokBuild, config)
    }
}
