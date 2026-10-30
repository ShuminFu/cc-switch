//! 运行期事件路由：single-instance 回调、深链接、窗口关闭与 `RunEvent`。
//!
//! 所有 `ccswitch://` 深链接入口（single-instance 参数、`on_open_url`、macOS
//! `RunEvent::Opened`）都经 [`route_deeplinks`] 处理，日志脱敏只在这一处完成。

use tauri::{Emitter, Manager, RunEvent};
use tauri_plugin_deep_link::DeepLinkExt;

/// 轻量模式下主窗口已销毁，需要展示 UI 的入口先重建窗口
fn leave_lightweight_mode(app: &tauri::AppHandle) {
    if crate::lightweight::is_lightweight_mode(app) {
        if let Err(e) = crate::lightweight::exit_lightweight_mode(app) {
            log::error!("退出轻量模式重建窗口失败: {e}");
        }
    }
}

fn show_main_window(app: &tauri::AppHandle) -> bool {
    let Some(window) = app.get_webview_window("main") else {
        return false;
    };
    let _ = window.unminimize();
    let _ = window.show();
    let _ = window.set_focus();
    #[cfg(target_os = "linux")]
    {
        crate::linux_fix::nudge_main_window(window.clone());
    }
    true
}

// ============================================================
// 深链接
// ============================================================

/// 统一处理 ccswitch:// 深链接 URL
///
/// - 解析 URL
/// - 向前端发射 `deeplink-import` / `deeplink-error` 事件
/// - 可选：在成功时聚焦主窗口
fn handle_deeplink_url(
    app: &tauri::AppHandle,
    url_str: &str,
    focus_main_window: bool,
    source: &str,
) -> bool {
    if !url_str.starts_with("ccswitch://") {
        return false;
    }

    let redacted_url = crate::deeplink::redact_url_for_log(url_str);
    log::info!("✓ Deep link URL detected from {source}: {redacted_url}");

    match crate::deeplink::parse_deeplink_url(url_str) {
        Ok(request) => {
            log::info!(
                "✓ Successfully parsed deep link: resource={}, app={:?}, name={:?}",
                request.resource,
                request.app,
                request.name
            );

            if let Err(e) = app.emit("deeplink-import", &request) {
                log::error!("✗ Failed to emit deeplink-import event: {e}");
            } else {
                log::info!("✓ Emitted deeplink-import event to frontend");
            }

            if focus_main_window && show_main_window(app) {
                log::info!("✓ Window shown and focused");
            }
        }
        Err(e) => {
            log::error!("✗ Failed to parse deep link URL: {e}");

            if let Err(emit_err) = app.emit(
                "deeplink-error",
                crate::deeplink::deeplink_error_payload(url_str, &e.to_string()),
            ) {
                log::error!("✗ Failed to emit deeplink-error event: {emit_err}");
            }
        }
    }

    true
}

/// 深链接路由：只处理第一个 `ccswitch://` URL，返回是否找到
pub fn route_deeplinks<'a>(
    app: &tauri::AppHandle,
    urls: impl IntoIterator<Item = &'a str>,
    focus_main_window: bool,
    source: &str,
) -> bool {
    for (i, url) in urls.into_iter().enumerate() {
        log::debug!(
            "  [{source}] URL[{i}]: {}",
            crate::deeplink::redact_url_for_log(url)
        );
        if handle_deeplink_url(app, url, focus_main_window, source) {
            return true;
        }
    }
    false
}

/// single-instance 回调：第二个实例启动时唤起已有窗口，并转发命令行中的深链接
pub fn on_second_instance(app: &tauri::AppHandle, args: Vec<String>, _cwd: String) {
    log::info!("=== Single Instance Callback Triggered ===");
    log::debug!("Args count: {}", args.len());

    leave_lightweight_mode(app);

    // Check for deep link URL in args (mainly for Windows/Linux command line)
    if !route_deeplinks(
        app,
        args.iter().map(String::as_str),
        false,
        "single_instance args",
    ) {
        log::info!(
            "ℹ No deep link URL found in args (this is expected on macOS when launched via system)"
        );
    }

    // Show and focus window regardless
    show_main_window(app);
}

