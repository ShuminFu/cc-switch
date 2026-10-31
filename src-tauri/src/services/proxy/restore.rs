//! Releasing a takeover: restoring live configs from the backup, the SSOT or
//! by cleaning placeholders, plus takeover detection and crash recovery.

use super::*;

impl ProxyService {
    pub(super) async fn restore_live_config_for_app_inner(
        &self,
        app_type: &AppType,
    ) -> Result<(), String> {
        if !crate::adapters::adapter_for(app_type)
            .capabilities()
            .proxy_takeover
        {
            return Ok(());
        }
        let app_type_str = app_type.as_str();
        if let Ok(Some(backup)) = self.db.get_live_backup(app_type_str).await {
            let config: Value = serde_json::from_str(&backup.original_config)
                .map_err(|e| format!("解析 {app_type_str} 备份失败: {e}"))?;
            self.write_live_config_for_app(app_type, &config)?;
            log::info!("{app_type_str} Live 配置已恢复");
        }

        Ok(())
    }

    /// 恢复原始 Live 配置
    pub(super) async fn restore_live_configs(&self) -> Result<(), String> {
        let mut errors = Vec::new();

        for adapter in crate::adapters::takeover_adapters() {
            if let Err(e) = self
                .restore_live_config_for_app_with_fallback(&adapter.app())
                .await
            {
                errors.push(e);
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("；"))
        }
    }

    pub(super) async fn restore_live_config_for_app_with_fallback(
        &self,
        app_type: &AppType,
    ) -> Result<(), String> {
        let _guard = self.switch_locks.lock_for_app(app_type.as_str()).await;
        self.restore_live_config_for_app_with_fallback_inner(app_type)
            .await
    }

    pub(crate) async fn restore_live_config_for_app_with_fallback_inner(
        &self,
        app_type: &AppType,
    ) -> Result<(), String> {
        let app_type_str = app_type.as_str();

        // 1) 优先从 Live 备份恢复（这是"原始 Live"的唯一可靠来源）
        let backup = self
            .db
            .get_live_backup(app_type_str)
            .await
            .map_err(|e| format!("获取 {app_type_str} Live 备份失败: {e}"))?;
        if let Some(backup) = backup {
            let config: Value = serde_json::from_str(&backup.original_config)
                .map_err(|e| format!("解析 {app_type_str} 备份失败: {e}"))?;

            // 备份若是代理占位符（异常历史：上次 stop 失败导致 Live 留在了代理状态，
            // 下次接管时又被错误地备份成"原始 Live"），不能直接用 — 否则 stop 后
            // Live 永远卡在 127.0.0.1:15721。落到下面的 SSOT 兜底重建。
            if Self::live_has_proxy_placeholder_for_app(app_type, &config) {
                log::warn!(
                    "{app_type_str} 备份本身已是代理占位符（异常历史状态），跳过备份，改走 SSOT 重建 Live"
                );
            } else {
                self.write_live_config_for_app(app_type, &config)?;
                log::info!("{app_type_str} Live 配置已从备份恢复");
                return Ok(());
            }
        }

        // 2) 兜底：备份缺失，但 Live 仍包含接管占位符（异常退出/历史 bug 场景）
        if !self.detect_takeover_in_live_config_for_app(app_type) {
            return Ok(());
        }

        // 2.1) 优先从 SSOT（当前供应商）重建 Live（比"清理字段"更可用）
        match self.restore_live_from_ssot_for_app(app_type) {
            Ok(true) => {
                log::info!("{app_type_str} Live 配置已从 SSOT 恢复（无备份兜底）");
                return Ok(());
            }
            Ok(false) => {
                log::warn!(
                    "{app_type_str} Live 备份缺失，且无法从 SSOT 恢复，将尝试清理接管占位符"
                );
            }
            Err(e) => {
                log::error!(
                    "{app_type_str} Live 备份缺失，SSOT 恢复失败，将尝试清理接管占位符: {e}"
                );
            }
        }

        // 2.2) 最后兜底：尽力清理占位符与本地代理地址，避免长期卡在代理占位符状态
        self.cleanup_takeover_placeholders_in_live_for_app(app_type)?;
        log::info!("{app_type_str} Live 接管占位符已清理（无备份兜底）");
        Ok(())
    }

