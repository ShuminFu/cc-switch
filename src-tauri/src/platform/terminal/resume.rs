//! 会话恢复终端（`launch_session_terminal`）：在用户首选终端中执行后端生成的 resume 命令。
//! 命令与工作目录由 `session_manager::resolve_resume_target` 按固定模板生成（CCS-074）。
//! macOS 走下方按 target 区分的 AppleScript / `open` 启动器；Linux 写出 POSIX sh 启动脚本、
//! Windows 写出 `.bat`，再交给与 provider 终端相同的首选终端选择逻辑（CCS-184）。

// macOS 专属启动器在 Linux/Windows 非 test 编译下为死代码；脚本生成函数三平台都参与单测。
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use super::posix::{build_exec_line, build_final_shell_cd_command, build_user_shell_command_line};
use crate::platform::shell::{build_windows_cwd_command_str, shell_single_quote};
use crate::session_manager::ResumeTarget;
use std::path::Path;
use std::process::Command;

pub(crate) fn launch_resume_terminal(
    target: &str,
    resume: &ResumeTarget,
    custom_config: Option<&str>,
) -> Result<(), String> {
    if resume.command.trim().is_empty() || resume.argv.is_empty() {
        return Err("Resume command is empty".to_string());
    }

    #[cfg(target_os = "macos")]
    {
        launch_macos_resume_terminal(
            target,
            &resume.command,
            resume.cwd.as_deref(),
            custom_config,
        )
    }

    #[cfg(target_os = "linux")]
    {
        let _ = (target, custom_config);
        let cwd = super::resolve_launch_cwd(resume.cwd.clone())?;
        super::linux::launch_resume_terminal(&resume.command, cwd.as_deref())
    }

    #[cfg(target_os = "windows")]
    {
        let _ = (target, custom_config);
        let cwd = super::resolve_launch_cwd(resume.cwd.clone())?;
        super::windows::launch_resume_terminal(&resume.argv, cwd.as_deref())
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        let _ = (target, custom_config);
        Err("Terminal resume is not supported on this operating system".to_string())
    }
}

fn launch_macos_resume_terminal(
    target: &str,
    command: &str,
    cwd: Option<&str>,
    custom_config: Option<&str>,
) -> Result<(), String> {
    match target {
        "terminal" => launch_macos_terminal(command, cwd),
        "iTerm" | "iterm" => launch_iterm(command, cwd),
        "ghostty" => launch_ghostty(command, cwd),
        "kitty" => launch_kitty(command, cwd),
        "wezterm" => launch_wezterm(command, cwd),
        "kaku" => launch_kaku(command, cwd),
        "alacritty" => launch_alacritty(command, cwd),
        #[cfg(unix)]
        "warp" => launch_warp(command, cwd),
        "custom" => launch_custom(command, cwd, custom_config),
        _ => Err(format!("Unsupported terminal target: {target}")),
    }
}

/// Linux（以及任何 POSIX 终端）的会话恢复脚本，总以 `sh <path>` 方式执行：
/// 通过用户 shell 的交互模式运行 resume 命令（加载用户 PATH），退出后删除脚本自身，
/// 再 `exec` 切回用户交互式 shell，让窗口保留在会话目录中。
/// `exec` 会替换进程、EXIT trap 不再触发，因此在 exec 前显式删除；trap 仅兜底提前退出。
pub(super) fn build_posix_resume_script(shell: &str, command: &str, cwd: Option<&Path>) -> String {
    let resume_command = build_user_shell_command_line(shell, command, cwd);
    let final_cd_command = build_final_shell_cd_command(shell, cwd);
    let exec_line = build_exec_line(shell, cwd);

    format!(
        r#"#!/usr/bin/env sh
trap 'rm -f "$0"' EXIT
{resume_command}
rm -f "$0"
{final_cd_command}
{exec_line}
"#
    )
}

