//! 代理运行期间的供应商热切换与 Live 同步

use super::*;
use crate::app_config::AppType;
use crate::provider::Provider;
use crate::services::provider::build_effective_settings_with_common_config;
use serde_json::Value;
use std::str::FromStr;

impl ProxyService {
    pub async fn sync_claude_live_from_provider_while_proxy_active(
        &self,
        provider: &Provider,
    ) -> Result<(), String> {
        let effective_provider = self.claude_provider_with_effective_settings(provider)?;
        let mut effective_settings = effective_provider.settings_config.clone();
        let proxy_url = self.takeover_base_url(&AppType::Claude).await?;

        Self::apply_claude_takeover_fields_for_provider(
            &mut effective_settings,
            &proxy_url,
            &effective_provider,
        );
        self.write_claude_live(&effective_settings)?;
        Ok(())
    }

    /// 代理运行期间保存当前供应商后，刷新被接管的 Live。
    ///
    /// Claude 总是刷新；Codex 只在 Live 已被接管时刷新（模型映射投影在生成的
    /// model_catalog_json 中，不刷新会让旧的目录指针与能力继续生效）。
    pub async fn refresh_live_after_provider_save(
        &self,
        app_type: &AppType,
        provider: &Provider,
        live_taken_over: bool,
    ) -> Result<(), crate::error::AppError> {
        match app_type {
            AppType::Claude => self
                .sync_claude_live_from_provider_while_proxy_active(provider)
                .await
                .map_err(|e| {
                    crate::error::AppError::Message(format!("同步 Claude Live 配置失败: {e}"))
                }),
            AppType::Codex if live_taken_over => self
                .sync_codex_live_from_provider_while_proxy_active(provider)
                .await
                .map_err(|e| {
                    crate::error::AppError::Message(format!("同步 Codex Live 配置失败: {e}"))
                }),
            _ => Ok(()),
        }
    }

    pub async fn sync_codex_live_from_provider_while_proxy_active(
        &self,
        provider: &Provider,
    ) -> Result<(), String> {
        let existing_live = self.read_codex_live().ok();
        let mut effective_settings = build_effective_settings_with_common_config(
            self.db.as_ref(),
            &AppType::Codex,
            provider,
        )
        .map_err(|e| format!("构建 codex 有效配置失败: {e}"))?;
        if let Some(existing_live) = existing_live.as_ref() {
            Self::preserve_toml_mcp_servers_from_existing_config(
                &mut effective_settings,
                existing_live,
            )?;
        }
        let proxy_codex_base_url = self.takeover_base_url(&AppType::Codex).await?;

        Self::apply_codex_takeover_fields_for_provider(
            &mut effective_settings,
            &proxy_codex_base_url,
            provider,
        )?;

        self.write_codex_takeover_live_for_provider(&effective_settings, Some(provider))?;
        Ok(())
    }

    pub async fn sync_grok_live_from_provider_while_proxy_active(
        &self,
        provider: &Provider,
    ) -> Result<(), String> {
        let existing_live = self.read_grok_live().ok();
        let mut effective_settings = build_effective_settings_with_common_config(
            self.db.as_ref(),
            &AppType::GrokBuild,
            provider,
        )
        .map_err(|e| format!("构建 Grok Build 有效配置失败: {e}"))?;
        if let Some(existing_live) = existing_live.as_ref() {
            Self::preserve_toml_mcp_servers_from_existing_config(
                &mut effective_settings,
                existing_live,
            )?;
        }
        let proxy_grok_base_url = self.takeover_base_url(&AppType::GrokBuild).await?;
        Self::apply_grok_takeover_fields(&mut effective_settings, &proxy_grok_base_url)?;
        self.write_grok_live(&effective_settings)
    }

    pub(in crate::services::proxy) async fn refresh_active_target_from_current_provider(
        &self,
        app_type: &AppType,
    ) {
        let Ok(Some(provider)) = self.get_current_provider_for_app(app_type) else {
            return;
        };
        if let Some(server) = self.server.read().await.as_ref() {
            server
                .set_active_target(app_type.as_str(), &provider.id, &provider.name)
                .await;
        }
    }

