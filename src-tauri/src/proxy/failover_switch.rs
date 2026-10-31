//! 故障转移切换模块
//!
//! 处理故障转移成功后的供应商切换逻辑，包括：
//! - 去重控制（避免多个请求同时触发）
//! - 托盘菜单更新
//! - 前端事件发射

use crate::database::Database;
use crate::error::AppError;
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
    db: Arc<Database>,
}

impl FailoverSwitchManager {
    pub fn new(db: Arc<Database>) -> Self {
        Self {
            pending_switches: Arc::new(RwLock::new(HashSet::new())),
            db,
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

        // PROXY-27: a burst of requests served by the same provider must not
        // rewrite the live config / tray once per request. Checked inside the
        // dedup section, so concurrent and sequential repeats are both no-ops.
        let result = if is_current_provider(app_type, provider_id) {
            log::debug!("[Failover] {app_type} 已是 {provider_id}，跳过切换");
            Ok(false)
        } else {
            self.do_switch(app_handle, app_type, provider_id, provider_name)
                .await
        };

        // 清理 pending 标记
        {
            let mut pending = self.pending_switches.write().await;
            pending.remove(&switch_key);
        }

        result
    }

    #[cfg(test)]
    pub(crate) async fn pending_count(&self) -> usize {
        self.pending_switches.read().await.len()
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
        let app_enabled = match self.db.get_proxy_config_for_app(app_type).await {
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

                // Debounced; skips `set_menu` when nothing visible changed (CMD-16).
                crate::tray::request_tray_rebuild(app, false);
            }

            // 发射事件到前端
            let event_data = serde_json::json!({
                "appType": app_type,
                "providerId": provider_id,
                "source": "failover"  // 标识来源是故障转移
            });
            if let Err(e) = app.emit(crate::events::PROVIDER_SWITCHED, event_data) {
                log::error!("[Failover] 发射事件失败: {e}");
            }
            crate::services::notifications::notify_failover_switched(
                app,
                app_type,
                provider_id,
                provider_name,
            );
        }

        Ok(switched)
    }
}

/// Whether the device's current provider for `app_type` already is `provider_id`.
fn is_current_provider(app_type: &str, provider_id: &str) -> bool {
    app_type
        .parse::<crate::app_config::AppType>()
        .ok()
        .and_then(|app| crate::settings::get_current_provider(&app))
        .is_some_and(|current| current == provider_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_apps_are_never_current() {
        assert!(!is_current_provider("not-an-app", "p1"));
    }

    #[tokio::test]
    async fn concurrent_switches_to_the_same_target_leave_no_pending_state() {
        let db = Arc::new(Database::memory().expect("db"));
        let manager = FailoverSwitchManager::new(db);
        let results = futures::future::join_all(
            (0..5).map(|_| manager.try_switch(None, "claude", "p-b", "B")),
        )
        .await;
        // No app handle: nothing is switched, and nothing stays marked pending.
        assert!(results.iter().all(|r| matches!(r, Ok(false))));
        assert_eq!(manager.pending_count().await, 0);
    }
}
