//! Codex 会话日志使用追踪
//!
//! 从 ~/.codex/sessions/ 下的 JSONL 会话文件中提取精确 token 使用数据，
//! 替代原有的 state_5.sqlite 估算方案。
//!
//! ## 数据流
//! ```text
//! ~/.codex/sessions/YYYY/MM/DD/*.jsonl → 增量解析 → delta 计算 → 费用计算 → proxy_request_logs 表
//! ```
//!
//! ## 解析的事件类型
//! - `session_meta` → 提取唯一 thread_id（子代理的 session_id 指向父线程）
//! - `turn_context` → 提取当前 model
//! - `event_msg` (type=token_count) → 提取累计 token 用量，计算 delta

use crate::codex_config::get_codex_config_dir;
use crate::database::Database;
use crate::error::AppError;
use crate::services::session_usage::ingest::{
    commit_session_rows, created_at_from_rfc3339, load_sync_cursors, prune_missing_cursors,
    CursorUpdate, PricingCache, SessionSource, SessionUsageRow, SyncCursors, WritePolicy,
};
use crate::services::session_usage::{
    for_each_complete_line, metadata_modified_nanos, update_sync_state, SessionSyncResult,
};
use std::collections::HashSet;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

/// Codex 的 token_count 是增量事件，一经写入不再变化。
const CODEX_SESSION_SOURCE: SessionSource = SessionSource {
    app_type: "codex",
    provider_id: "_codex_session",
    data_source: "codex_session",
    policy: WritePolicy::InsertOnly,
};

const CODEX_THREAD_REQUEST_ID_PREFIX: &str = "codex_session:thread-v1";

/// 累计 token 用量（跟踪 total_token_usage 字段）
#[derive(Debug, Clone, Default)]
struct CumulativeTokens {
    input: u64,
    cached_input: u64,
    output: u64,
}

/// 单次 API 调用的 token 增量
#[derive(Debug)]
struct DeltaTokens {
    input: u32,
    cached_input: u32,
    output: u32,
}

impl DeltaTokens {
    fn is_zero(&self) -> bool {
        self.input == 0 && self.cached_input == 0 && self.output == 0
    }
}

/// 单文件解析时的运行状态
struct FileParseState {
    thread_id: Option<String>,
    current_model: String,
    prev_total: Option<CumulativeTokens>,
    event_index: u32,
    history_replay_boundary: Option<i64>,
}

/// Codex 子代理日志中的 `id` 是当前线程的唯一 ID，`session_id` 则指向父线程。
#[derive(Debug, Clone, PartialEq, Eq)]
struct CodexSessionIdentity {
    thread_id: String,
    carries_history_snapshot: bool,
}

fn parse_codex_session_identity(payload: &serde_json::Value) -> Option<CodexSessionIdentity> {
    let thread_id = payload
        .get("id")
        .or_else(|| payload.get("thread_id"))
        .or_else(|| payload.get("threadId"))
        .or_else(|| payload.get("session_id"))
        .or_else(|| payload.get("sessionId"))
        .and_then(|value| value.as_str())?
        .to_string();
    let session_id = payload
        .get("session_id")
        .or_else(|| payload.get("sessionId"))
        .and_then(|value| value.as_str());
    let carries_history_snapshot = payload
        .get("forked_from_id")
        .and_then(|value| value.as_str())
        .is_some_and(|value| !value.is_empty())
        || payload
            .get("source")
            .and_then(|source| source.get("subagent"))
            .is_some()
        || session_id.is_some_and(|session_id| session_id != thread_id);

    Some(CodexSessionIdentity {
        thread_id,
        carries_history_snapshot,
    })
}

fn read_codex_session_identity(file_path: &Path) -> Option<CodexSessionIdentity> {
    let file = fs::File::open(file_path).ok()?;

    for line in BufReader::new(file).lines() {
        let Ok(line) = line else {
            continue;
        };
        if !line.contains("\"session_meta\"") {
            continue;
        }
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        if value.get("type").and_then(|value| value.as_str()) != Some("session_meta") {
            continue;
        }
        if let Some(identity) = value.get("payload").and_then(parse_codex_session_identity) {
            return Some(identity);
        }
    }

    None
}

