//! Usage Logger - 记录 API 请求使用情况
//!
//! 代理热路径不直接写库：请求结束时把一条 [`UsageLogEntry`] 投递给
//! [`super::writer::UsageLogWriter`]（单个后台线程，批量事务写入），
//! 定价配置与模型价格也在写线程中、在同一连接上查询。
//! [`UsageLogger`] 保留同步直写接口，供测试与非热路径使用。

use super::calculator::{CostBreakdown, CostCalculator, ModelPricing};
use super::parser::TokenUsage;
#[cfg(test)]
use crate::database::Database;
use crate::database::{PRICING_SOURCE_REQUEST, PRICING_SOURCE_RESPONSE};
use crate::error::AppError;
use crate::provider::ProviderMeta;
use crate::services::sql_helpers::{INPUT_TOKEN_SEMANTICS_FRESH, INPUT_TOKEN_SEMANTICS_TOTAL};
use crate::services::usage_stats::{find_model_pricing_row, is_placeholder_pricing_model};
use rusqlite::{Connection, OptionalExtension};
use rust_decimal::Decimal;
use std::str::FromStr;

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

impl RequestLog {
    /// 构造一条失败请求日志（无 usage、未计价）
    #[allow(clippy::too_many_arguments)]
    pub fn error(
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
    ) -> Self {
        let request_model = model.clone();
        Self {
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
        }
    }
}

/// 一次代理请求的原始使用量，计价（倍率、计费模式、模型价格）推迟到写线程中完成。
#[derive(Debug, Clone)]
pub(crate) struct ProxyUsage {
    pub request_id: String,
    pub provider_id: String,
    pub app_type: String,
    /// 上游回显的模型（「按响应计价」锚点）
    pub model: String,
    /// 客户端请求的模型
    pub request_model: String,
    /// 实际发往上游的模型（「按请求计价」锚点）
    pub outbound_model: String,
    pub usage: TokenUsage,
    pub latency_ms: u64,
    pub first_token_ms: Option<u64>,
    pub status_code: u16,
    pub session_id: Option<String>,
    pub provider_type: Option<String>,
    pub is_streaming: bool,
}

/// 投递给后台写线程的一条使用量日志
#[derive(Debug, Clone)]
pub(crate) enum UsageLogEntry {
    /// 已经完整的行（例如失败请求）
    Ready(RequestLog),
    /// 需要在写线程中计价的代理请求
    Proxy(ProxyUsage),
}

