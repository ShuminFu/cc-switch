//! Reading and writing each app's live config files on behalf of the proxy.

use super::*;

impl ProxyService {
    pub(super) fn read_claude_live(&self) -> Result<Value, TakeoverError> {
        let path = get_claude_settings_path();
        if !path.exists() {
            return Err(TakeoverError::new(
                TakeoverErrorKind::LiveRead,
                "Claude 配置文件不存在",
            ));
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
            return Err(TakeoverError::new(
                TakeoverErrorKind::LiveRead,
                format!(
                    "Claude 配置文件格式错误：根节点必须是 JSON 对象（当前为 {kind}），路径: {}",
                    path.display()
                ),
            ));
        }

        Ok(value)
    }

    pub(super) fn write_claude_live(&self, config: &Value) -> Result<(), TakeoverError> {
        #[cfg(test)]
        if super::tests::FAIL_CLAUDE_LIVE_WRITE.with(|fail| fail.get()) {
            return Err(TakeoverError::new(
                TakeoverErrorKind::LiveWrite,
                "写入 Claude 配置失败: injected test failure",
            ));
        }
        let path = get_claude_settings_path();
        let settings = crate::services::provider::sanitize_claude_settings_for_live(config);
        write_json_file(&path, &settings).map_err(TakeoverError::with(
            TakeoverErrorKind::LiveWrite,
            "写入 Claude 配置失败",
        ))
    }

    pub(super) fn read_codex_live(&self) -> Result<Value, TakeoverError> {
        crate::codex_config::read_codex_live_settings().map_err(TakeoverError::with(
            TakeoverErrorKind::LiveRead,
            "读取 Codex Live 配置失败",
        ))
    }

    pub(super) fn write_codex_live(&self, config: &Value) -> Result<(), TakeoverError> {
        self.write_codex_live_verbatim(config)
    }

    pub(super) fn write_codex_live_for_provider(
        &self,
        config: &Value,
        provider: Option<&Provider>,
    ) -> Result<(), TakeoverError> {
        let Some(provider) = provider else {
            if crate::settings::preserve_codex_official_auth_on_switch() {
                if let (Some(auth), Some(config_str)) = (
                    config.get("auth"),
                    config.get("config").and_then(|v| v.as_str()),
                ) {
                    if auth.get("OPENAI_API_KEY").and_then(|v| v.as_str())
                        == Some(PROXY_TOKEN_PLACEHOLDER)
                    {
                        let live_config = crate::codex_config::prepare_codex_provider_live_config(
                            auth, config_str,
                        )
                        .map_err(|e| format!("写入 Codex 配置失败: {e}"))?;
                        crate::codex_config::write_codex_live_config_atomic(Some(&live_config))
                            .map_err(|e| format!("写入 Codex 配置失败: {e}"))?;
                        return Ok(());
                    }
                }
            }

            return self.write_codex_live_verbatim(config);
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
        .map_err(TakeoverError::with(
            TakeoverErrorKind::LiveWrite,
            "写入 Codex 配置失败",
        ))
    }

    pub(super) fn codex_auth_has_proxy_placeholder(auth: &Value) -> bool {
        auth.get("OPENAI_API_KEY").and_then(|v| v.as_str()) == Some(PROXY_TOKEN_PLACEHOLDER)
    }

    pub(super) fn write_codex_takeover_live_for_provider(
        &self,
        config: &Value,
        provider: Option<&Provider>,
    ) -> Result<(), TakeoverError> {
        let official_passthrough =
            provider.is_some_and(crate::proxy::providers::is_codex_official_provider);
        let placeholder_auth = config
            .get("auth")
            .is_some_and(Self::codex_auth_has_proxy_placeholder);

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

        self.write_codex_live_for_provider(config, provider)
    }

    pub(super) fn write_codex_live_verbatim(&self, config: &Value) -> Result<(), TakeoverError> {
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
        // provider switch rewrites it via write_live_snapshot. Acceptable known
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
                write_json_file(&auth_path, auth)
                    .map_err(|e| format!("写入 Codex auth 失败: {e}"))?;
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

    pub(super) fn read_gemini_live(&self) -> Result<Value, TakeoverError> {
        use crate::gemini_config::{env_to_json, get_gemini_env_path, read_gemini_env};

        let env_path = get_gemini_env_path();
        if !env_path.exists() {
            return Err(TakeoverError::new(
                TakeoverErrorKind::LiveRead,
                "Gemini .env 文件不存在",
            ));
        }

        let env_map = read_gemini_env().map_err(|e| format!("读取 Gemini env 失败: {e}"))?;
        Ok(env_to_json(&env_map))
    }

    pub(super) fn write_gemini_live(&self, config: &Value) -> Result<(), TakeoverError> {
        use crate::gemini_config::{json_to_env, write_gemini_env_atomic};

        let env_map = json_to_env(config).map_err(|e| format!("转换 Gemini 配置失败: {e}"))?;
        write_gemini_env_atomic(&env_map).map_err(|e| format!("写入 Gemini env 失败: {e}"))?;
        Ok(())
    }

    pub(super) fn read_grok_live(&self) -> Result<Value, TakeoverError> {
        crate::grok_config::read_grok_live_settings().map_err(TakeoverError::with(
            TakeoverErrorKind::LiveRead,
            "读取 Grok Build 配置失败",
        ))
    }

    pub(super) fn write_grok_live(&self, config: &Value) -> Result<(), TakeoverError> {
        crate::grok_config::write_grok_live_settings(config).map_err(TakeoverError::with(
            TakeoverErrorKind::LiveWrite,
            "写入 Grok Build 配置失败",
        ))
    }

    // ==================== 原有方法 ====================
}
