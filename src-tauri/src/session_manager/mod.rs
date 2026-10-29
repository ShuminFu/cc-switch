pub mod providers;
pub mod terminal;

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use providers::{claude, codex, gemini, grokbuild, hermes, openclaw, opencode};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionMeta {
    pub provider_id: String,
    pub session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_dir: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_active_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resume_command: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionMessage {
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ts: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteSessionRequest {
    pub provider_id: String,
    pub session_id: String,
    pub source_path: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteSessionOutcome {
    pub provider_id: String,
    pub session_id: String,
    pub source_path: String,
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

pub fn scan_sessions() -> Vec<SessionMeta> {
    let (r1, r2, r3, r4, r5, r6, r7) = std::thread::scope(|s| {
        let h1 = s.spawn(codex::scan_sessions);
        let h2 = s.spawn(claude::scan_sessions);
        let h3 = s.spawn(opencode::scan_sessions);
        let h4 = s.spawn(openclaw::scan_sessions);
        let h5 = s.spawn(gemini::scan_sessions);
        let h6 = s.spawn(hermes::scan_sessions);
        let h7 = s.spawn(grokbuild::scan_sessions);
        (
            h1.join().unwrap_or_default(),
            h2.join().unwrap_or_default(),
            h3.join().unwrap_or_default(),
            h4.join().unwrap_or_default(),
            h5.join().unwrap_or_default(),
            h6.join().unwrap_or_default(),
            h7.join().unwrap_or_default(),
        )
    });

    let mut sessions = Vec::new();
    sessions.extend(r1);
    sessions.extend(r2);
    sessions.extend(r3);
    sessions.extend(r4);
    sessions.extend(r5);
    sessions.extend(r6);
    sessions.extend(r7);

    sessions.sort_by(|a, b| {
        let a_ts = a.last_active_at.or(a.created_at).unwrap_or(0);
        let b_ts = b.last_active_at.or(b.created_at).unwrap_or(0);
        b_ts.cmp(&a_ts)
    });

    sessions
}

/// 根据固定模板构建会话恢复命令。session id 作为单个 shell 参数被引用，
/// 永远不会以原始字符串拼接进 shell；不支持 CLI 恢复的 provider 返回 None。
pub fn build_resume_command(provider_id: &str, session_id: &str) -> Option<String> {
    let program = match provider_id {
        "claude" => "claude --resume",
        "codex" => "codex resume",
        "gemini" => "gemini --resume",
        "grokbuild" => "grok --resume",
        "opencode" => "opencode -s",
        _ => return None,
    };
    Some(format!("{program} {}", shell_quote_arg(session_id)))
}

/// 只由安全字符组成的参数保持原样（保证常见 UUID 形式的命令可读），
/// 其余一律走 POSIX 单引号转义。
fn shell_quote_arg(value: &str) -> String {
    let is_plain = !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    if is_plain {
        value.to_string()
    } else {
        shell_single_quote(value)
    }
}

fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

/// 校验来自 webview 的 session id：拒绝空值、控制字符以及会被 CLI 当作选项的前导 `-`。
fn validate_resume_session_id(session_id: &str) -> Result<(), String> {
    if session_id.trim().is_empty() {
        return Err("Session id is empty".to_string());
    }
    if session_id.starts_with('-') {
        return Err("Session id must not start with '-'".to_string());
    }
    if session_id.chars().any(char::is_control) {
        return Err("Session id contains control characters".to_string());
    }
    Ok(())
}

/// 恢复会话所需的终端参数：由后端生成的命令与会话元数据中的工作目录。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResumeTarget {
    pub command: String,
    pub cwd: Option<String>,
}

/// 按 `{provider_id, session_id}` 在后端扫描到的会话中查找目标，并用固定模板生成恢复命令。
/// webview 不再能提供命令字符串或工作目录。
pub fn resolve_resume_target(provider_id: &str, session_id: &str) -> Result<ResumeTarget, String> {
    validate_resume_session_id(session_id)?;
    let sessions = scan_provider_sessions(provider_id)?;
    resume_target_from_sessions(&sessions, provider_id, session_id)
}

fn scan_provider_sessions(provider_id: &str) -> Result<Vec<SessionMeta>, String> {
    let sessions = match provider_id {
        "codex" => codex::scan_sessions(),
        "claude" => claude::scan_sessions(),
        "opencode" => opencode::scan_sessions(),
        "gemini" => gemini::scan_sessions(),
        "grokbuild" => grokbuild::scan_sessions(),
        "openclaw" | "hermes" => {
            return Err(format!(
                "Provider {provider_id} does not support terminal resume"
            ))
        }
        _ => return Err(format!("Unsupported provider: {provider_id}")),
    };
    Ok(sessions)
}

fn resume_target_from_sessions(
    sessions: &[SessionMeta],
    provider_id: &str,
    session_id: &str,
) -> Result<ResumeTarget, String> {
    validate_resume_session_id(session_id)?;
    let meta = sessions
        .iter()
        .find(|meta| meta.provider_id == provider_id && meta.session_id == session_id)
        .ok_or_else(|| format!("Session not found: {provider_id}/{session_id}"))?;
    let command = build_resume_command(provider_id, &meta.session_id)
        .ok_or_else(|| format!("Provider {provider_id} does not support terminal resume"))?;
    let cwd = meta
        .project_dir
        .clone()
        .filter(|dir| !dir.trim().is_empty());
    Ok(ResumeTarget { command, cwd })
}

pub fn load_messages(provider_id: &str, source_path: &str) -> Result<Vec<SessionMessage>, String> {
    // SQLite sessions use a "sqlite:" prefixed source_path
    if provider_id == "opencode" && source_path.starts_with("sqlite:") {
        return opencode::load_messages_sqlite(source_path);
    }
    if provider_id == "hermes" && source_path.starts_with("sqlite:") {
        return hermes::load_messages_sqlite(source_path);
    }

    let path = Path::new(source_path);
    match provider_id {
        "codex" => codex::load_messages(path),
        "claude" => claude::load_messages(path),
        "opencode" => opencode::load_messages(path),
        "openclaw" => openclaw::load_messages(path),
        "gemini" => gemini::load_messages(path),
        "grokbuild" => grokbuild::load_messages(path),
        "hermes" => hermes::load_messages(path),
        _ => Err(format!("Unsupported provider: {provider_id}")),
    }
}

pub fn delete_session(
    provider_id: &str,
    session_id: &str,
    source_path: &str,
) -> Result<bool, String> {
    // SQLite sessions bypass the file-based deletion path
    if provider_id == "opencode" && source_path.starts_with("sqlite:") {
        return opencode::delete_session_sqlite(session_id, source_path);
    }
    if provider_id == "hermes" && source_path.starts_with("sqlite:") {
        return hermes::delete_session_sqlite(session_id, source_path);
    }

    let roots = provider_roots(provider_id)?;
    delete_session_with_roots(provider_id, session_id, Path::new(source_path), &roots)
}

pub fn delete_sessions(requests: &[DeleteSessionRequest]) -> Vec<DeleteSessionOutcome> {
    collect_delete_session_outcomes(requests, |request| {
        delete_session(
            &request.provider_id,
            &request.session_id,
            &request.source_path,
        )
    })
}

fn delete_session_with_roots(
    provider_id: &str,
    session_id: &str,
    source_path: &Path,
    roots: &[PathBuf],
) -> Result<bool, String> {
    let validated_source = canonicalize_existing_path(source_path, "session source")?;

    let mut saw_existing_root = false;
    for root in roots {
        if !root.exists() {
            continue;
        }

        saw_existing_root = true;
        let validated_root = canonicalize_existing_path(root, "session root")?;
        if validated_source.starts_with(&validated_root) {
            return match provider_id {
                "codex" => codex::delete_session(&validated_root, &validated_source, session_id),
                "claude" => claude::delete_session(&validated_root, &validated_source, session_id),
                "opencode" => {
                    opencode::delete_session(&validated_root, &validated_source, session_id)
                }
                "openclaw" => {
                    openclaw::delete_session(&validated_root, &validated_source, session_id)
                }
                "gemini" => gemini::delete_session(&validated_root, &validated_source, session_id),
                "grokbuild" => {
                    grokbuild::delete_session(&validated_root, &validated_source, session_id)
                }
                "hermes" => hermes::delete_session(&validated_root, &validated_source, session_id),
                _ => Err(format!("Unsupported provider: {provider_id}")),
            };
        }
    }

    if !saw_existing_root {
        return Err(format!(
            "Session root not found for provider {provider_id}: {}",
            roots
                .first()
                .map(|root| root.display().to_string())
                .unwrap_or_else(|| "<none>".to_string())
        ));
    }

    Err(format!(
        "Session source path is outside provider roots: {}",
        source_path.display()
    ))
}

fn provider_roots(provider_id: &str) -> Result<Vec<PathBuf>, String> {
    let roots = match provider_id {
        "codex" => codex::session_roots(),
        "claude" => vec![crate::config::get_claude_config_dir().join("projects")],
        "opencode" => vec![opencode::get_opencode_data_dir()],
        "openclaw" => vec![crate::openclaw_config::get_openclaw_dir().join("agents")],
        "gemini" => vec![crate::gemini_config::get_gemini_dir().join("tmp")],
        "grokbuild" => grokbuild::session_roots(),
        "hermes" => vec![crate::hermes_config::get_hermes_dir().join("sessions")],
        _ => return Err(format!("Unsupported provider: {provider_id}")),
    };

    Ok(roots)
}

fn canonicalize_existing_path(path: &Path, label: &str) -> Result<PathBuf, String> {
    if !path.exists() {
        return Err(format!("{label} not found: {}", path.display()));
    }

    path.canonicalize()
        .map_err(|e| format!("Failed to resolve {label} {}: {e}", path.display()))
}

fn collect_delete_session_outcomes<F>(
    requests: &[DeleteSessionRequest],
    mut deleter: F,
) -> Vec<DeleteSessionOutcome>
where
    F: FnMut(&DeleteSessionRequest) -> Result<bool, String>,
{
    requests
        .iter()
        .map(|request| match deleter(request) {
            Ok(true) => DeleteSessionOutcome {
                provider_id: request.provider_id.clone(),
                session_id: request.session_id.clone(),
                source_path: request.source_path.clone(),
                success: true,
                error: None,
            },
            Ok(false) => DeleteSessionOutcome {
                provider_id: request.provider_id.clone(),
                session_id: request.session_id.clone(),
                source_path: request.source_path.clone(),
                success: false,
                error: Some("Session was not deleted".to_string()),
            },
            Err(error) => DeleteSessionOutcome {
                provider_id: request.provider_id.clone(),
                session_id: request.session_id.clone(),
                source_path: request.source_path.clone(),
                success: false,
                error: Some(error),
            },
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write_codex_session(path: &Path, session_id: &str) {
        std::fs::write(
            path,
            format!(
                "{{\"timestamp\":\"2026-03-06T21:50:12Z\",\"type\":\"session_meta\",\"payload\":{{\"id\":\"{session_id}\",\"cwd\":\"/tmp/project\"}}}}\n\
                 {{\"timestamp\":\"2026-03-06T21:50:13Z\",\"type\":\"response_item\",\"payload\":{{\"type\":\"message\",\"role\":\"user\",\"content\":\"hello\"}}}}\n",
            ),
        )
        .expect("write source");
    }

    #[test]
    fn accepts_source_path_under_any_allowed_provider_root() {
        let active_root = tempdir().expect("active root");
        let archived_root = tempdir().expect("archived root");
        let source = archived_root.path().join("session.jsonl");
        write_codex_session(&source, "archived-session");

        let deleted = delete_session_with_roots(
            "codex",
            "archived-session",
            &source,
            &[
                active_root.path().to_path_buf(),
                archived_root.path().to_path_buf(),
            ],
        )
        .expect("delete archived session");

        assert!(deleted);
        assert!(!source.exists());
    }

    #[test]
    fn rejects_source_path_outside_provider_root() {
        let root = tempdir().expect("tempdir");
        let outside = tempdir().expect("tempdir");
        let source = outside.path().join("session.jsonl");
        std::fs::write(&source, "{}").expect("write source");

        let err =
            delete_session_with_roots("codex", "session-1", &source, &[root.path().to_path_buf()])
                .expect_err("expected outside-root path to be rejected");

        assert!(err.contains("outside provider roots"));
    }

    #[test]
    fn rejects_missing_source_path() {
        let root = tempdir().expect("tempdir");
        let missing = root.path().join("missing.jsonl");

        let err =
            delete_session_with_roots("codex", "session-1", &missing, &[root.path().to_path_buf()])
                .expect_err("expected missing source path to fail");

        assert!(err.contains("session source not found"));
    }

    #[test]
    fn batch_delete_collects_successes_and_failures_in_order() {
        let requests = vec![
            DeleteSessionRequest {
                provider_id: "codex".to_string(),
                session_id: "s1".to_string(),
                source_path: "/tmp/s1".to_string(),
            },
            DeleteSessionRequest {
                provider_id: "claude".to_string(),
                session_id: "s2".to_string(),
                source_path: "/tmp/s2".to_string(),
            },
            DeleteSessionRequest {
                provider_id: "gemini".to_string(),
                session_id: "s3".to_string(),
                source_path: "/tmp/s3".to_string(),
            },
        ];

        let outcomes = collect_delete_session_outcomes(&requests, |request| {
            match request.session_id.as_str() {
                "s1" => Ok(true),
                "s2" => Err("boom".to_string()),
                _ => Ok(false),
            }
        });

        assert_eq!(outcomes.len(), 3);
        assert!(outcomes[0].success);
        assert_eq!(outcomes[0].error, None);
        assert!(!outcomes[1].success);
        assert_eq!(outcomes[1].error.as_deref(), Some("boom"));
        assert!(!outcomes[2].success);
        assert_eq!(
            outcomes[2].error.as_deref(),
            Some("Session was not deleted")
        );
    }

    fn session_meta(provider_id: &str, session_id: &str, project_dir: Option<&str>) -> SessionMeta {
        SessionMeta {
            provider_id: provider_id.to_string(),
            session_id: session_id.to_string(),
            title: None,
            summary: None,
            project_dir: project_dir.map(str::to_string),
            created_at: None,
            last_active_at: None,
            source_path: None,
            resume_command: build_resume_command(provider_id, session_id),
        }
    }

    #[test]
    fn resume_command_uses_fixed_templates_for_plain_ids() {
        assert_eq!(
            build_resume_command("claude", "abc-123").as_deref(),
            Some("claude --resume abc-123")
        );
        assert_eq!(
            build_resume_command("codex", "019a_x.y").as_deref(),
            Some("codex resume 019a_x.y")
        );
        assert_eq!(
            build_resume_command("gemini", "g1").as_deref(),
            Some("gemini --resume g1")
        );
        assert_eq!(
            build_resume_command("grokbuild", "k1").as_deref(),
            Some("grok --resume k1")
        );
        assert_eq!(
            build_resume_command("opencode", "ses_1").as_deref(),
            Some("opencode -s ses_1")
        );
        assert_eq!(build_resume_command("openclaw", "x"), None);
        assert_eq!(build_resume_command("hermes", "x"), None);
        assert_eq!(build_resume_command("unknown", "x"), None);
    }

    #[test]
    fn resume_command_single_quotes_shell_metacharacters_in_session_id() {
        assert_eq!(
            build_resume_command("claude", "abc;rm -rf ~").as_deref(),
            Some("claude --resume 'abc;rm -rf ~'")
        );
        assert_eq!(
            build_resume_command("codex", "a'$(id)'b").as_deref(),
            Some("codex resume 'a'\"'\"'$(id)'\"'\"'b'")
        );
    }

    #[test]
    fn resume_target_quotes_malicious_session_id_and_uses_backend_cwd() {
        let sessions = vec![
            session_meta("codex", "abc;rm -rf ~", Some("/tmp/project")),
            session_meta("claude", "abc;rm -rf ~", Some("/tmp/other")),
        ];

        let target = resume_target_from_sessions(&sessions, "claude", "abc;rm -rf ~")
            .expect("resolve target");

        assert_eq!(
            target,
            ResumeTarget {
                command: "claude --resume 'abc;rm -rf ~'".to_string(),
                cwd: Some("/tmp/other".to_string()),
            }
        );
    }

    #[test]
    fn resume_target_rejects_unknown_or_unsafe_session_ids() {
        let sessions = vec![
            session_meta("claude", "known", None),
            session_meta("claude", "-x", None),
            session_meta("claude", "a\nb", None),
        ];

        assert!(resume_target_from_sessions(&sessions, "claude", "known;rm -rf ~").is_err());
        assert!(resume_target_from_sessions(&sessions, "codex", "known").is_err());
        assert!(resume_target_from_sessions(&sessions, "claude", "-x").is_err());
        assert!(resume_target_from_sessions(&sessions, "claude", "a\nb").is_err());
        assert!(resume_target_from_sessions(&sessions, "claude", "  ").is_err());
        assert_eq!(
            resume_target_from_sessions(&sessions, "claude", "known")
                .expect("known session")
                .cwd,
            None
        );
    }

    #[test]
    fn resolve_resume_target_rejects_providers_without_cli_resume() {
        assert!(resolve_resume_target("openclaw", "abc").is_err());
        assert!(resolve_resume_target("hermes", "abc").is_err());
        assert!(resolve_resume_target("unknown", "abc").is_err());
        assert!(resolve_resume_target("claude", "-abc").is_err());
    }
}
