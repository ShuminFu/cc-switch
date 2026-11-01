//! 代理服务业务逻辑层
//!
//! 提供代理服务器的启动、停止和配置管理

use crate::app_config::AppType;
use crate::config::{get_claude_settings_path, read_json_file, write_json_file};
use crate::database::Database;
use crate::provider::Provider;
use crate::proxy::server::ProxyServer;
use crate::proxy::switch_lock::SwitchLockManager;
use crate::proxy::types::*;
use crate::services::provider::{
    build_effective_settings_with_common_config, write_live_with_common_config,
};
use serde_json::{json, Map, Value};
use std::str::FromStr;
use std::sync::Arc;
use tauri::Emitter;
use tokio::sync::RwLock;

/// 用于接管 Live 配置时的占位符（避免客户端提示缺少 key，同时不泄露真实 Token）
mod claude_fields;
mod error;
mod hot_switch;
mod live_io;
mod restore;
mod takeover;

pub use error::{TakeoverError, TakeoverErrorKind};

const PROXY_TOKEN_PLACEHOLDER: &str = "PROXY_MANAGED";

/// 代理接管模式下需要从 Claude Live 配置中移除的"模型覆盖"字段。
///
/// 原因：接管模式下 `*_MODEL` 必须由 CC Switch 写成稳定的 Claude 角色别名，
/// 再由本地代理映射到当前供应商真实模型；`*_MODEL_NAME` 也需要同步接管，
/// 否则 Claude Code 模型菜单会残留上一个供应商的显示名称。
const CLAUDE_MODEL_OVERRIDE_ENV_KEYS: [&str; 12] = [
    "ANTHROPIC_MODEL",
    "ANTHROPIC_REASONING_MODEL", // legacy: 已废弃，但旧配置可能残留
    "ANTHROPIC_DEFAULT_HAIKU_MODEL",
    "ANTHROPIC_DEFAULT_HAIKU_MODEL_NAME",
    "ANTHROPIC_DEFAULT_SONNET_MODEL",
    "ANTHROPIC_DEFAULT_SONNET_MODEL_NAME",
    "ANTHROPIC_DEFAULT_OPUS_MODEL",
    "ANTHROPIC_DEFAULT_OPUS_MODEL_NAME",
    "ANTHROPIC_DEFAULT_FABLE_MODEL",
    "ANTHROPIC_DEFAULT_FABLE_MODEL_NAME",
    "ANTHROPIC_SMALL_FAST_MODEL", // Legacy key (已废弃)：历史版本使用该字段区分 small/fast 模型
    "CLAUDE_CODE_SUBAGENT_MODEL",
];

const CLAUDE_TAKEOVER_HAIKU_MODEL: &str = "claude-haiku-4-5";
const CLAUDE_TAKEOVER_SONNET_MODEL: &str = "claude-sonnet-4-6";
const CLAUDE_TAKEOVER_OPUS_MODEL: &str = "claude-opus-4-8";
const CLAUDE_TAKEOVER_FABLE_MODEL: &str = "claude-fable-5";
// 写给 Claude Code 时沿用文档示例的大写形式；解析侧大小写不敏感。
const CLAUDE_ONE_M_MARKER_FOR_CLIENT: &str = "[1M]";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClaudeTakeoverAuthPolicy {
    PreserveExistingOrAuthToken,
    ManagedAccount { keep_auth_token: bool },
}

#[derive(Clone)]
pub struct ProxyService {
    db: Arc<Database>,
    server: Arc<RwLock<Option<ProxyServer>>>,
    /// AppHandle，用于传递给 ProxyServer 以支持故障转移时的 UI 更新
    app_handle: Arc<RwLock<Option<tauri::AppHandle>>>,
    switch_locks: SwitchLockManager,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct HotSwitchOutcome {
    pub logical_target_changed: bool,
}

impl ProxyService {
    pub fn new(db: Arc<Database>) -> Self {
        Self {
            db,
            server: Arc::new(RwLock::new(None)),
            app_handle: Arc::new(RwLock::new(None)),
            switch_locks: SwitchLockManager::new(),
        }
    }

    /// 设置 AppHandle（在应用初始化时调用）
    pub fn set_app_handle(&self, handle: tauri::AppHandle) {
        crate::services::provider::block_on_async(async {
            *self.app_handle.write().await = Some(handle);
        });
    }

