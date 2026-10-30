//! Windows 终端：写私有 `.bat` 后经 `cmd /C start` 交给 cmd / PowerShell / Windows Terminal。

use super::resume::build_windows_resume_batch;
use super::tempfiles::{private_launch_temp_dir, write_private_temp_file};
use crate::platform::process::{decode_command_output, CREATE_NO_WINDOW};
use crate::platform::shell::{
    build_windows_cwd_command_str, escape_windows_batch_value, powershell_single_quote,
};
use std::os::windows::process::CommandExt;
use std::path::Path;

/// Windows: provider 终端——写出 `.bat` 后交给首选终端
pub(super) fn launch_provider_terminal(
    temp_dir: &Path,
    config_file: &Path,
    cwd: Option<&Path>,
) -> Result<(), String> {
    let config_path_for_batch = escape_windows_batch_value(&config_file.to_string_lossy());
    let cwd_command = build_windows_cwd_command(cwd);

    let content = format!(
        "@echo off
{cwd_command}
echo Using provider-specific claude config:
echo {}
claude --settings \"{}\"
del \"{}\" >nul 2>&1
del \"%~f0\" >nul 2>&1
",
        config_path_for_batch,
        config_path_for_batch,
        config_path_for_batch,
        cwd_command = cwd_command,
    );

    let bat_file = write_private_temp_file(
        temp_dir,
        "cc_switch_claude_",
        ".bat",
        content.as_bytes(),
        false,
    )
    .map_err(|e| {
        let _ = std::fs::remove_file(config_file);
        format!("写入批处理文件失败: {e}")
    })?;

    start_bat_in_preferred_terminal(&bat_file)
}

/// Windows: 在首选终端中执行一段可信命令（见 `launch_terminal_running`）
pub(super) fn launch_running_terminal(
    temp_dir: &Path,
    file_prefix: &str,
    command_line: &str,
    label: &str,
) -> Result<(), String> {
    let content = format!(
        "@echo off\r\necho [cc-switch] Starting: {label}\r\necho.\r\n{cmd}\r\necho.\r\necho [cc-switch] Command exited. Press any key to close.\r\npause >nul\r\ndel \"%~f0\" >nul 2>&1\r\n",
        label = label,
        cmd = command_line,
    );
    let bat_file =
        write_private_temp_file(temp_dir, file_prefix, ".bat", content.as_bytes(), false)
            .map_err(|e| format!("写入批处理文件失败: {e}"))?;

    let result = start_bat_in_preferred_terminal(&bat_file);

    // The .bat self-deletes (`del "%~f0"`) after it runs, but that only
    // fires if *some* terminal actually launched it. If every attempt
    // failed, sweep the temp file ourselves to avoid pollution.
    if result.is_err() {
        let _ = std::fs::remove_file(&bat_file);
    }
    result
}

/// Windows: 会话恢复——写出 resume `.bat` 后交给首选终端（见 `launch_resume_terminal`）
pub(super) fn launch_resume_terminal(argv: &[String], cwd: Option<&Path>) -> Result<(), String> {
    let cwd = cwd.map(|dir| dir.to_string_lossy().into_owned());
    let content = build_windows_resume_batch(argv, cwd.as_deref())?;
    let bat_file = write_private_temp_file(
        &private_launch_temp_dir(),
        "cc_switch_resume_",
        ".bat",
        content.as_bytes(),
        false,
    )
    .map_err(|e| format!("写入批处理文件失败: {e}"))?;

    let result = start_bat_in_preferred_terminal(&bat_file);
    if result.is_err() {
        let _ = std::fs::remove_file(&bat_file);
    }
    result
}

/// Windows: 根据用户首选终端运行 `.bat`，首选终端失败时回退到 cmd
fn start_bat_in_preferred_terminal(bat_file: &Path) -> Result<(), String> {
    let preferred = crate::settings::get_preferred_terminal();
    let terminal = preferred.as_deref().unwrap_or("cmd");

    let bat_path = bat_file.to_string_lossy();
    let ps_cmd = format!("& {}", powershell_single_quote(&bat_path));

    // Try the preferred terminal first
    let result = match terminal {
        "powershell" => run_windows_start_command(
            &["powershell", "-NoExit", "-Command", &ps_cmd],
            "PowerShell",
        ),
        "wt" => run_windows_start_command(&["wt", "cmd", "/K", &bat_path], "Windows Terminal"),
        _ => run_windows_start_command(&["cmd", "/K", &bat_path], "cmd"), // "cmd" or default
    };

    // If preferred terminal fails and it's not the default, try cmd as fallback
    if result.is_err() && terminal != "cmd" {
        log::warn!(
            "首选终端 {} 启动失败，回退到 cmd: {:?}",
            terminal,
            result.as_ref().err()
        );
        return run_windows_start_command(&["cmd", "/K", &bat_path], "cmd");
    }

    result
}

fn build_windows_cwd_command(cwd: Option<&Path>) -> String {
    cwd.map(|dir| build_windows_cwd_command_str(&dir.to_string_lossy()))
        .unwrap_or_default()
}

/// Windows: Run a start command with common error handling
fn run_windows_start_command(args: &[&str], terminal_name: &str) -> Result<(), String> {
    use std::process::Command;

    let mut full_args = vec!["/C", "start"];
    full_args.extend(args);

    let output = Command::new("cmd")
        .args(&full_args)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("启动 {} 失败: {e}", terminal_name))?;

    if !output.status.success() {
        let stderr = decode_command_output(&output.stderr);
        return Err(format!(
            "{} 启动失败 (exit code: {:?}): {}",
            terminal_name,
            output.status.code(),
            stderr
        ));
    }

    Ok(())
}