    pub(super) fn write_live_config_for_app(
        &self,
        app_type: &AppType,
        config: &Value,
    ) -> Result<(), String> {
        match app_type {
            AppType::Claude => self.write_claude_live(config),
            AppType::Codex => self.write_codex_live(config),
            AppType::Gemini => self.write_gemini_live(config),
            AppType::GrokBuild => self.write_grok_live(config),
            _ => Err("该应用不支持代理功能".to_string()),
        }
    }

    /// 读取代理接管所备份/恢复的原始 Live 快照（每个支持接管的应用一处分派）。
    pub(super) fn read_live_config_for_app(&self, app_type: &AppType) -> Result<Value, String> {
        match app_type {
            AppType::Claude => self.read_claude_live(),
            AppType::Codex => self.read_codex_live(),
            AppType::Gemini => self.read_gemini_live(),
            AppType::GrokBuild => self.read_grok_live(),
            _ => Err("该应用不支持代理功能".to_string()),
        }
    }

    pub fn detect_takeover_in_live_config_for_app(&self, app_type: &AppType) -> bool {
        match self.read_live_config_for_app(app_type) {
            Ok(config) => Self::live_has_proxy_placeholder_for_app(app_type, &config),
            Err(_) => false,
        }
    }

    /// 当 Live 备份缺失时，尝试用 SSOT（当前供应商）写回 Live，以解除占位符接管。
    ///
    /// 返回值：
    /// - Ok(true)：已成功写回
    /// - Ok(false)：缺少当前供应商/供应商不存在/供应商本身含占位符，无法写回
    pub(super) fn restore_live_from_ssot_for_app(
        &self,
        app_type: &AppType,
    ) -> Result<bool, String> {
        let current_id = crate::settings::get_effective_current_provider(&self.db, app_type)
            .map_err(|e| format!("获取 {app_type:?} 当前供应商失败: {e}"))?;

        let Some(current_id) = current_id else {
            return Ok(false);
        };

        let providers = self
            .db
            .get_all_providers(app_type.as_str())
            .map_err(|e| format!("读取 {app_type:?} 供应商列表失败: {e}"))?;

        let Some(provider) = providers.get(&current_id) else {
            return Ok(false);
        };

        // 供应商配置本身含接管占位符时不可写回（历史异常：接管期间 Live 被
        // 误导入成了供应商）。写回只会把占位符固化进 Live；返回 Ok(false)
        // 让调用方落到"清理占位符"兜底。
        if Self::live_has_proxy_placeholder_for_app(app_type, &provider.settings_config) {
            log::warn!(
                "{app_type:?} 当前供应商配置含代理接管占位符（疑似接管期间被导入的残留），跳过 SSOT 写回，改走占位符清理"
            );
            return Ok(false);
        }

        write_live_with_common_config(self.db.as_ref(), app_type, provider)
            .map_err(|e| format!("写入 {app_type:?} Live 配置失败: {e}"))?;

        Ok(true)
    }

    pub(super) fn cleanup_takeover_placeholders_in_live_for_app(
        &self,
        app_type: &AppType,
    ) -> Result<(), String> {
        match app_type {
            AppType::Claude => self.cleanup_claude_takeover_placeholders_in_live(),
            AppType::Codex => self.cleanup_codex_takeover_placeholders_in_live(),
            AppType::Gemini => self.cleanup_gemini_takeover_placeholders_in_live(),
            AppType::GrokBuild => self.cleanup_grok_takeover_placeholders_in_live(),
            _ => Ok(()),
        }
    }

