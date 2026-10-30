//! macOS / Linux 共用的 provider 启动脚本与用户 shell 解析。

use super::tempfiles::{private_launch_temp_dir, write_private_temp_file};
use crate::platform::shell::{is_valid_shell, shell_single_quote};
use std::path::{Path, PathBuf};

// 以下 shell 解析辅助函数仅被 macOS/Linux 的终端启动逻辑使用；Windows 非 test 编译下为死代码。
#[cfg_attr(windows, allow(dead_code))]
pub(super) fn fallback_user_shell() -> &'static str {
    if cfg!(target_os = "macos") {
        "/bin/zsh"
    } else {
        "/bin/bash"
    }
}

#[cfg_attr(windows, allow(dead_code))]
pub(super) fn valid_user_shell_path(shell: &str) -> bool {
    if shell.is_empty()
        || !shell.starts_with('/')
        || !is_valid_shell(shell)
        || shell.chars().any(char::is_control)
    {
        return false;
    }

    let path = std::path::Path::new(shell);
    path.is_file() && is_executable_file(path)
}

#[cfg(unix)]
pub(super) fn is_executable_file(path: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    path.metadata()
        .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
#[cfg_attr(windows, allow(dead_code))]
pub(super) fn is_executable_file(path: &std::path::Path) -> bool {
    path.is_file()
}

/// 获取用户默认 shell 的完整路径；异常或被污染的 SHELL 回退到平台默认值。
#[cfg_attr(windows, allow(dead_code))]
pub(super) fn get_user_shell() -> String {
    std::env::var("SHELL")
        .ok()
        .filter(|shell| valid_user_shell_path(shell))
        .unwrap_or_else(|| fallback_user_shell().to_string())
}

/// 构建 exec 行：引号保护 shell 路径，交还用户 shell 让其按默认规则加载 rc 配置。
#[cfg_attr(windows, allow(dead_code))]
pub(super) fn build_exec_line(shell: &str, cwd: Option<&Path>) -> String {
    let quoted_shell = shell_single_quote(shell);

    match shell.rsplit('/').next().unwrap_or(shell) {
        "zsh" => cwd
            .map(|dir| {
                let command = format!(
                    "cd {} || exit 1; exec {} -i",
                    shell_single_quote(&dir.to_string_lossy()),
                    quoted_shell
                );
                format!("exec {} -lc {}", quoted_shell, shell_single_quote(&command))
            })
            .unwrap_or_else(|| format!("exec {quoted_shell} -l")),
        _ => format!("exec {quoted_shell}"),
    }
}

/// 构建 provider 命令行：通过用户 shell 的交互模式执行，确保 GUI 启动的终端也加载用户 PATH。
#[cfg_attr(windows, allow(dead_code))]
pub(super) fn build_provider_command_line(
    shell: &str,
    config_path: &str,
    cwd: Option<&Path>,
) -> String {
    let claude_command = format!("claude --settings {}", shell_single_quote(config_path));
    let command = cwd
        .map(|dir| {
            format!(
                "cd {} && {}",
                shell_single_quote(&dir.to_string_lossy()),
                claude_command
            )
        })
        .unwrap_or(claude_command);

    format!(
        "{} {} {}",
        shell_single_quote(shell),
        provider_command_flag_for_shell(shell),
        shell_single_quote(&command)
    )
}

#[cfg_attr(windows, allow(dead_code))]
pub(super) fn provider_command_flag_for_shell(shell: &str) -> &'static str {
    match shell.rsplit('/').next().unwrap_or(shell) {
        "dash" | "sh" => "-c",
        "zsh" => "-lic",
        _ => "-ic",
    }
}

#[cfg_attr(windows, allow(dead_code))]
pub(super) fn build_final_shell_cd_command(shell: &str, cwd: Option<&Path>) -> String {
    if matches!(shell.rsplit('/').next().unwrap_or(shell), "zsh") {
        return String::new();
    }

    cwd.map(|dir| {
        format!(
            "cd {} || exit 1\n",
            shell_single_quote(&dir.to_string_lossy())
        )
    })
    .unwrap_or_default()
}

