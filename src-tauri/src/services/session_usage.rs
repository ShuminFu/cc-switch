//! Claude Code 会话日志使用追踪
//!
//! 从 ~/.claude/projects/ 下的 JSONL 会话文件中提取 token 使用数据，
//! 实现无代理模式下的使用统计。
//!
//! ## 数据流
//! ```text
//! ~/.claude/projects/*/*.jsonl → 增量解析 → 去重 → 费用计算 → proxy_request_logs 表
//! ```
//!
//! 四个会话来源（Claude/Codex/Gemini/OpenCode）共用 [`ingest`] 中的写入逻辑，
//! 并通过 [`sync_all_session_usage`] 以单飞方式统一调度。

use crate::config::get_claude_config_dir;
use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::services::usage_stats::effective_usage_log_filter;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, TryLockError};
use std::time::{Instant, SystemTime};

pub(crate) mod ingest;

use ingest::{
    commit_session_rows, created_at_from_rfc3339, load_byte_cursors, load_sync_cursors,
    prune_missing_cursors, update_sync_state_on, ByteCursor, CursorUpdate, PricingCache,
    SessionSource, SessionUsageRow, WritePolicy,
};

/// 同步结果
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSyncResult {
    pub imported: u32,
    pub skipped: u32,
    pub files_scanned: u32,
    pub errors: Vec<String>,
}

impl SessionSyncResult {
    fn merge(&mut self, other: SessionSyncResult) {
        self.imported += other.imported;
        self.skipped += other.skipped;
        self.files_scanned += other.files_scanned;
        self.errors.extend(other.errors);
    }
}

/// 数据来源分布
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DataSourceSummary {
    pub data_source: String,
    pub request_count: u32,
    pub total_cost_usd: String,
}

/// 从 JSONL 中解析出的 assistant 消息使用数据
#[derive(Debug)]
struct ParsedAssistantUsage {
    message_id: String,
    model: String,
    input_tokens: u32,
    output_tokens: u32,
    cache_read_tokens: u32,
    cache_creation_tokens: u32,
    stop_reason: Option<String>,
    timestamp: Option<String>,
    session_id: Option<String>,
}

/// Claude 会话日志写入策略：同一 message.id 的流式快照以 output 最大者为准，
/// 之后的同步读到更完整的快照时会更新已落库的行（SESS-35）。
const CLAUDE_SESSION_SOURCE: SessionSource = SessionSource {
    app_type: "claude",
    provider_id: "_session",
    data_source: "session_log",
    policy: WritePolicy::KeepMaxOutput,
};

/// Who asked for a combined session-usage sync.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncTrigger {
    /// Startup/periodic sync: skipped when another sync is already running.
    Background,
    /// User-initiated sync: waits for a running sync to finish, then runs.
    Manual,
}

static SYNC_RUNNING: Mutex<()> = Mutex::new(());

type SyncFn = fn(&Database) -> Result<SessionSyncResult, AppError>;

/// Run all four session-usage importers once (single-flight, SESS-08).
///
/// Returns `None` when `trigger` is [`SyncTrigger::Background`] and another sync is
/// already in progress. Usage events raised while importing are coalesced into one
/// notification emitted when the run ends. This is blocking work: call it from
/// `spawn_blocking` or a plain thread, never directly on the async runtime.
pub fn sync_all_session_usage(db: &Database, trigger: SyncTrigger) -> Option<SessionSyncResult> {
    let _running = match trigger {
        SyncTrigger::Manual => SYNC_RUNNING.lock().unwrap_or_else(|e| e.into_inner()),
        SyncTrigger::Background => match SYNC_RUNNING.try_lock() {
            Ok(guard) => guard,
            Err(TryLockError::Poisoned(e)) => e.into_inner(),
            Err(TryLockError::WouldBlock) => {
                log::debug!("[SESSION-SYNC] 已有同步在运行，跳过本轮");
                return None;
            }
        },
    };
    let _batch = crate::usage_events::begin_batch();
    let started = Instant::now();

    let sources: [(&str, SyncFn); 4] = [
        ("Claude", sync_claude_session_logs),
        (
            "Codex",
            crate::services::session_usage_codex::sync_codex_usage,
        ),
        (
            "Gemini",
            crate::services::session_usage_gemini::sync_gemini_usage,
        ),
        (
            "OpenCode",
            crate::services::session_usage_opencode::sync_opencode_usage,
        ),
    ];
    let mut total = SessionSyncResult::default();
    for (name, sync) in sources {
        match sync(db) {
            Ok(result) => total.merge(result),
            Err(e) => {
                log::warn!("[SESSION-SYNC] {name} 同步失败: {e}");
                total.errors.push(format!("{name} 同步失败: {e}"));
            }
        }
    }

    log::debug!(
        "[SESSION-SYNC] pass finished in {:?}: imported {}, skipped {}, files {}",
        started.elapsed(),
        total.imported,
        total.skipped,
        total.files_scanned
    );
    Some(total)
}

