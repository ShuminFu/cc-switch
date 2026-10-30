//! Claude Code 会话日志使用追踪
//!
//! 从 ~/.claude/projects/ 下的 JSONL 会话文件中提取 token 使用数据，
//! 实现无代理模式下的使用统计。
//!
//! ## 数据流
//! ```text
//! ~/.claude/projects/*/*.jsonl → 增量解析 → 去重 → 费用计算 → proxy_request_logs 表
//! ```

use crate::config::get_claude_config_dir;
use crate::database::repo::session_cursor::{self, SessionCursor};
use crate::database::repo::usage::{self as usage_repo, SessionUsageRow, UsageCosts, UsageSource};
use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::pricing::resolver::find_model_pricing;
use crate::proxy::usage::anthropic_fields::AnthropicUsageFields;
use crate::proxy::usage::calculator::{CostCalculator, ModelPricing};
use crate::proxy::usage::parser::TokenUsage;
use crate::services::usage_stats::{should_skip_session_insert, DedupKey};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::SystemTime;

/// 同步结果
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct SessionSyncResult {
    pub imported: u32,
    pub skipped: u32,
    pub files_scanned: u32,
    pub errors: Vec<String>,
}

/// 数据来源分布与数据质量报告
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct DataSourceSummary {
    pub data_source: String,
    pub request_count: u32,
    pub total_cost_usd: String,
    /// 被代理日志覆盖、不计入统计的重复会话行
    pub duplicate_count: u32,
    /// 有 token 但成本为 0 的行（缺定价）
    pub unpriced_count: u32,
    /// 没有 session_id 的行
    pub missing_session_count: u32,
    /// output_tokens 为 0 的行（可能只记录了流式开头快照，可通过重扫修复）
    pub zero_output_count: u32,
    /// 最早 / 最晚一条记录的时间（Unix 秒）
    pub first_created_at: Option<i64>,
    pub last_created_at: Option<i64>,
}

/// 会话导入模式
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum SyncMode {
    /// 常规增量同步：从游标处继续，已导入的行只按各来源既有规则更新
    #[default]
    Incremental,
    /// 重扫：忽略游标从 offset 0 重新导入，修复 token 偏少的已有行（只升不降）
    Rescan {
        /// 已归档时段的边界（见 [`usage_repo::rollup_horizon`]），早于它的新行不再插入
        rollup_horizon: Option<i64>,
    },
}

impl SyncMode {
    /// 读取 `key` 的同步进度；重扫模式视为从未同步 `(0, 0)`。
    pub(crate) fn sync_state(self, db: &Database, key: &str) -> Result<(i64, i64), AppError> {
        match self {
            SyncMode::Incremental => get_sync_state(db, key),
            SyncMode::Rescan { .. } => Ok((0, 0)),
        }
    }

    pub(crate) fn is_rescan(self) -> bool {
        matches!(self, SyncMode::Rescan { .. })
    }

    /// 重扫模式下先处理已存在的同来源行与已归档时段的行。
    ///
    /// 返回 `Some(wrote)` 表示该行已处理完毕（修复或跳过），`None` 表示继续常规
    /// 插入路径。增量模式总是返回 `None`。
    pub(crate) fn rescan_write(
        self,
        conn: &rusqlite::Connection,
        row: &SessionUsageRow<'_>,
    ) -> Result<Option<bool>, AppError> {
        let SyncMode::Rescan { rollup_horizon } = self else {
            return Ok(None);
        };
        if let Some(repaired) = usage_repo::repair_session_row(conn, row)? {
            return Ok(Some(repaired));
        }
        if rollup_horizon.is_some_and(|horizon| row.created_at < horizon) {
            return Ok(Some(false));
        }
        Ok(None)
    }
}

/// 重扫进度（每个来源完成后推送一次）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct SessionRescanProgress {
    /// 刚完成的来源名（Claude / Codex / Gemini / OpenCode）
    pub source: String,
    pub completed: u32,
    pub total: u32,
    /// 截至目前累计的新导入 + 修复行数
    pub imported: u32,
    pub done: bool,
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

/// 同步 Claude Code 会话日志到使用统计数据库
pub fn sync_claude_session_logs(db: &Database) -> Result<SessionSyncResult, AppError> {
    sync_claude_session_logs_with_mode(db, SyncMode::Incremental)
}