/// fork/子代理日志会先重放父线程历史，再以接管事件开始当前线程。
/// 返回接管事件所在行；此前的 token_count 只用于恢复累计值基线。
fn codex_history_replay_boundary(
    file_path: &Path,
    identity: Option<&CodexSessionIdentity>,
) -> Option<i64> {
    if !identity.is_some_and(|identity| identity.carries_history_snapshot) {
        return None;
    }

    let file = fs::File::open(file_path).ok()?;
    for (index, line) in BufReader::new(file).lines().enumerate() {
        let Ok(line) = line else {
            continue;
        };
        if !line.contains("\"thread_settings_applied\"")
            && !line.contains("\"inter_agent_communication")
        {
            continue;
        }
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        let Some(event_type) = value.get("type").and_then(|value| value.as_str()) else {
            continue;
        };
        let is_replay_boundary = event_type.starts_with("inter_agent_communication")
            || (event_type == "event_msg"
                && value
                    .get("payload")
                    .and_then(|payload| payload.get("type"))
                    .and_then(|value| value.as_str())
                    == Some("thread_settings_applied"));
        if is_replay_boundary {
            return Some(index as i64 + 1);
        }
    }

    None
}

fn is_history_snapshot_event(state: &FileParseState, line_offset: i64) -> bool {
    state
        .history_replay_boundary
        .is_some_and(|boundary| line_offset < boundary)
}

/// 读取 Codex 文件的同步游标；归档文件首次出现时继承原 sessions/ 路径的游标。
///
/// 继承查找在本轮预加载的游标表中完成（不再对 session_log_sync 做后缀全表扫描），
/// 继承结果立即持久化，原路径的游标随后可被安全清理。
fn get_codex_sync_state(
    db: &Database,
    file_path: &Path,
    cursors: &SyncCursors,
) -> Result<(i64, i64), AppError> {
    let file_path_str = file_path.to_string_lossy().to_string();
    let state = cursors.get(&file_path_str).copied().unwrap_or((0, 0));
    if state != (0, 0)
        || file_path
            .parent()
            .and_then(Path::file_name)
            .and_then(|name| name.to_str())
            != Some("archived_sessions")
    {
        return Ok(state);
    }

    let Some(file_name) = file_path.file_name().and_then(|name| name.to_str()) else {
        return Ok(state);
    };
    let slash_suffix = format!("/{file_name}");
    let backslash_suffix = format!("\\{file_name}");
    let inherited = cursors
        .iter()
        .filter(|(key, _)| {
            key.as_str() != file_path_str
                && (key.ends_with(&slash_suffix) || key.ends_with(&backslash_suffix))
        })
        .map(|(_, (modified, offset))| (*offset, *modified))
        .max()
        .map(|(offset, modified)| (modified, offset));

    match inherited {
        Some(inherited) => {
            update_sync_state(db, &file_path_str, inherited.0, inherited.1)?;
            Ok(inherited)
        }
        None => Ok(state),
    }
}

/// 归一化 Codex 模型名
///
/// 处理规则（按顺序）：
/// 1. 转小写：`GLM-4.6` → `glm-4.6`
/// 2. 剥离 provider 前缀：`openai/gpt-5.4` → `gpt-5.4`
/// 3. 剥离 ISO 日期后缀：`gpt-5.4-2026-03-05` → `gpt-5.4`
/// 4. 剥离紧凑日期后缀：`gpt-5.4-20260305` → `gpt-5.4`
fn normalize_codex_model(raw: &str) -> String {
    // Step 1: 小写
    let mut name = raw.to_lowercase();

    // Step 2: 剥离 "provider/" 前缀（如 openai/, azure/）
    if let Some(pos) = name.rfind('/') {
        name = name[pos + 1..].to_string();
    }

    // Step 3: 剥离 ISO 日期后缀 -YYYY-MM-DD（正好 11 字符）
    if name.len() > 11 && name.is_char_boundary(name.len() - 11) {
        let suffix = &name[name.len() - 11..];
        if suffix.is_ascii()
            && suffix.as_bytes()[0] == b'-'
            && suffix[1..5].chars().all(|c| c.is_ascii_digit())
            && suffix.as_bytes()[5] == b'-'
            && suffix[6..8].chars().all(|c| c.is_ascii_digit())
            && suffix.as_bytes()[8] == b'-'
            && suffix[9..11].chars().all(|c| c.is_ascii_digit())
        {
            name.truncate(name.len() - 11);
        }
    }

    // Step 4: 剥离紧凑日期后缀 -YYYYMMDD（正好 9 字符）
    if name.len() > 9 {
        let parts: Vec<&str> = name.rsplitn(2, '-').collect();
        if parts.len() == 2 {
            if let Some(suffix) = parts.first() {
                if suffix.len() == 8 && suffix.chars().all(|c| c.is_ascii_digit()) {
                    name = parts[1].to_string();
                }
            }
        }
    }

    name
}

