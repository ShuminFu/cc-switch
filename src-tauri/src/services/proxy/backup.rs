//! 接管前 Live 配置备份与停止接管时的恢复

use super::*;
use crate::app_config::AppType;
use crate::provider::Provider;
use crate::services::provider::{
    build_effective_settings_with_common_config, write_live_with_common_config,
};
use crate::services::takeover_snapshot::RawLiveSnapshot;
use serde_json::{json, Value};
use std::str::FromStr;

impl ProxyService {
    /// 备份各应用的 Live 配置
    pub(in crate::services::proxy) async fn backup_live_configs(&self) -> Result<(), String> {
        for descriptor in crate::apps::registry::with_caps(crate::apps::AppCaps::PROXY_TAKEOVER) {
            let Ok(config) = self.read_live_for_takeover(&descriptor.app) else {
                continue;
            };
            // 跳过已被代理接管的 Live：避免把代理占位符当作"原始 Live"存进备份槽。
            // 否则下次 start_with_takeover 在异常历史状态下（Live 已是占位符）再次
            // 调用本函数，会用代理配置覆盖一个原本正常的备份；之后 stop 恢复时
            // 即便走到备份路径也会把代理占位符再写回 Live，永久卡在 127.0.0.1:15721。
            if Self::live_has_proxy_placeholder_for_app(&descriptor.app, &config) {
                log::warn!(
                    "{} Live 已被代理接管，不备份（避免把代理配置固化进备份槽）；下次 stop 会从 SSOT 重建 Live",
                    descriptor.slug
                );
                continue;
            }
            self.save_takeover_backup(&descriptor.app, &config)
                .await
                .map_err(|e| format!("备份 {} 配置失败: {e}", descriptor.display_name))?;
        }

        log::info!("已备份所有应用的 Live 配置");
        Ok(())
    }

    /// 备份指定应用的 Live 配置（严格模式：目标配置不存在则返回错误）
    pub(in crate::services::proxy) async fn backup_live_config_strict(
        &self,
        app_type: &AppType,
    ) -> Result<(), String> {
        let app_type_str = app_type.as_str();
        let config = self.read_live_for_takeover(app_type)?;

        // 跳过已被代理接管的 Live：避免把代理占位符当作"原始 Live"存进备份槽
        // （见 backup_live_configs 中的注释）。
        if Self::live_has_proxy_placeholder_for_app(app_type, &config) {
            log::warn!(
                "{app_type_str} Live 已被代理接管，不备份（避免把代理配置固化进备份槽）；下次 stop 会从 SSOT 重建 Live"
            );
            return Ok(());
        }

        self.save_takeover_backup(app_type, &config)
            .await
            .map_err(|e| format!("备份 {app_type_str} 配置失败: {e}"))?;

        Ok(())
    }

    /// 保存接管前的 Live 备份；`unified` 引擎还会附上 live 文件的原始字节快照
    ///（CCS-152），恢复时据此逐字节还原。
    ///
    /// 快照只在原文与刚读到的 `config` 一致时记录：读完 `config` 后若 live 又被
    /// 改写，快照与备份不再对应，宁可不记录（恢复回退为按备份 JSON 写入）。
    pub(in crate::services::proxy) async fn save_takeover_backup(
        &self,
        app_type: &AppType,
        config: &Value,
    ) -> Result<(), String> {
        let app_type_str = app_type.as_str();
        let json_str = serde_json::to_string(config)
            .map_err(|e| format!("序列化 {app_type_str} 配置失败: {e}"))?;
        self.db
            .save_live_backup(app_type_str, &json_str)
            .await
            .map_err(|e| e.to_string())?;

        if !crate::settings::takeover_engine().uses_raw_snapshot() {
            return Ok(());
        }
        let Some(snapshot) = RawLiveSnapshot::capture(app_type) else {
            return Ok(());
        };
        match self.read_live_for_takeover(app_type) {
            Ok(current) if current == *config => {}
            _ => {
                log::warn!("{app_type_str} Live 在备份期间发生变化，不记录接管快照");
                return Ok(());
            }
        }
        // 快照只影响恢复后的格式：序列化失败（如路径不是 UTF-8）同样不阻止接管
        let raw = match snapshot.to_json() {
            Ok(raw) => raw,
            Err(e) => {
                log::warn!(
                    "{app_type_str} 接管快照无法序列化，不记录（恢复将按备份 JSON 写入）: {e}"
                );
                return Ok(());
            }
        };
        if let Err(e) = self
            .db
            .save_live_backup_raw_snapshot(app_type_str, &raw)
            .await
        {
            // 快照只影响恢复后的格式，保存失败不阻止接管
            log::warn!("保存 {app_type_str} 接管快照失败（恢复将按备份 JSON 写入）: {e}");
        }
        Ok(())
    }

