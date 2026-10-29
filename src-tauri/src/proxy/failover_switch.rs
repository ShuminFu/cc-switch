//! 故障转移切换模块
//!
//! 处理故障转移成功后的供应商切换逻辑，包括：
//! - 去重控制（避免多个请求同时触发）
//! - 托盘菜单更新
//! - 前端事件发射

use crate::database::Database;
use crate::error::AppError;
use crate::proxy::runtime_config::RuntimeConfigStore;
use std::collections::HashSet;
use std::sync::Arc;
use tauri::{Emitter, Manager};
use tokio::sync::RwLock;

/// 故障转移切换管理器
///
/// 负责处理故障转移成功后的供应商切换，确保 UI 能够直观反映当前使用的供应商。
#[derive(Clone)]
pub struct FailoverSwitchManager {
    /// 正在处理中的切换（key = "app_type:provider_id"）
    pending_switches: Arc<RwLock<HashSet<String>>>,
    /// 运行时配置快照（读取 proxy_config.enabled 不直连数据库）
    runtime_config: Arc<RuntimeConfigStore>,
}

impl FailoverSwitchManager {
    pub fn new(db: Arc<Database>) -> Self {
        Self::with_runtime_config(Arc::new(RuntimeConfigStore::new(db)))
    }

    /// 使用共享的运行时配置快照创建（代理服务器内与 `ProxyState` 共用同一份）
    pub fn with_runtime_config(runtime_config: Arc<RuntimeConfigStore>) -> Self {
        Self {
            pending_switches: Arc::new(RwLock::new(HashSet::new())),
            runtime_config,
        }
    }

    /// 尝试执行故障转移切换
    ///
    /// 如果相同的切换已在进行中，则跳过；否则执行切换逻辑。
    ///
    /// # Returns
    /// - `Ok(true)` - 切换成功执行
    /// - `Ok(false)` - 切换已在进行中，跳过
    /// - `Err(e)` - 切换过程中发生错误
    pub async fn try_switch(
        &self,
        app_handle: Option<&tauri::AppHandle>,
        app_type: &str,
        provider_id: &str,
        provider_name: &str,
    ) -> Result<bool, AppError> {
        let switch_key = format!("{app_type}:{provider_id}");

        // 去重检查：如果相同切换已在进行中，跳过
        {
            let mut pending = self.pending_switches.write().await;
            if pending.contains(&switch_key) {
                log::debug!("[Failover] 切换已在进行中，跳过: {app_type} -> {provider_id}");
                return Ok(false);
            }
            pending.insert(switch_key.clone());
        }

        // 执行切换（确保最后清理 pending 标记）
        let result = self
            .do_switch(app_handle, app_type, provider_id, provider_name)
            .await;

        // 清理 pending 标记
        {
            let mut pending = self.pending_switches.write().await;
            pending.remove(&switch_key);
        }

        result
    }

    async fn do_switch(
        &self,
        app_handle: Option<&tauri::AppHandle>,
        app_type: &str,
        provider_id: &str,
        provider_name: &str,
    ) -> Result<bool, AppError> {
        // 检查该应用是否已被代理接管（enabled=true）
        // 只有被接管的应用才允许执行故障转移切换
        let app_enabled = match self.runtime_config.app_proxy_config(app_type).await {
            Ok(config) => config.enabled,
            Err(e) => {
                log::warn!("[FO-002] 无法读取 {app_type} 配置: {e}，跳过切换");
                return Ok(false);
            }
        };

        if !app_enabled {
            log::debug!("[Failover] {app_type} 未启用代理，跳过切换");
            return Ok(false);
        }

        log::info!("[FO-001] 切换: {app_type} → {provider_name}");

        let mut switched = false;

        if let Some(app) = app_handle {
            if let Some(app_state) = app.try_state::<crate::store::AppState>() {
                switched = app_state
                    .proxy_service
                    .hot_switch_provider(app_type, provider_id)
                    .await
                    .map_err(AppError::Message)?
                    .logical_target_changed;

                if !switched {
                    return Ok(false);
                }

                if let Ok(new_menu) = crate::tray::create_tray_menu(app, app_state.inner()) {
                    if let Some(tray) = app.tray_by_id(crate::tray::TRAY_ID) {
                        if let Err(e) = tray.set_menu(Some(new_menu)) {
                            log::error!("[Failover] 更新托盘菜单失败: {e}");
                        }
                    }
                }
            }

            // 发射事件到前端
            let event_data = serde_json::json!({
                "appType": app_type,
                "providerId": provider_id,
                "source": "failover"  // 标识来源是故障转移
            });
            if let Err(e) = app.emit("provider-switched", event_data) {
                log::error!("[Failover] 发射事件失败: {e}");
            }
        }

        Ok(switched)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manager() -> FailoverSwitchManager {
        FailoverSwitchManager::new(Arc::new(Database::memory().expect("memory db")))
    }

    async fn set_app_enabled(manager: &FailoverSwitchManager, app_type: &str, enabled: bool) {
        let mut config = manager
            .runtime_config
            .db()
            .get_proxy_config_for_app(app_type)
            .await
            .expect("read proxy config");
        config.enabled = enabled;
        manager
            .runtime_config
            .db()
            .update_proxy_config_for_app(config)
            .await
            .expect("update proxy config");
    }

    async fn is_pending(manager: &FailoverSwitchManager, key: &str) -> bool {
        manager.pending_switches.read().await.contains(key)
    }

    #[tokio::test]
    async fn try_switch_skips_when_same_switch_is_pending() {
        let manager = manager();
        set_app_enabled(&manager, "claude", true).await;
        manager
            .pending_switches
            .write()
            .await
            .insert("claude:p1".to_string());

        let result = manager
            .try_switch(None, "claude", "p1", "Provider 1")
            .await
            .expect("dedup skip is not an error");

        assert!(!result);
        // 被跳过的调用不得清理正在进行中的切换标记
        assert!(is_pending(&manager, "claude:p1").await);
    }

    #[tokio::test]
    async fn try_switch_clears_pending_marker_after_completion() {
        let manager = manager();
        set_app_enabled(&manager, "claude", true).await;

        let result = manager
            .try_switch(None, "claude", "p1", "Provider 1")
            .await
            .expect("switch without app handle");

        // 无 AppHandle 时无法热切换，结果为 false，但标记必须被清理
        assert!(!result);
        assert!(!is_pending(&manager, "claude:p1").await);
        assert!(manager.pending_switches.read().await.is_empty());
    }

    #[tokio::test]
    async fn try_switch_skips_when_app_not_taken_over() {
        let manager = manager();
        set_app_enabled(&manager, "codex", false).await;

        let result = manager
            .try_switch(None, "codex", "p2", "Provider 2")
            .await
            .expect("disabled app is not an error");

        assert!(!result);
        assert!(!is_pending(&manager, "codex:p2").await);
    }

    #[tokio::test]
    async fn pending_marker_is_scoped_to_app_and_provider() {
        let manager = manager();
        set_app_enabled(&manager, "claude", true).await;
        manager
            .pending_switches
            .write()
            .await
            .insert("claude:p1".to_string());

        // 其他 provider / 其他应用的切换不受影响，且完成后只清理自己的标记
        manager
            .try_switch(None, "claude", "p2", "Provider 2")
            .await
            .expect("other provider");
        manager
            .try_switch(None, "codex", "p1", "Provider 1")
            .await
            .expect("other app");

        let pending = manager.pending_switches.read().await;
        assert_eq!(pending.len(), 1);
        assert!(pending.contains("claude:p1"));
    }
}
