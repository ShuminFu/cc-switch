//! Provider 预算告警
//!
//! 每次 `usage-log-recorded`（防抖后）触发一次评估：对所有配置了日/月限额的
//! provider 计算本地日、本地月窗口内的用量占比，跨过 80% / 100% 时发送系统通知。
//!
//! 设计要点：
//! - 每个 (app, provider, 周期) 在同一周期内每档阈值最多告警一次；
//!   直接从 <80% 跳到 ≥100% 时只发 100% 一条。
//! - 去重状态写入 settings 表（本机专属，不随备份/同步导出），重启后不重复告警。
//! - 评估失败只记 warn，不影响日志写入与前端刷新。

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;

use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};

use crate::database::Database;
use crate::error::AppError;

/// settings 表中保存去重状态的键
pub(crate) const BUDGET_ALERT_STATE_KEY: &str = "usage_budget_alert_state";

/// 告警阈值：用量 / 限额 ≥ 0.8
const WARNING_RATIO: f64 = 0.8;

/// 串行化评估，避免两个防抖批次并发时重复告警
static EVAL_LOCK: Mutex<()> = Mutex::new(());

/// 告警档位（有序：Warning < Exceeded）
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BudgetLevel {
    /// 达到 80%
    Warning,
    /// 达到 100%
    Exceeded,
}

impl BudgetLevel {
    fn percent(self) -> u32 {
        match self {
            BudgetLevel::Warning => 80,
            BudgetLevel::Exceeded => 100,
        }
    }
}

/// 限额周期
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetPeriod {
    Daily,
    Monthly,
}

impl BudgetPeriod {
    fn as_str(self) -> &'static str {
        match self {
            BudgetPeriod::Daily => "daily",
            BudgetPeriod::Monthly => "monthly",
        }
    }

    /// 周期标识：本地日 `YYYY-MM-DD` / 本地月 `YYYY-MM`
    fn period_key(self, now: DateTime<Local>) -> String {
        match self {
            BudgetPeriod::Daily => now.format("%Y-%m-%d").to_string(),
            BudgetPeriod::Monthly => now.format("%Y-%m").to_string(),
        }
    }
}

/// 一条待发送的预算告警
#[derive(Debug, Clone, PartialEq)]
pub struct BudgetAlert {
    pub app_type: String,
    pub provider_id: String,
    pub provider_name: String,
    pub period: BudgetPeriod,
    pub level: BudgetLevel,
    pub usage_usd: f64,
    pub limit_usd: f64,
}

/// 某个 (app, provider, 周期) 最近一次告警到的档位
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct AlertMark {
    period: String,
    level: BudgetLevel,
    /// 告警时的限额（两位小数文本）；限额被修改后按新限额重新计档
    #[serde(default, skip_serializing_if = "Option::is_none")]
    limit: Option<String>,
}

/// 持久化的去重状态
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct AlertState {
    #[serde(default)]
    entries: BTreeMap<String, AlertMark>,
}

impl AlertState {
    fn key(app_type: &str, provider_id: &str, period: BudgetPeriod) -> String {
        // JSON 数组编码，避免 id 中的分隔符造成键冲突
        serde_json::json!([app_type, provider_id, period.as_str()]).to_string()
    }

    /// 记录本次档位；返回 true 表示需要告警（本周期、当前限额下首次达到该档或更高档）。
    fn record(&mut self, key: &str, period_key: &str, limit: &str, level: BudgetLevel) -> bool {
        match self.entries.get(key) {
            Some(mark)
                if mark.period == period_key
                    && mark.limit.as_deref().is_none_or(|l| l == limit)
                    && mark.level >= level =>
            {
                false
            }
            _ => {
                self.entries.insert(
                    key.to_string(),
                    AlertMark {
                        period: period_key.to_string(),
                        level,
                        limit: Some(limit.to_string()),
                    },
                );
                true
            }
        }
    }
}

