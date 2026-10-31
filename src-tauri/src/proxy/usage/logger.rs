//! Usage Logger - 记录 API 请求使用情况

use super::calculator::{CostBreakdown, CostCalculator, ModelPricing};
use super::parser::TokenUsage;
use crate::database::{Database, WriteJob, PRICING_SOURCE_REQUEST, PRICING_SOURCE_RESPONSE};
use crate::error::AppError;
use crate::services::sql_helpers::{INPUT_TOKEN_SEMANTICS_FRESH, INPUT_TOKEN_SEMANTICS_TOTAL};
use crate::services::usage_stats::{find_model_pricing_row, is_placeholder_pricing_model};
use rusqlite::Connection;
use rust_decimal::Decimal;
use std::str::FromStr;
use std::sync::Arc;

/// 请求日志
#[derive(Debug, Clone)]
pub struct RequestLog {
    pub request_id: String,
    pub provider_id: String,
    pub app_type: String,
    pub model: String,
    pub request_model: String,
    /// 写入时实际用于计价的模型名（pricing_model_source 解析后的结果）。
    /// 落库供回填使用：缺价行补价后必须按写入时的基准重算，而不是
    /// 用 model/request_model 猜——路由接管下三者可能各不相同。
    /// 错误行（未计价）为空字符串。
    pub pricing_model: String,
    pub usage: TokenUsage,
    pub cost: Option<CostBreakdown>,
    pub latency_ms: u64,
    pub first_token_ms: Option<u64>,
    pub status_code: u16,
    pub error_message: Option<String>,
    pub session_id: Option<String>,
    /// 供应商类型 (claude, claude_auth, codex, gemini, gemini_cli, openrouter)
    pub provider_type: Option<String>,
    /// 是否为流式请求
    pub is_streaming: bool,
    /// 成本倍数
    pub cost_multiplier: String,
}

/// 使用量记录器
///
/// 所有 SQLite 工作（定价查询、`INSERT OR REPLACE`）都在数据库的使用量写线程上
/// 批量执行（见 `database::usage_writer`）；这里的方法只做内存计算并非阻塞入队，
/// 不在调用方（Tokio worker / 请求路径）上获取数据库锁。测试或退出时调用
/// [`Database::flush_usage_writes`] 等待写入落库。
pub struct UsageLogger<'a> {
    db: &'a Arc<Database>,
}

const INSERT_REQUEST_LOG_SQL: &str = "INSERT OR REPLACE INTO proxy_request_logs (
    request_id, provider_id, app_type, model, request_model, pricing_model,
    input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
    input_token_semantics,
    input_cost_usd, output_cost_usd, cache_read_cost_usd, cache_creation_cost_usd, total_cost_usd,
    latency_ms, first_token_ms, status_code, error_message, session_id,
    provider_type, is_streaming, cost_multiplier, created_at
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25)";

/// 在给定连接上写入一条请求日志（使用缓存的预编译语句）。
fn insert_request_log(
    conn: &Connection,
    log: &RequestLog,
    created_at: i64,
) -> Result<(), AppError> {
    let (input_cost, output_cost, cache_read_cost, cache_creation_cost, total_cost) =
        if let Some(cost) = &log.cost {
            (
                cost.input_cost.to_string(),
                cost.output_cost.to_string(),
                cost.cache_read_cost.to_string(),
                cost.cache_creation_cost.to_string(),
                cost.total_cost.to_string(),
            )
        } else {
            (
                "0".to_string(),
                "0".to_string(),
                "0".to_string(),
                "0".to_string(),
                "0".to_string(),
            )
        };

    let input_token_semantics = if matches!(log.app_type.as_str(), "codex" | "gemini" | "grokbuild")
    {
        INPUT_TOKEN_SEMANTICS_TOTAL
    } else {
        INPUT_TOKEN_SEMANTICS_FRESH
    };

    let mut stmt = conn
        .prepare_cached(INSERT_REQUEST_LOG_SQL)
        .map_err(|e| AppError::Database(format!("记录请求日志失败: {e}")))?;
    stmt.execute(rusqlite::params![
        log.request_id,
        log.provider_id,
        log.app_type,
        log.model,
        log.request_model,
        log.pricing_model,
        log.usage.input_tokens,
        log.usage.output_tokens,
        log.usage.cache_read_tokens,
        log.usage.cache_creation_tokens,
        input_token_semantics,
        input_cost,
        output_cost,
        cache_read_cost,
        cache_creation_cost,
        total_cost,
        log.latency_ms as i64,
        log.first_token_ms.map(|v| v as i64),
        log.status_code as i64,
        log.error_message,
        log.session_id,
        log.provider_type,
        log.is_streaming as i64,
        log.cost_multiplier,
        created_at,
    ])
    .map_err(|e| AppError::Database(format!("记录请求日志失败: {e}")))?;
    Ok(())
}

