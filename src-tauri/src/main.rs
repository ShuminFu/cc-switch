// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // PRODUCT-10: `cc-switch --headless` runs the local proxy in the foreground
    // without creating a window or tray (servers, WSL, SSH sessions).
    if cc_switch_lib::cli::is_headless_invocation(std::env::args_os()) {
        std::process::exit(cc_switch_lib::cli::run_headless(std::env::args_os()));
    }

    // 在 Linux 上设置 WebKit 环境变量以解决 DMA-BUF 渲染问题
    // 某些 Linux 系统（如 Debian 13.2、Nvidia GPU）上 WebKitGTK 的 DMA-BUF 渲染器可能导致白屏/黑屏
    // 参考: https://github.com/tauri-apps/tauri/issues/9394
    // 禁用 WebKitGTK 合成模式，规避 resize 时 webview 崩溃以及部分 Wayland
    // 合成器下的 surface 协商问题（整窗 UI 点击无响应、必须最大化-还原才能恢复）。
    // 参考: https://github.com/tauri-apps/tauri/issues/9394
    //
    // CI-35: both workarounds cost GPU compositing. They stay on by default
    // (the reports above come from native installs, not only the AppImage),
    // but a user whose WebKitGTK renders fine can opt out with
    // CC_SWITCH_WEBKIT_GPU=1; an explicitly set WEBKIT_* variable always wins.
    #[cfg(target_os = "linux")]
    {
        let gpu_opt_in = std::env::var("CC_SWITCH_WEBKIT_GPU").ok();
        for name in webkit_workarounds(gpu_opt_in.as_deref()) {
            if std::env::var_os(name).is_none() {
                std::env::set_var(name, "1");
            }
        }

        // AppImage 的 GTK 启动钩子 (linuxdeploy-plugin-gtk.sh) 会无条件
        // `export GDK_BACKEND=x11` 强制走 XWayland，以规避历史上的 Wayland 崩溃
        // (tauri-apps/tauri#8541)。但在较新的 Wayland + NVIDIA 环境下，强制 XWayland
        // 反而使 WebKitGTK 的 webview 收不到指针事件（标题栏可点、网页内容点不动），
        // resize 后黑屏；改回原生 Wayland 即可解决，且该崩溃在 WebKitGTK 2.52 上已不复现。
        // 由于该钩子会覆盖用户预设的 GDK_BACKEND，这里提供一个钩子不会触碰的逃生开关：
        // 设置 CC_SWITCH_GDK_BACKEND=wayland 即可强制覆盖，默认行为保持不变（零回归）。
        if let Ok(backend) = std::env::var("CC_SWITCH_GDK_BACKEND") {
            if !backend.is_empty() {
                std::env::set_var("GDK_BACKEND", backend);
            }
        }
    }

    // Hold the database writer lock while the app runs so `ccs` write
    // commands refuse to modify live files underneath it (PRODUCT-11).
    cc_switch_lib::cli::acquire_gui_writer_lock();

    cc_switch_lib::run();
}

/// The WebKitGTK workaround variables to default to `1` on Linux (CI-35).
/// `CC_SWITCH_WEBKIT_GPU=1` (or `true`/`on`/`yes`) keeps WebKitGTK's GPU paths.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn webkit_workarounds(gpu_opt_in: Option<&str>) -> &'static [&'static str] {
    let keep_gpu = matches!(
        gpu_opt_in.map(|v| v.trim().to_ascii_lowercase()).as_deref(),
        Some("1" | "true" | "on" | "yes")
    );
    if keep_gpu {
        &[]
    } else {
        &[
            "WEBKIT_DISABLE_DMABUF_RENDERER",
            "WEBKIT_DISABLE_COMPOSITING_MODE",
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::webkit_workarounds;

    #[test]
    fn webkit_workarounds_default_on_and_can_be_opted_out() {
        let all = [
            "WEBKIT_DISABLE_DMABUF_RENDERER",
            "WEBKIT_DISABLE_COMPOSITING_MODE",
        ];
        assert_eq!(webkit_workarounds(None), all);
        assert_eq!(webkit_workarounds(Some("")), all);
        assert_eq!(webkit_workarounds(Some("0")), all);
        for on in ["1", "true", "ON", " yes "] {
            assert!(webkit_workarounds(Some(on)).is_empty(), "{on}");
        }
    }
}
