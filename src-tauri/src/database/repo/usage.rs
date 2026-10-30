//! 使用量仓库：`proxy_request_logs` / `usage_daily_rollups` 的读写 SQL
//!
//! - Dashboard 聚合查询（汇总、趋势、Provider / 模型统计、请求日志、额度检查）；
//! - 零成本行回填；
//! - [`UsageQuery`]：请求日志筛选 + keyset 分页；
//! - [`UsageSource`]：数据来源（代理 / 各应用会话日志）及其占位 provider_id；
//! - 会话导入写入（[`insert_session_row`] 等），由调用方放在每个文件的事务里，
//!   与 [`super::session_cursor`] 的游标一起提交。

use crate::database::{local_midnight_timestamp, lock_conn, Database};
use crate::error::AppError;
use crate::pricing::engine::{self, InputBasis, ModelPricing, TokenCounts};
use crate::pricing::resolver::{
    find_model_pricing_row, is_placeholder_pricing_model, model_pricing_candidates,
    should_try_pricing_prefix_match,
};
use crate::services::sql_helpers::{fresh_input_sql, input_token_semantics_for_app_type};
use crate::services::usage_stats::{
    DailyStats, LogFilters, ModelStats, PaginatedLogs, ProviderLimitStatus, ProviderStats,
    RequestLogDetail, UnpricedModelStats, UsageSummary, UsageSummaryByApp,
};
use chrono::{Datelike, Local, NaiveDate, TimeZone, Timelike};
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::HashMap;
use std::str::FromStr;

/// Helper: compute (real_total, hit_rate) from the four token counters.
/// All inputs must already be cache-normalized (i.e. input excludes cache).
fn derive_real_total_and_hit_rate(
    fresh_input: u64,
    output: u64,
    cache_creation: u64,
    cache_read: u64,
) -> (u64, f64) {
    let real_total = fresh_input + output + cache_creation + cache_read;
    let cacheable_input = fresh_input + cache_creation + cache_read;
    let hit_rate = if cacheable_input > 0 {
        cache_read as f64 / cacheable_input as f64
    } else {
        0.0
    };
    (real_total, hit_rate)
}
/// 把 26 列的查询结果映射为 `RequestLogDetail`。
///
/// 调用方的 SELECT **必须**按以下顺序返回 26 列：
/// `request_id, provider_id, provider_name, app_type, model, request_model,
///  cost_multiplier, input_tokens, output_tokens, cache_read_tokens,
///  cache_creation_tokens, input_cost_usd, output_cost_usd, cache_read_cost_usd,
///  cache_creation_cost_usd, total_cost_usd, is_streaming, latency_ms,
///  first_token_ms, duration_ms, status_code, error_message, created_at,
///  data_source, pricing_model, input_token_semantics`
///
/// 不需要 provider_name 时（如 backfill）SELECT `NULL AS provider_name` 占位即可。
fn row_to_request_log_detail(row: &rusqlite::Row<'_>) -> rusqlite::Result<RequestLogDetail> {
    Ok(RequestLogDetail {
        request_id: row.get(0)?,
        provider_id: row.get(1)?,
        provider_name: row.get(2)?,
        app_type: row.get(3)?,
        model: row.get(4)?,
        request_model: row.get(5)?,
        cost_multiplier: row
            .get::<_, Option<String>>(6)?
            .unwrap_or_else(|| "1".to_string()),
        input_tokens: row.get::<_, i64>(7)? as u32,
        output_tokens: row.get::<_, i64>(8)? as u32,
        cache_read_tokens: row.get::<_, i64>(9)? as u32,
        cache_creation_tokens: row.get::<_, i64>(10)? as u32,
        input_cost_usd: row.get(11)?,
        output_cost_usd: row.get(12)?,
        cache_read_cost_usd: row.get(13)?,
        cache_creation_cost_usd: row.get(14)?,
        total_cost_usd: row.get(15)?,
        is_streaming: row.get::<_, i64>(16)? != 0,
        latency_ms: row.get::<_, i64>(17)? as u64,
        first_token_ms: row.get::<_, Option<i64>>(18)?.map(|v| v as u64),
        duration_ms: row.get::<_, Option<i64>>(19)?.map(|v| v as u64),
        status_code: row.get::<_, i64>(20)? as u16,
        error_message: row.get(21)?,
        created_at: row.get(22)?,
        data_source: row.get(23)?,
        pricing_model: row.get(24)?,
        input_token_semantics: row.get::<_, i64>(25)?,
    })
}

/// SQL fragment: resolve provider_name with fallback for session-based entries.
/// Session logs use placeholder provider_ids (e.g., `_session`, `_<app>_session`)
/// that don't exist in the providers table — the CASE expression below is the
/// authoritative mapping from placeholder to readable name.
fn provider_name_coalesce(log_alias: &str, provider_alias: &str) -> String {
    let arms: String = UsageSource::SESSIONS
        .iter()
        .filter_map(|source| {
            Some(format!(
                "WHEN '{}' THEN '{}' ",
                source.placeholder_provider_id()?,
                source.session_display_name()?
            ))
        })
        .collect();
    format!(
        "COALESCE({provider_alias}.name, CASE {log_alias}.provider_id \
         {arms}ELSE {log_alias}.provider_id END)"
    )
}

/// SQL 字面量列表：全部会话来源的 data_source 值，如 `'session_log', 'codex_session'`。
fn session_sources_sql_list() -> String {
    UsageSource::SESSIONS
        .iter()
        .map(|source| format!("'{}'", source.as_str()))
        .collect::<Vec<_>>()
        .join(", ")
}

pub(crate) const SESSION_PROXY_DEDUP_WINDOW_SECONDS: i64 = 10 * 60;

/// SQL 片段：把指定别名的 `data_source` 包成 COALESCE，NULL 视作 'proxy'。
///
/// 防御 schema v9 之前可能写入的 NULL data_source 行（见
/// `tests::create_legacy_nullable_logs_table`）。所有用到 data_source 的查询
/// 都应通过此 helper 生成片段，避免遗漏。
fn data_source_expr(log_alias: &str) -> String {
    format!(
        "COALESCE({log_alias}.data_source, '{}')",
        UsageSource::Proxy.as_str()
    )
}

/// SQL 标量表达式：把 Claude Desktop 网关的 `claude-desktop` app_type 在“展示口径”
/// 上折叠进 `claude`，其余 app_type 原样返回。
///
/// 背景：Desktop 网关流量在记账层按各自入口写为 `app_type='claude-desktop'`，
/// 以保留路由接管的账单审计精度（不要回退这一点）。但 Dashboard 把它当作
/// Claude Code 呈现——它本质就是跑在 Desktop 壳里的内嵌 Claude Code 运行时，
/// 且 Desktop 聊天用量永远不经过本软件，单列只会让用户误以为是“桌面版全部用量”。
///
/// 用法：把任一参与“按应用筛选/分组”的 `app_type` 列包进此表达式即可，
/// 这样 `= 'claude'` 过滤会同时命中 `claude-desktop`、`GROUP BY` 会把两者合并，
/// 而不改动任何已存储的行（详情面板仍读原始 `app_type`）。
///
/// 注意：包裹后该列上的索引在此比较中失效，但这些都是已带时间过滤的聚合扫描，
/// app_type 本就不是主访问路径，可接受。仅用于读侧；去重匹配（`has_matching_
/// proxy_usage_log`）与额度检查（`check_provider_limits`）必须保留原始精确比较。
fn folded_app_type_sql(column: &str) -> String {
    format!("CASE WHEN {column} = 'claude-desktop' THEN 'claude' ELSE {column} END")
}

/// SQL 片段：把日志/汇总行 LEFT JOIN 到 providers 表以取得供应商名称。
/// `proxy_request_logs` 与 `usage_daily_rollups` 的 (provider_id, app_type)
/// 形状相同，两者皆可作为 `log_alias`。providers 主键即 (id, app_type)，
/// 连接至多 1:1，不会放大行数。
fn providers_join(log_alias: &str, provider_alias: &str) -> String {
    format!(
        "LEFT JOIN providers {provider_alias} \
         ON {log_alias}.provider_id = {provider_alias}.id \
         AND {log_alias}.app_type = {provider_alias}.app_type"
    )
}

/// SQL 标量表达式：行的「有效计价模型」—— pricing_model 非空优先，NULL/'' 回落
/// model。这是 `get_model_stats` 的分组键，也是 Dashboard 模型筛选的匹配口径：
/// 筛选值来自模型统计列表，两边必须用同一表达式才能选得中。
fn effective_model_sql(alias: &str) -> String {
    format!("COALESCE(NULLIF({alias}.pricing_model, ''), {alias}.model)")
}

/// 把 Dashboard 顶部的 Provider/模型筛选追加到查询条件。
///
/// Provider 按展示名精确匹配（复用 [`provider_name_coalesce`]，会话占位行的
/// 可读名如 "Claude (Session)" 也能选中）；模型按 [`effective_model_sql`] 匹配。
/// 注意：传入 `provider_name` 时调用方必须把 [`providers_join`] 拼进 FROM，
/// 否则 `{provider_alias}.name` 无法解析。
fn push_provider_model_filters(
    conditions: &mut Vec<String>,
    params: &mut Vec<Box<dyn rusqlite::ToSql>>,
    log_alias: &str,
    provider_alias: &str,
    provider_name: Option<&str>,
    model: Option<&str>,
) {
    if let Some(name) = provider_name {
        conditions.push(format!(
            "{} = ?",
            provider_name_coalesce(log_alias, provider_alias)
        ));
        params.push(Box::new(name.to_string()));
    }
    if let Some(m) = model {
        conditions.push(format!("{} = ?", effective_model_sql(log_alias)));
        params.push(Box::new(m.to_string()));
    }
}

/// 未计价请求：2xx、有 token、总成本为 0，且倍率不是显式的 0（倍率 0 是
/// 用户有意免费，不算缺定价）。与前端 `isUnpricedUsage` 同口径。
fn unpriced_log_filter(log_alias: &str) -> String {
    format!(
        "({log_alias}.status_code >= 200 AND {log_alias}.status_code < 300
          AND ({log_alias}.input_tokens > 0 OR {log_alias}.output_tokens > 0
               OR {log_alias}.cache_read_tokens > 0 OR {log_alias}.cache_creation_tokens > 0)
          AND CAST({log_alias}.total_cost_usd AS REAL) = 0
          AND (CAST({log_alias}.cost_multiplier AS REAL) <> 0
               OR {log_alias}.cost_multiplier NOT GLOB '*[0-9]*'))"
    )
}

/// 补价时应填写的模型 ID：与回填查价同序，落库的计价模型优先，占位符时退回 model 列
fn unpriced_model_id(log: &RequestLogDetail) -> String {
    log.pricing_model
        .as_deref()
        .filter(|pm| !is_placeholder_pricing_model(pm))
        .unwrap_or(&log.model)
        .to_string()
}