/// 构建 provider 启动脚本（POSIX sh）。
/// 脚本末尾的 `exec` 会替换当前进程，EXIT trap 不会触发，
/// 因此在 claude 退出后、exec 之前显式删除配置文件与脚本自身（`$0`，脚本总以 `sh <path>` 方式执行）；
/// trap 仅兜底提前退出的情况。
#[cfg_attr(windows, allow(dead_code))]
pub(super) fn build_provider_launcher_script(
    config_path: &str,
    provider_command: &str,
    final_cd_command: &str,
    exec_line: &str,
) -> String {
    format!(
        r#"#!/usr/bin/env sh
cc_switch_config={config}
trap 'rm -f "$cc_switch_config" "$0"' EXIT
echo "Using provider-specific claude config:"
echo "$cc_switch_config"
{provider_command}
rm -f "$cc_switch_config" "$0"
{final_cd_command}
{exec_line}
"#,
        config = shell_single_quote(config_path),
    )
}

/// 为 provider 终端写出私有启动脚本（macOS / Linux 共用）：脚本运行 `claude --settings`，
/// 退出后删除配置文件并切回用户交互式 shell。写入失败时一并删除配置文件。
#[cfg_attr(windows, allow(dead_code))]
pub(super) fn write_provider_launcher(
    config_file: &Path,
    cwd: Option<&Path>,
) -> Result<PathBuf, String> {
    let shell = get_user_shell();
    let exec_line = build_exec_line(&shell, cwd);
    let final_cd_command = build_final_shell_cd_command(&shell, cwd);

    let config_path = config_file.to_string_lossy();
    let provider_command = build_provider_command_line(&shell, &config_path, cwd);

    // 脚本使用 POSIX sh 语法确保可移植性，exec 行切换到用户交互式 shell
    let script_content = build_provider_launcher_script(
        &config_path,
        &provider_command,
        &final_cd_command,
        &exec_line,
    );
    write_private_temp_file(
        &private_launch_temp_dir(),
        "cc_switch_launcher_",
        ".sh",
        script_content.as_bytes(),
        true,
    )
    .map_err(|e| {
        let _ = std::fs::remove_file(config_file);
        format!("写入启动脚本失败: {e}")
    })
}

