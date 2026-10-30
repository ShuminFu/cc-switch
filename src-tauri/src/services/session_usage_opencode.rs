//! OpenCode 会话日志使用追踪
//!
//! 从 ~/.local/share/opencode/opencode.db (SQLite) 中提取精确 token 使用数据。
//!
//! ## 数据流
//! ```text
//! ~/.local/share/opencode/opencode.db
//!   → session 表获取所有会话
//!   → message 表获取 assistant 消息
//!   → 解析 data JSON 提取 tokens/cost/model
//!   → proxy_request_logs 表
//! ```

use crate::database::Database;
use crate::error::AppError;
use crate::opencode_config::get_opencode_db_path;
use crate::services::session_usage::ingest::{
    commit_session_rows, load_sync_cursors, now_secs, CursorUpdate, PricingCache, SessionSource,
    SessionUsageRow, WritePolicy,
};
use crate::services::session_usage::{
    metadata_modified_nanos, update_sync_state, SessionSyncResult,
};
use std::fs;
use std::path::Path;
use std::time::Duration;

/// 只导入已完成的消息，写入后不再变化。
const OPENCODE_SESSION_SOURCE: SessionSource = SessionSource {
    app_type: "opencode",
    provider_id: "_opencode_session",
    data_source: "opencode_session",
    policy: WritePolicy::InsertOnly,
};

/// opencode 运行时可能持有写锁；读取时最多等待这么久（与会话管理器一致）。
const OPENCODE_DB_BUSY_TIMEOUT: Duration = Duration::from_secs(2);

/// 从 opencode message.data JSON 中提取的 token 和费用数据
struct OpenCodeMessageData {
    input_tokens: u32,
    output_tokens: u32,
    reasoning_tokens: u32,
    cache_read_tokens: u32,
    cache_write_tokens: u32,
    cost: f64,
    model_id: String,
    timestamp_ms: i64,
}

struct OpenCodeMessageQueryResult {
    messages: Vec<(String, OpenCodeMessageData)>,
    has_incomplete_usage: bool,
}

/// 以只读方式打开 opencode.db，并设置 busy timeout，避免 opencode 写入时立即 SQLITE_BUSY。
fn open_opencode_db(db_path: &Path) -> Result<rusqlite::Connection, AppError> {
    use rusqlite::OpenFlags;
    let conn = rusqlite::Connection::open_with_flags(
        db_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| AppError::Database(format!("无法打开 opencode.db: {e}")))?;
    conn.busy_timeout(OPENCODE_DB_BUSY_TIMEOUT)
        .map_err(|e| AppError::Database(format!("设置 opencode.db busy timeout 失败: {e}")))?;
    Ok(conn)
}

/// 同步 OpenCode 使用数据
pub fn sync_opencode_usage(db: &Database) -> Result<SessionSyncResult, AppError> {
    sync_opencode_db(db, &get_opencode_db_path())
}

