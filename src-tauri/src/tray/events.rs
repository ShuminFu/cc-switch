//! 托盘事件：驱动菜单重建 / 标题软更新的事件总线，以及菜单点击处理。
//!
//! 任何模块只需 [`publish_tray_event`]（或 [`refresh_tray_menu`] /
//! [`schedule_tray_refresh`]）投递事件；总线在异步任务里合并同一时间窗内的
//! 事件，先探测代理运行态再重建菜单，菜单构建本身从不等待异步任务。

use std::sync::OnceLock;

use tauri::{Emitter, Manager};
use tauri_plugin_opener::OpenerExt;
use tokio::sync::mpsc;

use super::{menu, model, AUTO_SUFFIX, TRAY_SECTIONS};
use crate::app_config::AppType;
use crate::error::AppError;
use crate::store::AppState;

/// 托盘事件
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayEvent {
    /// 菜单结构或勾选态可能变化（供应商 / 项目切换、代理启停、轻量模式、设置……）：
    /// 重新探测代理运行态并整菜单重建
    MenuChanged,
    /// 仅用量缓存变化：就地更新各分区子菜单标题，避免关闭用户打开中的菜单
    UsageChanged,
}

/// 一批事件合并后要执行的动作
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum TrayEffect {
    RelabelUsage,
    Rebuild,
}

impl TrayEvent {
    fn effect(self) -> TrayEffect {
        match self {
            TrayEvent::MenuChanged => TrayEffect::Rebuild,
            TrayEvent::UsageChanged => TrayEffect::RelabelUsage,
        }
    }
}

/// 合并一批事件：整菜单重建已包含标题更新，取最强的动作
fn coalesce(events: &[TrayEvent]) -> Option<TrayEffect> {
    events.iter().map(|e| e.effect()).max()
}

/// 50ms 合窗：让同一轮 React Query / 托盘批量刷新触发的多个写入共享一次更新。
const COALESCE_WINDOW: std::time::Duration = std::time::Duration::from_millis(50);

static TRAY_EVENT_BUS: OnceLock<mpsc::UnboundedSender<TrayEvent>> = OnceLock::new();

/// 投递托盘事件；首次调用时启动总线消费任务
pub fn publish_tray_event(app: &tauri::AppHandle, event: TrayEvent) {
    let sender = TRAY_EVENT_BUS.get_or_init(|| {
        let (tx, rx) = mpsc::unbounded_channel();
        tauri::async_runtime::spawn(run_event_bus(app.clone(), rx));
        tx
    });
    if let Err(e) = sender.send(event) {
        log::warn!("[Tray] 投递托盘事件失败: {e}");
    }
}

async fn run_event_bus(app: tauri::AppHandle, mut rx: mpsc::UnboundedReceiver<TrayEvent>) {
    while let Some(first) = rx.recv().await {
        tokio::time::sleep(COALESCE_WINDOW).await;
        let mut batch = vec![first];
        while let Ok(event) = rx.try_recv() {
            batch.push(event);
        }
        match coalesce(&batch) {
            Some(TrayEffect::Rebuild) => {
                if let Err(e) = menu::rebuild_tray_menu(&app).await {
                    log::error!("刷新托盘菜单失败: {e}");
                }
            }
            Some(TrayEffect::RelabelUsage) => {
                let app = app.clone();
                if let Err(e) = tauri::async_runtime::spawn_blocking(move || {
                    menu::update_tray_usage_labels(&app)
                })
                .await
                {
                    log::debug!("[Tray] 更新用量标题任务失败: {e}");
                }
            }
            None => {}
        }
    }
}

/// 请求整菜单重建（异步执行，调用方不阻塞）
pub fn refresh_tray_menu(app: &tauri::AppHandle) {
    publish_tray_event(app, TrayEvent::MenuChanged);
}

/// 请求 usage 标题软更新：批量刷新期间多个 usage 命令同时成功时，只会产生
/// 一次就地 `set_text` 批量调用。走软更新而不是整建，避免用户打开中的菜单
/// 被 macOS 系统关闭。
pub fn schedule_tray_refresh(app: &tauri::AppHandle) {
    publish_tray_event(app, TrayEvent::UsageChanged);
}