/// Windows 会话恢复批处理（交给 `cmd /K` / PowerShell `-NoExit` / Windows Terminal 运行，
/// 命令退出后窗口保留）。`call` 保证 npm 安装的 `claude.cmd` 之类的 shim 返回后继续执行。
///
/// cmd 的参数引用规则无法安全覆盖任意字符（`%`、`!`、`"`、`^` 在批处理与 shim 中会被再次解析），
/// 因此只接受由 `[A-Za-z0-9._-]` 组成的参数；其他 session id 直接拒绝，前端回退为复制命令。
pub(super) fn build_windows_resume_batch(
    argv: &[String],
    cwd: Option<&str>,
) -> Result<String, String> {
    if argv.is_empty() {
        return Err("Resume command is empty".to_string());
    }
    if let Some(arg) = argv.iter().find(|arg| !is_plain_batch_arg(arg)) {
        return Err(format!(
            "Resume argument cannot be passed safely to cmd.exe: {arg}"
        ));
    }

    let cwd_command = cwd
        .filter(|dir| !dir.trim().is_empty())
        .map(build_windows_cwd_command_str)
        .unwrap_or_default();

    Ok(format!(
        "@echo off\r\n{cwd_command}call {command}\r\ndel \"%~f0\" >nul 2>&1\r\n",
        command = argv.join(" "),
    ))
}

fn is_plain_batch_arg(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

fn launch_macos_terminal(command: &str, cwd: Option<&str>) -> Result<(), String> {
    let full_command = build_shell_command(command, cwd);
    let escaped = escape_osascript(&full_command);
    let script = format!(
        r#"tell application "Terminal"
    activate
    do script "{escaped}"
end tell"#
    );

    let status = Command::new("osascript")
        .arg("-e")
        .arg(script)
        .status()
        .map_err(|e| format!("Failed to launch Terminal: {e}"))?;

    if status.success() {
        Ok(())
    } else {
        Err("Terminal command execution failed".to_string())
    }
}

fn launch_iterm(command: &str, cwd: Option<&str>) -> Result<(), String> {
    let full_command = build_shell_command(command, cwd);
    let escaped = escape_osascript(&full_command);
    // iTerm2 AppleScript to create a new window and execute command
    let script = format!(
        r#"tell application "iTerm"
    activate
    create window with default profile
    tell current session of current window
        write text "{escaped}"
    end tell
end tell"#
    );

    let status = Command::new("osascript")
        .arg("-e")
        .arg(script)
        .status()
        .map_err(|e| format!("Failed to launch iTerm: {e}"))?;

    if status.success() {
        Ok(())
    } else {
        Err("iTerm command execution failed".to_string())
    }
}

fn launch_ghostty(command: &str, cwd: Option<&str>) -> Result<(), String> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());

    let mut args = vec![
        "-na".to_string(),
        "Ghostty".to_string(),
        "--args".to_string(),
        "--quit-after-last-window-closed=true".to_string(),
    ];

    if let Some(dir) = cwd {
        if !dir.trim().is_empty() {
            args.push(format!("--working-directory={dir}"));
        }
    }

    args.push("-e".to_string());
    args.push(shell);
    args.push("-l".to_string());
    args.push("-c".to_string());
    args.push(command.to_string());

    let status = Command::new("open")
        .args(&args)
        .status()
        .map_err(|e| format!("Failed to launch Ghostty: {e}"))?;

    if status.success() {
        Ok(())
    } else {
        Err("Failed to launch Ghostty. Make sure it is installed.".to_string())
    }
}

fn launch_kitty(command: &str, cwd: Option<&str>) -> Result<(), String> {
    let full_command = build_shell_command(command, cwd);

    // 获取用户默认 shell
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());

    let status = Command::new("open")
        .arg("-na")
        .arg("kitty")
        .arg("--args")
        .arg("-e")
        .arg(&shell)
        .arg("-l")
        .arg("-c")
        .arg(&full_command)
        .status()
        .map_err(|e| format!("Failed to launch Kitty: {e}"))?;

    if status.success() {
        Ok(())
    } else {
        Err("Failed to launch Kitty. Make sure it is installed.".to_string())
    }
}

fn launch_wezterm(command: &str, cwd: Option<&str>) -> Result<(), String> {
    // wezterm start --cwd ... -- command
    // To invoke via `open`, we use `open -na "WezTerm" --args start ...`
    let args = build_wezterm_compatible_args("WezTerm", command, cwd);

    let status = Command::new("open")
        .args(args.iter().map(String::as_str))
        .status()
        .map_err(|e| format!("Failed to launch WezTerm: {e}"))?;

    if status.success() {
        Ok(())
    } else {
        Err("Failed to launch WezTerm.".to_string())
    }
}

