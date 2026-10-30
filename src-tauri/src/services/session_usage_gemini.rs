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

use crate::database::repo::session_cursor::SessionCursor;
use crate::database::repo::usage::{self as usage_repo, SessionUsageRow, UsageCosts, UsageSource};
use crate::database::Database;
use crate::error::AppError;
use crate::gemini_config::get_gemini_dir;
use crate::pricing::resolver::find_model_pricing;
use crate::proxy::usage::calculator::{CostCalculator, ModelPricing};
use crate::proxy::usage::parser::TokenUsage;
use crate::services::session_usage::{
    get_sync_state, ingest_file, metadata_modified_nanos, IngestBatch, SessionSyncResult,
};
use crate::services::usage_stats::{has_matching_proxy_usage_log, DedupKey};
use rust_decimal::Decimal;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

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
    let gemini_dir = get_gemini_dir();

    let files = collect_gemini_session_files(&gemini_dir);

    let mut result = SessionSyncResult {
        imported: 0,
        skipped: 0,
        files_scanned: files.len() as u32,
        errors: vec![],
    };

    if files.is_empty() {
        return Ok(result);
    }

    for file_path in &files {
        match sync_single_gemini_file(db, file_path) {
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
fn sync_single_gemini_file(db: &Database, file_path: &Path) -> Result<(u32, u32), AppError> {
    let file_path_str = file_path.to_string_lossy().to_string();

    // 获取文件元数据
    let metadata = fs::metadata(file_path)
        .map_err(|e| AppError::Config(format!("无法读取文件元数据: {e}")))?;
    let file_modified = metadata_modified_nanos(&metadata);

    // 检查同步状态
    let (last_modified, _last_offset) = get_sync_state(db, &file_path_str)?;

    // 文件未变化则跳过
    if file_modified <= last_modified {
        return Ok((0, 0));
    }

    // 读取并解析整个 JSON 文件
    let content = fs::read_to_string(file_path)
        .map_err(|e| AppError::Config(format!("无法读取文件: {e}")))?;
    let value: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| AppError::Config(format!("JSON 解析失败: {e}")))?;

    // 提取顶层 sessionId
    let session_id = value
        .get("sessionId")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    // 遍历 messages 数组
    let messages = match value.get("messages").and_then(|v| v.as_array()) {
        Some(msgs) => msgs,
        None => return Ok((0, 0)),
    };

    let mut gemini_msg_count: i64 = 0;
    let mut pending: Vec<PendingGeminiEntry<'_>> = Vec::new();

    for msg in messages {
        // 只处理 type == "gemini" 的消息
        if msg.get("type").and_then(|t| t.as_str()) != Some("gemini") {
            continue;
        }

        // 提取 tokens 对象
        let tokens_obj = match msg.get("tokens") {
            Some(t) if t.is_object() => t,
            _ => continue,
        };

        let tokens = parse_gemini_tokens(tokens_obj);
        if tokens.input == 0 && tokens.output == 0 && tokens.thoughts == 0 && tokens.cached == 0 {
            continue; // 跳过全零的空 token 消息
        }

        gemini_msg_count += 1;

        // 提取消息 ID 和模型
        let message_id = msg.get("id").and_then(|v| v.as_str()).unwrap_or("unknown");
        let model = msg
            .get("model")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let timestamp = msg.get("timestamp").and_then(|v| v.as_str());

        // 生成唯一 request_id
        let session_id_str = session_id.as_deref().unwrap_or("unknown");
        let request_id = format!("gemini_session:{session_id_str}:{message_id}");

        pending.push(PendingGeminiEntry {
            request_id,
            tokens,
            model,
            timestamp,
        });
    }

    // 本文件的所有行与游标在同一个事务里提交
    let cursor = SessionCursor {
        last_modified: file_modified,
        last_offset: gemini_msg_count,
    };
    ingest_file(db, &file_path_str, |conn| {
        let mut imported: u32 = 0;
        let mut skipped: u32 = 0;
        for entry in &pending {
            match insert_gemini_session_entry(
                conn,
                &entry.request_id,
                &entry.tokens,
                entry.model,
                session_id.as_deref(),
                entry.timestamp,
            ) {
                Ok(true) => imported += 1,
                Ok(false) => skipped += 1,
                Err(e) => {
                    log::warn!("[GEMINI-SYNC] 插入失败 ({}): {e}", entry.request_id);
                    skipped += 1;
                }
            }
        }
        Ok(IngestBatch {
            value: (imported, skipped),
            wrote_rows: imported > 0,
            cursor: Some(cursor),
        })
    })
}

/// 解析阶段收集、在文件事务里写入的一条 Gemini 用量
struct PendingGeminiEntry<'a> {
    request_id: String,
    tokens: GeminiTokens,
    model: &'a str,
    timestamp: Option<&'a str>,
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

/// 插入单条 Gemini 会话记录到 proxy_request_logs
fn insert_gemini_session_entry(
    conn: &rusqlite::Connection,
    request_id: &str,
    tokens: &GeminiTokens,
    model: &str,
    session_id: Option<&str>,
    timestamp: Option<&str>,
) -> Result<bool, AppError> {
    let created_at = timestamp
        .and_then(|ts| {
            chrono::DateTime::parse_from_rfc3339(ts)
                .ok()
                .map(|dt| dt.timestamp())
        })
        .unwrap_or_else(|| {
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0)
        });

    // 合并 thoughts 到 output（思考 token 按输出计费）
    let output_tokens = tokens.output + tokens.thoughts;

    let dedup_key = DedupKey {
        app_type: "gemini",
        model,
        input_tokens: tokens.input,
        output_tokens,
        cache_read_tokens: tokens.cached,
        cache_creation_tokens: 0,
        created_at,
    };
    // 只按代理日志去重，不能用 should_skip_session_insert：它在 request_id 已存在时
    // 直接跳过，导致下方 UPSERT 的 DO UPDATE 分支永远走不到，重读文件拿到的新值被丢弃。
    // 自身的会话行由 ON CONFLICT(request_id) 处理（值相同则不变，值变化则更新）。
    if has_matching_proxy_usage_log(conn, &dedup_key)? {
        return Ok(false);
    }

    // 计算费用
    let usage = TokenUsage {
        input_tokens: tokens.input,
        output_tokens,
        cache_read_tokens: tokens.cached,
        cache_creation_tokens: 0,
        model: Some(model.to_string()),
        message_id: None,
    };

    let pricing = find_gemini_pricing(conn, model);
    let multiplier = Decimal::from(1);
    let costs = match pricing {
        Some(p) => UsageCosts::from_breakdown(&CostCalculator::calculate_for_app(
            "gemini", &usage, &p, multiplier,
        )),
        None => UsageCosts::zero(),
    };

    // 使用 UPSERT：新记录插入，已存在记录更新 token 和费用（Gemini 全量重读可能携带更新值）。
    // 返回 true 表示新插入或已更新，false 表示值完全相同（无实际变更）
    usage_repo::upsert_session_row(
        conn,
        &SessionUsageRow {
            request_id,
            source: UsageSource::GeminiSession,
            model,
            pricing_model: model,
            session_id,
            input_tokens: tokens.input,
            output_tokens,
            cache_read_tokens: tokens.cached,
            cache_creation_tokens: 0,
            costs,
            created_at,
        },
    )
}

