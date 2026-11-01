//! Safe construction of session "resume" commands.
//!
//! Session ids and working directories come from files written by external CLIs
//! (JSONL transcripts, SQLite databases). They must never be interpolated into a
//! shell string verbatim. This module validates the session id against a strict
//! pattern, builds the invocation from structured parts and quotes every
//! argument for the target shell.

use std::path::{Path, PathBuf};

/// Maximum accepted session id length.
const MAX_SESSION_ID_LEN: usize = 128;

/// A resume invocation represented as a program plus discrete arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResumeSpec {
    pub program: &'static str,
    pub args: Vec<String>,
}

/// Shell dialects a resume command can be rendered for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellKind {
    Posix,
    PowerShell,
    /// cmd.exe; not used by the current launchers but kept so callers can
    /// render a copyable command for it.
    #[allow(dead_code)]
    Cmd,
}

/// Returns true when `id` matches `^[A-Za-z0-9._-]{1,128}$` and does not start
/// with `-` (which a CLI could otherwise parse as an option).
pub fn is_valid_session_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_SESSION_ID_LEN
        && !id.starts_with('-')
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// Build the structured resume invocation for a provider/session pair.
/// Returns `None` for providers without CLI resume or for invalid session ids.
pub fn resume_spec(provider_id: &str, session_id: &str) -> Option<ResumeSpec> {
    if !is_valid_session_id(session_id) {
        return None;
    }
    let id = session_id.to_string();
    let (program, args) = match provider_id {
        "claude" => ("claude", vec!["--resume".to_string(), id]),
        "codex" => ("codex", vec!["resume".to_string(), id]),
        "gemini" => ("gemini", vec!["--resume".to_string(), id]),
        "grokbuild" => ("grok", vec!["--resume".to_string(), id]),
        "opencode" => ("opencode", vec!["-s".to_string(), id]),
        _ => return None,
    };
    Some(ResumeSpec { program, args })
}

impl ResumeSpec {
    /// Render the invocation as a single command line for `shell`, quoting
    /// each argument that needs it.
    pub fn render(&self, shell: ShellKind) -> String {
        std::iter::once(self.program)
            .chain(self.args.iter().map(String::as_str))
            .map(|part| quote_arg(part, shell))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// Display/copy string used in `SessionMeta.resume_command`.
pub fn resume_command(provider_id: &str, session_id: &str) -> Option<String> {
    let shell = if cfg!(windows) {
        ShellKind::PowerShell
    } else {
        ShellKind::Posix
    };
    resume_spec(provider_id, session_id).map(|spec| spec.render(shell))
}

fn is_plain_word(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|b| {
            b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b'/' | b':' | b'=' | b',')
        })
}

fn quote_arg(value: &str, shell: ShellKind) -> String {
    if is_plain_word(value) {
        return value.to_string();
    }
    match shell {
        ShellKind::Posix => quote_posix(value),
        ShellKind::PowerShell => quote_powershell(value),
        ShellKind::Cmd => quote_cmd(value),
    }
}

/// POSIX single-quote escaping: `'` becomes `'\''`. Nothing inside single
/// quotes is expanded by sh/bash/zsh.
pub fn quote_posix(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

/// PowerShell single-quoted literal: `'` is escaped by doubling. Variables and
/// subexpressions are not expanded inside single quotes. Typographic single
/// quotes are also treated as quote characters by PowerShell, so they are
/// doubled too.
pub fn quote_powershell(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('\'');
    for ch in value.chars() {
        if matches!(ch, '\'' | '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}') {
            out.push(ch);
        }
        out.push(ch);
    }
    out.push('\'');
    out
}

/// cmd.exe quoting: wrap in double quotes, double embedded quotes and
/// caret-escape `%` and `!` so environment expansion cannot fire.
pub fn quote_cmd(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\"\""),
            '%' | '!' => {
                out.push('^');
                out.push(ch);
            }
            _ => out.push(ch),
        }
    }
    out.push('"');
    out
}

