//! 旧接管路径（`takeoverEngine = "legacy"`，保留一个版本后删除）
//!
//! 三个接管变体按 CCS-154 之前的实现原样保留：统一引擎（[`super::engine`]）出现
//! 回归时，可在设置中切回此路径。代理地址与统一引擎一致，均来自
//! [`ProxyService::takeover_base_url`]。

use super::claude::ClaudeTakeoverAuthPolicy;
use super::*;
use crate::app_config::AppType;
use serde_json::json;

impl ProxyService {
    /// 旧路径使用的 `(代理地址, Codex 基础地址)`
    async fn build_proxy_urls_legacy(&self) -> Result<(String, String), String> {
        let proxy_url = self.takeover_base_url(&AppType::Claude).await?;
        let proxy_codex_base_url = self.takeover_base_url(&AppType::Codex).await?;
        Ok((proxy_url, proxy_codex_base_url))
    }

    /// 接管各应用的 Live 配置（写入代理地址）
    ///
    /// 代理服务器的路由已经根据 API 端点自动区分应用类型：
    /// - `/v1/messages` → Claude
    /// - `/v1/chat/completions`, `/v1/responses` → Codex
    /// - `/v1beta/*` → Gemini
    ///
    /// 因此不需要在 URL 中添加应用前缀。
    pub(in crate::services::proxy) async fn takeover_live_configs_legacy(
        &self,
    ) -> Result<(), String> {
        let (proxy_url, proxy_codex_base_url) = self.build_proxy_urls_legacy().await?;
        let proxy_grok_base_url = self.takeover_base_url(&AppType::GrokBuild).await?;

        // Claude: 修改 ANTHROPIC_BASE_URL，使用占位符替代真实 Token（代理会注入真实 Token）
        if let Ok(mut live_config) = self.read_claude_live() {
            let claude_provider = self.require_current_provider_for_app(&AppType::Claude)?;
            let claude_provider = self.claude_provider_with_effective_settings(&claude_provider)?;
            Self::apply_claude_takeover_fields_for_provider(
                &mut live_config,
                &proxy_url,
                &claude_provider,
            );
            self.write_claude_live(&live_config)?;
            log::info!("Claude Live 配置已接管，代理地址: {proxy_url}");
        }

        // Codex: project the selected provider through the local Responses endpoint.
        if let Ok(mut live_config) = self.read_codex_live() {
            let codex_provider = self.require_current_provider_for_app(&AppType::Codex)?;
            Self::apply_codex_takeover_fields_for_provider(
                &mut live_config,
                &proxy_codex_base_url,
                &codex_provider,
            )?;

            self.write_codex_takeover_live_for_provider(&live_config, Some(&codex_provider))?;
            log::info!("Codex Live 配置已接管，代理地址: {proxy_codex_base_url}");
        }

        // Gemini: 修改 GOOGLE_GEMINI_BASE_URL，使用占位符替代真实 Token（代理会注入真实 Token）
        if let Ok(mut live_config) = self.read_gemini_live() {
            if let Some(env) = live_config.get_mut("env").and_then(|v| v.as_object_mut()) {
                env.insert("GOOGLE_GEMINI_BASE_URL".to_string(), json!(&proxy_url));
                // 使用占位符，避免显示缺少 key 的警告
                env.insert("GEMINI_API_KEY".to_string(), json!(PROXY_TOKEN_PLACEHOLDER));
            } else {
                live_config["env"] = json!({
                    "GOOGLE_GEMINI_BASE_URL": &proxy_url,
                    "GEMINI_API_KEY": PROXY_TOKEN_PLACEHOLDER
                });
            }
            self.write_gemini_live(&live_config)?;
            log::info!("Gemini Live 配置已接管，代理地址: {proxy_url}");
        }

        // Grok Build: keep its own provider namespace while reusing Responses forwarding.
        if let Ok(mut live_config) = self.read_grok_live() {
            Self::apply_grok_takeover_fields(&mut live_config, &proxy_grok_base_url)?;
            self.write_grok_live(&live_config)?;
            log::info!("Grok Build Live 配置已接管，代理地址: {proxy_grok_base_url}");
        }

        Ok(())
    }

