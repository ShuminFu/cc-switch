//! 使用统计服务
//!
//! 提供使用量数据的聚合查询功能。这里只定义 DTO；查询 SQL 在
//! [`crate::database::repo::usage`]（`impl Database` 的 Dashboard 查询方法也在那里）。

use serde::{Deserialize, Serialize};

pub(crate) use crate::database::repo::usage::{
    effective_usage_log_filter, has_matching_proxy_usage_log, should_skip_session_insert, DedupKey,
};

/// 使用量汇总
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct UsageSummary {
    pub total_requests: u64,
    pub total_cost: String,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_cache_creation_tokens: u64,
    pub total_cache_read_tokens: u64,
    #[cfg_attr(test, specta(type = specta_typescript::Number))]
    pub success_rate: f32,
    /// input + output + cache_creation + cache_read — the total tokens
    /// actually processed by the model (including cache hits). Used as the
    /// headline "real consumption" number in the usage hero.
    pub real_total_tokens: u64,
    /// cache_read / (input + cache_creation + cache_read). Range 0.0–1.0.
    /// Reported as a fraction; multiply by 100 in UI for percentage display.
    #[cfg_attr(test, specta(type = specta_typescript::Number))]
    pub cache_hit_rate: f64,
}

/// Per-app-type usage summary used by the dashboard breakdown rail.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct UsageSummaryByApp {
    pub app_type: String,
    pub summary: UsageSummary,
}

/// 每日统计
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct DailyStats {
    pub date: String,
    pub request_count: u64,
    pub total_cost: String,
    pub total_tokens: u64,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_cache_creation_tokens: u64,
    pub total_cache_read_tokens: u64,
}

/// Provider 统计
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct ProviderStats {
    pub provider_id: String,
    pub provider_name: String,
    pub request_count: u64,
    pub total_tokens: u64,
    pub total_cost: String,
    #[cfg_attr(test, specta(type = specta_typescript::Number))]
    pub success_rate: f32,
    pub avg_latency_ms: u64,
}

/// 模型统计
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct ModelStats {
    pub model: String,
    pub request_count: u64,
    pub total_tokens: u64,
    pub total_cost: String,
    pub avg_cost_per_request: String,
}

/// 未计价模型统计（按有效计价模型分组），供"导出未计价请求"使用
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct UnpricedModelStats {
    /// 补价时应填写的模型 ID（落库的计价模型，缺失时为 model 列）
    pub model: String,
    pub request_count: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_creation_tokens: u64,
    pub first_seen_at: i64,
    pub last_seen_at: i64,
}

/// 请求日志过滤器
#[derive(Debug, Clone, Default, Deserialize)]
#[cfg_attr(test, derive(specta::Type))]
// 所有字段均可缺省（前端只传设置了的筛选项）
#[serde(rename_all = "camelCase", default)]
pub struct LogFilters {
    pub app_type: Option<String>,
    pub provider_name: Option<String>,
    pub model: Option<String>,
    pub status_code: Option<u16>,
    pub start_date: Option<i64>,
    pub end_date: Option<i64>,
    /// 只看未计价的成功请求（有 token、成本为 0、倍率非 0）
    pub unpriced: Option<bool>,
}