/// 同步 Claude Code 会话日志到使用统计数据库
pub fn sync_claude_session_logs(db: &Database) -> Result<SessionSyncResult, AppError> {
    let projects_dir = get_claude_config_dir().join("projects");
    sync_claude_projects_dir(db, &projects_dir)
}

fn sync_claude_projects_dir(
    db: &Database,
    projects_dir: &Path,
) -> Result<SessionSyncResult, AppError> {
    let mut result = SessionSyncResult::default();
    if !projects_dir.exists() {
        return Ok(result);
    }

    // 一次查询载入全部同步游标；未变化的文件只需一次 stat（PERF-10）
    let cursors = load_sync_cursors(db)?;
    let byte_cursors = load_byte_cursors(db)?;
    let mut pricing = PricingCache::default();

    // 收集所有 .jsonl 文件
    let jsonl_files = collect_jsonl_files(projects_dir);
    let mut discovered = HashSet::with_capacity(jsonl_files.len());

    for file_path in &jsonl_files {
        result.files_scanned += 1;
        let key = file_path.to_string_lossy().to_string();
        let cursor = cursors.get(&key).copied().unwrap_or((0, 0));
        discovered.insert(key.clone());

        match sync_claude_file(db, file_path, cursor, byte_cursors.get(&key), &mut pricing) {
            Ok((imported, skipped)) => {
                result.imported += imported;
                result.skipped += skipped;
            }
            Err(e) => {
                let msg = format!("{}: {e}", file_path.display());
                log::warn!("[SESSION-SYNC] 文件解析失败: {msg}");
                result.errors.push(msg);
            }
        }
    }

    if let Err(e) = prune_missing_cursors(db, projects_dir, &cursors, &discovered) {
        log::warn!("[SESSION-SYNC] 清理失效同步状态失败: {e}");
    }

    if result.imported > 0 {
        log::info!(
            "[SESSION-SYNC] 同步完成: 导入 {} 条, 跳过 {} 条, 扫描 {} 个文件",
            result.imported,
            result.skipped,
            result.files_scanned
        );
    }

    Ok(result)
}

