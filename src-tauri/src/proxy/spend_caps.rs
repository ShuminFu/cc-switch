//! 供应商消费限额执行（日 / 月 USD 限额）
//!
//! 限额配置在 provider meta 的 `limitDailyUsd` / `limitMonthlyUsd`，过去只在 UI 展示与
//! 预算告警中使用；这里让代理选路真正执行它：
//! - 每个配置了限额的 (应用, provider) 在内存中维护本地日 / 本地月累计消费，
//!   首次检查时从 usage 仓库（明细 + 日汇总，与 `check_provider_limits` 同一口径）播种；
//! - 每条代理请求日志写入后把本次成本累加进去，稳态下选路不查库；
//! - 跨日 / 跨月或距上次播种超过 [`RESEED_INTERVAL`] 时重新播种，用于吸收日志清理、
//!   会话日志导入等不经过代理的写入，以及播种与累加之间的竞争造成的偏差。
//!
//! 未配置限额的 provider 不产生任何数据库访问。达到限额（用量 ≥ 限额）的 provider
//! 在选路时被跳过；播种读库失败时记录错误并放行（不缓存，下次重试），
//! 避免统计库故障阻断全部流量。

use crate::database::Database;
use crate::provider::Provider;
use crate::proxy::protocol::BreakerKey;
use chrono::{DateTime, Datelike, Local, NaiveDate};
use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// 内存累计值的最长使用时间，超过后下次检查从数据库重新播种
pub const RESEED_INTERVAL: Duration = Duration::from_secs(300);

/// provider meta 中配置的消费限额（USD）
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpendCaps {
    pub daily_usd: Option<f64>,
    pub monthly_usd: Option<f64>,
}

impl SpendCaps {
    /// 解析 provider 的限额；两项都未配置（或无法解析）时返回 `None`。
    ///
    /// 解析规则与 `Database::check_provider_limits_at` 一致，保证 UI 显示的「已超限」
    /// 与代理实际执行的结果相同。
    pub fn from_provider(provider: &Provider) -> Option<Self> {
        let meta = provider.meta.as_ref()?;
        let parse = |value: &Option<String>| value.as_deref().and_then(|s| s.parse::<f64>().ok());
        let caps = Self {
            daily_usd: parse(&meta.limit_daily_usd),
            monthly_usd: parse(&meta.limit_monthly_usd),
        };
        (caps.daily_usd.is_some() || caps.monthly_usd.is_some()).then_some(caps)
    }
}

/// 达到的限额周期
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpendCapPeriod {
    Daily,
    Monthly,
}

/// provider 已达到某项限额
#[derive(Debug, Clone, PartialEq)]
pub struct SpendCapHit {
    pub provider_id: String,
    pub provider_name: String,
    pub period: SpendCapPeriod,
    pub usage_usd: f64,
    pub limit_usd: f64,
}

impl fmt::Display for SpendCapHit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let period = match self.period {
            SpendCapPeriod::Daily => "daily",
            SpendCapPeriod::Monthly => "monthly",
        };
        write!(
            f,
            "{} ({}): {period} spend ${:.4} reached the ${:.2} limit",
            self.provider_name, self.provider_id, self.usage_usd, self.limit_usd
        )
    }
}

/// 某个 provider 的内存累计消费
#[derive(Debug, Clone)]
struct SpendTotals {
    day: NaiveDate,
    month: (i32, u32),
    daily_usd: f64,
    monthly_usd: f64,
    seeded_at: Instant,
}

impl SpendTotals {
    fn is_current(&self, now: &DateTime<Local>) -> bool {
        self.day == now.date_naive()
            && self.month == (now.year(), now.month())
            && self.seeded_at.elapsed() < RESEED_INTERVAL
    }
}

/// 按 (应用, provider) 维护内存累计消费并判定是否达到限额
pub struct SpendCapTracker {
    db: Arc<Database>,
    totals: Mutex<HashMap<BreakerKey, SpendTotals>>,
}

impl SpendCapTracker {
    pub fn new(db: Arc<Database>) -> Self {
        Self {
            db,
            totals: Mutex::new(HashMap::new()),
        }
    }

    /// provider 当前是否已达到限额（未配置限额时直接返回 `None`，不查库）
    pub fn check(&self, provider: &Provider, app_type: &str) -> Option<SpendCapHit> {
        self.check_at(provider, app_type, Local::now())
    }

