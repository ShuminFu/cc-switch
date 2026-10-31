//! macOS 终端的 AppleScript / `sh -c` 命令构造器：纯字符串拼接、与平台无关，
//! 因此在非 macOS 平台也编译进测试，让这些构造器的测试在 Linux CI 上运行。

use crate::platform::shell::shell_single_quote;

/// Escape a value as an AppleScript string literal.
fn applescript_string_literal(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Build the launcher command literal used by AppleScript.
fn applescript_launcher_command(script_file: &std::path::Path) -> String {
    applescript_string_literal(&format!(
        "sh {}",
        shell_single_quote(&script_file.to_string_lossy())
    ))
}

/// Build a launcher command that replaces the terminal-created shell session.
fn applescript_exec_launcher_command(script_file: &std::path::Path) -> String {
    applescript_string_literal(&format!(
        "exec sh {}",
        shell_single_quote(&script_file.to_string_lossy())
    ))
}

/// macOS: Terminal.app AppleScript.
/// A cold `activate` creates a default empty window before `do script` opens the command session.
/// Use `launch` for cold starts so `do script` can create the only new session without reusing restored windows.
pub(super) fn build_macos_terminal_applescript(script_file: &std::path::Path) -> String {
    format!(
        r#"set launcher_script to {launcher}
set was_running to application "Terminal" is running
tell application "Terminal"
    if was_running then
        activate
        do script launcher_script
    else
        launch
        do script launcher_script
        activate
    end if
end tell"#,
        launcher = applescript_exec_launcher_command(script_file)
    )
}

/// macOS: iTerm2
pub(super) fn build_macos_iterm2_applescript(script_file: &std::path::Path) -> String {
    format!(
        r#"set launcher_script to {launcher}
set was_running to application "iTerm" is running
tell application "iTerm"
    if was_running then
        activate
        if (count of windows) = 0 then
            create window with default profile
        else
            tell current window
                create tab with default profile
            end tell
        end if
    else
        activate
        set waited to 0
        repeat while (count of windows) = 0
            delay 0.1
            set waited to waited + 1
            if waited >= 30 then exit repeat
        end repeat
        if (count of windows) = 0 then
            create window with default profile
        end if
    end if
    tell current session of current window
        write text launcher_script
    end tell
end tell"#,
        launcher = applescript_exec_launcher_command(script_file)
    )
}

/// Keep the launcher path inside a `sh -c` string.
/// A bare `.sh` passed through `open --args` may also be opened as a document.
pub(super) fn build_macos_dash_c_command(script_file: &std::path::Path) -> String {
    format!(
        "exec sh {}",
        shell_single_quote(&script_file.to_string_lossy())
    )
}

/// macOS: Ghostty.
/// Warm starts use AppleScript to create one command window.
/// Cold starts use `initial-command` so the first default surface runs the launcher.
/// Do not use `initial-window=false` plus `new window`: cold launch can still create the default window first.
pub(super) fn build_macos_ghostty_applescript(script_file: &std::path::Path) -> String {
    format!(
        r#"set launcher_command to {launcher}
set was_running to application "Ghostty" is running
if was_running then
    tell application "Ghostty"
        new window with configuration {{command:launcher_command}}
    end tell
else
    do shell script "open -na Ghostty --args --quit-after-last-window-closed=true " & quoted form of ("--initial-command=" & launcher_command)
end if
"#,
        launcher = applescript_launcher_command(script_file)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn iterm2_applescript_cold_start_avoids_current_window_before_one_exists() {
        let script = build_macos_iterm2_applescript(Path::new("/tmp/cc_switch_launcher.sh"));

        let cold_start_branch = script
            .split("else\n        activate")
            .nth(1)
            .expect("cold start branch should be present")
            .split("    end if\n    tell current session")
            .next()
            .expect("cold start branch should end before writing command");

        assert!(cold_start_branch.contains("repeat while (count of windows) = 0"));
        assert!(cold_start_branch.contains("create window with default profile"));
        assert!(!cold_start_branch.contains("tell current window"));
        assert!(!cold_start_branch.contains("create tab with default profile"));
    }

    #[test]
    fn iterm2_applescript_keeps_new_tab_behavior_for_existing_windows() {
        let script = build_macos_iterm2_applescript(Path::new("/tmp/cc_switch_launcher.sh"));

        let running_branch = script
            .split("if was_running then")
            .nth(1)
            .expect("already-running branch should be present")
            .split("else\n        activate")
            .next()
            .expect("already-running branch should end before cold start branch");

        assert!(running_branch.contains("if (count of windows) = 0 then"));
        assert!(running_branch.contains("create window with default profile"));
        assert!(running_branch.contains("create tab with default profile"));
    }

    /// Terminal `activate` creates a default empty window on cold start; `launch` does not.
    #[test]
    fn terminal_applescript_cold_start_uses_launch_before_do_script() {
        let script = build_macos_terminal_applescript(Path::new("/tmp/cc_switch_launcher.sh"));

        assert!(
            script.contains(r#"set was_running to application "Terminal" is running"#),
            "missing was_running detection:\n{script}"
        );
        // Cold launches avoid `activate` until after `do script`, so no default empty window is created first.
        assert!(
            script.contains(
                "else\n        launch\n        do script launcher_script\n        activate"
            ),
            "cold start should launch before activating:\n{script}"
        );
        // Already-running launches should create a fresh session.
        assert!(
            script.contains(
                "if was_running then\n        activate\n        do script launcher_script\n"
            ),
            "already-running branch should use bare do script:\n{script}"
        );
        assert!(
            script.contains(r#"set launcher_script to "exec sh '/tmp/cc_switch_launcher.sh'""#),
            "Terminal should replace the auto-created shell:\n{script}"
        );
    }

    /// Restored windows should not receive the launcher command.
    #[test]
    fn terminal_applescript_does_not_hijack_restored_windows() {
        let script = build_macos_terminal_applescript(Path::new("/tmp/cc_switch_launcher.sh"));
        assert!(
            !script.contains(" in window 1"),
            "should not inject into an existing/restored Terminal window:\n{script}"
        );
        assert!(
            !script.contains("count of windows"),
            "should not infer restored-window safety from window count:\n{script}"
        );
    }

    /// Ghostty cold starts use `initial-command`; warm starts use the scripting dictionary.
    #[test]
    fn ghostty_applescript_cold_start_uses_initial_command() {
        let script = build_macos_ghostty_applescript(Path::new("/tmp/cc_switch_launcher.sh"));

        // Warm launches execute through the AppleScript command property, not `open -na ... -e`.
        assert!(
            script.contains(r#"set launcher_command to "sh '/tmp/cc_switch_launcher.sh'""#),
            "missing launcher_command:\n{script}"
        );
        assert!(script.contains("if was_running then"));
        assert!(script.contains("new window with configuration {command:launcher_command}"));
        assert!(
            !script.contains(" --args -e"),
            "should not execute through open -na -e:\n{script}"
        );
        // Cold launches make Ghostty's first default surface execute the launcher.
        assert!(script.contains(r#"set was_running to application "Ghostty" is running"#));
        assert!(
            script.contains(
                r#"do shell script "open -na Ghostty --args --quit-after-last-window-closed=true " & quoted form of ("--initial-command=" & launcher_command)"#
            ),
            "cold start should use initial-command:\n{script}"
        );
        assert!(
            !script.contains("--initial-window=false"),
            "should not rely on initial-window=false:\n{script}"
        );
        assert!(
            !script.contains("delay 0.5"),
            "should not rely on a fixed delay:\n{script}"
        );
        assert!(
            !script.contains("old_ids"),
            "should not track default windows for closing:\n{script}"
        );
        assert!(
            !script.contains("close window"),
            "should not close a default window:\n{script}"
        );
    }

    #[test]
    fn dash_c_command_wraps_script_path_inside_quoted_arg() {
        // The script path must stay inside the `-c` string, not as a bare argv.
        let s = build_macos_dash_c_command(Path::new("/tmp/cc_switch_launcher_1.sh"));
        assert_eq!(s, "exec sh '/tmp/cc_switch_launcher_1.sh'");

        // Spaces and single quotes must stay shell-safe too.
        let s2 = build_macos_dash_c_command(Path::new("/Users/me/it's dir/x.sh"));
        assert_eq!(s2, r#"exec sh '/Users/me/it'"'"'s dir/x.sh'"#);
    }

    /// AppleScript launchers need both shell-path quoting and AppleScript string quoting.
    #[test]
    fn applescript_builders_safely_quote_special_paths() {
        // First shell-quote the path, then wrap the whole command as an AppleScript string.
        let expected = r#""sh '/Users/me/it'\"'\"'s dir/x.sh'""#;
        let p = Path::new("/Users/me/it's dir/x.sh");
        assert_eq!(applescript_launcher_command(p), expected);
        assert_eq!(
            applescript_exec_launcher_command(p),
            r#""exec sh '/Users/me/it'\"'\"'s dir/x.sh'""#
        );
        assert!(
            build_macos_terminal_applescript(p)
                .contains(r#""exec sh '/Users/me/it'\"'\"'s dir/x.sh'""#),
            "Terminal did not quote safely"
        );
        assert!(
            build_macos_iterm2_applescript(p)
                .contains(r#""exec sh '/Users/me/it'\"'\"'s dir/x.sh'""#),
            "iTerm2 did not quote safely"
        );
        assert!(
            build_macos_ghostty_applescript(p).contains(expected),
            "Ghostty did not keep the non-exec launcher"
        );
    }

    #[test]
    fn applescript_string_literal_escapes_backslashes_and_quotes() {
        assert_eq!(applescript_string_literal("plain"), r#""plain""#);
        assert_eq!(applescript_string_literal(r#"a\b"c"#), r#""a\\b\"c""#);
    }
}
