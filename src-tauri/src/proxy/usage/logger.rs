//! Usage Logger - 记录 API 请求使用情况

use super::calculator::{CostBreakdown, CostCalculator, ModelPricing};
use super::parity::{check_usage_parity, encode_log_metadata};
use super::parser::TokenUsage;
use crate::database::{Database, PRICING_SOURCE_REQUEST, PRICING_SOURCE_RESPONSE};
use crate::error::AppError;
use crate::proxy::runtime_config::RuntimeConfigStore;
use crate::services::sql_helpers::{INPUT_TOKEN_SEMANTICS_FRESH, INPUT_TOKEN_SEMANTICS_TOTAL};
use crate::services::usage_stats::{find_model_pricing_row, is_placeholder_pricing_model};
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

/// 使用量记录器
pub struct UsageLogger<'a> {
    db: &'a Database,
    /// 代理请求路径上使用运行时配置快照读取计费配置与定价，避免每个请求查库
    runtime_config: Option<&'a RuntimeConfigStore>,
    /// 代理请求路径上开启 usage parity 检查：`Some(raw)` 为上游原始 usage 块（可缺失）
    parity_raw_usage: Option<Option<serde_json::Value>>,
}

impl<'a> UsageLogger<'a> {
    pub fn new(db: &'a Database) -> Self {
        Self {
            db,
            runtime_config: None,
            parity_raw_usage: None,
        }
    }

    /// 计费配置、供应商与模型定价改从运行时配置快照读取（快照随配置写入自动失效）
    pub fn with_runtime_config(mut self, runtime_config: &'a RuntimeConfigStore) -> Self {
        self.runtime_config = Some(runtime_config);
        self
    }

    /// 对成功行做 usage parity 检查（规范化 usage 对比上游原始 usage 块），
    /// 不一致时把标记写入该行的 JSON 元数据，见 [`super::parity`]
    pub fn with_usage_parity(mut self, raw_usage: Option<serde_json::Value>) -> Self {
        self.parity_raw_usage = Some(raw_usage);
        self
    }

    /// 记录成功的请求
    pub fn log_request(&self, log: &RequestLog) -> Result<(), AppError> {
        let conn = crate::database::lock_conn!(self.db.conn);

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
        let input_token_semantics =
            if matches!(log.app_type.as_str(), "codex" | "gemini" | "grokbuild") {
                INPUT_TOKEN_SEMANTICS_TOTAL
            } else {
                INPUT_TOKEN_SEMANTICS_FRESH
            };
        // 非流式响应完全没有 usage 块时不是生成类响应（如 Gemini countTokens、
        // GET models），不做 parity 检查；流缺 usage 仍按全 0 打标。
        let error_message = match &self.parity_raw_usage {
            Some(raw_usage)
                if log.error_message.is_none()
                    && (200..300).contains(&log.status_code)
                    && (raw_usage.is_some() || log.is_streaming) =>
            {
                check_usage_parity(
                    &log.usage,
                    input_token_semantics == INPUT_TOKEN_SEMANTICS_TOTAL,
                    raw_usage.as_ref(),
                )
                .map(|parity| {
                    log::warn!(
                        "[USG-004] usage 与上游原始 usage 不一致: id={}, {parity:?}",
                        log.request_id
                    );
                    encode_log_metadata(&parity)
                })
            }
            _ => log.error_message.clone(),
        };

        conn.execute(
            "INSERT OR REPLACE INTO proxy_request_logs (
                request_id, provider_id, app_type, model, request_model, pricing_model,
                input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                input_token_semantics,
                input_cost_usd, output_cost_usd, cache_read_cost_usd, cache_creation_cost_usd, total_cost_usd,
                latency_ms, first_token_ms, status_code, error_message, session_id,
                provider_type, is_streaming, cost_multiplier, created_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25)",
            rusqlite::params![
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
                error_message,
                log.session_id,
                log.provider_type,
                log.is_streaming as i64,
                log.cost_multiplier,
                created_at,
            ],
        )
        .map_err(|e| AppError::Database(format!("记录请求日志失败: {e}")))?;

