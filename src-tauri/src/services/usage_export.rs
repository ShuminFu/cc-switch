//! Usage export: request logs and per-period statements as CSV / JSON.
//!
//! Usage data is local-only (excluded from cloud sync) and detail rows are
//! pruned into daily rollups after 30 days, so a file export is the only way
//! to keep or share a statement.

use crate::database::Database;
use crate::error::AppError;
use crate::services::usage_stats::{LogFilters, RequestLogDetail};
use serde::Serialize;
use std::str::FromStr;

/// Hard cap on exported detail rows so a runaway filter cannot exhaust memory.
pub const MAX_EXPORT_ROWS: usize = 200_000;
const PAGE_SIZE: u32 = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsageExportFormat {
    Csv,
    Json,
}

impl UsageExportFormat {
    pub fn extension(self) -> &'static str {
        match self {
            UsageExportFormat::Csv => "csv",
            UsageExportFormat::Json => "json",
        }
    }

    pub fn dialog_label(self) -> &'static str {
        match self {
            UsageExportFormat::Csv => "CSV",
            UsageExportFormat::Json => "JSON",
        }
    }
}

impl FromStr for UsageExportFormat {
    type Err = AppError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "csv" => Ok(UsageExportFormat::Csv),
            "json" => Ok(UsageExportFormat::Json),
            other => Err(AppError::InvalidInput(format!(
                "Unsupported export format: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsageExportKind {
    /// Every request log row matching the filters.
    Logs,
    /// Summary plus per-provider and per-model statements for the range.
    Statement,
}

impl FromStr for UsageExportKind {
    type Err = AppError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "logs" => Ok(UsageExportKind::Logs),
            "statement" => Ok(UsageExportKind::Statement),
            other => Err(AppError::InvalidInput(format!(
                "Unsupported export kind: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageExportResult {
    pub path: String,
    pub rows: usize,
    pub truncated: bool,
}

/// RFC 4180 quoting: wrap when the field holds a separator, quote or newline.
pub fn csv_escape(value: &str) -> String {
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

fn csv_line<I, S>(fields: I) -> String
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut line = fields
        .into_iter()
        .map(|f| csv_escape(f.as_ref()))
        .collect::<Vec<_>>()
        .join(",");
    line.push('\n');
    line
}

fn format_local_timestamp(unix_seconds: i64) -> String {
    use chrono::TimeZone;
    chrono::Utc
        .timestamp_opt(unix_seconds, 0)
        .single()
        .map(|dt| dt.with_timezone(&chrono::Local).to_rfc3339())
        .unwrap_or_default()
}

const LOG_COLUMNS: &[&str] = &[
    "request_id",
    "created_at",
    "created_at_local",
    "app_type",
    "provider_id",
    "provider_name",
    "model",
    "request_model",
    "pricing_model",
    "status_code",
    "is_streaming",
    "input_tokens",
    "output_tokens",
    "cache_read_tokens",
    "cache_creation_tokens",
    "input_cost_usd",
    "output_cost_usd",
    "cache_read_cost_usd",
    "cache_creation_cost_usd",
    "total_cost_usd",
    "cost_multiplier",
    "latency_ms",
    "first_token_ms",
    "data_source",
    "error_message",
];

fn log_row(log: &RequestLogDetail) -> Vec<String> {
    let opt = |value: &Option<String>| value.clone().unwrap_or_default();
    vec![
        log.request_id.clone(),
        log.created_at.to_string(),
        format_local_timestamp(log.created_at),
        log.app_type.clone(),
        log.provider_id.clone(),
        opt(&log.provider_name),
        log.model.clone(),
        opt(&log.request_model),
        opt(&log.pricing_model),
        log.status_code.to_string(),
        log.is_streaming.to_string(),
        log.input_tokens.to_string(),
        log.output_tokens.to_string(),
        log.cache_read_tokens.to_string(),
        log.cache_creation_tokens.to_string(),
        log.input_cost_usd.clone(),
        log.output_cost_usd.clone(),
        log.cache_read_cost_usd.clone(),
        log.cache_creation_cost_usd.clone(),
        log.total_cost_usd.clone(),
        log.cost_multiplier.clone(),
        log.latency_ms.to_string(),
        log.first_token_ms
            .map(|v| v.to_string())
            .unwrap_or_default(),
        opt(&log.data_source),
        opt(&log.error_message),
    ]
}

/// Collect every matching log row (newest first, as the UI lists them), up to
/// `MAX_EXPORT_ROWS`. Returns the rows and whether the cap was hit.
fn collect_logs(
    db: &Database,
    filters: &LogFilters,
) -> Result<(Vec<RequestLogDetail>, bool), AppError> {
    let mut rows = Vec::new();
    let mut page = 0u32;
    loop {
        let batch = db.get_request_logs(filters, page, PAGE_SIZE)?;
        let fetched = batch.data.len();
        rows.extend(batch.data);
        if rows.len() >= MAX_EXPORT_ROWS {
            rows.truncate(MAX_EXPORT_ROWS);
            return Ok((rows, true));
        }
        if fetched < PAGE_SIZE as usize {
            return Ok((rows, false));
        }
        page += 1;
    }
}

/// Render the request logs matching `filters`. Returns (document, rows, truncated).
pub fn render_request_logs(
    db: &Database,
    filters: &LogFilters,
    format: UsageExportFormat,
) -> Result<(String, usize, bool), AppError> {
    let (rows, truncated) = collect_logs(db, filters)?;
    let count = rows.len();
    let document = match format {
        UsageExportFormat::Csv => {
            let mut out = csv_line(LOG_COLUMNS.iter().copied());
            for log in &rows {
                out.push_str(&csv_line(log_row(log)));
            }
            out
        }
        UsageExportFormat::Json => {
            let mut json = serde_json::to_string_pretty(&rows)
                .map_err(|e| AppError::Message(format!("Failed to serialize logs: {e}")))?;
            json.push('\n');
            json
        }
    };
    Ok((document, count, truncated))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StatementEnvelope {
    filters: StatementFilters,
    summary: crate::services::usage_stats::UsageSummary,
    providers: Vec<crate::services::usage_stats::ProviderStats>,
    models: Vec<crate::services::usage_stats::ModelStats>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StatementFilters {
    start_date: Option<i64>,
    end_date: Option<i64>,
    app_type: Option<String>,
    provider_name: Option<String>,
    model: Option<String>,
}

/// Render the summary / per-provider / per-model statement for the range.
/// Returns (document, rows = providers + models, truncated = false).
pub fn render_statement(
    db: &Database,
    filters: &LogFilters,
    format: UsageExportFormat,
) -> Result<(String, usize, bool), AppError> {
    let app_type = filters.app_type.as_deref();
    let provider_name = filters.provider_name.as_deref();
    let model = filters.model.as_deref();
    let summary = db.get_usage_summary(
        filters.start_date,
        filters.end_date,
        app_type,
        provider_name,
        model,
    )?;
    let providers = db.get_provider_stats(
        filters.start_date,
        filters.end_date,
        app_type,
        provider_name,
        model,
    )?;
    let models = db.get_model_stats(
        filters.start_date,
        filters.end_date,
        app_type,
        provider_name,
        model,
    )?;
    let rows = providers.len() + models.len();

    let document = match format {
        UsageExportFormat::Json => {
            let envelope = StatementEnvelope {
                filters: StatementFilters {
                    start_date: filters.start_date,
                    end_date: filters.end_date,
                    app_type: filters.app_type.clone(),
                    provider_name: filters.provider_name.clone(),
                    model: filters.model.clone(),
                },
                summary,
                providers,
                models,
            };
            let mut json = serde_json::to_string_pretty(&envelope)
                .map_err(|e| AppError::Message(format!("Failed to serialize statement: {e}")))?;
            json.push('\n');
            json
        }
        UsageExportFormat::Csv => {
            let mut out = String::new();
            out.push_str(&csv_line(["section", "key", "value"]));
            let range_start = filters
                .start_date
                .map(format_local_timestamp)
                .unwrap_or_default();
            let range_end = filters
                .end_date
                .map(format_local_timestamp)
                .unwrap_or_default();
            for (key, value) in [
                ("start", range_start),
                ("end", range_end),
                ("app_type", filters.app_type.clone().unwrap_or_default()),
                (
                    "provider_name",
                    filters.provider_name.clone().unwrap_or_default(),
                ),
                ("model", filters.model.clone().unwrap_or_default()),
                ("total_requests", summary.total_requests.to_string()),
                ("total_cost_usd", summary.total_cost.clone()),
                ("total_input_tokens", summary.total_input_tokens.to_string()),
                (
                    "total_output_tokens",
                    summary.total_output_tokens.to_string(),
                ),
                (
                    "total_cache_read_tokens",
                    summary.total_cache_read_tokens.to_string(),
                ),
                (
                    "total_cache_creation_tokens",
                    summary.total_cache_creation_tokens.to_string(),
                ),
                ("success_rate", format!("{:.4}", summary.success_rate)),
                ("cache_hit_rate", format!("{:.4}", summary.cache_hit_rate)),
            ] {
                out.push_str(&csv_line(["summary", key, value.as_str()]));
            }

            out.push('\n');
            out.push_str(&csv_line([
                "provider_id",
                "provider_name",
                "request_count",
                "total_tokens",
                "total_cost_usd",
                "success_rate",
                "avg_latency_ms",
            ]));
            for p in &providers {
                out.push_str(&csv_line([
                    p.provider_id.clone(),
                    p.provider_name.clone(),
                    p.request_count.to_string(),
                    p.total_tokens.to_string(),
                    p.total_cost.clone(),
                    format!("{:.4}", p.success_rate),
                    p.avg_latency_ms.to_string(),
                ]));
            }

            out.push('\n');
            out.push_str(&csv_line([
                "model",
                "request_count",
                "total_tokens",
                "total_cost_usd",
                "avg_cost_per_request_usd",
            ]));
            for m in &models {
                out.push_str(&csv_line([
                    m.model.clone(),
                    m.request_count.to_string(),
                    m.total_tokens.to_string(),
                    m.total_cost.clone(),
                    m.avg_cost_per_request.clone(),
                ]));
            }
            out
        }
    };
    Ok((document, rows, false))
}

pub fn render_export(
    db: &Database,
    kind: UsageExportKind,
    filters: &LogFilters,
    format: UsageExportFormat,
) -> Result<(String, usize, bool), AppError> {
    match kind {
        UsageExportKind::Logs => render_request_logs(db, filters, format),
        UsageExportKind::Statement => render_statement(db, filters, format),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proxy::usage::logger::{RequestLog, UsageLogger};
    use crate::proxy::usage::TokenUsage;

    fn seed(db: &Database, request_id: &str, model: &str, status: u16) {
        let logger = UsageLogger::new(db);
        let log = RequestLog {
            request_id: request_id.to_string(),
            provider_id: "relay".to_string(),
            app_type: "claude".to_string(),
            model: model.to_string(),
            request_model: model.to_string(),
            pricing_model: String::new(),
            usage: TokenUsage {
                input_tokens: 10,
                output_tokens: 5,
                ..TokenUsage::default()
            },
            cost: None,
            latency_ms: 42,
            first_token_ms: Some(7),
            status_code: status,
            error_message: None,
            session_id: None,
            provider_type: None,
            is_streaming: false,
            cost_multiplier: "1".to_string(),
        };
        logger.log_request(&log).expect("log request");
    }

    #[test]
    fn escapes_csv_fields() {
        assert_eq!(csv_escape("plain"), "plain");
        assert_eq!(csv_escape("a,b"), "\"a,b\"");
        assert_eq!(csv_escape("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(csv_escape("line\nbreak"), "\"line\nbreak\"");
    }

    #[test]
    fn parses_format_and_kind() {
        assert_eq!(
            "CSV".parse::<UsageExportFormat>().unwrap(),
            UsageExportFormat::Csv
        );
        assert_eq!(
            "json".parse::<UsageExportFormat>().unwrap(),
            UsageExportFormat::Json
        );
        assert!("xlsx".parse::<UsageExportFormat>().is_err());
        assert_eq!(
            "logs".parse::<UsageExportKind>().unwrap(),
            UsageExportKind::Logs
        );
        assert_eq!(
            "Statement".parse::<UsageExportKind>().unwrap(),
            UsageExportKind::Statement
        );
        assert!("pdf".parse::<UsageExportKind>().is_err());
    }

    #[test]
    fn exports_logs_as_csv_and_json() {
        let db = Database::memory().expect("memory db");
        seed(&db, "req-a", "model,with comma", 200);
        seed(&db, "req-b", "plain-model", 500);

        let filters = LogFilters::default();
        let (csv, rows, truncated) =
            render_request_logs(&db, &filters, UsageExportFormat::Csv).expect("csv");
        assert_eq!(rows, 2);
        assert!(!truncated);
        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(lines.len(), 3);
        assert!(lines[0].starts_with("request_id,created_at,created_at_local,app_type"));
        assert!(csv.contains("\"model,with comma\""));
        assert!(csv.contains("req-b"));

        let (json, rows, _) =
            render_request_logs(&db, &filters, UsageExportFormat::Json).expect("json");
        assert_eq!(rows, 2);
        let value: serde_json::Value = serde_json::from_str(&json).expect("valid json");
        let array = value.as_array().expect("array");
        assert_eq!(array.len(), 2);
        assert!(array
            .iter()
            .any(|row| row["requestId"] == "req-a" && row["model"] == "model,with comma"));
    }

    #[test]
    fn exports_statement_sections() {
        let db = Database::memory().expect("memory db");
        seed(&db, "req-a", "m1", 200);
        seed(&db, "req-b", "m2", 200);

        let filters = LogFilters {
            app_type: Some("claude".to_string()),
            ..LogFilters::default()
        };
        let (csv, rows, _) =
            render_statement(&db, &filters, UsageExportFormat::Csv).expect("csv statement");
        assert!(rows >= 3, "one provider + two models, got {rows}");
        assert!(csv.starts_with("section,key,value\n"));
        assert!(csv.contains("summary,total_requests,2"));
        assert!(csv.contains("provider_id,provider_name,request_count"));
        assert!(csv.contains("model,request_count,total_tokens"));
        assert!(csv.contains("\nm1,"));

        let (json, _, _) =
            render_statement(&db, &filters, UsageExportFormat::Json).expect("json statement");
        let value: serde_json::Value = serde_json::from_str(&json).expect("valid json");
        assert_eq!(value["summary"]["totalRequests"], 2);
        assert_eq!(value["filters"]["appType"], "claude");
        assert_eq!(value["models"].as_array().map(Vec::len), Some(2));
    }
}
