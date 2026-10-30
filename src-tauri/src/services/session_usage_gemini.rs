//! Gemini CLI 会话日志使用追踪
//!
//! 从 ~/.gemini/tmp/<project_hash>/chats/session-*.json 中提取精确 token 使用数据。
//!
//! ## 数据流
//! ```text
//! ~/.gemini/tmp/*/chats/session-*.json → 全量解析 → 费用计算 → proxy_request_logs 表
//! ```
//!
//! ## 与 Claude/Codex 解析器的差异
//! - JSON 格式（非 JSONL）：每个文件是单个 JSON 对象，包含 messages 数组
//! - 无需 delta 计算：tokens 字段是 per-message 独立值
//! - 无需状态恢复：不依赖前一条消息的累计值
//! - 天然去重：每条消息有唯一 id 字段

use crate::database::Database;
use crate::error::AppError;
use crate::gemini_config::get_gemini_dir;
use crate::services::session_usage::ingest::{
    commit_session_rows, created_at_from_rfc3339, load_sync_cursors, prune_missing_cursors,
    CursorUpdate, PricingCache, SessionSource, SessionUsageRow, WritePolicy,
};
use crate::services::session_usage::{metadata_modified_nanos, SessionSyncResult};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

/// Gemini 每次全量重读会话文件；已存在的消息若 token/模型有变化则更新（SESS-35）。
const GEMINI_SESSION_SOURCE: SessionSource = SessionSource {
    app_type: "gemini",
    provider_id: "_gemini_session",
    data_source: "gemini_session",
    policy: WritePolicy::ReplaceChanged,
};

/// 从 Gemini message 中提取的 token 数据
#[derive(Debug)]
struct GeminiTokens {
    input: u32,
    output: u32,
    cached: u32,
    thoughts: u32,
}

/// 同步 Gemini 使用数据（从 JSON 会话日志）
pub fn sync_gemini_usage(db: &Database) -> Result<SessionSyncResult, AppError> {
    sync_gemini_dir(db, &get_gemini_dir())
}

fn sync_gemini_dir(db: &Database, gemini_dir: &Path) -> Result<SessionSyncResult, AppError> {
    let files = collect_gemini_session_files(gemini_dir);

    let mut result = SessionSyncResult {
        files_scanned: files.len() as u32,
        ..Default::default()
    };

    if files.is_empty() {
        return Ok(result);
    }

    // 一次查询载入全部游标，每个模型的定价在本轮只解析一次（PERF-10）
    let cursors = load_sync_cursors(db)?;
    let mut pricing = PricingCache::default();
    let mut discovered = HashSet::with_capacity(files.len());

    for file_path in &files {
        let key = file_path.to_string_lossy().to_string();
        let last_modified = cursors
            .get(&key)
            .map(|(modified, _)| *modified)
            .unwrap_or(0);
        discovered.insert(key);
        match sync_single_gemini_file(db, file_path, last_modified, &mut pricing) {
            Ok((imported, skipped)) => {
                result.imported += imported;
                result.skipped += skipped;
            }
            Err(e) => {
                let msg = format!("Gemini 会话文件解析失败 {}: {e}", file_path.display());
                log::warn!("[GEMINI-SYNC] {msg}");
                result.errors.push(msg);
            }
        }
    }

    if let Err(e) = prune_missing_cursors(db, &gemini_dir.join("tmp"), &cursors, &discovered) {
        log::warn!("[GEMINI-SYNC] 清理失效同步状态失败: {e}");
    }

    if result.imported > 0 {
        log::info!(
            "[GEMINI-SYNC] 同步完成: 导入 {} 条, 跳过 {} 条, 扫描 {} 个文件",
            result.imported,
            result.skipped,
            result.files_scanned
        );
    }

    Ok(result)
}