    pub(super) fn is_local_proxy_url(url: &str) -> bool {
        let url = url.trim();
        if !url.starts_with("http://") {
            return false;
        }
        let rest = &url["http://".len()..];
        rest.starts_with("127.0.0.1")
            || rest.starts_with("localhost")
            || rest.starts_with("0.0.0.0")
            || rest.starts_with("[::1]")
            || rest.starts_with("[::]")
            || rest.starts_with("::1")
            || rest.starts_with("::")
    }

    pub(super) fn proxy_urls_match(actual: &str, expected: &str) -> bool {
        actual.trim().trim_end_matches('/') == expected.trim().trim_end_matches('/')
    }

    pub(super) fn codex_config_has_base_url_matching(
        config_text: &str,
        predicate: impl Fn(&str) -> bool,
    ) -> bool {
        let Ok(doc) = toml::from_str::<toml::Value>(config_text) else {
            return false;
        };

        let active_provider = doc
            .get("model_provider")
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|id| !id.is_empty());

        if let Some(provider_id) = active_provider {
            if doc
                .get("model_providers")
                .and_then(|value| value.get(provider_id))
                .and_then(|value| value.get("base_url"))
                .and_then(|value| value.as_str())
                .is_some_and(&predicate)
            {
                return true;
            }
        }

