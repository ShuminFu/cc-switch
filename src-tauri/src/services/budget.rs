//! Provider budget limits: optional hard-limit enforcement (PRODUCT-05).
//!
//! Daily / monthly USD limits live in provider meta (`limitDailyUsd` /
//! `limitMonthlyUsd`) and are summed by `Database::check_provider_limits`.
//! By default limits are informational only (progress on the usage dashboard
//! and provider card). When the user turns on enforcement, the proxy calls
//! [`check_hard_limit`] before sending a request to a provider and refuses (or
//! fails over past) a provider whose limit has been reached.

use crate::database::Database;
use crate::error::AppError;
use crate::services::usage_stats::ProviderLimitStatus;

/// Key in the generic `settings` table holding the enforcement switch.
pub const BUDGET_ENFORCEMENT_KEY: &str = "budget_hard_limit_enforcement";

/// Stable i18n key carried by the localized error returned on a breach.
pub const BUDGET_EXCEEDED_ERROR_KEY: &str = "budget.hardLimitExceeded";

/// Whether hard-limit enforcement is turned on (off by default).
pub fn is_enforcement_enabled(db: &Database) -> Result<bool, AppError> {
    db.get_bool_flag(BUDGET_ENFORCEMENT_KEY)
}

/// Turn hard-limit enforcement on or off.
pub fn set_enforcement_enabled(db: &Database, enabled: bool) -> Result<(), AppError> {
    db.set_setting(
        BUDGET_ENFORCEMENT_KEY,
        if enabled { "true" } else { "false" },
    )
}

/// Build the user-facing error for an exceeded limit, or `None` when the
/// provider is still within its limits.
pub fn exceeded_error(status: &ProviderLimitStatus) -> Option<AppError> {
    let (period_zh, period_en, usage, limit) = if status.daily_exceeded {
        (
            "每日",
            "daily",
            &status.daily_usage,
            status.daily_limit.as_deref().unwrap_or("?"),
        )
    } else if status.monthly_exceeded {
        (
            "每月",
            "monthly",
            &status.monthly_usage,
            status.monthly_limit.as_deref().unwrap_or("?"),
        )
    } else {
        return None;
    };

    let usage = usage
        .parse::<f64>()
        .map(|v| format!("{v:.2}"))
        .unwrap_or_else(|_| usage.clone());

    Some(AppError::localized(
        BUDGET_EXCEEDED_ERROR_KEY,
        format!(
            "供应商 {} 已达到{period_zh}消费上限（${usage} / ${limit}），CC Switch 已按预算设置拦截该请求。可在用量面板调整上限或关闭强制模式。",
            status.provider_id
        ),
        format!(
            "Provider {} has reached its {period_en} budget limit (${usage} of ${limit}); CC Switch blocked this request because budget enforcement is on. Raise the limit or turn off enforcement in the Usage dashboard.",
            status.provider_id
        ),
    ))
}

/// Check whether a request to `provider_id` (of `app_type`) may proceed.
///
/// Returns `Ok(())` when enforcement is off, when the provider has no limits,
/// or while usage is below every configured limit. Returns a localized
/// [`AppError`] once a daily or monthly limit has been reached.
///
/// Intended call site: the proxy forwarder, once per provider attempt, before
/// the upstream request is sent (see the PRODUCT-05 report for details).
#[cfg_attr(not(test), allow(dead_code))] // the proxy uses hard_limit_breach
pub fn check_hard_limit(db: &Database, app_type: &str, provider_id: &str) -> Result<(), AppError> {
    match hard_limit_breach(db, app_type, provider_id)? {
        Some((err, _)) => Err(err),
        None => Ok(()),
    }
}