/// 收集所有 Gemini 会话 JSON 文件
fn collect_gemini_session_files(gemini_dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();

    let tmp_dir = gemini_dir.join("tmp");
    if !tmp_dir.is_dir() {
        return files;
    }

    // 遍历 tmp/<project_hash>/chats/session-*.json
    let project_dirs = match fs::read_dir(&tmp_dir) {
        Ok(entries) => entries,
        Err(_) => return files,
    };

    for entry in project_dirs.flatten() {
        let chats_dir = entry.path().join("chats");
        if !chats_dir.is_dir() {
            continue;
        }

        let chat_files = match fs::read_dir(&chats_dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };

        for file_entry in chat_files.flatten() {
            let path = file_entry.path();
            let is_session = path
                .file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.starts_with("session-") && n.ends_with(".json"))
                .unwrap_or(false);
            if is_session {
                files.push(path);
            }
        }
    }

    files
}

/// 同步单个 Gemini 会话 JSON 文件，返回 (imported, skipped)
fn sync_single_gemini_file(
    db: &Database,
    file_path: &Path,
    last_modified: i64,
    pricing: &mut PricingCache,
) -> Result<(u32, u32), AppError> {
    let file_path_str = file_path.to_string_lossy().to_string();

    // 获取文件元数据
    let metadata = fs::metadata(file_path)
        .map_err(|e| AppError::Config(format!("无法读取文件元数据: {e}")))?;
    let file_modified = metadata_modified_nanos(&metadata);

    // 文件未变化则跳过
    if file_modified <= last_modified {
        return Ok((0, 0));
    }

    // 读取并解析整个 JSON 文件（写到一半的文件解析失败，游标不推进，下次重试）
    let content = fs::read_to_string(file_path)
        .map_err(|e| AppError::Config(format!("无法读取文件: {e}")))?;
    let value: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| AppError::Config(format!("JSON 解析失败: {e}")))?;

    let rows = parse_gemini_session(&value, file_path);
    let message_count = rows.len() as i64;

    commit_session_rows(
        db,
        &GEMINI_SESSION_SOURCE,
        &rows,
        pricing,
        Some(CursorUpdate {
            key: &file_path_str,
            last_modified: file_modified,
            last_offset: message_count,
        }),
    )
}

/// 把一个 Gemini 会话 JSON 解析为待写入的用量行。
fn parse_gemini_session(value: &serde_json::Value, file_path: &Path) -> Vec<SessionUsageRow> {
    // 提取顶层 sessionId；缺失时用文件名派生稳定标识（SESS-42）
    let session_id = value
        .get("sessionId")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let request_session_key = session_id.clone().unwrap_or_else(|| {
        format!(
            "file-{}",
            file_path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or("unknown")
        )
    });

    let Some(messages) = value.get("messages").and_then(|v| v.as_array()) else {
        return Vec::new();
    };

    let mut rows = Vec::new();
    for (index, msg) in messages.iter().enumerate() {
        // 只处理 type == "gemini" 的消息
        if msg.get("type").and_then(|t| t.as_str()) != Some("gemini") {
            continue;
        }

        // 提取 tokens 对象
        let Some(tokens_obj) = msg.get("tokens").filter(|t| t.is_object()) else {
            continue;
        };

        let tokens = parse_gemini_tokens(tokens_obj);
        if tokens.input == 0 && tokens.output == 0 && tokens.thoughts == 0 && tokens.cached == 0 {
            continue; // 跳过全零的空 token 消息
        }

        let model = msg
            .get("model")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let timestamp = msg.get("timestamp").and_then(|v| v.as_str());

        // 消息缺少 id 时用数组下标 + 时间戳派生，避免同一会话内互相覆盖（SESS-42）
        let message_key = match msg.get("id").and_then(|v| v.as_str()) {
            Some(id) => id.to_string(),
            None => format!("idx{index}-{}", timestamp.unwrap_or("")),
        };

        rows.push(SessionUsageRow {
            request_id: format!("gemini_session:{request_session_key}:{message_key}"),
            model: model.to_string(),
            pricing_model: model.to_string(),
            input_tokens: tokens.input,
            // 合并 thoughts 到 output（思考 token 按输出计费）
            output_tokens: tokens.output + tokens.thoughts,
            cache_read_tokens: tokens.cached,
            cache_creation_tokens: 0,
            session_id: session_id.clone(),
            created_at: created_at_from_rfc3339(timestamp),
            precomputed_total_cost: None,
        });
    }
    rows
}