        doc.get("base_url")
            .and_then(|value| value.as_str())
            .is_some_and(predicate)
    }

    pub(super) async fn live_takeover_matches_current_proxy(
        &self,
        app_type: &AppType,
    ) -> Result<bool, String> {
        let (proxy_url, proxy_codex_base_url) = self.build_proxy_urls().await?;
        let proxy_grok_base_url = format!("{}/grokbuild/v1", proxy_url.trim_end_matches('/'));

        match app_type {
            AppType::Claude => {
                let config = self.read_claude_live()?;
                let base_url_matches = config
                    .get("env")
                    .and_then(|value| value.get("ANTHROPIC_BASE_URL"))
                    .and_then(|value| value.as_str())
                    .is_some_and(|url| Self::proxy_urls_match(url, &proxy_url));
                Ok(Self::is_claude_live_taken_over(&config) && base_url_matches)
            }
            AppType::Codex => {
                let config = self.read_codex_live()?;
                let base_url_matches = config
                    .get("config")
                    .and_then(|value| value.as_str())
                    .is_some_and(|config_text| {
                        Self::codex_config_has_base_url_matching(config_text, |url| {
                            Self::proxy_urls_match(url, &proxy_codex_base_url)
                        })
                    });
                Ok(Self::is_codex_live_taken_over(&config) && base_url_matches)
            }
            AppType::Gemini => {
                let config = self.read_gemini_live()?;
                let base_url_matches = config
                    .get("env")
                    .and_then(|value| value.get("GOOGLE_GEMINI_BASE_URL"))
                    .and_then(|value| value.as_str())
                    .is_some_and(|url| Self::proxy_urls_match(url, &proxy_url));
                Ok(Self::is_gemini_live_taken_over(&config) && base_url_matches)
            }
            AppType::GrokBuild => {
                let config = self.read_grok_live()?;
                let base_url_matches =
                    config
                        .get("config")
                        .and_then(Value::as_str)
                        .is_some_and(|config_toml| {
                            crate::grok_config::base_url_matches(config_toml, |url| {
                                Self::proxy_urls_match(url, &proxy_grok_base_url)
                            })
                        });
                Ok(Self::is_grok_live_taken_over(&config) && base_url_matches)
            }
            _ => Ok(false),
        }
    }

    pub(super) fn cleanup_claude_takeover_placeholders_in_live(&self) -> Result<(), String> {
        let mut config = self.read_claude_live()?;

        let Some(env) = config.get_mut("env").and_then(|v| v.as_object_mut()) else {
            return Ok(());
        };

        for key in [
            "ANTHROPIC_AUTH_TOKEN",
            "ANTHROPIC_API_KEY",
            "OPENROUTER_API_KEY",
            "OPENAI_API_KEY",
        ] {
            if env.get(key).and_then(|v| v.as_str()) == Some(PROXY_TOKEN_PLACEHOLDER) {
                env.remove(key);
            }
        }

        if env
            .get("ANTHROPIC_BASE_URL")
            .and_then(|v| v.as_str())
            .map(Self::is_local_proxy_url)
            .unwrap_or(false)
        {
            env.remove("ANTHROPIC_BASE_URL");
        }

        self.write_claude_live(&config)?;
        Ok(())
    }

    pub(super) fn cleanup_codex_takeover_placeholders_in_live(&self) -> Result<(), String> {
        let mut config = self.read_codex_live()?;

        if let Some(auth) = config.get_mut("auth").and_then(|v| v.as_object_mut()) {
            if auth.get("OPENAI_API_KEY").and_then(|v| v.as_str()) == Some(PROXY_TOKEN_PLACEHOLDER)
            {
                auth.remove("OPENAI_API_KEY");
            }
        }

        if let Some(cfg_str) = config.get("config").and_then(|v| v.as_str()) {
            let updated = Self::remove_local_toml_base_url(cfg_str);
            let updated =
                crate::codex_config::remove_codex_experimental_bearer_token_if(&updated, |token| {
                    token == PROXY_TOKEN_PLACEHOLDER
                })
                .map_err(|e| format!("清理 Codex 接管占位符失败: {e}"))?;
            let updated = crate::codex_config::remove_codex_official_proxy_route(&updated)
                .map_err(|e| format!("清理 Codex 官方接管路由失败: {e}"))?;
            config["config"] = json!(updated);
        }

        self.write_codex_live(&config)?;
        Ok(())
    }

    /// Remove local proxy base_url from TOML（委托给 codex_config 共享实现）
    pub(super) fn remove_local_toml_base_url(toml_str: &str) -> String {
        crate::codex_config::remove_codex_toml_base_url_if(toml_str, Self::is_local_proxy_url)
    }

    pub(super) fn cleanup_gemini_takeover_placeholders_in_live(&self) -> Result<(), String> {
        let mut config = self.read_gemini_live()?;

        let Some(env) = config.get_mut("env").and_then(|v| v.as_object_mut()) else {
            return Ok(());
        };

        if env.get("GEMINI_API_KEY").and_then(|v| v.as_str()) == Some(PROXY_TOKEN_PLACEHOLDER) {
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

    pub(super) fn cleanup_grok_takeover_placeholders_in_live(&self) -> Result<(), String> {
        let config = self.read_grok_live()?;
        let Some(config_toml) = config.get("config").and_then(Value::as_str) else {
            return Ok(());
        };
        if !crate::grok_config::has_proxy_placeholder(config_toml, PROXY_TOKEN_PLACEHOLDER) {
            return Ok(());
        }

        // A valid provider snapshot should normally restore before this fallback.
        // Clearing the token prevents a stale local route from looking usable.
        let updated = crate::grok_config::update_api_key(config_toml, "")
            .map_err(|e| format!("清理 Grok Build 接管占位符失败: {e}"))?;
        crate::config::write_text_file(&crate::grok_config::get_grok_config_path(), &updated)
            .map_err(|e| format!("写入 Grok Build 配置失败: {e}"))
    }

    /// 检查是否处于 Live 接管模式
    pub async fn is_takeover_active(&self) -> Result<bool, String> {
        let status = self.get_takeover_status().await?;
        Ok(status.claude || status.codex || status.gemini || status.grokbuild)
    }

    /// 从异常退出中恢复（启动时调用）
    ///
    /// 检测到 Live 备份残留时调用此方法。
    /// 会恢复 Live 配置、清除接管标志、删除备份。
    pub async fn recover_from_crash(&self) -> Result<(), String> {
        // 1. 恢复 Live 配置
        self.restore_live_configs().await?;

        // 2. 清除接管标志
        self.db
            .set_live_takeover_active(false)
            .await
            .map_err(|e| format!("清除接管状态失败: {e}"))?;

        // 3. 删除备份
        self.db
            .delete_all_live_backups()
            .await
            .map_err(|e| format!("删除备份失败: {e}"))?;

        log::info!("已从异常退出中恢复 Live 配置");
        Ok(())
    }

    /// 检测 Live 配置是否处于"被接管"的残留状态
    ///
    /// 用于兜底处理：当数据库备份缺失但 Live 文件已经写成代理占位符时，
    /// 启动流程可以据此触发恢复逻辑。
    pub fn detect_takeover_in_live_configs(&self) -> bool {
        crate::adapters::takeover_adapters()
            .any(|adapter| self.detect_takeover_in_live_config_for_app(&adapter.app()))
    }

    pub(super) fn is_claude_live_taken_over(config: &Value) -> bool {
        let env = match config.get("env").and_then(|v| v.as_object()) {
            Some(env) => env,
            None => return false,
        };

        for key in [
            "ANTHROPIC_AUTH_TOKEN",
            "ANTHROPIC_API_KEY",
            "OPENROUTER_API_KEY",
            "OPENAI_API_KEY",
        ] {
            if env.get(key).and_then(|v| v.as_str()) == Some(PROXY_TOKEN_PLACEHOLDER) {
                return true;
            }
        }

        false
    }

    pub(super) fn codex_live_has_proxy_placeholder(config: &Value) -> bool {
        if config
            .get("auth")
            .and_then(|v| v.as_object())
            .and_then(|auth| auth.get("OPENAI_API_KEY"))
            .and_then(|v| v.as_str())
            == Some(PROXY_TOKEN_PLACEHOLDER)
        {
            return true;
        }

        config
            .get("config")
            .and_then(|v| v.as_str())
            .and_then(crate::codex_config::extract_codex_experimental_bearer_token)
            .as_deref()
            == Some(PROXY_TOKEN_PLACEHOLDER)
    }

    pub(super) fn is_codex_live_taken_over(config: &Value) -> bool {
        Self::codex_live_has_proxy_placeholder(config)
            || config
                .get("config")
                .and_then(|v| v.as_str())
                .is_some_and(crate::codex_config::codex_config_has_official_proxy_route)
    }

    pub(super) fn is_gemini_live_taken_over(config: &Value) -> bool {
        let env = match config.get("env").and_then(|v| v.as_object()) {
            Some(env) => env,
            None => return false,
        };
        env.get("GEMINI_API_KEY").and_then(|v| v.as_str()) == Some(PROXY_TOKEN_PLACEHOLDER)
    }

    pub(super) fn is_grok_live_taken_over(config: &Value) -> bool {
        config
            .get("config")
            .and_then(Value::as_str)
            .is_some_and(|config_toml| {
                crate::grok_config::has_proxy_placeholder(config_toml, PROXY_TOKEN_PLACEHOLDER)
            })
    }

    /// 判断给定的 Live/备份配置是否已被代理接管（包含占位符）
    ///
    /// 用途：检测"备份里存的其实是代理配置"这种异常历史状态。
    /// 如果发现，备份不可信，备份路径不能写入（否则会把代理配置固化进备份槽），
    /// 恢复路径不能读取（否则会把代理占位符原样写回 Live，永久卡在代理地址）。
    /// 两种情况下都应该走 SSOT 兜底重建 Live。
    pub(super) fn live_has_proxy_placeholder_for_app(app_type: &AppType, config: &Value) -> bool {
        match app_type {
            AppType::Claude => Self::is_claude_live_taken_over(config),
            AppType::Codex => Self::is_codex_live_taken_over(config),
            AppType::Gemini => Self::is_gemini_live_taken_over(config),
            AppType::GrokBuild => Self::is_grok_live_taken_over(config),
            _ => false,
        }
    }
}