pub(crate) fn effective_usage_log_filter(log_alias: &str) -> String {
    let data_source = data_source_expr(log_alias);
    let proxy_data_source = data_source_expr("proxy_dedup");
    let session_sources = session_sources_sql_list();
    format!(
        "NOT (
            {data_source} IN ({session_sources})
            AND EXISTS (
                SELECT 1
                FROM proxy_request_logs proxy_dedup
                WHERE {proxy_data_source} = 'proxy'
                  AND proxy_dedup.app_type = {log_alias}.app_type
                  AND proxy_dedup.status_code >= 200
                  AND proxy_dedup.status_code < 300
                  AND proxy_dedup.input_tokens = {log_alias}.input_tokens
                  AND proxy_dedup.output_tokens = {log_alias}.output_tokens
                  AND proxy_dedup.cache_read_tokens = {log_alias}.cache_read_tokens
                  AND (
                      proxy_dedup.cache_creation_tokens = {log_alias}.cache_creation_tokens
                      OR (
                          {log_alias}.cache_creation_tokens = 0
                          AND {data_source} IN ('codex_session', 'gemini_session', 'opencode_session')
                      )
                  )
                  AND proxy_dedup.created_at BETWEEN
                      {log_alias}.created_at - {SESSION_PROXY_DEDUP_WINDOW_SECONDS}
                      AND {log_alias}.created_at + {SESSION_PROXY_DEDUP_WINDOW_SECONDS}
                  AND (
                      LOWER(proxy_dedup.model) = LOWER({log_alias}.model)
                      OR LOWER(proxy_dedup.model) = 'unknown'
                      OR LOWER({log_alias}.model) = 'unknown'
                  )
            )
        )"
    )
}

/// 跨源去重指纹键。
///
/// `cache_creation_tokens`：Codex/Gemini session 日志不暴露该字段，调用方传 0
/// 表示"未知"，匹配器会放行 proxy 侧任意 cache_creation_tokens 值。
#[derive(Debug, Clone, Copy)]
pub(crate) struct DedupKey<'a> {
    pub app_type: &'a str,
    pub model: &'a str,
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub cache_read_tokens: u32,
    pub cache_creation_tokens: u32,
    pub created_at: i64,
}

/// session 日志写入前的统一去重判定。
///
/// 命中以下任一条件即跳过插入：① `request_id` 已存在；② 时间窗口内存在
/// 与 `key` 匹配的 proxy 日志（指纹去重）。
pub(crate) fn should_skip_session_insert(
    conn: &Connection,
    request_id: &str,
    key: &DedupKey,
) -> Result<bool, AppError> {
    if proxy_request_id_exists(conn, request_id)? {
        return Ok(true);
    }
    has_matching_proxy_usage_log(conn, key)
}

fn proxy_request_id_exists(conn: &Connection, request_id: &str) -> Result<bool, AppError> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM proxy_request_logs WHERE request_id = ?1)",
        params![request_id],
        |row| row.get::<_, bool>(0),
    )
    .map_err(|e| AppError::Database(format!("查询 request_id 失败: {e}")))
}

pub(crate) fn has_matching_proxy_usage_log(
    conn: &Connection,
    key: &DedupKey,
) -> Result<bool, AppError> {
    // 这里描述的是哪些会话日志来源可能不上报 cache_creation，用于去重匹配，
    // 与 input 语义（AppType::input_semantics）无关，因此不复用该判定。
    let allow_missing_cache_creation =
        matches!(key.app_type, "codex" | "gemini" | "opencode") && key.cache_creation_tokens == 0;

    let l_data_source = data_source_expr("l");
    let sql = format!(
        "SELECT EXISTS (
            SELECT 1
            FROM proxy_request_logs l
            WHERE {l_data_source} = 'proxy'
              AND l.app_type = ?1
              AND l.status_code >= 200
              AND l.status_code < 300
              AND l.input_tokens = ?3
              AND l.output_tokens = ?4
              AND l.cache_read_tokens = ?5
              AND (l.cache_creation_tokens = ?6 OR ?9 = 1)
              AND l.created_at BETWEEN ?7 - ?8 AND ?7 + ?8
              AND (
                  LOWER(l.model) = LOWER(?2)
                  OR LOWER(l.model) = 'unknown'
                  OR LOWER(?2) = 'unknown'
              )
        )"
    );

    conn.query_row(
        &sql,
        params![
            key.app_type,
            key.model,
            key.input_tokens as i64,
            key.output_tokens as i64,
            key.cache_read_tokens as i64,
            key.cache_creation_tokens as i64,
            key.created_at,
            SESSION_PROXY_DEDUP_WINDOW_SECONDS,
            allow_missing_cache_creation as i64,
        ],
        |row| row.get::<_, bool>(0),
    )
    .map_err(|e| AppError::Database(format!("查询重复代理用量日志失败: {e}")))
}

#[derive(Debug, Clone, Default)]
struct RollupDateBounds {
    start: Option<String>,
    end: Option<String>,
    is_empty: bool,
}

fn local_datetime_from_timestamp(ts: i64) -> Result<chrono::DateTime<Local>, AppError> {
    Local
        .timestamp_opt(ts, 0)
        .single()
        .ok_or_else(|| AppError::Database(format!("无法解析本地时间戳: {ts}")))
}

fn compute_rollup_date_bounds(
    start_ts: Option<i64>,
    end_ts: Option<i64>,
) -> Result<RollupDateBounds, AppError> {
    let start = match start_ts {
        Some(ts) => {
            let local = local_datetime_from_timestamp(ts)?;
            let day = local.date_naive();
            if local.time().num_seconds_from_midnight() == 0 {
                Some(day.format("%Y-%m-%d").to_string())
            } else {
                day.succ_opt()
                    .map(|next| next.format("%Y-%m-%d").to_string())
            }
        }
        None => None,
    };

    let end = match end_ts {
        Some(ts) => {
            let local = local_datetime_from_timestamp(ts)?;
            let day = local.date_naive();
            if local.time().hour() == 23 && local.time().minute() == 59 {
                Some(day.format("%Y-%m-%d").to_string())
            } else {
                day.pred_opt()
                    .map(|prev| prev.format("%Y-%m-%d").to_string())
            }
        }
        None => None,
    };

    let is_empty = matches!((&start, &end), (Some(start), Some(end)) if start > end);

    Ok(RollupDateBounds {
        start,
        end,
        is_empty,
    })
}

fn push_rollup_date_filters(
    conditions: &mut Vec<String>,
    params: &mut Vec<Box<dyn rusqlite::ToSql>>,
    column: &str,
    bounds: &RollupDateBounds,
) {
    if bounds.is_empty {
        conditions.push("1 = 0".to_string());
        return;
    }

    if let Some(start) = &bounds.start {
        conditions.push(format!("{column} >= ?"));
        params.push(Box::new(start.clone()));
    }

    if let Some(end) = &bounds.end {
        conditions.push(format!("{column} <= ?"));
        params.push(Box::new(end.clone()));
    }
}

fn local_day_start_rfc3339(day: NaiveDate) -> String {
    let local_midnight = day
        .and_hms_opt(0, 0, 0)
        .and_then(|naive| match Local.from_local_datetime(&naive) {
            chrono::LocalResult::Single(dt) => Some(dt),
            chrono::LocalResult::Ambiguous(earliest, _) => Some(earliest),
            chrono::LocalResult::None => None,
        })
        .unwrap_or_else(Local::now);

    local_midnight.to_rfc3339()
}

/// 额度检查的窗口用量：明细按 `created_at` 时间戳 `[?3, ?4)`，
/// 汇总按本地日期字符串 `[?5, ?6)`。
pub(crate) const PROVIDER_LIMIT_WINDOW_SQL: &str = "SELECT COALESCE(SUM(cost), 0) FROM (
    SELECT CAST(total_cost_usd AS REAL) as cost
    FROM proxy_request_logs
    WHERE provider_id = ?1 AND app_type = ?2
      AND created_at >= ?3 AND created_at < ?4
    UNION ALL
    SELECT CAST(total_cost_usd AS REAL)
    FROM usage_daily_rollups
    WHERE provider_id = ?1 AND app_type = ?2
      AND date >= ?5 AND date < ?6
)";

impl Database {
    /// 获取使用量汇总
    pub fn get_usage_summary(
        &self,
        start_date: Option<i64>,
        end_date: Option<i64>,
        app_type: Option<&str>,
        provider_name: Option<&str>,
        model: Option<&str>,
    ) -> Result<UsageSummary, AppError> {
        let conn = lock_conn!(self.conn);

        // Build detail WHERE clause
        let mut conditions = vec![effective_usage_log_filter("l")];
        let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if let Some(start) = start_date {
            conditions.push("l.created_at >= ?".to_string());
            params_vec.push(Box::new(start));
        }
        if let Some(end) = end_date {
            conditions.push("l.created_at <= ?".to_string());
            params_vec.push(Box::new(end));
        }
        if let Some(at) = app_type {
            conditions.push(format!("{} = ?", folded_app_type_sql("l.app_type")));
            params_vec.push(Box::new(at.to_string()));
        }
        push_provider_model_filters(
            &mut conditions,
            &mut params_vec,
            "l",
            "p",
            provider_name,
            model,
        );

        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };
        let detail_join = if provider_name.is_some() {
            providers_join("l", "p")
        } else {
            String::new()
        };

        // Only include rolled-up rows for full local days that are fully covered by the range.
        let mut rollup_conditions: Vec<String> = Vec::new();
        let mut rollup_params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        let rollup_bounds = compute_rollup_date_bounds(start_date, end_date)?;

        push_rollup_date_filters(
            &mut rollup_conditions,
            &mut rollup_params,
            "r.date",
            &rollup_bounds,
        );
        if let Some(at) = app_type {
            rollup_conditions.push(format!("{} = ?", folded_app_type_sql("r.app_type")));
            rollup_params.push(Box::new(at.to_string()));
        }
        push_provider_model_filters(
            &mut rollup_conditions,
            &mut rollup_params,
            "r",
            "p2",
            provider_name,
            model,
        );