/// 从 tokens JSON 对象中提取 token 数据
fn parse_gemini_tokens(tokens: &serde_json::Value) -> GeminiTokens {
    GeminiTokens {
        input: tokens.get("input").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
        output: tokens.get("output").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
        cached: tokens.get("cached").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
        thoughts: tokens.get("thoughts").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::lock_conn;

    fn gemini_message(
        id: Option<&str>,
        input: u64,
        output: u64,
        cached: u64,
        thoughts: u64,
        timestamp: &str,
    ) -> serde_json::Value {
        let mut msg = serde_json::json!({
            "type": "gemini",
            "model": "gemini-2.5-pro",
            "timestamp": timestamp,
            "tokens": { "input": input, "output": output, "cached": cached, "thoughts": thoughts }
        });
        if let Some(id) = id {
            msg["id"] = serde_json::Value::String(id.to_string());
        }
        msg
    }

    fn write_session(dir: &Path, messages: Vec<serde_json::Value>) -> PathBuf {
        let chats = dir.join("tmp").join("proj").join("chats");
        fs::create_dir_all(&chats).unwrap();
        let file = chats.join("session-1.json");
        let body = serde_json::json!({ "sessionId": "sid", "messages": messages });
        fs::write(&file, body.to_string()).unwrap();
        file
    }

    fn force_resync(db: &Database, file: &Path) {
        // 规避 mtime 精度：把游标的 mtime 清零，下次同步必定重读
        crate::services::session_usage::update_sync_state(db, &file.to_string_lossy(), 0, 0)
            .unwrap();
    }

    #[test]
    fn test_changed_token_counts_update_existing_row() -> Result<(), AppError> {
        // SESS-35：Gemini 的 UPSERT 之前被 request_id 早退挡住，变化的 token 永远写不进去。
        let db = Database::memory()?;
        let tmp = tempfile::tempdir().unwrap();
        let file = write_session(
            tmp.path(),
            vec![gemini_message(
                Some("m1"),
                100,
                5,
                0,
                0,
                "2026-06-07T13:01:23Z",
            )],
        );
        assert_eq!(sync_gemini_dir(&db, tmp.path())?.imported, 1);

        write_session(
            tmp.path(),
            vec![gemini_message(
                Some("m1"),
                100,
                50,
                0,
                7,
                "2026-06-07T13:01:23Z",
            )],
        );
        force_resync(&db, &file);
        let second = sync_gemini_dir(&db, tmp.path())?;
        assert_eq!((second.imported, second.skipped), (1, 0));

        // 值不变时不重复写入
        force_resync(&db, &file);
        let third = sync_gemini_dir(&db, tmp.path())?;
        assert_eq!((third.imported, third.skipped), (0, 1));

        let conn = lock_conn!(db.conn);
        let (count, output): (i64, i64) = conn.query_row(
            "SELECT COUNT(*), MAX(output_tokens) FROM proxy_request_logs
             WHERE request_id = 'gemini_session:sid:m1'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        assert_eq!((count, output), (1, 57));
        Ok(())
    }

    #[test]
    fn test_messages_without_id_do_not_collide() -> Result<(), AppError> {
        // SESS-42：同一会话里两条无 id 的 gemini 消息应导入 2 行。
        let db = Database::memory()?;
        let tmp = tempfile::tempdir().unwrap();
        write_session(
            tmp.path(),
            vec![
                gemini_message(None, 100, 5, 0, 0, "2026-06-07T13:01:23Z"),
                gemini_message(None, 200, 9, 0, 0, "2026-06-07T13:02:23Z"),
            ],
        );
        assert_eq!(sync_gemini_dir(&db, tmp.path())?.imported, 2);
        Ok(())
    }

    #[test]
    fn test_collect_gemini_session_files_nonexistent() {
        let files = collect_gemini_session_files(Path::new("/nonexistent/path"));
        assert!(files.is_empty());
    }

    #[test]
    fn test_insert_gemini_session_skips_matching_proxy_log() -> Result<(), AppError> {
        let db = Database::memory()?;
        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model, request_model,
                    input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                    total_cost_usd, latency_ms, status_code, created_at, data_source
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                rusqlite::params![
                    "gemini-proxy",
                    "google",
                    "gemini",
                    "gemini-2.5-pro",
                    "gemini-2.5-pro",
                    10,
                    7,
                    1,
                    0,
                    "0.01",
                    100,
                    200,
                    1000,
                    "proxy"
                ],
            )?;
        }

        let session = serde_json::json!({
            "sessionId": "session-1",
            "messages": [gemini_message(Some("m1"), 10, 2, 1, 5, "1970-01-01T00:16:45Z")]
        });
        let rows = parse_gemini_session(&session, Path::new("session-x.json"));
        let written = commit_session_rows(
            &db,
            &GEMINI_SESSION_SOURCE,
            &rows,
            &mut PricingCache::default(),
            None,
        )?;
        assert_eq!(written, (0, 1));

        let conn = lock_conn!(db.conn);
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM proxy_request_logs", [], |row| {
            row.get(0)
        })?;
        assert_eq!(count, 1);

        Ok(())
    }

    #[test]
    fn test_parse_gemini_tokens() {
        let json: serde_json::Value = serde_json::json!({
            "input": 8522,
            "output": 29,
            "cached": 3138,
            "thoughts": 405,
            "tool": 0,
            "total": 8956
        });
        let tokens = parse_gemini_tokens(&json);
        assert_eq!(tokens.input, 8522);
        assert_eq!(tokens.output, 29);
        assert_eq!(tokens.cached, 3138);
        assert_eq!(tokens.thoughts, 405);
        // output + thoughts = 29 + 405 = 434（用于计费）
        assert_eq!(tokens.output + tokens.thoughts, 434);
    }

    #[test]
    fn test_parse_gemini_tokens_missing_fields() {
        // 缺少某些字段时应返回 0
        let json: serde_json::Value = serde_json::json!({
            "input": 100,
            "output": 50
        });
        let tokens = parse_gemini_tokens(&json);
        assert_eq!(tokens.input, 100);
        assert_eq!(tokens.output, 50);
        assert_eq!(tokens.cached, 0);
        assert_eq!(tokens.thoughts, 0);
    }

    #[test]
    fn test_parse_gemini_tokens_all_zero() {
        let json: serde_json::Value = serde_json::json!({
            "input": 0,
            "output": 0,
            "cached": 0,
            "thoughts": 0,
            "tool": 0,
            "total": 0
        });
        let tokens = parse_gemini_tokens(&json);
        assert_eq!(tokens.input, 0);
        assert_eq!(tokens.output, 0);
        // 全零（包括 cached=0）会被 sync 逻辑跳过
        assert!(
            tokens.input == 0 && tokens.output == 0 && tokens.thoughts == 0 && tokens.cached == 0
        );
    }

    #[test]
    fn test_parse_gemini_tokens_cache_only_not_skipped() {
        // 纯缓存命中消息（input/output/thoughts=0 但 cached>0）不应被跳过
        let json: serde_json::Value = serde_json::json!({
            "input": 0,
            "output": 0,
            "cached": 5000,
            "thoughts": 0
        });
        let tokens = parse_gemini_tokens(&json);
        assert_eq!(tokens.cached, 5000);
        // 跳过条件：所有四个字段都为 0 才跳过
        let should_skip =
            tokens.input == 0 && tokens.output == 0 && tokens.thoughts == 0 && tokens.cached == 0;
        assert!(!should_skip, "纯缓存命中记录不应被跳过");
    }
}