    pub(in crate::services::proxy) async fn rollback_hot_switch_preparation(
        &self,
        app_type: &AppType,
        previous_backup: Option<&LiveBackup>,
        previous_provider_id: Option<&str>,
        should_sync_backup: bool,
        live_taken_over: bool,
        previous_live_before_direct_write: Option<&Value>,
    ) {
        if !should_sync_backup {
            return;
        }

        let rollback_result = match previous_backup {
            Some(backup) => {
                self.db
                    .save_live_backup(app_type.as_str(), &backup.original_config)
                    .await
            }
            None => self.db.delete_live_backup(app_type.as_str()).await,
        };
        if let Err(error) = rollback_result {
            log::error!("{} 热切换失败后恢复原备份失败: {error}", app_type.as_str());
        }

        if let Some(previous_live) = previous_live_before_direct_write {
            if let Err(error) = self.write_codex_live_verbatim(previous_live) {
                log::error!(
                    "{} 热切换失败后恢复直接写入前的 Live 配置失败: {error}",
                    app_type.as_str()
                );
            }
            return;
        }

        let Some(previous_provider_id) = previous_provider_id else {
            return;
        };
        let Ok(Some(previous_provider)) = self
            .db
            .get_provider_by_id(previous_provider_id, app_type.as_str())
        else {
            return;
        };

        let live_result = if matches!(app_type, AppType::Claude) {
            self.sync_claude_live_from_provider_while_proxy_active(&previous_provider)
                .await
        } else if live_taken_over && matches!(app_type, AppType::Codex) {
            self.sync_codex_live_from_provider_while_proxy_active(&previous_provider)
                .await
        } else if live_taken_over && matches!(app_type, AppType::GrokBuild) {
            self.sync_grok_live_from_provider_while_proxy_active(&previous_provider)
                .await
        } else {
            Ok(())
        };
        if let Err(error) = live_result {
            log::error!(
                "{} 热切换失败后恢复原 Live 配置失败: {error}",
                app_type.as_str()
            );
        }
    }

    pub async fn hot_switch_provider(
        &self,
        app_type: &str,
        provider_id: &str,
    ) -> Result<HotSwitchOutcome, String> {
        let _guard = self.switch_locks.lock_for_app(app_type).await;
        self.hot_switch_provider_inner(app_type, provider_id).await
    }