/// Validate a working directory for a terminal launch: it must be an existing
/// directory. Returns the canonical path.
pub fn validate_cwd(cwd: &str) -> Result<PathBuf, String> {
    let trimmed = cwd.trim();
    if trimmed.is_empty() {
        return Err("Working directory is empty".to_string());
    }
    if trimmed.contains('\0') || trimmed.contains('\n') || trimmed.contains('\r') {
        return Err("Working directory contains invalid characters".to_string());
    }
    let path = Path::new(trimmed);
    if !path.is_dir() {
        return Err(format!("Working directory does not exist: {trimmed}"));
    }
    path.canonicalize()
        .map_err(|e| format!("Failed to resolve working directory {trimmed}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_uuid_and_simple_ids() {
        assert!(is_valid_session_id("0f3c2a9e-1234-4abc-9def-0123456789ab"));
        assert!(is_valid_session_id("ses_abc.DEF_1"));
    }

    #[test]
    fn rejects_ids_with_shell_metacharacters_or_whitespace() {
        for bad in [
            "",
            "x; touch /tmp/pwned",
            "$(touch pwned)",
            "`id`",
            "a b",
            "a\nb",
            "a'b",
            "a\"b",
            "a|b",
            "a&b",
            "-rf",
            "--help",
            "é",
        ] {
            assert!(!is_valid_session_id(bad), "should reject {bad:?}");
            assert!(resume_spec("claude", bad).is_none());
            assert!(resume_command("claude", bad).is_none());
        }
        assert!(!is_valid_session_id(&"a".repeat(129)));
        assert!(is_valid_session_id(&"a".repeat(128)));
    }

    #[test]
    fn builds_provider_specific_commands() {
        let posix = |p: &str| resume_spec(p, "abc-123").map(|s| s.render(ShellKind::Posix));
        assert_eq!(posix("claude").as_deref(), Some("claude --resume abc-123"));
        assert_eq!(posix("codex").as_deref(), Some("codex resume abc-123"));
        assert_eq!(posix("gemini").as_deref(), Some("gemini --resume abc-123"));
        assert_eq!(posix("grokbuild").as_deref(), Some("grok --resume abc-123"));
        assert_eq!(posix("opencode").as_deref(), Some("opencode -s abc-123"));
        assert_eq!(posix("hermes"), None);
        assert_eq!(posix("openclaw"), None);
        assert_eq!(posix("unknown"), None);
    }

    #[test]
    fn posix_quoting_escapes_single_quotes_and_neutralises_expansion() {
        assert_eq!(quote_posix("/tmp/a b"), "'/tmp/a b'");
        assert_eq!(quote_posix("it's"), r"'it'\''s'");
        assert_eq!(quote_posix("/tmp/$(touch pwned)"), "'/tmp/$(touch pwned)'");
        assert_eq!(quote_arg("abc-123", ShellKind::Posix), "abc-123");
        assert_eq!(quote_arg("", ShellKind::Posix), "''");
    }

    #[test]
    fn powershell_and_cmd_quoting() {
        assert_eq!(quote_powershell("C:\\a b"), "'C:\\a b'");
        assert_eq!(quote_powershell("it's $env:X"), "'it''s $env:X'");
        assert_eq!(quote_powershell("a\u{2019}b"), "'a\u{2019}\u{2019}b'");
        assert_eq!(quote_cmd("C:\\a b"), "\"C:\\a b\"");
        assert_eq!(quote_cmd("a\"b%PATH%!x!"), "\"a\"\"b^%PATH^%^!x^!\"");
    }

    /// Run the POSIX-quoted string through a real shell and verify the payload
    /// is passed literally instead of being executed.
    #[cfg(unix)]
    #[test]
    fn posix_quoted_payload_is_not_executed_by_sh() {
        let dir = tempfile::tempdir().expect("tempdir");
        let marker = dir.path().join("pwned");
        let payload = format!(
            "$(touch {}) `touch {}`; touch {}",
            marker.display(),
            marker.display(),
            marker.display()
        );
        let script = format!("printf '%s' {}", quote_posix(&payload));
        let out = std::process::Command::new("sh")
            .arg("-c")
            .arg(&script)
            .output()
            .expect("run sh");
        assert!(out.status.success());
        assert_eq!(String::from_utf8_lossy(&out.stdout), payload);
        assert!(!marker.exists(), "payload must not execute");
    }

    #[test]
    fn validate_cwd_requires_existing_directory() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(validate_cwd(&dir.path().to_string_lossy()).is_ok());
        assert!(validate_cwd("").is_err());
        assert!(validate_cwd(&dir.path().join("missing").to_string_lossy()).is_err());
        let file = dir.path().join("file.txt");
        std::fs::write(&file, "x").unwrap();
        assert!(validate_cwd(&file.to_string_lossy()).is_err());
        assert!(validate_cwd("/tmp/$(touch pwned)").is_err());
    }
}
