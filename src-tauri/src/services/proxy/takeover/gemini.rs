//! Gemini Live 配置接管字段与读写

use super::*;
use crate::app_config::AppType;
use crate::proxy::inbound_auth::is_proxy_token;
use serde_json::Value;

impl ProxyService {
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

    pub(in crate::services::proxy) fn is_gemini_live_taken_over(config: &Value) -> bool {
        Self::adapter_live_has_placeholder(&AppType::Gemini, config)
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
