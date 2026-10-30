//! 仓库层测试：UsageQuery（筛选 + keyset 分页）、UsageSource、会话导入事务，
//! 以及「database/ 之外没有 SQL」守卫。

use super::session_cursor;
use super::usage::{
    insert_proxy_row, insert_session_row, ProxyUsageRow, SessionUsageRow, UsageCosts, UsageCursor,
    UsageQuery, UsageSource,
};
use crate::database::Database;
use crate::error::AppError;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

fn proxy_row<'a>(request_id: &'a str, app_type: &'a str, created_at: i64) -> ProxyUsageRow<'a> {
    ProxyUsageRow {
        request_id,
        provider_id: "p1",
        app_type,
        model: "claude-sonnet-4-5",
        request_model: "claude-sonnet-4-5",
        pricing_model: "claude-sonnet-4-5",
        input_tokens: 10,
        output_tokens: 5,
        cache_read_tokens: 0,
        cache_creation_tokens: 0,
        costs: UsageCosts::zero(),
        latency_ms: 100,
        first_token_ms: None,
        status_code: 200,
        error_message: None,
        session_id: None,
        provider_type: None,
        is_streaming: false,
        cost_multiplier: "1",
        created_at,
    }
}

fn session_row(request_id: &str, source: UsageSource, created_at: i64) -> SessionUsageRow<'_> {
    SessionUsageRow {
        request_id,
        source,
        model: "gpt-5.4",
        pricing_model: "gpt-5.4",
        session_id: Some("s1"),
        // 与 proxy_row 的 token 不同，避免被跨源去重过滤
        input_tokens: 77,
        output_tokens: 3,
        cache_read_tokens: 0,
        cache_creation_tokens: 0,
        costs: UsageCosts::zero(),
        created_at,
    }
}

fn ids(rows: &[crate::services::usage_stats::RequestLogDetail]) -> Vec<String> {
    rows.iter().map(|r| r.request_id.clone()).collect()
}

#[test]
fn usage_source_columns_match_stored_values() {
    assert_eq!(UsageSource::Proxy.as_str(), "proxy");
    assert_eq!(UsageSource::ClaudeSession.as_str(), "session_log");
    assert_eq!(
        UsageSource::ClaudeSession.placeholder_provider_id(),
        Some("_session")
    );
    for source in UsageSource::SESSIONS {
        let app = source.app_type().expect("session source has an app");
        assert!(source.as_str().contains("session"));
        assert!(source
            .placeholder_provider_id()
            .is_some_and(|id| id.starts_with('_') && id.ends_with("_session")));
        assert!(source.session_display_name().is_some());
        assert!(!app.is_empty());
    }
    assert_eq!(UsageSource::Proxy.app_type(), None);
}

#[test]
fn session_row_writes_source_columns() -> Result<(), AppError> {
    let db = Database::memory()?;
    let conn = db.conn.lock().expect("lock conn");
    let row = session_row("codex_session:t:1", UsageSource::CodexSession, 1_000);
    assert!(insert_session_row(&conn, &row)?);
    // INSERT OR IGNORE：重复写入不产生第二行
    assert!(!insert_session_row(&conn, &row)?);

    let (provider_id, app_type, data_source, provider_type): (String, String, String, String) =
        conn.query_row(
            "SELECT provider_id, app_type, data_source, provider_type
             FROM proxy_request_logs WHERE request_id = 'codex_session:t:1'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )?;
    assert_eq!(provider_id, "_codex_session");
    assert_eq!(app_type, "codex");
    assert_eq!(data_source, "codex_session");
    assert_eq!(provider_type, "codex_session");
    Ok(())
}