/// 计算两次累计值之间的 delta
fn compute_delta(prev: &Option<CumulativeTokens>, current: &CumulativeTokens) -> DeltaTokens {
    match prev {
        None => DeltaTokens {
            input: current.input as u32,
            cached_input: current.cached_input as u32,
            output: current.output as u32,
        },
        Some(p) => DeltaTokens {
            input: current.input.saturating_sub(p.input) as u32,
            cached_input: current.cached_input.saturating_sub(p.cached_input) as u32,
            output: current.output.saturating_sub(p.output) as u32,
        },
    }
}

/// 从 JSON Value 中提取累计 token 用量
fn parse_cumulative_tokens(total_usage: &serde_json::Value) -> Option<CumulativeTokens> {
    if total_usage.is_null() || !total_usage.is_object() {
        return None;
    }
    Some(CumulativeTokens {
        input: total_usage
            .get("input_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
        cached_input: total_usage
            .get("cached_input_tokens")
            .or_else(|| total_usage.get("cache_read_input_tokens"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
        output: total_usage
            .get("output_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
    })
}

/// 同步 Codex 使用数据（从 JSONL 会话日志）
pub fn sync_codex_usage(db: &Database) -> Result<SessionSyncResult, AppError> {
    sync_codex_dir(db, &get_codex_config_dir())
}

fn sync_codex_dir(db: &Database, codex_dir: &Path) -> Result<SessionSyncResult, AppError> {
    let files = collect_codex_session_files(codex_dir);

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
        discovered.insert(file_path.to_string_lossy().to_string());
        match sync_codex_file(db, file_path, &cursors, &mut pricing) {
            Ok((imported, skipped)) => {
                result.imported += imported;
                result.skipped += skipped;
            }
            Err(e) => {
                let msg = format!("Codex 会话文件解析失败 {}: {e}", file_path.display());
                log::warn!("[CODEX-SYNC] {msg}");
                result.errors.push(msg);
            }
        }
    }

    for root in [
        codex_dir.join("sessions"),
        codex_dir.join("archived_sessions"),
    ] {
        if let Err(e) = prune_missing_cursors(db, &root, &cursors, &discovered) {
            log::warn!("[CODEX-SYNC] 清理失效同步状态失败: {e}");
        }
    }

    if result.imported > 0 {
        log::info!(
            "[CODEX-SYNC] 同步完成: 导入 {} 条, 跳过 {} 条, 扫描 {} 个文件",
            result.imported,
            result.skipped,
            result.files_scanned
        );
    }

    Ok(result)
}

/// 收集所有 Codex 会话 JSONL 文件
fn collect_codex_session_files(codex_dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();

    // 1. 扫描 sessions/YYYY/MM/DD/*.jsonl（日期分区目录）
    let sessions_dir = codex_dir.join("sessions");
    if sessions_dir.is_dir() {
        collect_jsonl_recursive(&sessions_dir, &mut files, 0, 3);
    }

    // 2. 扫描 archived_sessions/*.jsonl（扁平归档目录）
    let archived_dir = codex_dir.join("archived_sessions");
    if archived_dir.is_dir() {
        if let Ok(entries) = fs::read_dir(&archived_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
                    files.push(path);
                }
            }
        }
    }

    files
}

/// 递归扫描目录下的 .jsonl 文件（限制最大深度）
fn collect_jsonl_recursive(dir: &Path, files: &mut Vec<PathBuf>, depth: u32, max_depth: u32) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() && depth < max_depth {
            collect_jsonl_recursive(&path, files, depth + 1, max_depth);
        } else if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
            files.push(path);
        }
    }
}