    pub(crate) fn check_at(
        &self,
        provider: &Provider,
        app_type: &str,
        now: DateTime<Local>,
    ) -> Option<SpendCapHit> {
        let caps = SpendCaps::from_provider(provider)?;
        let totals = self.totals_at(&provider.id, app_type, now)?;

        let hit = |period, usage_usd: f64, limit: Option<f64>| {
            limit
                .filter(|limit| usage_usd >= *limit)
                .map(|limit_usd| SpendCapHit {
                    provider_id: provider.id.clone(),
                    provider_name: provider.name.clone(),
                    period,
                    usage_usd,
                    limit_usd,
                })
        };
        hit(SpendCapPeriod::Daily, totals.daily_usd, caps.daily_usd).or_else(|| {
            hit(
                SpendCapPeriod::Monthly,
                totals.monthly_usd,
                caps.monthly_usd,
            )
        })
    }

    /// 一条代理请求日志写入后累加其成本
    ///
    /// 只更新已播种且仍在当前窗口内的累计值；尚未播种的 provider 下次检查时
    /// 从数据库播种，自然包含这条日志。
    pub fn record(&self, provider_id: &str, app_type: &str, cost_usd: f64) {
        self.record_at(provider_id, app_type, cost_usd, Local::now());
    }

    pub(crate) fn record_at(
        &self,
        provider_id: &str,
        app_type: &str,
        cost_usd: f64,
        now: DateTime<Local>,
    ) {
        if !cost_usd.is_finite() || cost_usd <= 0.0 {
            return;
        }
        let key = BreakerKey::new(app_type, provider_id);
        let mut totals = self.totals.lock().unwrap_or_else(|e| e.into_inner());
        let Some(entry) = totals.get_mut(&key) else {
            return;
        };
        if entry.is_current(&now) {
            entry.daily_usd += cost_usd;
            entry.monthly_usd += cost_usd;
        } else {
            totals.remove(&key);
        }
    }

    /// 丢弃全部累计值，下次检查重新播种（如用量日志被清理后）
    #[allow(dead_code)]
    pub fn invalidate(&self) {
        self.totals
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
    }