#[test]
fn usage_query_filters_by_source_app_and_time() -> Result<(), AppError> {
    let db = Database::memory()?;
    let conn = db.conn.lock().expect("lock conn");
    insert_proxy_row(&conn, &proxy_row("p-1", "claude", 100))?;
    insert_proxy_row(&conn, &proxy_row("p-2", "claude-desktop", 200))?;
    insert_proxy_row(&conn, &proxy_row("p-3", "codex", 300))?;
    insert_session_row(
        &conn,
        &session_row("codex_session:t:1", UsageSource::CodexSession, 400),
    )?;

    let all = UsageQuery::new().fetch(&conn)?;
    assert_eq!(ids(&all.rows), ["codex_session:t:1", "p-3", "p-2", "p-1"]);
    assert_eq!(all.next, None);
    assert_eq!(UsageQuery::new().count(&conn)?, 4);

    // claude 同时命中 claude-desktop
    let claude = UsageQuery::new().app_type("claude").fetch(&conn)?;
    assert_eq!(ids(&claude.rows), ["p-2", "p-1"]);

    let sessions = UsageQuery::new()
        .source(UsageSource::CodexSession)
        .fetch(&conn)?;
    assert_eq!(ids(&sessions.rows), ["codex_session:t:1"]);
    assert_eq!(
        UsageQuery::new().source(UsageSource::Proxy).count(&conn)?,
        3
    );

    let window = UsageQuery::new().since(200).until(300).fetch(&conn)?;
    assert_eq!(ids(&window.rows), ["p-3", "p-2"]);
    Ok(())
}

#[test]
fn usage_query_keyset_pages_are_stable_while_rows_arrive() -> Result<(), AppError> {
    let db = Database::memory()?;
    let conn = db.conn.lock().expect("lock conn");
    // 同一秒内的行靠 request_id 决定顺序
    let request_ids: Vec<String> = (0..7).map(|i| format!("req-{i:02}")).collect();
    for (i, id) in request_ids.iter().enumerate() {
        insert_proxy_row(&conn, &proxy_row(id, "claude", 1_000 + (i as i64 / 2)))?;
    }

    let first = UsageQuery::new().limit(3).fetch(&conn)?;
    assert_eq!(ids(&first.rows), ["req-06", "req-05", "req-04"]);
    let cursor = first.next.clone().expect("more pages");
    assert_eq!(
        cursor,
        UsageCursor {
            created_at: 1_002,
            request_id: "req-04".to_string(),
        }
    );

    // 翻页期间新写入的行不会把第二页挤偏（OFFSET 分页会重复返回 req-04）
    insert_proxy_row(&conn, &proxy_row("req-new", "claude", 5_000))?;

    let second = UsageQuery::new().limit(3).after(cursor).fetch(&conn)?;
    assert_eq!(ids(&second.rows), ["req-03", "req-02", "req-01"]);
    let third = UsageQuery::new()
        .limit(3)
        .after(second.next.clone().expect("more pages"))
        .fetch(&conn)?;
    assert_eq!(ids(&third.rows), ["req-00"]);
    assert_eq!(third.next, None);

    // count 不受游标影响
    let total = UsageQuery::new()
        .after(UsageCursor {
            created_at: 1_001,
            request_id: "req-02".to_string(),
        })
        .count(&conn)?;
    assert_eq!(total, 8);
    Ok(())
}

#[test]
fn usage_query_offset_matches_request_logs_ipc() -> Result<(), AppError> {
    let db = Database::memory()?;
    {
        let conn = db.conn.lock().expect("lock conn");
        for i in 0..5 {
            insert_proxy_row(&conn, &proxy_row(&format!("r{i}"), "claude", 10 + i))?;
        }
    }
    let filters = crate::services::usage_stats::LogFilters::default();
    let page = db.get_request_logs(&filters, 1, 2)?;
    assert_eq!(page.total, 5);
    assert_eq!(ids(&page.data), ["r2", "r1"]);

    let conn = db.conn.lock().expect("lock conn");
    let via_query = UsageQuery::from_filters(&filters)
        .limit(2)
        .offset(2)
        .fetch(&conn)?;
    assert_eq!(ids(&via_query.rows), ids(&page.data));
    Ok(())
}

fn unpriced_row<'a>(request_id: &'a str, model: &'a str, created_at: i64) -> ProxyUsageRow<'a> {
    ProxyUsageRow {
        model,
        request_model: model,
        pricing_model: model,
        ..proxy_row(request_id, "claude", created_at)
    }
}