    pub(crate) async fn lock_switch_for_app(
        &self,
        app_type: &str,
    ) -> tokio::sync::OwnedMutexGuard<()> {
        self.switch_locks.lock_for_app(app_type).await
    }

    /// 启动代理服务器
    pub async fn start(&self) -> Result<ProxyServerInfo, TakeoverError> {
        // 1. 启动时自动设置 proxy_enabled = true
        let mut global_config = self
            .db
            .get_global_proxy_config()
            .await
            .map_err(|e| format!("获取全局代理配置失败: {e}"))?;

        if !global_config.proxy_enabled {
            global_config.proxy_enabled = true;
            self.db
                .update_global_proxy_config(global_config.clone())
                .await
                .map_err(|e| format!("更新代理总开关失败: {e}"))?;
        }

        // 2. 获取配置
        let config = self
            .db
            .get_proxy_config()
            .await
            .map_err(|e| format!("获取代理配置失败: {e}"))?;

        // 3. 若已在运行：确保持久化状态（如需要）并返回当前信息
        if let Some(server) = self.server.read().await.as_ref() {
            let status = server.get_status().await;
            return Ok(ProxyServerInfo {
                address: status.address,
                port: status.port,
                // 无法精确取回首次启动时间，返回当前时间用于 UI 展示即可
                started_at: chrono::Utc::now().to_rfc3339(),
            });
        }

        // 4. 创建并启动服务器
        let app_handle = self.app_handle.read().await.clone();
        let server = ProxyServer::new(config.clone(), self.db.clone(), app_handle);
        let info = server.start().await.map_err(TakeoverError::with(
            TakeoverErrorKind::StartFailed,
            "启动代理服务器失败",
        ))?;
        if let Err(e) = self
            .persist_ephemeral_listen_port_if_needed(&config, info.port)
            .await
        {
            let _ = server.stop().await;
            return Err(e);
        }

        // 5. 保存服务器实例
        *self.server.write().await = Some(server);

        log::info!("代理服务器已启动: {}:{}", info.address, info.port);
        Ok(info)
    }

    async fn persist_ephemeral_listen_port_if_needed(
        &self,
        config: &ProxyConfig,
        actual_port: u16,
    ) -> Result<(), TakeoverError> {
        if config.listen_port != 0 {
            return Ok(());
        }

        let mut resolved_config = config.clone();
        resolved_config.listen_port = actual_port;
        self.db
            .update_proxy_config(resolved_config)
            .await
            .map_err(TakeoverError::with(
                TakeoverErrorKind::Config,
                "保存动态代理端口失败",
            ))
    }

    /// 停止代理服务器
    pub async fn stop(&self) -> Result<(), TakeoverError> {
        let mut slot = self.server.write().await;
        let Some(server) = slot.as_ref() else {
            return Err(TakeoverError::new(
                TakeoverErrorKind::NotRunning,
                "代理服务器未运行",
            ));
        };
        // SVC-50: the handle stays in the slot until `ProxyServer::stop` has
        // returned (it aborts the serve task itself after the drain timeout),
        // and `proxy_enabled` is cleared whatever the outcome, so the DB never
        // claims a proxy that the UI shows as stopped.
        let stop_result = server.stop().await.map_err(TakeoverError::with(
            TakeoverErrorKind::StopFailed,
            "停止代理服务器失败",
        ));
        slot.take();
        drop(slot);

        match self.db.get_global_proxy_config().await {
            Ok(mut global_config) if global_config.proxy_enabled => {
                global_config.proxy_enabled = false;
                if let Err(e) = self.db.update_global_proxy_config(global_config).await {
                    log::warn!("更新代理总开关失败: {e}");
                }
            }
            Ok(_) => {}
            Err(e) => log::warn!("获取全局代理配置失败: {e}"),
        }

        stop_result?;
        log::info!("代理服务器已停止");
        Ok(())
    }