/// 有效的倍率与计费模式来源（供应商优先，未配置则回退全局默认）。
///
/// 只做只读查询，可以在写线程的事务内对同一连接调用。
pub(crate) fn resolve_pricing_config_on_conn(
    conn: &Connection,
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

    let defaults: Option<(String, String)> = match conn
        .query_row(
            "SELECT default_cost_multiplier, pricing_model_source
             FROM proxy_config WHERE app_type = ?1",
            [default_app_type],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
    {
        Ok(v) => v,
        Err(e) => {
            log::warn!("[USG-003] 获取默认计费配置失败 (app_type={app_type}): {e}");
            None
        }
    };
    let (default_multiplier_raw, default_pricing_source_raw) =
        defaults.unwrap_or_else(|| ("1".to_string(), PRICING_SOURCE_RESPONSE.to_string()));

    let default_multiplier = match Decimal::from_str(&default_multiplier_raw) {
        Ok(value) => value,
        Err(e) => {
            log::warn!(
                "[USG-003] 默认倍率解析失败 (app_type={app_type}): {default_multiplier_raw} - {e}"
            );
            Decimal::from(1)
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

    let meta: Option<ProviderMeta> = conn
        .query_row(
            "SELECT meta FROM providers WHERE id = ?1 AND app_type = ?2",
            [provider_id, app_type],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .ok()
        .flatten()
        .map(|meta_str| serde_json::from_str(&meta_str).unwrap_or_default());

    let (provider_multiplier, provider_pricing_source) = meta
        .as_ref()
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
            default_pricing_source
        }
        None => default_pricing_source,
    };

    (cost_multiplier, pricing_model_source)
}

/// 在给定连接上查询模型定价
pub(crate) fn model_pricing_on_conn(
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

/// 按模型价格计算成本并构造日志行
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_calculated_log(
    conn: &Connection,
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
) -> Result<RequestLog, AppError> {
    let pricing = model_pricing_on_conn(conn, &pricing_model)?;

    let has_usage = usage.input_tokens > 0
        || usage.output_tokens > 0
        || usage.cache_read_tokens > 0
        || usage.cache_creation_tokens > 0;

    if pricing.is_none() && has_usage && !is_placeholder_pricing_model(&pricing_model) {
        log::warn!("[USG-002] 模型定价未找到，成本将记录为 0: {pricing_model}");
    }

    let cost =
        CostCalculator::try_calculate_for_app(&app_type, &usage, pricing.as_ref(), cost_multiplier);

    Ok(RequestLog {
        request_id,
        provider_id,
        app_type,
        model,
        request_model,
        pricing_model,
        usage,
        cost,
        latency_ms,
        first_token_ms,
        status_code,
        error_message: None,
        session_id,
        provider_type,
        is_streaming,
        cost_multiplier: cost_multiplier.to_string(),
    })
}

/// 解析计价配置、计价并构造代理请求的日志行（写线程使用）
pub(crate) fn price_proxy_usage(
    conn: &Connection,
    entry: ProxyUsage,
    pricing_config: (Decimal, String),
) -> Result<RequestLog, AppError> {
    let (multiplier, pricing_model_source) = pricing_config;
    let pricing_model = if pricing_model_source == PRICING_SOURCE_REQUEST {
        entry.outbound_model
    } else {
        entry.model.clone()
    };
    build_calculated_log(
        conn,
        entry.request_id,
        entry.provider_id,
        entry.app_type,
        entry.model,
        entry.request_model,
        pricing_model,
        entry.usage,
        multiplier,
        entry.latency_ms,
        entry.first_token_ms,
        entry.status_code,
        entry.session_id,
        entry.provider_type,
        entry.is_streaming,
    )
}

/// 插入一条请求日志（不发送前端通知，由调用方在提交后统一通知）
pub(crate) fn insert_request_log(conn: &Connection, log: &RequestLog) -> Result<(), AppError> {
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

    let created_at = chrono::Utc::now().timestamp();
    let input_token_semantics = if matches!(log.app_type.as_str(), "codex" | "gemini" | "grokbuild")
    {
        INPUT_TOKEN_SEMANTICS_TOTAL
    } else {
        INPUT_TOKEN_SEMANTICS_FRESH
    };

    conn.prepare_cached(
        "INSERT OR REPLACE INTO proxy_request_logs (
            request_id, provider_id, app_type, model, request_model, pricing_model,
            input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
            input_token_semantics,
            input_cost_usd, output_cost_usd, cache_read_cost_usd, cache_creation_cost_usd, total_cost_usd,
            latency_ms, first_token_ms, status_code, error_message, session_id,
            provider_type, is_streaming, cost_multiplier, created_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25)",
    )
    .and_then(|mut stmt| {
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
    })
    .map_err(|e| AppError::Database(format!("记录请求日志失败: {e}")))?;

    Ok(())
}

/// 使用量记录器（同步直写，绕过后台写线程）
///
/// 生产路径统一走 [`Database::enqueue_usage_log`]；这里保留同步接口供测试
/// 直接写入并立即断言。
#[cfg(test)]
pub struct UsageLogger<'a> {
    db: &'a Database,
}

#[cfg(test)]
impl<'a> UsageLogger<'a> {
    pub fn new(db: &'a Database) -> Self {
        Self { db }
    }

    /// 记录成功的请求
    pub fn log_request(&self, log: &RequestLog) -> Result<(), AppError> {
        {
            let conn = crate::database::lock_conn!(self.db.conn);
            insert_request_log(&conn, log)?;
        }

        // 通知前端使用统计有更新（200ms 防抖合并，不阻塞写入路径）
        crate::usage_events::notify_log_recorded();

        Ok(())
    }

    /// 记录失败的请求
    ///
    /// 用于记录无法从上游获取 usage 信息的失败请求
    #[allow(clippy::too_many_arguments)]
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
        self.log_request(&RequestLog::error(
            request_id,
            provider_id,
            app_type,
            model,
            status_code,
            error_message,
            latency_ms,
            false,
            None,
            None,
        ))
    }

    /// 记录失败的请求（带更多上下文信息）
    ///
    /// 相比 log_error，这个方法接受更多参数以提供完整的请求上下文
    #[allow(dead_code, clippy::too_many_arguments)]
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
        self.log_request(&RequestLog::error(
            request_id,
            provider_id,
            app_type,
            model,
            status_code,
            error_message,
            latency_ms,
            is_streaming,
            session_id,
            provider_type,
        ))
    }

    /// 获取有效的倍率与计费模式来源（供应商优先，未配置则回退全局默认）
    pub fn resolve_pricing_config(&self, provider_id: &str, app_type: &str) -> (Decimal, String) {
        match self.db.read_conn() {
            Ok(conn) => resolve_pricing_config_on_conn(&conn, provider_id, app_type),
            Err(e) => {
                log::warn!("[USG-003] 获取计费配置失败 (app_type={app_type}): {e}");
                (Decimal::from(1), PRICING_SOURCE_RESPONSE.to_string())
            }
        }
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
        let log = {
            let conn = crate::database::lock_conn!(self.db.conn);
            build_calculated_log(
                &conn,
                request_id,
                provider_id,
                app_type,
                model,
                request_model,
                pricing_model,
                usage,
                cost_multiplier,
                latency_ms,
                first_token_ms,
                status_code,
                session_id,
                provider_type,
                is_streaming,
            )?
        };
        self.log_request(&log)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_request() -> Result<(), AppError> {
        let db = Database::memory()?;

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
        let db = Database::memory()?;
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
        let db = Database::memory()?;
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

        let conn = crate::database::lock_conn!(db.conn);
        let semantics: i64 = conn.query_row(
            "SELECT input_token_semantics FROM proxy_request_logs WHERE request_id = 'grok-semantics'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(semantics, INPUT_TOKEN_SEMANTICS_TOTAL);
        Ok(())
    }
}