/// 注册 deep-link URL 处理器（使用正确的 DeepLinkExt API）
pub fn register_deep_links(app: &tauri::App) {
    log::info!("=== Registering deep-link URL handler ===");

    // Linux 和 Windows 调试模式需要显式注册
    #[cfg(target_os = "linux")]
    {
        // Use Tauri's path API to get correct path (includes app identifier)
        // tauri-plugin-deep-link writes to: ~/.local/share/com.ccswitch.desktop/applications/cc-switch-handler.desktop
        // Only register if .desktop file doesn't exist to avoid overwriting user customizations
        let should_register = app
            .path()
            .data_dir()
            .map(|d| !d.join("applications/cc-switch-handler.desktop").exists())
            .unwrap_or(true);

        if should_register {
            if let Err(e) = app.deep_link().register_all() {
                log::error!("✗ Failed to register deep link schemes: {}", e);
            } else {
                log::info!("✓ Deep link schemes registered (Linux)");
            }
        } else {
            log::info!("⊘ Deep link handler already exists, skipping registration");
        }
    }

    #[cfg(all(debug_assertions, windows))]
    {
        if let Err(e) = app.deep_link().register_all() {
            log::error!("✗ Failed to register deep link schemes: {}", e);
        } else {
            log::info!("✓ Deep link schemes registered (Windows debug)");
        }
    }

    // 注册 URL 处理回调（所有平台通用）
    let app_handle = app.handle().clone();
    app.deep_link().on_open_url(move |event| {
        log::info!("=== Deep Link Event Received (on_open_url) ===");
        let urls = event.urls();
        log::info!("Received {} URL(s)", urls.len());

        leave_lightweight_mode(&app_handle);
        route_deeplinks(
            &app_handle,
            urls.iter().map(|u| u.as_str()),
            true,
            "on_open_url",
        );
    });
    log::info!("✓ Deep-link URL handler registered");
}

// ============================================================
// 窗口事件
// ============================================================

/// 拦截窗口关闭：根据设置决定是否最小化到托盘
pub fn on_window_event(window: &tauri::Window, event: &tauri::WindowEvent) {
    let tauri::WindowEvent::CloseRequested { api, .. } = event else {
        return;
    };

    // 数据库版本过新的恢复模式下没有托盘可唤回，关闭即退出，避免应用隐身后台
    let in_db_recovery = crate::init_status::get_init_error()
        .map(|p| p.kind.as_deref() == Some(crate::init_status::DB_VERSION_TOO_NEW))
        .unwrap_or(false);
    if in_db_recovery {
        api.prevent_close();
        window.app_handle().exit(0);
        return;
    }

    let settings = crate::settings::get_settings();

    if settings.minimize_to_tray_on_close {
        api.prevent_close();
        let _ = window.hide();
        #[cfg(target_os = "windows")]
        {
            let _ = window.set_skip_taskbar(true);
        }
        #[cfg(target_os = "macos")]
        {
            crate::tray::apply_tray_policy(window.app_handle(), false);
        }
    } else {
        api.prevent_close();
        window.app_handle().exit(0);
    }
}

// ============================================================
// RunEvent
// ============================================================

/// `RunEvent::ExitRequested` 的三类来源，处理方式必须区分。
///
/// 关键约束：重启请求（`code == RESTART_EXIT_CODE`）上 `prevent_exit()` 会被
/// Tauri 静默忽略（见 `ExitRequestApi::prevent_exit` 文档），事件循环必定继续
/// 退出并触发各插件的 `RunEvent::Exit` 钩子；任何与之并发的自定义清理任务都
/// 可能与插件退出钩子争用同一状态而死锁。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExitRequestAction {
    /// `code` 为 `None`：运行时自动触发（如隐藏窗口的 WebView 被回收导致无存活
    /// 窗口），阻止退出、保持托盘后台运行。
    StayInTray,
    /// `code` 为 `RESTART_EXIT_CODE`：`app.restart()` / 自更新 relaunch 发起的
    /// 重启，不拦截、不做自定义清理，交还 Tauri 默认 re-exec 流程。
    DeferToTauriRestart,
    /// 其它 `Some(_)`：用户主动退出（托盘「退出」等），执行完整异步清理后结束进程。
    CleanupAndExit,
}

fn classify_exit_request(code: Option<i32>) -> ExitRequestAction {
    match code {
        None => ExitRequestAction::StayInTray,
        Some(tauri::RESTART_EXIT_CODE) => ExitRequestAction::DeferToTauriRestart,
        Some(_) => ExitRequestAction::CleanupAndExit,
    }
}