#[test]
fn unpriced_filter_and_model_stats_only_cover_unpriced_rows() -> Result<(), AppError> {
    let db = Database::memory()?;
    {
        let conn = db.conn.lock().expect("lock conn");
        insert_proxy_row(&conn, &unpriced_row("u1", "mystery-a", 10))?;
        insert_proxy_row(&conn, &unpriced_row("u2", "mystery-a", 30))?;
        insert_proxy_row(&conn, &unpriced_row("u3", "mystery-b", 20))?;
        // 有成本：已计价
        insert_proxy_row(
            &conn,
            &ProxyUsageRow {
                costs: UsageCosts {
                    total: "0.01".to_string(),
                    ..UsageCosts::zero()
                },
                ..unpriced_row("priced", "mystery-a", 40)
            },
        )?;
        // 失败请求 / 无 token / 倍率 0（有意免费）都不算缺定价
        insert_proxy_row(
            &conn,
            &ProxyUsageRow {
                status_code: 500,
                ..unpriced_row("failed", "mystery-c", 50)
            },
        )?;
        insert_proxy_row(
            &conn,
            &ProxyUsageRow {
                input_tokens: 0,
                output_tokens: 0,
                ..unpriced_row("empty", "mystery-c", 60)
            },
        )?;
        insert_proxy_row(
            &conn,
            &ProxyUsageRow {
                cost_multiplier: "0",
                ..unpriced_row("free", "mystery-c", 70)
            },
        )?;
        // 回填时能查到定价的零成本行：读取时被补价，不再算未计价
        conn.execute(
            "INSERT INTO model_pricing (model_id, display_name, input_cost_per_million, output_cost_per_million)
             VALUES ('late-priced', 'Late Priced', '1', '2')",
            [],
        )?;
        insert_proxy_row(&conn, &unpriced_row("late", "late-priced", 80))?;
    }

    let filters = crate::services::usage_stats::LogFilters {
        unpriced: Some(true),
        ..Default::default()
    };
    let page = db.get_request_logs(&filters, 0, 2)?;
    assert_eq!(page.total, 3, "{:?}", ids(&page.data));
    assert_eq!(ids(&page.data), ["u2", "u3"]);

    let stats = db.get_unpriced_model_stats(&filters)?;
    let models: Vec<_> = stats.iter().map(|s| s.model.as_str()).collect();
    assert_eq!(models, ["mystery-a", "mystery-b"]);
    assert_eq!(stats[0].request_count, 2);
    assert_eq!(stats[0].input_tokens, 20);
    assert_eq!(stats[0].output_tokens, 10);
    assert_eq!((stats[0].first_seen_at, stats[0].last_seen_at), (10, 30));

    // 与其他筛选叠加
    let only_b = db.get_unpriced_model_stats(&crate::services::usage_stats::LogFilters {
        model: Some("mystery-b".to_string()),
        ..filters.clone()
    })?;
    assert_eq!(only_b.len(), 1);
    assert_eq!(only_b[0].model, "mystery-b");

    // 不带 unpriced 时行为不变
    let all = db.get_request_logs(&Default::default(), 0, 50)?;
    assert_eq!(all.total, 8);
    Ok(())
}

// ---------------------------------------------------------------------------
// 会话导入：一个文件一个事务
// ---------------------------------------------------------------------------

fn assistant_line(message_id: &str, input: u32, output: u32, ts: &str) -> String {
    format!(
        r#"{{"type":"assistant","message":{{"id":"{message_id}","model":"claude-sonnet-4-5","stop_reason":"end_turn","usage":{{"input_tokens":{input},"output_tokens":{output}}}}},"timestamp":"{ts}","sessionId":"sess"}}"#
    )
}

fn write_claude_projects(home: &Path) -> Vec<PathBuf> {
    let project = home.join(".claude").join("projects").join("proj");
    fs::create_dir_all(&project).unwrap();
    let mut files = Vec::new();
    for (name, base) in [("a", 100u32), ("b", 200), ("c", 300)] {
        let path = project.join(format!("{name}.jsonl"));
        let lines: Vec<String> = (0..3)
            .map(|i| {
                assistant_line(
                    &format!("msg_{name}{i}"),
                    base + i,
                    7,
                    &format!("2026-06-07T13:0{i}:00Z"),
                )
            })
            .collect();
        fs::write(&path, lines.join("\n") + "\n").unwrap();
        files.push(path);
    }
    files
}