fn launch_kaku(command: &str, cwd: Option<&str>) -> Result<(), String> {
    // Kaku is a WezTerm-derived terminal and keeps a compatible `start` entrypoint.
    let args = build_wezterm_compatible_args("Kaku", command, cwd);

    let status = Command::new("open")
        .args(args.iter().map(String::as_str))
        .status()
        .map_err(|e| format!("Failed to launch Kaku: {e}"))?;

    if status.success() {
        Ok(())
    } else {
        Err("Failed to launch Kaku.".to_string())
    }
}

fn build_wezterm_compatible_args(app_name: &str, command: &str, cwd: Option<&str>) -> Vec<String> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());
    build_wezterm_compatible_args_with_shell(app_name, command, cwd, &shell)
}

fn build_wezterm_compatible_args_with_shell(
    app_name: &str,
    command: &str,
    cwd: Option<&str>,
    shell: &str,
) -> Vec<String> {
    let full_command = build_shell_command(command, None);
    let mut args = vec![
        "-na".to_string(),
        app_name.to_string(),
        "--args".to_string(),
        "start".to_string(),
    ];

    if let Some(dir) = cwd {
        args.push("--cwd".to_string());
        args.push(dir.to_string());
    }

    // Invoke shell to run the command string (to handle pipes, etc)
    args.push("--".to_string());
    args.push(shell.to_string());
    args.push("-c".to_string());
    args.push(full_command);
    args
}

#[cfg(unix)]
fn launch_warp(command: &str, cwd: Option<&str>) -> Result<(), String> {
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;

    let cwd = cwd.ok_or("Failed to resume session without cwd")?;

    let mut script_file = tempfile::Builder::new()
        .disable_cleanup(true)
        .permissions(std::fs::Permissions::from_mode(0o755))
        .tempfile_in(cwd)
        .map_err(|e| format!("Failed to create temporary script file for launching Warp: {e}"))?;

    writeln!(
        &mut script_file,
        r#"#!/usr/bin/env sh

        rm -- "$0"

        exec {command}
        "#,
    )
    .map_err(|e| format!("Failed to write to temporary script file for Warp: {e}"))?;

    let mut warp_url = url::Url::parse("warp://action/new_tab").unwrap();
    warp_url
        .query_pairs_mut()
        .append_pair("path", &script_file.path().to_string_lossy());
    let warp_url = warp_url.to_string();

    let status = Command::new("open")
        .args(["-a", "Warp", &warp_url])
        .status()
        .map_err(|e| format!("Failed to launch Warp: {e}"))?;

    if status.success() {
        Ok(())
    } else {
        Err("Failed to launch Warp.".to_string())
    }
}

fn launch_alacritty(command: &str, cwd: Option<&str>) -> Result<(), String> {
    // Alacritty: open -na Alacritty --args --working-directory ... -e shell -c command
    let full_command = build_shell_command(command, None);
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());

    let mut args = vec!["-na", "Alacritty", "--args"];

    if let Some(dir) = cwd {
        args.push("--working-directory");
        args.push(dir);
    }

    args.push("-e");
    args.push(&shell);
    args.push("-c");
    args.push(&full_command);

    let status = Command::new("open")
        .args(&args)
        .status()
        .map_err(|e| format!("Failed to launch Alacritty: {e}"))?;

    if status.success() {
        Ok(())
    } else {
        Err("Failed to launch Alacritty.".to_string())
    }
}

fn launch_custom(
    command: &str,
    cwd: Option<&str>,
    custom_config: Option<&str>,
) -> Result<(), String> {
    let template = custom_config.ok_or("No custom terminal config provided")?;

    if template.trim().is_empty() {
        return Err("Custom terminal command template is empty".to_string());
    }

    let cmd_str = command;
    let dir_str = cwd.unwrap_or(".");

    let final_cmd_line = template
        .replace("{command}", cmd_str)
        .replace("{cwd}", dir_str);

    // Execute via sh -c
    let status = Command::new("sh")
        .arg("-c")
        .arg(&final_cmd_line)
        .status()
        .map_err(|e| format!("Failed to execute custom terminal launcher: {e}"))?;

    if status.success() {
        Ok(())
    } else {
        Err("Custom terminal execution returned error code".to_string())
    }
}

fn build_shell_command(command: &str, cwd: Option<&str>) -> String {
    match cwd {
        Some(dir) if !dir.trim().is_empty() => {
            format!("cd {} && {}", shell_single_quote(dir), command)
        }
        _ => command.to_string(),
    }
}