pub(crate) fn sync_claude_session_logs_with_mode(
    db: &Database,
    mode: SyncMode,
) -> Result<SessionSyncResult, AppError> {
    let projects_dir = get_claude_config_dir().join("projects");
    if !projects_dir.exists() {
        return Ok(SessionSyncResult {
            imported: 0,
            skipped: 0,
            files_scanned: 0,
            errors: vec![],
        });
    }

    let mut result = SessionSyncResult {
        imported: 0,
        skipped: 0,
        files_scanned: 0,
        errors: vec![],
    };

    // 收集所有 .jsonl 文件
    let jsonl_files = collect_jsonl_files(&projects_dir);

    for file_path in &jsonl_files {
        result.files_scanned += 1;

        match sync_single_file(db, file_path, mode) {
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
/// 会被 `sync_single_file` 天然跳过，因此这里无需按文件名过滤。
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

/// 同步单个 JSONL 文件，返回 (imported, skipped)
fn sync_single_file(
    db: &Database,
    file_path: &Path,
    mode: SyncMode,
) -> Result<(u32, u32), AppError> {
    let file_path_str = file_path.to_string_lossy().to_string();

    // 获取文件元数据
    let metadata = fs::metadata(file_path)
        .map_err(|e| AppError::Config(format!("无法读取文件元数据: {e}")))?;
    let file_modified = metadata_modified_nanos(&metadata);

    // 检查同步状态（重扫模式从 offset 0 开始）
    let (last_modified, last_offset) = mode.sync_state(db, &file_path_str)?;

    // 文件未变化则跳过
    if file_modified <= last_modified {
        return Ok((0, 0));
    }

    // 从上次偏移位置开始增量解析
    let file =
        fs::File::open(file_path).map_err(|e| AppError::Config(format!("无法打开文件: {e}")))?;
    let reader = BufReader::new(file);

    let mut line_offset: i64 = 0;
    let mut messages: HashMap<String, ParsedAssistantUsage> = HashMap::new();
    let mut current_session_id: Option<String> = None;

    // 只遍历以 '\n' 结尾的完整行，正在写入的末尾半行不计入 offset
    for line_result in CompleteLines::new(reader) {
        line_offset += 1;

        // 跳过已处理的行
        if line_offset <= last_offset {
            continue;
        }

        let line = match line_result {
            Ok(l) => l,
            Err(_) => continue, // 非 UTF-8 的完整行
        };

        if line.trim().is_empty() {
            continue;
        }

        let value: serde_json::Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };

        // 提取 session ID (从 system 或首条消息)
        if current_session_id.is_none() {
            if let Some(sid) = value.get("sessionId").and_then(|v| v.as_str()) {
                current_session_id = Some(sid.to_string());
            }
        }

        // 只处理 assistant 类型的消息
        if value.get("type").and_then(|t| t.as_str()) != Some("assistant") {
            continue;
        }

        let message = match value.get("message") {
            Some(m) => m,
            None => continue,
        };

        let msg_id = match message.get("id").and_then(|v| v.as_str()) {
            Some(id) => id.to_string(),
            None => continue,
        };

        let usage = match message.get("usage") {
            Some(u) => u,
            None => continue,
        };

        let counts = AnthropicUsageFields::read(usage).to_token_usage();
        let parsed = ParsedAssistantUsage {
            message_id: msg_id.clone(),
            model: message
                .get("model")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string(),
            input_tokens: counts.input_tokens,
            output_tokens: counts.output_tokens,
            cache_read_tokens: counts.cache_read_tokens,
            cache_creation_tokens: counts.cache_creation_tokens,
            stop_reason: message
                .get("stop_reason")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            timestamp: value
                .get("timestamp")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            session_id: current_session_id.clone(),
        };

        // 按 message.id 去重：优先保留有 stop_reason 的条目，否则保留最新的
        let should_replace = match messages.get(&msg_id) {
            None => true,
            Some(existing) => {
                // 新条目有 stop_reason 而旧条目没有 → 替换
                if parsed.stop_reason.is_some() && existing.stop_reason.is_none() {
                    true
                }
                // 两个都有或都没有 stop_reason → 取 output_tokens 更大的
                else if parsed.stop_reason.is_some() == existing.stop_reason.is_some() {
                    parsed.output_tokens > existing.output_tokens
                } else {
                    false
                }
            }
        };

        if should_replace {
            messages.insert(msg_id, parsed);
        }
    }

    // 写入数据库：本文件的所有行与游标在同一个事务里提交
    let cursor = SessionCursor {
        last_modified: file_modified,
        last_offset: line_offset,
    };
    ingest_file(db, &file_path_str, |conn| {
        let mut imported: u32 = 0;
        let mut skipped: u32 = 0;

        for msg in messages.values() {
            // 只要产生了真实计费 token 就导入，不再强制要求 stop_reason 或 output>0。
            //
            // Anthropic 在受理请求时即对 input + cache_read + cache_creation 计费
            // （这些在请求开始就确定），output 按实际生成量计。Workflow / 子 agent 的
            // 并行短命请求经常只写了 message_start 快照（output=1、stop_reason=None）
            // 却没有写最终块，但其 cache/input 成本已被真实计费。旧逻辑用 stop_reason
            // 非空 + output>0 双重过滤，会把这类请求整条丢弃，实测系统性低估约 4.1%，
            // 且 92% 集中在 workflow/subagent。这里改为「任一计费维度 > 0 即导入」。
            //
            // 去重选择逻辑（上方按 message.id 取 stop_reason 优先 / output 最大者）保持
            // 不变：它选出的代表行的 input/cache 本就准确；request_id = session:msg_id
            // 主键 + INSERT OR IGNORE 保证一个 message 仍只落库一次，放宽 gate 不会双算。
            let has_billable_tokens = msg.input_tokens > 0
                || msg.output_tokens > 0
                || msg.cache_read_tokens > 0
                || msg.cache_creation_tokens > 0;
            if !has_billable_tokens {
                continue;
            }

            let request_id = format!(
                "{}{}",
                crate::proxy::usage::parser::SESSION_REQUEST_ID_PREFIX,
                msg.message_id
            );

            match insert_session_log_entry(conn, &request_id, msg, mode) {
                Ok(true) => imported += 1,
                Ok(false) => skipped += 1,
                Err(e) => {
                    log::warn!("[SESSION-SYNC] 插入失败 ({}): {e}", msg.message_id);
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

/// 一个导入来源（文件 / OpenCode 会话）在一个事务里写入的结果
pub(crate) struct IngestBatch<T> {
    pub value: T,
    /// 是否写入或更新了使用量行（用于通知前端刷新）
    pub wrote_rows: bool,
    /// 要推进到的游标；`None` 表示本次不推进（下次重试）
    pub cursor: Option<SessionCursor>,
}

/// 在一个事务里执行 `write` 并写入 `key` 的游标，一起提交。
///
/// 进程在两个文件之间（或文件写到一半时）被杀掉，要么整个文件连同游标都已提交，
/// 要么都没有：重跑时不会出现「行已写入、游标未推进」的重复导入窗口。
/// 单行写入失败由 `write` 自行记录并跳过；游标写入或提交失败时整个文件回滚。
pub(crate) fn ingest_file<T>(
    db: &Database,
    key: &str,
    write: impl FnOnce(&rusqlite::Connection) -> Result<IngestBatch<T>, AppError>,
) -> Result<T, AppError> {
    let mut conn = lock_conn!(db.conn);
    let tx = conn
        .transaction()
        .map_err(|e| AppError::Database(format!("开启导入事务失败: {e}")))?;
    let batch = write(&tx)?;
    // 某些错误（磁盘满、IO 错误等）会让 SQLite 自动回滚整个事务；此时再写游标会在
    // 自动提交模式下单独落库，出现「行已回滚、游标已推进」而永久漏导入。
    if tx.is_autocommit() {
        return Err(AppError::Database(
            "导入事务已被 SQLite 回滚，本文件稍后重试".to_string(),
        ));
    }
    if let Some(cursor) = batch.cursor {
        session_cursor::put(&tx, key, cursor)?;
    }
    tx.commit()
        .map_err(|e| AppError::Database(format!("提交导入事务失败: {e}")))?;
    drop(conn);

    if batch.wrote_rows {
        crate::usage_events::notify_log_recorded();
    }
    Ok(batch.value)
}

/// 逐行读取 JSONL，只产出以 `\n` 结尾的完整行（已去掉行尾 `\n` / `\r\n`）。
///
/// 末尾尚未写完的半行不会被产出，也就不会计入 `last_line_offset`；
/// 写入方补全该行后 mtime 变化，下次同步会从这一行继续解析。
/// 非 UTF-8 的完整行产出 `Err`，调用方应计数后跳过。
///
/// Shared by the line-based session_usage_* parsers.
pub(crate) struct CompleteLines<R> {
    reader: R,
    buf: Vec<u8>,
}

impl<R: BufRead> CompleteLines<R> {
    pub(crate) fn new(reader: R) -> Self {
        Self {
            reader,
            buf: Vec::new(),
        }
    }
}

impl<R: BufRead> Iterator for CompleteLines<R> {
    type Item = Result<String, std::string::FromUtf8Error>;

    fn next(&mut self) -> Option<Self::Item> {
        self.buf.clear();
        match self.reader.read_until(b'\n', &mut self.buf) {
            Ok(0) | Err(_) => None,
            // EOF 前的半行：不产出，留待下次同步
            Ok(_) if self.buf.last() != Some(&b'\n') => None,
            Ok(_) => {
                self.buf.pop();
                if self.buf.last() == Some(&b'\r') {
                    self.buf.pop();
                }
                Some(String::from_utf8(std::mem::take(&mut self.buf)))
            }
        }
    }
}

/// 获取 session_log_sync 表中某条目的同步进度 `(last_modified, last_offset)`。
///
/// Shared by all session_usage_* parsers.
pub(crate) fn get_sync_state(db: &Database, file_path: &str) -> Result<(i64, i64), AppError> {
    let conn = lock_conn!(db.conn);
    let cursor = session_cursor::get(&conn, file_path)?;
    Ok((cursor.last_modified, cursor.last_offset))
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

/// 更新 session_log_sync 表中某条目的同步进度（单独提交，不与使用量行同事务）。
///
/// Shared by all session_usage_* parsers.
pub(crate) fn update_sync_state(
    db: &Database,
    file_path: &str,
    last_modified: i64,
    last_offset: i64,
) -> Result<(), AppError> {
    let conn = lock_conn!(db.conn);
    session_cursor::put(
        &conn,
        file_path,
        SessionCursor {
            last_modified,
            last_offset,
        },
    )
}

/// 插入单条会话日志到 proxy_request_logs，返回是否写入了数据
/// (true=新插入或升级了已有的流式快照行, false=已存在/跳过)
///
/// 去重只在单次解析内按 message.id 进行；若上一轮同步已导入了同一 message 的
/// 早期流式快照（stop_reason 为空、output_tokens 偏小），本轮读到最终条目时
/// 不能被 `should_skip_session_insert` / `INSERT OR IGNORE` 丢弃，否则 output
/// 与费用会被永久低估。表中没有保存 stop_reason 的列，因此按 output_tokens
/// 更大来判定是否为更新的快照，并重写 token 与费用列。
///
/// 重扫模式下由 [`SyncMode::rescan_write`] 先按 token 总量修复已有行。
fn insert_session_log_entry(
    conn: &rusqlite::Connection,
    request_id: &str,
    msg: &ParsedAssistantUsage,
    mode: SyncMode,
) -> Result<bool, AppError> {
    let created_at = msg
        .timestamp
        .as_ref()
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

    // 计算费用
    let usage = TokenUsage {
        input_tokens: msg.input_tokens,
        output_tokens: msg.output_tokens,
        cache_read_tokens: msg.cache_read_tokens,
        cache_creation_tokens: msg.cache_creation_tokens,
        model: Some(msg.model.clone()),
        message_id: None,
    };

    let pricing = find_model_pricing_for_session(conn, &msg.model);
    let multiplier = Decimal::from(1);
    let costs = match pricing {
        Some(p) => UsageCosts::from_breakdown(&CostCalculator::calculate(&usage, &p, multiplier)),
        None => UsageCosts::zero(),
    };

    let row = SessionUsageRow {
        request_id,
        source: UsageSource::ClaudeSession,
        model: &msg.model,
        pricing_model: &msg.model, // 会话日志按响应模型计价
        session_id: msg.session_id.as_deref(),
        input_tokens: msg.input_tokens,
        output_tokens: msg.output_tokens,
        cache_read_tokens: msg.cache_read_tokens,
        cache_creation_tokens: msg.cache_creation_tokens,
        costs,
        created_at,
    };

    if let Some(wrote) = mode.rescan_write(conn, &row)? {
        return Ok(wrote);
    }

    let dedup_key = DedupKey {
        app_type: "claude",
        model: &msg.model,
        input_tokens: msg.input_tokens,
        output_tokens: msg.output_tokens,
        cache_read_tokens: msg.cache_read_tokens,
        cache_creation_tokens: msg.cache_creation_tokens,
        created_at,
    };
    // 已有同 message 的会话日志行：仅当新条目 output 更大时升级（见函数注释）
    let existing_output =
        usage_repo::session_row_output_tokens(conn, request_id, UsageSource::ClaudeSession)?;
    let upgrade_existing = match existing_output {
        Some(old_output) if i64::from(msg.output_tokens) > old_output => true,
        Some(_) => return Ok(false),
        None => false,
    };

    if !upgrade_existing && should_skip_session_insert(conn, request_id, &dedup_key)? {
        return Ok(false);
    }

    if upgrade_existing {
        return usage_repo::upgrade_session_row(conn, &row);
    }

    usage_repo::insert_session_row(conn, &row)?;
    Ok(true)
}

/// 从 model_pricing 表查找模型定价（支持模糊匹配）
fn find_model_pricing_for_session(
    conn: &rusqlite::Connection,
    model_id: &str,
) -> Option<ModelPricing> {
    find_model_pricing(conn, model_id)
}

/// 查询数据来源分布与数据质量统计
pub fn get_data_source_breakdown(db: &Database) -> Result<Vec<DataSourceSummary>, AppError> {
    let conn = lock_conn!(db.conn);
    let to_u32 = |v: i64| v.clamp(0, i64::from(u32::MAX)) as u32;
    Ok(usage_repo::data_source_breakdown(&conn)?
        .into_iter()
        .map(|stats| DataSourceSummary {
            data_source: stats.data_source,
            request_count: to_u32(stats.request_count),
            total_cost_usd: format!("{:.6}", stats.total_cost_usd),
            duplicate_count: to_u32(stats.duplicate_count),
            unpriced_count: to_u32(stats.unpriced_count),
            missing_session_count: to_u32(stats.missing_session_count),
            zero_output_count: to_u32(stats.zero_output_count),
            first_created_at: stats.first_created_at,
            last_created_at: stats.last_created_at,
        })
        .collect())
}

/// 会话数据同步函数签名
type SessionSyncFn = fn(&Database, SyncMode) -> Result<SessionSyncResult, AppError>;

/// Claude 之外的会话来源，按同步顺序排列
const OTHER_SESSION_SOURCES: [(&str, SessionSyncFn); 3] = [
    (
        "Codex",
        crate::services::session_usage_codex::sync_codex_usage_with_mode,
    ),
    (
        "Gemini",
        crate::services::session_usage_gemini::sync_gemini_usage_with_mode,
    ),
    (
        "OpenCode",
        crate::services::session_usage_opencode::sync_opencode_usage_with_mode,
    ),
];

fn merge_sync_result(into: &mut SessionSyncResult, other: SessionSyncResult) {
    into.imported += other.imported;
    into.skipped += other.skipped;
    into.files_scanned += other.files_scanned;
    into.errors.extend(other.errors);
}

/// 依次同步 Claude / Codex / Gemini / OpenCode 的会话数据并合并结果。
///
/// Claude 同步失败直接返回错误；其他来源失败只记录到 `errors` 中。
/// 这是阻塞函数（文件扫描 + 同步 Mutex 数据库操作），
/// 在异步上下文中请使用 [`sync_all_session_usage_blocking`]。
pub fn sync_all_session_usage(db: &Database) -> Result<SessionSyncResult, AppError> {
    // 同步 Claude 会话日志
    let mut result = sync_claude_session_logs(db)?;

    // 同步 Codex / Gemini / OpenCode 使用数据
    for (name, sync) in OTHER_SESSION_SOURCES {
        match sync(db, SyncMode::Incremental) {
            Ok(other) => merge_sync_result(&mut result, other),
            Err(e) => {
                result.errors.push(format!("{name} 同步失败: {e}"));
            }
        }
    }

    Ok(result)
}

/// 正在进行的重扫（同一时间只允许一个）
static RESCAN_RUNNING: AtomicBool = AtomicBool::new(false);

/// 重扫全部会话来源：忽略游标、从 offset 0 重新导入，修复 P1 修复前被低估的
/// 已有行（按 request_id upsert，只升不降，不会产生重复行）。
///
/// 每个来源完成后调用一次 `on_progress`。早于日汇总边界的时段只修复仍在明细表
/// 里的行，不再插入新行（见 [`usage_repo::rollup_horizon`]）。各来源的游标在
/// 各自的文件事务里重写为本次扫描到的位置。已有重扫在进行时返回错误。
pub fn rescan_all_session_usage(
    db: &Database,
    on_progress: impl FnMut(SessionRescanProgress),
) -> Result<SessionSyncResult, AppError> {
    let sources = std::iter::once((
        "Claude",
        sync_claude_session_logs_with_mode as SessionSyncFn,
    ))
    .chain(OTHER_SESSION_SOURCES)
    .collect::<Vec<_>>();
    rescan_sources(db, &sources, on_progress)
}

/// [`rescan_all_session_usage`] 的实现：按顺序以重扫模式同步 `sources`。
fn rescan_sources(
    db: &Database,
    sources: &[(&str, SessionSyncFn)],
    mut on_progress: impl FnMut(SessionRescanProgress),
) -> Result<SessionSyncResult, AppError> {
    if RESCAN_RUNNING.swap(true, Ordering::AcqRel) {
        return Err(AppError::Message("会话日志重扫正在进行中".to_string()));
    }
    struct RunningGuard;
    impl Drop for RunningGuard {
        fn drop(&mut self) {
            RESCAN_RUNNING.store(false, Ordering::Release);
        }
    }
    let _guard = RunningGuard;

    let rollup_horizon = {
        let conn = lock_conn!(db.conn);
        usage_repo::rollup_horizon(&conn)?
    };
    let mode = SyncMode::Rescan { rollup_horizon };
    log::info!("[SESSION-SYNC] 开始重扫会话日志 (归档边界: {rollup_horizon:?})");

    let total = sources.len() as u32;
    let mut result = SessionSyncResult {
        imported: 0,
        skipped: 0,
        files_scanned: 0,
        errors: vec![],
    };
    for (index, (name, sync)) in sources.iter().enumerate() {
        match sync(db, mode) {
            Ok(other) => merge_sync_result(&mut result, other),
            Err(e) => result.errors.push(format!("{name} 重扫失败: {e}")),
        }
        let completed = index as u32 + 1;
        on_progress(SessionRescanProgress {
            source: name.to_string(),
            completed,
            total,
            imported: result.imported,
            done: completed == total,
        });
    }

    log::info!(
        "[SESSION-SYNC] 重扫完成: 导入/修复 {} 条, 跳过 {} 条, 扫描 {} 个来源文件, 错误 {} 个",
        result.imported,
        result.skipped,
        result.files_scanned,
        result.errors.len()
    );
    Ok(result)
}

/// 在阻塞线程池中执行同步任务，避免占用 tokio 工作线程或 Tauri 主线程。
pub async fn run_sync_blocking<T, F>(label: &'static str, f: F) -> Result<T, AppError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, AppError> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(move || {
        log::debug!(
            "[SESSION-SYNC] {label} running on thread {:?}",
            std::thread::current().name().unwrap_or("<unnamed>")
        );
        f()
    })
    .await
    .map_err(|e| AppError::Message(format!("{label} 任务执行失败: {e}")))?
}

/// [`sync_all_session_usage`] 的异步版本：在阻塞线程池中运行。
pub async fn sync_all_session_usage_blocking(
    db: Arc<Database>,
) -> Result<SessionSyncResult, AppError> {
    run_sync_blocking("Session usage sync", move || sync_all_session_usage(&db)).await
}

/// [`rescan_all_session_usage`] 的异步版本：在阻塞线程池中运行，进度通过
/// [`crate::usage_events::notify_rescan_progress`] 推送给前端。
pub async fn rescan_all_session_usage_blocking(
    db: Arc<Database>,
) -> Result<SessionSyncResult, AppError> {
    run_sync_blocking("Session usage rescan", move || {
        rescan_all_session_usage(&db, crate::usage_events::notify_rescan_progress)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proxy::usage::test_data::claude_session_line;

    #[test]
    fn test_parse_usage_from_jsonl_line() {
        let line = claude_session_line(
            "msg_test123",
            "claude-opus-4-6",
            Some("end_turn"),
            [3, 150, 5000, 10000],
            "2026-04-05T12:00:00Z",
            "session-abc",
        );

        let value: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(
            value.get("type").and_then(|t| t.as_str()),
            Some("assistant")
        );

        let message = value.get("message").unwrap();
        let usage = AnthropicUsageFields::read(message.get("usage").unwrap()).to_token_usage();

        assert_eq!(usage.input_tokens, 3);
        assert_eq!(usage.output_tokens, 150);
        assert_eq!(usage.cache_read_tokens, 5000);
        assert_eq!(usage.cache_creation_tokens, 10000);
        assert_eq!(
            message.get("stop_reason").unwrap().as_str().unwrap(),
            "end_turn"
        );
    }

    #[test]
    fn test_dedup_by_message_id() {
        // 同一个 message.id 有多条，应该取 stop_reason 有值的那条
        let mut messages: HashMap<String, ParsedAssistantUsage> = HashMap::new();

        // 中间条目（无 stop_reason）
        let intermediate = ParsedAssistantUsage {
            message_id: "msg_1".to_string(),
            model: "claude-opus-4-6".to_string(),
            input_tokens: 3,
            output_tokens: 26,
            cache_read_tokens: 5000,
            cache_creation_tokens: 10000,
            stop_reason: None,
            timestamp: Some("2026-04-05T12:00:00Z".to_string()),
            session_id: None,
        };
        messages.insert("msg_1".to_string(), intermediate);

        // 最终条目（有 stop_reason）
        let final_entry = ParsedAssistantUsage {
            message_id: "msg_1".to_string(),
            model: "claude-opus-4-6".to_string(),
            input_tokens: 3,
            output_tokens: 1349,
            cache_read_tokens: 5000,
            cache_creation_tokens: 10000,
            stop_reason: Some("end_turn".to_string()),
            timestamp: Some("2026-04-05T12:00:00Z".to_string()),
            session_id: None,
        };

        // 应该替换
        let should_replace = final_entry.stop_reason.is_some()
            && messages.get("msg_1").unwrap().stop_reason.is_none();
        assert!(should_replace);

        messages.insert("msg_1".to_string(), final_entry);
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

        let inserted = insert_session_log_entry(
            &db.conn.lock().unwrap(),
            "session:msg_1",
            &msg,
            SyncMode::Incremental,
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
        let tmp = std::env::temp_dir().join(format!("cc-switch-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&tmp).unwrap();
        let file = tmp.join("agent-wf.jsonl");

        // 第一行：无 stop_reason、output=1，但 cache_read/cache_creation 很大 → 应导入
        // 第二行：全部 token 为 0 → 应跳过（无计费意义）
        let billable = claude_session_line(
            "msg_nostop",
            "claude-opus-4-8",
            None,
            [2, 1, 48719, 2061],
            "2026-06-07T13:01:23Z",
            "session-wf",
        );
        let empty = claude_session_line(
            "msg_empty",
            "claude-opus-4-8",
            None,
            [0, 0, 0, 0],
            "2026-06-07T13:01:24Z",
            "session-wf",
        );
        fs::write(&file, format!("{billable}\n{empty}\n")).unwrap();

        let (imported, _skipped) = sync_single_file(&db, &file, SyncMode::Incremental)?;
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
        drop(conn);

        fs::remove_dir_all(&tmp).ok();
        Ok(())
    }

    #[test]
    fn test_sync_defers_partial_trailing_line_until_complete() -> Result<(), AppError> {
        // 回归：末尾正在写入的半行不能计入 line_offset，否则补全后会被永久跳过。
        let db = Database::memory()?;
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("partial.jsonl");
        let first = r#"{"type":"assistant","message":{"id":"msg_first","model":"claude-opus-4-8","stop_reason":"end_turn","usage":{"input_tokens":10,"output_tokens":5}},"timestamp":"2026-06-07T13:01:23Z","sessionId":"session-partial"}"#;
        let second = r#"{"type":"assistant","message":{"id":"msg_second","model":"claude-opus-4-8","stop_reason":"end_turn","usage":{"input_tokens":20,"output_tokens":7}},"timestamp":"2026-06-07T13:01:24Z","sessionId":"session-partial"}"#;
        let (head, tail) = second.split_at(second.len() / 2);
        fs::write(&file, format!("{first}\n{head}")).unwrap();

        let (imported, _) = sync_single_file(&db, &file, SyncMode::Incremental)?;
        assert_eq!(imported, 1, "只有完整的第一行应被导入");
        let (first_mtime, offset) = get_sync_state(&db, &file.to_string_lossy())?;
        assert_eq!(offset, 1, "未写完的半行不能计入 offset");

        // 写入方补全第二行；回拨已记录的 mtime 模拟文件更新（避免依赖时间戳精度）
        fs::write(&file, format!("{first}\n{head}{tail}\n")).unwrap();
        update_sync_state(&db, &file.to_string_lossy(), first_mtime - 1, offset)?;

        let (imported, _) = sync_single_file(&db, &file, SyncMode::Incremental)?;
        assert_eq!(imported, 1, "补全后的第二行应被导入");
        assert_eq!(get_sync_state(&db, &file.to_string_lossy())?.1, 2);

        let conn = lock_conn!(db.conn);
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM proxy_request_logs
             WHERE request_id IN ('session:msg_first', 'session:msg_second')",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(count, 2);
        Ok(())
    }

    #[test]
    fn test_sync_upgrades_partial_snapshot_when_final_entry_arrives() -> Result<(), AppError> {
        // 回归：上一轮同步已导入流式早期快照（output=1、stop_reason=null），
        // 本轮读到同一 message 的最终条目时必须升级该行，而不是被去重丢弃。
        let db = Database::memory()?;
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("streaming.jsonl");
        let partial = claude_session_line(
            "msg_1",
            "claude-sonnet-4-5",
            None,
            [10, 1, 100, 50],
            "2026-06-07T13:01:23Z",
            "session-stream",
        );
        let final_entry = claude_session_line(
            "msg_1",
            "claude-sonnet-4-5",
            Some("end_turn"),
            [10, 500, 100, 50],
            "2026-06-07T13:01:25Z",
            "session-stream",
        );
        let path_str = file.to_string_lossy().to_string();

        fs::write(&file, format!("{partial}\n")).unwrap();
        let (imported, _) = sync_single_file(&db, &file, SyncMode::Incremental)?;
        assert_eq!(imported, 1);
        let read_row = |db: &Database| -> Result<(i64, String, String, i64), AppError> {
            let conn = lock_conn!(db.conn);
            Ok(conn.query_row(
                "SELECT output_tokens, output_cost_usd, total_cost_usd,
                        (SELECT COUNT(*) FROM proxy_request_logs WHERE request_id = 'session:msg_1')
                 FROM proxy_request_logs WHERE request_id = 'session:msg_1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )?)
        };
        let (output, _, partial_total, _) = read_row(&db)?;
        assert_eq!(output, 1);

        // 写入方追加最终条目；回拨已记录的 mtime 模拟文件更新
        fs::write(&file, format!("{partial}\n{final_entry}\n")).unwrap();
        let (mtime, offset) = get_sync_state(&db, &path_str)?;
        update_sync_state(&db, &path_str, mtime - 1, offset)?;
        let (imported, _) = sync_single_file(&db, &file, SyncMode::Incremental)?;
        assert_eq!(imported, 1, "最终条目应升级已有快照行");

        let (output, output_cost, total_cost, count) = read_row(&db)?;
        assert_eq!(output, 500);
        assert_eq!(count, 1, "同一 message 只能有一行");

        let expected = {
            let conn = lock_conn!(db.conn);
            let pricing = find_model_pricing_for_session(&conn, "claude-sonnet-4-5")
                .expect("seeded pricing for claude-sonnet-4-5");
            let usage = TokenUsage {
                input_tokens: 10,
                output_tokens: 500,
                cache_read_tokens: 100,
                cache_creation_tokens: 50,
                model: Some("claude-sonnet-4-5".to_string()),
                message_id: None,
            };
            CostCalculator::calculate(&usage, &pricing, Decimal::from(1))
        };
        assert_eq!(output_cost, expected.output_cost.to_string());
        assert_eq!(total_cost, expected.total_cost.to_string());
        assert_ne!(total_cost, partial_total, "费用必须按最终条目重算");

        // 再次读到更小 output 的旧快照时不能降级
        let stale = ParsedAssistantUsage {
            message_id: "msg_1".to_string(),
            model: "claude-sonnet-4-5".to_string(),
            input_tokens: 10,
            output_tokens: 1,
            cache_read_tokens: 100,
            cache_creation_tokens: 50,
            stop_reason: None,
            timestamp: Some("2026-06-07T13:01:23Z".to_string()),
            session_id: Some("session-stream".to_string()),
        };
        assert!(!insert_session_log_entry(
            &db.conn.lock().unwrap(),
            "session:msg_1",
            &stale,
            SyncMode::Incremental
        )?);
        assert_eq!(read_row(&db)?.0, 500);
        Ok(())
    }

    #[test]
    fn test_complete_lines_skips_unterminated_tail() {
        let lines: Vec<_> = CompleteLines::new(&b"a\r\n\nb\n\xff\nc"[..])
            .map(|line| line.ok())
            .collect();
        assert_eq!(
            lines,
            vec![
                Some("a".to_string()),
                Some(String::new()),
                Some("b".to_string()),
                None,
            ]
        );
    }

    #[test]
    fn test_run_sync_blocking_runs_off_caller_thread() -> Result<(), AppError> {
        let caller = std::thread::current().id();
        let (worker, value) =
            tauri::async_runtime::block_on(run_sync_blocking("test sync", || {
                Ok((std::thread::current().id(), 42))
            }))?;
        assert_ne!(
            worker, caller,
            "sync work must not run on the caller thread"
        );
        assert_eq!(value, 42);
        Ok(())
    }

    #[test]
    fn test_run_sync_blocking_does_not_stall_async_workers() -> Result<(), AppError> {
        // 单线程运行时中，阻塞任务期间其他异步任务仍应能推进
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .map_err(|e| AppError::Message(e.to_string()))?;
        rt.block_on(async {
            let (tx, rx) = std::sync::mpsc::channel::<()>();
            let blocking = run_sync_blocking("test sync", move || {
                rx.recv_timeout(std::time::Duration::from_secs(5))
                    .map_err(|e| AppError::Message(e.to_string()))
            });
            let ticker = async move {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                tx.send(()).ok();
            };
            let (res, ()) = tokio::join!(blocking, ticker);
            res
        })
    }

    #[test]
    fn test_run_sync_blocking_propagates_errors() {
        let err = tauri::async_runtime::block_on(run_sync_blocking::<(), _>("test sync", || {
            Err(AppError::Message("boom".into()))
        }))
        .unwrap_err();
        assert!(err.to_string().contains("boom"));
    }

    #[test]
    fn test_insert_claude_session_persists_semantics_and_pricing_model() -> Result<(), AppError> {
        let db = Database::memory()?;
        let msg = ParsedAssistantUsage {
            message_id: "msg_sem".to_string(),
            model: "claude-sonnet-4-5".to_string(),
            input_tokens: 100,
            output_tokens: 20,
            cache_read_tokens: 10,
            cache_creation_tokens: 5,
            stop_reason: Some("end_turn".to_string()),
            timestamp: Some("1970-01-01T00:16:45Z".to_string()),
            session_id: Some("session-1".to_string()),
        };
        assert!(insert_session_log_entry(
            &db.conn.lock().unwrap(),
            "session:msg_sem",
            &msg,
            SyncMode::Incremental
        )?);

        let conn = lock_conn!(db.conn);
        let (pricing_model, semantics): (Option<String>, i64) = conn.query_row(
            "SELECT pricing_model, input_token_semantics FROM proxy_request_logs
             WHERE request_id = 'session:msg_sem'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        assert_eq!(pricing_model.as_deref(), Some("claude-sonnet-4-5"));
        assert_eq!(
            semantics,
            crate::services::sql_helpers::INPUT_TOKEN_SEMANTICS_FRESH
        );
        Ok(())
    }

    /// 重扫测试用：往 proxy_request_logs 写一条 token 偏少的旧会话行
    fn insert_undercounted_claude_row(
        db: &Database,
        request_id: &str,
        output_tokens: u32,
        cache_creation_tokens: u32,
        created_at: i64,
    ) -> Result<(), AppError> {
        let conn = lock_conn!(db.conn);
        usage_repo::insert_session_row(
            &conn,
            &SessionUsageRow {
                request_id,
                source: UsageSource::ClaudeSession,
                model: "claude-sonnet-4-5",
                pricing_model: "claude-sonnet-4-5",
                session_id: Some("session-rescan"),
                input_tokens: 10,
                output_tokens,
                cache_read_tokens: 100,
                cache_creation_tokens,
                costs: UsageCosts::zero(),
                created_at,
            },
        )?;
        Ok(())
    }

    #[test]
    fn test_rescan_repairs_undercounted_row_without_duplicates() -> Result<(), AppError> {
        // 回归：P1 修复前导入的行 token 偏少（这里漏记了 cache_creation），游标已推进
        // 到文件末尾，增量同步无法修复；即使把游标清零，增量路径也只在 output 变大时
        // 升级旧行。重扫必须按 request_id 修正该行、补上漏导入的 message，且不产生重复行。
        let db = Database::memory()?;
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("rescan.jsonl");
        let path_str = file.to_string_lossy().to_string();
        let final_entry = claude_session_line(
            "msg_under",
            "claude-sonnet-4-5",
            Some("end_turn"),
            [10, 500, 100, 50],
            "2026-06-07T13:01:25Z",
            "session-rescan",
        );
        let missed = claude_session_line(
            "msg_missed",
            "claude-sonnet-4-5",
            None,
            [3, 1, 4000, 20],
            "2026-06-07T13:01:26Z",
            "session-rescan",
        );
        fs::write(&file, format!("{final_entry}\n{missed}\n")).unwrap();

        let created_at = chrono::DateTime::parse_from_rfc3339("2026-06-07T13:01:25Z")
            .unwrap()
            .timestamp();
        insert_undercounted_claude_row(&db, "session:msg_under", 500, 0, created_at)?;
        let mtime = metadata_modified_nanos(&fs::metadata(&file).unwrap());
        update_sync_state(&db, &path_str, mtime, 2)?;

        // 增量同步：游标已在末尾，什么都不做
        assert_eq!(sync_single_file(&db, &file, SyncMode::Incremental)?, (0, 0));

        let read_row = |db: &Database| -> Result<(i64, i64, String, i64), AppError> {
            let conn = lock_conn!(db.conn);
            Ok(conn.query_row(
                "SELECT output_tokens, cache_creation_tokens, total_cost_usd,
                        (SELECT COUNT(*) FROM proxy_request_logs WHERE request_id = 'session:msg_under')
                 FROM proxy_request_logs WHERE request_id = 'session:msg_under'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )?)
        };
        assert_eq!(read_row(&db)?.1, 0, "增量同步不应改动旧行");

        // 仅清零游标的增量重导：补上漏导入的 message，但 output 未变的旧行保持低估
        update_sync_state(&db, &path_str, 0, 0)?;
        assert_eq!(sync_single_file(&db, &file, SyncMode::Incremental)?, (1, 1));
        assert_eq!(read_row(&db)?.1, 0, "增量路径无法修复 cache 低估");

        let rescan = SyncMode::Rescan {
            rollup_horizon: None,
        };
        let (imported, _) = sync_single_file(&db, &file, rescan)?;
        assert_eq!(imported, 1, "只修复低估的那一行");

        let (output, cache_creation, total_cost, count) = read_row(&db)?;
        assert_eq!(output, 500);
        assert_eq!(cache_creation, 50);
        assert_eq!(count, 1, "重扫不能产生重复行");
        let expected = {
            let conn = lock_conn!(db.conn);
            let pricing = find_model_pricing_for_session(&conn, "claude-sonnet-4-5")
                .expect("seeded pricing for claude-sonnet-4-5");
            let usage = TokenUsage {
                input_tokens: 10,
                output_tokens: 500,
                cache_read_tokens: 100,
                cache_creation_tokens: 50,
                model: Some("claude-sonnet-4-5".to_string()),
                message_id: None,
            };
            CostCalculator::calculate(&usage, &pricing, Decimal::from(1))
        };
        assert_eq!(
            total_cost,
            expected.total_cost.to_string(),
            "费用按修复后的 token 重算"
        );

        let total_rows = |db: &Database| -> Result<i64, AppError> {
            let conn = lock_conn!(db.conn);
            Ok(conn.query_row(
                "SELECT COUNT(*) FROM proxy_request_logs WHERE data_source = 'session_log'",
                [],
                |row| row.get(0),
            )?)
        };
        assert_eq!(total_rows(&db)?, 2);
        assert_eq!(
            get_sync_state(&db, &path_str)?.1,
            2,
            "游标重写为本次扫描位置"
        );

        // 再次重扫是幂等的：没有新写入，也没有重复行
        assert_eq!(sync_single_file(&db, &file, rescan)?.0, 0);
        assert_eq!(total_rows(&db)?, 2);
        assert_eq!(read_row(&db)?.0, 500);
        Ok(())
    }

    #[test]
    fn test_rescan_never_downgrades_and_skips_rolled_up_period() -> Result<(), AppError> {
        let db = Database::memory()?;
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("rolled.jsonl");
        // msg_old 早于日汇总边界、明细已被裁剪；msg_big 已有的行比文件里的值更大
        let old = r#"{"type":"assistant","message":{"id":"msg_old","model":"claude-sonnet-4-5","stop_reason":"end_turn","usage":{"input_tokens":10,"output_tokens":5}},"timestamp":"2026-01-01T12:00:00Z","sessionId":"s"}"#;
        let big = r#"{"type":"assistant","message":{"id":"msg_big","model":"claude-sonnet-4-5","stop_reason":"end_turn","usage":{"input_tokens":10,"output_tokens":5,"cache_read_input_tokens":100}},"timestamp":"2026-06-07T12:00:00Z","sessionId":"s"}"#;
        fs::write(&file, format!("{old}\n{big}\n")).unwrap();
        let big_at = chrono::DateTime::parse_from_rfc3339("2026-06-07T12:00:00Z")
            .unwrap()
            .timestamp();
        insert_undercounted_claude_row(&db, "session:msg_big", 900, 0, big_at)?;
        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO usage_daily_rollups (date, app_type, provider_id, model, request_count)
                 VALUES ('2026-03-01', 'claude', '_session', 'claude-sonnet-4-5', 1)",
                [],
            )?;
        }
        let horizon = {
            let conn = lock_conn!(db.conn);
            usage_repo::rollup_horizon(&conn)?
        };
        let expected_horizon = crate::database::local_midnight_timestamp(
            chrono::NaiveDate::from_ymd_opt(2026, 3, 2).unwrap(),
        )?;
        assert_eq!(horizon, Some(expected_horizon));

        let (imported, _) = sync_single_file(
            &db,
            &file,
            SyncMode::Rescan {
                rollup_horizon: horizon,
            },
        )?;
        assert_eq!(imported, 0);

        let conn = lock_conn!(db.conn);
        let old_exists: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM proxy_request_logs WHERE request_id = 'session:msg_old')",
            [],
            |row| row.get(0),
        )?;
        assert!(!old_exists, "已归档时段不能重新插入明细，否则会重复计数");
        let big_output: i64 = conn.query_row(
            "SELECT output_tokens FROM proxy_request_logs WHERE request_id = 'session:msg_big'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(big_output, 900, "重扫只升不降");
        Ok(())
    }

    #[test]
    fn test_rescan_sources_reports_progress_and_rejects_concurrent_runs() -> Result<(), AppError> {
        fn ok_source(_db: &Database, mode: SyncMode) -> Result<SessionSyncResult, AppError> {
            assert!(mode.is_rescan());
            Ok(SessionSyncResult {
                imported: 2,
                skipped: 1,
                files_scanned: 3,
                errors: vec![],
            })
        }
        fn nested_source(db: &Database, _mode: SyncMode) -> Result<SessionSyncResult, AppError> {
            // 重扫进行中再次发起应被拒绝
            let err =
                rescan_sources(db, &[("Nested", ok_source as SessionSyncFn)], |_| {}).unwrap_err();
            Err(err)
        }

        let db = Database::memory()?;
        let mut progress = Vec::new();
        let result = rescan_sources(
            &db,
            &[
                ("A", ok_source as SessionSyncFn),
                ("B", nested_source as SessionSyncFn),
                ("C", ok_source as SessionSyncFn),
            ],
            |p| progress.push(p),
        )?;
        assert_eq!(result.imported, 4);
        assert_eq!(result.files_scanned, 6);
        assert_eq!(result.errors.len(), 1, "单个来源失败只记录错误");
        assert!(result.errors[0].contains("正在进行中"));

        let summary: Vec<_> = progress
            .iter()
            .map(|p| (p.source.as_str(), p.completed, p.total, p.imported, p.done))
            .collect();
        assert_eq!(
            summary,
            vec![
                ("A", 1, 3, 2, false),
                ("B", 2, 3, 2, false),
                ("C", 3, 3, 4, true)
            ]
        );

        // 完成后可以再次重扫
        rescan_sources(&db, &[("A", ok_source as SessionSyncFn)], |_| {})?;
        Ok(())
    }

    #[test]
    fn test_data_source_breakdown_reports_quality() -> Result<(), AppError> {
        let db = Database::memory()?;
        insert_undercounted_claude_row(&db, "session:q1", 0, 0, 1_000)?;
        insert_undercounted_claude_row(&db, "session:q2", 20, 0, 2_000)?;
        // q3 与一条代理日志指纹相同：只计入 duplicate_count，不参与其它质量统计与时间范围
        insert_undercounted_claude_row(&db, "session:q3", 30, 0, 500)?;
        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "UPDATE proxy_request_logs SET total_cost_usd = '0.5', session_id = NULL
                 WHERE request_id = 'session:q2'",
                [],
            )?;
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model, request_model,
                    input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                    input_cost_usd, output_cost_usd, cache_read_cost_usd, cache_creation_cost_usd,
                    total_cost_usd, latency_ms, status_code, created_at, data_source
                ) VALUES ('proxy-q3', 'p', 'claude', 'claude-sonnet-4-5', 'claude-sonnet-4-5',
                          10, 30, 100, 0, '0', '0', '0', '0', '0.2', 100, 200, 500, 'proxy')",
                [],
            )?;
        }

        let breakdown = get_data_source_breakdown(&db)?;
        let claude = breakdown
            .iter()
            .find(|s| s.data_source == "session_log")
            .expect("session_log source");
        assert_eq!(claude.request_count, 2);
        assert_eq!(claude.total_cost_usd, "0.500000");
        assert_eq!(claude.duplicate_count, 1);
        assert_eq!(claude.unpriced_count, 1);
        assert_eq!(claude.missing_session_count, 1);
        assert_eq!(claude.zero_output_count, 1);
        assert_eq!(claude.first_created_at, Some(1_000));
        assert_eq!(claude.last_created_at, Some(2_000));
        let proxy = breakdown
            .iter()
            .find(|s| s.data_source == "proxy")
            .expect("proxy source");
        assert_eq!((proxy.request_count, proxy.duplicate_count), (1, 0));
        Ok(())
    }
}