/// (行数, input 总和) for session_log rows whose request_id starts with `session:msg_{prefix}`
fn session_totals(db: &Database, prefix: &str) -> (i64, i64) {
    let conn = db.conn.lock().expect("lock conn");
    conn.query_row(
        "SELECT COUNT(*), COALESCE(SUM(input_tokens), 0) FROM proxy_request_logs
         WHERE data_source = 'session_log' AND request_id LIKE ?1",
        [format!("session:msg_{prefix}%")],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .unwrap()
}

#[test]
fn session_import_killed_between_files_does_not_double_count() -> Result<(), AppError> {
    use crate::services::session_usage::sync_claude_session_logs;

    let home = tempfile::tempdir().unwrap();
    let _paths = crate::paths::scoped(crate::paths::Paths::with_home(home.path()));
    let files = write_claude_projects(home.path());
    let db = Database::memory()?;

    // 模拟进程在写完 b.jsonl 的行、推进其游标之前被杀：游标写入在该点失败。
    {
        let conn = db.conn.lock().expect("lock conn");
        conn.execute_batch(
            "CREATE TEMP TRIGGER crash_on_b_cursor
             BEFORE INSERT ON session_log_sync
             WHEN NEW.file_path LIKE '%b.jsonl'
             BEGIN SELECT RAISE(ABORT, 'simulated crash'); END;",
        )?;
    }
    let first = sync_claude_session_logs(&db)?;
    assert_eq!(first.errors.len(), 1, "b.jsonl 的导入应失败: {first:?}");
    assert_eq!(first.imported, 6);

    // b 的行与游标一起回滚；a / c 完整提交
    assert_eq!(session_totals(&db, "b"), (0, 0));
    {
        let conn = db.conn.lock().expect("lock conn");
        let b_key = files[1].to_string_lossy().to_string();
        assert_eq!(
            session_cursor::get(&conn, &b_key)?,
            session_cursor::SessionCursor::default()
        );
        let a_key = files[0].to_string_lossy().to_string();
        assert_eq!(session_cursor::get(&conn, &a_key)?.last_offset, 3);
    }
    assert_eq!(session_totals(&db, "a"), (3, 100 + 101 + 102));

    // 重启后重跑：只补上 b，不重复计 a / c
    {
        let conn = db.conn.lock().expect("lock conn");
        conn.execute_batch("DROP TRIGGER crash_on_b_cursor;")?;
    }
    let second = sync_claude_session_logs(&db)?;
    assert!(second.errors.is_empty(), "{second:?}");
    assert_eq!(second.imported, 3);
    let third = sync_claude_session_logs(&db)?;
    assert_eq!(third.imported, 0);

    for (prefix, base) in [("a", 100), ("b", 200), ("c", 300)] {
        assert_eq!(
            session_totals(&db, prefix),
            (3, 3 * base + 3),
            "{prefix}.jsonl 应恰好导入一次"
        );
    }

    // 与一次无故障的导入结果一致
    let clean = Database::memory()?;
    sync_claude_session_logs(&clean)?;
    assert_eq!(session_totals(&clean, ""), session_totals(&db, ""));
    Ok(())
}

#[test]
fn ingest_file_does_not_advance_cursor_after_sqlite_rolled_back_the_transaction(
) -> Result<(), AppError> {
    use crate::services::session_usage::{ingest_file, IngestBatch};

    let db = Database::memory()?;
    // 模拟 SQLITE_FULL / IOERR 之类让 SQLite 自动回滚整个事务的错误
    let result = ingest_file(&db, "/x/a.jsonl", |conn| {
        insert_session_row(
            conn,
            &session_row("codex_session:t:1", UsageSource::CodexSession, 1_000),
        )?;
        conn.execute_batch("ROLLBACK")?;
        Ok(IngestBatch {
            value: (),
            wrote_rows: true,
            cursor: Some(session_cursor::SessionCursor {
                last_modified: 9,
                last_offset: 3,
            }),
        })
    });
    assert!(result.is_err());

    let conn = db.conn.lock().expect("lock conn");
    assert_eq!(
        session_cursor::get(&conn, "/x/a.jsonl")?,
        session_cursor::SessionCursor::default(),
        "游标不能在自动提交模式下单独落库"
    );
    assert_eq!(UsageQuery::new().count(&conn)?, 0);
    Ok(())
}

// ---------------------------------------------------------------------------
// 守卫：database/ 之外不写本应用数据库的 SQL
// ---------------------------------------------------------------------------

/// 允许出现 SQL 的 database/ 之外的文件，及原因。这些文件访问的不是本应用的数据库。
const SQL_ALLOWLIST: &[(&str, &str)] = &[
    (
        "session_manager/providers/codex.rs",
        "reads Codex's own state_*.sqlite",
    ),
    (
        "session_manager/providers/hermes.rs",
        "reads Hermes' own sessions database",
    ),
    (
        "session_manager/providers/opencode.rs",
        "reads OpenCode's own opencode.db",
    ),
    (
        "services/session_usage_opencode.rs",
        "reads OpenCode's own opencode.db",
    ),
    (
        "apps/codex/history_migration.rs",
        "migrates Codex's own state_*.sqlite",
    ),
    (
        "proxy_harness.rs",
        "end-to-end test harness inspecting the in-memory database",
    ),
];

const SQL_KEYWORDS: [&str; 3] = ["SELECT ", "INSERT ", "UPDATE "];

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

/// `#[cfg(test)] mod foo;` 声明的测试专用文件 / 目录
fn test_only_modules(file: &Path, source: &str) -> Vec<PathBuf> {
    let stem = file.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let parent = file.parent().unwrap();
    let module_dir = if matches!(stem, "mod" | "lib" | "main") {
        parent.to_path_buf()
    } else {
        parent.join(stem)
    };

    let lines: Vec<&str> = source.lines().map(str::trim).collect();
    let mut out = Vec::new();
    for pair in lines.windows(2) {
        if pair[0] != "#[cfg(test)]" {
            continue;
        }
        let decl = pair[1]
            .trim_start_matches("pub(crate) ")
            .trim_start_matches("pub ");
        if let Some(name) = decl
            .strip_prefix("mod ")
            .and_then(|rest| rest.strip_suffix(';'))
        {
            out.push(module_dir.join(format!("{name}.rs")));
            out.push(module_dir.join(name));
        }
    }
    out
}

/// 行内 SQL 关键字命中（跳过注释与文件末尾的 `#[cfg(test)] mod tests { .. }`）
fn sql_hits(source: &str) -> Vec<(usize, String)> {
    let lines: Vec<&str> = source.lines().collect();
    let mut hits = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed == "#[cfg(test)]"
            && lines
                .get(index + 1)
                .is_some_and(|next| next.trim().starts_with("mod ") && next.contains('{'))
        {
            break;
        }
        if trimmed.starts_with("//") {
            continue;
        }
        if SQL_KEYWORDS.iter().any(|kw| line.contains(kw)) {
            hits.push((index + 1, trimmed.to_string()));
        }
    }
    hits
}