        let rollup_where = if rollup_conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", rollup_conditions.join(" AND "))
        };
        let rollup_join = if provider_name.is_some() {
            providers_join("r", "p2")
        } else {
            String::new()
        };

        let fresh_input_detail = fresh_input_sql("l");
        let fresh_input_rollup = fresh_input_sql("r");
        let sql = format!(
            "SELECT
                COALESCE(d.total_requests, 0) + COALESCE(r.total_requests, 0),
                COALESCE(d.total_cost, 0) + COALESCE(r.total_cost, 0),
                COALESCE(d.total_input_tokens, 0) + COALESCE(r.total_input_tokens, 0),
                COALESCE(d.total_output_tokens, 0) + COALESCE(r.total_output_tokens, 0),
                COALESCE(d.total_cache_creation_tokens, 0) + COALESCE(r.total_cache_creation_tokens, 0),
                COALESCE(d.total_cache_read_tokens, 0) + COALESCE(r.total_cache_read_tokens, 0),
                COALESCE(d.success_count, 0) + COALESCE(r.success_count, 0)
            FROM
                (SELECT
                    COUNT(*) as total_requests,
                    COALESCE(SUM(CAST(l.total_cost_usd AS REAL)), 0) as total_cost,
                    COALESCE(SUM({fresh_input_detail}), 0) as total_input_tokens,
                    COALESCE(SUM(l.output_tokens), 0) as total_output_tokens,
                    COALESCE(SUM(l.cache_creation_tokens), 0) as total_cache_creation_tokens,
                    COALESCE(SUM(l.cache_read_tokens), 0) as total_cache_read_tokens,
                    COALESCE(SUM(CASE WHEN l.status_code >= 200 AND l.status_code < 300 THEN 1 ELSE 0 END), 0) as success_count
                 FROM proxy_request_logs l {detail_join} {where_clause}) d,
                (SELECT
                    COALESCE(SUM(r.request_count), 0) as total_requests,
                    COALESCE(SUM(CAST(r.total_cost_usd AS REAL)), 0) as total_cost,
                    COALESCE(SUM({fresh_input_rollup}), 0) as total_input_tokens,
                    COALESCE(SUM(r.output_tokens), 0) as total_output_tokens,
                    COALESCE(SUM(r.cache_creation_tokens), 0) as total_cache_creation_tokens,
                    COALESCE(SUM(r.cache_read_tokens), 0) as total_cache_read_tokens,
                    COALESCE(SUM(r.success_count), 0) as success_count
                 FROM usage_daily_rollups r {rollup_join} {rollup_where}) r"
        );

        // Combine params: detail params first, then rollup params
        let mut all_params: Vec<Box<dyn rusqlite::ToSql>> = params_vec;
        all_params.extend(rollup_params);
        let param_refs: Vec<&dyn rusqlite::ToSql> = all_params.iter().map(|p| p.as_ref()).collect();

        let result = conn.query_row(&sql, param_refs.as_slice(), |row| {
            let total_requests: i64 = row.get(0)?;
            let total_cost: f64 = row.get(1)?;
            let total_input_tokens: i64 = row.get(2)?;
            let total_output_tokens: i64 = row.get(3)?;
            let total_cache_creation_tokens: i64 = row.get(4)?;
            let total_cache_read_tokens: i64 = row.get(5)?;
            let success_count: i64 = row.get(6)?;

            let success_rate = if total_requests > 0 {
                (success_count as f32 / total_requests as f32) * 100.0
            } else {
                0.0
            };

            let (real_total_tokens, cache_hit_rate) = derive_real_total_and_hit_rate(
                total_input_tokens as u64,
                total_output_tokens as u64,
                total_cache_creation_tokens as u64,
                total_cache_read_tokens as u64,
            );

            Ok(UsageSummary {
                total_requests: total_requests as u64,
                total_cost: format!("{total_cost:.6}"),
                total_input_tokens: total_input_tokens as u64,
                total_output_tokens: total_output_tokens as u64,
                total_cache_creation_tokens: total_cache_creation_tokens as u64,
                total_cache_read_tokens: total_cache_read_tokens as u64,
                success_rate,
                real_total_tokens,
                cache_hit_rate,
            })
        })?;

        Ok(result)
    }

    /// 按 app_type 维度拆分的使用量汇总，用于 Dashboard 的分应用展示条。
    /// 返回所有有数据的 app_type，按 real_total_tokens 降序。
    ///
    /// Single SQL with `GROUP BY app_type` — avoids the N+1 round-trip that
    /// would result from invoking `get_usage_summary` once per app_type.
    pub fn get_usage_summary_by_app(
        &self,
        start_date: Option<i64>,
        end_date: Option<i64>,
        provider_name: Option<&str>,
        model: Option<&str>,
    ) -> Result<Vec<UsageSummaryByApp>, AppError> {
        let conn = lock_conn!(self.conn);

        let mut detail_conditions = vec![effective_usage_log_filter("l")];
        let mut detail_params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        if let Some(start) = start_date {
            detail_conditions.push("l.created_at >= ?".to_string());
            detail_params.push(Box::new(start));
        }
        if let Some(end) = end_date {
            detail_conditions.push("l.created_at <= ?".to_string());
            detail_params.push(Box::new(end));
        }
        push_provider_model_filters(
            &mut detail_conditions,
            &mut detail_params,
            "l",
            "p",
            provider_name,
            model,
        );
        let detail_where = format!("WHERE {}", detail_conditions.join(" AND "));
        let detail_join = if provider_name.is_some() {
            providers_join("l", "p")
        } else {
            String::new()
        };

        let rollup_bounds = compute_rollup_date_bounds(start_date, end_date)?;
        let mut rollup_conditions: Vec<String> = Vec::new();
        let mut rollup_params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        push_rollup_date_filters(
            &mut rollup_conditions,
            &mut rollup_params,
            "r.date",
            &rollup_bounds,
        );
        push_provider_model_filters(
            &mut rollup_conditions,
            &mut rollup_params,
            "r",
            "p2",
            provider_name,
            model,
        );
        let rollup_where = if rollup_conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", rollup_conditions.join(" AND "))
        };
        let rollup_join = if provider_name.is_some() {
            providers_join("r", "p2")
        } else {
            String::new()
        };

        let fresh_input_detail = fresh_input_sql("l");
        let fresh_input_rollup = fresh_input_sql("r");
        // 折叠 claude-desktop → claude：内层投影成同一桶名，外层 GROUP BY 自然合并。
        let detail_app_type = folded_app_type_sql("l.app_type");
        let rollup_app_type = folded_app_type_sql("r.app_type");

        let sql = format!(
            "SELECT app_type,
                SUM(req_count) as req_count,
                SUM(cost) as cost,
                SUM(input_t) as input_t,
                SUM(output_t) as output_t,
                SUM(cache_create_t) as cache_create_t,
                SUM(cache_read_t) as cache_read_t,
                SUM(success_count) as success_count
            FROM (
                SELECT {detail_app_type} as app_type,
                    COUNT(*) as req_count,
                    COALESCE(SUM(CAST(l.total_cost_usd AS REAL)), 0) as cost,
                    COALESCE(SUM({fresh_input_detail}), 0) as input_t,
                    COALESCE(SUM(l.output_tokens), 0) as output_t,
                    COALESCE(SUM(l.cache_creation_tokens), 0) as cache_create_t,
                    COALESCE(SUM(l.cache_read_tokens), 0) as cache_read_t,
                    COALESCE(SUM(CASE WHEN l.status_code >= 200 AND l.status_code < 300 THEN 1 ELSE 0 END), 0) as success_count
                FROM proxy_request_logs l {detail_join} {detail_where}
                GROUP BY l.app_type
                UNION ALL
                SELECT {rollup_app_type} as app_type,
                    COALESCE(SUM(r.request_count), 0),
                    COALESCE(SUM(CAST(r.total_cost_usd AS REAL)), 0),
                    COALESCE(SUM({fresh_input_rollup}), 0),
                    COALESCE(SUM(r.output_tokens), 0),
                    COALESCE(SUM(r.cache_creation_tokens), 0),
                    COALESCE(SUM(r.cache_read_tokens), 0),
                    COALESCE(SUM(r.success_count), 0)
                FROM usage_daily_rollups r {rollup_join} {rollup_where}
                GROUP BY r.app_type
            )
            GROUP BY app_type"
        );

        let mut combined: Vec<Box<dyn rusqlite::ToSql>> = detail_params;
        combined.extend(rollup_params);
        let refs: Vec<&dyn rusqlite::ToSql> = combined.iter().map(|p| p.as_ref()).collect();

        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(refs.as_slice(), |row| {
            let app_type: String = row.get(0)?;
            let total_requests: i64 = row.get(1)?;
            let total_cost: f64 = row.get(2)?;
            let total_input_tokens: i64 = row.get(3)?;
            let total_output_tokens: i64 = row.get(4)?;
            let total_cache_creation_tokens: i64 = row.get(5)?;
            let total_cache_read_tokens: i64 = row.get(6)?;
            let success_count: i64 = row.get(7)?;

            let success_rate = if total_requests > 0 {
                (success_count as f32 / total_requests as f32) * 100.0
            } else {
                0.0
            };
            let (real_total_tokens, cache_hit_rate) = derive_real_total_and_hit_rate(
                total_input_tokens as u64,
                total_output_tokens as u64,
                total_cache_creation_tokens as u64,
                total_cache_read_tokens as u64,
            );

            Ok(UsageSummaryByApp {
                app_type,
                summary: UsageSummary {
                    total_requests: total_requests as u64,
                    total_cost: format!("{total_cost:.6}"),
                    total_input_tokens: total_input_tokens as u64,
                    total_output_tokens: total_output_tokens as u64,
                    total_cache_creation_tokens: total_cache_creation_tokens as u64,
                    total_cache_read_tokens: total_cache_read_tokens as u64,
                    success_rate,
                    real_total_tokens,
                    cache_hit_rate,
                },
            })
        })?;

        let mut summaries = Vec::new();
        for row in rows {
            let item = row?;
            if item.summary.total_requests == 0 && item.summary.real_total_tokens == 0 {
                continue;
            }
            summaries.push(item);
        }
        summaries.sort_by(|a, b| {
            b.summary
                .real_total_tokens
                .cmp(&a.summary.real_total_tokens)
        });
        Ok(summaries)
    }

    /// 获取每日趋势（滑动窗口，<=24h 按小时，>24h 按天，窗口与汇总一致）
    pub fn get_daily_trends(
        &self,
        start_date: Option<i64>,
        end_date: Option<i64>,
        app_type: Option<&str>,
        provider_name: Option<&str>,
        model: Option<&str>,
    ) -> Result<Vec<DailyStats>, AppError> {
        let conn = lock_conn!(self.conn);

        let end_ts = end_date.unwrap_or_else(|| Local::now().timestamp());
        let mut start_ts = start_date.unwrap_or_else(|| end_ts - 24 * 60 * 60);

        if start_ts >= end_ts {
            start_ts = end_ts - 24 * 60 * 60;
        }

        let duration = end_ts - start_ts;
        if duration <= 24 * 60 * 60 {
            let bucket_seconds: i64 = 60 * 60;
            let mut bucket_count: i64 = if duration <= 0 {
                1
            } else {
                (duration + bucket_seconds - 1) / bucket_seconds
            };

            if bucket_count < 1 {
                bucket_count = 1;
            }

            let mut extra_conditions: Vec<String> = Vec::new();
            let mut extra_params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
            if let Some(at) = app_type {
                extra_conditions.push(format!("{} = ?", folded_app_type_sql("l.app_type")));
                extra_params.push(Box::new(at.to_string()));
            }
            push_provider_model_filters(
                &mut extra_conditions,
                &mut extra_params,
                "l",
                "p",
                provider_name,
                model,
            );
            let extra_filter = extra_conditions
                .iter()
                .map(|c| format!("AND {c}"))
                .collect::<Vec<_>>()
                .join(" ");
            let detail_join = if provider_name.is_some() {
                providers_join("l", "p")
            } else {
                String::new()
            };

            let effective_filter = effective_usage_log_filter("l");
            let fresh_input = fresh_input_sql("l");
            let sql = format!(
                "SELECT
                    CAST((l.created_at - ?1) / ?3 AS INTEGER) as bucket_idx,
                    COUNT(*) as request_count,
                    COALESCE(SUM(CAST(l.total_cost_usd AS REAL)), 0) as total_cost,
                    COALESCE(SUM({fresh_input} + l.output_tokens), 0) as total_tokens,
                    COALESCE(SUM({fresh_input}), 0) as total_input_tokens,
                    COALESCE(SUM(l.output_tokens), 0) as total_output_tokens,
                    COALESCE(SUM(l.cache_creation_tokens), 0) as total_cache_creation_tokens,
                    COALESCE(SUM(l.cache_read_tokens), 0) as total_cache_read_tokens
                FROM proxy_request_logs l {detail_join}
                WHERE l.created_at >= ?1 AND l.created_at <= ?2
                  AND {effective_filter} {extra_filter}
                GROUP BY bucket_idx
                ORDER BY bucket_idx ASC"
            );

            let mut stmt = conn.prepare(&sql)?;
            let row_mapper = |row: &rusqlite::Row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    DailyStats {
                        date: String::new(),
                        request_count: row.get::<_, i64>(1)? as u64,
                        total_cost: format!("{:.6}", row.get::<_, f64>(2)?),
                        total_tokens: row.get::<_, i64>(3)? as u64,
                        total_input_tokens: row.get::<_, i64>(4)? as u64,
                        total_output_tokens: row.get::<_, i64>(5)? as u64,
                        total_cache_creation_tokens: row.get::<_, i64>(6)? as u64,
                        total_cache_read_tokens: row.get::<_, i64>(7)? as u64,
                    },
                ))
            };

            let mut map: HashMap<i64, DailyStats> = HashMap::new();

            let mut all_params: Vec<Box<dyn rusqlite::ToSql>> = vec![
                Box::new(start_ts),
                Box::new(end_ts),
                Box::new(bucket_seconds),
            ];
            all_params.extend(extra_params);
            let param_refs: Vec<&dyn rusqlite::ToSql> =
                all_params.iter().map(|p| p.as_ref()).collect();
            let rows = stmt.query_map(param_refs.as_slice(), row_mapper)?;
            for row in rows {
                let (mut bucket_idx, stat) = row?;
                if bucket_idx < 0 {
                    continue;
                }
                if bucket_idx >= bucket_count {
                    bucket_idx = bucket_count - 1;
                }
                map.insert(bucket_idx, stat);
            }

            let mut stats = Vec::with_capacity(bucket_count as usize);
            for i in 0..bucket_count {
                let bucket_start_ts = start_ts + i * bucket_seconds;
                let bucket_start = local_datetime_from_timestamp(bucket_start_ts)?;
                let date = bucket_start.to_rfc3339();

                if let Some(mut stat) = map.remove(&i) {
                    stat.date = date;
                    stats.push(stat);
                } else {
                    stats.push(DailyStats {
                        date,
                        request_count: 0,
                        total_cost: "0.000000".to_string(),
                        total_tokens: 0,
                        total_input_tokens: 0,
                        total_output_tokens: 0,
                        total_cache_creation_tokens: 0,
                        total_cache_read_tokens: 0,
                    });
                }
            }

            return Ok(stats);
        }

        let start_day = local_datetime_from_timestamp(start_ts)?.date_naive();
        let end_day = local_datetime_from_timestamp(end_ts)?.date_naive();
        let bucket_count = (end_day.signed_duration_since(start_day).num_days() + 1) as usize;

        let mut extra_conditions: Vec<String> = Vec::new();
        let mut extra_params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        if let Some(at) = app_type {
            extra_conditions.push(format!("{} = ?", folded_app_type_sql("l.app_type")));
            extra_params.push(Box::new(at.to_string()));
        }
        push_provider_model_filters(
            &mut extra_conditions,
            &mut extra_params,
            "l",
            "p",
            provider_name,
            model,
        );
        let extra_filter = extra_conditions
            .iter()
            .map(|c| format!("AND {c}"))
            .collect::<Vec<_>>()
            .join(" ");
        let detail_join = if provider_name.is_some() {
            providers_join("l", "p")
        } else {
            String::new()
        };

        let effective_filter = effective_usage_log_filter("l");
        let fresh_input = fresh_input_sql("l");
        let detail_sql = format!(
            "SELECT
                date(l.created_at, 'unixepoch', 'localtime') as bucket_date,
                COUNT(*) as request_count,
                COALESCE(SUM(CAST(l.total_cost_usd AS REAL)), 0) as total_cost,
                COALESCE(SUM({fresh_input} + l.output_tokens), 0) as total_tokens,
                COALESCE(SUM({fresh_input}), 0) as total_input_tokens,
                COALESCE(SUM(l.output_tokens), 0) as total_output_tokens,
                COALESCE(SUM(l.cache_creation_tokens), 0) as total_cache_creation_tokens,
                COALESCE(SUM(l.cache_read_tokens), 0) as total_cache_read_tokens
            FROM proxy_request_logs l {detail_join}
            WHERE l.created_at >= ?1 AND l.created_at <= ?2
              AND {effective_filter} {extra_filter}
            GROUP BY bucket_date
            ORDER BY bucket_date ASC"
        );

        let mut detail_stmt = conn.prepare(&detail_sql)?;
        let detail_row_mapper = |row: &rusqlite::Row| {
            Ok((
                row.get::<_, String>(0)?,
                DailyStats {
                    date: String::new(),
                    request_count: row.get::<_, i64>(1)? as u64,
                    total_cost: format!("{:.6}", row.get::<_, f64>(2)?),
                    total_tokens: row.get::<_, i64>(3)? as u64,
                    total_input_tokens: row.get::<_, i64>(4)? as u64,
                    total_output_tokens: row.get::<_, i64>(5)? as u64,
                    total_cache_creation_tokens: row.get::<_, i64>(6)? as u64,
                    total_cache_read_tokens: row.get::<_, i64>(7)? as u64,
                },
            ))
        };

        let mut map: HashMap<NaiveDate, DailyStats> = HashMap::new();
        let mut detail_all_params: Vec<Box<dyn rusqlite::ToSql>> =
            vec![Box::new(start_ts), Box::new(end_ts)];
        detail_all_params.extend(extra_params);
        let detail_param_refs: Vec<&dyn rusqlite::ToSql> =
            detail_all_params.iter().map(|p| p.as_ref()).collect();
        let detail_rows = detail_stmt.query_map(detail_param_refs.as_slice(), detail_row_mapper)?;

        for row in detail_rows {
            let (bucket_date, stat) = row?;
            let date = NaiveDate::parse_from_str(&bucket_date, "%Y-%m-%d")
                .map_err(|err| AppError::Database(format!("解析趋势日期失败: {err}")))?;
            map.insert(date, stat);
        }

        let rollup_bounds = compute_rollup_date_bounds(Some(start_ts), Some(end_ts))?;
        let mut rollup_conditions = Vec::new();
        let mut rollup_params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        push_rollup_date_filters(
            &mut rollup_conditions,
            &mut rollup_params,
            "r.date",
            &rollup_bounds,
        );
        if let Some(at) = app_type {
            rollup_conditions.push(format!("{} = ?", folded_app_type_sql("r.app_type")));
            rollup_params.push(Box::new(at.to_string()));
        }
        push_provider_model_filters(
            &mut rollup_conditions,
            &mut rollup_params,
            "r",
            "p2",
            provider_name,
            model,
        );

        let rollup_where = if rollup_conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", rollup_conditions.join(" AND "))
        };
        let rollup_join = if provider_name.is_some() {
            providers_join("r", "p2")
        } else {
            String::new()
        };

        let fresh_input_rollup = fresh_input_sql("r");
        let rollup_sql = format!(
            "SELECT
                r.date,
                COALESCE(SUM(r.request_count), 0),
                COALESCE(SUM(CAST(r.total_cost_usd AS REAL)), 0),
                COALESCE(SUM({fresh_input_rollup} + r.output_tokens), 0),
                COALESCE(SUM({fresh_input_rollup}), 0),
                COALESCE(SUM(r.output_tokens), 0),
                COALESCE(SUM(r.cache_creation_tokens), 0),
                COALESCE(SUM(r.cache_read_tokens), 0)
            FROM usage_daily_rollups r {rollup_join}
            {rollup_where}
            GROUP BY r.date
            ORDER BY r.date ASC"
        );

        let mut rollup_stmt = conn.prepare(&rollup_sql)?;
        let rollup_row_mapper = |row: &rusqlite::Row| {
            Ok((
                row.get::<_, String>(0)?,
                (
                    row.get::<_, i64>(1)? as u64,
                    row.get::<_, f64>(2)?,
                    row.get::<_, i64>(3)? as u64,
                    row.get::<_, i64>(4)? as u64,
                    row.get::<_, i64>(5)? as u64,
                    row.get::<_, i64>(6)? as u64,
                    row.get::<_, i64>(7)? as u64,
                ),
            ))
        };
        let rollup_param_refs: Vec<&dyn rusqlite::ToSql> =
            rollup_params.iter().map(|param| param.as_ref()).collect();
        let rollup_rows = rollup_stmt.query_map(rollup_param_refs.as_slice(), rollup_row_mapper)?;

        for row in rollup_rows {
            let (bucket_date, (req, cost, tok, inp, out, cc, cr)) = row?;
            let date = NaiveDate::parse_from_str(&bucket_date, "%Y-%m-%d")
                .map_err(|err| AppError::Database(format!("解析 rollup 趋势日期失败: {err}")))?;
            let entry = map.entry(date).or_insert_with(|| DailyStats {
                date: String::new(),
                request_count: 0,
                total_cost: "0.000000".to_string(),
                total_tokens: 0,
                total_input_tokens: 0,
                total_output_tokens: 0,
                total_cache_creation_tokens: 0,
                total_cache_read_tokens: 0,
            });
            entry.request_count += req;
            let existing_cost: f64 = entry.total_cost.parse().unwrap_or(0.0);
            entry.total_cost = format!("{:.6}", existing_cost + cost);
            entry.total_tokens += tok;
            entry.total_input_tokens += inp;
            entry.total_output_tokens += out;
            entry.total_cache_creation_tokens += cc;
            entry.total_cache_read_tokens += cr;
        }

        let mut stats = Vec::with_capacity(bucket_count);
        let mut current_day = start_day;
        for _ in 0..bucket_count {
            let date = local_day_start_rfc3339(current_day);

            if let Some(mut stat) = map.remove(&current_day) {
                stat.date = date;
                stats.push(stat);
            } else {
                stats.push(DailyStats {
                    date,
                    request_count: 0,
                    total_cost: "0.000000".to_string(),
                    total_tokens: 0,
                    total_input_tokens: 0,
                    total_output_tokens: 0,
                    total_cache_creation_tokens: 0,
                    total_cache_read_tokens: 0,
                });
            }

            current_day = current_day.succ_opt().unwrap_or(current_day);
        }

        Ok(stats)
    }

    /// 获取 Provider 统计
    pub fn get_provider_stats(
        &self,
        start_date: Option<i64>,
        end_date: Option<i64>,
        app_type: Option<&str>,
        provider_name: Option<&str>,
        model: Option<&str>,
    ) -> Result<Vec<ProviderStats>, AppError> {
        let conn = lock_conn!(self.conn);

        let mut detail_conditions = vec![effective_usage_log_filter("l")];
        let mut detail_params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        if let Some(start) = start_date {
            detail_conditions.push("l.created_at >= ?".to_string());
            detail_params.push(Box::new(start));
        }
        if let Some(end) = end_date {
            detail_conditions.push("l.created_at <= ?".to_string());
            detail_params.push(Box::new(end));
        }
        if let Some(at) = app_type {
            detail_conditions.push(format!("{} = ?", folded_app_type_sql("l.app_type")));
            detail_params.push(Box::new(at.to_string()));
        }
        push_provider_model_filters(
            &mut detail_conditions,
            &mut detail_params,
            "l",
            "p",
            provider_name,
            model,
        );
        let detail_where = if detail_conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", detail_conditions.join(" AND "))
        };

        let mut rollup_conditions = Vec::new();
        let mut rollup_params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        let rollup_bounds = compute_rollup_date_bounds(start_date, end_date)?;
        push_rollup_date_filters(
            &mut rollup_conditions,
            &mut rollup_params,
            "r.date",
            &rollup_bounds,
        );
        if let Some(at) = app_type {
            rollup_conditions.push(format!("{} = ?", folded_app_type_sql("r.app_type")));
            rollup_params.push(Box::new(at.to_string()));
        }
        push_provider_model_filters(
            &mut rollup_conditions,
            &mut rollup_params,
            "r",
            "p2",
            provider_name,
            model,
        );
        let rollup_where = if rollup_conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", rollup_conditions.join(" AND "))
        };

        // UNION detail logs + rollup data, then aggregate
        let detail_pname = provider_name_coalesce("l", "p");
        let rollup_pname = provider_name_coalesce("r", "p2");
        let fresh_input_detail = fresh_input_sql("l");
        let fresh_input_rollup = fresh_input_sql("r");
        let sql = format!(
            "SELECT
                provider_id, app_type, provider_name,
                SUM(request_count) as request_count,
                SUM(total_tokens) as total_tokens,
                SUM(total_cost) as total_cost,
                SUM(success_count) as success_count,
                CASE WHEN SUM(request_count) > 0
                    THEN SUM(latency_sum) / SUM(request_count)
                    ELSE 0 END as avg_latency
            FROM (
                SELECT l.provider_id, l.app_type,
                    {detail_pname} as provider_name,
                    COUNT(*) as request_count,
                    COALESCE(SUM({fresh_input_detail} + l.output_tokens), 0) as total_tokens,
                    COALESCE(SUM(CAST(l.total_cost_usd AS REAL)), 0) as total_cost,
                    COALESCE(SUM(CASE WHEN l.status_code >= 200 AND l.status_code < 300 THEN 1 ELSE 0 END), 0) as success_count,
                    COALESCE(SUM(l.latency_ms), 0) as latency_sum
                FROM proxy_request_logs l
                LEFT JOIN providers p ON l.provider_id = p.id AND l.app_type = p.app_type
                {detail_where}
                GROUP BY l.provider_id, l.app_type
                UNION ALL
                SELECT r.provider_id, r.app_type,
                    {rollup_pname} as provider_name,
                    COALESCE(SUM(r.request_count), 0),
                    COALESCE(SUM({fresh_input_rollup} + r.output_tokens), 0),
                    COALESCE(SUM(CAST(r.total_cost_usd AS REAL)), 0),
                    COALESCE(SUM(r.success_count), 0),
                    COALESCE(SUM(r.avg_latency_ms * r.request_count), 0)
                FROM usage_daily_rollups r
                LEFT JOIN providers p2 ON r.provider_id = p2.id AND r.app_type = p2.app_type
                {rollup_where}
                GROUP BY r.provider_id, r.app_type
            )
            GROUP BY provider_id, app_type
            ORDER BY total_cost DESC"
        );

        let mut stmt = conn.prepare(&sql)?;
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = detail_params;
        params.extend(rollup_params);
        let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let row_mapper = |row: &rusqlite::Row| {
            let request_count: i64 = row.get(3)?;
            let success_count: i64 = row.get(6)?;
            let success_rate = if request_count > 0 {
                (success_count as f32 / request_count as f32) * 100.0
            } else {
                0.0
            };

            Ok(ProviderStats {
                provider_id: row.get(0)?,
                provider_name: row.get(2)?,
                request_count: request_count as u64,
                total_tokens: row.get::<_, i64>(4)? as u64,
                total_cost: format!("{:.6}", row.get::<_, f64>(5)?),
                success_rate,
                avg_latency_ms: row.get::<_, f64>(7)? as u64,
            })
        };

        let rows = stmt.query_map(param_refs.as_slice(), row_mapper)?;

        let mut stats = Vec::new();
        for row in rows {
            stats.push(row?);
        }

        Ok(stats)
    }

    /// 获取模型统计
    pub fn get_model_stats(
        &self,
        start_date: Option<i64>,
        end_date: Option<i64>,
        app_type: Option<&str>,
        provider_name: Option<&str>,
        model: Option<&str>,
    ) -> Result<Vec<ModelStats>, AppError> {
        let conn = lock_conn!(self.conn);

        let mut detail_conditions = vec![effective_usage_log_filter("l")];
        let mut detail_params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        if let Some(start) = start_date {
            detail_conditions.push("l.created_at >= ?".to_string());
            detail_params.push(Box::new(start));
        }
        if let Some(end) = end_date {
            detail_conditions.push("l.created_at <= ?".to_string());
            detail_params.push(Box::new(end));
        }
        if let Some(at) = app_type {
            detail_conditions.push(format!("{} = ?", folded_app_type_sql("l.app_type")));
            detail_params.push(Box::new(at.to_string()));
        }
        push_provider_model_filters(
            &mut detail_conditions,
            &mut detail_params,
            "l",
            "p",
            provider_name,
            model,
        );
        let detail_where = if detail_conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", detail_conditions.join(" AND "))
        };
        let detail_join = if provider_name.is_some() {
            providers_join("l", "p")
        } else {
            String::new()
        };

        let mut rollup_conditions = Vec::new();
        let mut rollup_params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        let rollup_bounds = compute_rollup_date_bounds(start_date, end_date)?;
        push_rollup_date_filters(
            &mut rollup_conditions,
            &mut rollup_params,
            "r.date",
            &rollup_bounds,
        );
        if let Some(at) = app_type {
            rollup_conditions.push(format!("{} = ?", folded_app_type_sql("r.app_type")));
            rollup_params.push(Box::new(at.to_string()));
        }
        push_provider_model_filters(
            &mut rollup_conditions,
            &mut rollup_params,
            "r",
            "p2",
            provider_name,
            model,
        );
        let rollup_where = if rollup_conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", rollup_conditions.join(" AND "))
        };
        let rollup_join = if provider_name.is_some() {
            providers_join("r", "p2")
        } else {
            String::new()
        };

        // UNION detail logs + rollup data
        //
        // 分组键用「有效计价模型」：pricing_model 非空时优先（成本就是按它的
        // 定价算的，金额与定价表自洽），NULL/'' 回落 model。默认 response 计价
        // 模式下两者相同，行为不变；request 模式 + 路由接管下，钱挂在实际计价
        // 基准名下，而不是上游回显/客户端别名名下。
        let fresh_input_detail = fresh_input_sql("l");
        let fresh_input_rollup = fresh_input_sql("r");
        let detail_model = effective_model_sql("l");
        let rollup_model = effective_model_sql("r");
        let sql = format!(
            "SELECT
                model,
                SUM(request_count) as request_count,
                SUM(total_tokens) as total_tokens,
                SUM(total_cost) as total_cost
            FROM (
                SELECT {detail_model} as model,
                    COUNT(*) as request_count,
                    COALESCE(SUM({fresh_input_detail} + l.output_tokens), 0) as total_tokens,
                    COALESCE(SUM(CAST(l.total_cost_usd AS REAL)), 0) as total_cost
                FROM proxy_request_logs l
                {detail_join}
                {detail_where}
                GROUP BY {detail_model}
                UNION ALL
                SELECT {rollup_model},
                    COALESCE(SUM(r.request_count), 0),
                    COALESCE(SUM({fresh_input_rollup} + r.output_tokens), 0),
                    COALESCE(SUM(CAST(r.total_cost_usd AS REAL)), 0)
                FROM usage_daily_rollups r
                {rollup_join}
                {rollup_where}
                GROUP BY {rollup_model}
            )
            GROUP BY model
            ORDER BY total_cost DESC"
        );

        let mut stmt = conn.prepare(&sql)?;
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = detail_params;
        params.extend(rollup_params);
        let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let row_mapper = |row: &rusqlite::Row| {
            let request_count: i64 = row.get(1)?;
            let total_cost: f64 = row.get(3)?;
            let avg_cost = if request_count > 0 {
                total_cost / request_count as f64
            } else {
                0.0
            };

            Ok(ModelStats {
                model: row.get(0)?,
                request_count: request_count as u64,
                total_tokens: row.get::<_, i64>(2)? as u64,
                total_cost: format!("{total_cost:.6}"),
                avg_cost_per_request: format!("{avg_cost:.6}"),
            })
        };

        let rows = stmt.query_map(param_refs.as_slice(), row_mapper)?;

        let mut stats = Vec::new();
        for row in rows {
            stats.push(row?);
        }

        Ok(stats)
    }

    /// 获取请求日志列表（分页）
    pub fn get_request_logs(
        &self,
        filters: &LogFilters,
        page: u32,
        page_size: u32,
    ) -> Result<PaginatedLogs, AppError> {
        let conn = lock_conn!(self.conn);

        let query = UsageQuery::from_filters(filters)
            .limit(page_size)
            .offset(page.saturating_mul(page_size));
        if query.unpriced {
            // 先把已补上定价的候选行回填掉，总数与分页才只剩真正缺定价的行
            Self::backfill_unpriced_candidates(&conn, &query)?;
        }
        let total = query.count(&conn)?;
        let page_rows = query.fetch(&conn)?;

        let mut logs = Vec::with_capacity(page_rows.rows.len());
        let mut pricing_cache = HashMap::new();
        for mut log in page_rows.rows {
            Self::maybe_backfill_log_costs(&conn, &mut log, &mut pricing_cache)?;
            logs.push(log);
        }

        Ok(PaginatedLogs {
            data: logs,
            total,
            page,
            page_size,
        })
    }

    /// 按有效计价模型汇总未计价的成功请求（导出 CSV 用）。
    ///
    /// 与 `get_request_logs` 的 `unpriced` 筛选同口径：先尝试按当前定价回填，
    /// 回填后仍为 0 成本的行才计入；按 `request_count` 降序、模型名升序排列。
    pub fn get_unpriced_model_stats(
        &self,
        filters: &LogFilters,
    ) -> Result<Vec<UnpricedModelStats>, AppError> {
        let conn = lock_conn!(self.conn);

        let query = UsageQuery::from_filters(filters).unpriced();
        let rows = Self::backfill_unpriced_candidates(&conn, &query)?;

        let mut by_model: HashMap<String, UnpricedModelStats> = HashMap::new();
        for log in rows {
            let model = unpriced_model_id(&log);
            let entry = by_model
                .entry(model.clone())
                .or_insert_with(|| UnpricedModelStats {
                    model,
                    request_count: 0,
                    input_tokens: 0,
                    output_tokens: 0,
                    cache_read_tokens: 0,
                    cache_creation_tokens: 0,
                    first_seen_at: log.created_at,
                    last_seen_at: log.created_at,
                });
            entry.request_count += 1;
            entry.input_tokens += u64::from(log.input_tokens);
            entry.output_tokens += u64::from(log.output_tokens);
            entry.cache_read_tokens += u64::from(log.cache_read_tokens);
            entry.cache_creation_tokens += u64::from(log.cache_creation_tokens);
            entry.first_seen_at = entry.first_seen_at.min(log.created_at);
            entry.last_seen_at = entry.last_seen_at.max(log.created_at);
        }

        let mut stats: Vec<_> = by_model.into_values().collect();
        stats.sort_by(|a, b| {
            b.request_count
                .cmp(&a.request_count)
                .then_with(|| a.model.cmp(&b.model))
        });
        Ok(stats)
    }

    /// 对满足 `query` 筛选（须带 `unpriced`）的全部候选行尝试回填成本，
    /// 返回回填后仍未计价的行（忽略 `query` 的分页设置）。
    fn backfill_unpriced_candidates(
        conn: &Connection,
        query: &UsageQuery,
    ) -> Result<Vec<RequestLogDetail>, AppError> {
        let candidates = UsageQuery {
            after: None,
            offset: None,
            limit: u32::MAX - 1,
            ..query.clone()
        }
        .unpriced()
        .fetch(conn)?;

        let mut pricing_cache = HashMap::new();
        let mut still_unpriced = Vec::with_capacity(candidates.rows.len());
        for mut log in candidates.rows {
            if !Self::maybe_backfill_log_costs(conn, &mut log, &mut pricing_cache)? {
                still_unpriced.push(log);
            }
        }
        Ok(still_unpriced)
    }

    /// 获取单个请求详情
    pub fn get_request_detail(
        &self,
        request_id: &str,
    ) -> Result<Option<RequestLogDetail>, AppError> {
        let conn = lock_conn!(self.conn);

        let detail_pname = provider_name_coalesce("l", "p");
        let detail_sql = format!(
            "SELECT l.request_id, l.provider_id, {detail_pname} as provider_name, l.app_type, l.model,
                    l.request_model, l.cost_multiplier,
                    input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                    input_cost_usd, output_cost_usd, cache_read_cost_usd, cache_creation_cost_usd, total_cost_usd,
                    is_streaming, latency_ms, first_token_ms, duration_ms,
                    status_code, error_message, created_at, l.data_source, l.pricing_model,
                    l.input_token_semantics
             FROM proxy_request_logs l
             LEFT JOIN providers p ON l.provider_id = p.id AND l.app_type = p.app_type
             WHERE l.request_id = ?"
        );
        let result = conn.query_row(&detail_sql, [request_id], row_to_request_log_detail);

        match result {
            Ok(mut detail) => {
                let mut pricing_cache = HashMap::new();
                Self::maybe_backfill_log_costs(&conn, &mut detail, &mut pricing_cache)?;
                Ok(Some(detail))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(AppError::Database(e.to_string())),
        }
    }

    /// 检查 Provider 使用限额
    pub fn check_provider_limits(
        &self,
        provider_id: &str,
        app_type: &str,
    ) -> Result<ProviderLimitStatus, AppError> {
        self.check_provider_limits_at(provider_id, app_type, Local::now())
    }

    /// 以 `now` 所在的本地日 / 本地月为窗口检查限额。
    ///
    /// 窗口在 Rust 中按本地午夜计算为半开区间 `[start, end)`，SQL 只做
    /// `created_at` 范围比较，可走 `idx_request_logs_provider_created_at` 索引；
    /// 任何数据库错误都会向上返回，而不是被当作 $0 用量。
    pub(crate) fn check_provider_limits_at(
        &self,
        provider_id: &str,
        app_type: &str,
        now: chrono::DateTime<Local>,
    ) -> Result<ProviderLimitStatus, AppError> {
        let today = now.date_naive();
        let tomorrow = today
            .succ_opt()
            .ok_or_else(|| AppError::Database("限额日窗口溢出".to_string()))?;
        let month_start = today
            .with_day(1)
            .ok_or_else(|| AppError::Database("限额月窗口计算失败".to_string()))?;
        let next_month_start = month_start
            .checked_add_months(chrono::Months::new(1))
            .ok_or_else(|| AppError::Database("限额月窗口溢出".to_string()))?;

        let day_start_ts = local_midnight_timestamp(today)?;
        let day_end_ts = local_midnight_timestamp(tomorrow)?;
        let month_start_ts = local_midnight_timestamp(month_start)?;
        let month_end_ts = local_midnight_timestamp(next_month_start)?;

        let conn = lock_conn!(self.conn);

        // 获取 provider 的限额设置（provider 不存在视为未设置限额；meta 解析失败同样视为未设置）
        let meta_str: Option<String> = conn
            .query_row(
                "SELECT meta FROM providers WHERE id = ? AND app_type = ?",
                params![provider_id, app_type],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()
            .map_err(|e| AppError::Database(format!("读取 provider 限额设置失败: {e}")))?
            .flatten();
        let (limit_daily, limit_monthly) = meta_str
            .and_then(|meta_str| serde_json::from_str::<serde_json::Value>(&meta_str).ok())
            .map(|meta| {
                let daily = meta
                    .get("limitDailyUsd")
                    .and_then(|v| v.as_str())
                    .and_then(|s| s.parse::<f64>().ok());
                let monthly = meta
                    .get("limitMonthlyUsd")
                    .and_then(|v| v.as_str())
                    .and_then(|s| s.parse::<f64>().ok());
                (daily, monthly)
            })
            .unwrap_or((None, None));

        // 明细 [start_ts, end_ts) + 汇总 [start_date, end_date)
        let sum_window = |start_ts: i64,
                          end_ts: i64,
                          start_date: NaiveDate,
                          end_date: NaiveDate|
         -> Result<f64, AppError> {
            conn.query_row(
                PROVIDER_LIMIT_WINDOW_SQL,
                params![
                    provider_id,
                    app_type,
                    start_ts,
                    end_ts,
                    start_date.format("%Y-%m-%d").to_string(),
                    end_date.format("%Y-%m-%d").to_string()
                ],
                |row| row.get(0),
            )
            .map_err(|e| AppError::Database(format!("统计 provider 限额用量失败: {e}")))
        };

        // 计算今日使用量 (detail logs + rollup)
        let daily_usage = sum_window(day_start_ts, day_end_ts, today, tomorrow)?;
        // 计算本月使用量 (detail logs + rollup)
        let monthly_usage =
            sum_window(month_start_ts, month_end_ts, month_start, next_month_start)?;

        let daily_exceeded = limit_daily
            .map(|limit| daily_usage >= limit)
            .unwrap_or(false);
        let monthly_exceeded = limit_monthly
            .map(|limit| monthly_usage >= limit)
            .unwrap_or(false);

        Ok(ProviderLimitStatus {
            provider_id: provider_id.to_string(),
            daily_usage: format!("{daily_usage:.6}"),
            daily_limit: limit_daily.map(|l| format!("{l:.2}")),
            daily_exceeded,
            monthly_usage: format!("{monthly_usage:.6}"),
            monthly_limit: limit_monthly.map(|l| format!("{l:.2}")),
            monthly_exceeded,
        })
    }
}

impl Database {
    /// Recalculate stored zero-cost usage rows once pricing becomes available.
    pub(crate) fn backfill_missing_usage_costs(&self) -> Result<u64, AppError> {
        let conn = lock_conn!(self.conn);
        Self::backfill_missing_usage_costs_on_conn(&conn, None)
    }

    /// 仅回填指定 model_id 相关的零成本行；用于单条定价更新后的精准回填。
    pub(crate) fn backfill_missing_usage_costs_for_model(
        &self,
        model_id: &str,
    ) -> Result<u64, AppError> {
        let conn = lock_conn!(self.conn);
        Self::backfill_missing_usage_costs_on_conn(&conn, Some(model_id))
    }

    pub(crate) fn backfill_missing_usage_costs_on_conn(
        conn: &Connection,
        only_model_id: Option<&str>,
    ) -> Result<u64, AppError> {
        const BASE_SQL: &str =
            "SELECT request_id, provider_id, NULL AS provider_name, app_type, model, request_model,
                        cost_multiplier,
                        input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                        input_cost_usd, output_cost_usd, cache_read_cost_usd,
                        cache_creation_cost_usd, total_cost_usd, is_streaming, latency_ms,
                        first_token_ms, duration_ms, status_code, error_message, created_at,
                        data_source, pricing_model, input_token_semantics
             FROM proxy_request_logs
             WHERE CAST(total_cost_usd AS REAL) <= 0
               AND (input_tokens > 0 OR output_tokens > 0
                    OR cache_read_tokens > 0 OR cache_creation_tokens > 0)";

        let mut logs = {
            let mut stmt = conn.prepare(BASE_SQL)?;
            let rows = stmt.query_map([], row_to_request_log_detail)?;
            rows.collect::<Result<Vec<_>, _>>()?
        };

        // 精准回填的行筛选必须与查价层共用 candidates 归一化：SQL 精确匹配会漏掉
        // 以原始别名落库的行（如 openrouter/anthropic/claude-sonnet-4.5:free），
        // 这些行查价时能归一化命中新定价，却在筛选层被挡掉，导致导入定价后
        // 历史成本要等下次全量回填才更新。误纳无害——查不到价的行会被跳过。
        if let Some(model_id) = only_model_id {
            let target = model_pricing_candidates(model_id);
            logs.retain(|log| log_pricing_scope_matches(log, &target));
        }

        if logs.is_empty() {
            return Ok(0);
        }

        let tx = conn
            .unchecked_transaction()
            .map_err(|e| AppError::Database(format!("启动用量成本回填事务失败: {e}")))?;

        let mut updated = 0u64;
        let mut pricing_cache = HashMap::new();
        for log in &mut logs {
            if Self::maybe_backfill_log_costs(&tx, log, &mut pricing_cache)? {
                updated += 1;
            }
        }
        tx.commit()
            .map_err(|e| AppError::Database(format!("提交用量成本回填事务失败: {e}")))?;

        if updated > 0 {
            log::info!("已回填 {updated} 条缺失的用量成本");
        }

        Ok(updated)
    }

    /// 尝试为单条 log 回填成本字段。返回是否实际写入（true=已 UPDATE，false=跳过）。
    fn maybe_backfill_log_costs(
        conn: &Connection,
        log: &mut RequestLogDetail,
        pricing_cache: &mut HashMap<String, ModelPricing>,
    ) -> Result<bool, AppError> {
        let existing_cost = rust_decimal::Decimal::from_str(&log.total_cost_usd)
            .unwrap_or(rust_decimal::Decimal::ZERO);
        let has_cost = existing_cost > rust_decimal::Decimal::ZERO;
        let has_usage = log.input_tokens > 0
            || log.output_tokens > 0
            || log.cache_read_tokens > 0
            || log.cache_creation_tokens > 0;

        if has_cost || !has_usage {
            return Ok(false);
        }

        let pricing = match Self::get_log_model_pricing_cached(conn, pricing_cache, log)? {
            Some(info) => info,
            None => return Ok(false),
        };
        let multiplier =
            rust_decimal::Decimal::from_str(&log.cost_multiplier).unwrap_or_else(|e| {
                log::warn!(
                    "历史用量倍率解析失败 request_id={}: {} - {e}",
                    log.request_id,
                    log.cost_multiplier
                );
                rust_decimal::Decimal::ONE
            });

        // 与 ingest 走同一个成本函数；落库行的 input 语义由 app_type 与
        // input_token_semantics 共同决定（LEGACY 行只含 cache read）。
        let cost = engine::cost(
            &TokenCounts {
                input: u64::from(log.input_tokens),
                output: u64::from(log.output_tokens),
                cache_read: u64::from(log.cache_read_tokens),
                cache_creation: u64::from(log.cache_creation_tokens),
            },
            InputBasis::for_stored_row(&log.app_type, log.input_token_semantics),
            &pricing,
            multiplier,
        );

        // 与 UsageLogger 落库格式一致（Decimal 原样输出），回填结果与 ingest 逐字相同。
        log.input_cost_usd = cost.input_cost.to_string();
        log.output_cost_usd = cost.output_cost.to_string();
        log.cache_read_cost_usd = cost.cache_read_cost.to_string();
        log.cache_creation_cost_usd = cost.cache_creation_cost.to_string();
        log.total_cost_usd = cost.total_cost.to_string();

        conn.execute(
            "UPDATE proxy_request_logs
             SET input_cost_usd = ?1,
                 output_cost_usd = ?2,
                 cache_read_cost_usd = ?3,
                 cache_creation_cost_usd = ?4,
                 total_cost_usd = ?5
             WHERE request_id = ?6",
            params![
                log.input_cost_usd,
                log.output_cost_usd,
                log.cache_read_cost_usd,
                log.cache_creation_cost_usd,
                log.total_cost_usd,
                log.request_id
            ],
        )
        .map_err(|e| AppError::Database(format!("更新请求成本失败: {e}")))?;

        Ok(true)
    }

    fn get_model_pricing_cached(
        conn: &Connection,
        cache: &mut HashMap<String, ModelPricing>,
        model: &str,
    ) -> Result<Option<ModelPricing>, AppError> {
        if let Some(info) = cache.get(model) {
            return Ok(Some(info.clone()));
        }

        let Some((input, output, cache_read, cache_creation)) =
            find_model_pricing_row(conn, model)?
        else {
            return Ok(None);
        };
        let pricing = ModelPricing::from_strings(&input, &output, &cache_read, &cache_creation)
            .map_err(|e| AppError::Database(format!("解析模型 {model} 定价失败: {e}")))?;

        cache.insert(model.to_string(), pricing.clone());
        Ok(Some(pricing))
    }

    fn get_log_model_pricing_cached(
        conn: &Connection,
        cache: &mut HashMap<String, ModelPricing>,
        log: &RequestLogDetail,
    ) -> Result<Option<ModelPricing>, AppError> {
        // 写入时的计价基准已落库（v11+）：回填只按它重算，找不到就保持 0 成本
        // 等补价。不能换用 model/request_model 猜——路由接管 + request 计价模式下
        // 三者可能各不相同（model=上游回显、request_model=客户端别名、
        // pricing_model=实际出站模型），换基准会按错误价格永久固化。
        // 占位符（"" = 未计价错误行 / "unknown"）视同缺失，走历史行逻辑。
        if let Some(pricing_model) = log
            .pricing_model
            .as_deref()
            .filter(|pm| !is_placeholder_pricing_model(pm))
        {
            return Self::get_model_pricing_cached(conn, cache, pricing_model);
        }

        if let Some(pricing) = Self::get_model_pricing_cached(conn, cache, &log.model)? {
            return Ok(Some(pricing));
        }

        // 仅当 model 列是占位符（解析失败留下的 ""/"unknown" 等）时才回退到
        // request_model 定价。model 是真实模型名但缺定价时必须保持 0 成本等待
        // 补价：路由接管下 request_model 是客户端别名（如 claude-sonnet-4-6），
        // 按别名回填会把真实上游模型的 tokens 按错误价格永久固化（行一旦有成本
        // 就不再进入回填范围）。
        if !is_placeholder_pricing_model(&log.model) {
            return Ok(None);
        }

        let Some(request_model) = log.request_model.as_deref() else {
            return Ok(None);
        };
        if request_model == log.model {
            return Ok(None);
        }

        Self::get_model_pricing_cached(conn, cache, request_model)
    }
}

/// 精准回填的行筛选：log 的任一模型字段归一化后与目标模型的 candidates 相交，
/// 或可按查价层的前缀规则命中目标，即视为相关。镜像 find_model_pricing_row 的
/// 匹配语义，宁可误纳（后续查价会兜底）不可漏筛。
fn log_pricing_scope_matches(log: &RequestLogDetail, target_candidates: &[String]) -> bool {
    [
        Some(log.model.as_str()),
        log.request_model.as_deref(),
        log.pricing_model.as_deref(),
    ]
    .into_iter()
    .flatten()
    .any(|field| {
        model_pricing_candidates(field).iter().any(|candidate| {
            target_candidates.iter().any(|target| {
                target == candidate
                    || (should_try_pricing_prefix_match(candidate)
                        && target
                            .strip_prefix(candidate.as_str())
                            .is_some_and(|rest| rest.starts_with('-')))
            })
        })
    })
}

// ---------------------------------------------------------------------------
// 数据来源
// ---------------------------------------------------------------------------

/// `proxy_request_logs.data_source` 的取值：代理记录或某个应用的会话日志导入。
///
/// 会话来源的行使用固定的占位 `provider_id`（不在 providers 表中），
/// Dashboard 用 [`UsageSource::session_display_name`] 展示可读名。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UsageSource {
    /// 代理记录（`data_source` 为 NULL 的旧行也视作代理）
    Proxy,
    /// Claude Code `~/.claude/projects/**/*.jsonl`
    ClaudeSession,
    /// Codex `~/.codex/sessions/**/*.jsonl`
    CodexSession,
    /// Gemini CLI `~/.gemini/tmp/*/chats/*.json`
    GeminiSession,
    /// OpenCode `opencode.db`
    OpenCodeSession,
}