/// 按用量占比得到档位；限额无效（≤0 或非有限数）时不告警。
fn level_for(usage: f64, limit: f64) -> Option<BudgetLevel> {
    if !limit.is_finite() || limit <= 0.0 || !usage.is_finite() {
        return None;
    }
    if usage >= limit {
        Some(BudgetLevel::Exceeded)
    } else if usage >= limit * WARNING_RATIO {
        Some(BudgetLevel::Warning)
    } else {
        None
    }
}

fn load_state(db: &Database) -> AlertState {
    match db.get_setting(BUDGET_ALERT_STATE_KEY) {
        Ok(Some(raw)) => serde_json::from_str(&raw).unwrap_or_else(|e| {
            log::warn!("[budget-alert] 去重状态解析失败，已重置: {e}");
            AlertState::default()
        }),
        Ok(None) => AlertState::default(),
        Err(e) => {
            log::warn!("[budget-alert] 读取去重状态失败: {e}");
            AlertState::default()
        }
    }
}

/// 以 `now` 为时间点评估所有限额 provider，返回需要发送的告警并落盘去重状态。
pub(crate) fn collect_budget_alerts(
    db: &Database,
    now: DateTime<Local>,
) -> Result<Vec<BudgetAlert>, AppError> {
    let providers = db.list_providers_with_spend_limits()?;
    let original = load_state(db);
    let mut state = original.clone();
    let mut live_keys = BTreeSet::new();
    let mut alerts = Vec::new();

    for (provider_id, app_type, provider_name) in providers {
        for period in [BudgetPeriod::Daily, BudgetPeriod::Monthly] {
            live_keys.insert(AlertState::key(&app_type, &provider_id, period));
        }

        let status = match db.check_provider_limits_at(&provider_id, &app_type, now) {
            Ok(status) => status,
            Err(e) => {
                log::warn!("[budget-alert] 检查 {app_type}/{provider_id} 限额失败: {e}");
                continue;
            }
        };

        let windows = [
            (
                BudgetPeriod::Daily,
                status.daily_usage.as_str(),
                status.daily_limit.as_deref(),
            ),
            (
                BudgetPeriod::Monthly,
                status.monthly_usage.as_str(),
                status.monthly_limit.as_deref(),
            ),
        ];
        for (period, usage, limit_raw) in windows {
            let Some(limit_raw) = limit_raw else {
                continue;
            };
            let (Some(usage), Some(limit)) =
                (usage.parse::<f64>().ok(), limit_raw.parse::<f64>().ok())
            else {
                continue;
            };
            let Some(level) = level_for(usage, limit) else {
                continue;
            };
            let key = AlertState::key(&app_type, &provider_id, period);
            if state.record(&key, &period.period_key(now), limit_raw, level) {
                alerts.push(BudgetAlert {
                    app_type: app_type.clone(),
                    provider_id: provider_id.clone(),
                    provider_name: provider_name.clone(),
                    period,
                    level,
                    usage_usd: usage,
                    limit_usd: limit,
                });
            }
        }
    }

    // 已删除 / 已取消限额的 provider 不再保留去重记录
    state.entries.retain(|key, _| live_keys.contains(key));

    if state != original {
        let raw = serde_json::to_string(&state)
            .map_err(|e| AppError::Database(format!("序列化预算告警状态失败: {e}")))?;
        db.set_setting(BUDGET_ALERT_STATE_KEY, &raw)?;
    }
    Ok(alerts)
}