/// 同步单个 Codex JSONL 文件，返回 (imported, skipped)
fn sync_codex_file(
    db: &Database,
    file_path: &Path,
    cursors: &SyncCursors,
    pricing: &mut PricingCache,
) -> Result<(u32, u32), AppError> {
    let file_path_str = file_path.to_string_lossy().to_string();

    // 获取文件元数据
    let metadata = fs::metadata(file_path)
        .map_err(|e| AppError::Config(format!("无法读取文件元数据: {e}")))?;
    let file_modified = metadata_modified_nanos(&metadata);

    // 检查同步状态
    let (last_modified, last_offset) = get_codex_sync_state(db, file_path, cursors)?;

    // 文件未变化则跳过
    if file_modified <= last_modified {
        return Ok((0, 0));
    }

    let identity = read_codex_session_identity(file_path);
    let history_replay_boundary = codex_history_replay_boundary(file_path, identity.as_ref());

    let mut state = FileParseState {
        thread_id: identity.map(|identity| identity.thread_id),
        current_model: "unknown".to_string(),
        prev_total: None,
        event_index: 0,
        history_replay_boundary,
    };
    // SESS-42：缺少 session_meta 时用文件名派生稳定的线程键，避免不同文件
    // 共用 `unknown` 前缀而互相吞掉第 N 个事件。
    let fallback_thread_id = codex_fallback_thread_id(file_path);

    let mut rows = Vec::new();
    let mut replay_skipped: u32 = 0;

    // 逐行解析（只读取以换行结尾的完整行，末尾半行留到下次）
    let line_offset = for_each_complete_line(file_path, |line_offset, line| {
        process_codex_line(
            &mut state,
            line,
            line_offset,
            last_offset,
            &fallback_thread_id,
            &mut rows,
            &mut replay_skipped,
        );
    })?;

    if state.thread_id.is_none() && !rows.is_empty() {
        log::warn!(
            "[CODEX-SYNC] {} 缺少 session_meta，使用文件名作为线程标识",
            file_path.display()
        );
    }

    let (imported, skipped) = commit_session_rows(
        db,
        &CODEX_SESSION_SOURCE,
        &rows,
        pricing,
        Some(CursorUpdate {
            key: &file_path_str,
            last_modified: file_modified,
            last_offset: line_offset,
            byte: None,
        }),
    )?;
    Ok((imported, skipped + replay_skipped))
}

fn codex_fallback_thread_id(file_path: &Path) -> String {
    let stem = file_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("unknown");
    format!("file-{stem}")
}

/// 处理一行 Codex 事件；新的非零 token delta 追加到 `rows`。
fn process_codex_line(
    state: &mut FileParseState,
    line: &str,
    line_offset: i64,
    last_offset: i64,
    fallback_thread_id: &str,
    rows: &mut Vec<SessionUsageRow>,
    replay_skipped: &mut u32,
) {
    if line.trim().is_empty() {
        return;
    }

    // 快速过滤：在 JSON 反序列化前跳过无关行
    let is_event_msg = line.contains("\"event_msg\"");
    let is_turn_context = line.contains("\"turn_context\"");
    let is_session_meta = line.contains("\"session_meta\"");

    if !is_event_msg && !is_turn_context && !is_session_meta {
        return;
    }
    if is_event_msg && !line.contains("\"token_count\"") {
        return;
    }

    let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
        return;
    };
    let Some(event_type) = value.get("type").and_then(|t| t.as_str()) else {
        return;
    };

    match event_type {
        "session_meta" if state.thread_id.is_none() => {
            state.thread_id = value
                .get("payload")
                .and_then(parse_codex_session_identity)
                .map(|identity| identity.thread_id);
        }
        "turn_context" => {
            if let Some(payload) = value.get("payload") {
                // model 可能在 payload.model 或 payload.info.model
                if let Some(model) = payload
                    .get("model")
                    .or_else(|| payload.get("info").and_then(|info| info.get("model")))
                    .and_then(|v| v.as_str())
                {
                    state.current_model = normalize_codex_model(model);
                }
            }
        }
        "event_msg" => {
            let Some(payload) = value.get("payload") else {
                return;
            };

            // 只处理 token_count 类型
            if payload.get("type").and_then(|t| t.as_str()) != Some("token_count") {
                return;
            }

            // 跳过 info 为 null 的首个事件
            let Some(info) = payload.get("info").filter(|i| !i.is_null()) else {
                return;
            };

            // 提取模型（token_count 事件也可能携带 model）
            if let Some(model) = info
                .get("model")
                .or_else(|| info.get("model_name"))
                .or_else(|| payload.get("model"))
                .and_then(|v| v.as_str())
            {
                state.current_model = normalize_codex_model(model);
            }

            // 优先用 total_token_usage（累计值），fallback 到 last_token_usage（增量值）
            let (cumulative, is_total) = if let Some(total) = info.get("total_token_usage") {
                (parse_cumulative_tokens(total), true)
            } else if let Some(last) = info.get("last_token_usage") {
                (parse_cumulative_tokens(last), false)
            } else {
                return;
            };

            let Some(cumulative) = cumulative else {
                return;
            };

            let delta = if is_total {
                // 累计值模式：计算与上次的 delta
                let d = compute_delta(&state.prev_total, &cumulative);
                state.prev_total = Some(cumulative);
                d
            } else {
                // 增量值模式：直接使用 last_token_usage 的值
                DeltaTokens {
                    input: cumulative.input as u32,
                    cached_input: cumulative.cached_input as u32,
                    output: cumulative.output as u32,
                }
            };

            // 钳制：cached 不应超过 input（防护异常数据）
            let delta = DeltaTokens {
                cached_input: delta.cached_input.min(delta.input),
                ..delta
            };

            if delta.is_zero() {
                return; // 跳过 task 边界的零 delta 事件
            }

            // 所有非零事件都占据稳定序号，包括已同步事件与 replay 快照。
            state.event_index += 1;

            // replay 快照更新了 prev_total，但不是当前线程的新用量。
            if is_history_snapshot_event(state, line_offset) {
                if line_offset > last_offset {
                    *replay_skipped += 1;
                }
                return;
            }

            // 跳过已处理的行（但仍需解析以恢复状态）
            if line_offset <= last_offset {
                return;
            }

            let timestamp = value.get("timestamp").and_then(|v| v.as_str());
            rows.push(codex_row(state, fallback_thread_id, &delta, timestamp));
        }
        _ => {}
    }
}