    /// 停止代理服务器（恢复 Live 配置，用户手动关闭时使用）
    ///
    /// 会清除 settings 表中的代理状态，下次启动不会自动恢复。
    pub async fn stop_with_restore(&self) -> Result<(), TakeoverError> {
        // 1. 停止代理服务器（即使未运行也继续执行恢复逻辑）
        if let Err(e) = self.stop().await {
            log::warn!("停止代理服务器失败（将继续恢复 Live 配置）: {e}");
        }

        // 2. 恢复原始 Live 配置
        self.restore_live_configs().await?;

        // 3. 清除 proxy_config 表中的接管状态（兼容旧版）
        self.db
            .set_live_takeover_active(false)
            .await
            .map_err(|e| format!("清除接管状态失败: {e}"))?;

        // 4. 清除所有应用的 enabled 状态（用户手动关闭，不需要下次自动恢复）
        for app_type in ["claude", "codex", "gemini", "grokbuild"] {
            if let Ok(mut config) = self.db.get_proxy_config_for_app(app_type).await {
                if config.enabled {
                    config.enabled = false;
                    if let Err(e) = self.db.update_proxy_config_for_app(config).await {
                        log::warn!("清除 {app_type} enabled 状态失败: {e}");
                    }
                }
            }
        }

        // 5. 删除备份
        self.db
            .delete_all_live_backups()
            .await
            .map_err(|e| format!("删除备份失败: {e}"))?;

        // 6. 重置健康状态（让健康徽章恢复为正常）
        self.db
            .clear_all_provider_health()
            .await
            .map_err(|e| format!("重置健康状态失败: {e}"))?;

        // 注意：不清除故障转移队列和开关状态，保留供下次开启代理时使用
        log::info!("代理已停止，Live 配置已恢复");
        Ok(())
    }

    /// 停止代理服务器（恢复 Live 配置，但保留 settings 表中的代理状态）
    ///
    /// 用于程序正常退出时，保留代理状态以便下次启动时自动恢复
    pub async fn stop_with_restore_keep_state(&self) -> Result<(), TakeoverError> {
        // 1. 停止代理服务器（即使未运行也继续执行恢复逻辑）
        if let Err(e) = self.stop().await {
            log::warn!("停止代理服务器失败（将继续恢复 Live 配置）: {e}");
        }

        // 2. 恢复原始 Live 配置
        self.restore_live_configs().await?;

        // 3. 更新 proxy_config 表中的 live_takeover_active 标志（兼容旧版）
        //    注意：保留 proxy_config.enabled 状态，下次启动时自动恢复
        if let Ok(mut config) = self.db.get_proxy_config().await {
            config.live_takeover_active = false;
            let _ = self.db.update_proxy_config(config).await;
        }

        // 4. 删除备份（Live 配置已恢复，备份不再需要）
        self.db
            .delete_all_live_backups()
            .await
            .map_err(|e| format!("删除备份失败: {e}"))?;

        // 5. 重置健康状态
        self.db
            .clear_all_provider_health()
            .await
            .map_err(|e| format!("重置健康状态失败: {e}"))?;

        log::info!("代理已停止，Live 配置已恢复（保留代理状态，下次启动将自动恢复）");
        Ok(())
    }

    /// 获取服务器状态
    pub async fn get_status(&self) -> Result<ProxyStatus, TakeoverError> {
        if let Some(server) = self.server.read().await.as_ref() {
            Ok(server.get_status().await)
        } else {
            // 服务器未运行时返回默认状态
            Ok(ProxyStatus {
                running: false,
                ..Default::default()
            })
        }
    }

    /// 获取代理配置
    pub async fn get_config(&self) -> Result<ProxyConfig, TakeoverError> {
        self.db
            .get_proxy_config()
            .await
            .map_err(TakeoverError::with(
                TakeoverErrorKind::Config,
                "获取代理配置失败",
            ))
    }