impl UsageSource {
    /// 全部会话来源（导入写入的行）
    pub const SESSIONS: [UsageSource; 4] = [
        UsageSource::ClaudeSession,
        UsageSource::CodexSession,
        UsageSource::GeminiSession,
        UsageSource::OpenCodeSession,
    ];

    /// `data_source` 列的存储值
    pub const fn as_str(self) -> &'static str {
        match self {
            UsageSource::Proxy => "proxy",
            UsageSource::ClaudeSession => "session_log",
            UsageSource::CodexSession => "codex_session",
            UsageSource::GeminiSession => "gemini_session",
            UsageSource::OpenCodeSession => "opencode_session",
        }
    }

    /// 会话来源对应的 app_type
    pub const fn app_type(self) -> Option<&'static str> {
        match self {
            UsageSource::Proxy => None,
            UsageSource::ClaudeSession => Some("claude"),
            UsageSource::CodexSession => Some("codex"),
            UsageSource::GeminiSession => Some("gemini"),
            UsageSource::OpenCodeSession => Some("opencode"),
        }
    }

    /// 会话来源写入的占位 provider_id
    pub const fn placeholder_provider_id(self) -> Option<&'static str> {
        match self {
            UsageSource::Proxy => None,
            UsageSource::ClaudeSession => Some("_session"),
            UsageSource::CodexSession => Some("_codex_session"),
            UsageSource::GeminiSession => Some("_gemini_session"),
            UsageSource::OpenCodeSession => Some("_opencode_session"),
        }
    }

    /// 占位 provider_id 在 Dashboard 中的展示名
    pub const fn session_display_name(self) -> Option<&'static str> {
        match self {
            UsageSource::Proxy => None,
            UsageSource::ClaudeSession => Some("Claude (Session)"),
            UsageSource::CodexSession => Some("Codex (Session)"),
            UsageSource::GeminiSession => Some("Gemini (Session)"),
            UsageSource::OpenCodeSession => Some("OpenCode (Session)"),
        }
    }
}

