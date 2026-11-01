//! Showing, hiding and setting up the main window in one place.
//!
//! Every path that brings the window back (tray "show", single-instance relaunch,
//! deep link, macOS Dock reopen, leaving lightweight mode) must undo what the
//! hide paths did: Windows `set_skip_taskbar(true)` and the macOS Accessory
//! activation policy. Before this module three of those paths forgot, leaving a
//! window with no taskbar button / Dock icon (CMD-45).
//!
//! [`apply_window_setup`] holds the one-time webview setup (Linux hardware
//! acceleration, decorations) so a window recreated by lightweight mode gets the
//! same treatment as the startup window (CMD-37).

use tauri::{Manager, WebviewWindow};

use crate::settings::AppSettings;

pub(crate) const MAIN_WINDOW_LABEL: &str = "main";

/// Show, unminimize and focus the main window, restoring its taskbar button
/// (Windows) and Dock icon (macOS). Returns `false` when there is no window
/// (e.g. lightweight mode destroyed it).
pub(crate) fn show_main_window(app: &tauri::AppHandle) -> bool {
    let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) else {
        return false;
    };
    #[cfg(target_os = "windows")]
    {
        let _ = window.set_skip_taskbar(false);
    }
    let _ = window.unminimize();
    let _ = window.show();
    let _ = window.set_focus();
    #[cfg(target_os = "macos")]
    {
        crate::tray::apply_tray_policy(app, true);
    }
    #[cfg(target_os = "linux")]
    {
        crate::linux_fix::nudge_main_window(window.clone());
    }
    true
}

/// Show the main window, recreating it first when lightweight mode destroyed it.
pub(crate) fn show_or_recreate_main_window(app: &tauri::AppHandle) {
    if show_main_window(app) {
        return;
    }
    if crate::lightweight::is_lightweight_mode() {
        // 重建的 webview 需要重新注册监听，期间到达的深链接先缓存
        crate::commands::mark_deeplink_frontend_not_ready();
        if let Err(e) = crate::lightweight::exit_lightweight_mode(app) {
            log::error!("退出轻量模式重建窗口失败: {e}");
        }
    }
}

/// Hide the main window to the tray: no taskbar button (Windows), no Dock
/// icon (macOS).
pub(crate) fn hide_main_window_to_tray(window: &tauri::Window) {
    let _ = window.hide();
    hide_from_taskbar(window.app_handle(), window.label());
}

/// Remove the taskbar button / Dock icon for a window that is (being) hidden.
pub(crate) fn hide_from_taskbar(app: &tauri::AppHandle, label: &str) {
    #[cfg(target_os = "windows")]
    {
        if let Some(window) = app.get_webview_window(label) {
            let _ = window.set_skip_taskbar(true);
        }
    }
    #[cfg(target_os = "macos")]
    {
        crate::tray::apply_tray_policy(app, false);
    }
    let _ = (app, label);
}

/// One-time setup for a freshly created main webview: Linux WebKitGTK
/// hardware acceleration off (EGL white-screen guard) and the decoration
/// setting. Used by `setup` and by lightweight mode's window rebuild.
pub(crate) fn apply_window_setup(window: &WebviewWindow, settings: &AppSettings) {
    #[cfg(target_os = "linux")]
    {
        let _ = window.with_webview(|webview| {
            use webkit2gtk::{HardwareAccelerationPolicy, SettingsExt, WebViewExt};
            let wk_webview = webview.inner();
            if let Some(settings) = WebViewExt::settings(&wk_webview) {
                SettingsExt::set_hardware_acceleration_policy(
                    &settings,
                    HardwareAccelerationPolicy::Never,
                );
                log::info!("已禁用 WebKitGTK 硬件加速");
            }
        });
        // 仅 Linux 生效：解决 Wayland 下系统窗口按钮不可用的问题
        let _ = window.set_decorations(!settings.use_app_window_controls);
    }
    let _ = (window, settings);
}

/// What closing the main window should do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CloseAction {
    HideToTray,
    Exit,
}

/// Closing hides to the tray only when the user asked for it *and* the tray
/// icon is visible: with `show_in_tray = false` a hidden window could not be
/// brought back, so the app exits instead (CMD-15).
pub(crate) fn close_action(settings: &AppSettings) -> CloseAction {
    if settings.minimize_to_tray_on_close && settings.show_in_tray {
        CloseAction::HideToTray
    } else {
        CloseAction::Exit
    }
}

/// Show or hide the tray icon to match `show_in_tray` (CMD-15).
pub(crate) fn apply_tray_visibility(app: &tauri::AppHandle, settings: &AppSettings) {
    if let Some(tray) = app.tray_by_id(crate::tray::TRAY_ID) {
        if let Err(e) = tray.set_visible(settings.show_in_tray) {
            log::warn!("设置托盘图标可见性失败: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{close_action, CloseAction};
    use crate::settings::AppSettings;

    #[test]
    fn close_hides_to_tray_only_when_the_tray_is_visible() {
        let mut settings = AppSettings {
            minimize_to_tray_on_close: true,
            show_in_tray: true,
            ..AppSettings::default()
        };
        assert_eq!(close_action(&settings), CloseAction::HideToTray);

        // Hidden tray: a hidden window would be unreachable, so exit instead.
        settings.show_in_tray = false;
        assert_eq!(close_action(&settings), CloseAction::Exit);

        settings.minimize_to_tray_on_close = false;
        settings.show_in_tray = true;
        assert_eq!(close_action(&settings), CloseAction::Exit);
    }
}
