use std::process::Command;

use super::resume::{quote_posix, resume_spec, validate_cwd, ShellKind};

/// Launch a terminal that resumes `session_id` of `provider_id`.
///
/// The command line is built here from validated structured parts; callers
/// (including the webview) never supply a free-form command string.
pub fn launch_session_terminal(
    target: &str,
    provider_id: &str,
    session_id: &str,
    cwd: Option<&str>,
) -> Result<(), String> {
    let (command, cwd) = prepare_resume(provider_id, session_id, cwd)?;
    launch_terminal(target, &command, cwd.as_deref())
}

/// Validate inputs and build the POSIX resume command plus canonical cwd.
fn prepare_resume(
    provider_id: &str,
    session_id: &str,
    cwd: Option<&str>,
) -> Result<(String, Option<String>), String> {
    let spec = resume_spec(provider_id, session_id).ok_or_else(|| {
        format!(
            "Session cannot be resumed (unsupported provider or invalid session id): {provider_id}"
        )
    })?;
    let cwd = match cwd {
        Some(dir) if !dir.trim().is_empty() => {
            Some(validate_cwd(dir)?.to_string_lossy().into_owned())
        }
        _ => None,
    };
    Ok((spec.render(ShellKind::Posix), cwd))
}

fn launch_terminal(target: &str, command: &str, cwd: Option<&str>) -> Result<(), String> {
    if command.trim().is_empty() {
        return Err("Resume command is empty".to_string());
    }

    if !cfg!(target_os = "macos") {
        return Err("Terminal resume is only supported on macOS".to_string());
    }

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
        _ => Err(format!("Unsupported terminal target: {target}")),
    }
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
        .permissions(std::fs::Permissions::from_mode(0o700))
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

fn build_shell_command(command: &str, cwd: Option<&str>) -> String {
    match cwd {
        Some(dir) if !dir.trim().is_empty() => {
            format!("cd {} && {}", quote_posix(dir), command)
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

        // Paths with spaces are single-quoted for shells that need a `cd` prefix
        assert_eq!(
            build_shell_command(command, Some(cwd)),
            "cd '/tmp/project dir' && claude --resume abc-123"
        );
    }

    #[test]
    fn prepare_resume_rejects_injected_session_id() {
        let err = prepare_resume("claude", "x; touch /tmp/pwned", None)
            .expect_err("injected session id must be rejected");
        assert!(err.contains("invalid session id"));
        assert!(prepare_resume("hermes", "abc", None).is_err());
    }

    #[test]
    fn prepare_resume_rejects_missing_cwd() {
        let dir = tempfile::tempdir().expect("tempdir");
        let missing = dir.path().join("$(touch pwned)");
        assert!(prepare_resume("claude", "abc-123", Some(&missing.to_string_lossy())).is_err());
    }

    #[test]
    fn prepare_resume_builds_command_from_parts() {
        let (cmd, cwd) = prepare_resume("codex", "abc-123", None).expect("valid");
        assert_eq!(cmd, "codex resume abc-123");
        assert_eq!(cwd, None);
    }

    /// A directory literally named with a command substitution must not be
    /// evaluated by the shell when the resume command is run.
    #[cfg(unix)]
    #[test]
    fn shell_command_with_malicious_cwd_does_not_execute_payload() {
        let root = tempfile::tempdir().expect("tempdir");
        let marker = root.path().join("pwned");
        let evil = root.path().join(format!(
            "a$(touch {m})`touch {m}`'b",
            m = marker.file_name().unwrap().to_string_lossy()
        ));
        std::fs::create_dir_all(&evil).expect("create evil dir");

        let (_cmd, cwd) =
            prepare_resume("claude", "abc-123", Some(&evil.to_string_lossy())).expect("valid");
        let script = build_shell_command("pwd", cwd.as_deref());
        let out = Command::new("sh")
            .arg("-c")
            .arg(&script)
            .current_dir(root.path())
            .output()
            .expect("run sh");
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(!marker.exists(), "payload in cwd must not execute");
        let printed = String::from_utf8_lossy(&out.stdout);
        assert!(printed.trim_end().ends_with("`touch pwned`'b"));
    }
}