    /// 按备份写回 Live 之后，用接管快照逐字节还原 live 文件（CCS-152）。
    ///
    /// 仅当备份行仍带快照（即备份未被热切换等路径重写）且引擎为 `unified` 时生效；
    /// 失败只记录警告：此时 live 已按备份写好，只是格式可能与接管前不同。
    pub(in crate::services::proxy) async fn restore_takeover_raw_snapshot(
        &self,
        app_type: &AppType,
    ) {
        if !crate::settings::takeover_engine().uses_raw_snapshot() {
            return;
        }
        let app_type_str = app_type.as_str();
        let raw = match self.db.get_live_backup_raw_snapshot(app_type_str).await {
            Ok(Some(raw)) => raw,
            Ok(None) => return,
            Err(e) => {
                log::warn!("读取 {app_type_str} 接管快照失败: {e}");
                return;
            }
        };
        match RawLiveSnapshot::from_json(&raw).and_then(|snapshot| snapshot.restore(app_type)) {
            Ok(()) => log::info!("{app_type_str} Live 文件已按接管快照逐字节还原"),
            Err(e) => log::warn!("{app_type_str} 接管快照还原失败（保留按备份写入的结果）: {e}"),
        }
    }

    pub(in crate::services::proxy) async fn restore_live_config_for_app_inner(
        &self,
        app_type: &AppType,
    ) -> Result<(), String> {
        let descriptor = crate::apps::registry::descriptor(app_type);
        if !descriptor.has(crate::apps::AppCaps::PROXY_TAKEOVER) {
            return Ok(());
        }
        let display_name = descriptor.display_name;
        if let Ok(Some(backup)) = self.db.get_live_backup(descriptor.slug).await {
            let config: Value = serde_json::from_str(&backup.original_config)
                .map_err(|e| format!("解析 {display_name} 备份失败: {e}"))?;
            self.write_live_config_for_app(app_type, &config)?;
            self.restore_takeover_raw_snapshot(app_type).await;
            log::info!("{display_name} Live 配置已恢复");
        }

        Ok(())
    }