    /// 更新代理配置
    pub async fn update_config(&self, config: &ProxyConfig) -> Result<(), TakeoverError> {
        // 记录旧配置用于判定是否需要重启
        let previous = self
            .db
            .get_proxy_config()
            .await
            .map_err(|e| format!("获取代理配置失败: {e}"))?;

        // 保存到数据库（保持 live_takeover_active 状态不变）
        let mut new_config = config.clone();
        new_config.live_takeover_active = previous.live_takeover_active;

        self.db
            .update_proxy_config(new_config.clone())
            .await
            .map_err(|e| format!("保存代理配置失败: {e}"))?;

        // 检查服务器当前状态
        let mut server_guard = self.server.write().await;
        if server_guard.is_none() {
            return Ok(());
        }

        // 判断是否需要重启（地址或端口变更）
        let require_restart = new_config.listen_address != previous.listen_address
            || new_config.listen_port != previous.listen_port;

        if require_restart {
            // SVC-43: a port that is already taken must not cost the running
            // server. Probe the new address first (only when the port changes;
            // the same port is still held by the running server) and keep the
            // old server and config on failure.
            if new_config.listen_port != 0 && new_config.listen_port != previous.listen_port {
                if let Err(e) =
                    probe_listen_address(&new_config.listen_address, new_config.listen_port)
                {
                    drop(server_guard);
                    self.revert_proxy_config(&previous).await;
                    return Err(port_unavailable_error(
                        &new_config.listen_address,
                        new_config.listen_port,
                        e.message(),
                    ));
                }
            }

            if let Some(server) = server_guard.take() {
                if let Err(e) = server.stop().await {
                    *server_guard = Some(server);
                    drop(server_guard);
                    self.revert_proxy_config(&previous).await;
                    return Err(TakeoverError::new(
                        TakeoverErrorKind::StopFailed,
                        format!("重启前停止代理服务器失败: {e}"),
                    ));
                }
            }

            let app_handle = self.app_handle.read().await.clone();
            let new_server = ProxyServer::new(new_config.clone(), self.db.clone(), app_handle);
            let info = match new_server.start().await {
                Ok(info) => info,
                Err(e) => {
                    // Bring the previous server back; if even that fails, hand
                    // the live files back to the providers so clients do not
                    // point at a dead port.
                    self.revert_proxy_config(&previous).await;
                    let app_handle = self.app_handle.read().await.clone();
                    let old_server =
                        ProxyServer::new(previous.clone(), self.db.clone(), app_handle);
                    match old_server.start().await {
                        Ok(_) => {
                            *server_guard = Some(old_server);
                            log::warn!("重启代理服务器失败，已恢复原端口: {e}");
                        }
                        Err(restart_err) => {
                            drop(server_guard);
                            log::error!("重启代理服务器失败且无法恢复原端口: {e}; {restart_err}");
                            self.release_all_takeovers_after_dead_restart().await;
                        }
                    }
                    return Err(port_unavailable_error(
                        &new_config.listen_address,
                        new_config.listen_port,
                        &e.to_string(),
                    ));
                }
            };
            if let Err(e) = self
                .persist_ephemeral_listen_port_if_needed(&new_config, info.port)
                .await
            {
                let _ = new_server.stop().await;
                return Err(e);
            }

            *server_guard = Some(new_server);
            log::info!("代理配置已更新，服务器已自动重启应用最新配置");

            // 如果当前存在任意 app 的 Live 接管，需要同步更新 Live 中的代理地址（否则客户端仍指向旧端口）
            drop(server_guard);
            if let Ok(takeover) = self.get_takeover_status().await {
                let mut updated_any = false;
                // Rewrite every taken-over app even if one fails, then report all
                // failures together instead of claiming success (SVC-06).
                let mut failures: Vec<String> = Vec::new();

                for (taken_over, app_type) in [
                    (takeover.claude, AppType::Claude),
                    (takeover.codex, AppType::Codex),
                    (takeover.gemini, AppType::Gemini),
                    (takeover.grokbuild, AppType::GrokBuild),
                ] {
                    if !taken_over {
                        continue;
                    }
                    match self.takeover_live_config_best_effort(&app_type).await {
                        Ok(()) => updated_any = true,
                        Err(e) => {
                            log::error!(
                                "更新 {} Live 配置中的代理地址失败: {e}",
                                app_type.as_str()
                            );
                            failures.push(format!("{}: {e}", app_type.as_str()));
                        }
                    }
                }

                if updated_any {
                    log::info!("已同步更新 Live 配置中的代理地址");
                }
                if !failures.is_empty() {
                    return Err(TakeoverError::new(
                        TakeoverErrorKind::LiveWrite,
                        format!(
                            "代理服务器已重启，但更新 Live 配置中的代理地址失败（客户端可能仍指向旧端口）: {}",
                            failures.join("; ")
                        ),
                    ));
                }
            }

            return Ok(());
        } else if let Some(server) = server_guard.as_ref() {
            server.apply_runtime_config(&new_config).await;
            log::info!("代理配置已实时应用，无需重启代理服务器");
        }

        Ok(())
    }

