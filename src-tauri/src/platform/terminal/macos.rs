//! macOS 终端：Terminal.app / iTerm2 / Ghostty（AppleScript）、Warp（URI scheme）
//! 与 Alacritty / Kitty / WezTerm / Kaku（`open -na ... --args`）。

use super::applescript::{
    build_macos_dash_c_command, build_macos_ghostty_applescript, build_macos_iterm2_applescript,
    build_macos_terminal_applescript,
};
use super::posix::{build_running_script, write_provider_launcher};
use super::tempfiles::write_private_temp_file;
use crate::platform::process::decode_command_output;
use crate::platform::shell::shell_single_quote;
use std::path::Path;

/// macOS: provider 终端——写出启动脚本后交给用户首选终端
pub(super) fn launch_provider_terminal(
    config_file: &Path,
    cwd: Option<&Path>,
) -> Result<(), String> {
    let script_file = write_provider_launcher(config_file, cwd)?;
    launch_script_in_preferred_terminal(&script_file)
}

/// macOS: 在首选终端中执行一段可信命令（见 `launch_terminal_running`）
pub(super) fn launch_running_terminal(
    temp_dir: &Path,
    file_prefix: &str,
    command_line: &str,
    label: &str,
) -> Result<(), String> {
    let script_content = build_running_script(command_line, label);
    let script_file = write_private_temp_file(
        temp_dir,
        file_prefix,
        ".sh",
        script_content.as_bytes(),
        true,
    )
    .map_err(|e| format!("写入启动脚本失败: {e}"))?;

    launch_script_in_preferred_terminal(&script_file)
}

/// macOS: 根据用户首选终端执行启动脚本
fn launch_script_in_preferred_terminal(script_file: &Path) -> Result<(), String> {
    let preferred = crate::settings::get_preferred_terminal();
    let terminal = preferred.as_deref().unwrap_or("terminal");

    // Try the preferred terminal first, fall back to Terminal.app if it fails
    // Note: Kitty doesn't need the -e flag, others do
    let result = match terminal {
        "iterm2" => launch_macos_iterm2(script_file),
        "warp" => launch_macos_warp(script_file),
        "alacritty" => launch_macos_open_app("Alacritty", script_file, true),
        "kitty" => launch_macos_open_app("kitty", script_file, false),
        "ghostty" => launch_macos_ghostty(script_file),
        "wezterm" => launch_macos_open_app("WezTerm", script_file, true),
        "kaku" => launch_macos_open_app("Kaku", script_file, true),
        _ => launch_macos_terminal_app(script_file),
    };

    // If preferred terminal fails and it's not the default, try Terminal.app as fallback
    if result.is_err() && terminal != "terminal" {
        log::warn!(
            "首选终端 {} 启动失败，回退到 Terminal.app: {:?}",
            terminal,
            result.as_ref().err()
        );
        return launch_macos_terminal_app(script_file);
    }

    result
}

/// Run AppleScript through `osascript -e` with shared error handling.
fn run_terminal_osascript(applescript: &str, terminal_label: &str) -> Result<(), String> {
    use std::process::Command;

    let output = Command::new("osascript")
        .arg("-e")
        .arg(applescript)
        .output()
        .map_err(|e| format!("执行 osascript 失败: {e}"))?;

    if !output.status.success() {
        let stderr = decode_command_output(&output.stderr);
        return Err(format!(
            "{terminal_label} 执行失败 (exit code: {:?}): {}",
            output.status.code(),
            stderr
        ));
    }

    Ok(())
}

/// macOS: Terminal.app
fn launch_macos_terminal_app(script_file: &std::path::Path) -> Result<(), String> {
    run_terminal_osascript(
        &build_macos_terminal_applescript(script_file),
        "Terminal.app",
    )
}

/// macOS: iTerm2
fn launch_macos_iterm2(script_file: &std::path::Path) -> Result<(), String> {
    run_terminal_osascript(&build_macos_iterm2_applescript(script_file), "iTerm2")
}

/// macOS: Ghostty
fn launch_macos_ghostty(script_file: &std::path::Path) -> Result<(), String> {
    match run_terminal_osascript(&build_macos_ghostty_applescript(script_file), "Ghostty") {
        Ok(()) => Ok(()),
        Err(applescript_error) => {
            log::warn!(
                "Ghostty AppleScript launch failed, falling back to open -na: {applescript_error}"
            );
            launch_macos_open_app("Ghostty", script_file, true)
        }
    }
}

/// macOS: 使用 open -na 启动支持 --args 参数的终端（Alacritty/Kitty/WezTerm/Kaku）
fn launch_macos_open_app(
    app_name: &str,
    script_file: &std::path::Path,
    use_e_flag: bool,
) -> Result<(), String> {
    use std::process::Command;

    let mut cmd = Command::new("open");
    cmd.arg("-na").arg(app_name).arg("--args");

    if use_e_flag {
        cmd.arg("-e");
    }
    // Keep the script path inside `sh -c`; a trailing bare `.sh` can be opened as a document.
    cmd.arg("sh")
        .arg("-c")
        .arg(build_macos_dash_c_command(script_file));

    let output = cmd
        .output()
        .map_err(|e| format!("启动 {app_name} 失败: {e}"))?;

    if !output.status.success() {
        let stderr = decode_command_output(&output.stderr);
        return Err(format!(
            "{} 启动失败 (exit code: {:?}): {}",
            app_name,
            output.status.code(),
            stderr
        ));
    }

    Ok(())
}

fn launch_macos_warp(script_file: &std::path::Path) -> Result<(), String> {
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;

    let mut cmd = Command::new("open");
    cmd.arg("-a").arg("Warp");

    // Warp URI scheme cannot work well with script_file, because:
    //
    // 1. script_file's name ends up with .sh, so Warp would open the file rather than execute it
    // 2. script_file has no execution permission, so we need to add one more indirection
    let mut second_script_file = tempfile::Builder::new()
        .disable_cleanup(true)
        .permissions(std::fs::Permissions::from_mode(0o755))
        .tempfile()
        .map_err(|e| format!("Failed to create temporary script file: {e}"))?;

    writeln!(
        &mut second_script_file,
        r#"#!/usr/bin/env sh

        rm -- "$0"

        exec sh {quoted_script}
        "#,
        quoted_script = shell_single_quote(&script_file.to_string_lossy()),
    )
    .map_err(|e| format!("Failed to write to temporary script file for Warp: {e}"))?;

    let mut warp_url = url::Url::parse("warp://action/new_tab").unwrap();
    warp_url
        .query_pairs_mut()
        .append_pair("path", &second_script_file.path().to_string_lossy());
    let warp_url = warp_url.to_string();
    cmd.arg(warp_url);

    let output = cmd.output().map_err(|e| format!("启动 Warp 失败: {e}"))?;
    if !output.status.success() {
        let stderr = decode_command_output(&output.stderr);
        return Err(format!(
            "Warp 启动失败 (exit code: {:?}): {}",
            output.status.code(),
            stderr
        ));
    }

    Ok(())
}