    /// 恢复原始 Live 配置
    pub(in crate::services::proxy) async fn restore_live_configs(&self) -> Result<(), String> {
        let mut errors = Vec::new();

        for app_type in
            crate::apps::registry::with_caps(crate::apps::AppCaps::PROXY_TAKEOVER).map(|d| &d.app)
        {
            if let Err(e) = self
                .restore_live_config_for_app_with_fallback(app_type)
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

    pub(in crate::services::proxy) async fn restore_live_config_for_app_with_fallback(
        &self,
        app_type: &AppType,
    ) -> Result<(), String> {
        let _guard = self.switch_locks.lock_for_app(app_type.as_str()).await;
        self.restore_live_config_for_app_with_fallback_inner(app_type)
            .await
    }

    /// 修复 Live 中残留的接管占位符（漂移检测的 reconcile 动作）。
    ///
    /// 走与关闭接管相同的 备份 → SSOT → 清理占位符 三层兜底，然后删除该应用的
    /// 备份（避免长期存储敏感 Token）。`proxy_config.enabled` 保持不变：与
    /// `stop_with_restore_keep_state` 一致，下次启动代理时仍会重新接管。
    pub async fn repair_takeover_leftover_for_app(&self, app_type: &AppType) -> Result<(), String> {
        let app_type_str = app_type.as_str();
        let _guard = self.switch_locks.lock_for_app(app_type_str).await;
        self.restore_live_config_for_app_with_fallback_inner(app_type)
            .await?;
        self.db
            .delete_live_backup(app_type_str)
            .await
            .map_err(|e| format!("删除 {app_type_str} Live 备份失败: {e}"))?;
        log::info!("{app_type_str} Live 残留接管占位符已修复");
        Ok(())
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
                self.restore_takeover_raw_snapshot(app_type).await;
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

    /// 当 Live 备份缺失时，尝试用 SSOT（当前供应商）写回 Live，以解除占位符接管。
    ///
    /// 返回值：
    /// - Ok(true)：已成功写回
    /// - Ok(false)：缺少当前供应商/供应商不存在/供应商本身含占位符，无法写回
    pub(in crate::services::proxy) fn restore_live_from_ssot_for_app(
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

    /// 从供应商配置更新 Live 备份（用于代理模式下的热切换）
    ///
    /// 与 backup_live_configs() 不同，此方法从供应商的 settings_config 生成备份，
    /// 而不是从 Live 文件读取（因为 Live 文件已被代理接管）。
    pub async fn update_live_backup_from_provider(
        &self,
        app_type: &str,
        provider: &Provider,
    ) -> Result<(), String> {
        let _guard = self.switch_locks.lock_for_app(app_type).await;
        self.update_live_backup_from_provider_inner(app_type, provider)
            .await
    }

    /// 仅供已持有 per-app 切换锁的调用方使用。
    pub(in crate::services::proxy) async fn update_live_backup_from_provider_inner(
        &self,
        app_type: &str,
        provider: &Provider,
    ) -> Result<(), String> {
        let app_type_enum =
            AppType::from_str(app_type).map_err(|_| format!("未知的应用类型: {app_type}"))?;
        let mut effective_settings =
            build_effective_settings_with_common_config(self.db.as_ref(), &app_type_enum, provider)
                .map_err(|e| format!("构建 {app_type} 有效配置失败: {e}"))?;

        if matches!(app_type_enum, AppType::Codex) {
            let existing_backup_value = self
                .db
                .get_live_backup(app_type)
                .await
                .map_err(|e| format!("读取 {app_type} 现有备份失败: {e}"))?
                .map(|backup| {
                    serde_json::from_str::<Value>(&backup.original_config)
                        .map_err(|e| format!("解析 {app_type} 现有备份失败: {e}"))
                })
                .transpose()?;
            // A stale takeover marker can survive without its DB backup (for
            // example after a partial cleanup). In that abnormal state the live
            // auth file is still the only copy of the user's login, so use it as
            // the preservation source instead of replacing it with the empty
            // official seed snapshot.
            let existing_backup_value =
                existing_backup_value.or_else(|| self.read_codex_live().ok());

            if let Some(existing_value) = existing_backup_value.as_ref() {
                Self::preserve_toml_mcp_servers_from_existing_config(
                    &mut effective_settings,
                    existing_value,
                )?;
                Self::preserve_codex_auth_in_backup(
                    &mut effective_settings,
                    existing_value,
                    crate::proxy::providers::is_codex_official_provider(provider),
                )?;
            }

            // 统一会话开关：备份是接管释放时恢复 live 的来源，官方配置的
            // 共享 custom 路由注入必须落在备份里，否则恢复后开关失效。
            crate::codex_config::apply_codex_unified_session_bucket_to_settings(
                provider.category.as_deref(),
                &mut effective_settings,
            )
            .map_err(|e| format!("注入统一会话路由失败: {e}"))?;
        }

        if matches!(app_type_enum, AppType::GrokBuild) {
            let existing_value = self
                .db
                .get_live_backup(app_type)
                .await
                .map_err(|e| format!("读取 {app_type} 现有备份失败: {e}"))?
                .map(|backup| {
                    serde_json::from_str::<Value>(&backup.original_config)
                        .map_err(|e| format!("解析 {app_type} 现有备份失败: {e}"))
                })
                .transpose()?
                .or_else(|| self.read_grok_live().ok());
            if let Some(existing_value) = existing_value.as_ref() {
                Self::preserve_toml_mcp_servers_from_existing_config(
                    &mut effective_settings,
                    existing_value,
                )?;
            }
        }

        let backup_json = match app_type_enum {
            AppType::Claude => serde_json::to_string(&effective_settings)
                .map_err(|e| format!("序列化 Claude 配置失败: {e}"))?,
            AppType::Codex => serde_json::to_string(&effective_settings)
                .map_err(|e| format!("序列化 Codex 配置失败: {e}"))?,
            AppType::GrokBuild => serde_json::to_string(&effective_settings)
                .map_err(|e| format!("序列化 Grok Build 配置失败: {e}"))?,
            AppType::Gemini => {
                // Gemini takeover 仅修改 .env；settings.json（含 mcpServers）保持原样。
                let env_backup = if let Some(env) = effective_settings.get("env") {
                    json!({ "env": env })
                } else {
                    json!({ "env": {} })
                };
                serde_json::to_string(&env_backup)
                    .map_err(|e| format!("序列化 Gemini 配置失败: {e}"))?
            }
            _ => return Err(format!("未知的应用类型: {app_type}")),
        };

        self.db
            .save_live_backup(app_type, &backup_json)
            .await
            .map_err(|e| format!("更新 {app_type} 备份失败: {e}"))?;

        log::info!("已更新 {app_type} Live 备份（热切换）");
        Ok(())
    }

    pub(in crate::services::proxy) fn preserve_toml_mcp_servers_from_existing_config(
        target_settings: &mut Value,
        existing_config: &Value,
    ) -> Result<(), String> {
        let target_obj = target_settings
            .as_object_mut()
            .ok_or_else(|| "TOML 应用备份必须是 JSON 对象".to_string())?;

        let target_config = target_obj
            .get("config")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let mut target_doc = if target_config.trim().is_empty() {
            toml_edit::DocumentMut::new()
        } else {
            target_config
                .parse::<toml_edit::DocumentMut>()
                .map_err(|e| format!("解析新的 config.toml 失败: {e}"))?
        };

        let existing_config = existing_config
            .get("config")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if existing_config.trim().is_empty() {
            target_obj.insert("config".to_string(), json!(target_doc.to_string()));
            return Ok(());
        }

        let existing_doc = existing_config
            .parse::<toml_edit::DocumentMut>()
            .map_err(|e| format!("解析现有 config.toml 备份失败: {e}"))?;

        if let Some(existing_mcp_servers) = existing_doc.get("mcp_servers") {
            match target_doc.get_mut("mcp_servers") {
                Some(target_mcp_servers) => {
                    if let (Some(target_table), Some(existing_table)) = (
                        target_mcp_servers.as_table_like_mut(),
                        existing_mcp_servers.as_table_like(),
                    ) {
                        for (server_id, server_item) in existing_table.iter() {
                            if target_table.get(server_id).is_none() {
                                target_table.insert(server_id, server_item.clone());
                            }
                        }
                    } else {
                        log::warn!(
                            "config.toml contains a non-table mcp_servers section; skipping MCP merge"
                        );
                    }
                }
                None => {
                    target_doc["mcp_servers"] = existing_mcp_servers.clone();
                }
            }
        }

        target_obj.insert("config".to_string(), json!(target_doc.to_string()));
        Ok(())
    }

    pub(in crate::services::proxy) fn preserve_codex_auth_in_backup(
        target_settings: &mut Value,
        existing_backup: &Value,
        preserve_api_key: bool,
    ) -> Result<(), String> {
        let Some(existing_auth) = existing_backup
            .get("auth")
            .filter(|auth| {
                !Self::codex_auth_has_proxy_placeholder(auth)
                    && (crate::codex_config::codex_auth_has_oauth_login_material(auth)
                        || (preserve_api_key
                            && crate::codex_config::codex_auth_has_login_material(auth)))
            })
            .cloned()
        else {
            return Ok(());
        };

        let Some(target_obj) = target_settings.as_object_mut() else {
            return Ok(());
        };

        let provider_auth = target_obj.get("auth").cloned().unwrap_or_else(|| json!({}));
        if let Some(config_text) = target_obj.get("config").and_then(|value| value.as_str()) {
            let live_config = crate::codex_config::prepare_codex_provider_live_config(
                &provider_auth,
                config_text,
            )
            .map_err(|e| format!("更新 Codex 备份配置失败: {e}"))?;
            target_obj.insert("config".to_string(), json!(live_config));
        }
        target_obj.insert("auth".to_string(), existing_auth);

        Ok(())
    }
}