/// 收集目录下所有 .jsonl 文件（含子 agent 文件）
///
/// 扫描固定深度，不使用递归，避免死循环：
///   projects_dir/项目目录/*.jsonl                                      (主会话)
///   projects_dir/项目目录/SESSION_ID/subagents/*.jsonl                  (Task/Agent 子 agent)
///   projects_dir/项目目录/SESSION_ID/subagents/workflows/wf_*/*.jsonl   (Workflow 子 agent)
///
/// 最后一层是 Claude Code Workflow 功能产生的子 agent transcript，比普通子
/// agent 多嵌套一层 `workflows/wf_<ID>/`。漏掉这一层会让 Workflow 的 token
/// 用量完全不计入统计；`journal.jsonl` 不含 `type=="assistant"` 行，解析时
/// 会被 `sync_claude_file` 天然跳过，因此这里无需按文件名过滤。
fn collect_jsonl_files(projects_dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();

    let entries = match fs::read_dir(projects_dir) {
        Ok(e) => e,
        Err(_) => return files,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        // 每个项目目录下的 .jsonl 文件
        if let Ok(sub_entries) = fs::read_dir(&path) {
            for sub_entry in sub_entries.flatten() {
                let sub_path = sub_entry.path();
                if sub_path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
                    // 主会话 JSONL 文件
                    files.push(sub_path);
                } else if sub_path.is_dir() {
                    // 扫描子 agent 目录: 项目/SESSION_ID/subagents/*.jsonl
                    let subagents_dir = sub_path.join("subagents");
                    if subagents_dir.is_dir() {
                        push_jsonl_children(&subagents_dir, &mut files);

                        // 额外下探 Workflow 子 agent:
                        // 项目/SESSION_ID/subagents/workflows/wf_<ID>/*.jsonl
                        let workflows_dir = subagents_dir.join("workflows");
                        if workflows_dir.is_dir() {
                            if let Ok(wf_entries) = fs::read_dir(&workflows_dir) {
                                for wf_entry in wf_entries.flatten() {
                                    let wf_path = wf_entry.path();
                                    if wf_path.is_dir() {
                                        push_jsonl_children(&wf_path, &mut files);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    files
}

/// 将 `dir` 下直接子层的所有 `.jsonl` 文件追加到 `files`（不递归）。
fn push_jsonl_children(dir: &Path, files: &mut Vec<PathBuf>) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
                files.push(path);
            }
        }
    }
}

/// 逐行读取 JSONL 文件中**以换行结尾**的完整行，回调参数为 (从 1 开始的行号, 行内容)。
///
/// 返回完整行的数量，作为新的行游标。末尾尚未写完（无换行）的行不会被读取，
/// 也不计入游标，下次同步会重新读取它（SESS-36）。非 UTF-8 行占用行号但被跳过。
pub(crate) fn for_each_complete_line(
    file_path: &Path,
    mut on_line: impl FnMut(i64, &str),
) -> Result<i64, AppError> {
    let file =
        fs::File::open(file_path).map_err(|e| AppError::Config(format!("无法打开文件: {e}")))?;
    let mut reader = BufReader::new(file);
    let mut buf = Vec::new();
    let mut line_no: i64 = 0;
    loop {
        buf.clear();
        let read = reader
            .read_until(b'\n', &mut buf)
            .map_err(|e| AppError::Config(format!("读取文件失败: {e}")))?;
        if read == 0 || buf.last() != Some(&b'\n') {
            break;
        }
        line_no += 1;
        if let Ok(line) = std::str::from_utf8(&buf) {
            on_line(line_no, line.trim_end_matches(['\n', '\r']));
        }
    }
    Ok(line_no)
}

/// Bytes of the file start hashed into a byte cursor's `prefix_hash`.
const PREFIX_HASH_BYTES: u64 = 4096;

/// Hash of the first `min(len, 4 KiB)` bytes: detects a file that was rewritten
/// (rather than appended to) since the cursor was stored.
pub(crate) fn file_prefix_hash(file_path: &Path, len: u64) -> Option<String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let take = len.min(PREFIX_HASH_BYTES);
    let mut buf = Vec::with_capacity(take as usize);
    fs::File::open(file_path)
        .ok()?
        .take(take)
        .read_to_end(&mut buf)
        .ok()?;
    if (buf.len() as u64) < take {
        return None;
    }
    let digest = Sha256::digest(&buf);
    Some(format!("{take}:{}", hex_prefix(&digest)))
}

fn hex_prefix(bytes: &[u8]) -> String {
    bytes[..16].iter().map(|b| format!("{b:02x}")).collect()
}

/// Whether `cursor` still describes a prefix of the current file: the file is
/// at least as long, the byte before the offset is a newline and the prefix
/// hash matches.
fn byte_cursor_still_valid(file_path: &Path, file_len: u64, cursor: &ByteCursor) -> bool {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(offset) = u64::try_from(cursor.offset) else {
        return false;
    };
    if offset == 0 || offset > file_len {
        return false;
    }
    let Ok(mut file) = fs::File::open(file_path) else {
        return false;
    };
    let mut last = [0u8; 1];
    if file.seek(SeekFrom::Start(offset - 1)).is_err() || file.read_exact(&mut last).is_err() {
        return false;
    }
    last[0] == b'\n' && file_prefix_hash(file_path, offset).as_deref() == Some(&cursor.prefix_hash)
}

/// Like [`for_each_complete_line`] but starts at byte `start` (a line boundary)
/// with line numbers continuing after `start_line`. Returns the total complete
/// line count and the byte position after the last complete line.
pub(crate) fn for_each_complete_line_from(
    file_path: &Path,
    start: u64,
    start_line: i64,
    mut on_line: impl FnMut(i64, &str),
) -> Result<(i64, u64), AppError> {
    use std::io::{Seek, SeekFrom};
    let mut file =
        fs::File::open(file_path).map_err(|e| AppError::Config(format!("无法打开文件: {e}")))?;
    file.seek(SeekFrom::Start(start))
        .map_err(|e| AppError::Config(format!("定位文件失败: {e}")))?;
    let mut reader = BufReader::new(file);
    let mut buf = Vec::new();
    let mut line_no = start_line;
    let mut pos = start;
    loop {
        buf.clear();
        let read = reader
            .read_until(b'\n', &mut buf)
            .map_err(|e| AppError::Config(format!("读取文件失败: {e}")))?;
        if read == 0 || buf.last() != Some(&b'\n') {
            break;
        }
        line_no += 1;
        pos += read as u64;
        if let Ok(line) = std::str::from_utf8(&buf) {
            on_line(line_no, line.trim_end_matches(['\n', '\r']));
        }
    }
    Ok((line_no, pos))
}

/// 同步单个 JSONL 文件，返回 (imported, skipped)
///
/// With a valid byte cursor only the appended bytes are read (SESS-28); a file
/// that was rewritten or truncated is re-read from the start.
fn sync_claude_file(
    db: &Database,
    file_path: &Path,
    (last_modified, last_offset): (i64, i64),
    byte_cursor: Option<&ByteCursor>,
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

    let mut messages: HashMap<String, ParsedAssistantUsage> = HashMap::new();
    let mut current_session_id: Option<String> = None;

    let (start_byte, start_line, last_offset) = match byte_cursor {
        Some(cursor) if byte_cursor_still_valid(file_path, metadata.len(), cursor) => {
            (cursor.offset as u64, last_offset, last_offset)
        }
        // The stored prefix no longer matches: the file was rewritten, so
        // re-read everything (rows are keyed by message id, no duplicates).
        Some(_) => (0, 0, 0),
        // Legacy cursor without a byte offset: skip by line count.
        None => (0, 0, last_offset),
    };

    // 从上次偏移位置开始增量解析
    let (line_offset, end_byte) =
        for_each_complete_line_from(file_path, start_byte, start_line, |line_no, line| {
            // 跳过已处理的行
            if line_no <= last_offset || line.trim().is_empty() {
                return;
            }
            let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
                return;
            };

            // 提取 session ID (从 system 或首条消息)
            if current_session_id.is_none() {
                if let Some(sid) = value.get("sessionId").and_then(|v| v.as_str()) {
                    current_session_id = Some(sid.to_string());
                }
            }

            if let Some(parsed) = parse_assistant_usage(&value, current_session_id.as_ref()) {
                merge_snapshot(&mut messages, parsed);
            }
        })?;

    // 只要产生了真实计费 token 就导入，不再强制要求 stop_reason 或 output>0。
    //
    // Anthropic 在受理请求时即对 input + cache_read + cache_creation 计费
    // （这些在请求开始就确定），output 按实际生成量计。Workflow / 子 agent 的
    // 并行短命请求经常只写了 message_start 快照（output=1、stop_reason=None）
    // 却没有写最终块，但其 cache/input 成本已被真实计费。旧逻辑用 stop_reason
    // 非空 + output>0 双重过滤，会把这类请求整条丢弃，实测系统性低估约 4.1%，
    // 且 92% 集中在 workflow/subagent。这里改为「任一计费维度 > 0 即导入」。
    //
    // request_id = session:msg_id 主键保证一个 message 只落库一次；若之后的同步
    // 读到 output 更大的快照（流式回复跨越了同步时刻），KeepMaxOutput 会更新该行。
    let rows: Vec<SessionUsageRow> = messages
        .values()
        .filter(|msg| {
            msg.input_tokens > 0
                || msg.output_tokens > 0
                || msg.cache_read_tokens > 0
                || msg.cache_creation_tokens > 0
        })
        .map(claude_row)
        .collect();

    commit_session_rows(
        db,
        &CLAUDE_SESSION_SOURCE,
        &rows,
        pricing,
        Some(CursorUpdate {
            key: &file_path_str,
            last_modified: file_modified,
            last_offset: line_offset,
            byte: file_prefix_hash(file_path, end_byte)
                .filter(|_| end_byte > 0)
                .map(|prefix_hash| ByteCursor {
                    offset: end_byte as i64,
                    prefix_hash,
                }),
        }),
    )
}

/// 从一行 JSONL 中提取 assistant 消息的 usage 快照
fn parse_assistant_usage(
    value: &serde_json::Value,
    session_id: Option<&String>,
) -> Option<ParsedAssistantUsage> {
    // 只处理 assistant 类型的消息
    if value.get("type").and_then(|t| t.as_str()) != Some("assistant") {
        return None;
    }
    let message = value.get("message")?;
    let msg_id = message.get("id").and_then(|v| v.as_str())?;
    let usage = message.get("usage")?;
    let token = |key: &str| usage.get(key).and_then(|v| v.as_u64()).unwrap_or(0) as u32;

    Some(ParsedAssistantUsage {
        message_id: msg_id.to_string(),
        model: message
            .get("model")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string(),
        input_tokens: token("input_tokens"),
        output_tokens: token("output_tokens"),
        cache_read_tokens: token("cache_read_input_tokens"),
        cache_creation_tokens: token("cache_creation_input_tokens"),
        stop_reason: message
            .get("stop_reason")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        timestamp: value
            .get("timestamp")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        session_id: session_id.cloned(),
    })
}

/// 按 message.id 去重：优先保留有 stop_reason 的条目，否则保留 output 最大的
fn merge_snapshot(
    messages: &mut HashMap<String, ParsedAssistantUsage>,
    parsed: ParsedAssistantUsage,
) {
    let should_replace = match messages.get(&parsed.message_id) {
        None => true,
        Some(existing) => {
            if parsed.stop_reason.is_some() && existing.stop_reason.is_none() {
                true
            } else if parsed.stop_reason.is_some() == existing.stop_reason.is_some() {
                parsed.output_tokens > existing.output_tokens
            } else {
                false
            }
        }
    };
    if should_replace {
        messages.insert(parsed.message_id.clone(), parsed);
    }
}

fn claude_row(msg: &ParsedAssistantUsage) -> SessionUsageRow {
    SessionUsageRow {
        request_id: format!(
            "{}{}",
            crate::proxy::usage::parser::SESSION_REQUEST_ID_PREFIX,
            msg.message_id
        ),
        model: msg.model.clone(),
        pricing_model: msg.model.clone(),
        input_tokens: msg.input_tokens,
        output_tokens: msg.output_tokens,
        cache_read_tokens: msg.cache_read_tokens,
        cache_creation_tokens: msg.cache_creation_tokens,
        session_id: msg.session_id.clone(),
        created_at: created_at_from_rfc3339(msg.timestamp.as_deref()),
        precomputed_total_cost: None,
    }
}

/// 获取 session_log_sync 表中某条目的同步进度（测试用；同步路径一次性预加载游标）。
#[cfg(test)]
pub(crate) fn get_sync_state(db: &Database, file_path: &str) -> Result<(i64, i64), AppError> {
    let conn = lock_conn!(db.conn);
    let result = conn.query_row(
        "SELECT last_modified, last_line_offset FROM session_log_sync WHERE file_path = ?1",
        rusqlite::params![file_path],
        |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
    );
    Ok(result.unwrap_or((0, 0)))
}

/// 返回文件 mtime 的纳秒时间戳。
///
/// `session_log_sync.last_modified` 旧数据是秒级时间戳；新写入纳秒值不需要
/// schema 迁移，旧值会自然触发一次增量重扫，并继续依赖行 offset 避免重复导入。
pub(crate) fn metadata_modified_nanos(metadata: &fs::Metadata) -> i64 {
    metadata
        .modified()
        .ok()
        .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos().min(i64::MAX as u128) as i64)
        .unwrap_or(0)
}

/// 更新 session_log_sync 表中某条目的同步进度。
///
/// Shared by all session_usage_* parsers.
pub(crate) fn update_sync_state(
    db: &Database,
    file_path: &str,
    last_modified: i64,
    last_offset: i64,
) -> Result<(), AppError> {
    let conn = lock_conn!(db.conn);
    update_sync_state_on(&conn, file_path, last_modified, last_offset)
}

/// 查询数据来源分布统计
pub fn get_data_source_breakdown(db: &Database) -> Result<Vec<DataSourceSummary>, AppError> {
    let conn = lock_conn!(db.conn);

    let effective_filter = effective_usage_log_filter("l");
    let sql = format!(
        "SELECT COALESCE(l.data_source, 'proxy') as ds, COUNT(*) as cnt,
                COALESCE(SUM(CAST(l.total_cost_usd AS REAL)), 0) as cost
         FROM proxy_request_logs l
         WHERE {effective_filter}
         GROUP BY ds
         ORDER BY cnt DESC"
    );

    let mut stmt = conn.prepare(&sql)?;

    let rows = stmt.query_map([], |row| {
        Ok(DataSourceSummary {
            data_source: row.get(0)?,
            request_count: row.get::<_, i64>(1)? as u32,
            total_cost_usd: format!("{:.6}", row.get::<_, f64>(2)?),
        })
    })?;

    let mut summaries = Vec::new();
    for row in rows {
        summaries.push(row.map_err(|e| AppError::Database(e.to_string()))?);
    }

    Ok(summaries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// 以当前持久化游标同步单个文件（测试辅助）。
    fn sync_single_file(db: &Database, file_path: &Path) -> Result<(u32, u32), AppError> {
        let key = file_path.to_string_lossy().to_string();
        let cursor = get_sync_state(db, &key)?;
        let byte = load_byte_cursors(db)?.remove(&key);
        sync_claude_file(
            db,
            file_path,
            cursor,
            byte.as_ref(),
            &mut PricingCache::default(),
        )
    }

    fn assistant_line(msg_id: &str, output: u32, stop_reason: Option<&str>) -> String {
        serde_json::json!({
            "type": "assistant",
            "sessionId": "session-stream",
            "timestamp": "2026-06-07T13:01:23Z",
            "message": {
                "id": msg_id,
                "model": "claude-sonnet-4-5",
                "stop_reason": stop_reason,
                "usage": {
                    "input_tokens": 3,
                    "output_tokens": output,
                    "cache_read_input_tokens": 5000,
                    "cache_creation_input_tokens": 100
                }
            }
        })
        .to_string()
    }

    fn row_tokens_and_cost(db: &Database, request_id: &str) -> (i64, i64, String) {
        let conn = db.conn.lock().unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM proxy_request_logs WHERE request_id = ?1",
                [request_id],
                |r| r.get(0),
            )
            .unwrap();
        let (output, cost): (i64, String) = conn
            .query_row(
                "SELECT output_tokens, total_cost_usd FROM proxy_request_logs WHERE request_id = ?1",
                [request_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        (count, output, cost)
    }

    /// 修改文件后确保 mtime 前进（部分文件系统 mtime 精度较粗）。
    fn bump_mtime(path: &Path, previous: i64) {
        for _ in 0..200 {
            let now = metadata_modified_nanos(&fs::metadata(path).unwrap());
            if now > previous {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
            let f = fs::OpenOptions::new().append(true).open(path).unwrap();
            f.set_modified(SystemTime::now()).unwrap();
        }
        panic!("mtime did not advance");
    }

    #[test]
    fn test_parse_usage_from_jsonl_line() {
        let line = r#"{"type":"assistant","message":{"id":"msg_test123","model":"claude-opus-4-6","usage":{"input_tokens":3,"output_tokens":150,"cache_read_input_tokens":5000,"cache_creation_input_tokens":10000},"stop_reason":"end_turn"},"timestamp":"2026-04-05T12:00:00Z","sessionId":"session-abc"}"#;

        let value: serde_json::Value = serde_json::from_str(line).unwrap();
        let parsed = parse_assistant_usage(&value, None).unwrap();
        assert_eq!(parsed.message_id, "msg_test123");
        assert_eq!(parsed.input_tokens, 3);
        assert_eq!(parsed.output_tokens, 150);
        assert_eq!(parsed.cache_read_tokens, 5000);
        assert_eq!(parsed.cache_creation_tokens, 10000);
        assert_eq!(parsed.stop_reason.as_deref(), Some("end_turn"));
    }

    #[test]
    fn test_dedup_by_message_id() {
        // 同一个 message.id 有多条，应该取 stop_reason 有值的那条
        let mut messages: HashMap<String, ParsedAssistantUsage> = HashMap::new();
        let snapshot = |output, stop_reason: Option<&str>| ParsedAssistantUsage {
            message_id: "msg_1".to_string(),
            model: "claude-opus-4-6".to_string(),
            input_tokens: 3,
            output_tokens: output,
            cache_read_tokens: 5000,
            cache_creation_tokens: 10000,
            stop_reason: stop_reason.map(str::to_string),
            timestamp: Some("2026-04-05T12:00:00Z".to_string()),
            session_id: None,
        };

        merge_snapshot(&mut messages, snapshot(26, None));
        merge_snapshot(&mut messages, snapshot(1349, Some("end_turn")));
        // 有 stop_reason 的最终条目不会被之后的中间快照覆盖
        merge_snapshot(&mut messages, snapshot(2000, None));
        assert_eq!(messages.get("msg_1").unwrap().output_tokens, 1349);
    }

    #[test]
    fn test_insert_claude_session_skips_matching_proxy_log() -> Result<(), AppError> {
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
                    "proxy-different-id",
                    "openai-compatible",
                    "claude",
                    "claude-sonnet-4-5",
                    "claude-sonnet-4-5",
                    100,
                    20,
                    10,
                    5,
                    "0.10",
                    100,
                    200,
                    1000,
                    "proxy"
                ],
            )?;
        }

        let msg = ParsedAssistantUsage {
            message_id: "msg_1".to_string(),
            model: "claude-sonnet-4-5".to_string(),
            input_tokens: 100,
            output_tokens: 20,
            cache_read_tokens: 10,
            cache_creation_tokens: 5,
            stop_reason: Some("end_turn".to_string()),
            timestamp: Some("1970-01-01T00:16:45Z".to_string()),
            session_id: Some("session-1".to_string()),
        };

        let (imported, skipped) = commit_session_rows(
            &db,
            &CLAUDE_SESSION_SOURCE,
            &[claude_row(&msg)],
            &mut PricingCache::default(),
            None,
        )?;
        assert_eq!((imported, skipped), (0, 1));

        let conn = lock_conn!(db.conn);
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM proxy_request_logs", [], |row| {
            row.get(0)
        })?;
        assert_eq!(count, 1);

        Ok(())
    }

    #[test]
    fn test_collect_jsonl_files_includes_subagents() {
        let tmp = std::env::temp_dir().join(format!("cc-switch-test-{}", uuid::Uuid::new_v4()));
        let project = tmp.join("project");
        let session_dir = project.join("test-session");
        let subagents_dir = session_dir.join("subagents");
        fs::create_dir_all(&subagents_dir).unwrap();

        fs::write(project.join("main.jsonl"), "{}").unwrap();
        fs::write(subagents_dir.join("agent-abc.jsonl"), "{}").unwrap();

        let files = collect_jsonl_files(&tmp);
        assert_eq!(files.len(), 2);
        let paths: Vec<String> = files
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect();
        assert!(paths.iter().any(|p| p.contains("main.jsonl")));
        assert!(paths.iter().any(|p| p.contains("agent-abc.jsonl")));

        fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn test_collect_jsonl_files_includes_workflow_subagents() {
        // Claude Code Workflow 把子 agent transcript 嵌在
        // 项目/SESSION_ID/subagents/workflows/wf_<ID>/ 下，比普通子 agent 深一层。
        let tmp = std::env::temp_dir().join(format!("cc-switch-test-{}", uuid::Uuid::new_v4()));
        let project = tmp.join("project");
        let session_dir = project.join("test-session");
        let subagents_dir = session_dir.join("subagents");
        let wf_dir = subagents_dir.join("workflows").join("wf_test123");
        fs::create_dir_all(&wf_dir).unwrap();

        fs::write(project.join("main.jsonl"), "{}").unwrap();
        fs::write(subagents_dir.join("agent-plain.jsonl"), "{}").unwrap();
        fs::write(wf_dir.join("agent-wf.jsonl"), "{}").unwrap();
        // journal.jsonl 也会被收集，但解析时因无 assistant 行而产出 0 条
        fs::write(wf_dir.join("journal.jsonl"), "{}").unwrap();

        let files = collect_jsonl_files(&tmp);
        let paths: Vec<String> = files
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect();

        // 主会话 + 普通子 agent + Workflow 子 agent(agent-wf + journal) = 4
        assert_eq!(files.len(), 4);
        assert!(paths.iter().any(|p| p.contains("main.jsonl")));
        assert!(paths.iter().any(|p| p.contains("agent-plain.jsonl")));
        assert!(
            paths.iter().any(|p| p.contains("agent-wf.jsonl")),
            "Workflow 子 agent transcript 必须被收集"
        );

        fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn test_sync_imports_billable_message_without_stop_reason() -> Result<(), AppError> {
        // 回归：stop_reason 缺失但有真实 cache/input 成本的 message（Workflow /
        // 子 agent 常见的「只有 message_start 快照、没写最终块」形态）必须被计入，
        // 不能因缺 stop_reason 或 output==0 而整条丢弃；全 0 token 的占位行仍应跳过。
        let db = Database::memory()?;
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("agent-wf.jsonl");

        // 第一行：无 stop_reason、output=1，但 cache_read/cache_creation 很大 → 应导入
        // 第二行：全部 token 为 0 → 应跳过（无计费意义）
        let billable = r#"{"type":"assistant","message":{"id":"msg_nostop","model":"claude-opus-4-8","usage":{"input_tokens":2,"output_tokens":1,"cache_read_input_tokens":48719,"cache_creation_input_tokens":2061}},"timestamp":"2026-06-07T13:01:23Z","sessionId":"session-wf"}"#;
        let empty = r#"{"type":"assistant","message":{"id":"msg_empty","model":"claude-opus-4-8","usage":{"input_tokens":0,"output_tokens":0,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}},"timestamp":"2026-06-07T13:01:24Z","sessionId":"session-wf"}"#;
        fs::write(&file, format!("{billable}\n{empty}\n")).unwrap();

        let (imported, _skipped) = sync_single_file(&db, &file)?;
        assert_eq!(
            imported, 1,
            "有 cache 成本但无 stop_reason 的 message 必须被导入"
        );

        let conn = lock_conn!(db.conn);
        let cache_read: i64 = conn.query_row(
            "SELECT cache_read_tokens FROM proxy_request_logs WHERE request_id = 'session:msg_nostop'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(cache_read, 48719, "cache_read 必须被完整记录");
        let empty_exists: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM proxy_request_logs WHERE request_id = 'session:msg_empty')",
            [],
            |row| row.get(0),
        )?;
        assert!(!empty_exists, "全 0 token 的 message 应被跳过");
        Ok(())
    }

    #[test]
    fn test_later_streaming_snapshot_updates_stored_row() -> Result<(), AppError> {
        // SESS-35：同步时刻落在流式回复中途，先导入了半截快照；
        // 追加最终快照后再次同步，应更新同一行而不是被 INSERT OR IGNORE 冻结。
        let db = Database::memory()?;
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("main.jsonl");
        fs::write(&file, format!("{}\n", assistant_line("msg_1", 26, None))).unwrap();

        assert_eq!(sync_single_file(&db, &file)?, (1, 0));
        let (_, partial_output, partial_cost) = row_tokens_and_cost(&db, "session:msg_1");
        assert_eq!(partial_output, 26);

        let previous = get_sync_state(&db, &file.to_string_lossy())?.0;
        let mut f = fs::OpenOptions::new().append(true).open(&file).unwrap();
        writeln!(f, "{}", assistant_line("msg_1", 1349, Some("end_turn"))).unwrap();
        drop(f);
        bump_mtime(&file, previous);

        assert_eq!(sync_single_file(&db, &file)?, (1, 0));
        let (count, output, cost) = row_tokens_and_cost(&db, "session:msg_1");
        assert_eq!(count, 1);
        assert_eq!(output, 1349);
        let partial: f64 = partial_cost.parse().unwrap();
        let final_cost: f64 = cost.parse().unwrap();
        assert!(
            final_cost > partial || final_cost == 0.0 && partial == 0.0,
            "cost must be recomputed from the final snapshot ({partial} -> {final_cost})"
        );
        Ok(())
    }

    #[test]
    fn test_partial_last_line_is_not_consumed() -> Result<(), AppError> {
        // SESS-36：末行写到一半（无换行）时不能推进游标，补全后应被导入。
        let db = Database::memory()?;
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("main.jsonl");
        let line_a = assistant_line("msg_a", 10, Some("end_turn"));
        let line_b = assistant_line("msg_b", 20, Some("end_turn"));
        let (b_head, b_tail) = line_b.split_at(line_b.len() / 2);
        fs::write(&file, format!("{line_a}\n{b_head}")).unwrap();

        assert_eq!(sync_single_file(&db, &file)?, (1, 0));
        let state = get_sync_state(&db, &file.to_string_lossy())?;
        assert_eq!(state.1, 1, "cursor must stop before the truncated line");

        let mut f = fs::OpenOptions::new().append(true).open(&file).unwrap();
        writeln!(f, "{b_tail}").unwrap();
        drop(f);
        bump_mtime(&file, state.0);

        assert_eq!(sync_single_file(&db, &file)?, (1, 0));
        assert_eq!(row_tokens_and_cost(&db, "session:msg_b").1, 20);
        assert_eq!(get_sync_state(&db, &file.to_string_lossy())?.1, 2);
        Ok(())
    }

    #[test]
    fn test_unchanged_files_are_skipped_and_missing_cursors_pruned() -> Result<(), AppError> {
        let db = Database::memory()?;
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path().join("project");
        fs::create_dir_all(&project).unwrap();
        let keep = project.join("keep.jsonl");
        let gone = project.join("gone.jsonl");
        fs::write(&keep, format!("{}\n", assistant_line("msg_k", 5, None))).unwrap();
        fs::write(&gone, format!("{}\n", assistant_line("msg_g", 5, None))).unwrap();

        let first = sync_claude_projects_dir(&db, tmp.path())?;
        assert_eq!((first.imported, first.files_scanned), (2, 2));

        // 未变化的文件不再解析、不再写入
        let second = sync_claude_projects_dir(&db, tmp.path())?;
        assert_eq!((second.imported, second.skipped), (0, 0));

        // SESS-46：文件被删除后，其同步游标在下一轮被清理
        fs::remove_file(&gone).unwrap();
        sync_claude_projects_dir(&db, tmp.path())?;
        let cursors = load_sync_cursors(&db)?;
        assert!(cursors.contains_key(keep.to_string_lossy().as_ref()));
        assert!(!cursors.contains_key(gone.to_string_lossy().as_ref()));
        // 已导入的用量行保持不变
        assert_eq!(row_tokens_and_cost(&db, "session:msg_g").0, 1);
        Ok(())
    }

    #[test]
    fn test_background_sync_is_skipped_while_another_runs() {
        let db = Database::memory().unwrap();
        let _held = SYNC_RUNNING.lock().unwrap_or_else(|e| e.into_inner());
        assert!(sync_all_session_usage(&db, SyncTrigger::Background).is_none());
    }

    #[test]
    fn test_byte_cursor_resumes_without_rereading_prefix() -> Result<(), AppError> {
        // SESS-28: after a sync only appended bytes are parsed. A line rewritten
        // in the already-consumed region (beyond the hashed prefix) is not seen
        // again, which proves the reader seeks instead of re-reading.
        let db = Database::memory()?;
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("main.jsonl");
        let filler = format!("{{\"type\":\"user\",\"pad\":\"{}\"}}", "x".repeat(5000));
        let line_a = assistant_line("msg_aaaa", 10, Some("end_turn"));
        fs::write(&file, format!("{filler}\n{line_a}\n")).unwrap();
        assert_eq!(sync_single_file(&db, &file)?, (1, 0));
        let key = file.to_string_lossy().to_string();
        let byte = load_byte_cursors(&db)?
            .remove(&key)
            .expect("byte cursor stored");
        assert_eq!(byte.offset as u64, fs::metadata(&file).unwrap().len());

        // Same length, different message id, outside the 4 KiB prefix.
        let line_z = assistant_line("msg_zzzz", 10, Some("end_turn"));
        assert_eq!(line_a.len(), line_z.len());
        let line_b = assistant_line("msg_bbbb", 30, Some("end_turn"));
        fs::write(&file, format!("{filler}\n{line_z}\n{line_b}\n")).unwrap();
        bump_mtime(&file, get_sync_state(&db, &key)?.0);

        assert_eq!(sync_single_file(&db, &file)?, (1, 0));
        assert_eq!(row_tokens_and_cost(&db, "session:msg_bbbb").1, 30);
        let conn = db.conn.lock().unwrap();
        let z: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM proxy_request_logs WHERE request_id = 'session:msg_zzzz'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(z, 0, "consumed bytes are not parsed again");
        drop(conn);
        assert_eq!(get_sync_state(&db, &key)?.1, 3);
        Ok(())
    }

    #[test]
    fn test_rewritten_file_is_reread_from_start() -> Result<(), AppError> {
        let db = Database::memory()?;
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("main.jsonl");
        let line_a = assistant_line("msg_a", 10, Some("end_turn"));
        let line_b = assistant_line("msg_b", 20, Some("end_turn"));
        fs::write(&file, format!("{line_a}\n{line_b}\n")).unwrap();
        assert_eq!(sync_single_file(&db, &file)?, (2, 0));
        let key = file.to_string_lossy().to_string();
        let mtime = get_sync_state(&db, &key)?.0;

        // Rewritten with a different first line: the prefix hash no longer
        // matches, so the whole file is parsed again.
        let line_c = assistant_line("msg_c", 40, Some("end_turn"));
        fs::write(&file, format!("{line_c}\n{line_b}\n{line_a}\n")).unwrap();
        bump_mtime(&file, mtime);
        let (imported, _) = sync_single_file(&db, &file)?;
        assert!(imported >= 1);
        assert_eq!(row_tokens_and_cost(&db, "session:msg_c").1, 40);
        assert_eq!(get_sync_state(&db, &key)?.1, 3);
        Ok(())
    }

    #[test]
    fn test_truncated_file_invalidates_byte_cursor() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("t.jsonl");
        fs::write(&file, "aaaa\nbbbb\n").unwrap();
        let cursor = ByteCursor {
            offset: 10,
            prefix_hash: file_prefix_hash(&file, 10).unwrap(),
        };
        assert!(byte_cursor_still_valid(&file, 10, &cursor));
        fs::write(&file, "aaaa\n").unwrap();
        assert!(!byte_cursor_still_valid(&file, 5, &cursor));
        // An offset that is not on a line boundary is rejected.
        fs::write(&file, "aaaa\nbbbb\n").unwrap();
        let mid = ByteCursor {
            offset: 7,
            prefix_hash: file_prefix_hash(&file, 7).unwrap(),
        };
        assert!(!byte_cursor_still_valid(&file, 10, &mid));
    }
}