/// 处理项目 Profile 托盘事件，返回是否已处理
///
/// 事件 id 形如 `profile_<scope>_<uuid>`（同一项目在各分组子菜单里各有一项，
/// 应用时只作用于该分组）；`profile_none_<scope>` 表示某分组"不使用项目"
/// （只清该分组标记，不动配置）。
fn handle_profile_tray_event(app: &tauri::AppHandle, event_id: &str) -> bool {
    let Some(suffix) = event_id.strip_prefix("profile_") else {
        return false;
    };

    if let Some(scope_str) = suffix.strip_prefix("none_") {
        let Ok(scope) = crate::services::profile::ProfileScope::parse(scope_str) else {
            log::error!("未知的项目分组托盘事件: {event_id}");
            return true;
        };
        if let Some(app_state) = app.try_state::<AppState>() {
            if let Err(e) = app_state.db.set_current_profile_id(scope.as_str(), None) {
                log::error!("清除当前项目失败: {e}");
            }
        }
        // 通知主窗口刷新（profileId=null 表示该分组已清除当前项目）
        if let Err(e) = app.emit(
            "profile-applied",
            serde_json::json!({ "profileId": null, "scope": scope.as_str() }),
        ) {
            log::error!("发射 profile-applied 事件失败: {e}");
        }
        refresh_tray_menu(app);
        return true;
    }

    // scope 是固定枚举字符串（不含下划线），uuid 只含连字符，首个下划线即分界
    let Some((scope_str, profile_id)) = suffix.split_once('_') else {
        log::error!("无法解析项目托盘事件: {event_id}");
        return true;
    };
    let Ok(scope) = crate::services::profile::ProfileScope::parse(scope_str) else {
        log::error!("未知的项目分组托盘事件: {event_id}");
        return true;
    };

    log::info!("应用项目: {profile_id}（{scope_str} 组）");
    let app_handle = app.clone();
    let profile_id = profile_id.to_string();
    tauri::async_runtime::spawn_blocking(move || {
        let Some(app_state) = app_handle.try_state::<AppState>() else {
            return;
        };
        match crate::services::profile::ProfileService::apply(app_state.inner(), &profile_id, scope)
        {
            Ok((warnings, should_stop_proxy)) => {
                for warning in &warnings {
                    log::warn!("[Profile] 应用项目 {profile_id} 警告: {warning}");
                }

                if should_stop_proxy {
                    let app_handle2 = app_handle.clone();
                    let proxy_service = app_state.proxy_service.clone();
                    tauri::async_runtime::spawn(async move {
                        if let Err(e) = proxy_service.stop().await {
                            log::warn!("托盘切换项目后停止代理服务失败: {e}");
                        }
                        if let Some(state) = app_handle2.try_state::<AppState>() {
                            crate::commands::emit_profile_apply_events(
                                &app_handle2,
                                state.inner(),
                                &profile_id,
                                scope,
                            );
                        }
                    });
                } else {
                    crate::commands::emit_profile_apply_events(
                        &app_handle,
                        app_state.inner(),
                        &profile_id,
                        scope,
                    );
                }
            }
            Err(e) => {
                log::error!("应用项目 {profile_id} 失败: {e}");
                refresh_tray_menu(&app_handle);
            }
        }
    });
    true
}

/// 处理供应商托盘事件
fn handle_provider_tray_event(app: &tauri::AppHandle, event_id: &str) -> bool {
    for section in TRAY_SECTIONS.iter() {
        if let Some(suffix) = event_id.strip_prefix(section.prefix.as_str()) {
            // 处理 Auto 点击
            if suffix == AUTO_SUFFIX {
                log::info!("切换到{} Auto模式", section.log_name);
                let app_handle = app.clone();
                let app_type = section.app_type.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(e) = handle_auto_click(&app_handle, &app_type).await {
                        log::error!("切换{}Auto模式失败: {e}", section.log_name);
                    }
                });
                return true;
            }

            // 处理供应商点击
            log::info!("切换到{}供应商: {suffix}", section.log_name);
            let app_handle = app.clone();
            let provider_id = suffix.to_string();
            let app_type = section.app_type.clone();
            tauri::async_runtime::spawn_blocking(move || {
                if let Err(e) = handle_provider_click(&app_handle, &app_type, &provider_id) {
                    log::error!("切换{}供应商失败: {e}", section.log_name);
                }
            });
            return true;
        }
    }
    false
}

