//! Background health prober (PRODUCT-03 / PROXY-22).
//!
//! Without it, breaker recovery is purely passive: an Open breaker only moves
//! to HalfOpen when a real user request arrives, and that request is the probe
//! (paying the full upstream timeout if the provider is still down).
//!
//! When enabled (opt-in, `settings.healthProbe.enabled`), this task wakes every
//! [`TICK`] and, only while the proxy is running, looks at the router's breakers
//! for failover-queue members that are Open or HalfOpen. For each one it takes
//! the breaker's HalfOpen permit exactly like a user request would (so the
//! Open → HalfOpen timeout and the one-probe-at-a-time limit still apply), sends
//! the cheap credentialed GET from
//! [`StreamCheckService::probe_for_recovery`](crate::services::stream_check::StreamCheckService::probe_for_recovery)
//! and records the result on the breaker and in `provider_health`.
//!
//! Cost and rate limits: at most one network probe per provider per interval,
//! at most [`MAX_PROBES_PER_CYCLE`] providers per tick, no model call, and a
//! 429 answer backs the provider off exponentially (capped at [`MAX_BACKOFF`])
//! without counting as success or failure.

use crate::app_config::AppType;
use crate::database::Database;
use crate::provider::Provider;
use crate::proxy::circuit_breaker::{
    registered_breakers, split_breaker_key, CircuitBreaker, CircuitState,
};
use crate::services::stream_check::{RecoveryProbe, StreamCheckService};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// How often the prober wakes up to look for breakers to probe.
pub const TICK: Duration = Duration::from_secs(15);
/// Default minimum gap between two network probes of one provider.
pub const DEFAULT_INTERVAL_SECS: u64 = 120;
pub const MIN_INTERVAL_SECS: u64 = 30;
pub const MAX_INTERVAL_SECS: u64 = 3600;
/// Upper bound on providers probed per tick.
pub const MAX_PROBES_PER_CYCLE: usize = 4;
/// Upper bound on back-to-back probes for one provider within a tick (needed
/// when `success_threshold` > 1 to close a HalfOpen breaker).
pub const MAX_PROBES_PER_PROVIDER: u32 = 5;
/// Longest back-off after repeated 429 answers or an unprobeable provider.
pub const MAX_BACKOFF: Duration = Duration::from_secs(3600);
/// Timeout of a single probe request.
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(8);

/// User-facing configuration, stored in `settings.json` under `healthProbe`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct HealthProbeSettings {
    /// Opt-in: off by default.
    pub enabled: bool,
    /// Minimum seconds between two probes of the same provider.
    pub interval_secs: u64,
}

impl Default for HealthProbeSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            interval_secs: DEFAULT_INTERVAL_SECS,
        }
    }
}

impl HealthProbeSettings {
    /// Interval clamped to the supported range.
    pub fn interval(&self) -> Duration {
        Duration::from_secs(
            self.interval_secs
                .clamp(MIN_INTERVAL_SECS, MAX_INTERVAL_SECS),
        )
    }

    /// Current settings from `settings.json`.
    pub fn current() -> Self {
        crate::settings::get_settings()
            .health_probe
            .unwrap_or_default()
    }
}

#[derive(Debug, Clone, Copy)]
struct ProbeEntry {
    next_allowed: Instant,
    rate_limit_strikes: u32,
}

/// Per-provider probe schedule: when each breaker key may be probed next.
#[derive(Debug, Default)]
pub struct ProbeScheduler {
    entries: HashMap<String, ProbeEntry>,
}

impl ProbeScheduler {
    /// Whether `key` may be probed at `now`.
    pub fn is_due(&self, key: &str, now: Instant) -> bool {
        self.entries
            .get(key)
            .is_none_or(|entry| now >= entry.next_allowed)
    }

    /// A probe was sent and answered (healthy or not): wait one interval.
    pub fn record_probe(&mut self, key: &str, now: Instant, interval: Duration) {
        self.entries.insert(
            key.to_string(),
            ProbeEntry {
                next_allowed: now + interval,
                rate_limit_strikes: 0,
            },
        );
    }

