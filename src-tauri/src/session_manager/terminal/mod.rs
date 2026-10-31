use std::process::Command;

use super::resume::{quote_posix, resume_spec, validate_cwd, ResumeSpec, ShellKind};

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
    if cfg!(target_os = "macos") {
        return launch_terminal(target, &command, cwd.as_deref());
    }
    // Windows / Linux: argv-based launchers (SESS-15).
    let spec = resume_spec(provider_id, session_id).ok_or("Session cannot be resumed")?;
    let (program, args) = if cfg!(windows) {
        windows_argv(target, &spec, cwd.as_deref())?
    } else {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
        linux_argv(target, &command, cwd.as_deref(), &shell)?
    };
    spawn_detached(&program, &args)
}

/// Default terminal for the current platform when none is configured.
pub fn default_terminal() -> &'static str {
    if cfg!(target_os = "macos") {
        "terminal"
    } else if cfg!(windows) {
        "powershell"
    } else {
        "x-terminal-emulator"
    }
}

fn spawn_detached(program: &str, args: &[String]) -> Result<(), String> {
    let mut cmd = Command::new(program);
    cmd.args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
        cmd.creation_flags(CREATE_NEW_CONSOLE);
    }
    cmd.spawn()
        .map(|_| ())
        .map_err(|e| format!("Failed to launch {program}: {e}"))
}

/// Windows launchers: Windows Terminal, PowerShell, cmd.
fn windows_argv(
    target: &str,
    spec: &ResumeSpec,
    cwd: Option<&str>,
) -> Result<(String, Vec<String>), String> {
    use super::resume::{quote_cmd, quote_powershell};
    let ps_command = spec.render(ShellKind::PowerShell);
    match target {
        "wt" => {
            let mut args = Vec::new();
            if let Some(dir) = cwd {
                args.push("-d".to_string());
                args.push(dir.to_string());
            }
            args.push("--".to_string());
            args.push("powershell.exe".to_string());
            args.push("-NoExit".to_string());
            args.push("-Command".to_string());
            args.push(ps_command);
            Ok(("wt.exe".to_string(), args))
        }
        "cmd" => {
            let command = spec.render(ShellKind::Cmd);
            let line = match cwd {
                Some(dir) => format!("cd /d {} && {command}", quote_cmd(dir)),
                None => command,
            };
            Ok(("cmd.exe".to_string(), vec!["/K".to_string(), line]))
        }
        "powershell" | "terminal" => {
            let line = match cwd {
                Some(dir) => format!(
                    "Set-Location -LiteralPath {}; {ps_command}",
                    quote_powershell(dir)
                ),
                None => ps_command,
            };
            Ok((
                "powershell.exe".to_string(),
                vec!["-NoExit".to_string(), "-Command".to_string(), line],
            ))
        }
        other => Err(format!("Unsupported terminal target on Windows: {other}")),
    }
}