/// 在给定连接上查询模型定价。
fn model_pricing_on_conn(
    conn: &Connection,
    model_id: &str,
) -> Result<Option<ModelPricing>, AppError> {
    match find_model_pricing_row(conn, model_id)? {
        Some((input, output, cache_read, cache_creation)) => {
            ModelPricing::from_strings(&input, &output, &cache_read, &cache_creation)
                .map(Some)
                .map_err(|e| AppError::Database(format!("解析定价数据失败: {e}")))
        }
        None => Ok(None),
    }
}

impl<'a> UsageLogger<'a> {
    pub fn new(db: &'a Arc<Database>) -> Self {
        Self { db }
    }

    /// 记录请求（非阻塞入队，由写线程批量写入）
    ///
    /// `created_at` 取入队时刻，而不是落库时刻。
    pub fn log_request(&self, log: &RequestLog) -> Result<(), AppError> {
        let created_at = chrono::Utc::now().timestamp();
        let log = log.clone();
        self.db.enqueue_usage_write(
            WriteJob::new("proxy_request_log", move |conn| {
                insert_request_log(conn, &log, created_at)
            })
            .notify_usage(),
        )
    }

    /// 记录失败的请求
    ///
    /// 用于记录无法从上游获取 usage 信息的失败请求
    #[allow(dead_code, clippy::too_many_arguments)]
    pub fn log_error(
        &self,
        request_id: String,
        provider_id: String,
        app_type: String,
        model: String,
        status_code: u16,
        error_message: String,
        latency_ms: u64,
    ) -> Result<(), AppError> {
        let request_model = model.clone();
        let log = RequestLog {
            request_id,
            provider_id,
            app_type,
            model,
            request_model,
            // 错误行未经过计价，留空（回填的 has_usage 闸门也不会碰全 0 行）
            pricing_model: String::new(),
            usage: TokenUsage::default(),
            cost: None,
            latency_ms,
            first_token_ms: None,
            status_code,
            error_message: Some(error_message),
            session_id: None,
            provider_type: None,
            is_streaming: false,
            cost_multiplier: "1.0".to_string(),
        };

        self.log_request(&log)
    }

    /// 记录失败的请求（带更多上下文信息）
    ///
    /// 相比 log_error，这个方法接受更多参数以提供完整的请求上下文
    #[allow(clippy::too_many_arguments)]
    pub fn log_error_with_context(
        &self,
        request_id: String,
        provider_id: String,
        app_type: String,
        model: String,
        status_code: u16,
        error_message: String,
        latency_ms: u64,
        is_streaming: bool,
        session_id: Option<String>,
        provider_type: Option<String>,
    ) -> Result<(), AppError> {
        let request_model = model.clone();
        let log = RequestLog {
            request_id,
            provider_id,
            app_type,
            model,
            request_model,
            // 错误行未经过计价，留空（回填的 has_usage 闸门也不会碰全 0 行）
            pricing_model: String::new(),
            usage: TokenUsage::default(),
            cost: None,
            latency_ms,
            first_token_ms: None,
            status_code,
            error_message: Some(error_message),
            session_id,
            provider_type,
            is_streaming,
            cost_multiplier: "1.0".to_string(),
        };

        self.log_request(&log)
    }