/// 处理 Auto 点击：启用 proxy 和 auto_failover
async fn handle_auto_click(app: &tauri::AppHandle, app_type: &AppType) -> Result<(), AppError> {
    if let Some(app_state) = app.try_state::<AppState>() {
        let app_type_str = app_type.as_str();

        // 强一致语义：Auto 模式开启后立即切到队列 P1（P1→P2→...）
        // 若队列为空，则尝试把“当前供应商”自动加入队列作为 P1，避免用户陷入无法开启的死锁。
        let mut queue = app_state.db.get_failover_queue(app_type_str)?;
        if queue.is_empty() {
            let current_id =
                crate::settings::get_effective_current_provider(&app_state.db, app_type)?;
            let Some(current_id) = current_id else {
                return Err(AppError::Message(
                    "故障转移队列为空，且未设置当前供应商，无法启用 Auto 模式".to_string(),
                ));
            };
            app_state
                .db
                .add_to_failover_queue(app_type_str, &current_id)?;
            queue = app_state.db.get_failover_queue(app_type_str)?;
        }

        let p1_provider_id = queue
            .first()
            .map(|item| item.provider_id.clone())
            .ok_or_else(|| AppError::Message("故障转移队列为空，无法启用 Auto 模式".to_string()))?;

        // 真正启用 failover：启动代理服务 + 执行接管 + 开启 auto_failover
        let proxy_service = &app_state.proxy_service;

        // 1) 确保代理服务运行（会自动设置 proxy_enabled = true）
        let is_running = proxy_service.is_running().await;
        if !is_running {
            log::info!("[Tray] Auto 模式：启动代理服务");
            if let Err(e) = proxy_service.start().await {
                log::error!("[Tray] 启动代理服务失败: {e}");
                return Err(AppError::Message(format!("启动代理服务失败: {e}")));
            }
        }

        // 2) 执行 Live 配置接管（确保该 app 被代理接管）
        log::info!("[Tray] Auto 模式：对 {app_type_str} 执行接管");
        if let Err(e) = proxy_service.set_takeover_for_app(app_type_str, true).await {
            log::error!("[Tray] 执行接管失败: {e}");
            return Err(AppError::Message(format!("执行接管失败: {e}")));
        }

        // 3) 设置 auto_failover_enabled = true
        app_state
            .db
            .set_proxy_flags_sync(app_type_str, true, true)?;

        // 3.1) 立即切到队列 P1（热切换：不写 Live，仅更新 DB/settings/备份）
        if let Err(e) = proxy_service
            .switch_proxy_target(app_type_str, &p1_provider_id)
            .await
        {
            log::error!("[Tray] Auto 模式切换到队列 P1 失败: {e}");
            return Err(AppError::Message(format!(
                "Auto 模式切换到队列 P1 失败: {e}"
            )));
        }

        // 4) 更新托盘菜单
        publish_tray_event(app, TrayEvent::MenuChanged);

        // 5) 发射事件到前端
        let event_data = serde_json::json!({
            "appType": app_type_str,
            "proxyEnabled": true,
            "autoFailoverEnabled": true,
            "providerId": p1_provider_id
        });
        if let Err(e) = app.emit("proxy-flags-changed", event_data.clone()) {
            log::error!("发射 proxy-flags-changed 事件失败: {e}");
        }
        // 发射 provider-switched 事件（保持向后兼容，Auto 切换也算一种切换）
        if let Err(e) = app.emit("provider-switched", event_data) {
            log::error!("发射 provider-switched 事件失败: {e}");
        }
    }
    Ok(())
}