/// Linux launchers. The resume command runs in the user's shell (`-lc`) so
/// PATH set up by login files (nvm, pipx, ...) applies.
fn linux_argv(
    target: &str,
    command: &str,
    cwd: Option<&str>,
    shell: &str,
) -> Result<(String, Vec<String>), String> {
    let run = vec![shell.to_string(), "-lc".to_string(), command.to_string()];
    let with_dir = |flag: &str, joined: bool| -> Vec<String> {
        match cwd {
            Some(dir) if joined => vec![format!("{flag}={dir}")],
            Some(dir) => vec![flag.to_string(), dir.to_string()],
            None => Vec::new(),
        }
    };
    let (program, mut args) = match target {
        "gnome-terminal" => {
            let mut a = with_dir("--working-directory", true);
            a.push("--".to_string());
            ("gnome-terminal", a)
        }
        "konsole" => {
            let mut a = with_dir("--workdir", false);
            a.push("-e".to_string());
            ("konsole", a)
        }
        "xfce4-terminal" => {
            let mut a = with_dir("--working-directory", false);
            a.push("-x".to_string());
            ("xfce4-terminal", a)
        }
        "alacritty" => {
            let mut a = with_dir("--working-directory", false);
            a.push("-e".to_string());
            ("alacritty", a)
        }
        "kitty" => ("kitty", with_dir("--directory", false)),
        "ghostty" => {
            let mut a = with_dir("--working-directory", true);
            a.push("-e".to_string());
            ("ghostty", a)
        }
        "terminal" | "x-terminal-emulator" => {
            // Debian alternatives wrapper: no portable cwd flag, so cd first.
            let line = build_shell_command(command, cwd);
            return Ok((
                "x-terminal-emulator".to_string(),
                vec!["-e".to_string(), shell.to_string(), "-lc".to_string(), line],
            ));
        }
        other => return Err(format!("Unsupported terminal target on Linux: {other}")),
    };
    args.extend(run);
    Ok((program.to_string(), args))
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

    // The launcher script lives in the app's temp dir, never in the project
    // (it used to show up in `git status`), and `cd`s itself so sessions
    // without a project dir work too (SESS-26).
    let script_dir = warp_script_dir();
    std::fs::create_dir_all(&script_dir)
        .map_err(|e| format!("Failed to create Warp script directory: {e}"))?;
    prune_stale_scripts(&script_dir, std::time::Duration::from_secs(3600));

    let mut script_file = tempfile::Builder::new()
        .prefix("resume-")
        .suffix(".sh")
        .disable_cleanup(true)
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempfile_in(&script_dir)
        .map_err(|e| format!("Failed to create temporary script file for launching Warp: {e}"))?;

    write!(&mut script_file, "{}", warp_script(command, cwd))
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

#[cfg(unix)]
fn warp_script_dir() -> std::path::PathBuf {
    std::env::temp_dir().join("cc-switch").join("warp")
}

/// Script body: removes itself, changes into the project (if any) and execs
/// the already-quoted resume command.
#[cfg(unix)]
fn warp_script(command: &str, cwd: Option<&str>) -> String {
    let cd = cwd
        .map(|dir| format!("cd {} || exit 1\n", quote_posix(dir)))
        .unwrap_or_default();
    format!("#!/usr/bin/env sh\nrm -- \"$0\"\n{cd}exec {command}\n")
}

/// Delete launcher scripts Warp never ran.
#[cfg(unix)]
fn prune_stale_scripts(dir: &std::path::Path, max_age: std::time::Duration) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let old = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age > max_age);
        if old {
            let _ = std::fs::remove_file(entry.path());
        }
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

    fn spec() -> ResumeSpec {
        resume_spec("claude", "abc-123").unwrap()
    }

    #[test]
    fn windows_argv_per_terminal() {
        let (p, a) = windows_argv("wt", &spec(), Some(r"C:\work\proj")).unwrap();
        assert_eq!(p, "wt.exe");
        assert_eq!(&a[..3], &["-d", r"C:\work\proj", "--"]);
        assert_eq!(a.last().unwrap(), "claude --resume abc-123");

        let (p, a) = windows_argv("powershell", &spec(), Some(r"C:\it's")).unwrap();
        assert_eq!(p, "powershell.exe");
        assert_eq!(a[0], "-NoExit");
        assert!(
            a[2].starts_with("Set-Location -LiteralPath 'C:\\it''s'"),
            "{}",
            a[2]
        );
        assert!(a[2].ends_with("claude --resume abc-123"));

        let (p, a) = windows_argv("cmd", &spec(), None).unwrap();
        assert_eq!(p, "cmd.exe");
        assert_eq!(a, vec!["/K", "claude --resume abc-123"]);
        assert!(windows_argv("iterm", &spec(), None).is_err());
    }

    #[test]
    fn linux_argv_per_terminal() {
        let cmd = "claude --resume abc-123";
        let (p, a) = linux_argv("gnome-terminal", cmd, Some("/w"), "/bin/bash").unwrap();
        assert_eq!(p, "gnome-terminal");
        assert_eq!(
            a,
            vec!["--working-directory=/w", "--", "/bin/bash", "-lc", cmd]
        );

        let (_, a) = linux_argv("konsole", cmd, Some("/w"), "/bin/zsh").unwrap();
        assert_eq!(a, vec!["--workdir", "/w", "-e", "/bin/zsh", "-lc", cmd]);

        let (_, a) = linux_argv("kitty", cmd, Some("/w"), "/bin/sh").unwrap();
        assert_eq!(a, vec!["--directory", "/w", "/bin/sh", "-lc", cmd]);

        let (_, a) = linux_argv("xfce4-terminal", cmd, None, "/bin/sh").unwrap();
        assert_eq!(a, vec!["-x", "/bin/sh", "-lc", cmd]);

        let (_, a) = linux_argv("ghostty", cmd, Some("/w"), "/bin/sh").unwrap();
        assert_eq!(a[0], "--working-directory=/w");

        let (p, a) = linux_argv("terminal", cmd, Some("/my dir"), "/bin/sh").unwrap();
        assert_eq!(p, "x-terminal-emulator");
        assert_eq!(a[3], "cd '/my dir' && claude --resume abc-123");
        assert!(linux_argv("wt", cmd, None, "/bin/sh").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn warp_script_cds_itself_and_lives_outside_the_project() {
        let script = warp_script("claude --resume abc-123", Some("/my proj"));
        assert!(script.contains("cd '/my proj' || exit 1"));
        assert!(script.contains("exec claude --resume abc-123"));
        assert!(!warp_script("x", None).contains("cd "));
        assert!(warp_script_dir().starts_with(std::env::temp_dir()));

        let dir = tempfile::tempdir().unwrap();
        let stale = dir.path().join("resume-old.sh");
        std::fs::write(&stale, "x").unwrap();
        prune_stale_scripts(dir.path(), std::time::Duration::from_secs(3600));
        assert!(stale.exists(), "fresh scripts are kept");
        prune_stale_scripts(dir.path(), std::time::Duration::ZERO);
        assert!(!stale.exists());
    }
}
