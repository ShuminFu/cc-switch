//! Linux 终端：按首选终端 + 常见终端列表依次尝试，`<terminal> <args> sh <script>`。

use super::posix::{build_running_script, get_user_shell, write_provider_launcher};
use super::resume::build_posix_resume_script;
use super::tempfiles::{private_launch_temp_dir, write_private_temp_file};
use std::path::Path;
use std::process::Command;

/// Linux: provider 终端——写出启动脚本后交给首选终端；全部失败时清理脚本与配置文件
pub(super) fn launch_provider_terminal(
    config_file: &Path,
    cwd: Option<&Path>,
) -> Result<(), String> {
    let script_file = write_provider_launcher(config_file, cwd)?;

    let result = spawn_script_in_preferred_terminal(&script_file);
    if result.is_err() {
        // Clean up on failure
        let _ = std::fs::remove_file(&script_file);
        let _ = std::fs::remove_file(config_file);
    }
    result
}

/// Linux: 在首选终端中执行一段可信命令（见 `launch_terminal_running`）
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

    let result = spawn_script_in_preferred_terminal(&script_file);
    if result.is_err() {
        let _ = std::fs::remove_file(&script_file);
    }
    result
}

/// Linux: 会话恢复——写出 resume 启动脚本后交给首选终端（见 `launch_resume_terminal`）
pub(super) fn launch_resume_terminal(command: &str, cwd: Option<&Path>) -> Result<(), String> {
    let script_content = build_posix_resume_script(&get_user_shell(), command, cwd);
    let script_file = write_private_temp_file(
        &private_launch_temp_dir(),
        "cc_switch_resume_",
        ".sh",
        script_content.as_bytes(),
        true,
    )
    .map_err(|e| format!("写入启动脚本失败: {e}"))?;

    let result = spawn_script_in_preferred_terminal(&script_file);
    if result.is_err() {
        let _ = std::fs::remove_file(&script_file);
    }
    result
}

/// Linux: 根据用户首选终端启动脚本，首选终端不可用时依次尝试默认终端列表
fn spawn_script_in_preferred_terminal(script_file: &Path) -> Result<(), String> {
    let preferred = crate::settings::get_preferred_terminal();

    // Default terminal list with their arguments
    let default_terminals = [
        ("gnome-terminal", vec!["--"]),
        ("konsole", vec!["-e"]),
        ("xfce4-terminal", vec!["-e"]),
        ("mate-terminal", vec!["--"]),
        ("lxterminal", vec!["-e"]),
        ("alacritty", vec!["-e"]),
        ("kitty", vec!["-e"]),
        ("ghostty", vec!["-e"]),
    ];

    // Build terminal list: preferred terminal first (if specified), then defaults
    let terminals_to_try: Vec<(&str, Vec<&str>)> = if let Some(ref pref) = preferred {
        // Find the preferred terminal's args from default list
        let pref_args = default_terminals
            .iter()
            .find(|(name, _)| *name == pref.as_str())
            .map(|(_, args)| args.to_vec())
            .unwrap_or_else(|| vec!["-e"]); // Default args for unknown terminals

        let mut list = vec![(pref.as_str(), pref_args)];
        // Add remaining terminals as fallbacks
        for (name, args) in &default_terminals {
            if *name != pref.as_str() {
                list.push((*name, args.to_vec()));
            }
        }
        list
    } else {
        default_terminals
            .iter()
            .map(|(name, args)| (*name, args.to_vec()))
            .collect()
    };

    let mut last_error = String::from("未找到可用的终端");

    for (terminal, args) in terminals_to_try {
        // Check if terminal exists in common paths
        let terminal_exists = ["/usr/bin", "/bin", "/usr/local/bin"]
            .iter()
            .any(|dir| Path::new(&format!("{dir}/{terminal}")).exists())
            || which_command(terminal);

        if terminal_exists {
            let result = Command::new(terminal)
                .args(&args)
                .arg("sh")
                .arg(script_file.to_string_lossy().as_ref())
                .spawn();

            match result {
                Ok(_) => return Ok(()),
                Err(e) => {
                    last_error = format!("执行 {} 失败: {}", terminal, e);
                }
            }
        }
    }

    Err(last_error)
}

/// Check if a command exists using `which`
fn which_command(cmd: &str) -> bool {
    Command::new("which")
        .arg(cmd)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}
