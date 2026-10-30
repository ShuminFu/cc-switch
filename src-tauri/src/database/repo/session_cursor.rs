//! 会话导入游标：`session_log_sync` 表
//!
//! 每个被导入的会话来源（JSONL 文件、Gemini JSON、OpenCode 库与其中的会话）
//! 一行，记录上次导入时的 mtime（纳秒；旧数据为秒）与已处理的行数 / 消息数。
//! 导入方把游标与该文件的使用量行放在同一个事务里写入（见
//! [`crate::services::session_usage::ingest_file`]），进程在两个文件之间被杀掉时
//! 不会出现「行已写入、游标未推进」导致的重复导入。

use crate::error::AppError;
use rusqlite::{params, Connection, OptionalExtension};
use std::time::SystemTime;

/// 一个导入来源的游标
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct SessionCursor {
    /// 上次导入时来源的 mtime（或 OpenCode 会话的 time_updated）
    pub last_modified: i64,
    /// 已处理的完整行数 / 消息数
    pub last_offset: i64,
}

/// 读取游标；没有记录时返回 `(0, 0)`。
pub(crate) fn get(conn: &Connection, key: &str) -> Result<SessionCursor, AppError> {
    conn.query_row(
        "SELECT last_modified, last_line_offset FROM session_log_sync WHERE file_path = ?1",
        params![key],
        |row| {
            Ok(SessionCursor {
                last_modified: row.get(0)?,
                last_offset: row.get(1)?,
            })
        },
    )
    .optional()
    .map(Option::unwrap_or_default)
    .map_err(|e| AppError::Database(format!("查询同步状态失败: {e}")))
}

/// 写入（覆盖）游标。
pub(crate) fn put(conn: &Connection, key: &str, cursor: SessionCursor) -> Result<(), AppError> {
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    conn.execute(
        "INSERT OR REPLACE INTO session_log_sync (file_path, last_modified, last_line_offset, last_synced_at)
         VALUES (?1, ?2, ?3, ?4)",
        params![key, cursor.last_modified, cursor.last_offset, now],
    )
    .map_err(|e| AppError::Database(format!("更新同步状态失败: {e}")))?;
    Ok(())
}

/// Codex 归档文件继承游标：`archived_sessions/<name>` 是从 `sessions/.../<name>`
/// 移过来的同一个文件，按文件名后缀找到原路径上进度最大的游标。
pub(crate) fn find_by_file_name(
    conn: &Connection,
    exclude_key: &str,
    file_name: &str,
) -> Result<Option<SessionCursor>, AppError> {
    let slash_suffix = format!("/{file_name}");
    let backslash_suffix = format!("\\{file_name}");
    conn.query_row(
        "SELECT last_modified, last_line_offset
         FROM session_log_sync
         WHERE file_path <> ?1
           AND (substr(file_path, -length(?2)) = ?2
                OR substr(file_path, -length(?3)) = ?3)
         ORDER BY last_line_offset DESC, last_modified DESC
         LIMIT 1",
        params![exclude_key, slash_suffix, backslash_suffix],
        |row| {
            Ok(SessionCursor {
                last_modified: row.get(0)?,
                last_offset: row.get(1)?,
            })
        },
    )
    .optional()
    .map_err(|e| AppError::Database(format!("查询 Codex 归档文件同步状态失败: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;

    #[test]
    fn missing_cursor_reads_as_zero_and_put_round_trips() -> Result<(), AppError> {
        let db = Database::memory()?;
        let conn = db.conn.lock().expect("lock conn");
        assert_eq!(get(&conn, "/a.jsonl")?, SessionCursor::default());

        let cursor = SessionCursor {
            last_modified: 42,
            last_offset: 7,
        };
        put(&conn, "/a.jsonl", cursor)?;
        assert_eq!(get(&conn, "/a.jsonl")?, cursor);

        // 覆盖写入
        let newer = SessionCursor {
            last_modified: 43,
            last_offset: 9,
        };
        put(&conn, "/a.jsonl", newer)?;
        assert_eq!(get(&conn, "/a.jsonl")?, newer);
        Ok(())
    }

    #[test]
    fn find_by_file_name_matches_either_separator_and_prefers_progress() -> Result<(), AppError> {
        let db = Database::memory()?;
        let conn = db.conn.lock().expect("lock conn");
        put(
            &conn,
            "/home/u/.codex/sessions/2026/01/01/rollout-x.jsonl",
            SessionCursor {
                last_modified: 1,
                last_offset: 3,
            },
        )?;
        put(
            &conn,
            "C:\\Users\\u\\.codex\\sessions\\rollout-x.jsonl",
            SessionCursor {
                last_modified: 2,
                last_offset: 5,
            },
        )?;
        put(
            &conn,
            "/home/u/.codex/sessions/2026/01/01/other-rollout-x.jsonl.bak",
            SessionCursor {
                last_modified: 9,
                last_offset: 99,
            },
        )?;

        let archived = "/home/u/.codex/archived_sessions/rollout-x.jsonl";
        let found = find_by_file_name(&conn, archived, "rollout-x.jsonl")?;
        assert_eq!(
            found,
            Some(SessionCursor {
                last_modified: 2,
                last_offset: 5,
            })
        );
        assert_eq!(find_by_file_name(&conn, archived, "missing.jsonl")?, None);
        Ok(())
    }
}
