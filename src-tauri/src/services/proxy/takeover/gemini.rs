//! Gemini Live 配置接管字段与读写

use super::*;
use crate::proxy::inbound_auth::is_proxy_token;
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

    pub(in crate::services::proxy) fn cleanup_gemini_takeover_placeholders_in_live(
        &self,
    ) -> Result<(), String> {
        let mut config = self.read_gemini_live()?;

        let Some(env) = config.get_mut("env").and_then(|v| v.as_object_mut()) else {
            return Ok(());
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
            .map(Self::is_local_proxy_url)
            .unwrap_or(false)
        {
            env.remove("GOOGLE_GEMINI_BASE_URL");
        }

        self.write_gemini_live(&config)?;
        Ok(())
    }

    pub(in crate::services::proxy) fn read_gemini_live(&self) -> Result<Value, String> {
        use crate::gemini_config::{env_to_json, get_gemini_env_path, read_gemini_env};

        let env_path = get_gemini_env_path();
        if !env_path.exists() {
            return Err("Gemini .env 文件不存在".to_string());
        }

        let env_map = read_gemini_env().map_err(|e| format!("读取 Gemini env 失败: {e}"))?;
        Ok(env_to_json(&env_map))
    }

    pub(in crate::services::proxy) fn write_gemini_live(
        &self,
        config: &Value,
    ) -> Result<(), String> {
        use crate::gemini_config::{json_to_env, write_gemini_env_atomic};

        let config = self.bind_proxy_token(config)?;
        let env_map = json_to_env(&config).map_err(|e| format!("转换 Gemini 配置失败: {e}"))?;
        write_gemini_env_atomic(&env_map).map_err(|e| format!("写入 Gemini env 失败: {e}"))?;
        Ok(())
    }
}
