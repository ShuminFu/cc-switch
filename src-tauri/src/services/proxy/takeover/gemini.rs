//! Gemini Live 配置接管字段
//!
//! Live 的读写、清理与凭据回写位于 [`crate::apps::gemini`] 的 `ProxyTakeover` 实现。

use super::*;
use serde_json::{json, Value};

impl ProxyService {
    /// 写入 `GOOGLE_GEMINI_BASE_URL`，并用占位符替代真实 Token（代理会注入真实 Token）
    pub(in crate::services::proxy) fn apply_gemini_takeover_fields(
        config: &mut Value,
        proxy_base_url: &str,
    ) {
        if let Some(env) = config.get_mut("env").and_then(|v| v.as_object_mut()) {
            env.insert("GOOGLE_GEMINI_BASE_URL".to_string(), json!(proxy_base_url));
            // 使用占位符，避免显示缺少 key 的警告
            env.insert("GEMINI_API_KEY".to_string(), json!(PROXY_TOKEN_PLACEHOLDER));
        } else {
            config["env"] = json!({
                "GOOGLE_GEMINI_BASE_URL": proxy_base_url,
                "GEMINI_API_KEY": PROXY_TOKEN_PLACEHOLDER
            });
        }
    }

    pub(in crate::services::proxy) fn read_gemini_live(&self) -> Result<Value, String> {
        self.read_live_for_takeover(&AppType::Gemini)
    }

    pub(in crate::services::proxy) fn write_gemini_live(
        &self,
        config: &Value,
    ) -> Result<(), String> {
        self.write_live_config_for_app(&AppType::Gemini, config)
    }
}