fn sync_opencode_db(db: &Database, db_path: &Path) -> Result<SessionSyncResult, AppError> {
    if !db_path.exists() {
        return Ok(SessionSyncResult::default());
    }

    let db_path_str = db_path.to_string_lossy().to_string();

    // 检查文件修改时间。
    // opencode 的数据库运行在 WAL 模式：新提交先落在 -wal 文件里，
    // 主库文件只有在 checkpoint 时才更新。因此必须同时考虑 -wal 的
    // mtime，否则会在 checkpoint 之前漏掉刚写入的会话。
    let metadata = fs::metadata(db_path)
        .map_err(|e| AppError::Config(format!("无法读取 opencode.db 元数据: {e}")))?;
    let mut file_modified = metadata_modified_nanos(&metadata);

    let wal_path = db_path.with_extension("db-wal");
    if let Ok(wal_meta) = fs::metadata(&wal_path) {
        file_modified = file_modified.max(metadata_modified_nanos(&wal_meta));
    }

    // 一次查询载入全部游标（含每个会话的游标）
    let cursors = load_sync_cursors(db)?;
    let last_modified = cursors.get(&db_path_str).map(|s| s.0).unwrap_or(0);

    let mut result = SessionSyncResult {
        files_scanned: 1,
        ..Default::default()
    };

    // 文件未变化则跳过
    if file_modified <= last_modified {
        return Ok(result);
    }

    // 打开 opencode 的 SQLite 数据库（只读 + busy timeout）
    let opencode_conn = open_opencode_db(db_path)?;
    let mut pricing = PricingCache::default();
    let mut has_sync_errors = false;

    // 查询所有会话
    let sessions = query_sessions(&opencode_conn)?;

    for (session_id, time_updated) in &sessions {
        // 检查会话是否需要重新同步
        let sync_key = format!("{db_path_str}:{session_id}");
        let sess_last_modified = cursors.get(&sync_key).map(|s| s.0).unwrap_or(0);
        if *time_updated <= sess_last_modified {
            continue; // 会话未更新，跳过
        }

        // 查询该会话的所有 assistant 消息
        let query_result = match query_assistant_messages(&opencode_conn, session_id) {
            Ok(query_result) => query_result,
            Err(e) => {
                let msg = format!("OpenCode 会话消息查询失败 {session_id}: {e}");
                log::warn!("[OPENCODE-SYNC] {msg}");
                result.errors.push(msg);
                has_sync_errors = true;
                continue;
            }
        };

        let rows: Vec<SessionUsageRow> = query_result
            .messages
            .iter()
            .map(|(message_id, msg_data)| opencode_row(session_id, message_id, msg_data))
            .collect();

        // 仍有未完成消息时不推进会话游标，下次同步会再读取它们。
        let cursor = (!query_result.has_incomplete_usage).then(|| CursorUpdate {
            key: &sync_key,
            last_modified: *time_updated,
            last_offset: 0,
        });

        // 消息与会话游标在同一事务内提交；失败时整体回滚，下次重试。
        match commit_session_rows(db, &OPENCODE_SESSION_SOURCE, &rows, &mut pricing, cursor) {
            Ok((imported, skipped)) => {
                result.imported += imported;
                result.skipped += skipped;
            }
            Err(e) => {
                let msg = format!("OpenCode 会话写入失败 {session_id}: {e}");
                log::warn!("[OPENCODE-SYNC] {msg}");
                result.errors.push(msg);
                has_sync_errors = true;
            }
        }
    }

    // 仅在本轮完全成功时推进文件级状态；否则保留下次重试入口。
    if !has_sync_errors {
        update_sync_state(db, &db_path_str, file_modified, 0)?;
    }

    if result.imported > 0 {
        log::info!(
            "[OPENCODE-SYNC] 同步完成: 导入 {} 条, 跳过 {} 条, 扫描 {} 个会话",
            result.imported,
            result.skipped,
            sessions.len()
        );
    }

    Ok(result)
}

/// 查询所有会话的 (id, sync_watermark)
fn query_sessions(conn: &rusqlite::Connection) -> Result<Vec<(String, i64)>, AppError> {
    let mut stmt = conn
        .prepare(
            "SELECT s.id,
                    MAX(s.time_updated, COALESCE(MAX(m.time_updated), s.time_updated)) AS sync_watermark
             FROM session s
             LEFT JOIN message m ON m.session_id = s.id
             GROUP BY s.id
             ORDER BY sync_watermark",
        )
        .map_err(|e| AppError::Database(format!("准备会话查询失败: {e}")))?;

    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
        .map_err(|e| AppError::Database(format!("查询会话失败: {e}")))?;

    let mut sessions = Vec::new();
    for row in rows {
        sessions.push(row.map_err(|e| AppError::Database(format!("读取会话行失败: {e}")))?);
    }

    Ok(sessions)
}

