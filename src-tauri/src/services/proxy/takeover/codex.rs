//! Codex Live 配置接管字段
//!
//! Live 的读写、清理与凭据回写位于 [`crate::apps::codex::takeover`]；这里保留绑定
//! 代理 token 后转发的入口。

use super::*;
use crate::provider::Provider;
use serde_json::{json, Value};

impl ProxyService {
    /// 接管 Codex 时，本地客户端必须继续以 Responses wire API 访问代理。
    /// 真实上游是否走 Chat Completions 由 provider 配置决定，并在代理内部转换。
    pub(in crate::services::proxy) fn apply_codex_proxy_toml_config_for_provider(
        toml_str: &str,
        proxy_url: &str,
        provider: Option<&Provider>,
    ) -> Result<String, String> {
        if provider.is_some_and(crate::proxy::providers::is_codex_official_provider) {
            return crate::codex_config::apply_codex_official_proxy_route(toml_str, proxy_url)
                .map_err(|e| format!("生成 Codex 官方接管配置失败: {e}"));
        }

        let updated = crate::codex_config::update_codex_toml_field(toml_str, "base_url", proxy_url)
            .map_err(|e| format!("更新 Codex 代理地址失败: {e}"))?;
        let mut updated =
            crate::codex_config::update_codex_toml_field(&updated, "wire_api", "responses")
                .map_err(|e| format!("更新 Codex wire_api 失败: {e}"))?;

        if let Some(upstream_model) =
            provider.and_then(crate::proxy::providers::codex_provider_upstream_model)
        {
            updated =
                crate::codex_config::update_codex_toml_field(&updated, "model", &upstream_model)
                    .map_err(|e| format!("更新 Codex 上游模型失败: {e}"))?;
        }

        Ok(updated)
    }

    pub(in crate::services::proxy) fn apply_codex_takeover_auth_placeholder(
        settings: &mut Value,
        provider: Option<&Provider>,
    ) {
        if provider.is_some_and(crate::proxy::providers::is_codex_official_provider) {
            return;
        }

        if let Some(auth) = settings.get_mut("auth").and_then(|v| v.as_object_mut()) {
            auth.insert("OPENAI_API_KEY".to_string(), json!(PROXY_TOKEN_PLACEHOLDER));
        } else if let Some(root) = settings.as_object_mut() {
            root.insert(
                "auth".to_string(),
                json!({ "OPENAI_API_KEY": PROXY_TOKEN_PLACEHOLDER }),
            );
        }
    }

    pub(in crate::services::proxy) fn apply_codex_takeover_fields_for_provider(
        settings: &mut Value,
        proxy_base_url: &str,
        provider: &Provider,
    ) -> Result<(), String> {
        Self::apply_codex_takeover_auth_placeholder(settings, Some(provider));
        let config_text = settings
            .get("config")
            .and_then(|value| value.as_str())
            .unwrap_or("")
            .to_string();
        let projected = Self::apply_codex_proxy_toml_config_for_provider(
            &config_text,
            proxy_base_url,
            Some(provider),
        )?;
        settings["config"] = json!(projected);
        Self::attach_codex_model_catalog_from_provider(settings, Some(provider));
        Ok(())
    }

    pub(in crate::services::proxy) fn attach_codex_model_catalog_from_provider(
        live_config: &mut Value,
        provider: Option<&Provider>,
    ) {
        let Some(provider) = provider else {
            return;
        };

        let model_catalog = provider
            .settings_config
            .get("modelCatalog")
            .cloned()
            .unwrap_or_else(|| json!({ "models": [] }));

        if let Some(root) = live_config.as_object_mut() {
            root.insert("modelCatalog".to_string(), model_catalog);
        }
    }

    pub(in crate::services::proxy) fn read_codex_live(&self) -> Result<Value, String> {
        self.read_live_for_takeover(&AppType::Codex)
    }

    #[cfg(test)]
    pub(in crate::services::proxy) fn write_codex_live(
        &self,
        config: &Value,
    ) -> Result<(), String> {
        self.write_codex_live_verbatim(config)
    }

    #[cfg(test)]
    pub(in crate::services::proxy) fn write_codex_live_for_provider(
        &self,
        config: &Value,
        provider: Option<&Provider>,
    ) -> Result<(), String> {
        let config = self.bind_proxy_token(config)?;
        crate::apps::codex::takeover::write_live_for_provider(&config, provider)
    }

    pub(in crate::services::proxy) fn codex_auth_has_proxy_placeholder(auth: &Value) -> bool {
        crate::apps::codex::takeover::auth_has_proxy_placeholder(auth)
    }

    pub(in crate::services::proxy) fn write_codex_takeover_live_for_provider(
        &self,
        config: &Value,
        provider: Option<&Provider>,
    ) -> Result<(), String> {
        let config = self.bind_proxy_token(config)?;
        crate::apps::codex::takeover::write_takeover_live(&config, provider)
    }

    pub(in crate::services::proxy) fn write_codex_live_verbatim(
        &self,
        config: &Value,
    ) -> Result<(), String> {
        self.write_live_config_for_app(&AppType::Codex, config)
    }
}
