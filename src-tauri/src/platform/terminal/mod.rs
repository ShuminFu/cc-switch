//! 统一的终端启动栈：provider 专属终端（`open_provider_terminal`）、可信命令终端
//! （`launch_terminal_running`）与会话恢复（`launch_session_terminal`）都从这里拉起外部终端，
//! 共用私有临时文件（`tempfiles`）、shell 引号（`platform::shell`）与各平台的首选终端选择。

mod posix;
mod resume;
mod tempfiles;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

pub(crate) use resume::launch_resume_terminal;

use std::path::{Path, PathBuf};
use tempfiles::{private_launch_temp_dir, sanitize_temp_file_component, write_claude_config};

pub(crate) fn resolve_launch_cwd(cwd: Option<String>) -> Result<Option<PathBuf>, String> {
    let Some(raw_path) = cwd.filter(|value| !value.trim().is_empty()) else {
        return Ok(None);
    };

    if raw_path.contains('\n') || raw_path.contains('\r') {
        return Err("目录路径包含非法换行符".to_string());
    }

    let path = Path::new(&raw_path);
    if !path.exists() {
        return Err(format!("目录不存在: {raw_path}"));
    }

    let resolved = std::fs::canonicalize(path).map_err(|e| format!("解析目录失败: {e}"))?;
    if !resolved.is_dir() {
        return Err(format!("选择的路径不是文件夹: {}", resolved.display()));
    }

    // Strip Windows extended-length prefix that canonicalize produces,
    // as it can break batch scripts and other shell commands.
    // Special-case \\?\UNC\server\share -> \\server\share for network/WSL paths.
    #[cfg(target_os = "windows")]
    let resolved = {
        let s = resolved.to_string_lossy();
        if let Some(unc) = s.strip_prefix(r"\\?\UNC\") {
            PathBuf::from(format!(r"\\{unc}"))
        } else if let Some(stripped) = s.strip_prefix(r"\\?\") {
            PathBuf::from(stripped)
        } else {
            resolved
        }
    };

    Ok(Some(resolved))
}

/// 创建临时配置文件并启动 claude 终端
/// 使用 --settings 参数传入提供商特定的 API 配置
pub(crate) fn launch_terminal_with_env(
    env_vars: Vec<(String, String)>,
    provider_id: &str,
    cwd: Option<&Path>,
) -> Result<(), String> {
    let temp_dir = private_launch_temp_dir();

    // 创建并写入配置文件（0600、随机文件名、O_EXCL）
    let config_file = write_claude_config(&temp_dir, provider_id, &env_vars)?;

    #[cfg(target_os = "macos")]
    {
        macos::launch_provider_terminal(&config_file, cwd)
    }

    #[cfg(target_os = "linux")]
    {
        linux::launch_provider_terminal(&config_file, cwd)
    }

    #[cfg(target_os = "windows")]
    {
        windows::launch_provider_terminal(&temp_dir, &config_file, cwd)
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        let _ = (config_file, cwd);
        Err("不支持的操作系统".to_string())
    }
}

/// 打开用户首选终端并在其中执行一段可信命令脚本。脚本尾部 `read -r` / `pause`
/// 是刻意设计的——让命令退出后窗口不要瞬间关闭，用户才看得到 `command
/// not found` / `ModuleNotFoundError` 这类诊断信息。
///
/// **Security**：`command_line` 会被原样拼进 shell/batch 脚本，调用方必须
/// 保证它是可信字符串（当前只由后端硬编码调用）。
pub(crate) fn launch_terminal_running(command_line: &str, label: &str) -> Result<(), String> {
    let temp_dir = private_launch_temp_dir();
    let file_prefix = format!("cc_switch_{}_", sanitize_temp_file_component(label));

    #[cfg(target_os = "macos")]
    {
        macos::launch_running_terminal(&temp_dir, &file_prefix, command_line, label)
    }

    #[cfg(target_os = "linux")]
    {
        linux::launch_running_terminal(&temp_dir, &file_prefix, command_line, label)
    }

    #[cfg(target_os = "windows")]
    {
        windows::launch_running_terminal(&temp_dir, &file_prefix, command_line, label)
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        let _ = (temp_dir, file_prefix, command_line, label);
        Err("不支持的操作系统".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_launch_cwd_accepts_existing_directory() {
        let resolved =
            resolve_launch_cwd(Some(std::env::temp_dir().to_string_lossy().into_owned()))
                .expect("temp dir should resolve")
                .expect("temp dir should be present");

        assert!(resolved.is_dir());
    }

    #[test]
    fn resolve_launch_cwd_rejects_missing_directory() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock should be after epoch")
            .as_nanos();
        let missing = std::env::temp_dir().join(format!("cc-switch-missing-{unique}"));

        let error = resolve_launch_cwd(Some(missing.to_string_lossy().into_owned()))
            .expect_err("missing directory should fail");

        assert!(error.contains("目录不存在"));
    }
}
