//! shell / cmd / PowerShell 引号与 shell 名校验。纯字符串函数，三平台都参与编译与单测。

/// 校验 WSL 发行版名称是否合法
/// WSL 发行版名称只允许字母、数字、连字符和下划线
#[cfg(target_os = "windows")]
pub(crate) fn is_valid_wsl_distro_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
}

/// Validate that the given shell name is one of the allowed shells.
pub(crate) fn is_valid_shell(shell: &str) -> bool {
    matches!(
        shell.rsplit('/').next().unwrap_or(shell),
        "sh" | "bash" | "zsh" | "fish" | "dash"
    )
}

/// Validate that the given shell flag is one of the allowed flags.
#[cfg(target_os = "windows")]
pub(crate) fn is_valid_shell_flag(flag: &str) -> bool {
    matches!(flag, "-c" | "-lc" | "-lic")
}

/// Return the default invocation flag for the given shell.
pub(crate) fn default_flag_for_shell(shell: &str) -> &'static str {
    match shell.rsplit('/').next().unwrap_or(shell) {
        "dash" | "sh" => "-c",
        "fish" => "-lc",
        _ => "-lic",
    }
}

/// POSIX 单引号转义：双引号内仍会展开 `$(...)` 与反引号，因此统一使用单引号。
#[cfg_attr(windows, allow(dead_code))]
pub(crate) fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(crate) fn is_windows_unc_path(path: &str) -> bool {
    path.starts_with(r"\\")
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(crate) fn build_windows_cwd_command_str(path: &str) -> String {
    // 路径位于双引号内：cmd 在引号内不再解析 `^ & | < > ( )`（`^` 会被当作字面量保留），
    // 只有批处理阶段的 `%` 展开仍然生效，因此仅需把 `%` 写成 `%%`。
    let escaped = path.replace('%', "%%");

    if is_windows_unc_path(path) {
        // `cmd.exe` cannot make a UNC path current via `cd`; `pushd` maps it first.
        format!("pushd \"{escaped}\" || exit /b 1\r\n")
    } else {
        format!("cd /d \"{escaped}\" || exit /b 1\r\n")
    }
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(crate) fn escape_windows_batch_value(value: &str) -> String {
    value
        .replace('^', "^^")
        .replace('%', "%%")
        .replace('&', "^&")
        .replace('|', "^|")
        .replace('<', "^<")
        .replace('>', "^>")
        .replace('(', "^(")
        .replace(')', "^)")
}

/// 生成 PowerShell 单引号字符串字面量：单引号内只有 `'` 需要转义（写成 `''`）。
/// PowerShell 同样把 U+2018/U+2019/U+201A/U+201B 当作单引号，一并加倍。
/// 用于 `& '<path>'`，避免 `C:\Users\O'Brien\...` 这类路径解析失败。
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(crate) fn powershell_single_quote(value: &str) -> String {
    let mut quoted = String::with_capacity(value.len() + 2);
    quoted.push('\'');
    for ch in value.chars() {
        if matches!(ch, '\'' | '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}') {
            quoted.push(ch);
        }
        quoted.push(ch);
    }
    quoted.push('\'');
    quoted
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "windows")]
    mod wsl_helpers {
        use super::super::*;

        #[test]
        fn test_is_valid_shell() {
            assert!(is_valid_shell("bash"));
            assert!(is_valid_shell("zsh"));
            assert!(is_valid_shell("sh"));
            assert!(is_valid_shell("fish"));
            assert!(is_valid_shell("dash"));
            assert!(is_valid_shell("/usr/bin/bash"));
            assert!(is_valid_shell("/bin/zsh"));
            assert!(!is_valid_shell("powershell"));
            assert!(!is_valid_shell("cmd"));
            assert!(!is_valid_shell(""));
        }

        #[test]
        fn test_is_valid_shell_flag() {
            assert!(is_valid_shell_flag("-c"));
            assert!(is_valid_shell_flag("-lc"));
            assert!(is_valid_shell_flag("-lic"));
            assert!(!is_valid_shell_flag("-x"));
            assert!(!is_valid_shell_flag(""));
            assert!(!is_valid_shell_flag("--login"));
        }

        #[test]
        fn test_default_flag_for_shell() {
            assert_eq!(default_flag_for_shell("sh"), "-c");
            assert_eq!(default_flag_for_shell("dash"), "-c");
            assert_eq!(default_flag_for_shell("/bin/dash"), "-c");
            assert_eq!(default_flag_for_shell("fish"), "-lc");
            assert_eq!(default_flag_for_shell("bash"), "-lic");
            assert_eq!(default_flag_for_shell("zsh"), "-lic");
            assert_eq!(default_flag_for_shell("/usr/bin/zsh"), "-lic");
        }

        #[test]
        fn test_is_valid_wsl_distro_name() {
            assert!(is_valid_wsl_distro_name("Ubuntu"));
            assert!(is_valid_wsl_distro_name("Ubuntu-22.04"));
            assert!(is_valid_wsl_distro_name("my_distro"));
            assert!(!is_valid_wsl_distro_name(""));
            assert!(!is_valid_wsl_distro_name("distro with spaces"));
            assert!(!is_valid_wsl_distro_name(&"a".repeat(65)));
        }
    }

    #[test]
    fn build_windows_cwd_command_str_uses_cd_for_drive_paths() {
        let command = build_windows_cwd_command_str(r"C:\work\repo");

        assert_eq!(command, "cd /d \"C:\\work\\repo\" || exit /b 1\r\n");
    }

    #[test]
    fn build_windows_cwd_command_str_uses_pushd_for_unc_paths() {
        let command = build_windows_cwd_command_str(r"\\wsl$\Ubuntu\home\coder\repo");

        assert_eq!(
            command,
            "pushd \"\\\\wsl$\\Ubuntu\\home\\coder\\repo\" || exit /b 1\r\n"
        );
    }

    #[test]
    fn build_windows_cwd_command_str_escapes_batch_metacharacters() {
        let command = build_windows_cwd_command_str(r"\\server\share\100%&(test)");

        assert_eq!(
            command,
            "pushd \"\\\\server\\share\\100%%&(test)\" || exit /b 1\r\n"
        );
    }

    #[test]
    fn powershell_single_quote_doubles_apostrophes() {
        assert_eq!(
            powershell_single_quote(r"C:\Users\O'Brien\AppData\Local\Temp\x.bat"),
            r"'C:\Users\O''Brien\AppData\Local\Temp\x.bat'"
        );
        assert_eq!(
            powershell_single_quote(r"C:\Temp\plain.bat"),
            r"'C:\Temp\plain.bat'"
        );
        assert_eq!(powershell_single_quote("''"), "''''''");
        // `$`、反引号、`&` 在单引号内是字面量，不应被改写。
        assert_eq!(
            powershell_single_quote(r"C:\a$b`c&d\x.bat"),
            r"'C:\a$b`c&d\x.bat'"
        );
    }

    #[test]
    fn powershell_single_quote_doubles_typographic_single_quotes() {
        assert_eq!(
            powershell_single_quote("C:\\Users\\O\u{2019}Brien\\x.bat"),
            "'C:\\Users\\O\u{2019}\u{2019}Brien\\x.bat'"
        );
        assert_eq!(
            powershell_single_quote("\u{2018}\u{201A}\u{201B}"),
            "'\u{2018}\u{2018}\u{201A}\u{201A}\u{201B}\u{201B}'"
        );
    }

    #[test]
    fn powershell_single_quote_used_at_windows_launch_sites() {
        // Windows 启动点都是 cfg(windows)，Linux 上无法直接运行，
        // 因此检查源码确保不再手工拼接未转义的 `& '<path>'`。provider 终端与
        // `launch_terminal_running` 共用 `start_bat_in_preferred_terminal` 这一处拼接。
        let source = include_str!("terminal/windows.rs");
        let unescaped = concat!("format!(\"& '", "{}'\"");
        assert!(
            !source.contains(unescaped),
            "PowerShell launch command must use powershell_single_quote"
        );
        assert_eq!(
            source
                .matches("format!(\"& {}\", powershell_single_quote(&bat_path))")
                .count(),
            1
        );
    }
}