    /// 当前窗口的累计值；缺失或过期时从数据库播种，播种失败返回 `None`（放行）
    fn totals_at(
        &self,
        provider_id: &str,
        app_type: &str,
        now: DateTime<Local>,
    ) -> Option<SpendTotals> {
        let key = BreakerKey::new(app_type, provider_id);
        // 持锁播种：同一时刻只播种一次，且播种与累加互斥
        let mut totals = self.totals.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(entry) = totals.get(&key).filter(|entry| entry.is_current(&now)) {
            return Some(entry.clone());
        }
        match self.db.provider_spend_at(provider_id, app_type, now) {
            Ok(spend) => {
                let entry = SpendTotals {
                    day: now.date_naive(),
                    month: (now.year(), now.month()),
                    daily_usd: spend.daily_usd,
                    monthly_usd: spend.monthly_usd,
                    seeded_at: Instant::now(),
                };
                totals.insert(key, entry.clone());
                Some(entry)
            }
            Err(e) => {
                totals.remove(&key);
                log::error!(
                    "[{app_type}] 读取 provider 消费用量失败，本次不执行限额: provider_id={provider_id}, {e}"
                );
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::ProviderMeta;
    use chrono::TimeZone;
    use serde_json::json;

    fn local(y: i32, m: u32, d: u32, h: u32) -> DateTime<Local> {
        Local
            .with_ymd_and_hms(y, m, d, h, 0, 0)
            .single()
            .expect("unambiguous local time")
    }

    fn capped_provider(id: &str, daily: Option<&str>, monthly: Option<&str>) -> Provider {
        let mut provider = Provider::with_id(id.to_string(), format!("P {id}"), json!({}), None);
        provider.meta = Some(ProviderMeta {
            limit_daily_usd: daily.map(str::to_string),
            limit_monthly_usd: monthly.map(str::to_string),
            ..Default::default()
        });
        provider
    }

    fn insert_log(db: &Database, provider_id: &str, cost: &str, at: DateTime<Local>) {
        let conn = db.conn.lock().expect("db lock");
        conn.execute(
            "INSERT INTO proxy_request_logs (
                request_id, provider_id, app_type, model, input_tokens, output_tokens,
                total_cost_usd, latency_ms, status_code, created_at
            ) VALUES (?1, ?2, 'claude', 'm', 1, 1, ?3, 1, 200, ?4)",
            rusqlite::params![
                uuid::Uuid::new_v4().to_string(),
                provider_id,
                cost,
                at.timestamp()
            ],
        )
        .expect("insert usage log");
    }

    #[test]
    fn caps_are_parsed_from_provider_meta() {
        assert_eq!(
            SpendCaps::from_provider(&capped_provider("p", Some("1.5"), None)),
            Some(SpendCaps {
                daily_usd: Some(1.5),
                monthly_usd: None
            })
        );
        assert_eq!(
            SpendCaps::from_provider(&capped_provider("p", Some("abc"), None)),
            None
        );
        let uncapped = Provider::with_id("p".into(), "p".into(), json!({}), None);
        assert_eq!(SpendCaps::from_provider(&uncapped), None);
    }

    #[test]
    fn uncapped_provider_never_reads_the_database() {
        let db = Arc::new(Database::memory().expect("db"));
        let tracker = SpendCapTracker::new(db);
        let uncapped = Provider::with_id("p".into(), "p".into(), json!({}), None);
        assert_eq!(tracker.check(&uncapped, "claude"), None);
        assert!(tracker.totals.lock().unwrap().is_empty());
    }

    #[test]
    fn seeds_from_usage_repo_and_accumulates_recorded_costs() {
        let db = Arc::new(Database::memory().expect("db"));
        let now = local(2026, 3, 15, 12);
        insert_log(&db, "p1", "0.60", now);
        // 本月早些时候的消费只计入月限额
        insert_log(&db, "p1", "4.00", local(2026, 3, 2, 12));

        let tracker = SpendCapTracker::new(db.clone());
        let provider = capped_provider("p1", Some("1.00"), Some("5.00"));
        assert_eq!(tracker.check_at(&provider, "claude", now), None);

        // 代理先写日志再累加；累加后达到日限额（≥ 限额即视为达到），且不再查库
        insert_log(&db, "p1", "0.40", now);
        tracker.record_at("p1", "claude", 0.40, now);
        let hit = tracker
            .check_at(&provider, "claude", now)
            .expect("daily cap reached");
        assert_eq!(hit.period, SpendCapPeriod::Daily);
        assert!((hit.usage_usd - 1.0).abs() < 1e-9);
        assert_eq!(hit.limit_usd, 1.0);

        // 第二天日累计清零，但月累计（4.6 + 0.4 = 5.0）达到月限额
        let tomorrow = local(2026, 3, 16, 9);
        let hit = tracker
            .check_at(&provider, "claude", tomorrow)
            .expect("monthly cap reached");
        assert_eq!(hit.period, SpendCapPeriod::Monthly);
    }

    #[test]
    fn record_before_seed_is_not_double_counted() {
        let db = Arc::new(Database::memory().expect("db"));
        let now = local(2026, 3, 15, 12);
        let tracker = SpendCapTracker::new(db.clone());
        let provider = capped_provider("p1", Some("1.00"), None);

        // 日志先落库再累加：未播种时累加被忽略，播种结果已包含该日志
        insert_log(&db, "p1", "0.70", now);
        tracker.record_at("p1", "claude", 0.70, now);
        assert_eq!(tracker.check_at(&provider, "claude", now), None);
        let totals = tracker.totals_at("p1", "claude", now).expect("seeded");
        assert!((totals.daily_usd - 0.70).abs() < 1e-9);
    }

    #[test]
    fn invalidate_reseeds_from_the_database() {
        let db = Arc::new(Database::memory().expect("db"));
        let now = local(2026, 3, 15, 12);
        let tracker = SpendCapTracker::new(db.clone());
        let provider = capped_provider("p1", Some("1.00"), None);
        assert_eq!(tracker.check_at(&provider, "claude", now), None);

        // 不经过代理写入的消费在重新播种后才可见
        insert_log(&db, "p1", "2.00", now);
        assert_eq!(tracker.check_at(&provider, "claude", now), None);
        tracker.invalidate();
        assert!(tracker.check_at(&provider, "claude", now).is_some());
    }
}