#[test]
fn no_sql_outside_database() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rust_files(&src, &mut files);

    let mut test_only: BTreeSet<PathBuf> = BTreeSet::new();
    for file in &files {
        let source = fs::read_to_string(file).unwrap();
        test_only.extend(test_only_modules(file, &source));
    }

    let mut violations = Vec::new();
    let mut allowlisted_with_sql = BTreeSet::new();
    for file in &files {
        let rel = file
            .strip_prefix(&src)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        if rel.starts_with("database/") {
            continue;
        }
        if test_only
            .iter()
            .any(|t| file == t || (t.is_dir() && file.starts_with(t)))
        {
            continue;
        }
        let source = fs::read_to_string(file).unwrap();
        let hits = sql_hits(&source);
        if hits.is_empty() {
            continue;
        }
        if SQL_ALLOWLIST.iter().any(|(path, _)| *path == rel) {
            allowlisted_with_sql.insert(rel);
            continue;
        }
        for (line, text) in hits {
            violations.push(format!("src-tauri/src/{rel}:{line}: {text}"));
        }
    }

    assert!(
        violations.is_empty(),
        "SQL outside src-tauri/src/database (move it into database/repo or database/dao):\n{}",
        violations.join("\n")
    );
    // 允许列表不能过期：条目必须仍然存在且仍含 SQL
    for (path, reason) in SQL_ALLOWLIST {
        assert!(
            allowlisted_with_sql.contains(*path),
            "stale SQL allowlist entry {path} ({reason}): remove it"
        );
    }
}

#[test]
fn sql_guard_skips_comments_and_trailing_test_module() {
    let source = "fn a() {}\n// SELECT in a comment\n/// INSERT in docs\nconst Q: &str = \"UPDATE t SET x = 1\";\n#[cfg(test)]\nmod tests {\n    const T: &str = \"SELECT 1\";\n}\n";
    assert_eq!(
        sql_hits(source),
        vec![(4, "const Q: &str = \"UPDATE t SET x = 1\";".to_string())]
    );

    let decl = "#[cfg(test)]\nmod cost_golden;\nmod real;\n";
    let found = test_only_modules(Path::new("/x/proxy/usage/mod.rs"), decl);
    assert!(found.contains(&PathBuf::from("/x/proxy/usage/cost_golden.rs")));
    assert!(!found.iter().any(|p| p.ends_with("real.rs")));
}