// ---------------------------------------------------------------------------
// UsageQuery：请求日志筛选 + 分页
// ---------------------------------------------------------------------------

/// keyset 分页游标：上一页最后一行的 `(created_at, request_id)`。
///
/// 请求日志按 `created_at DESC, request_id DESC` 排序，`request_id` 是主键，
/// 因此游标唯一确定位置；翻页期间有新行写入也不会重复或漏行（OFFSET 分页会）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageCursor {
    pub created_at: i64,
    pub request_id: String,
}

/// [`UsageQuery::fetch`] 的结果
#[derive(Debug, Clone)]
#[allow(dead_code)] // `next` 供 keyset 调用方使用，页码式 IPC 不读
pub struct UsagePage {
    pub rows: Vec<RequestLogDetail>,
    /// 还有下一页时为本页最后一行的游标
    pub next: Option<UsageCursor>,
}

/// 请求日志查询构建器。
///
/// 默认排除已被代理日志覆盖的会话行（与 Dashboard 其余查询同口径，见
/// [`effective_usage_log_filter`]）。筛选条件与 [`LogFilters`] 一致；分页可用
/// [`UsageQuery::after`]（keyset）或 [`UsageQuery::offset`]（页码，供现有 IPC 使用）。
#[derive(Debug, Clone)]
pub struct UsageQuery {
    app_type: Option<String>,
    provider_name: Option<String>,
    model: Option<String>,
    status_code: Option<u16>,
    since: Option<i64>,
    until: Option<i64>,
    source: Option<UsageSource>,
    unpriced: bool,
    after: Option<UsageCursor>,
    offset: Option<u32>,
    limit: u32,
}

