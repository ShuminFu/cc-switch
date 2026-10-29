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
    if let Some(expected_db) = sqlite_db_path(provider_id, source_path) {
        return load_messages_sqlite_with_db(provider_id, source_path, &expected_db);
    }

    let roots = provider_roots(provider_id)?;
    load_messages_with_roots(provider_id, Path::new(source_path), &roots)
}

pub fn delete_session(
    provider_id: &str,
    session_id: &str,
    source_path: &str,
) -> Result<bool, String> {
    if let Some(expected_db) = sqlite_db_path(provider_id, source_path) {
        return delete_session_sqlite_with_db(provider_id, session_id, source_path, &expected_db);
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

fn load_messages_with_roots(
    provider_id: &str,
    source_path: &Path,
    roots: &[PathBuf],
) -> Result<Vec<SessionMessage>, String> {
    let (_, validated_source) = validate_source_in_roots(provider_id, source_path, roots)?;
    let path = validated_source.as_path();
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

fn delete_session_with_roots(
    provider_id: &str,
    session_id: &str,
    source_path: &Path,
    roots: &[PathBuf],
) -> Result<bool, String> {
    let (validated_root, validated_source) =
        validate_source_in_roots(provider_id, source_path, roots)?;
    match provider_id {
        "codex" => codex::delete_session(&validated_root, &validated_source, session_id),
        "claude" => claude::delete_session(&validated_root, &validated_source, session_id),
        "opencode" => opencode::delete_session(&validated_root, &validated_source, session_id),
        "openclaw" => openclaw::delete_session(&validated_root, &validated_source, session_id),
        "gemini" => gemini::delete_session(&validated_root, &validated_source, session_id),
        "grokbuild" => grokbuild::delete_session(&validated_root, &validated_source, session_id),
        "hermes" => hermes::delete_session(&validated_root, &validated_source, session_id),
        _ => Err(format!("Unsupported provider: {provider_id}")),
    }
}

/// 校验来自 webview 的会话源路径必须位于该 provider 的某个会话根目录之下。
/// 返回规范化后的 `(root, source)`，调用方只应使用规范化路径继续读写。
fn validate_source_in_roots(
    provider_id: &str,
    source_path: &Path,
    roots: &[PathBuf],
) -> Result<(PathBuf, PathBuf), String> {
    let validated_source = canonicalize_existing_path(source_path, "session source")?;

    let mut saw_existing_root = false;
    for root in roots {
        if !root.exists() {
            continue;
        }

        saw_existing_root = true;
        let validated_root = canonicalize_existing_path(root, "session root")?;
        if validated_source.starts_with(&validated_root) {
            return Ok((validated_root, validated_source));
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

/// `sqlite:` 来源只对使用数据库存储的 provider 有效，返回其已知的数据库路径。
fn sqlite_db_path(provider_id: &str, source_path: &str) -> Option<PathBuf> {
    if !source_path.starts_with("sqlite:") {
        return None;
    }
    match provider_id {
        "opencode" => Some(opencode::get_opencode_db_path()),
        "hermes" => Some(hermes::get_hermes_db_path()),
        _ => None,
    }
}

/// 解析 `sqlite:` 来源并要求其中的数据库就是该 provider 的已知数据库，
/// 返回规范化的数据库路径与会话 id。
fn validate_sqlite_source(
    provider_id: &str,
    source_path: &str,
    expected_db: &Path,
) -> Result<(PathBuf, String), String> {
    let parsed = match provider_id {
        "opencode" => opencode::parse_sqlite_source(source_path),
        "hermes" => hermes::parse_sqlite_source(source_path),
        _ => return Err(format!("Unsupported provider: {provider_id}")),
    };
    let (db_path, session_id) =
        parsed.ok_or_else(|| format!("Invalid SQLite source reference: {source_path}"))?;

    let validated_db = canonicalize_existing_path(&db_path, "SQLite database")?;
    let validated_expected = canonicalize_existing_path(expected_db, "expected SQLite database")?;
    if validated_db != validated_expected {
        return Err(format!(
            "SQLite source is outside the known {provider_id} database: {}",
            db_path.display()
        ));
    }

    Ok((validated_db, session_id))
}

fn load_messages_sqlite_with_db(
    provider_id: &str,
    source_path: &str,
    expected_db: &Path,
) -> Result<Vec<SessionMessage>, String> {
    let (db_path, session_id) = validate_sqlite_source(provider_id, source_path, expected_db)?;
    match provider_id {
        "opencode" => opencode::load_messages_sqlite(&db_path, &session_id),
        "hermes" => hermes::load_messages_sqlite(&db_path, &session_id),
        _ => Err(format!("Unsupported provider: {provider_id}")),
    }
}

fn delete_session_sqlite_with_db(
    provider_id: &str,
    session_id: &str,
    source_path: &str,
    expected_db: &Path,
) -> Result<bool, String> {
    let (db_path, ref_session_id) = validate_sqlite_source(provider_id, source_path, expected_db)?;
    if ref_session_id != session_id {
        return Err(format!(
            "SQLite session ID mismatch: expected {session_id}, found {ref_session_id}"
        ));
    }
    match provider_id {
        "opencode" => opencode::delete_session_sqlite(&db_path, session_id),
        "hermes" => hermes::delete_session_sqlite(&db_path, session_id),
        _ => Err(format!("Unsupported provider: {provider_id}")),
    }
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
    fn load_messages_rejects_jsonl_outside_provider_roots() {
        let root = tempdir().expect("root");
        let outside = tempdir().expect("outside");
        let source = outside.path().join("session.jsonl");
        write_codex_session(&source, "outside-session");

        let err = load_messages_with_roots("codex", &source, &[root.path().to_path_buf()])
            .expect_err("expected outside-root path to be rejected");

        assert!(err.contains("outside provider roots"), "{err}");
    }

    #[test]
    fn load_messages_reads_jsonl_inside_provider_root() {
        let root = tempdir().expect("root");
        let source = root.path().join("session.jsonl");
        write_codex_session(&source, "inside-session");

        let messages = load_messages_with_roots("codex", &source, &[root.path().to_path_buf()])
            .expect("load session inside root");

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[0].content, "hello");
    }

    #[test]
    fn load_messages_rejects_parent_traversal_out_of_root() {
        let base = tempdir().expect("base");
        let root = base.path().join("sessions");
        std::fs::create_dir_all(&root).expect("create root");
        let source = base.path().join("secret.jsonl");
        write_codex_session(&source, "secret");

        let traversal = root.join("..").join("secret.jsonl");
        let err = load_messages_with_roots("codex", &traversal, &[root])
            .expect_err("expected traversal to be rejected");

        assert!(err.contains("outside provider roots"), "{err}");
    }

    #[cfg(unix)]
    #[test]
    fn load_messages_rejects_symlink_escaping_root() {
        let root = tempdir().expect("root");
        let outside = tempdir().expect("outside");
        let target = outside.path().join("session.jsonl");
        write_codex_session(&target, "linked");
        let link = root.path().join("link.jsonl");
        std::os::unix::fs::symlink(&target, &link).expect("create symlink");

        let err = load_messages_with_roots("codex", &link, &[root.path().to_path_buf()])
            .expect_err("expected symlink escaping root to be rejected");

        assert!(err.contains("outside provider roots"), "{err}");
    }

    fn create_hermes_db(path: &Path, session_id: &str) {
        let conn = rusqlite::Connection::open(path).expect("open sqlite db");
        conn.execute_batch(
            "CREATE TABLE sessions (id TEXT PRIMARY KEY);
             CREATE TABLE messages (session_id TEXT, role TEXT, content TEXT, created_at INTEGER);",
        )
        .expect("create schema");
        conn.execute("INSERT INTO sessions (id) VALUES (?1)", [session_id])
            .expect("insert session");
        conn.execute(
            "INSERT INTO messages (session_id, role, content, created_at) VALUES (?1, 'user', 'hi', 1000)",
            [session_id],
        )
        .expect("insert message");
    }

    #[test]
    fn sqlite_load_reads_known_database() {
        let known = tempdir().expect("known");
        let db = known.path().join("state.db");
        create_hermes_db(&db, "s1");

        let source = format!("sqlite:{}#s1", db.display());
        let messages =
            load_messages_sqlite_with_db("hermes", &source, &db).expect("load known database");

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].content, "hi");
    }

    #[test]
    fn sqlite_load_rejects_database_outside_known_location() {
        let known = tempdir().expect("known");
        let outside = tempdir().expect("outside");
        let expected = known.path().join("state.db");
        create_hermes_db(&expected, "s1");
        let foreign = outside.path().join("state.db");
        create_hermes_db(&foreign, "s1");

        let source = format!("sqlite:{}#s1", foreign.display());
        let err = load_messages_sqlite_with_db("hermes", &source, &expected)
            .expect_err("expected foreign database to be rejected");
        assert!(err.contains("outside the known hermes database"), "{err}");

        let opencode_source = format!("sqlite:{}:ses_1", foreign.display());
        let err = load_messages_sqlite_with_db("opencode", &opencode_source, &expected)
            .expect_err("expected foreign opencode database to be rejected");
        assert!(err.contains("outside the known opencode database"), "{err}");
    }

    #[test]
    fn sqlite_load_rejects_missing_database() {
        let known = tempdir().expect("known");
        let expected = known.path().join("state.db");
        create_hermes_db(&expected, "s1");
        let missing = known.path().join("missing.db");

        let source = format!("sqlite:{}#s1", missing.display());
        let err = load_messages_sqlite_with_db("hermes", &source, &expected)
            .expect_err("expected missing database to be rejected");
        assert!(err.contains("SQLite database not found"), "{err}");
    }

    #[test]
    fn sqlite_delete_rejects_database_outside_known_location() {
        let known = tempdir().expect("known");
        let outside = tempdir().expect("outside");
        let expected = known.path().join("state.db");
        create_hermes_db(&expected, "s1");
        let foreign = outside.path().join("state.db");
        create_hermes_db(&foreign, "s1");

        let source = format!("sqlite:{}#s1", foreign.display());
        let err = delete_session_sqlite_with_db("hermes", "s1", &source, &expected)
            .expect_err("expected foreign database to be rejected");
        assert!(err.contains("outside the known hermes database"), "{err}");

        let conn = rusqlite::Connection::open(&foreign).expect("reopen foreign db");
        let remaining: i64 = conn
            .query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))
            .expect("count sessions");
        assert_eq!(remaining, 1);
    }

    #[test]
    fn sqlite_delete_removes_session_from_known_database() {
        let known = tempdir().expect("known");
        let db = known.path().join("state.db");
        create_hermes_db(&db, "s1");

        let source = format!("sqlite:{}#s1", db.display());
        let err = delete_session_sqlite_with_db("hermes", "s2", &source, &db)
            .expect_err("expected session id mismatch to be rejected");
        assert!(err.contains("mismatch"), "{err}");

        let deleted =
            delete_session_sqlite_with_db("hermes", "s1", &source, &db).expect("delete session");
        assert!(deleted);
    }

    #[test]
    fn sqlite_prefix_is_not_a_database_source_for_file_providers() {
        assert!(sqlite_db_path("codex", "sqlite:/tmp/x.db#s1").is_none());
        assert!(sqlite_db_path("hermes", "/tmp/x.jsonl").is_none());
        assert!(sqlite_db_path("hermes", "sqlite:/tmp/x.db#s1").is_some());
        assert!(sqlite_db_path("opencode", "sqlite:/tmp/x.db:ses_1").is_some());
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