/// 处理供应商点击：关闭 auto_failover + 切换供应商
fn handle_provider_click(
    app: &tauri::AppHandle,
    app_type: &AppType,
    provider_id: &str,
) -> Result<(), AppError> {
    if let Some(app_state) = app.try_state::<AppState>() {
        let app_type_str = app_type.as_str();

        // 获取当前 proxy 状态，保持 enabled 不变，只关闭 auto_failover
        let (proxy_enabled, _) = app_state.db.get_proxy_flags_sync(app_type_str);
        app_state
            .db
            .set_proxy_flags_sync(app_type_str, proxy_enabled, false)?;

        // 切换供应商。需要本地路由的供应商也不在这里自动启动代理，
        // 由用户在页面/设置中手动开启。
        crate::services::ProviderService::switch(app_state.inner(), app_type.clone(), provider_id)?;

        // 更新托盘菜单
        publish_tray_event(app, TrayEvent::MenuChanged);

        // 发射事件到前端
        let event_data = serde_json::json!({
            "appType": app_type_str,
            "proxyEnabled": proxy_enabled,
            "autoFailoverEnabled": false,
            "providerId": provider_id
        });
        if let Err(e) = app.emit("proxy-flags-changed", event_data.clone()) {
            log::error!("发射 proxy-flags-changed 事件失败: {e}");
        }
        // 发射 provider-switched 事件（保持向后兼容）
        if let Err(e) = app.emit("provider-switched", event_data) {
            log::error!("发射 provider-switched 事件失败: {e}");
        }
    }
    Ok(())
}

/// 处理托盘菜单事件
pub fn handle_tray_menu_event(app: &tauri::AppHandle, event_id: &str) {
    log::info!("处理托盘菜单事件: {event_id}");

    match event_id {
        model::SHOW_MAIN_ID => {
            if let Some(window) = app.get_webview_window("main") {
                #[cfg(target_os = "windows")]
                {
                    let _ = window.set_skip_taskbar(false);
                }
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
                #[cfg(target_os = "linux")]
                {
                    crate::linux_fix::nudge_main_window(window.clone());
                }
                #[cfg(target_os = "macos")]
                {
                    super::apply_tray_policy(app, true);
                }
            } else if crate::lightweight::is_lightweight_mode(app) {
                if let Err(e) = crate::lightweight::exit_lightweight_mode(app) {
                    log::error!("退出轻量模式重建窗口失败: {e}");
                }
            }
        }
        model::OPEN_WEBSITE_ID => {
            if let Err(e) = app.opener().open_url("https://ccswitch.io", None::<String>) {
                log::error!("打开官方网站失败: {e}");
            }
        }
        model::LIGHTWEIGHT_MODE_ID => {
            if crate::lightweight::is_lightweight_mode(app) {
                if let Err(e) = crate::lightweight::exit_lightweight_mode(app) {
                    log::error!("退出轻量模式失败: {e}");
                }
            } else if let Err(e) = crate::lightweight::enter_lightweight_mode(app) {
                log::error!("进入轻量模式失败: {e}");
            }
        }
        model::QUIT_ID => {
            log::info!("退出应用");
            app.exit(0);
        }
        _ => {
            if handle_profile_tray_event(app, event_id) {
                return;
            }
            if handle_provider_tray_event(app, event_id) {
                return;
            }
            log::warn!("未处理的菜单事件: {event_id}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{coalesce, TrayEffect, TrayEvent};

    #[test]
    fn empty_batch_has_no_effect() {
        assert_eq!(coalesce(&[]), None);
    }

    #[test]
    fn usage_only_batch_relabels_in_place() {
        assert_eq!(
            coalesce(&[TrayEvent::UsageChanged, TrayEvent::UsageChanged]),
            Some(TrayEffect::RelabelUsage)
        );
    }

    #[test]
    fn any_menu_change_upgrades_batch_to_rebuild() {
        assert_eq!(
            coalesce(&[
                TrayEvent::UsageChanged,
                TrayEvent::MenuChanged,
                TrayEvent::UsageChanged
            ]),
            Some(TrayEffect::Rebuild)
        );
    }
}