fn codex_row(
    state: &FileParseState,
    fallback_thread_id: &str,
    delta: &DeltaTokens,
    timestamp: Option<&str>,
) -> SessionUsageRow {
    let thread_id = state.thread_id.as_deref().unwrap_or(fallback_thread_id);
    SessionUsageRow {
        request_id: format!(
            "{CODEX_THREAD_REQUEST_ID_PREFIX}:{thread_id}:{}",
            state.event_index
        ),
        model: state.current_model.clone(),
        pricing_model: normalize_codex_model(&state.current_model),
        input_tokens: delta.input,
        output_tokens: delta.output,
        cache_read_tokens: delta.cached_input,
        cache_creation_tokens: 0, // Codex 日志无此数据
        session_id: state.thread_id.clone(),
        created_at: created_at_from_rfc3339(timestamp),
        precomputed_total_cost: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::lock_conn;
    use crate::services::session_usage::get_sync_state;
    use tempfile::tempdir;

    /// 以当前持久化游标同步单个文件（测试辅助）。
    fn sync_single_codex_file(db: &Database, file_path: &Path) -> Result<(u32, u32), AppError> {
        let cursors = load_sync_cursors(db)?;
        sync_codex_file(db, file_path, &cursors, &mut PricingCache::default())
    }

    #[test]
    fn test_files_without_session_meta_do_not_collide() -> Result<(), AppError> {
        // SESS-42：两个缺少 session_meta 的文件各有 3 个事件，应导入 6 行。
        let db = Database::memory()?;
        let temp = tempdir().unwrap();
        for name in ["rollout-a.jsonl", "rollout-b.jsonl"] {
            write_jsonl(
                &temp.path().join(name),
                &[
                    turn_context(),
                    token_count(100, 10, 5),
                    token_count(200, 20, 10),
                    token_count(300, 30, 15),
                ],
            );
        }
        assert_eq!(
            sync_single_codex_file(&db, &temp.path().join("rollout-a.jsonl"))?,
            (3, 0)
        );
        assert_eq!(
            sync_single_codex_file(&db, &temp.path().join("rollout-b.jsonl"))?,
            (3, 0)
        );
        Ok(())
    }

    #[test]
    fn test_truncated_last_event_is_imported_after_completion() -> Result<(), AppError> {
        // SESS-36：末行未写完时游标停在上一完整行，补全后导入该事件。
        let db = Database::memory()?;
        let temp = tempdir().unwrap();
        let file = temp.path().join("rollout-t.jsonl");
        let last = token_count(200, 20, 10).to_string();
        let (head, tail) = last.split_at(last.len() / 2);
        fs::write(
            &file,
            format!(
                "{}\n{}\n{}\n{head}",
                session_meta("t", "t"),
                turn_context(),
                token_count(100, 10, 5)
            ),
        )
        .unwrap();
        assert_eq!(sync_single_codex_file(&db, &file)?, (1, 0));
        assert_eq!(get_sync_state(&db, &file.to_string_lossy())?.1, 3);

        let mut contents = fs::read_to_string(&file).unwrap();
        contents.push_str(tail);
        contents.push('\n');
        fs::write(&file, contents).unwrap();
        update_sync_state(&db, &file.to_string_lossy(), 0, 3)?; // force re-read regardless of mtime granularity
        assert_eq!(sync_single_codex_file(&db, &file)?, (1, 0));
        Ok(())
    }

    fn write_jsonl(path: &Path, values: &[serde_json::Value]) {
        let contents = values
            .iter()
            .map(serde_json::Value::to_string)
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        fs::write(path, contents).unwrap();
    }

    fn session_meta(thread_id: &str, session_id: &str) -> serde_json::Value {
        serde_json::json!({
            "timestamp": "2026-07-10T03:00:00Z",
            "type": "session_meta",
            "payload": {
                "id": thread_id,
                "session_id": session_id,
                "source": if thread_id == session_id {
                    serde_json::Value::String("cli".to_string())
                } else {
                    serde_json::json!({ "subagent": {} })
                }
            }
        })
    }

    fn turn_context() -> serde_json::Value {
        serde_json::json!({
            "timestamp": "2026-07-10T03:00:01Z",
            "type": "turn_context",
            "payload": { "model": "gpt-5.6-sol" }
        })
    }

    fn token_count(input: u64, cached: u64, output: u64) -> serde_json::Value {
        serde_json::json!({
            "timestamp": "2026-07-10T03:00:02Z",
            "type": "event_msg",
            "payload": {
                "type": "token_count",
                "info": { "total_token_usage": {
                    "input_tokens": input,
                    "cached_input_tokens": cached,
                    "output_tokens": output
                }}
            }
        })
    }

    #[test]
    fn test_delta_first_event() {
        let prev = None;
        let current = CumulativeTokens {
            input: 17934,
            cached_input: 9600,
            output: 454,
        };
        let delta = compute_delta(&prev, &current);
        assert_eq!(delta.input, 17934);
        assert_eq!(delta.cached_input, 9600);
        assert_eq!(delta.output, 454);
        assert!(!delta.is_zero());
    }

    #[test]
    fn test_delta_subsequent_event() {
        let prev = Some(CumulativeTokens {
            input: 17934,
            cached_input: 9600,
            output: 454,
        });
        let current = CumulativeTokens {
            input: 36722,
            cached_input: 27904,
            output: 804,
        };
        let delta = compute_delta(&prev, &current);
        assert_eq!(delta.input, 36722 - 17934);
        assert_eq!(delta.cached_input, 27904 - 9600);
        assert_eq!(delta.output, 804 - 454);
    }

    #[test]
    fn test_delta_zero_at_task_boundary() {
        let prev = Some(CumulativeTokens {
            input: 58346,
            cached_input: 46976,
            output: 1045,
        });
        // task 边界：相同的累计值
        let current = CumulativeTokens {
            input: 58346,
            cached_input: 46976,
            output: 1045,
        };
        let delta = compute_delta(&prev, &current);
        assert!(delta.is_zero());
    }

    #[test]
    fn test_delta_saturating_sub() {
        // 异常情况：当前值小于前值（不应发生，但需防护）
        let prev = Some(CumulativeTokens {
            input: 100,
            cached_input: 50,
            output: 30,
        });
        let current = CumulativeTokens {
            input: 80,
            cached_input: 40,
            output: 20,
        };
        let delta = compute_delta(&prev, &current);
        assert_eq!(delta.input, 0);
        assert_eq!(delta.cached_input, 0);
        assert_eq!(delta.output, 0);
        assert!(delta.is_zero());
    }

    #[test]
    fn test_parse_cumulative_tokens_valid() {
        let json: serde_json::Value = serde_json::json!({
            "input_tokens": 17934,
            "cached_input_tokens": 9600,
            "output_tokens": 454,
            "reasoning_output_tokens": 233,
            "total_tokens": 18388
        });
        let tokens = parse_cumulative_tokens(&json).unwrap();
        assert_eq!(tokens.input, 17934);
        assert_eq!(tokens.cached_input, 9600);
        assert_eq!(tokens.output, 454);
    }

    #[test]
    fn test_parse_cumulative_tokens_null() {
        let json = serde_json::Value::Null;
        assert!(parse_cumulative_tokens(&json).is_none());
    }

    #[test]
    fn test_parse_cumulative_tokens_alt_field_names() {
        // 某些版本可能使用 cache_read_input_tokens 而非 cached_input_tokens
        let json: serde_json::Value = serde_json::json!({
            "input_tokens": 1000,
            "cache_read_input_tokens": 500,
            "output_tokens": 200
        });
        let tokens = parse_cumulative_tokens(&json).unwrap();
        assert_eq!(tokens.cached_input, 500);
    }

    #[test]
    fn test_collect_codex_session_files_nonexistent() {
        let files = collect_codex_session_files(Path::new("/nonexistent/path"));
        assert!(files.is_empty());
    }

    #[test]
    fn test_subagent_identity_prefers_unique_thread_id() {
        let identity =
            parse_codex_session_identity(session_meta("child", "parent").get("payload").unwrap())
                .unwrap();

        assert_eq!(identity.thread_id, "child");
        assert!(identity.carries_history_snapshot);
    }

    #[test]
    fn test_subagent_replay_only_establishes_token_baseline() -> Result<(), AppError> {
        let db = Database::memory()?;
        let temp = tempdir().unwrap();
        let child = temp.path().join("child.jsonl");
        write_jsonl(
            &child,
            &[
                session_meta("child", "parent"),
                turn_context(),
                token_count(1_000, 900, 100),
                token_count(1_200, 1_000, 120),
                serde_json::json!({
                    "timestamp": "2026-07-10T03:00:03Z",
                    "type": "event_msg",
                    "payload": { "type": "thread_settings_applied" }
                }),
                token_count(1_300, 1_050, 150),
            ],
        );

        assert_eq!(sync_single_codex_file(&db, &child)?, (1, 2));

        let conn = lock_conn!(db.conn);
        let usage: (i64, i64, i64) = conn.query_row(
            "SELECT input_tokens, cache_read_tokens, output_tokens
             FROM proxy_request_logs
             WHERE request_id = 'codex_session:thread-v1:child:3'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        assert_eq!(usage, (100, 50, 30));

        Ok(())
    }

    #[test]
    fn test_subagents_under_same_parent_use_distinct_request_ids() -> Result<(), AppError> {
        let db = Database::memory()?;
        let temp = tempdir().unwrap();
        let child_a = temp.path().join("child-a.jsonl");
        let child_b = temp.path().join("child-b.jsonl");
        write_jsonl(
            &child_a,
            &[
                session_meta("child-a", "parent"),
                turn_context(),
                token_count(100, 50, 10),
            ],
        );
        write_jsonl(
            &child_b,
            &[
                session_meta("child-b", "parent"),
                turn_context(),
                token_count(200, 100, 20),
            ],
        );

        assert_eq!(sync_single_codex_file(&db, &child_a)?, (1, 0));
        assert_eq!(sync_single_codex_file(&db, &child_b)?, (1, 0));

        let conn = lock_conn!(db.conn);
        let request_ids = conn
            .prepare(
                "SELECT request_id FROM proxy_request_logs
                 WHERE data_source = 'codex_session' ORDER BY request_id",
            )?
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(
            request_ids,
            vec![
                "codex_session:thread-v1:child-a:1",
                "codex_session:thread-v1:child-b:1"
            ]
        );

        Ok(())
    }

    #[test]
    fn test_archived_log_inherits_cursor_and_only_imports_appended_usage() -> Result<(), AppError> {
        let db = Database::memory()?;
        let temp = tempdir().unwrap();
        let sessions = temp.path().join("sessions");
        let archived = temp.path().join("archived_sessions");
        fs::create_dir_all(&sessions).unwrap();
        fs::create_dir_all(&archived).unwrap();
        let source = sessions.join("rollout-parent.jsonl");
        let archived_file = archived.join("rollout-parent.jsonl");
        write_jsonl(
            &archived_file,
            &[
                session_meta("parent", "parent"),
                turn_context(),
                token_count(100, 50, 10),
                token_count(200, 100, 20),
            ],
        );

        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model, request_model,
                    input_tokens, output_tokens, cache_read_tokens,
                    total_cost_usd, latency_ms, status_code, session_id,
                    created_at, data_source
                ) VALUES ('codex_session:parent:2', '_codex_session', 'codex',
                          'gpt-5.6-sol', 'gpt-5.6-sol', 999, 99, 0, '0', 0,
                          200, 'parent', 1, 'codex_session')",
                [],
            )?;
        }
        let source_path = source.to_string_lossy().to_string();
        update_sync_state(&db, &source_path, 1, 3)?;

        assert_eq!(sync_single_codex_file(&db, &archived_file)?, (1, 0));
        assert_eq!(sync_single_codex_file(&db, &archived_file)?, (0, 0));

        let conn = lock_conn!(db.conn);
        let old_row_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM proxy_request_logs
             WHERE request_id = 'codex_session:parent:2'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(old_row_count, 1);
        let usage: (i64, i64, i64) = conn.query_row(
            "SELECT input_tokens, cache_read_tokens, output_tokens
             FROM proxy_request_logs
             WHERE request_id = 'codex_session:thread-v1:parent:2'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        assert_eq!(usage, (100, 50, 10));
        drop(conn);
        assert_eq!(get_sync_state(&db, &archived_file.to_string_lossy())?.1, 4);

        Ok(())
    }

    #[test]
    fn test_insert_codex_session_skips_matching_proxy_log() -> Result<(), AppError> {
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
                    "codex-proxy",
                    "openai",
                    "codex",
                    "gpt-5.4",
                    "gpt-5.4",
                    10,
                    2,
                    1,
                    7,
                    "0.01",
                    100,
                    200,
                    1000,
                    "proxy"
                ],
            )?;
        }

        let delta = DeltaTokens {
            input: 10,
            cached_input: 1,
            output: 2,
        };
        let state = FileParseState {
            thread_id: Some("session-1".to_string()),
            current_model: "gpt-5.4".to_string(),
            prev_total: None,
            event_index: 1,
            history_replay_boundary: None,
        };
        let row = codex_row(&state, "unused", &delta, Some("1970-01-01T00:16:45Z"));
        let written = commit_session_rows(
            &db,
            &CODEX_SESSION_SOURCE,
            &[row],
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

    // ── 模型名归一化测试 ──

    #[test]
    fn test_normalize_codex_model_lowercase() {
        assert_eq!(normalize_codex_model("GLM-4.6"), "glm-4.6");
        assert_eq!(normalize_codex_model("DeepSeek-Chat"), "deepseek-chat");
        assert_eq!(normalize_codex_model("GPT-5.4"), "gpt-5.4");
    }

    #[test]
    fn test_normalize_codex_model_strip_prefix() {
        assert_eq!(normalize_codex_model("openai/gpt-5.4"), "gpt-5.4");
        assert_eq!(
            normalize_codex_model("azure/gpt-5.2-codex"),
            "gpt-5.2-codex"
        );
        assert_eq!(normalize_codex_model("OPENAI/GPT-5.4"), "gpt-5.4");
    }

    #[test]
    fn test_normalize_codex_model_strip_iso_date() {
        assert_eq!(normalize_codex_model("gpt-5.4-2026-03-05"), "gpt-5.4");
        assert_eq!(
            normalize_codex_model("gpt-5.4-pro-2026-03-05"),
            "gpt-5.4-pro"
        );
    }

    #[test]
    fn test_normalize_codex_model_strip_compact_date() {
        assert_eq!(normalize_codex_model("gpt-5.4-20260305"), "gpt-5.4");
        assert_eq!(
            normalize_codex_model("claude-opus-4-6-20260206"),
            "claude-opus-4-6"
        );
    }

    #[test]
    fn test_normalize_codex_model_no_change() {
        assert_eq!(normalize_codex_model("gpt-5.4"), "gpt-5.4");
        assert_eq!(normalize_codex_model("gpt-5.2-codex"), "gpt-5.2-codex");
        assert_eq!(normalize_codex_model("o3"), "o3");
        assert_eq!(normalize_codex_model("deepseek-chat"), "deepseek-chat");
    }

    #[test]
    fn test_normalize_codex_model_combined() {
        // prefix + uppercase + ISO date
        assert_eq!(
            normalize_codex_model("openai/GPT-5.4-2026-03-05"),
            "gpt-5.4"
        );
        // prefix + compact date
        assert_eq!(normalize_codex_model("openai/gpt-5.4-20260305"), "gpt-5.4");
    }

    #[test]
    fn test_cached_clamped_to_input() {
        // cached > input 的异常场景应被 min() 钳制
        let prev = Some(CumulativeTokens {
            input: 100,
            cached_input: 0,
            output: 50,
        });
        let current = CumulativeTokens {
            input: 110,       // delta = 10
            cached_input: 80, // delta = 80（异常：大于 input delta）
            output: 60,
        };
        let delta = compute_delta(&prev, &current);
        // 钳制前：cached_input = 80, input = 10
        assert_eq!(delta.cached_input, 80);
        assert_eq!(delta.input, 10);
        // 实际钳制在调用侧：delta.cached_input.min(delta.input)
        let clamped = delta.cached_input.min(delta.input);
        assert_eq!(clamped, 10);
    }
}