    /// The upstream answered 429: back off `interval * 2^strikes`, capped.
    pub fn record_rate_limited(&mut self, key: &str, now: Instant, interval: Duration) {
        let strikes = self
            .entries
            .get(key)
            .map_or(0, |entry| entry.rate_limit_strikes)
            .saturating_add(1);
        let factor = 1u32 << strikes.min(16);
        let delay = interval.saturating_mul(factor).min(MAX_BACKOFF);
        self.entries.insert(
            key.to_string(),
            ProbeEntry {
                next_allowed: now + delay,
                rate_limit_strikes: strikes,
            },
        );
    }

    /// The provider cannot be probed (official / OAuth / no base URL).
    pub fn record_skipped(&mut self, key: &str, now: Instant) {
        self.entries.insert(
            key.to_string(),
            ProbeEntry {
                next_allowed: now + MAX_BACKOFF,
                rate_limit_strikes: 0,
            },
        );
    }

    /// Drop entries for keys that are no longer candidates (breaker closed or
    /// removed), so a later outage starts with a fresh schedule.
    pub fn retain_keys(&mut self, keys: &HashSet<String>) {
        self.entries.retain(|key, _| keys.contains(key));
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.entries.len()
    }
}

/// What one probe cycle did, for logging and tests.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct CycleReport {
    pub probed: Vec<String>,
    pub recovered: Vec<String>,
}

/// Run one probe cycle over `breakers` (normally [`registered_breakers`]).
///
/// `probe` performs the network probe; it is a parameter so tests can run the
/// scheduling and breaker logic without a network.
pub async fn run_cycle<F, Fut>(
    db: &Database,
    scheduler: &mut ProbeScheduler,
    settings: &HealthProbeSettings,
    breakers: Vec<(String, Arc<CircuitBreaker>)>,
    now: Instant,
    probe: F,
) -> CycleReport
where
    F: Fn(AppType, Provider) -> Fut,
    Fut: Future<Output = RecoveryProbe>,
{
    let mut report = CycleReport::default();
    let interval = settings.interval();

    let mut candidates = Vec::new();
    for (key, breaker) in breakers {
        let state = breaker.get_state().await;
        if matches!(state, CircuitState::Open | CircuitState::HalfOpen) {
            candidates.push((key, breaker));
        }
    }
    let keys: HashSet<String> = candidates.iter().map(|(k, _)| k.clone()).collect();
    scheduler.retain_keys(&keys);

    let mut queues: HashMap<String, HashSet<String>> = HashMap::new();
    for (key, breaker) in candidates {
        if report.probed.len() >= MAX_PROBES_PER_CYCLE {
            break;
        }
        if !scheduler.is_due(&key, now) {
            continue;
        }
        let Some((app, provider_id)) = split_breaker_key(&key) else {
            continue;
        };
        let Ok(app_type) = AppType::from_str(app) else {
            continue;
        };

        // Only failover-queue members are probed.
        if !queues.contains_key(app) {
            let members = db
                .get_failover_queue(app)
                .map(|items| items.into_iter().map(|i| i.provider_id).collect())
                .unwrap_or_default();
            queues.insert(app.to_string(), members);
        }
        if !queues.get(app).is_some_and(|q| q.contains(provider_id)) {
            continue;
        }
        let Ok(Some(provider)) = db.get_provider_by_id(provider_id, app) else {
            continue;
        };

        let failure_threshold = match db.get_proxy_config_for_app(app).await {
            Ok(config) => config.circuit_failure_threshold,
            Err(_) => crate::proxy::circuit_breaker::DEFAULT_FAILURE_THRESHOLD,
        };
        let success_threshold = breaker
            .config()
            .await
            .success_threshold
            .clamp(1, MAX_PROBES_PER_PROVIDER);

        let mut probed = false;
        for _ in 0..success_threshold {
            // Same gate as a user request: honours the Open timeout and the
            // single HalfOpen permit, so a concurrent user probe wins.
            let (allow, mut permit) = breaker.allow_request_guarded().await;
            if !allow.allowed {
                break;
            }
            probed = true;
            let outcome = probe(app_type.clone(), provider.clone()).await;
            match outcome {
                RecoveryProbe::Healthy { .. } => {
                    scheduler.record_probe(&key, now, interval);
                    breaker.record_success(permit.take()).await;
                    record_health(db, provider_id, app, true, None, failure_threshold).await;
                    if breaker.get_state().await == CircuitState::Closed {
                        log::info!("[HealthProbe] {key} recovered, breaker closed");
                        report.recovered.push(key.clone());
                        break;
                    }
                }
                RecoveryProbe::Unhealthy { reason } => {
                    scheduler.record_probe(&key, now, interval);
                    breaker.record_failure(permit.take()).await;
                    log::debug!("[HealthProbe] {key} still unhealthy: {reason}");
                    record_health(
                        db,
                        provider_id,
                        app,
                        false,
                        Some(format!("health probe: {reason}")),
                        failure_threshold,
                    )
                    .await;
                    break;
                }
                RecoveryProbe::RateLimited => {
                    if permit.take() {
                        breaker.release_half_open_permit();
                    }
                    scheduler.record_rate_limited(&key, now, interval);
                    log::debug!("[HealthProbe] {key} rate limited, backing off");
                    break;
                }
                RecoveryProbe::Skipped { reason } => {
                    if permit.take() {
                        breaker.release_half_open_permit();
                    }
                    scheduler.record_skipped(&key, now);
                    log::debug!("[HealthProbe] {key} not probeable: {reason}");
                    break;
                }
            }
        }
        if probed {
            report.probed.push(key);
        }
    }

    report
}