/// 构建 `launch_terminal_running` 的 POSIX sh 脚本。
/// 脚本总以 `sh <path>` 方式执行，`$0` 即脚本自身路径。
#[cfg_attr(windows, allow(dead_code))]
pub(super) fn build_running_script(command_line: &str, label: &str) -> String {
    format!(
        r#"#!/usr/bin/env sh
trap 'rm -f "$0"' EXIT
echo "[cc-switch] Starting: {label}"
echo ""
{cmd}
echo ""
echo "[cc-switch] Command exited. Press Enter to close."
read -r _
"#,
        label = label,
        cmd = command_line,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[cfg(unix)]
    fn set_test_executable(path: &Path, executable: bool) {
        use std::os::unix::fs::PermissionsExt;

        let mode = if executable { 0o755 } else { 0o644 };
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
            .expect("fixture permissions should be set");
    }

    #[test]
    fn test_build_exec_line() {
        assert_eq!(build_exec_line("/bin/zsh", None), "exec '/bin/zsh' -l");
        assert_eq!(build_exec_line("/bin/bash", None), "exec '/bin/bash'");
        assert_eq!(
            build_exec_line("/opt/homebrew dir/bin/fish", None),
            "exec '/opt/homebrew dir/bin/fish'"
        );
        assert_eq!(build_exec_line("/bin/sh", None), "exec '/bin/sh'");
        assert_eq!(
            build_exec_line("/tmp/shell'quote/zsh", None),
            "exec '/tmp/shell'\"'\"'quote/zsh' -l"
        );
        assert_eq!(
            build_exec_line("/bin/zsh", Some(Path::new("/tmp/project"))),
            r#"exec '/bin/zsh' -lc 'cd '"'"'/tmp/project'"'"' || exit 1; exec '"'"'/bin/zsh'"'"' -i'"#
        );
    }

    #[test]
    fn test_build_provider_command_line_uses_user_shell_environment() {
        assert_eq!(
            build_provider_command_line("/bin/zsh", "/tmp/claude config.json", None),
            "'/bin/zsh' -lic 'claude --settings '\"'\"'/tmp/claude config.json'\"'\"''"
        );
        assert_eq!(
            build_provider_command_line(
                "/bin/bash",
                "/tmp/claude config.json",
                Some(Path::new("/tmp/project"))
            ),
            r#"'/bin/bash' -ic 'cd '"'"'/tmp/project'"'"' && claude --settings '"'"'/tmp/claude config.json'"'"''"#
        );
        assert_eq!(
            build_provider_command_line(
                "/bin/sh",
                "/tmp/claude config.json",
                Some(Path::new("/tmp/project O'Brien"))
            ),
            r#"'/bin/sh' -c 'cd '"'"'/tmp/project O'"'"'"'"'"'"'"'"'Brien'"'"' && claude --settings '"'"'/tmp/claude config.json'"'"''"#
        );
    }

    /// macOS / Linux 的 `launch_terminal_running` 共用这一份脚本：trap 兜底删除脚本自身，
    /// 命令退出后 `read -r` 保留窗口供用户查看诊断信息。
    #[test]
    fn running_script_runs_command_then_waits_and_self_deletes() {
        let script = build_running_script("hermes dashboard", "hermes_dashboard");

        assert!(script.starts_with("#!/usr/bin/env sh\ntrap 'rm -f \"$0\"' EXIT\n"));
        let banner = script
            .find("echo \"[cc-switch] Starting: hermes_dashboard\"")
            .unwrap();
        let run = script.find("\nhermes dashboard\n").unwrap();
        let wait = script.find("read -r _").unwrap();
        assert!(banner < run && run < wait, "{script}");
    }

    #[test]
    fn test_build_final_shell_cd_command() {
        assert_eq!(build_final_shell_cd_command("/bin/zsh", None), "");
        assert_eq!(
            build_final_shell_cd_command("/bin/zsh", Some(Path::new("/tmp/project"))),
            ""
        );
        assert_eq!(
            build_final_shell_cd_command("/bin/bash", Some(Path::new("/tmp/project O'Brien"))),
            "cd '/tmp/project O'\"'\"'Brien' || exit 1\n"
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_get_user_shell_fallback() {
        // $SHELL 未设置时应按平台 fallback
        // 此测试验证 fallback 逻辑，但不验证环境变量值（取决于运行环境）
        let shell = get_user_shell();
        // 至少应返回一个合法的绝对路径
        assert!(valid_user_shell_path(&shell));
        // basename 应为合法 shell 名
        let basename = shell.rsplit('/').next().unwrap_or("sh");
        assert!(["sh", "bash", "zsh", "fish", "dash"].contains(&basename));
    }

    #[cfg(unix)]
    #[test]
    fn test_valid_user_shell_path() {
        let temp = tempfile::tempdir().expect("temp dir should be created");
        let executable_zsh = temp.path().join("zsh");
        std::fs::write(&executable_zsh, "#!/usr/bin/env sh\n")
            .expect("shell fixture should be written");
        set_test_executable(&executable_zsh, true);

        let executable_fish_dir = temp.path().join("homebrew dir/bin");
        std::fs::create_dir_all(&executable_fish_dir)
            .expect("shell fixture directory should be created");
        let executable_fish = executable_fish_dir.join("fish");
        std::fs::write(&executable_fish, "#!/usr/bin/env sh\n")
            .expect("shell fixture should be written");
        set_test_executable(&executable_fish, true);

        let non_executable_bash = temp.path().join("bash");
        std::fs::write(&non_executable_bash, "#!/usr/bin/env sh\n")
            .expect("shell fixture should be written");
        set_test_executable(&non_executable_bash, false);

        assert!(valid_user_shell_path(&executable_zsh.to_string_lossy()));
        assert!(valid_user_shell_path(&executable_fish.to_string_lossy()));
        assert!(!valid_user_shell_path(""));
        assert!(!valid_user_shell_path("zsh"));
        assert!(!valid_user_shell_path(
            &temp.path().join("missing/zsh").to_string_lossy()
        ));
        assert!(!valid_user_shell_path(
            &non_executable_bash.to_string_lossy()
        ));
        assert!(!valid_user_shell_path(
            &temp.path().join("zsh; rm -rf /").to_string_lossy()
        ));
        assert!(!valid_user_shell_path(&format!(
            "{}\n/bin/bash",
            executable_zsh.to_string_lossy()
        )));
        assert!(!valid_user_shell_path("/usr/bin/powershell"));
    }
}