    /// 接管指定应用的 Live 配置（严格模式：目标配置不存在则返回错误）
    pub(in crate::services::proxy) async fn takeover_live_config_strict_legacy(
        &self,
        app_type: &AppType,
    ) -> Result<(), String> {
        let (proxy_url, proxy_codex_base_url) = self.build_proxy_urls_legacy().await?;
        let proxy_grok_base_url = self.takeover_base_url(&AppType::GrokBuild).await?;

        match app_type {
            AppType::Claude => {
                let mut live_config = self.read_claude_live()?;
                let claude_provider = self.require_current_provider_for_app(&AppType::Claude)?;
                let claude_provider =
                    self.claude_provider_with_effective_settings(&claude_provider)?;
                Self::apply_claude_takeover_fields_for_provider(
                    &mut live_config,
                    &proxy_url,
                    &claude_provider,
                );
                self.write_claude_live(&live_config)?;
                log::info!("Claude Live 配置已接管，代理地址: {proxy_url}");
            }
            AppType::Codex => {
                let mut live_config = self.read_codex_live()?;
                let codex_provider = self.require_current_provider_for_app(&AppType::Codex)?;
                Self::apply_codex_takeover_fields_for_provider(
                    &mut live_config,
                    &proxy_codex_base_url,
                    &codex_provider,
                )?;

                self.write_codex_takeover_live_for_provider(&live_config, Some(&codex_provider))?;
                log::info!("Codex Live 配置已接管，代理地址: {proxy_codex_base_url}");
            }
            AppType::Gemini => {
                let mut live_config = self.read_gemini_live()?;

                if let Some(env) = live_config.get_mut("env").and_then(|v| v.as_object_mut()) {
                    env.insert("GOOGLE_GEMINI_BASE_URL".to_string(), json!(&proxy_url));
                    env.insert("GEMINI_API_KEY".to_string(), json!(PROXY_TOKEN_PLACEHOLDER));
                } else {
                    live_config["env"] = json!({
                        "GOOGLE_GEMINI_BASE_URL": &proxy_url,
                        "GEMINI_API_KEY": PROXY_TOKEN_PLACEHOLDER
                    });
                }

                self.write_gemini_live(&live_config)?;
                log::info!("Gemini Live 配置已接管，代理地址: {proxy_url}");
            }
            AppType::GrokBuild => {
                let mut live_config = self.read_grok_live()?;
                Self::apply_grok_takeover_fields(&mut live_config, &proxy_grok_base_url)?;
                self.write_grok_live(&live_config)?;
                log::info!("Grok Build Live 配置已接管，代理地址: {proxy_grok_base_url}");
            }
            _ => return Err("该应用不支持代理功能".to_string()),
        }

        Ok(())
    }

    /// 接管指定应用的 Live 配置（尽力而为：配置不存在/读取失败则跳过）
    pub(in crate::services::proxy) async fn takeover_live_config_best_effort_legacy(
        &self,
        app_type: &AppType,
    ) -> Result<(), String> {
        let (proxy_url, proxy_codex_base_url) = self.build_proxy_urls_legacy().await?;
        let proxy_grok_base_url = self.takeover_base_url(&AppType::GrokBuild).await?;

        match app_type {
            AppType::Claude => {
                if let Ok(mut live_config) = self.read_claude_live() {
                    let claude_provider = self
                        .get_current_provider_for_app(&AppType::Claude)
                        .ok()
                        .flatten();
                    if let Some(provider) = claude_provider.as_ref() {
                        let provider = self.claude_provider_with_effective_settings(provider)?;
                        Self::apply_claude_takeover_fields_for_provider(
                            &mut live_config,
                            &proxy_url,
                            &provider,
                        );
                    } else {
                        Self::apply_claude_takeover_fields_with_policy(
                            &mut live_config,
                            &proxy_url,
                            ClaudeTakeoverAuthPolicy::PreserveExistingOrAuthToken,
                        );
                    }
                    let _ = self.write_claude_live(&live_config);
                }
            }
            AppType::Codex => {
                if let Ok(mut live_config) = self.read_codex_live() {
                    let codex_provider = self.require_current_provider_for_app(&AppType::Codex)?;
                    Self::apply_codex_takeover_fields_for_provider(
                        &mut live_config,
                        &proxy_codex_base_url,
                        &codex_provider,
                    )?;

                    self.write_codex_takeover_live_for_provider(
                        &live_config,
                        Some(&codex_provider),
                    )?;
                }
            }
            AppType::Gemini => {
                if let Ok(mut live_config) = self.read_gemini_live() {
                    if let Some(env) = live_config.get_mut("env").and_then(|v| v.as_object_mut()) {
                        env.insert("GOOGLE_GEMINI_BASE_URL".to_string(), json!(&proxy_url));
                        env.insert("GEMINI_API_KEY".to_string(), json!(PROXY_TOKEN_PLACEHOLDER));
                    } else {
                        live_config["env"] = json!({
                            "GOOGLE_GEMINI_BASE_URL": &proxy_url,
                            "GEMINI_API_KEY": PROXY_TOKEN_PLACEHOLDER
                        });
                    }

                    let _ = self.write_gemini_live(&live_config);
                }
            }
            AppType::GrokBuild => {
                if let Ok(mut live_config) = self.read_grok_live() {
                    Self::apply_grok_takeover_fields(&mut live_config, &proxy_grok_base_url)?;
                    let _ = self.write_grok_live(&live_config);
                }
            }
            _ => {}
        }

        Ok(())
    }
}