async fn record_health(
    db: &Database,
    provider_id: &str,
    app: &str,
    success: bool,
    error: Option<String>,
    failure_threshold: u32,
) {
    if let Err(e) = db
        .update_provider_health_with_threshold(provider_id, app, success, error, failure_threshold)
        .await
    {
        log::warn!("[HealthProbe] failed to update health for {app}:{provider_id}: {e}");
    }
}

/// Spawn the background prober. Safe to call once at startup; the task idles
/// while probing is disabled or the proxy is stopped.
pub fn spawn(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        use tauri::Manager;
        let mut scheduler = ProbeScheduler::default();
        loop {
            tokio::time::sleep(TICK).await;

            let settings = HealthProbeSettings::current();
            if !settings.enabled {
                scheduler.clear();
                continue;
            }
            let Some(state) = app.try_state::<crate::store::AppState>() else {
                continue;
            };
            if !state.proxy_service.is_running().await {
                scheduler.clear();
                continue;
            }
            let db = state.db.clone();
            let report = run_cycle(
                &db,
                &mut scheduler,
                &settings,
                registered_breakers(),
                Instant::now(),
                |app_type, provider| async move {
                    StreamCheckService::probe_for_recovery(&app_type, &provider, PROBE_TIMEOUT)
                        .await
                },
            )
            .await;
            if !report.probed.is_empty() {
                log::debug!(
                    "[HealthProbe] probed {:?}, recovered {:?}",
                    report.probed,
                    report.recovered
                );
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proxy::circuit_breaker::CircuitBreakerConfig;
    use serde_json::json;
    use std::sync::atomic::{AtomicU32, Ordering};

    #[test]
    fn probing_is_opt_in_by_default() {
        let settings = HealthProbeSettings::default();
        assert!(!settings.enabled);
        assert_eq!(
            settings.interval(),
            Duration::from_secs(DEFAULT_INTERVAL_SECS)
        );

        // Missing keys in settings.json fall back to the defaults.
        let parsed: HealthProbeSettings = serde_json::from_value(json!({})).unwrap();
        assert_eq!(parsed, HealthProbeSettings::default());
        let parsed: HealthProbeSettings =
            serde_json::from_value(json!({ "enabled": true })).unwrap();
        assert!(parsed.enabled);
        assert_eq!(parsed.interval_secs, DEFAULT_INTERVAL_SECS);
    }

    #[test]
    fn interval_is_clamped() {
        let low = HealthProbeSettings {
            enabled: true,
            interval_secs: 1,
        };
        assert_eq!(low.interval(), Duration::from_secs(MIN_INTERVAL_SECS));
        let high = HealthProbeSettings {
            enabled: true,
            interval_secs: 1_000_000,
        };
        assert_eq!(high.interval(), Duration::from_secs(MAX_INTERVAL_SECS));
    }

    #[test]
    fn scheduler_spaces_probes_by_interval() {
        let mut s = ProbeScheduler::default();
        let t0 = Instant::now();
        let interval = Duration::from_secs(60);
        assert!(s.is_due("claude:a", t0));
        s.record_probe("claude:a", t0, interval);
        assert!(!s.is_due("claude:a", t0 + Duration::from_secs(59)));
        assert!(s.is_due("claude:a", t0 + interval));
        assert!(s.is_due("claude:b", t0), "other keys are independent");
    }

    #[test]
    fn scheduler_backs_off_exponentially_on_rate_limit() {
        let mut s = ProbeScheduler::default();
        let t0 = Instant::now();
        let interval = Duration::from_secs(60);
        s.record_rate_limited("k", t0, interval);
        assert!(!s.is_due("k", t0 + Duration::from_secs(119)));
        assert!(s.is_due("k", t0 + Duration::from_secs(120)));
        s.record_rate_limited("k", t0, interval);
        assert!(!s.is_due("k", t0 + Duration::from_secs(239)));
        assert!(s.is_due("k", t0 + Duration::from_secs(240)));
        for _ in 0..20 {
            s.record_rate_limited("k", t0, interval);
        }
        assert!(s.is_due("k", t0 + MAX_BACKOFF), "back-off is capped");
        // A normal answer resets the strikes.
        s.record_probe("k", t0, interval);
        s.record_rate_limited("k", t0, interval);
        assert!(s.is_due("k", t0 + Duration::from_secs(120)));
    }

    #[test]
    fn scheduler_forgets_keys_that_are_no_longer_candidates() {
        let mut s = ProbeScheduler::default();
        let t0 = Instant::now();
        s.record_probe("a", t0, Duration::from_secs(60));
        s.record_probe("b", t0, Duration::from_secs(60));
        s.retain_keys(&HashSet::from(["a".to_string()]));
        assert_eq!(s.len(), 1);
        assert!(s.is_due("b", t0));
    }

    fn setup_db(in_queue: bool) -> Database {
        let db = Database::memory().expect("memory db");
        let provider = Provider::with_id(
            "p1".to_string(),
            "Primary".to_string(),
            json!({ "env": { "ANTHROPIC_BASE_URL": "https://relay.invalid" } }),
            None,
        );
        db.save_provider("claude", &provider).unwrap();
        if in_queue {
            db.add_to_failover_queue("claude", "p1").unwrap();
        }
        db
    }

    async fn open_breaker(success_threshold: u32) -> Arc<CircuitBreaker> {
        let breaker = Arc::new(CircuitBreaker::new(CircuitBreakerConfig {
            failure_threshold: 1,
            success_threshold,
            timeout_seconds: 0,
            ..Default::default()
        }));
        breaker.record_failure(false).await;
        assert_eq!(breaker.get_state().await, CircuitState::Open);
        breaker
    }

    fn enabled() -> HealthProbeSettings {
        HealthProbeSettings {
            enabled: true,
            interval_secs: 60,
        }
    }

    #[tokio::test]
    async fn healthy_probes_close_breaker_without_user_requests() {
        let db = setup_db(true);
        let breaker = open_breaker(2).await;
        let calls = Arc::new(AtomicU32::new(0));
        let counter = calls.clone();
        let mut scheduler = ProbeScheduler::default();
        let now = Instant::now();

        let report = run_cycle(
            &db,
            &mut scheduler,
            &enabled(),
            vec![("claude:p1".to_string(), breaker.clone())],
            now,
            move |_, _| {
                counter.fetch_add(1, Ordering::SeqCst);
                async {
                    RecoveryProbe::Healthy {
                        http_status: 200,
                        latency_ms: 5,
                    }
                }
            },
        )
        .await;

        assert_eq!(breaker.get_state().await, CircuitState::Closed);
        assert_eq!(
            calls.load(Ordering::SeqCst),
            2,
            "one probe per success needed"
        );
        assert_eq!(report.recovered, vec!["claude:p1".to_string()]);
        let health = db.get_provider_health("p1", "claude").await.unwrap();
        assert!(health.is_healthy);
        assert!(health.last_success_at.is_some());
        assert!(!scheduler.is_due("claude:p1", now));
    }

    #[tokio::test]
    async fn unhealthy_probe_keeps_breaker_open_and_waits_an_interval() {
        let db = setup_db(true);
        let breaker = open_breaker(1).await;
        let mut scheduler = ProbeScheduler::default();
        let now = Instant::now();
        let probe = |_, _| async {
            RecoveryProbe::Unhealthy {
                reason: "HTTP 503".into(),
            }
        };
        let breakers = vec![("claude:p1".to_string(), breaker.clone())];

        let report = run_cycle(
            &db,
            &mut scheduler,
            &enabled(),
            breakers.clone(),
            now,
            probe,
        )
        .await;
        assert_eq!(report.probed.len(), 1);
        assert_eq!(breaker.get_state().await, CircuitState::Open);
        let health = db.get_provider_health("p1", "claude").await.unwrap();
        assert!(health.last_error.unwrap().contains("503"));

        // Not probed again before the interval elapses.
        let report = run_cycle(
            &db,
            &mut scheduler,
            &enabled(),
            breakers.clone(),
            now + Duration::from_secs(30),
            probe,
        )
        .await;
        assert!(report.probed.is_empty());
        let report = run_cycle(
            &db,
            &mut scheduler,
            &enabled(),
            breakers,
            now + Duration::from_secs(61),
            probe,
        )
        .await;
        assert_eq!(report.probed.len(), 1);
    }

    #[tokio::test]
    async fn rate_limited_probe_is_neutral_and_releases_permit() {
        let db = setup_db(true);
        let breaker = open_breaker(1).await;
        let mut scheduler = ProbeScheduler::default();
        let now = Instant::now();
        run_cycle(
            &db,
            &mut scheduler,
            &enabled(),
            vec![("claude:p1".to_string(), breaker.clone())],
            now,
            |_, _| async { RecoveryProbe::RateLimited },
        )
        .await;
        // Breaker moved to HalfOpen (timeout elapsed) but the probe slot is
        // free again, so the next user request can still probe.
        assert_eq!(breaker.get_state().await, CircuitState::HalfOpen);
        assert!(breaker.allow_request().await.allowed);
        assert!(!scheduler.is_due("claude:p1", now + Duration::from_secs(119)));
    }

    #[tokio::test]
    async fn providers_outside_the_failover_queue_are_not_probed() {
        let db = setup_db(false);
        let breaker = open_breaker(1).await;
        let mut scheduler = ProbeScheduler::default();
        let report = run_cycle(
            &db,
            &mut scheduler,
            &enabled(),
            vec![("claude:p1".to_string(), breaker.clone())],
            Instant::now(),
            |_, _| async {
                if true {
                    panic!("must not probe");
                }
                RecoveryProbe::RateLimited
            },
        )
        .await;
        assert!(report.probed.is_empty());
        assert_eq!(breaker.get_state().await, CircuitState::Open);
    }

    #[tokio::test]
    async fn closed_breakers_and_busy_half_open_slots_are_left_alone() {
        let db = setup_db(true);
        let closed = Arc::new(CircuitBreaker::new(CircuitBreakerConfig::default()));
        let busy = open_breaker(1).await;
        // A user request holds the only HalfOpen permit.
        let user = busy.allow_request().await;
        assert!(user.used_half_open_permit);

        let mut scheduler = ProbeScheduler::default();
        let report = run_cycle(
            &db,
            &mut scheduler,
            &enabled(),
            vec![
                ("claude:p1".to_string(), busy.clone()),
                ("claude:p2".to_string(), closed),
            ],
            Instant::now(),
            |_, _| async {
                if true {
                    panic!("must not probe");
                }
                RecoveryProbe::RateLimited
            },
        )
        .await;
        assert!(report.probed.is_empty());
        assert_eq!(busy.get_state().await, CircuitState::HalfOpen);
    }
}