/// 分页请求日志响应
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(test, derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct PaginatedLogs {
    pub data: Vec<RequestLogDetail>,
    pub total: u32,
    pub page: u32,
    pub page_size: u32,
}

/// 请求日志详情
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct RequestLogDetail {
    pub request_id: String,
    pub provider_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_name: Option<String>,
    pub app_type: String,
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_model: Option<String>,
    pub cost_multiplier: String,
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub cache_read_tokens: u32,
    pub cache_creation_tokens: u32,
    /// Internal storage semantics; omitted from the UI/API payload.
    #[serde(skip)]
    pub input_token_semantics: i64,
    pub input_cost_usd: String,
    pub output_cost_usd: String,
    pub cache_read_cost_usd: String,
    pub cache_creation_cost_usd: String,
    pub total_cost_usd: String,
    pub is_streaming: bool,
    pub latency_ms: u64,
    pub first_token_ms: Option<u64>,
    pub duration_ms: Option<u64>,
    pub status_code: u16,
    pub error_message: Option<String>,
    pub created_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_source: Option<String>,
    /// 写入时实际用于计价的模型名。None = v11 前的历史行，"" = 未计价的错误行。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pricing_model: Option<String>,
}

/// Provider 限额状态
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct ProviderLimitStatus {
    pub provider_id: String,
    pub daily_usage: String,
    pub daily_limit: Option<String>,
    pub daily_exceeded: bool,
    pub monthly_usage: String,
    pub monthly_limit: Option<String>,
    pub monthly_exceeded: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::repo::usage::PROVIDER_LIMIT_WINDOW_SQL;
    use crate::database::{lock_conn, Database};
    use crate::error::AppError;
    use crate::services::sql_helpers::INPUT_TOKEN_SEMANTICS_TOTAL;
    use chrono::{Local, TimeZone};
    use rusqlite::{params, Connection};
    use std::str::FromStr;

    /// 成本列按 Decimal 值比较（落库字符串与 ingest 一致，不固定小数位）。
    fn assert_cost(actual: &str, expected: &str) {
        assert_eq!(
            rust_decimal::Decimal::from_str(actual).unwrap(),
            rust_decimal::Decimal::from_str(expected).unwrap(),
            "stored cost {actual}"
        );
    }

    fn local_ts(year: i32, month: u32, day: u32, hour: u32, minute: u32, second: u32) -> i64 {
        match Local.with_ymd_and_hms(year, month, day, hour, minute, second) {
            chrono::LocalResult::Single(dt) => dt.timestamp(),
            chrono::LocalResult::Ambiguous(earliest, _) => earliest.timestamp(),
            chrono::LocalResult::None => panic!("valid local datetime"),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn insert_usage_log(
        conn: &Connection,
        request_id: &str,
        app_type: &str,
        provider_id: &str,
        model: &str,
        data_source: &str,
        created_at: i64,
        input_tokens: i64,
        output_tokens: i64,
        cache_read_tokens: i64,
        cache_creation_tokens: i64,
        status_code: i64,
        total_cost_usd: &str,
    ) -> Result<(), AppError> {
        conn.execute(
            "INSERT INTO proxy_request_logs (
                request_id, provider_id, app_type, model, request_model,
                input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                input_cost_usd, output_cost_usd, cache_read_cost_usd, cache_creation_cost_usd,
                total_cost_usd, latency_ms, status_code, created_at, data_source
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, '0', '0', '0', '0', ?, 100, ?, ?, ?)",
            params![
                request_id,
                provider_id,
                app_type,
                model,
                model,
                input_tokens,
                output_tokens,
                cache_read_tokens,
                cache_creation_tokens,
                total_cost_usd,
                status_code,
                created_at,
                data_source
            ],
        )?;
        Ok(())
    }

    fn create_legacy_nullable_logs_table(conn: &Connection) -> Result<(), AppError> {
        conn.execute(
            "CREATE TABLE proxy_request_logs (
                request_id TEXT PRIMARY KEY,
                app_type TEXT NOT NULL,
                model TEXT NOT NULL,
                input_tokens INTEGER NOT NULL,
                output_tokens INTEGER NOT NULL,
                cache_read_tokens INTEGER NOT NULL,
                cache_creation_tokens INTEGER NOT NULL,
                status_code INTEGER NOT NULL,
                created_at INTEGER NOT NULL,
                data_source TEXT
            )",
            [],
        )?;
        Ok(())
    }

    #[test]
    fn test_effective_filter_keeps_legacy_null_data_source_proxy_rows() -> Result<(), AppError> {
        let conn = Connection::open_in_memory()?;
        create_legacy_nullable_logs_table(&conn)?;
        conn.execute(
            "INSERT INTO proxy_request_logs (
                request_id, app_type, model, input_tokens, output_tokens,
                cache_read_tokens, cache_creation_tokens, status_code, created_at, data_source
            ) VALUES ('legacy-proxy', 'codex', 'gpt-5.5', 10, 2, 1, 0, 200, 1000, NULL)",
            [],
        )?;

        let filter = effective_usage_log_filter(&conn, "l")?;
        let sql = format!("SELECT COUNT(*) FROM proxy_request_logs l WHERE {filter}");
        let count: i64 = conn.query_row(&sql, [], |row| row.get(0))?;
        assert_eq!(count, 1);

        Ok(())
    }

    #[test]
    fn test_matching_proxy_log_treats_legacy_null_data_source_as_proxy() -> Result<(), AppError> {
        let conn = Connection::open_in_memory()?;
        create_legacy_nullable_logs_table(&conn)?;
        conn.execute(
            "INSERT INTO proxy_request_logs (
                request_id, app_type, model, input_tokens, output_tokens,
                cache_read_tokens, cache_creation_tokens, status_code, created_at, data_source
            ) VALUES ('legacy-proxy', 'codex', 'gpt-5.5', 10, 2, 1, 0, 200, 1000, NULL)",
            [],
        )?;

        let key = DedupKey {
            app_type: "codex",
            model: "gpt-5.5",
            input_tokens: 10,
            output_tokens: 2,
            cache_read_tokens: 1,
            cache_creation_tokens: 0,
            created_at: 1000,
        };
        assert!(has_matching_proxy_usage_log(&conn, &key)?);

        Ok(())
    }

    #[test]
    fn test_claude_desktop_folds_into_claude_for_display() -> Result<(), AppError> {
        let db = Database::memory()?;
        let ts = local_ts(2026, 6, 10, 12, 0, 0);

        {
            let conn = lock_conn!(db.conn);
            // 一条 Claude Code 行 + 一条 Claude Desktop 网关行，同一时间窗。
            insert_usage_log(
                &conn,
                "cc-1",
                "claude",
                "p-claude",
                "claude-sonnet-4-5",
                "proxy",
                ts,
                100,
                10,
                0,
                0,
                200,
                "0.5",
            )?;
            insert_usage_log(
                &conn,
                "cd-1",
                "claude-desktop",
                "p-desktop",
                "claude-opus-4-8",
                "proxy",
                ts,
                200,
                20,
                0,
                0,
                200,
                "1.5",
            )?;
        }

        // ① 分应用汇总：desktop 折叠进 claude，不再单列 claude-desktop 桶。
        let by_app = db.get_usage_summary_by_app(None, None, None, None)?;
        assert_eq!(by_app.len(), 1, "应只剩一个合并后的 claude 桶");
        assert_eq!(by_app[0].app_type, "claude");
        assert_eq!(by_app[0].summary.total_requests, 2, "两条行都计入 claude");
        assert!(
            !by_app.iter().any(|a| a.app_type == "claude-desktop"),
            "不应再出现 claude-desktop 桶"
        );

        // ② 选中 claude 过滤：汇总应同时覆盖 desktop 行。
        let claude_summary = db.get_usage_summary(None, None, Some("claude"), None, None)?;
        assert_eq!(claude_summary.total_requests, 2);

        // ③ 请求日志按 claude 过滤返回两行，且 desktop 行投影仍是原始 app_type。
        let logs = db.get_request_logs(
            &LogFilters {
                app_type: Some("claude".to_string()),
                ..Default::default()
            },
            0, // 页码从 0 开始
            50,
        )?;
        assert_eq!(logs.total, 2, "claude 过滤含 desktop 行");
        assert!(
            logs.data.iter().any(|r| r.app_type == "claude-desktop"),
            "详情面板需要看到真实入口，行投影不可被折叠"
        );

        // ④ 折叠不外溢：codex 过滤为空。
        let codex_summary = db.get_usage_summary(None, None, Some("codex"), None, None)?;
        assert_eq!(codex_summary.total_requests, 0);

        Ok(())
    }

    #[test]
    fn test_backfill_missing_usage_costs_uses_new_gpt_5_5_pricing() -> Result<(), AppError> {
        let db = Database::memory()?;

        {
            let conn = lock_conn!(db.conn);
            insert_usage_log(
                &conn,
                "codex-gpt-5-5-zero-cost",
                "codex",
                "_codex_session",
                "gpt-5.5",
                "codex_session",
                1000,
                1_000_000,
                1_000_000,
                0,
                0,
                200,
                "0",
            )?;
        }

        assert_eq!(db.backfill_missing_usage_costs()?, 1);

        let conn = lock_conn!(db.conn);
        let (input_cost, output_cost, total_cost): (String, String, String) = conn.query_row(
            "SELECT input_cost_usd, output_cost_usd, total_cost_usd
             FROM proxy_request_logs WHERE request_id = 'codex-gpt-5-5-zero-cost'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        assert_cost(&input_cost, "5.000000");
        assert_cost(&output_cost, "30.000000");
        assert_cost(&total_cost, "35.000000");

        Ok(())
    }

    #[test]
    fn test_backfill_distinguishes_legacy_and_total_cache_semantics() -> Result<(), AppError> {
        let db = Database::memory()?;

        {
            let conn = lock_conn!(db.conn);
            // v12 mirror row: input = fresh + read; creation was reported separately.
            insert_usage_log(
                &conn,
                "legacy-cache-semantics",
                "codex",
                "p1",
                "gpt-5.5",
                "proxy",
                1000,
                800_000,
                0,
                600_000,
                200_000,
                200,
                "0",
            )?;
            // v13 proxy row: input = fresh + read + creation.
            insert_usage_log(
                &conn,
                "total-cache-semantics",
                "codex",
                "p1",
                "gpt-5.5",
                "proxy",
                1001,
                1_000_000,
                0,
                600_000,
                200_000,
                200,
                "0",
            )?;
            conn.execute(
                "UPDATE proxy_request_logs
                 SET input_token_semantics = ?1
                 WHERE request_id = 'total-cache-semantics'",
                [INPUT_TOKEN_SEMANTICS_TOTAL],
            )?;
        }

        assert_eq!(db.backfill_missing_usage_costs()?, 2);

        let conn = lock_conn!(db.conn);
        let mut stmt = conn.prepare(
            "SELECT request_id, input_cost_usd
             FROM proxy_request_logs
             WHERE request_id IN ('legacy-cache-semantics', 'total-cache-semantics')
             ORDER BY request_id",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(
            rows,
            vec![
                ("legacy-cache-semantics".to_string(), "1".to_string()),
                ("total-cache-semantics".to_string(), "1".to_string()),
            ]
        );

        Ok(())
    }

    #[test]
    fn test_backfill_missing_usage_costs_uses_stored_multiplier() -> Result<(), AppError> {
        let db = Database::memory()?;

        {
            let conn = lock_conn!(db.conn);
            insert_usage_log(
                &conn,
                "codex-gpt-5-5-multiplier",
                "codex",
                "_codex_session",
                "gpt-5.5",
                "codex_session",
                1000,
                1_000_000,
                0,
                0,
                0,
                200,
                "0",
            )?;
            conn.execute(
                "UPDATE proxy_request_logs
                 SET cost_multiplier = '1.5'
                 WHERE request_id = 'codex-gpt-5-5-multiplier'",
                [],
            )?;
        }

        assert_eq!(db.backfill_missing_usage_costs()?, 1);

        let conn = lock_conn!(db.conn);
        let (input_cost, total_cost): (String, String) = conn.query_row(
            "SELECT input_cost_usd, total_cost_usd
             FROM proxy_request_logs WHERE request_id = 'codex-gpt-5-5-multiplier'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        assert_cost(&input_cost, "5.000000");
        assert_cost(&total_cost, "7.500000");

        Ok(())
    }

    #[test]
    fn test_backfill_missing_usage_costs_falls_back_to_request_model() -> Result<(), AppError> {
        let db = Database::memory()?;

        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model, request_model,
                    input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                    input_cost_usd, output_cost_usd, cache_read_cost_usd, cache_creation_cost_usd,
                    total_cost_usd, latency_ms, status_code, created_at, data_source
                ) VALUES (
                    'codex-request-model-fallback', '_codex_session', 'codex', 'unknown', 'gpt-5.5',
                    1000000, 0, 0, 0,
                    '0', '0', '0', '0',
                    '0', 100, 200, 1000, 'codex_session'
                )",
                [],
            )?;
        }

        assert_eq!(db.backfill_missing_usage_costs()?, 1);

        let conn = lock_conn!(db.conn);
        let total_cost: String = conn.query_row(
            "SELECT total_cost_usd
             FROM proxy_request_logs WHERE request_id = 'codex-request-model-fallback'",
            [],
            |row| row.get(0),
        )?;
        assert_cost(&total_cost, "5.000000");

        Ok(())
    }

    #[test]
    fn test_backfill_skips_request_model_fallback_for_real_unpriced_model() -> Result<(), AppError>
    {
        let db = Database::memory()?;

        {
            let conn = lock_conn!(db.conn);
            // 路由接管场景：model 是上游回显的真实模型（缺定价），request_model
            // 是客户端别名（有定价）。回填不得按别名定价，必须保持 0 成本等待补价。
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model, request_model,
                    input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                    input_cost_usd, output_cost_usd, cache_read_cost_usd, cache_creation_cost_usd,
                    total_cost_usd, latency_ms, status_code, created_at, data_source
                ) VALUES (
                    'takeover-unpriced-model', 'provider-1', 'claude',
                    'takeover-real-model-unpriced', 'claude-sonnet-4-6',
                    1000000, 0, 0, 0,
                    '0', '0', '0', '0',
                    '0', 100, 200, 1000, 'proxy'
                )",
                [],
            )?;
        }

        // request_model（claude-sonnet-4-6）有定价，但 model 是真实模型名：不得回退
        assert_eq!(db.backfill_missing_usage_costs()?, 0);

        {
            let conn = lock_conn!(db.conn);
            let total_cost: String = conn.query_row(
                "SELECT total_cost_usd
                 FROM proxy_request_logs WHERE request_id = 'takeover-unpriced-model'",
                [],
                |row| row.get(0),
            )?;
            assert_eq!(total_cost, "0");

            // 补上真实模型定价后，回填必须按真实模型价格修复（0 成本行未被污染固化）
            conn.execute(
                "INSERT INTO model_pricing (model_id, display_name, input_cost_per_million, output_cost_per_million)
                 VALUES ('takeover-real-model-unpriced', 'Takeover Real Model', '0.6', '2.5')",
                [],
            )?;
        }

        assert_eq!(db.backfill_missing_usage_costs()?, 1);

        let conn = lock_conn!(db.conn);
        let total_cost: String = conn.query_row(
            "SELECT total_cost_usd
             FROM proxy_request_logs WHERE request_id = 'takeover-unpriced-model'",
            [],
            |row| row.get(0),
        )?;
        assert_cost(&total_cost, "0.600000");

        Ok(())
    }

    #[test]
    fn test_backfill_uses_persisted_pricing_model() -> Result<(), AppError> {
        let db = Database::memory()?;

        {
            let conn = lock_conn!(db.conn);
            // request 计价模式 + 接管：写入时锚定出站模型 kimi-k2-novel（当时缺价），
            // 但上游回显了别名 → model/request_model 都是 claude-sonnet-4-6（有定价）。
            // 回填必须按落库的 pricing_model 重算，不得换用 model 列的别名价格。
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model, request_model, pricing_model,
                    input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                    input_cost_usd, output_cost_usd, cache_read_cost_usd, cache_creation_cost_usd,
                    total_cost_usd, latency_ms, status_code, created_at, data_source
                ) VALUES (
                    'persisted-pricing-model', 'provider-1', 'claude',
                    'claude-sonnet-4-6', 'claude-sonnet-4-6', 'kimi-k2-novel',
                    1000000, 0, 0, 0,
                    '0', '0', '0', '0',
                    '0', 100, 200, 1000, 'proxy'
                )",
                [],
            )?;
        }

        // pricing_model（kimi-k2-novel）缺价：不得回退到 model 列的别名价格
        assert_eq!(db.backfill_missing_usage_costs()?, 0);

        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO model_pricing (model_id, display_name, input_cost_per_million, output_cost_per_million)
                 VALUES ('kimi-k2-novel', 'Kimi K2 Novel', '0.6', '2.5')",
                [],
            )?;
        }

        // 按 pricing_model 也能定位到该行（model/request_model 都不是 kimi-k2-novel）
        assert_eq!(
            db.backfill_missing_usage_costs_for_model("kimi-k2-novel")?,
            1
        );

        let conn = lock_conn!(db.conn);
        let total_cost: String = conn.query_row(
            "SELECT total_cost_usd
             FROM proxy_request_logs WHERE request_id = 'persisted-pricing-model'",
            [],
            |row| row.get(0),
        )?;
        assert_cost(&total_cost, "0.600000");

        Ok(())
    }

    #[test]
    fn test_scoped_backfill_matches_raw_alias_rows() -> Result<(), AppError> {
        let db = Database::memory()?;

        {
            let conn = lock_conn!(db.conn);
            // 代理日志按上游原文落库：带路由前缀和 :free 后缀的别名形式。
            // 精准回填的筛选必须归一化后匹配，否则这类行要等全量回填才更新。
            insert_usage_log(
                &conn,
                "openrouter-alias-zero-cost",
                "claude",
                "provider-1",
                "openrouter/moonshot/kimi-k2-novel:free",
                "proxy",
                1000,
                1_000_000,
                0,
                0,
                0,
                200,
                "0",
            )?;
        }

        // 定价缺失时不应回填
        assert_eq!(db.backfill_missing_usage_costs()?, 0);

        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO model_pricing (model_id, display_name, input_cost_per_million, output_cost_per_million)
                 VALUES ('kimi-k2-novel', 'Kimi K2 Novel', '0.6', '2.5')",
                [],
            )?;
        }

        // 按归一化 ID 精准回填，应命中以原始别名落库的行
        assert_eq!(
            db.backfill_missing_usage_costs_for_model("kimi-k2-novel")?,
            1
        );

        let conn = lock_conn!(db.conn);
        let total_cost: String = conn.query_row(
            "SELECT total_cost_usd
             FROM proxy_request_logs WHERE request_id = 'openrouter-alias-zero-cost'",
            [],
            |row| row.get(0),
        )?;
        assert_cost(&total_cost, "0.600000");

        Ok(())
    }

    #[test]
    fn test_backfill_missing_usage_costs_keeps_claude_fresh_input() -> Result<(), AppError> {
        let db = Database::memory()?;

        {
            let conn = lock_conn!(db.conn);
            insert_usage_log(
                &conn,
                "claude-cache-fresh-input",
                "claude",
                "_session",
                "claude-haiku-4-5",
                "session_log",
                1000,
                100,
                0,
                200,
                0,
                200,
                "0",
            )?;
        }

        assert_eq!(db.backfill_missing_usage_costs()?, 1);

        let conn = lock_conn!(db.conn);
        let (input_cost, cache_read_cost, total_cost): (String, String, String) = conn.query_row(
            "SELECT input_cost_usd, cache_read_cost_usd, total_cost_usd
             FROM proxy_request_logs WHERE request_id = 'claude-cache-fresh-input'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        assert_cost(&input_cost, "0.000100");
        assert_cost(&cache_read_cost, "0.000020");
        assert_cost(&total_cost, "0.000120");

        Ok(())
    }

    #[test]
    fn test_get_usage_summary() -> Result<(), AppError> {
        let db = Database::memory()?;

        // 插入测试数据
        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model,
                    input_tokens, output_tokens, total_cost_usd,
                    latency_ms, status_code, created_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params!["req1", "p1", "claude", "claude-3", 100, 50, "0.01", 100, 200, 1000],
            )?;
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model,
                    input_tokens, output_tokens, total_cost_usd,
                    latency_ms, status_code, created_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params!["req2", "p1", "claude", "claude-3", 200, 100, "0.02", 150, 200, 2000],
            )?;
        }

        let summary = db.get_usage_summary(None, None, None, None, None)?;
        assert_eq!(summary.total_requests, 2);
        assert_eq!(summary.success_rate, 100.0);

        Ok(())
    }

    #[test]
    fn test_get_usage_summary_excludes_partial_rollup_boundary_days() -> Result<(), AppError> {
        let db = Database::memory()?;
        let start = local_ts(2024, 1, 1, 12, 0, 0);
        let end = local_ts(2024, 1, 3, 12, 0, 0);

        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO usage_daily_rollups (
                    date, app_type, provider_id, model,
                    request_count, success_count, input_tokens, output_tokens,
                    cache_read_tokens, cache_creation_tokens, total_cost_usd, avg_latency_ms
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params![
                    "2024-01-01",
                    "claude",
                    "p1",
                    "claude-3",
                    10,
                    10,
                    1000,
                    500,
                    0,
                    0,
                    "1.00",
                    100
                ],
            )?;
            conn.execute(
                "INSERT INTO usage_daily_rollups (
                    date, app_type, provider_id, model,
                    request_count, success_count, input_tokens, output_tokens,
                    cache_read_tokens, cache_creation_tokens, total_cost_usd, avg_latency_ms
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params![
                    "2024-01-02",
                    "claude",
                    "p1",
                    "claude-3",
                    20,
                    19,
                    2000,
                    1000,
                    0,
                    0,
                    "2.00",
                    120
                ],
            )?;
            conn.execute(
                "INSERT INTO usage_daily_rollups (
                    date, app_type, provider_id, model,
                    request_count, success_count, input_tokens, output_tokens,
                    cache_read_tokens, cache_creation_tokens, total_cost_usd, avg_latency_ms
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params![
                    "2024-01-03",
                    "claude",
                    "p1",
                    "claude-3",
                    30,
                    29,
                    3000,
                    1500,
                    0,
                    0,
                    "3.00",
                    140
                ],
            )?;
        }

        let summary = db.get_usage_summary(Some(start), Some(end), Some("claude"), None, None)?;
        assert_eq!(summary.total_requests, 20);
        assert_eq!(summary.total_input_tokens, 2000);
        assert_eq!(summary.total_output_tokens, 1000);

        Ok(())
    }

    #[test]
    fn test_provider_and_model_filters_cover_detail_and_rollup() -> Result<(), AppError> {
        let db = Database::memory()?;
        let detail_ts = local_ts(2026, 6, 10, 12, 0, 0);

        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO providers (id, app_type, name, settings_config) VALUES
                 ('prov-a', 'claude', 'Packy', '{}'),
                 ('prov-b', 'claude', 'DeepSeek', '{}')",
                [],
            )?;

            insert_usage_log(
                &conn,
                "a-1",
                "claude",
                "prov-a",
                "claude-sonnet-4-6",
                "proxy",
                detail_ts,
                100,
                10,
                0,
                0,
                200,
                "1.0",
            )?;
            insert_usage_log(
                &conn,
                "b-1",
                "claude",
                "prov-b",
                "deepseek-v3",
                "proxy",
                detail_ts,
                200,
                20,
                0,
                0,
                200,
                "2.0",
            )?;
            // 会话占位行：providers 表无此 id，展示名走 CASE 映射。
            insert_usage_log(
                &conn,
                "s-1",
                "claude",
                "_session",
                "claude-sonnet-4-6",
                "session_log",
                detail_ts,
                999,
                99,
                0,
                0,
                200,
                "0.5",
            )?;
            // 计价模型与请求模型不同的行：模型筛选必须按有效计价模型命中。
            insert_usage_log(
                &conn,
                "a-2",
                "claude",
                "prov-a",
                "alias-model",
                "proxy",
                detail_ts,
                50,
                5,
                0,
                0,
                200,
                "0.3",
            )?;
            conn.execute(
                "UPDATE proxy_request_logs SET pricing_model = 'real-model' WHERE request_id = 'a-2'",
                [],
            )?;

            // rollup 历史日行：无范围过滤时全部计入。
            conn.execute(
                "INSERT INTO usage_daily_rollups (
                    date, app_type, provider_id, model,
                    request_count, success_count, input_tokens, output_tokens,
                    cache_read_tokens, cache_creation_tokens, total_cost_usd, avg_latency_ms
                ) VALUES
                ('2026-06-08', 'claude', 'prov-a', 'claude-sonnet-4-6', 5, 5, 500, 50, 0, 0, '5.0', 100),
                ('2026-06-08', 'claude', 'prov-b', 'deepseek-v3', 7, 7, 700, 70, 0, 0, '7.0', 100)",
                [],
            )?;
        }

        // ① 汇总按 Provider 展示名过滤：明细 + rollup 都命中。
        let packy = db.get_usage_summary(None, None, None, Some("Packy"), None)?;
        assert_eq!(packy.total_requests, 7, "a-1 + a-2 + rollup 5");

        // ② 汇总按模型过滤（有效计价模型口径）。
        let deepseek = db.get_usage_summary(None, None, None, None, Some("deepseek-v3"))?;
        assert_eq!(deepseek.total_requests, 8, "b-1 + rollup 7");

        // ③ pricing_model 优先于 model：alias-model 查不到，real-model 查得到。
        let by_alias = db.get_usage_summary(None, None, None, None, Some("alias-model"))?;
        assert_eq!(by_alias.total_requests, 0);
        let by_real = db.get_usage_summary(None, None, None, None, Some("real-model"))?;
        assert_eq!(by_real.total_requests, 1);

        // ④ 会话占位行可按可读名选中。
        let session = db.get_usage_summary(None, None, None, Some("Claude (Session)"), None)?;
        assert_eq!(session.total_requests, 1);

        // ⑤ Provider 统计 + 模型过滤：只剩 DeepSeek 一行。
        let provider_stats = db.get_provider_stats(None, None, None, None, Some("deepseek-v3"))?;
        assert_eq!(provider_stats.len(), 1);
        assert_eq!(provider_stats[0].provider_name, "DeepSeek");
        assert_eq!(provider_stats[0].request_count, 8);

        // ⑥ 模型统计 + Provider 过滤：只剩 Packy 名下的模型。
        let model_stats = db.get_model_stats(None, None, None, Some("Packy"), None)?;
        let models: Vec<&str> = model_stats.iter().map(|m| m.model.as_str()).collect();
        assert!(models.contains(&"claude-sonnet-4-6"));
        assert!(models.contains(&"real-model"));
        assert!(!models.contains(&"deepseek-v3"));

        // ⑦ 分应用汇总（Hero 卡片数据源）同样受过滤影响。
        let by_app = db.get_usage_summary_by_app(None, None, Some("Packy"), None)?;
        assert_eq!(by_app.len(), 1);
        assert_eq!(by_app[0].app_type, "claude");
        assert_eq!(by_app[0].summary.total_requests, 7);

        // ⑧ 趋势（>24h 走天分桶 + rollup 分支）。
        let t_start = local_ts(2026, 6, 8, 0, 0, 0);
        let t_end = local_ts(2026, 6, 10, 23, 59, 0);
        let trends = db.get_daily_trends(Some(t_start), Some(t_end), None, Some("Packy"), None)?;
        let total_req: u64 = trends.iter().map(|d| d.request_count).sum();
        assert_eq!(total_req, 7, "明细 2 + rollup 5");

        // ⑨ 趋势 ≤24h 走小时分桶分支（?1/?2/?3 编号参数与追加过滤混用的路径），
        //    同时验证 Provider + 模型组合过滤。
        let h_start = local_ts(2026, 6, 10, 0, 0, 0);
        let h_end = local_ts(2026, 6, 10, 20, 0, 0);
        let hourly = db.get_daily_trends(
            Some(h_start),
            Some(h_end),
            None,
            Some("Packy"),
            Some("claude-sonnet-4-6"),
        )?;
        let hourly_req: u64 = hourly.iter().map(|d| d.request_count).sum();
        assert_eq!(hourly_req, 1, "仅 a-1 命中（a-2 计价模型不同）");

        // ⑩ 请求日志列表与下拉同口径：精确名 + 有效计价模型。
        let logs = db.get_request_logs(
            &LogFilters {
                provider_name: Some("Packy".to_string()),
                model: Some("real-model".to_string()),
                ..Default::default()
            },
            0,
            10,
        )?;
        assert_eq!(logs.total, 1);
        assert_eq!(logs.data[0].request_id, "a-2");

        Ok(())
    }

    #[test]
    fn test_get_usage_summary_includes_end_day_rollup_for_minute_precision_end_time(
    ) -> Result<(), AppError> {
        let db = Database::memory()?;
        let start = local_ts(2024, 1, 1, 0, 0, 0);
        let end = local_ts(2024, 1, 2, 23, 59, 0);

        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO usage_daily_rollups (
                    date, app_type, provider_id, model,
                    request_count, success_count, input_tokens, output_tokens,
                    cache_read_tokens, cache_creation_tokens, total_cost_usd, avg_latency_ms
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params![
                    "2024-01-01",
                    "claude",
                    "p1",
                    "claude-3",
                    10,
                    10,
                    1000,
                    500,
                    0,
                    0,
                    "1.00",
                    100
                ],
            )?;
            conn.execute(
                "INSERT INTO usage_daily_rollups (
                    date, app_type, provider_id, model,
                    request_count, success_count, input_tokens, output_tokens,
                    cache_read_tokens, cache_creation_tokens, total_cost_usd, avg_latency_ms
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params![
                    "2024-01-02",
                    "claude",
                    "p1",
                    "claude-3",
                    20,
                    19,
                    2000,
                    1000,
                    0,
                    0,
                    "2.00",
                    120
                ],
            )?;
        }

        let summary = db.get_usage_summary(Some(start), Some(end), Some("claude"), None, None)?;
        assert_eq!(summary.total_requests, 30);
        assert_eq!(summary.total_input_tokens, 3000);
        assert_eq!(summary.total_output_tokens, 1500);

        Ok(())
    }

    #[test]
    fn test_effective_usage_dedup_prefers_proxy_for_session_sources() -> Result<(), AppError> {
        let db = Database::memory()?;

        {
            let conn = lock_conn!(db.conn);
            insert_usage_log(
                &conn,
                "codex-proxy",
                "codex",
                "openai",
                "GPT-5.4",
                "proxy",
                10_000,
                100,
                20,
                10,
                7,
                200,
                "0.10",
            )?;
            insert_usage_log(
                &conn,
                "codex-session-dup",
                "codex",
                "_codex_session",
                "gpt-5.4",
                "codex_session",
                10_060,
                100,
                20,
                10,
                0,
                200,
                "0.10",
            )?;
            insert_usage_log(
                &conn,
                "claude-proxy",
                "claude",
                "openai-compatible",
                "claude-sonnet-4-5",
                "proxy",
                25_000,
                300,
                60,
                20,
                5,
                200,
                "0.30",
            )?;
            insert_usage_log(
                &conn,
                "claude-session-dup",
                "claude",
                "_session",
                "claude-sonnet-4-5",
                "session_log",
                25_060,
                300,
                60,
                20,
                5,
                200,
                "0.30",
            )?;
            insert_usage_log(
                &conn,
                "gemini-proxy",
                "gemini",
                "google",
                "gemini-2.5-pro",
                "proxy",
                20_000,
                200,
                40,
                30,
                0,
                200,
                "0.20",
            )?;
            insert_usage_log(
                &conn,
                "gemini-session-dup",
                "gemini",
                "_gemini_session",
                "gemini-2.5-pro",
                "gemini_session",
                20_060,
                200,
                40,
                30,
                0,
                200,
                "0.20",
            )?;
            insert_usage_log(
                &conn,
                "codex-session-only",
                "codex",
                "_codex_session",
                "gpt-5.4",
                "codex_session",
                30_000,
                50,
                5,
                0,
                0,
                200,
                "0.02",
            )?;
        }

        let summary = db.get_usage_summary(None, None, None, None, None)?;
        assert_eq!(summary.total_requests, 4);
        // codex-proxy contributes 100-10=90; gemini-proxy contributes 200-30=170
        // (input includes cache for both providers). claude-proxy=300, codex-session-only=50.
        // 90 + 170 + 300 + 50 = 610.
        assert_eq!(summary.total_input_tokens, 610);
        assert_eq!(summary.total_output_tokens, 125);
        assert_eq!(summary.total_cache_read_tokens, 60);
        assert_eq!(summary.total_cache_creation_tokens, 12);
        // real_total = fresh_input(610) + output(125) + cache_create(12) + cache_read(60) = 807
        assert_eq!(summary.real_total_tokens, 807);
        // hit_rate = 60 / (610 + 12 + 60) = 60 / 682
        let expected_hit_rate = 60.0_f64 / 682.0_f64;
        assert!((summary.cache_hit_rate - expected_hit_rate).abs() < 1e-9);

        let trends = db.get_daily_trends(Some(0), Some(40_000), None, None, None)?;
        assert_eq!(trends.iter().map(|stat| stat.request_count).sum::<u64>(), 4);

        let provider_stats = db.get_provider_stats(None, None, None, None, None)?;
        assert_eq!(
            provider_stats
                .iter()
                .map(|stat| stat.request_count)
                .sum::<u64>(),
            4
        );
        assert!(provider_stats
            .iter()
            .any(|stat| stat.provider_id == "_codex_session" && stat.request_count == 1));
        assert!(!provider_stats
            .iter()
            .any(|stat| stat.provider_id == "_gemini_session"));
        assert!(!provider_stats
            .iter()
            .any(|stat| stat.provider_id == "_session"));

        let model_stats = db.get_model_stats(None, None, None, None, None)?;
        assert_eq!(
            model_stats
                .iter()
                .map(|stat| stat.request_count)
                .sum::<u64>(),
            4
        );

        let logs = db.get_request_logs(&LogFilters::default(), 0, 10)?;
        let request_ids: Vec<&str> = logs
            .data
            .iter()
            .map(|log| log.request_id.as_str())
            .collect();
        assert_eq!(logs.total, 4);
        assert!(request_ids.contains(&"codex-proxy"));
        assert!(request_ids.contains(&"claude-proxy"));
        assert!(request_ids.contains(&"gemini-proxy"));
        assert!(request_ids.contains(&"codex-session-only"));
        assert!(!request_ids.contains(&"codex-session-dup"));
        assert!(!request_ids.contains(&"claude-session-dup"));
        assert!(!request_ids.contains(&"gemini-session-dup"));

        let breakdown = crate::services::session_usage::get_data_source_breakdown(&db)?;
        let proxy_count = breakdown
            .iter()
            .find(|item| item.data_source == "proxy")
            .map(|item| item.request_count);
        let codex_session_count = breakdown
            .iter()
            .find(|item| item.data_source == "codex_session")
            .map(|item| item.request_count);
        let gemini_session_count = breakdown
            .iter()
            .find(|item| item.data_source == "gemini_session")
            .map(|item| item.request_count);
        let session_log_count = breakdown
            .iter()
            .find(|item| item.data_source == "session_log")
            .map(|item| item.request_count);
        assert_eq!(proxy_count, Some(3));
        assert_eq!(codex_session_count, Some(1));
        assert_eq!(gemini_session_count, None);
        assert_eq!(session_log_count, None);

        Ok(())
    }

    #[test]
    fn test_effective_usage_dedup_keeps_non_matching_session_rows() -> Result<(), AppError> {
        let db = Database::memory()?;

        {
            let conn = lock_conn!(db.conn);
            insert_usage_log(
                &conn,
                "proxy-base",
                "codex",
                "openai",
                "gpt-5.4",
                "proxy",
                10_000,
                100,
                20,
                10,
                0,
                200,
                "0.10",
            )?;
            insert_usage_log(
                &conn,
                "session-outside-window",
                "codex",
                "_codex_session",
                "gpt-5.4",
                "codex_session",
                10_601,
                100,
                20,
                10,
                0,
                200,
                "0.10",
            )?;
            insert_usage_log(
                &conn,
                "session-token-mismatch",
                "codex",
                "_codex_session",
                "gpt-5.4",
                "codex_session",
                10_060,
                101,
                20,
                10,
                0,
                200,
                "0.10",
            )?;
            insert_usage_log(
                &conn,
                "session-app-mismatch",
                "gemini",
                "_gemini_session",
                "gpt-5.4",
                "gemini_session",
                10_060,
                100,
                20,
                10,
                0,
                200,
                "0.10",
            )?;
            insert_usage_log(
                &conn,
                "session-model-mismatch",
                "codex",
                "_codex_session",
                "different-model",
                "codex_session",
                10_060,
                100,
                20,
                10,
                0,
                200,
                "0.10",
            )?;
            insert_usage_log(
                &conn,
                "proxy-error",
                "codex",
                "openai",
                "gpt-5.4",
                "proxy",
                20_000,
                300,
                60,
                0,
                0,
                500,
                "0.00",
            )?;
            insert_usage_log(
                &conn,
                "session-matches-error-proxy",
                "codex",
                "_codex_session",
                "gpt-5.4",
                "codex_session",
                20_060,
                300,
                60,
                0,
                0,
                200,
                "0.30",
            )?;
            insert_usage_log(
                &conn,
                "claude-proxy-cache-creation",
                "claude",
                "anthropic",
                "claude-sonnet-4-5",
                "proxy",
                30_000,
                100,
                20,
                10,
                5,
                200,
                "0.10",
            )?;
            insert_usage_log(
                &conn,
                "claude-session-cache-creation-mismatch",
                "claude",
                "_session",
                "claude-sonnet-4-5",
                "session_log",
                30_060,
                100,
                20,
                10,
                0,
                200,
                "0.10",
            )?;
        }

        let summary = db.get_usage_summary(None, None, None, None, None)?;
        assert_eq!(summary.total_requests, 9);

        let logs = db.get_request_logs(&LogFilters::default(), 0, 10)?;
        let request_ids: Vec<&str> = logs
            .data
            .iter()
            .map(|log| log.request_id.as_str())
            .collect();
        assert_eq!(logs.total, 9);
        assert!(request_ids.contains(&"session-outside-window"));
        assert!(request_ids.contains(&"session-token-mismatch"));
        assert!(request_ids.contains(&"session-app-mismatch"));
        assert!(request_ids.contains(&"session-model-mismatch"));
        assert!(request_ids.contains(&"session-matches-error-proxy"));
        assert!(request_ids.contains(&"claude-session-cache-creation-mismatch"));

        Ok(())
    }

    #[test]
    fn test_get_model_stats() -> Result<(), AppError> {
        let db = Database::memory()?;

        // 插入测试数据
        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model,
                    input_tokens, output_tokens, total_cost_usd,
                    latency_ms, status_code, created_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params![
                    "req1",
                    "p1",
                    "claude",
                    "claude-3-sonnet",
                    100,
                    50,
                    "0.01",
                    100,
                    200,
                    1000
                ],
            )?;
        }

        let stats = db.get_model_stats(None, None, None, None, None)?;
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].model, "claude-3-sonnet");
        assert_eq!(stats[0].request_count, 1);

        Ok(())
    }

    #[test]
    fn test_get_provider_stats_with_time_filter() -> Result<(), AppError> {
        let db = Database::memory()?;

        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model,
                    input_tokens, output_tokens, total_cost_usd,
                    latency_ms, status_code, created_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params!["old", "p1", "claude", "claude-3", 100, 50, "0.01", 100, 200, 1000],
            )?;
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model,
                    input_tokens, output_tokens, total_cost_usd,
                    latency_ms, status_code, created_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params!["new", "p1", "claude", "claude-3", 200, 75, "0.02", 120, 200, 2000],
            )?;
        }

        let stats = db.get_provider_stats(Some(1500), Some(2500), Some("claude"), None, None)?;
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].provider_id, "p1");
        assert_eq!(stats[0].request_count, 1);
        assert_eq!(stats[0].total_tokens, 275);

        Ok(())
    }

    #[test]
    fn test_get_provider_stats_labels_opencode_session_provider() -> Result<(), AppError> {
        let db = Database::memory()?;

        {
            let conn = lock_conn!(db.conn);
            insert_usage_log(
                &conn,
                "opencode-session",
                "opencode",
                "_opencode_session",
                "opencode-model",
                "opencode_session",
                1000,
                100,
                50,
                0,
                0,
                200,
                "0.01",
            )?;
        }

        let stats = db.get_provider_stats(None, None, Some("opencode"), None, None)?;
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].provider_id, "_opencode_session");
        assert_eq!(stats[0].provider_name, "OpenCode (Session)");

        Ok(())
    }

    #[test]
    fn test_get_provider_stats_excludes_partial_rollup_boundary_days() -> Result<(), AppError> {
        let db = Database::memory()?;
        let start = local_ts(2024, 2, 1, 12, 0, 0);
        let end = local_ts(2024, 2, 3, 12, 0, 0);

        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO usage_daily_rollups (
                    date, app_type, provider_id, model,
                    request_count, success_count, input_tokens, output_tokens,
                    cache_read_tokens, cache_creation_tokens, total_cost_usd, avg_latency_ms
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params![
                    "2024-02-01",
                    "claude",
                    "p-rollup",
                    "claude-3",
                    5,
                    5,
                    500,
                    250,
                    0,
                    0,
                    "0.50",
                    100
                ],
            )?;
            conn.execute(
                "INSERT INTO usage_daily_rollups (
                    date, app_type, provider_id, model,
                    request_count, success_count, input_tokens, output_tokens,
                    cache_read_tokens, cache_creation_tokens, total_cost_usd, avg_latency_ms
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params![
                    "2024-02-02",
                    "claude",
                    "p-rollup",
                    "claude-3",
                    8,
                    7,
                    800,
                    400,
                    0,
                    0,
                    "0.80",
                    120
                ],
            )?;
            conn.execute(
                "INSERT INTO usage_daily_rollups (
                    date, app_type, provider_id, model,
                    request_count, success_count, input_tokens, output_tokens,
                    cache_read_tokens, cache_creation_tokens, total_cost_usd, avg_latency_ms
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params![
                    "2024-02-03",
                    "claude",
                    "p-rollup",
                    "claude-3",
                    12,
                    11,
                    1200,
                    600,
                    0,
                    0,
                    "1.20",
                    140
                ],
            )?;
        }

        let stats = db.get_provider_stats(Some(start), Some(end), Some("claude"), None, None)?;
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].provider_id, "p-rollup");
        assert_eq!(stats[0].request_count, 8);
        assert_eq!(stats[0].total_tokens, 1200);

        Ok(())
    }

    #[test]
    fn test_get_daily_trends_respects_shorter_than_24_hours() -> Result<(), AppError> {
        let db = Database::memory()?;

        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model,
                    input_tokens, output_tokens, total_cost_usd,
                    latency_ms, status_code, created_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params![
                    "req-short",
                    "p1",
                    "claude",
                    "claude-3",
                    100,
                    50,
                    "0.01",
                    100,
                    200,
                    10_800
                ],
            )?;
        }

        let stats = db.get_daily_trends(Some(0), Some(15 * 60 * 60), Some("claude"), None, None)?;
        assert_eq!(stats.len(), 15);
        assert_eq!(stats[3].request_count, 1);

        Ok(())
    }

    #[test]
    fn test_get_daily_trends_groups_ranges_longer_than_24_hours_by_local_day(
    ) -> Result<(), AppError> {
        let db = Database::memory()?;
        let start = local_ts(2024, 3, 1, 12, 0, 0);
        let end = local_ts(2024, 3, 3, 12, 0, 0);

        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model,
                    input_tokens, output_tokens, total_cost_usd,
                    latency_ms, status_code, created_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params![
                    "day-1-detail",
                    "p1",
                    "claude",
                    "claude-3",
                    100,
                    50,
                    "0.01",
                    100,
                    200,
                    local_ts(2024, 3, 1, 13, 0, 0)
                ],
            )?;
            conn.execute(
                "INSERT INTO proxy_request_logs (
                    request_id, provider_id, app_type, model,
                    input_tokens, output_tokens, total_cost_usd,
                    latency_ms, status_code, created_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params![
                    "day-3-detail",
                    "p1",
                    "claude",
                    "claude-3",
                    200,
                    75,
                    "0.02",
                    110,
                    200,
                    local_ts(2024, 3, 3, 10, 0, 0)
                ],
            )?;
            conn.execute(
                "INSERT INTO usage_daily_rollups (
                    date, app_type, provider_id, model,
                    request_count, success_count, input_tokens, output_tokens,
                    cache_read_tokens, cache_creation_tokens, total_cost_usd, avg_latency_ms
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params![
                    "2024-03-02",
                    "claude",
                    "p1",
                    "claude-3",
                    4,
                    4,
                    400,
                    200,
                    0,
                    0,
                    "0.40",
                    120
                ],
            )?;
        }

        let stats = db.get_daily_trends(Some(start), Some(end), Some("claude"), None, None)?;
        assert_eq!(stats.len(), 3);
        assert_eq!(stats[0].request_count, 1);
        assert_eq!(stats[0].total_tokens, 150);
        assert_eq!(stats[1].request_count, 4);
        assert_eq!(stats[1].total_tokens, 600);
        assert_eq!(stats[2].request_count, 1);
        assert_eq!(stats[2].total_tokens, 275);

        Ok(())
    }

    #[test]
    fn test_get_model_stats_excludes_partial_rollup_boundary_days() -> Result<(), AppError> {
        let db = Database::memory()?;
        let start = local_ts(2024, 4, 1, 12, 0, 0);
        let end = local_ts(2024, 4, 3, 12, 0, 0);

        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO usage_daily_rollups (
                    date, app_type, provider_id, model,
                    request_count, success_count, input_tokens, output_tokens,
                    cache_read_tokens, cache_creation_tokens, total_cost_usd, avg_latency_ms
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params![
                    "2024-04-01",
                    "claude",
                    "p1",
                    "claude-3-haiku",
                    6,
                    6,
                    600,
                    300,
                    0,
                    0,
                    "0.60",
                    100
                ],
            )?;
            conn.execute(
                "INSERT INTO usage_daily_rollups (
                    date, app_type, provider_id, model,
                    request_count, success_count, input_tokens, output_tokens,
                    cache_read_tokens, cache_creation_tokens, total_cost_usd, avg_latency_ms
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params![
                    "2024-04-02",
                    "claude",
                    "p1",
                    "claude-3-haiku",
                    9,
                    8,
                    900,
                    450,
                    0,
                    0,
                    "0.90",
                    110
                ],
            )?;
            conn.execute(
                "INSERT INTO usage_daily_rollups (
                    date, app_type, provider_id, model,
                    request_count, success_count, input_tokens, output_tokens,
                    cache_read_tokens, cache_creation_tokens, total_cost_usd, avg_latency_ms
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params![
                    "2024-04-03",
                    "claude",
                    "p1",
                    "claude-3-haiku",
                    12,
                    11,
                    1200,
                    600,
                    0,
                    0,
                    "1.20",
                    130
                ],
            )?;
        }

        let stats = db.get_model_stats(Some(start), Some(end), Some("claude"), None, None)?;
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].model, "claude-3-haiku");
        assert_eq!(stats[0].request_count, 9);
        assert_eq!(stats[0].total_tokens, 1350);

        Ok(())
    }

    fn local_dt(
        year: i32,
        month: u32,
        day: u32,
        hour: u32,
        minute: u32,
        second: u32,
    ) -> chrono::DateTime<Local> {
        Local
            .timestamp_opt(local_ts(year, month, day, hour, minute, second), 0)
            .single()
            .expect("valid local timestamp")
    }

    fn insert_limit_rollup(
        conn: &Connection,
        date: &str,
        provider_id: &str,
        model: &str,
        total_cost_usd: &str,
    ) -> Result<(), AppError> {
        conn.execute(
            "INSERT INTO usage_daily_rollups (
                date, app_type, provider_id, model,
                request_count, success_count, input_tokens, output_tokens,
                cache_read_tokens, cache_creation_tokens, total_cost_usd, avg_latency_ms
            ) VALUES (?, 'claude', ?, ?, 1, 1, 0, 0, 0, 0, ?, 100)",
            params![date, provider_id, model, total_cost_usd],
        )?;
        Ok(())
    }

    #[test]
    fn test_check_provider_limits_midnight_boundaries() -> Result<(), AppError> {
        let db = Database::memory()?;
        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO providers (id, app_type, name, settings_config, meta) VALUES
                 ('p1', 'claude', 'Limited', '{}',
                  '{\"limitDailyUsd\":\"1.50\",\"limitMonthlyUsd\":\"100\"}')",
                [],
            )?;
            let rows = [
                // 今日本地午夜整点：属于今天
                ("today-midnight", local_ts(2026, 3, 15, 0, 0, 0), "1.0"),
                // 前一秒：属于昨天（但仍在本月）
                ("yesterday-last", local_ts(2026, 3, 14, 23, 59, 59), "2.0"),
                // 明日午夜：日窗口为半开区间，排除
                ("tomorrow-midnight", local_ts(2026, 3, 16, 0, 0, 0), "4.0"),
                // 本月第一秒：属于本月
                ("month-first", local_ts(2026, 3, 1, 0, 0, 0), "8.0"),
                // 上月最后一秒：不属于本月
                ("prev-month-last", local_ts(2026, 2, 28, 23, 59, 59), "16.0"),
                // 下月第一秒：不属于本月
                ("next-month-first", local_ts(2026, 4, 1, 0, 0, 0), "32.0"),
            ];
            for (id, ts, cost) in rows {
                insert_usage_log(
                    &conn, id, "claude", "p1", "claude-3", "proxy", ts, 1, 1, 0, 0, 200, cost,
                )?;
            }
            // 其他 provider 不计入
            insert_usage_log(
                &conn,
                "other",
                "claude",
                "p2",
                "claude-3",
                "proxy",
                local_ts(2026, 3, 15, 12, 0, 0),
                1,
                1,
                0,
                0,
                200,
                "64.0",
            )?;
            insert_limit_rollup(&conn, "2026-03-15", "p1", "rollup-today", "0.5")?;
            insert_limit_rollup(&conn, "2026-03-01", "p1", "rollup-month", "0.125")?;
            insert_limit_rollup(&conn, "2026-02-28", "p1", "rollup-prev", "0.25")?;
            insert_limit_rollup(&conn, "2026-04-01", "p1", "rollup-next", "0.0625")?;
        }

        let status =
            db.check_provider_limits_at("p1", "claude", local_dt(2026, 3, 15, 12, 0, 0))?;
        assert_eq!(status.daily_usage, "1.500000");
        assert!(status.daily_exceeded, "1.5 >= 1.50 应判定超额");
        assert_eq!(status.monthly_usage, "15.625000");
        assert!(!status.monthly_exceeded);

        // 同一组数据，换到 3 月 14 日最后一秒：午夜行不再属于“今天”
        let status =
            db.check_provider_limits_at("p1", "claude", local_dt(2026, 3, 14, 23, 59, 59))?;
        assert_eq!(status.daily_usage, "2.000000");
        assert_eq!(status.monthly_usage, "15.625000");

        Ok(())
    }

    #[test]
    fn test_check_provider_limits_propagates_db_errors() -> Result<(), AppError> {
        let db = Database::memory()?;
        {
            let conn = lock_conn!(db.conn);
            insert_usage_log(
                &conn,
                "r1",
                "claude",
                "p1",
                "claude-3",
                "proxy",
                Local::now().timestamp(),
                1,
                1,
                0,
                0,
                200,
                "1.0",
            )?;
            conn.execute("DROP TABLE usage_daily_rollups", [])?;
        }

        let result = db.check_provider_limits("p1", "claude");
        assert!(
            matches!(result, Err(AppError::Database(_))),
            "数据库错误应返回 Err 而不是 $0 用量: {result:?}"
        );
        Ok(())
    }

    #[test]
    fn test_check_provider_limits_query_uses_provider_created_at_index() -> Result<(), AppError> {
        let db = Database::memory()?;
        let conn = lock_conn!(db.conn);
        let mut stmt = conn.prepare(&format!("EXPLAIN QUERY PLAN {PROVIDER_LIMIT_WINDOW_SQL}"))?;
        let plan: Vec<String> = stmt
            .query_map(
                params!["p1", "claude", 0_i64, 1_i64, "2026-01-01", "2026-01-02"],
                |row| row.get::<_, String>(3),
            )?
            .collect::<Result<_, _>>()?;
        let plan_text = plan.join("\n");
        assert!(
            plan.iter()
                .any(|detail| detail.contains("proxy_request_logs")
                    && detail.contains("idx_request_logs_provider_created_at")
                    && detail.contains("created_at>? AND created_at<?")),
            "明细查询应按 (provider_id, app_type, created_at) 索引 seek:\n{plan_text}"
        );
        assert!(
            !plan
                .iter()
                .any(|detail| detail.starts_with("SCAN proxy_request_logs")),
            "明细查询不应全表扫描:\n{plan_text}"
        );
        Ok(())
    }
}