/// 查找 Gemini 模型定价
fn find_gemini_pricing(conn: &rusqlite::Connection, model_id: &str) -> Option<ModelPricing> {
    find_model_pricing(conn, model_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::lock_conn;

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

        let tokens = GeminiTokens {
            input: 10,
            output: 2,
            cached: 1,
            thoughts: 5,
        };
        let inserted = insert_gemini_session_entry(
            &db.conn.lock().unwrap(),
            "gemini-session-dup",
            &tokens,
            "gemini-2.5-pro",
            Some("session-1"),
            Some("1970-01-01T00:16:45Z"),
        )?;
        assert!(!inserted);

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

    #[test]
    fn test_insert_gemini_session_persists_semantics_and_pricing_model() -> Result<(), AppError> {
        let db = Database::memory()?;
        let tokens = GeminiTokens {
            input: 1000,
            output: 20,
            cached: 300,
            thoughts: 5,
        };
        assert!(insert_gemini_session_entry(
            &db.conn.lock().unwrap(),
            "gemini_session:s1:m1",
            &tokens,
            "gemini-2.5-pro",
            Some("s1"),
            Some("1970-01-01T00:16:45Z"),
        )?);

        let conn = lock_conn!(db.conn);
        let (pricing_model, semantics): (Option<String>, i64) = conn.query_row(
            "SELECT pricing_model, input_token_semantics FROM proxy_request_logs
             WHERE request_id = 'gemini_session:s1:m1'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        assert_eq!(pricing_model.as_deref(), Some("gemini-2.5-pro"));
        assert_eq!(
            semantics,
            crate::services::sql_helpers::INPUT_TOKEN_SEMANTICS_TOTAL
        );
        Ok(())
    }

    #[test]
    fn test_gemini_reimport_updates_existing_row_instead_of_duplicating() -> Result<(), AppError> {
        let db = Database::memory()?;
        let request_id = "gemini_session:s1:m1";
        let first = GeminiTokens {
            input: 1000,
            output: 20,
            cached: 300,
            thoughts: 0,
        };
        assert!(insert_gemini_session_entry(
            &db.conn.lock().unwrap(),
            request_id,
            &first,
            "gemini-2.5-pro",
            Some("s1"),
            Some("1970-01-01T00:16:45Z"),
        )?);

        // 相同值重读：不产生变更
        assert!(!insert_gemini_session_entry(
            &db.conn.lock().unwrap(),
            request_id,
            &first,
            "gemini-2.5-pro",
            Some("s1"),
            Some("1970-01-01T00:16:45Z"),
        )?);

        // 文件重写后同一消息携带更新后的 token：必须原地更新
        let updated = GeminiTokens {
            input: 1500,
            output: 80,
            cached: 400,
            thoughts: 10,
        };
        assert!(insert_gemini_session_entry(
            &db.conn.lock().unwrap(),
            request_id,
            &updated,
            "gemini-2.5-pro",
            Some("s1"),
            Some("1970-01-01T00:16:45Z"),
        )?);

        let conn = lock_conn!(db.conn);
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM proxy_request_logs WHERE request_id = ?1",
            [request_id],
            |row| row.get(0),
        )?;
        assert_eq!(count, 1);
        let (input, output, cached): (i64, i64, i64) = conn.query_row(
            "SELECT input_tokens, output_tokens, cache_read_tokens
             FROM proxy_request_logs WHERE request_id = ?1",
            [request_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        assert_eq!((input, output, cached), (1500, 90, 400));
        Ok(())
    }

    #[test]
    fn test_gemini_reimport_upgrades_legacy_row_semantics() -> Result<(), AppError> {
        let db = Database::memory()?;
        let request_id = "gemini_session:s1:legacy";
        {
            // 修复前导入的行：pricing_model 为 NULL、语义为 LEGACY
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model, request_model,
                    input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                    total_cost_usd, latency_ms, status_code, created_at, data_source
                ) VALUES (?1, '_gemini_session', 'gemini', 'gemini-2.5-pro', 'gemini-2.5-pro',
                          1000, 20, 300, 0, '0', 0, 200, 1005, 'gemini_session')",
                [request_id],
            )?;
        }
        let tokens = GeminiTokens {
            input: 1000,
            output: 20,
            cached: 300,
            thoughts: 0,
        };
        assert!(insert_gemini_session_entry(
            &db.conn.lock().unwrap(),
            request_id,
            &tokens,
            "gemini-2.5-pro",
            Some("s1"),
            Some("1970-01-01T00:16:45Z"),
        )?);

        let conn = lock_conn!(db.conn);
        let (pricing_model, semantics): (Option<String>, i64) = conn.query_row(
            "SELECT pricing_model, input_token_semantics FROM proxy_request_logs
             WHERE request_id = ?1",
            [request_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        assert_eq!(pricing_model.as_deref(), Some("gemini-2.5-pro"));
        assert_eq!(
            semantics,
            crate::services::sql_helpers::INPUT_TOKEN_SEMANTICS_TOTAL
        );
        Ok(())
    }
}