impl Default for UsageQuery {
    fn default() -> Self {
        Self::new()
    }
}

// 构建器的筛选 / keyset 方法目前只有 `get_request_logs`（页码式）与测试在用，
// 留给后续的 keyset IPC 与导出。
#[allow(dead_code)]
impl UsageQuery {
    /// 默认每页行数
    pub const DEFAULT_LIMIT: u32 = 50;

    pub fn new() -> Self {
        Self {
            app_type: None,
            provider_name: None,
            model: None,
            status_code: None,
            since: None,
            until: None,
            source: None,
            unpriced: false,
            after: None,
            offset: None,
            limit: Self::DEFAULT_LIMIT,
        }
    }

    /// 从前端的请求日志筛选器构建
    pub fn from_filters(filters: &LogFilters) -> Self {
        Self {
            app_type: filters.app_type.clone(),
            provider_name: filters.provider_name.clone(),
            model: filters.model.clone(),
            status_code: filters.status_code,
            since: filters.start_date,
            until: filters.end_date,
            unpriced: filters.unpriced.unwrap_or(false),
            ..Self::new()
        }
    }

    /// 按 app_type 过滤（`claude` 同时命中 `claude-desktop`）
    pub fn app_type(mut self, app_type: impl Into<String>) -> Self {
        self.app_type = Some(app_type.into());
        self
    }