/// `App::run` 的事件回调
pub fn on_run_event(app_handle: &tauri::AppHandle, event: RunEvent) {
    // 处理退出请求（所有平台）
    if let RunEvent::ExitRequested { api, code, .. } = &event {
        match classify_exit_request(*code) {
            // code 为 None 表示运行时自动触发（如隐藏窗口的 WebView 被回收导致无存活窗口），
            // 此时应仅阻止退出、保持托盘后台运行。
            ExitRequestAction::StayInTray => {
                log::info!("运行时触发退出请求（无存活窗口），阻止退出以保持托盘后台运行");
                api.prevent_exit();
                return;
            }
            // code 为 RESTART_EXIT_CODE：app.restart() / 自更新 relaunch 发起的重启。
            // 这条路径上 prevent_exit() 会被 Tauri 忽略，事件循环必定退出，随后由
            // Tauri 在 RunEvent::Exit 后用新二进制 re-exec（macOS 会按更新后的
            // Info.plist 解析可执行名）。
            //
            // 绝不能复用下面的异步清理任务：该任务在 tokio 线程调 save_window_state，
            // 持有 window-state 插件锁的同时向主线程查询窗口几何；而主线程此刻正在
            // 退出事件循环，并在插件自带的 RunEvent::Exit 钩子里等待同一把锁——双方
            // 互等造成进程永久卡死（更新已安装但应用冻结、不再重启，见 #3998）。
            //
            // 重启路径交还 Tauri 默认流程即可：
            //   - 窗口状态：插件 Exit 钩子在主线程保存（同线程读取窗口几何，无死锁）
            //   - 托盘图标：Tauri 内部 cleanup_before_exit 清理，正常走 Drop
            //   - 代理/Live 配置：无需恢复，重启后新实例立即接管并恢复代理状态
            //   - 100ms 落盘等待：重启前的 DB 写入均为命令驱动、此刻已完成，
            //     与所有 Tauri 应用默认重启路径的行为一致，无需额外等待
            ExitRequestAction::DeferToTauriRestart => {
                log::info!("收到重启请求 (code={code:?})，交由 Tauri 默认重启流程 re-exec");
                return;
            }
            // 其它 Some(_)：用户主动调用 app.exit() 退出（如托盘菜单"退出"），
            // 此时执行清理后退出。
            ExitRequestAction::CleanupAndExit => {}
        }

        log::info!("收到用户主动退出请求 (code={code:?})，开始清理...");
        api.prevent_exit();

        let app_handle = app_handle.clone();
        tauri::async_runtime::spawn(async move {
            crate::save_window_state_before_exit(&app_handle);
            crate::cleanup_before_exit(&app_handle).await;
            // 先于 std::process::exit 显式移除托盘图标。
            // 进程直接退出时 Tauri 运行时不走正常 Drop 流程，
            // 不会向 Windows Shell 发送 NIM_DELETE，导致已退出的进程
            // 注册的图标仍残留在系统托盘（鼠标悬停 Shell 才会重绘发现进程已死）。
            crate::remove_tray_icon_before_exit(&app_handle);
            log::info!("清理完成，退出应用");

            // 短暂等待确保所有 I/O 操作（如数据库写入）刷新到磁盘
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;

            // 使用 std::process::exit 避免再次触发 ExitRequested
            std::process::exit(0);
        });
        return;
    }

    #[cfg(target_os = "macos")]
    {
        match event {
            // macOS 在 Dock 图标被点击并重新激活应用时会触发 Reopen 事件，这里手动恢复主窗口
            RunEvent::Reopen { .. } => {
                if let Some(window) = app_handle.get_webview_window("main") {
                    let _ = window.unminimize();
                    let _ = window.show();
                    let _ = window.set_focus();
                    crate::tray::apply_tray_policy(app_handle, true);
                } else {
                    leave_lightweight_mode(app_handle);
                }
            }
            // 处理通过自定义 URL 协议触发的打开事件（例如 ccswitch://...）
            RunEvent::Opened { urls } => {
                if let Some(url) = urls.first() {
                    let url_str = url.to_string();
                    if url_str.starts_with("ccswitch://") {
                        leave_lightweight_mode(app_handle);
                    }
                    // 解析并广播深链接事件，复用与 single_instance 相同的路由
                    route_deeplinks(app_handle, [url_str.as_str()], true, "RunEvent::Opened");
                }
            }
            _ => {}
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app_handle, event);
    }
}

#[cfg(test)]
mod tests {
    use super::{classify_exit_request, ExitRequestAction};

    #[test]
    fn no_code_keeps_app_alive_in_tray() {
        assert_eq!(classify_exit_request(None), ExitRequestAction::StayInTray);
    }

    #[test]
    fn restart_exit_code_defers_to_tauri_default_restart() {
        assert_eq!(
            classify_exit_request(Some(tauri::RESTART_EXIT_CODE)),
            ExitRequestAction::DeferToTauriRestart
        );
    }

    #[test]
    fn user_exit_codes_run_cleanup_then_exit() {
        assert_eq!(
            classify_exit_request(Some(0)),
            ExitRequestAction::CleanupAndExit
        );
        assert_eq!(
            classify_exit_request(Some(1)),
            ExitRequestAction::CleanupAndExit
        );
    }
}