    /// 获取有效的倍率与计费模式来源（供应商优先，未配置则回退全局默认）
    pub async fn resolve_pricing_config(
        &self,
        provider_id: &str,
        app_type: &str,
    ) -> (Decimal, String) {
        // Claude Desktop 网关没有独立的全局计费配置（proxy_config 的 CHECK 仅
        // 允许 claude/codex/gemini，前端也只暴露三项），全局默认继承 claude；
        // 供应商级 meta 覆盖仍按 claude-desktop 查找（providers 表按该 app_type 存）。
        let default_app_type = if app_type == "claude-desktop" {
            "claude"
        } else {
            app_type
        };
        // 命中快照缓存时不访问数据库；未命中时在 spawn_blocking 上重建
        let default_snapshot = match self.db.proxy_request_snapshot_async(default_app_type).await {
            Ok(snapshot) => Some(snapshot),
            Err(e) => {
                log::warn!("[USG-003] 获取默认计费配置失败 (app_type={app_type}): {e}");
                None
            }
        };
        let default_multiplier_raw = match default_snapshot
            .as_ref()
            .and_then(|s| s.default_cost_multiplier.clone())
        {
            Some(value) => value,
            None => {
                log::warn!("[USG-003] 获取默认倍率失败 (app_type={app_type})");
                "1".to_string()
            }
        };
        let default_multiplier = match Decimal::from_str(&default_multiplier_raw) {
            Ok(value) => value,
            Err(e) => {
                log::warn!(
                    "[USG-003] 默认倍率解析失败 (app_type={app_type}): {default_multiplier_raw} - {e}"
                );
                Decimal::from(1)
            }
        };

        let default_pricing_source_raw = match default_snapshot
            .as_ref()
            .and_then(|s| s.default_pricing_model_source.clone())
        {
            Some(value) => value,
            None => {
                log::warn!("[USG-003] 获取默认计费模式失败 (app_type={app_type})");
                PRICING_SOURCE_RESPONSE.to_string()
            }
        };
        let default_pricing_source = if default_pricing_source_raw == PRICING_SOURCE_RESPONSE
            || default_pricing_source_raw == PRICING_SOURCE_REQUEST
        {
            default_pricing_source_raw
        } else {
            log::warn!(
                "[USG-003] 默认计费模式无效 (app_type={app_type}): {default_pricing_source_raw}"
            );
            PRICING_SOURCE_RESPONSE.to_string()
        };

        let app_snapshot = if app_type == default_app_type {
            default_snapshot
        } else {
            self.db.proxy_request_snapshot_async(app_type).await.ok()
        };
        let provider = app_snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.providers.get(provider_id));

        let (provider_multiplier, provider_pricing_source) = provider
            .as_ref()
            .and_then(|p| p.meta.as_ref())
            .map(|meta| {
                (
                    meta.cost_multiplier.as_deref(),
                    meta.pricing_model_source.as_deref(),
                )
            })
            .unwrap_or((None, None));

        let cost_multiplier = match provider_multiplier {
            Some(value) => match Decimal::from_str(value) {
                Ok(parsed) => parsed,
                Err(e) => {
                    log::warn!(
                        "[USG-003] 供应商倍率解析失败 (provider_id={provider_id}): {value} - {e}"
                    );
                    default_multiplier
                }
            },
            None => default_multiplier,
        };

        let pricing_model_source = match provider_pricing_source {
            Some(value) if value == PRICING_SOURCE_RESPONSE || value == PRICING_SOURCE_REQUEST => {
                value.to_string()
            }
            Some(value) => {
                log::warn!("[USG-003] 供应商计费模式无效 (provider_id={provider_id}): {value}");
                default_pricing_source.clone()
            }
            None => default_pricing_source.clone(),
        };