    /// 按 Provider 展示名过滤（会话占位行的可读名也能命中）
    pub fn provider_name(mut self, name: impl Into<String>) -> Self {
        self.provider_name = Some(name.into());
        self
    }

    /// 按有效计价模型过滤
    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    pub fn status_code(mut self, status: u16) -> Self {
        self.status_code = Some(status);
        self
    }

    /// `created_at >= ts`
    pub fn since(mut self, ts: i64) -> Self {
        self.since = Some(ts);
        self
    }

    /// `created_at <= ts`
    pub fn until(mut self, ts: i64) -> Self {
        self.until = Some(ts);
        self
    }

    /// 只看某个数据来源
    pub fn source(mut self, source: UsageSource) -> Self {
        self.source = Some(source);
        self
    }

    /// 只看未计价的成功请求，见 [`unpriced_log_filter`]
    pub fn unpriced(mut self) -> Self {
        self.unpriced = true;
        self
    }

    /// keyset 分页：从游标之后（更早的行）开始
    pub fn after(mut self, cursor: UsageCursor) -> Self {
        self.after = Some(cursor);
        self
    }

    /// OFFSET 分页（页码式 IPC）；与 [`UsageQuery::after`] 可同时使用
    pub fn offset(mut self, offset: u32) -> Self {
        self.offset = Some(offset);
        self
    }

    /// 每页行数
    pub fn limit(mut self, limit: u32) -> Self {
        self.limit = limit;
        self
    }

    /// WHERE 子句与参数。`include_cursor = false` 用于总数统计（不受翻页位置影响）。
    fn where_clause(&self, include_cursor: bool) -> (String, Vec<Box<dyn rusqlite::ToSql>>) {
        let mut conditions = vec![effective_usage_log_filter("l")];
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if let Some(ref app_type) = self.app_type {
            // 仅过滤口径折叠 claude-desktop→claude；行投影仍返回原始 app_type，
            // 详情面板据此展示真实入口（路由接管账单审计需要）。
            conditions.push(format!("{} = ?", folded_app_type_sql("l.app_type")));
            params.push(Box::new(app_type.clone()));
        }
        // 与 Dashboard 顶部下拉筛选同口径：Provider 按展示名精确匹配（会话占位
        // 行如 "Claude (Session)" 也能命中），模型按有效计价模型匹配。
        push_provider_model_filters(
            &mut conditions,
            &mut params,
            "l",
            "p",
            self.provider_name.as_deref(),
            self.model.as_deref(),
        );
        if let Some(status) = self.status_code {
            conditions.push("l.status_code = ?".to_string());
            params.push(Box::new(status as i64));
        }
        if let Some(start) = self.since {
            conditions.push("l.created_at >= ?".to_string());
            params.push(Box::new(start));
        }
        if let Some(end) = self.until {
            conditions.push("l.created_at <= ?".to_string());
            params.push(Box::new(end));
        }
        if let Some(source) = self.source {
            conditions.push(format!("{} = ?", data_source_expr("l")));
            params.push(Box::new(source.as_str()));
        }
        if self.unpriced {
            conditions.push(unpriced_log_filter("l"));
        }
        if include_cursor {
            if let Some(ref cursor) = self.after {
                conditions.push(
                    "(l.created_at < ? OR (l.created_at = ? AND l.request_id < ?))".to_string(),
                );
                params.push(Box::new(cursor.created_at));
                params.push(Box::new(cursor.created_at));
                params.push(Box::new(cursor.request_id.clone()));
            }
        }

        (format!("WHERE {}", conditions.join(" AND ")), params)
    }

    /// 满足筛选条件的总行数（忽略分页位置）
    pub fn count(&self, conn: &Connection) -> Result<u32, AppError> {
        let (where_clause, params) = self.where_clause(false);
        let sql = format!(
            "SELECT COUNT(*) FROM proxy_request_logs l
             {providers_join}
             {where_clause}",
            providers_join = providers_join("l", "p"),
        );
        let refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        Ok(conn.query_row(&sql, refs.as_slice(), |row| {
            row.get::<_, i64>(0).map(|v| v as u32)
        })?)
    }