    pub(crate) async fn hot_switch_provider_inner(
        &self,
        app_type: &str,
        provider_id: &str,
    ) -> Result<HotSwitchOutcome, String> {
        let app_type_enum =
            AppType::from_str(app_type).map_err(|_| format!("无效的应用类型: {app_type}"))?;
        let provider = self
            .db
            .get_provider_by_id(provider_id, app_type)
            .map_err(|e| format!("读取供应商失败: {e}"))?
            .ok_or_else(|| format!("供应商不存在: {provider_id}"))?;

        // Defense-in-depth: only the built-in Codex official provider supports
        // native OpenAI auth passthrough during takeover.
        if provider.category.as_deref() == Some("official")
            && !crate::services::provider::official_provider_supports_proxy_takeover(
                &app_type_enum,
                &provider,
            )
        {
            return Err(
                "代理接管模式下不能切换到官方供应商 (Cannot switch to official provider during proxy takeover)"
                    .to_string(),
            );
        }

        let previous_provider_id =
            crate::settings::get_effective_current_provider(&self.db, &app_type_enum)
                .map_err(|e| format!("读取当前供应商失败: {e}"))?;
        let previous_local_provider_id = crate::settings::get_current_provider(&app_type_enum);
        let logical_target_changed = previous_provider_id.as_deref() != Some(provider_id);

        let has_backup = self
            .db
            .get_live_backup(app_type_enum.as_str())
            .await
            .map_err(|e| format!("读取 {app_type} 备份失败: {e}"))?
            .is_some();
        let live_taken_over = self.detect_takeover_in_live_config_for_app(&app_type_enum);
        let should_sync_backup = has_backup || live_taken_over;

        // All fallible backup/live writes must finish before committing the logical
        // current provider. Otherwise a failed hot switch leaves the UI pointing at
        // the new provider while the proxy still serves the old one (and the next
        // query may fall back to the first provider).
        let previous_backup = if should_sync_backup {
            self.db
                .get_live_backup(app_type_enum.as_str())
                .await
                .map_err(|e| format!("读取 {app_type} 原备份失败: {e}"))?
        } else {
            None
        };
        let previous_live_before_direct_write =
            if has_backup && !live_taken_over && matches!(app_type_enum, AppType::Codex) {
                Some(
                    self.read_codex_live()
                        .map_err(|error| format!("读取 Codex 原 Live 配置失败: {error}"))?,
                )
            } else {
                None
            };

        let prepare_result: Result<(), String> = async {
            if should_sync_backup {
                self.update_live_backup_from_provider_inner(app_type, &provider)
                    .await?;

                if matches!(app_type_enum, AppType::Claude) {
                    self.sync_claude_live_from_provider_while_proxy_active(&provider)
                        .await?;
                } else if live_taken_over && matches!(app_type_enum, AppType::Codex) {
                    self.sync_codex_live_from_provider_while_proxy_active(&provider)
                        .await?;
                } else if live_taken_over && matches!(app_type_enum, AppType::GrokBuild) {
                    self.sync_grok_live_from_provider_while_proxy_active(&provider)
                        .await?;
                }
            }

            if has_backup && !live_taken_over && matches!(app_type_enum, AppType::Codex) {
                let effective_settings = build_effective_settings_with_common_config(
                    self.db.as_ref(),
                    &AppType::Codex,
                    &provider,
                )
                .map_err(|e| format!("构建 Codex 有效配置失败: {e}"))?;
                let auth = effective_settings
                    .get("auth")
                    .ok_or_else(|| "Codex 供应商缺少 auth 配置".to_string())?;
                let config_str = effective_settings.get("config").and_then(|v| v.as_str());
                let profile =
                    crate::proxy::providers::resolve_codex_catalog_tool_profile(&provider);

                crate::codex_config::write_codex_provider_live_with_catalog(
                    &effective_settings,
                    provider.category.as_deref(),
                    auth,
                    config_str,
                    profile,
                )
                .map_err(|e| format!("写入 Codex 配置失败: {e}"))?;
            }

            Ok(())
        }
        .await;

        if let Err(error) = prepare_result {
            self.rollback_hot_switch_preparation(
                &app_type_enum,
                previous_backup.as_ref(),
                previous_provider_id.as_deref(),
                should_sync_backup,
                live_taken_over,
                previous_live_before_direct_write.as_ref(),
            )
            .await;
            return Err(error);
        }

        if let Err(error) = crate::settings::set_current_provider(&app_type_enum, Some(provider_id))
        {
            self.rollback_hot_switch_preparation(
                &app_type_enum,
                previous_backup.as_ref(),
                previous_provider_id.as_deref(),
                should_sync_backup,
                live_taken_over,
                previous_live_before_direct_write.as_ref(),
            )
            .await;
            return Err(format!("更新本地当前供应商失败: {error}"));
        }
        if let Err(error) = self
            .db
            .set_current_provider(app_type_enum.as_str(), provider_id)
        {
            if let Err(rollback_error) = crate::settings::set_current_provider(
                &app_type_enum,
                previous_local_provider_id.as_deref(),
            ) {
                log::error!("数据库切换失败后恢复本地当前供应商失败: {rollback_error}");
            }
            self.rollback_hot_switch_preparation(
                &app_type_enum,
                previous_backup.as_ref(),
                previous_provider_id.as_deref(),
                should_sync_backup,
                live_taken_over,
                previous_live_before_direct_write.as_ref(),
            )
            .await;
            return Err(format!("更新当前供应商失败: {error}"));
        }

        if let Some(server) = self.server.read().await.as_ref() {
            server
                .set_active_target(app_type_enum.as_str(), &provider.id, &provider.name)
                .await;
        }

        Ok(HotSwitchOutcome {
            logical_target_changed,
        })
    }

    /// 代理模式下切换供应商（热切换，并按需刷新代理安全的 Live 显示字段）
    pub async fn switch_proxy_target(
        &self,
        app_type: &str,
        provider_id: &str,
    ) -> Result<(), String> {
        let outcome = self.hot_switch_provider(app_type, provider_id).await?;

        if outcome.logical_target_changed {
            log::info!("代理模式：已切换 {app_type} 的目标供应商为 {provider_id}");
        } else {
            log::debug!("代理模式：{app_type} 已对齐到目标供应商 {provider_id}");
        }
        Ok(())
    }
}