        // 通知前端使用统计有更新（200ms 防抖合并，不阻塞写入路径）
        crate::usage_events::notify_log_recorded();

        Ok(())
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

    /// 获取模型定价
    pub fn get_model_pricing(&self, model_id: &str) -> Result<Option<ModelPricing>, AppError> {
        match self.runtime_config {
            Some(runtime_config) => {
                runtime_config.model_pricing(model_id, || self.load_model_pricing(model_id))
            }
            None => self.load_model_pricing(model_id),
        }
    }

    fn load_model_pricing(&self, model_id: &str) -> Result<Option<ModelPricing>, AppError> {
        let conn = crate::database::lock_conn!(self.db.conn);
        let row = find_model_pricing_row(&conn, model_id)?;
        match row {
            Some((input, output, cache_read, cache_creation)) => {
                ModelPricing::from_strings(&input, &output, &cache_read, &cache_creation)
                    .map(Some)
                    .map_err(|e| AppError::Database(format!("解析定价数据失败: {e}")))
            }
            None => Ok(None),
        }
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
        let default_multiplier = match self.runtime_config {
            Some(runtime_config) => {
                runtime_config
                    .default_cost_multiplier(default_app_type)
                    .await
            }
            None => self.db.get_default_cost_multiplier(default_app_type).await,
        };
        let default_multiplier_raw = match default_multiplier {
            Ok(value) => value,
            Err(e) => {
                log::warn!("[USG-003] 获取默认倍率失败 (app_type={app_type}): {e}");
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

        let default_pricing_source = match self.runtime_config {
            Some(runtime_config) => runtime_config.pricing_model_source(default_app_type).await,
            None => self.db.get_pricing_model_source(default_app_type).await,
        };
        let default_pricing_source_raw = match default_pricing_source {
            Ok(value) => value,
            Err(e) => {
                log::warn!("[USG-003] 获取默认计费模式失败 (app_type={app_type}): {e}");
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

        let provider = match self.runtime_config {
            Some(runtime_config) => runtime_config
                .app_providers(app_type)
                .ok()
                .and_then(|providers| providers.all.get(provider_id).cloned()),
            None => self
                .db
                .get_provider_by_id(provider_id, app_type)
                .ok()
                .flatten(),
        };

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
        let pricing = self.get_model_pricing(&pricing_model)?;

        let has_usage = usage.input_tokens > 0
            || usage.output_tokens > 0
            || usage.cache_read_tokens > 0
            || usage.cache_creation_tokens > 0;

        if pricing.is_none() && has_usage && !is_placeholder_pricing_model(&pricing_model) {
            log::warn!("[USG-002] 模型定价未找到，成本将记录为 0: {pricing_model}");
        }

        let cost = CostCalculator::try_calculate_for_app(
            &app_type,
            &usage,
            pricing.as_ref(),
            cost_multiplier,
        );

        let log = RequestLog {
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

    fn parity_log(request_id: &str, app_type: &str, usage: TokenUsage) -> RequestLog {
        RequestLog {
            request_id: request_id.to_string(),
            provider_id: "provider-1".to_string(),
            app_type: app_type.to_string(),
            model: "test-model".to_string(),
            request_model: "test-model".to_string(),
            pricing_model: "test-model".to_string(),
            usage,
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

    fn logged_row(
        db: &Database,
        request_id: &str,
    ) -> Result<crate::services::usage_stats::RequestLogDetail, AppError> {
        let logs = db.get_request_logs(&Default::default(), 0, 100)?;
        Ok(logs
            .data
            .into_iter()
            .find(|log| log.request_id == request_id)
            .expect("logged row"))
    }

    #[test]
    fn inconsistent_upstream_usage_sets_parity_flag_on_log_row() -> Result<(), AppError> {
        use crate::proxy::usage::ir::UsageIR;
        use crate::proxy::usage::parity::UsageParityIssue;

        let db = Database::memory()?;
        // 故意不一致的上游 usage：cached_tokens 大于 prompt_tokens（缓存被重复计数）
        let inconsistent = serde_json::json!({
            "prompt_tokens": 100,
            "completion_tokens": 20,
            "total_tokens": 120,
            "prompt_tokens_details": {"cached_tokens": 150}
        });
        let usage = UsageIR::from_chat_completions(&inconsistent).to_token_usage();
        UsageLogger::new(&db)
            .with_usage_parity(Some(inconsistent))
            .log_request(&parity_log("parity-bad", "codex", usage))?;

        let consistent = serde_json::json!({
            "prompt_tokens": 100,
            "completion_tokens": 20,
            "total_tokens": 120,
            "prompt_tokens_details": {"cached_tokens": 40}
        });
        let usage = UsageIR::from_chat_completions(&consistent).to_token_usage();
        UsageLogger::new(&db)
            .with_usage_parity(Some(consistent))
            .log_request(&parity_log("parity-ok", "codex", usage))?;

        // 未开启 parity 检查（如会话导入）时全 0 也不打标
        UsageLogger::new(&db).log_request(&parity_log(
            "parity-off",
            "claude",
            TokenUsage::default(),
        ))?;
        // 代理路径上成功但 usage 全 0 的流
        let mut zero_stream = parity_log("parity-zero", "claude", TokenUsage::default());
        zero_stream.is_streaming = true;
        UsageLogger::new(&db)
            .with_usage_parity(None)
            .log_request(&zero_stream)?;
        // 非流式且无 usage 块（如 Gemini countTokens / GET models）不打标
        UsageLogger::new(&db)
            .with_usage_parity(None)
            .log_request(&parity_log(
                "parity-no-usage-block",
                "gemini",
                TokenUsage::default(),
            ))?;
        // 非流式但 usage 块全 0 仍打标
        UsageLogger::new(&db)
            .with_usage_parity(Some(serde_json::json!({
                "promptTokenCount": 0,
                "totalTokenCount": 0
            })))
            .log_request(&parity_log(
                "parity-zero-block",
                "gemini",
                TokenUsage::default(),
            ))?;

        let bad = logged_row(&db, "parity-bad")?;
        let parity = bad
            .usage_parity
            .expect("inconsistent usage must be flagged");
        assert_eq!(parity.issue, UsageParityIssue::CacheExceedsInput);
        assert_eq!(parity.normalized_total, 170);
        assert_eq!(parity.raw_total, Some(120));
        assert_eq!(bad.error_message, None);

        assert_eq!(logged_row(&db, "parity-ok")?.usage_parity, None);
        assert_eq!(logged_row(&db, "parity-off")?.usage_parity, None);
        assert_eq!(
            logged_row(&db, "parity-zero")?
                .usage_parity
                .map(|p| p.issue),
            Some(UsageParityIssue::ZeroUsage)
        );
        assert_eq!(logged_row(&db, "parity-no-usage-block")?.usage_parity, None);
        assert_eq!(
            logged_row(&db, "parity-zero-block")?
                .usage_parity
                .map(|p| p.issue),
            Some(UsageParityIssue::ZeroUsage)
        );
        Ok(())
    }

    #[test]
    fn parity_check_never_overwrites_error_rows() -> Result<(), AppError> {
        let db = Database::memory()?;
        let mut log = parity_log("parity-error", "claude", TokenUsage::default());
        log.status_code = 500;
        log.error_message = Some("boom".to_string());
        UsageLogger::new(&db)
            .with_usage_parity(None)
            .log_request(&log)?;
        let detail = logged_row(&db, "parity-error")?;
        assert_eq!(detail.error_message.as_deref(), Some("boom"));
        assert_eq!(detail.usage_parity, None);
        Ok(())
    }
}