    /// 取一页。多取一行判断是否还有下一页。
    pub fn fetch(&self, conn: &Connection) -> Result<UsagePage, AppError> {
        let (where_clause, mut params) = self.where_clause(true);
        params.push(Box::new(i64::from(self.limit) + 1));
        params.push(Box::new(i64::from(self.offset.unwrap_or(0))));

        let logs_pname = provider_name_coalesce("l", "p");
        let sql = format!(
            "SELECT l.request_id, l.provider_id, {logs_pname} as provider_name, l.app_type, l.model,
                    l.request_model, l.cost_multiplier,
                    l.input_tokens, l.output_tokens, l.cache_read_tokens, l.cache_creation_tokens,
                    l.input_cost_usd, l.output_cost_usd, l.cache_read_cost_usd, l.cache_creation_cost_usd, l.total_cost_usd,
                    l.is_streaming, l.latency_ms, l.first_token_ms, l.duration_ms,
                    l.status_code, l.error_message, l.created_at, l.data_source, l.pricing_model,
                    l.input_token_semantics
             FROM proxy_request_logs l
             {providers_join}
             {where_clause}
             ORDER BY l.created_at DESC, l.request_id DESC
             LIMIT ? OFFSET ?",
            providers_join = providers_join("l", "p"),
        );

        let mut stmt = conn.prepare(&sql)?;
        let refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let mut rows = stmt
            .query_map(refs.as_slice(), row_to_request_log_detail)?
            .collect::<Result<Vec<_>, _>>()?;

        let has_more = rows.len() > self.limit as usize;
        rows.truncate(self.limit as usize);
        let next = if has_more {
            rows.last().map(|row| UsageCursor {
                created_at: row.created_at,
                request_id: row.request_id.clone(),
            })
        } else {
            None
        };
        Ok(UsagePage { rows, next })
    }
}

// ---------------------------------------------------------------------------
// 写入
// ---------------------------------------------------------------------------

/// 五个成本列（USD 十进制字符串）
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UsageCosts {
    pub input: String,
    pub output: String,
    pub cache_read: String,
    pub cache_creation: String,
    pub total: String,
}

impl UsageCosts {
    /// 未计价（缺定价 / 错误行）
    pub fn zero() -> Self {
        Self {
            input: "0".to_string(),
            output: "0".to_string(),
            cache_read: "0".to_string(),
            cache_creation: "0".to_string(),
            total: "0".to_string(),
        }
    }

    pub fn from_breakdown(cost: &engine::CostBreakdown) -> Self {
        Self {
            input: cost.input_cost.to_string(),
            output: cost.output_cost.to_string(),
            cache_read: cost.cache_read_cost.to_string(),
            cache_creation: cost.cache_creation_cost.to_string(),
            total: cost.total_cost.to_string(),
        }
    }
}

/// 一条代理请求日志
#[derive(Debug, Clone)]
pub(crate) struct ProxyUsageRow<'a> {
    pub request_id: &'a str,
    pub provider_id: &'a str,
    pub app_type: &'a str,
    pub model: &'a str,
    pub request_model: &'a str,
    pub pricing_model: &'a str,
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub cache_read_tokens: u32,
    pub cache_creation_tokens: u32,
    pub costs: UsageCosts,
    pub latency_ms: u64,
    pub first_token_ms: Option<u64>,
    pub status_code: u16,
    pub error_message: Option<&'a str>,
    pub session_id: Option<&'a str>,
    pub provider_type: Option<&'a str>,
    pub is_streaming: bool,
    pub cost_multiplier: &'a str,
    pub created_at: i64,
}

/// 写入代理请求日志（同 request_id 覆盖）。
pub(crate) fn insert_proxy_row(conn: &Connection, row: &ProxyUsageRow<'_>) -> Result<(), AppError> {
    conn.execute(
        "INSERT OR REPLACE INTO proxy_request_logs (
            request_id, provider_id, app_type, model, request_model, pricing_model,
            input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
            input_token_semantics,
            input_cost_usd, output_cost_usd, cache_read_cost_usd, cache_creation_cost_usd, total_cost_usd,
            latency_ms, first_token_ms, status_code, error_message, session_id,
            provider_type, is_streaming, cost_multiplier, created_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25)",
        params![
            row.request_id,
            row.provider_id,
            row.app_type,
            row.model,
            row.request_model,
            row.pricing_model,
            row.input_tokens,
            row.output_tokens,
            row.cache_read_tokens,
            row.cache_creation_tokens,
            input_token_semantics_for_app_type(row.app_type),
            row.costs.input,
            row.costs.output,
            row.costs.cache_read,
            row.costs.cache_creation,
            row.costs.total,
            row.latency_ms as i64,
            row.first_token_ms.map(|v| v as i64),
            row.status_code as i64,
            row.error_message,
            row.session_id,
            row.provider_type,
            row.is_streaming as i64,
            row.cost_multiplier,
            row.created_at,
        ],
    )
    .map_err(|e| AppError::Database(format!("记录请求日志失败: {e}")))?;
    Ok(())
}

/// 一条会话日志导入行。provider_id / app_type / data_source 由 `source` 决定。
#[derive(Debug, Clone)]
pub(crate) struct SessionUsageRow<'a> {
    pub request_id: &'a str,
    pub source: UsageSource,
    pub model: &'a str,
    /// 计价基准（落库供回填 / 重算使用）
    pub pricing_model: &'a str,
    pub session_id: Option<&'a str>,
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub cache_read_tokens: u32,
    pub cache_creation_tokens: u32,
    pub costs: UsageCosts,
    pub created_at: i64,
}

impl SessionUsageRow<'_> {
    fn app_type(&self) -> &'static str {
        self.source.app_type().unwrap_or("claude")
    }
}

const SESSION_INSERT_COLUMNS: &str = "INSERT INTO proxy_request_logs (
    request_id, provider_id, app_type, model, request_model,
    input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
    input_cost_usd, output_cost_usd, cache_read_cost_usd, cache_creation_cost_usd, total_cost_usd,
    latency_ms, first_token_ms, status_code, error_message, session_id,
    provider_type, is_streaming, cost_multiplier, created_at, data_source,
    pricing_model, input_token_semantics
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26)";

fn execute_session_insert(
    conn: &Connection,
    sql: &str,
    row: &SessionUsageRow<'_>,
) -> rusqlite::Result<usize> {
    let app_type = row.app_type();
    let data_source = row.source.as_str();
    conn.execute(
        sql,
        params![
            row.request_id,
            row.source.placeholder_provider_id().unwrap_or(""),
            app_type,
            row.model,
            row.model, // request_model = model
            row.input_tokens,
            row.output_tokens,
            row.cache_read_tokens,
            row.cache_creation_tokens,
            row.costs.input,
            row.costs.output,
            row.costs.cache_read,
            row.costs.cache_creation,
            row.costs.total,
            0i64,                   // latency_ms: 会话日志无此数据
            Option::<i64>::None,    // first_token_ms
            200i64,                 // status_code: 产生计费 token 即视为成功
            Option::<String>::None, // error_message
            row.session_id,
            Some(data_source), // provider_type
            1i64,              // is_streaming
            "1.0",             // cost_multiplier
            row.created_at,
            data_source,
            row.pricing_model,
            input_token_semantics_for_app_type(app_type),
        ],
    )
}

/// `INSERT OR IGNORE` 一条会话行，返回是否新插入。
pub(crate) fn insert_session_row(
    conn: &Connection,
    row: &SessionUsageRow<'_>,
) -> Result<bool, AppError> {
    let sql = SESSION_INSERT_COLUMNS.replacen("INSERT INTO", "INSERT OR IGNORE INTO", 1);
    let inserted = execute_session_insert(conn, &sql, row).map_err(|e| {
        AppError::Database(format!("插入会话日志失败 ({}): {e}", row.source.as_str()))
    })?;
    Ok(inserted > 0)
}

/// 插入或更新一条会话行（Gemini 全量重读文件，同一消息可能带更新后的值）。
/// 值完全相同时不写，返回是否有变更。
pub(crate) fn upsert_session_row(
    conn: &Connection,
    row: &SessionUsageRow<'_>,
) -> Result<bool, AppError> {
    let sql = format!(
        "{SESSION_INSERT_COLUMNS}
        ON CONFLICT(request_id) DO UPDATE SET
            model = excluded.model,
            input_tokens = excluded.input_tokens,
            output_tokens = excluded.output_tokens,
            cache_read_tokens = excluded.cache_read_tokens,
            input_cost_usd = excluded.input_cost_usd,
            output_cost_usd = excluded.output_cost_usd,
            cache_read_cost_usd = excluded.cache_read_cost_usd,
            cache_creation_cost_usd = excluded.cache_creation_cost_usd,
            total_cost_usd = excluded.total_cost_usd,
            pricing_model = excluded.pricing_model,
            input_token_semantics = excluded.input_token_semantics
        WHERE input_tokens != excluded.input_tokens
           OR output_tokens != excluded.output_tokens
           OR cache_read_tokens != excluded.cache_read_tokens
           OR model != excluded.model
           OR pricing_model IS NOT excluded.pricing_model
           OR input_token_semantics != excluded.input_token_semantics"
    );
    let changed = execute_session_insert(conn, &sql, row).map_err(|e| {
        AppError::Database(format!("插入会话日志失败 ({}): {e}", row.source.as_str()))
    })?;
    Ok(changed > 0)
}

/// 已导入的同一会话行的 output_tokens（判断是否为更新的流式快照）。
pub(crate) fn session_row_output_tokens(
    conn: &Connection,
    request_id: &str,
    source: UsageSource,
) -> Result<Option<i64>, AppError> {
    conn.query_row(
        "SELECT output_tokens FROM proxy_request_logs
         WHERE request_id = ?1 AND data_source = ?2",
        params![request_id, source.as_str()],
        |row| row.get(0),
    )
    .optional()
    .map_err(|e| AppError::Database(format!("查询会话日志失败: {e}")))
}

/// 用 output 更大的新快照重写已导入会话行的 token 与成本列，返回是否更新。
pub(crate) fn upgrade_session_row(
    conn: &Connection,
    row: &SessionUsageRow<'_>,
) -> Result<bool, AppError> {
    let updated = conn
        .execute(
            "UPDATE proxy_request_logs SET
                input_tokens = ?2, output_tokens = ?3, cache_read_tokens = ?4, cache_creation_tokens = ?5,
                input_cost_usd = ?6, output_cost_usd = ?7, cache_read_cost_usd = ?8,
                cache_creation_cost_usd = ?9, total_cost_usd = ?10
             WHERE request_id = ?1 AND data_source = ?11 AND output_tokens < ?3",
            params![
                row.request_id,
                row.input_tokens,
                row.output_tokens,
                row.cache_read_tokens,
                row.cache_creation_tokens,
                row.costs.input,
                row.costs.output,
                row.costs.cache_read,
                row.costs.cache_creation,
                row.costs.total,
                row.source.as_str(),
            ],
        )
        .map_err(|e| AppError::Database(format!("更新会话日志失败: {e}")))?;
    Ok(updated > 0)
}

/// 数据来源分布：(data_source, 行数, 总成本)，按行数降序。
pub(crate) fn data_source_breakdown(
    conn: &Connection,
) -> Result<Vec<(String, i64, f64)>, AppError> {
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
    let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?;
    let mut summaries = Vec::new();
    for row in rows {
        summaries.push(row.map_err(|e| AppError::Database(e.to_string()))?);
    }
    Ok(summaries)
}