        (cost_multiplier, pricing_model_source)
    }

    /// 计算并记录请求
    #[allow(clippy::too_many_arguments)]
    pub fn log_with_calculation(
        &self,
        request_id: String,
        provider_id: String,
        app_type: String,
        model: String,
        request_model: String,
        pricing_model: String,
        usage: TokenUsage,
        cost_multiplier: Decimal,
        latency_ms: u64,
        first_token_ms: Option<u64>,
        status_code: u16,
        session_id: Option<String>,
        provider_type: Option<String>,
        is_streaming: bool,
    ) -> Result<(), AppError> {
        let has_usage = usage.input_tokens > 0
            || usage.output_tokens > 0
            || usage.cache_read_tokens > 0
            || usage.cache_creation_tokens > 0;
        let created_at = chrono::Utc::now().timestamp();

        let mut log = RequestLog {
            request_id,
            provider_id,
            app_type,
            model,
            request_model,
            pricing_model,
            usage,
            cost: None,
            latency_ms,
            first_token_ms,
            status_code,
            error_message: None,
            session_id,
            provider_type,
            is_streaming,
            cost_multiplier: cost_multiplier.to_string(),
        };

        // 定价查询与插入都在写线程的批事务里执行
        self.db.enqueue_usage_write(
            WriteJob::new("proxy_request_log_priced", move |conn| {
                let pricing = model_pricing_on_conn(conn, &log.pricing_model)?;
                if pricing.is_none()
                    && has_usage
                    && !is_placeholder_pricing_model(&log.pricing_model)
                {
                    log::warn!(
                        "[USG-002] 模型定价未找到，成本将记录为 0: {}",
                        log.pricing_model
                    );
                }
                log.cost = CostCalculator::try_calculate_for_app(
                    &log.app_type,
                    &log.usage,
                    pricing.as_ref(),
                    cost_multiplier,
                );
                insert_request_log(conn, &log, created_at)
            })
            .notify_usage(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_request() -> Result<(), AppError> {
        let db = Arc::new(Database::memory()?);

        // 插入测试定价
        {
            let conn = crate::database::lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO model_pricing (model_id, display_name, input_cost_per_million, output_cost_per_million)
                 VALUES ('test-model', 'Test Model', '3.0', '15.0')",
                [],
            )
            .unwrap();
        }

        let logger = UsageLogger::new(&db);

        let usage = TokenUsage {
            input_tokens: 1000,
            output_tokens: 500,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            model: None,
            message_id: None,
        };

        logger.log_with_calculation(
            "req-123".to_string(),
            "provider-1".to_string(),
            "claude".to_string(),
            "test-model".to_string(),
            "req-model".to_string(),
            "test-model".to_string(),
            usage,
            Decimal::from(1),
            100,
            None,
            200,
            None,
            Some("claude".to_string()),
            false,
        )?;

        // 验证记录已插入
        db.flush_usage_writes()?;
        let conn = crate::database::lock_conn!(db.conn);
        let (count, request_model): (i64, String) = conn
            .query_row(
                "SELECT COUNT(*), request_model FROM proxy_request_logs WHERE request_id = 'req-123'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(count, 1);
        assert_eq!(request_model, "req-model");
        Ok(())
    }

    #[test]
    fn test_log_error() -> Result<(), AppError> {
        let db = Arc::new(Database::memory()?);
        let logger = UsageLogger::new(&db);

        logger.log_error(
            "req-error".to_string(),
            "provider-1".to_string(),
            "claude".to_string(),
            "unknown-model".to_string(),
            500,
            "Internal Server Error".to_string(),
            50,
        )?;

        // 验证错误记录已插入
        db.flush_usage_writes()?;
        let conn = crate::database::lock_conn!(db.conn);
        let (status, error): (i64, Option<String>) = conn
            .query_row(
                "SELECT status_code, error_message FROM proxy_request_logs WHERE request_id = 'req-error'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(status, 500);
        assert_eq!(error, Some("Internal Server Error".to_string()));
        Ok(())
    }

    #[test]
    fn grokbuild_logs_total_input_token_semantics() -> Result<(), AppError> {
        let db = Arc::new(Database::memory()?);
        let logger = UsageLogger::new(&db);
        let log = RequestLog {
            request_id: "grok-semantics".to_string(),
            provider_id: "grok-provider".to_string(),
            app_type: "grokbuild".to_string(),
            model: "grok-4.5".to_string(),
            request_model: "grok-4.5".to_string(),
            pricing_model: String::new(),
            usage: TokenUsage::default(),
            cost: None,
            latency_ms: 1,
            first_token_ms: None,
            status_code: 200,
            error_message: None,
            session_id: None,
            provider_type: Some("grokbuild".to_string()),
            is_streaming: false,
            cost_multiplier: "1".to_string(),
        };

        logger.log_request(&log)?;

        db.flush_usage_writes()?;
        let conn = crate::database::lock_conn!(db.conn);
        let semantics: i64 = conn.query_row(
            "SELECT input_token_semantics FROM proxy_request_logs WHERE request_id = 'grok-semantics'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(semantics, INPUT_TOKEN_SEMANTICS_TOTAL);
        Ok(())
    }

    fn sample_log(request_id: &str) -> RequestLog {
        RequestLog {
            request_id: request_id.to_string(),
            provider_id: "provider-1".to_string(),
            app_type: "claude".to_string(),
            model: "m".to_string(),
            request_model: "m".to_string(),
            pricing_model: String::new(),
            usage: TokenUsage::default(),
            cost: None,
            latency_ms: 1,
            first_token_ms: None,
            status_code: 200,
            error_message: None,
            session_id: None,
            provider_type: None,
            is_streaming: false,
            cost_multiplier: "1".to_string(),
        }
    }

    /// PERF-03 / PROXY-04：记录日志不再在调用方线程上获取数据库锁。
    /// 若 log_request 仍同步获取锁，这里会在同一线程上死锁。
    #[test]
    fn log_request_does_not_touch_sqlite_on_caller_thread() -> Result<(), AppError> {
        let db = Arc::new(Database::memory()?);
        let logger = UsageLogger::new(&db);
        {
            let _held = db.conn.lock().unwrap();
            for i in 0..5 {
                logger.log_request(&sample_log(&format!("held-{i}")))?;
            }
            logger.log_with_calculation(
                "held-priced".to_string(),
                "provider-1".to_string(),
                "claude".to_string(),
                "m".to_string(),
                "m".to_string(),
                "m".to_string(),
                TokenUsage::default(),
                Decimal::from(1),
                1,
                None,
                200,
                None,
                None,
                false,
            )?;
        }
        db.flush_usage_writes()?;
        let conn = crate::database::lock_conn!(db.conn);
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM proxy_request_logs WHERE request_id LIKE 'held-%'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(count, 6);
        Ok(())
    }

    #[test]
    fn priced_log_is_costed_on_the_writer() -> Result<(), AppError> {
        let db = Arc::new(Database::memory()?);
        {
            let conn = crate::database::lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO model_pricing (model_id, display_name, input_cost_per_million, output_cost_per_million)
                 VALUES ('priced-model', 'Priced', '3.0', '15.0')",
                [],
            )?;
        }
        let logger = UsageLogger::new(&db);
        logger.log_with_calculation(
            "priced".to_string(),
            "provider-1".to_string(),
            "claude".to_string(),
            "priced-model".to_string(),
            "priced-model".to_string(),
            "priced-model".to_string(),
            TokenUsage {
                input_tokens: 1_000_000,
                output_tokens: 1_000_000,
                ..TokenUsage::default()
            },
            Decimal::from(2),
            1,
            None,
            200,
            None,
            None,
            false,
        )?;
        db.flush_usage_writes()?;
        let conn = crate::database::lock_conn!(db.conn);
        let total: String = conn.query_row(
            "SELECT total_cost_usd FROM proxy_request_logs WHERE request_id = 'priced'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(Decimal::from_str(&total).unwrap(), Decimal::from(36));
        Ok(())
    }

    /// PERF-05：计价配置来自缓存快照，供应商/全局配置写入后立即生效。
    #[tokio::test]
    async fn resolve_pricing_config_follows_config_writes() -> Result<(), AppError> {
        use crate::provider::{Provider, ProviderMeta};

        let db = Arc::new(Database::memory()?);
        let logger = UsageLogger::new(&db);

        let (multiplier, source) = logger.resolve_pricing_config("p1", "claude").await;
        assert_eq!(multiplier, Decimal::from(1));
        assert_eq!(source, PRICING_SOURCE_RESPONSE);

        db.set_default_cost_multiplier("claude", "1.5").await?;
        let (multiplier, _) = logger.resolve_pricing_config("p1", "claude").await;
        assert_eq!(multiplier, Decimal::from_str("1.5").unwrap());

        let mut provider = Provider::with_id(
            "p1".to_string(),
            "P1".to_string(),
            serde_json::json!({}),
            None,
        );
        provider.meta = Some(ProviderMeta {
            cost_multiplier: Some("2".to_string()),
            pricing_model_source: Some("request".to_string()),
            ..ProviderMeta::default()
        });
        db.save_provider("claude", &provider)?;
        let (multiplier, source) = logger.resolve_pricing_config("p1", "claude").await;
        assert_eq!(multiplier, Decimal::from(2));
        assert_eq!(source, PRICING_SOURCE_REQUEST);

        provider.meta.as_mut().unwrap().cost_multiplier = Some("3".to_string());
        db.save_provider("claude", &provider)?;
        let (multiplier, _) = logger.resolve_pricing_config("p1", "claude").await;
        assert_eq!(multiplier, Decimal::from(3));
        Ok(())
    }
}