/// Like [`check_hard_limit`], but returns the breach (error + usage status)
/// instead of failing, so the proxy can skip the provider and notify the user.
/// `Ok(None)` means the request may proceed.
pub fn hard_limit_breach(
    db: &Database,
    app_type: &str,
    provider_id: &str,
) -> Result<Option<(AppError, ProviderLimitStatus)>, AppError> {
    if !is_enforcement_enabled(db)? {
        return Ok(None);
    }
    let status = db.check_provider_limits(provider_id, app_type)?;
    Ok(exceeded_error(&status).map(|err| {
        log::warn!("[{app_type}] budget hard limit reached for provider {provider_id}: {err}");
        (err, status)
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::lock_conn;
    use rusqlite::params;

    fn insert_provider(db: &Database, meta: &str) -> Result<(), AppError> {
        let conn = lock_conn!(db.conn);
        conn.execute(
            "INSERT INTO providers (id, app_type, name, settings_config, meta)
             VALUES ('p1', 'claude', 'Relay', '{}', ?1)",
            params![meta],
        )?;
        Ok(())
    }

    fn insert_log(db: &Database, cost: &str) -> Result<(), AppError> {
        let conn = lock_conn!(db.conn);
        conn.execute(
            "INSERT INTO proxy_request_logs (
                request_id, provider_id, app_type, model,
                input_tokens, output_tokens, total_cost_usd,
                latency_ms, status_code, created_at
            ) VALUES (?1, 'p1', 'claude', 'm', 1, 1, ?2, 10, 200, ?3)",
            params![
                format!("req-{cost}"),
                cost,
                chrono::Local::now().timestamp()
            ],
        )?;
        Ok(())
    }

    #[test]
    fn enforcement_defaults_off_and_round_trips() -> Result<(), AppError> {
        let db = Database::memory()?;
        assert!(!is_enforcement_enabled(&db)?);
        set_enforcement_enabled(&db, true)?;
        assert!(is_enforcement_enabled(&db)?);
        set_enforcement_enabled(&db, false)?;
        assert!(!is_enforcement_enabled(&db)?);
        Ok(())
    }

    #[test]
    fn exceeded_limit_passes_when_enforcement_off() -> Result<(), AppError> {
        let db = Database::memory()?;
        insert_provider(&db, r#"{"limitDailyUsd":"1"}"#)?;
        insert_log(&db, "1.5")?;
        check_hard_limit(&db, "claude", "p1")?;
        Ok(())
    }

    #[test]
    fn exceeded_daily_limit_is_blocked_when_enforced() -> Result<(), AppError> {
        let db = Database::memory()?;
        insert_provider(&db, r#"{"limitDailyUsd":"1"}"#)?;
        insert_log(&db, "1.5")?;
        set_enforcement_enabled(&db, true)?;

        let err = check_hard_limit(&db, "claude", "p1").expect_err("limit should block");
        match err {
            AppError::Localized { key, en, .. } => {
                assert_eq!(key, BUDGET_EXCEEDED_ERROR_KEY);
                assert!(en.contains("daily"), "{en}");
                assert!(en.contains("$1.50"), "{en}");
            }
            other => panic!("unexpected error: {other:?}"),
        }
        Ok(())
    }

    #[test]
    fn under_limit_or_no_limit_passes_when_enforced() -> Result<(), AppError> {
        let db = Database::memory()?;
        insert_provider(&db, r#"{"limitDailyUsd":"5","limitMonthlyUsd":"50"}"#)?;
        insert_log(&db, "1.5")?;
        set_enforcement_enabled(&db, true)?;
        check_hard_limit(&db, "claude", "p1")?;
        // Unknown provider has no limits and must never be blocked.
        check_hard_limit(&db, "claude", "missing")?;
        Ok(())
    }

    #[test]
    fn monthly_limit_reports_monthly_period() {
        let status = ProviderLimitStatus {
            provider_id: "p1".into(),
            daily_usage: "0.5".into(),
            daily_limit: Some("10.00".into()),
            daily_exceeded: false,
            monthly_usage: "20.0".into(),
            monthly_limit: Some("20.00".into()),
            monthly_exceeded: true,
        };
        let Some(AppError::Localized { en, .. }) = exceeded_error(&status) else {
            panic!("expected localized error");
        };
        assert!(en.contains("monthly"), "{en}");
    }
}