/// 告警通知文本（标题, 正文）
fn alert_texts(alert: &BudgetAlert, language: &str) -> (String, String) {
    let percent = alert.level.percent();
    let name = &alert.provider_name;
    let app = &alert.app_type;
    let usage = format!("${:.2}", alert.usage_usd);
    let limit = format!("${:.2}", alert.limit_usd);
    match language {
        "en" => {
            let period = match alert.period {
                BudgetPeriod::Daily => "daily",
                BudgetPeriod::Monthly => "monthly",
            };
            let title = match alert.level {
                BudgetLevel::Warning => format!("{name} reached {percent}% of its {period} limit"),
                BudgetLevel::Exceeded => format!("{name} exceeded its {period} limit"),
            };
            (title, format!("{app}: {usage} of {limit} used"))
        }
        "ja" => {
            let period = match alert.period {
                BudgetPeriod::Daily => "日次",
                BudgetPeriod::Monthly => "月次",
            };
            let title = match alert.level {
                BudgetLevel::Warning => format!("{name} が{period}上限の {percent}% に達しました"),
                BudgetLevel::Exceeded => format!("{name} が{period}上限を超えました"),
            };
            (title, format!("{app}: {usage} / {limit} 使用済み"))
        }
        "zh-TW" => {
            let period = match alert.period {
                BudgetPeriod::Daily => "每日",
                BudgetPeriod::Monthly => "每月",
            };
            let title = match alert.level {
                BudgetLevel::Warning => format!("{name} 已達{period}限額的 {percent}%"),
                BudgetLevel::Exceeded => format!("{name} 已超出{period}限額"),
            };
            (title, format!("{app}：已使用 {usage} / {limit}"))
        }
        _ => {
            let period = match alert.period {
                BudgetPeriod::Daily => "每日",
                BudgetPeriod::Monthly => "每月",
            };
            let title = match alert.level {
                BudgetLevel::Warning => format!("{name} 已达{period}限额的 {percent}%"),
                BudgetLevel::Exceeded => format!("{name} 已超出{period}限额"),
            };
            (title, format!("{app}：已使用 {usage} / {limit}"))
        }
    }
}