/// 查询某会话的已完成 assistant 消息，并标记是否还有未完成 usage 消息。
fn query_assistant_messages(
    conn: &rusqlite::Connection,
    session_id: &str,
) -> Result<OpenCodeMessageQueryResult, AppError> {
    let mut stmt = conn
        .prepare("SELECT id, data FROM message WHERE session_id = ?1 ORDER BY time_created")
        .map_err(|e| AppError::Database(format!("准备消息查询失败: {e}")))?;

    let rows = stmt
        .query_map([session_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|e| AppError::Database(format!("查询消息失败: {e}")))?;

    let mut messages = Vec::new();
    let mut has_incomplete_usage = false;
    for row in rows {
        let (message_id, data_json) =
            row.map_err(|e| AppError::Database(format!("读取消息行失败: {e}")))?;

        // 只处理 assistant 消息
        let value: serde_json::Value = match serde_json::from_str(&data_json) {
            Ok(v) => v,
            Err(_) => continue,
        };

        if value.get("role").and_then(|r| r.as_str()) != Some("assistant") {
            continue;
        }

        // 必须有 tokens 字段
        if value.get("tokens").is_none() {
            continue;
        }

        // 跳过未完成的消息：进行中只有半截 token，且因 INSERT OR IGNORE 无法回填
        if value.get("time").and_then(|t| t.get("completed")).is_none() {
            has_incomplete_usage = true;
            continue;
        }

        if let Some(msg_data) = parse_message_data(&value) {
            messages.push((message_id, msg_data));
        }
    }

    Ok(OpenCodeMessageQueryResult {
        messages,
        has_incomplete_usage,
    })
}

/// 解析 opencode message.data JSON 为结构化数据
fn parse_message_data(value: &serde_json::Value) -> Option<OpenCodeMessageData> {
    let tokens = value.get("tokens")?;

    let input_tokens = tokens.get("input").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
    let output_tokens = tokens.get("output").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
    let reasoning_tokens = tokens
        .get("reasoning")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;

    let cache_obj = tokens.get("cache");
    let cache_read_tokens = cache_obj
        .and_then(|c| c.get("read"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    let cache_write_tokens = cache_obj
        .and_then(|c| c.get("write"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;

    // 跳过全零 token 的消息
    if input_tokens == 0
        && output_tokens == 0
        && reasoning_tokens == 0
        && cache_read_tokens == 0
        && cache_write_tokens == 0
    {
        return None;
    }

    let cost = value.get("cost").and_then(|v| v.as_f64()).unwrap_or(0.0);

    let model_id = value
        .get("modelID")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();

    let timestamp_ms = value
        .get("time")
        .and_then(|t| t.get("created"))
        .and_then(|v| v.as_i64())
        .unwrap_or(0);

    Some(OpenCodeMessageData {
        input_tokens,
        output_tokens,
        reasoning_tokens,
        cache_read_tokens,
        cache_write_tokens,
        cost,
        model_id,
        timestamp_ms,
    })
}

/// 把一条 OpenCode 消息转换为待写入的用量行
fn opencode_row(session_id: &str, message_id: &str, msg: &OpenCodeMessageData) -> SessionUsageRow {
    SessionUsageRow {
        request_id: format!("opencode_session:{session_id}:{message_id}"),
        model: msg.model_id.clone(),
        pricing_model: msg.model_id.clone(),
        // OpenCode 使用 Anthropic 风格：input 是新鲜输入，cache 单独计
        input_tokens: msg.input_tokens,
        // output 包含 reasoning tokens（按输出计费）
        output_tokens: msg.output_tokens + msg.reasoning_tokens,
        cache_read_tokens: msg.cache_read_tokens,
        cache_creation_tokens: msg.cache_write_tokens,
        session_id: Some(session_id.to_string()),
        created_at: if msg.timestamp_ms > 0 {
            msg.timestamp_ms / 1000
        } else {
            now_secs()
        },
        // opencode 已计算费用时直接使用（聚合值，无法拆分）；为 0 时按 cc-switch 定价计算
        precomputed_total_cost: Some(msg.cost),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opencode_fixture(dir: &Path) -> std::path::PathBuf {
        let path = dir.join("opencode.db");
        let conn = rusqlite::Connection::open(&path).unwrap();
        let done = serde_json::json!({
            "role": "assistant",
            "cost": 0.0,
            "tokens": { "input": 1000, "output": 200, "reasoning": 50 },
            "modelID": "m",
            "time": { "created": 1_779_755_333_700i64, "completed": 1_779_755_350_639i64 }
        })
        .to_string();
        conn.execute_batch(
            "CREATE TABLE session (id TEXT, time_updated INTEGER);
             CREATE TABLE message (id TEXT, session_id TEXT, time_created INTEGER,
                                   time_updated INTEGER, data TEXT);
             INSERT INTO session VALUES ('s1', 100);",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO message VALUES ('m1', 's1', 1, 100, ?1)",
            [done],
        )
        .unwrap();
        path
    }

    #[test]
    fn test_sync_reads_db_read_only_and_is_idempotent() -> Result<(), AppError> {
        let db = Database::memory()?;
        let tmp = tempfile::tempdir().unwrap();
        let path = opencode_fixture(tmp.path());

        let first = sync_opencode_db(&db, &path)?;
        assert_eq!((first.imported, first.errors.len()), (1, 0));
        {
            let conn = db.conn.lock().unwrap();
            let output: i64 = conn.query_row(
                "SELECT output_tokens FROM proxy_request_logs
                 WHERE request_id = 'opencode_session:s1:m1'",
                [],
                |r| r.get(0),
            )?;
            assert_eq!(output, 250);
        }

        // 游标已推进：强制重读文件级状态后，会话级游标仍阻止重复导入
        update_sync_state(&db, &path.to_string_lossy(), 0, 0)?;
        let second = sync_opencode_db(&db, &path)?;
        assert_eq!((second.imported, second.skipped), (0, 0));
        Ok(())
    }

    #[test]
    fn test_open_opencode_db_is_read_only() {
        let tmp = tempfile::tempdir().unwrap();
        let path = opencode_fixture(tmp.path());
        let conn = open_opencode_db(&path).unwrap();
        assert!(conn.execute("DELETE FROM message", []).is_err());
    }

    #[test]
    fn test_parse_message_data_full() {
        let json: serde_json::Value = serde_json::json!({
            "role": "assistant",
            "cost": 0.0023113,
            "tokens": {
                "total": 56554,
                "input": 3272,
                "output": 383,
                "reasoning": 419,
                "cache": {
                    "write": 0,
                    "read": 52480
                }
            },
            "modelID": "deepseek-v4-pro",
            "providerID": "deepseek",
            "time": {
                "created": 1779755333700i64,
                "completed": 1779755350639i64
            }
        });
        let data = parse_message_data(&json).unwrap();
        assert_eq!(data.input_tokens, 3272);
        assert_eq!(data.output_tokens, 383);
        assert_eq!(data.reasoning_tokens, 419);
        assert_eq!(data.cache_read_tokens, 52480);
        assert_eq!(data.cache_write_tokens, 0);
        assert!((data.cost - 0.0023113).abs() < 1e-10);
        assert_eq!(data.model_id, "deepseek-v4-pro");
        assert_eq!(data.timestamp_ms, 1779755333700);
    }

    #[test]
    fn test_parse_message_data_missing_cache() {
        let json: serde_json::Value = serde_json::json!({
            "role": "assistant",
            "cost": 0.0,
            "tokens": {
                "input": 1000,
                "output": 200
            },
            "modelID": "mimo-v2.5-pro",
            "time": { "created": 1779755333700i64 }
        });
        let data = parse_message_data(&json).unwrap();
        assert_eq!(data.input_tokens, 1000);
        assert_eq!(data.output_tokens, 200);
        assert_eq!(data.reasoning_tokens, 0);
        assert_eq!(data.cache_read_tokens, 0);
        assert_eq!(data.cache_write_tokens, 0);
    }

    #[test]
    fn test_parse_message_data_skips_zero_tokens() {
        let json: serde_json::Value = serde_json::json!({
            "role": "assistant",
            "tokens": {
                "input": 0,
                "output": 0,
                "reasoning": 0,
                "cache": { "read": 0, "write": 0 }
            },
            "modelID": "test"
        });
        assert!(parse_message_data(&json).is_none());
    }

    #[test]
    fn test_parse_message_data_ignores_role() {
        // parse_message_data does not filter by role; that's the caller's job
        let json: serde_json::Value = serde_json::json!({
            "role": "user",
            "tokens": { "input": 100, "output": 0 }
        });
        let data = parse_message_data(&json).unwrap();
        assert_eq!(data.input_tokens, 100);
    }

    #[test]
    fn test_query_assistant_messages_skips_incomplete() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE message (id TEXT, session_id TEXT, time_created INTEGER, data TEXT);",
        )
        .unwrap();

        let done = serde_json::json!({
            "role": "assistant",
            "tokens": { "input": 1000, "output": 200 },
            "modelID": "m",
            "time": { "created": 1, "completed": 2 }
        })
        .to_string();
        let in_progress = serde_json::json!({
            "role": "assistant",
            "tokens": { "input": 500, "output": 0 },
            "modelID": "m",
            "time": { "created": 3 }
        })
        .to_string();

        conn.execute(
            "INSERT INTO message VALUES ('done', 's1', 1, ?1), ('wip', 's1', 2, ?2)",
            rusqlite::params![done, in_progress],
        )
        .unwrap();

        let result = query_assistant_messages(&conn, "s1").unwrap();
        // 只返回已完成（带 time.completed）的消息，半截的被跳过
        assert_eq!(result.messages.len(), 1);
        assert_eq!(result.messages[0].0, "done");
        assert!(result.has_incomplete_usage);
    }

    #[test]
    fn test_query_sessions_uses_message_update_watermark() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE session (id TEXT, time_updated INTEGER);
             CREATE TABLE message (
                 id TEXT,
                 session_id TEXT,
                 time_created INTEGER,
                 time_updated INTEGER,
                 data TEXT
             );
             INSERT INTO session VALUES ('s1', 100);
             INSERT INTO message VALUES ('m1', 's1', 90, 200, '{}');",
        )
        .unwrap();

        let sessions = query_sessions(&conn).unwrap();
        assert_eq!(sessions, vec![("s1".to_string(), 200)]);
    }
}