fn escape_osascript(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|part| part.to_string()).collect()
    }

    /// Linux：脚本在用户 shell 中进入会话目录执行 resume 命令，删除自身后切回交互式 shell。
    #[test]
    fn posix_resume_script_runs_command_in_session_dir_then_execs_shell() {
        let script = build_posix_resume_script(
            "/bin/bash",
            "claude --resume abc-123",
            Some(Path::new("/tmp/project O'Brien")),
        );

        assert_eq!(
            script,
            concat!(
                "#!/usr/bin/env sh\n",
                "trap 'rm -f \"$0\"' EXIT\n",
                r#"'/bin/bash' -ic 'cd '"'"'/tmp/project O'"'"'"'"'"'"'"'"'Brien'"'"' && claude --resume abc-123'"#,
                "\n",
                "rm -f \"$0\"\n",
                "cd '/tmp/project O'\"'\"'Brien' || exit 1\n",
                "\n",
                "exec '/bin/bash'\n",
            )
        );
    }

    #[test]
    fn posix_resume_script_keeps_quoted_session_id_as_single_argument() {
        // build_resume_command 已把含元字符的 id 单引号化；脚本再整体引用一次交给 `shell -ic`
        let script = build_posix_resume_script("/bin/zsh", "claude --resume 'abc;rm -rf ~'", None);

        assert!(
            script.contains(r#"'/bin/zsh' -lic 'claude --resume '"'"'abc;rm -rf ~'"'"''"#),
            "{script}"
        );
        assert!(
            script.ends_with("rm -f \"$0\"\n\nexec '/bin/zsh' -l\n"),
            "{script}"
        );
        assert!(!script.contains("\nabc;rm"), "{script}");
    }

    /// 真正用 `sh` 执行生成的脚本：resume 命令在会话目录中以单个参数收到 session id，
    /// 脚本随后删除自身并 exec 回用户 shell（stdin 为空时立即退出）。
    #[cfg(unix)]
    #[test]
    fn posix_resume_script_executes_resume_in_session_dir_and_self_deletes() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().expect("temp dir should be created");
        let bin_dir = temp.path().join("bin");
        let project_dir = temp.path().join("project dir");
        std::fs::create_dir_all(&bin_dir).expect("bin dir should be created");
        std::fs::create_dir_all(&project_dir).expect("project dir should be created");

        let record = temp.path().join("record.txt");
        let fake_claude = bin_dir.join("claude");
        std::fs::write(
            &fake_claude,
            format!(
                "#!/bin/sh\n{{ pwd; for arg in \"$@\"; do printf '[%s]\\n' \"$arg\"; done; }} > {}\n",
                shell_single_quote(&record.to_string_lossy())
            ),
        )
        .expect("fake claude should be written");
        std::fs::set_permissions(&fake_claude, std::fs::Permissions::from_mode(0o755))
            .expect("fake claude should be executable");

        let script = build_posix_resume_script(
            "/bin/sh",
            "claude --resume 'abc;touch pwned'",
            Some(&project_dir),
        );
        let script_file = temp.path().join("resume.sh");
        std::fs::write(&script_file, script).expect("script should be written");

        let path = format!(
            "{}:{}",
            bin_dir.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        let status = Command::new("sh")
            .arg(&script_file)
            .env("PATH", path)
            .current_dir(temp.path())
            .stdin(std::process::Stdio::null())
            .status()
            .expect("script should run");

        assert!(status.success());
        let recorded = std::fs::read_to_string(&record).expect("fake claude should run");
        let canonical_project = std::fs::canonicalize(&project_dir).expect("canonical dir");
        assert_eq!(
            recorded,
            format!(
                "{}\n[--resume]\n[abc;touch pwned]\n",
                canonical_project.display()
            )
        );
        assert!(!script_file.exists(), "script should delete itself");
        assert!(!temp.path().join("pwned").exists());
        assert!(!project_dir.join("pwned").exists());
    }

    /// Windows：cd /d 进入会话目录，`call` 运行 CLI shim，最后删除批处理自身。
    #[test]
    fn windows_resume_batch_changes_dir_and_calls_cli() {
        let batch = build_windows_resume_batch(
            &argv(&["claude", "--resume", "abc-123"]),
            Some(r"C:\Users\me\proj (1)&x"),
        )
        .expect("plain argv should build");

        assert_eq!(
            batch,
            "@echo off\r\ncd /d \"C:\\Users\\me\\proj (1)&x\" || exit /b 1\r\ncall claude --resume abc-123\r\ndel \"%~f0\" >nul 2>&1\r\n"
        );
    }

    #[test]
    fn windows_resume_batch_uses_pushd_for_unc_and_skips_missing_cwd() {
        let unc = build_windows_resume_batch(
            &argv(&["codex", "resume", "019a_x.y"]),
            Some(r"\\wsl$\Ubuntu\home\me"),
        )
        .expect("unc cwd should build");
        assert!(
            unc.starts_with("@echo off\r\npushd \"\\\\wsl$\\Ubuntu\\home\\me\" || exit /b 1\r\n"),
            "{unc}"
        );

        let no_cwd = build_windows_resume_batch(&argv(&["opencode", "-s", "ses_1"]), None)
            .expect("no cwd should build");
        assert_eq!(
            no_cwd,
            "@echo off\r\ncall opencode -s ses_1\r\ndel \"%~f0\" >nul 2>&1\r\n"
        );
    }

    #[test]
    fn windows_resume_batch_rejects_args_cmd_would_reinterpret() {
        for id in ["abc&calc", "a%PATH%b", "a b", "a\"b", "a^b", "a!b", ""] {
            assert!(
                build_windows_resume_batch(&argv(&["claude", "--resume", id]), None).is_err(),
                "{id:?} should be rejected"
            );
        }
        assert!(build_windows_resume_batch(&[], None).is_err());
    }

    /// Linux 上把 Unix 绝对路径映射为 Wine 默认 `Z:` 盘下的 Windows 路径。
    #[cfg(unix)]
    fn wine_z_path(path: &Path) -> String {
        format!("Z:{}", path.to_string_lossy().replace('/', "\\"))
    }

    #[cfg(unix)]
    fn run_wine_cmd(args: &[&str], dir: &Path, wine_path: Option<&str>) -> std::process::Output {
        let mut command = Command::new("wine");
        command
            .arg("cmd")
            .args(args)
            .env("WINEDEBUG", "-all")
            .current_dir(dir)
            .stdin(std::process::Stdio::null());
        if let Some(wine_path) = wine_path {
            command.env("WINEPATH", wine_path);
        }
        command.output().expect("wine should be installed")
    }

    /// 在 Wine 的 cmd 中真正执行生成的 `.bat`（CCS-249）：会话目录含空格、括号、`&`、`^` 与 `%`，
    /// 假的 `claude.cmd` shim 记录 `%CD%` 与 `%*`。断言 `cd /d` 进入了带引号的目录、shim 恰好收到
    /// `--resume <id>`、`call` 返回后继续执行、`del "%~f0"` 删除了批处理自身；`/c` 与 `/K`（stdin 关闭）各跑一次。
    /// 只覆盖批处理内容本身，不覆盖 cmd / Windows Terminal / PowerShell 如何启动它。
    ///
    /// Wine 9.0 的 cmd 不为内建命令设置返回码，`cd ... || exit /b 1` 在成功时也会走 `||` 分支；
    /// 探测到该缺陷时去掉 ` || exit /b 1` 再执行（其余行逐字不变），真实 Windows 上该分支只在 cd 失败时触发。
    ///
    /// 运行：`cargo test --manifest-path src-tauri/Cargo.toml --lib under_wine -- --ignored`
    #[cfg(unix)]
    #[test]
    #[ignore = "needs wine; run with --ignored"]
    fn windows_resume_batch_runs_under_wine_cmd() {
        let probe_dir = tempfile::tempdir().expect("temp dir should be created");
        let probe = run_wine_cmd(&["/c", "cd /d Z:\\ || exit /b 7"], probe_dir.path(), None);
        let wine_honours_builtin_status = probe.status.code() == Some(0);

        for mode in ["/c", "/K"] {
            let temp = tempfile::tempdir().expect("temp dir should be created");
            let bin_dir = temp.path().join("bin");
            let project_dir = temp.path().join("proj (1)&x^y").join("100%");
            std::fs::create_dir_all(&bin_dir).expect("bin dir should be created");
            std::fs::create_dir_all(&project_dir).expect("project dir should be created");

            let marker = temp.path().join("marker.txt");
            // 延迟展开在解析之后才替换 `!CD!`，目录名里的 `&`、`(` 不会被当作命令分隔符。
            std::fs::write(
                bin_dir.join("claude.cmd"),
                format!(
                    "@echo off\r\nsetlocal EnableDelayedExpansion\r\n>>\"{m}\" echo CD=[!CD!]\r\n>>\"{m}\" echo ARGS=[%*]\r\nexit /b 0\r\n",
                    m = wine_z_path(&marker)
                ),
            )
            .expect("fake claude shim should be written");

            let session_cwd = wine_z_path(&project_dir);
            let mut batch = build_windows_resume_batch(
                &argv(&["claude", "--resume", "0199a1b2-c3d4.e5_f6"]),
                Some(&session_cwd),
            )
            .expect("plain argv should build");
            assert!(
                batch.contains("cd /d \"") && batch.contains("100%%\" || exit /b 1\r\n"),
                "{batch}"
            );
            if !wine_honours_builtin_status {
                batch = batch.replace(" || exit /b 1\r\n", "\r\n");
            }
            let batch_file = bin_dir.join("cc_switch_resume.bat");
            std::fs::write(&batch_file, &batch).expect("batch should be written");

            let output = run_wine_cmd(
                &[mode, &wine_z_path(&batch_file)],
                temp.path(),
                Some(&wine_z_path(&bin_dir)),
            );
            let stdout = String::from_utf8_lossy(&output.stdout);
            assert!(
                output.status.success(),
                "wine cmd {mode} failed: {:?}\nstdout: {stdout}\nstderr: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
            );

            let recorded = std::fs::read_to_string(&marker)
                .unwrap_or_else(|e| panic!("shim should run under {mode}: {e}\n{stdout}"));
            assert_eq!(
                recorded,
                format!("CD=[{session_cwd}]\r\nARGS=[--resume 0199a1b2-c3d4.e5_f6]\r\n"),
                "mode {mode}"
            );
            assert!(
                !batch_file.exists(),
                "batch should continue after `call` and delete itself under {mode}"
            );
            if mode == "/K" {
                // `/K` 在批处理结束后停在会话目录的交互提示符上，stdin 关闭后退出。
                assert!(stdout.contains(&format!("{session_cwd}>")), "{stdout}");
            }
        }
    }

    #[test]
    fn build_shell_command_keeps_command_without_cwd_prefix_when_not_provided() {
        assert_eq!(
            build_shell_command("claude --resume abc-123", None),
            "claude --resume abc-123"
        );
    }

    #[test]
    fn wezterm_compatible_terminals_use_start_and_cwd_arguments() {
        let args = build_wezterm_compatible_args_with_shell(
            "Kaku",
            "claude --resume abc-123",
            Some("/tmp/project dir"),
            "/bin/zsh",
        );

        assert_eq!(
            args,
            vec![
                "-na".to_string(),
                "Kaku".to_string(),
                "--args".to_string(),
                "start".to_string(),
                "--cwd".to_string(),
                "/tmp/project dir".to_string(),
                "--".to_string(),
                "/bin/zsh".to_string(),
                "-c".to_string(),
                "claude --resume abc-123".to_string(),
            ]
        );
    }

    #[test]
    fn ghostty_uses_working_directory_arg_for_cwd() {
        // cwd should be passed as --working-directory, not embedded in the shell command string
        // This avoids shell expansion of special characters in directory paths
        let cwd = "/tmp/project dir";
        let command = "claude --resume abc-123";

        // Verify build_shell_command does NOT include cwd when used in ghostty context
        // (ghostty passes cwd via --working-directory flag instead)
        assert_eq!(
            build_shell_command(command, None),
            "claude --resume abc-123"
        );

        // Verify shell_escape works correctly for paths with spaces
        assert_eq!(shell_single_quote(cwd), "'/tmp/project dir'");
    }

    #[test]
    fn shell_escape_neutralizes_command_substitution_in_cwd() {
        assert_eq!(
            build_shell_command("claude --resume abc", Some("/tmp/$(rm -rf ~)")),
            "cd '/tmp/$(rm -rf ~)' && claude --resume abc"
        );
        assert_eq!(shell_single_quote("it's"), "'it'\"'\"'s'");
    }
}