    /// Put the previous proxy config back after a failed restart (SVC-43).
    async fn revert_proxy_config(&self, previous: &ProxyConfig) {
        if let Err(e) = self.db.update_proxy_config(previous.clone()).await {
            log::error!("恢复代理配置失败: {e}");
        }
    }

    /// Last resort when neither the new nor the old listener can be bound:
    /// restore every taken-over app's live file and clear its flag, so the UI,
    /// DB and live files agree that takeover is off (SVC-43).
    async fn release_all_takeovers_after_dead_restart(&self) {
        for adapter in crate::adapters::takeover_adapters() {
            let app_type = adapter.app();
            let enabled = self
                .db
                .get_proxy_config_for_app(app_type.as_str())
                .await
                .map(|config| config.enabled)
                .unwrap_or(false);
            let has_backup = matches!(
                self.db.get_live_backup(app_type.as_str()).await,
                Ok(Some(_))
            );
            if !enabled && !has_backup {
                continue;
            }
            if let Err(e) = self.disable_takeover_for_app_sync_async(&app_type).await {
                log::error!("释放 {} 接管失败: {e}", app_type.as_str());
            }
        }
    }

    /// 检查服务器是否正在运行
    pub async fn is_running(&self) -> bool {
        self.server.read().await.is_some()
    }

    /// 热更新熔断器配置
    ///
    /// 如果代理服务器正在运行，将新配置应用到所有已创建的熔断器实例
    pub async fn update_circuit_breaker_configs(
        &self,
        config: crate::proxy::CircuitBreakerConfig,
    ) -> Result<(), TakeoverError> {
        if let Some(server) = self.server.read().await.as_ref() {
            server.update_circuit_breaker_configs(config).await;
            log::info!("已热更新运行中的熔断器配置");
        } else {
            log::debug!("代理服务器未运行，熔断器配置将在下次启动时生效");
        }
        Ok(())
    }

    /// 热更新指定应用的熔断器配置
    pub async fn update_circuit_breaker_config_for_app(
        &self,
        app_type: &str,
        config: crate::proxy::CircuitBreakerConfig,
    ) -> Result<(), TakeoverError> {
        if let Some(server) = self.server.read().await.as_ref() {
            server
                .update_circuit_breaker_config_for_app(app_type, config)
                .await;
            log::info!("已热更新 {app_type} 运行中的熔断器配置");
        } else {
            log::debug!("{app_type} 熔断器配置将在下次代理启动时生效");
        }
        Ok(())
    }

    /// 重置指定 Provider 的熔断器
    ///
    /// 如果代理服务器正在运行，立即重置内存中的熔断器状态
    pub async fn reset_provider_circuit_breaker(
        &self,
        provider_id: &str,
        app_type: &str,
    ) -> Result<(), TakeoverError> {
        if let Some(server) = self.server.read().await.as_ref() {
            server
                .reset_provider_circuit_breaker(provider_id, app_type)
                .await;
            log::info!("已重置 Provider {provider_id} (app: {app_type}) 的熔断器");
        }
        Ok(())
    }
}

/// Check that `address:port` can be bound right now (SVC-43). The probe
/// listener is dropped immediately.
fn probe_listen_address(address: &str, port: u16) -> Result<(), TakeoverError> {
    let host = address.trim().trim_start_matches('[').trim_end_matches(']');
    let host = if host.is_empty() { "127.0.0.1" } else { host };
    std::net::TcpListener::bind((host, port))
        .map(drop)
        .map_err(|e| TakeoverError::new(TakeoverErrorKind::PortUnavailable, e.to_string()))
}

/// User-facing error for a restart that could not bind the new address. The
/// key lets the frontend localize it; the message names the port.
fn port_unavailable_error(address: &str, port: u16, detail: &str) -> TakeoverError {
    let message = crate::error::AppError::localized(
        "proxy.port_unavailable",
        format!("无法监听 {address}:{port}（端口可能已被占用），已保留原代理配置: {detail}"),
        format!(
            "Cannot listen on {address}:{port} (the port may be in use); the previous proxy settings were kept: {detail}"
        ),
    )
    .to_string();
    TakeoverError::new(TakeoverErrorKind::PortUnavailable, message)
}

#[cfg(test)]
mod tests;