/// 使用日志写入后调用：评估限额并发送系统通知。
///
/// 在 `usage_events` 的防抖线程里执行；任何失败都只记录 warn。
pub fn on_usage_recorded(handle: &tauri::AppHandle) {
    use tauri::Manager;
    use tauri_plugin_notification::NotificationExt;

    let Some(state) = handle.try_state::<crate::store::AppState>() else {
        return;
    };
    let _guard = EVAL_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let alerts = match collect_budget_alerts(&state.db, Local::now()) {
        Ok(alerts) => alerts,
        Err(e) => {
            log::warn!("[budget-alert] 评估预算告警失败: {e}");
            return;
        }
    };
    if alerts.is_empty() {
        return;
    }

    let language = crate::settings::get_settings()
        .language
        .unwrap_or_else(|| crate::tray::detect_system_tray_language().to_string());
    for alert in alerts {
        log::info!(
            "[budget-alert] {}/{} {} {}%: ${:.4} / ${:.2}",
            alert.app_type,
            alert.provider_id,
            alert.period.as_str(),
            alert.level.percent(),
            alert.usage_usd,
            alert.limit_usd
        );
        let (title, body) = alert_texts(&alert, &language);
        if let Err(e) = handle
            .notification()
            .builder()
            .title(title)
            .body(body)
            .show()
        {
            log::warn!("[budget-alert] 发送系统通知失败: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::lock_conn;
    use chrono::TimeZone;
    use rusqlite::params;

    fn local_dt(year: i32, month: u32, day: u32, hour: u32) -> DateTime<Local> {
        Local
            .with_ymd_and_hms(year, month, day, hour, 0, 0)
            .single()
            .expect("valid local time")
    }

    fn insert_provider(db: &Database, id: &str, meta: &str) -> Result<(), AppError> {
        let conn = lock_conn!(db.conn);
        conn.execute(
            "INSERT INTO providers (id, app_type, name, settings_config, meta)
             VALUES (?1, 'claude', ?2, '{}', ?3)",
            params![id, format!("Provider {id}"), meta],
        )?;
        Ok(())
    }

    /// 用日汇总行模拟一次用量写入（`check_provider_limits_at` 同时统计汇总表）
    fn add_usage(db: &Database, id: &str, date: &str, model: &str, cost: &str) {
        let conn = db.conn.lock().expect("db lock");
        conn.execute(
            "INSERT INTO usage_daily_rollups (
                date, app_type, provider_id, model,
                request_count, success_count, input_tokens, output_tokens,
                cache_read_tokens, cache_creation_tokens, total_cost_usd, total_cost_micros,
                avg_latency_ms
            ) VALUES (?1, 'claude', ?2, ?3, 1, 1, 0, 0, 0, 0, ?4,
                CAST(ROUND(CAST(?4 AS REAL) * 1000000) AS INTEGER), 100)",
            params![date, id, model, cost],
        )
        .expect("insert rollup");
    }

    fn summary(alerts: &[BudgetAlert]) -> Vec<(BudgetPeriod, BudgetLevel)> {
        alerts.iter().map(|a| (a.period, a.level)).collect()
    }

    #[test]
    fn budget_level_thresholds() {
        assert_eq!(level_for(0.79, 1.0), None);
        assert_eq!(level_for(0.8, 1.0), Some(BudgetLevel::Warning));
        assert_eq!(level_for(0.99, 1.0), Some(BudgetLevel::Warning));
        assert_eq!(level_for(1.0, 1.0), Some(BudgetLevel::Exceeded));
        assert_eq!(level_for(5.0, 0.0), None, "零限额不告警");
        assert_eq!(level_for(5.0, f64::NAN), None);
    }

    #[test]
    fn budget_crossing_80_percent_alerts_once_per_period() -> Result<(), AppError> {
        let db = Database::memory()?;
        insert_provider(
            &db,
            "p1",
            r#"{"limitDailyUsd":"1.00","limitMonthlyUsd":"100"}"#,
        )?;
        let now = local_dt(2026, 3, 15, 12);

        // 低于 80%：无告警
        add_usage(&db, "p1", "2026-03-15", "m1", "0.5");
        assert!(collect_budget_alerts(&db, now)?.is_empty());

        // 跨过 80%：恰好一条日告警
        add_usage(&db, "p1", "2026-03-15", "m2", "0.35");
        let alerts = collect_budget_alerts(&db, now)?;
        assert_eq!(
            summary(&alerts),
            vec![(BudgetPeriod::Daily, BudgetLevel::Warning)]
        );
        assert_eq!(alerts[0].provider_id, "p1");
        assert_eq!(alerts[0].provider_name, "Provider p1");

        // 同一周期内继续增长但未到 100%：去重，不再告警
        add_usage(&db, "p1", "2026-03-15", "m3", "0.05");
        assert!(collect_budget_alerts(&db, now)?.is_empty());
        assert!(collect_budget_alerts(&db, now)?.is_empty());

        // 去重状态持久化：模拟重启后仍不重复
        assert!(db.get_setting(BUDGET_ALERT_STATE_KEY)?.is_some());
        assert!(collect_budget_alerts(&db, local_dt(2026, 3, 15, 23))?.is_empty());

        // 跨过 100%：升级告警一次
        add_usage(&db, "p1", "2026-03-15", "m4", "0.2");
        assert_eq!(
            summary(&collect_budget_alerts(&db, now)?),
            vec![(BudgetPeriod::Daily, BudgetLevel::Exceeded)]
        );
        assert!(collect_budget_alerts(&db, now)?.is_empty());

        // 新的一天：日周期重置，再次跨过 80% 时重新告警
        let tomorrow = local_dt(2026, 3, 16, 12);
        add_usage(&db, "p1", "2026-03-16", "m1", "0.5");
        assert!(collect_budget_alerts(&db, tomorrow)?.is_empty());
        add_usage(&db, "p1", "2026-03-16", "m2", "0.3");
        assert_eq!(
            summary(&collect_budget_alerts(&db, tomorrow)?),
            vec![(BudgetPeriod::Daily, BudgetLevel::Warning)]
        );
        assert!(collect_budget_alerts(&db, tomorrow)?.is_empty());
        Ok(())
    }

    #[test]
    fn budget_raised_limit_alerts_again_against_new_limit() -> Result<(), AppError> {
        let db = Database::memory()?;
        insert_provider(&db, "p1", r#"{"limitDailyUsd":"1"}"#)?;
        let now = local_dt(2026, 3, 15, 12);

        add_usage(&db, "p1", "2026-03-15", "m1", "1.2");
        assert_eq!(
            summary(&collect_budget_alerts(&db, now)?),
            vec![(BudgetPeriod::Daily, BudgetLevel::Exceeded)]
        );

        // 用户把限额调高到 2：当前 60% 不告警，之后按新限额重新告警
        {
            let conn = lock_conn!(db.conn);
            conn.execute(
                r#"UPDATE providers SET meta = '{"limitDailyUsd":"2"}' WHERE id = 'p1'"#,
                [],
            )?;
        }
        assert!(collect_budget_alerts(&db, now)?.is_empty());
        add_usage(&db, "p1", "2026-03-15", "m2", "0.5");
        assert_eq!(
            summary(&collect_budget_alerts(&db, now)?),
            vec![(BudgetPeriod::Daily, BudgetLevel::Warning)]
        );
        assert!(collect_budget_alerts(&db, now)?.is_empty());
        Ok(())
    }

    #[test]
    fn budget_monthly_alert_dedupes_across_days() -> Result<(), AppError> {
        let db = Database::memory()?;
        insert_provider(&db, "p1", r#"{"limitMonthlyUsd":"10"}"#)?;

        add_usage(&db, "p1", "2026-03-02", "m1", "8.5");
        assert_eq!(
            summary(&collect_budget_alerts(&db, local_dt(2026, 3, 2, 9))?),
            vec![(BudgetPeriod::Monthly, BudgetLevel::Warning)]
        );
        // 同月其他日子：不重复
        add_usage(&db, "p1", "2026-03-20", "m1", "0.5");
        assert!(collect_budget_alerts(&db, local_dt(2026, 3, 20, 9))?.is_empty());

        // 下个月重新计数
        add_usage(&db, "p1", "2026-04-01", "m1", "9");
        assert_eq!(
            summary(&collect_budget_alerts(&db, local_dt(2026, 4, 1, 9))?),
            vec![(BudgetPeriod::Monthly, BudgetLevel::Warning)]
        );
        Ok(())
    }

    #[test]
    fn budget_jump_past_100_sends_single_alert_and_skips_unlimited() -> Result<(), AppError> {
        let db = Database::memory()?;
        insert_provider(&db, "p1", r#"{"limitDailyUsd":"1"}"#)?;
        insert_provider(&db, "p2", "{}")?;
        let now = local_dt(2026, 3, 15, 12);

        add_usage(&db, "p1", "2026-03-15", "m1", "3");
        add_usage(&db, "p2", "2026-03-15", "m1", "300");
        let alerts = collect_budget_alerts(&db, now)?;
        assert_eq!(
            summary(&alerts),
            vec![(BudgetPeriod::Daily, BudgetLevel::Exceeded)]
        );
        assert_eq!(alerts[0].provider_id, "p1");
        assert!(collect_budget_alerts(&db, now)?.is_empty());
        Ok(())
    }

    #[test]
    fn budget_state_drops_providers_without_limits() -> Result<(), AppError> {
        let db = Database::memory()?;
        insert_provider(&db, "p1", r#"{"limitDailyUsd":"1"}"#)?;
        let now = local_dt(2026, 3, 15, 12);
        add_usage(&db, "p1", "2026-03-15", "m1", "0.9");
        assert_eq!(collect_budget_alerts(&db, now)?.len(), 1);

        {
            let conn = lock_conn!(db.conn);
            conn.execute("UPDATE providers SET meta = '{}' WHERE id = 'p1'", [])?;
        }
        assert!(collect_budget_alerts(&db, now)?.is_empty());
        let state: AlertState =
            serde_json::from_str(&db.get_setting(BUDGET_ALERT_STATE_KEY)?.unwrap()).unwrap();
        assert!(state.entries.is_empty());
        Ok(())
    }

    #[test]
    fn budget_alert_texts_are_localized() {
        let alert = BudgetAlert {
            app_type: "claude".into(),
            provider_id: "p1".into(),
            provider_name: "Acme".into(),
            period: BudgetPeriod::Daily,
            level: BudgetLevel::Warning,
            usage_usd: 0.8,
            limit_usd: 1.0,
        };
        let (title, body) = alert_texts(&alert, "en");
        assert_eq!(title, "Acme reached 80% of its daily limit");
        assert_eq!(body, "claude: $0.80 of $1.00 used");
        let (title, _) = alert_texts(&alert, "zh");
        assert_eq!(title, "Acme 已达每日限额的 80%");
    }
}
