//! Lock-free proxy counters (PROXY-34).
//!
//! The request pipeline used to mutate `ProxyStatus` under an async `RwLock` in a
//! dozen places, recomputing `success_rate` by hand each time. Counters now live in
//! atomics updated through a handful of `record_*` calls, the few string fields sit
//! behind a short synchronous mutex (never held across `.await`), and
//! `success_rate` is derived when the status is read.

use crate::proxy::types::ProxyStatus;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Upper bounds (seconds) of the per-provider attempt latency histogram
/// exported on `/metrics` (PROXY-20). Latency is time to upstream response
/// headers (first byte for streams), not full stream duration.
pub const LATENCY_BUCKETS_SECS: [f64; 11] = [
    0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0, 60.0, 120.0, 300.0,
];

#[derive(Debug, Default, Clone)]
struct ProviderCounters {
    name: String,
    success: u64,
    failure: u64,
    /// Non-cumulative bucket counts; the last slot is `+Inf`.
    buckets: [u64; LATENCY_BUCKETS_SECS.len() + 1],
    latency_sum_secs: f64,
}

/// One breaker's state for the `/metrics` gauge.
#[derive(Debug, Clone)]
pub struct BreakerGauge {
    pub app_type: String,
    pub provider_id: String,
    /// 0 = closed, 1 = half-open, 2 = open.
    pub state: u8,
}

#[derive(Debug, Default)]
struct Detail {
    current_provider: Option<String>,
    current_provider_id: Option<String>,
    last_request_at: Option<String>,
    last_error: Option<String>,
}

/// Request counters shared by every request handled by one proxy server.
#[derive(Debug, Default)]
pub struct ProxyMetrics {
    active_connections: AtomicUsize,
    total_requests: AtomicU64,
    success_requests: AtomicU64,
    failed_requests: AtomicU64,
    failover_count: AtomicU64,
    detail: Mutex<Detail>,
    /// Per `(app_type, provider_id)` attempt counters and latency histogram.
    providers: Mutex<BTreeMap<(String, String), ProviderCounters>>,
}

/// How one client request ended, as far as the counters are concerned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RequestOutcome {
    /// Served by a provider; `failover` is true when an earlier provider in the
    /// queue failed (or was skipped) for this request first.
    Served { failover: bool },
    /// No provider served the request.
    Failed { error: String },
}

impl ProxyMetrics {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    fn detail(&self) -> std::sync::MutexGuard<'_, Detail> {
        // A panic while holding this short lock cannot leave the counters
        // inconsistent; keep serving with the inner value.
        self.detail.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// A client request entered the pipeline.
    pub fn record_request_start(&self) {
        self.total_requests.fetch_add(1, Ordering::Relaxed);
        self.detail().last_request_at = Some(chrono::Utc::now().to_rfc3339());
    }

    /// An attempt is about to be sent to `provider`.
    pub fn record_attempt_target(&self, provider_id: &str, provider_name: &str) {
        let mut detail = self.detail();
        detail.current_provider = Some(provider_name.to_string());
        detail.current_provider_id = Some(provider_id.to_string());
    }

    /// The final outcome of a client request.
    pub fn record_outcome(&self, outcome: RequestOutcome) {
        match outcome {
            RequestOutcome::Served { failover } => {
                self.success_requests.fetch_add(1, Ordering::Relaxed);
                if failover {
                    self.failover_count.fetch_add(1, Ordering::Relaxed);
                }
                self.detail().last_error = None;
            }
            RequestOutcome::Failed { error } => {
                self.failed_requests.fetch_add(1, Ordering::Relaxed);
                self.detail().last_error = Some(error);
            }
        }
    }

    /// Update `last_error` without counting a request (per-attempt failures,
    /// mid-stream failures reported after the request was already counted).
    pub fn set_last_error(&self, error: String) {
        self.detail().last_error = Some(error);
    }

    /// One upstream attempt finished (PROXY-20): `success` is whether the
    /// provider served the request; `elapsed` is the time to response headers.
    pub fn record_attempt_result(
        &self,
        app_type: &str,
        provider_id: &str,
        provider_name: &str,
        success: bool,
        elapsed: Duration,
    ) {
        let secs = elapsed.as_secs_f64();
        let bucket = LATENCY_BUCKETS_SECS
            .iter()
            .position(|upper| secs <= *upper)
            .unwrap_or(LATENCY_BUCKETS_SECS.len());
        let mut providers = self.providers.lock().unwrap_or_else(|e| e.into_inner());
        let entry = providers
            .entry((app_type.to_string(), provider_id.to_string()))
            .or_default();
        if entry.name != provider_name {
            entry.name = provider_name.to_string();
        }
        if success {
            entry.success += 1;
        } else {
            entry.failure += 1;
        }
        entry.buckets[bucket] += 1;
        entry.latency_sum_secs += secs;
    }

    /// Render all counters in the Prometheus text exposition format (v0.0.4).
    pub fn render_prometheus(
        &self,
        breakers: &[BreakerGauge],
        extra_gauges: &[(&str, &str, u64)],
    ) -> String {
        let mut out = String::with_capacity(4096);
        let counter = |out: &mut String, name: &str, help: &str, value: u64| {
            let _ = writeln!(
                out,
                "# HELP {name} {help}\n# TYPE {name} counter\n{name} {value}"
            );
        };
        counter(
            &mut out,
            "ccswitch_proxy_requests_total",
            "Client requests that entered the proxy pipeline.",
            self.total_requests.load(Ordering::Relaxed),
        );
        counter(
            &mut out,
            "ccswitch_proxy_requests_succeeded_total",
            "Client requests served by a provider.",
            self.success_requests.load(Ordering::Relaxed),
        );
        counter(
            &mut out,
            "ccswitch_proxy_requests_failed_total",
            "Client requests no provider could serve.",
            self.failed_requests.load(Ordering::Relaxed),
        );
        counter(
            &mut out,
            "ccswitch_proxy_failovers_total",
            "Requests served after an earlier provider failed or was skipped.",
            self.failover_count.load(Ordering::Relaxed),
        );
        let _ = writeln!(
            out,
            "# HELP ccswitch_proxy_active_connections In-flight client requests.\n# TYPE ccswitch_proxy_active_connections gauge\nccswitch_proxy_active_connections {}",
            self.active_connections.load(Ordering::Relaxed)
        );
        for (name, help, value) in extra_gauges {
            let _ = writeln!(
                out,
                "# HELP {name} {help}\n# TYPE {name} gauge\n{name} {value}"
            );
        }

        let providers = self.providers.lock().unwrap_or_else(|e| e.into_inner());
        out.push_str("# HELP ccswitch_provider_attempts_total Upstream attempts per provider and result.\n# TYPE ccswitch_provider_attempts_total counter\n");
        for ((app, id), c) in providers.iter() {
            let labels = provider_labels(app, id, &c.name);
            let _ = writeln!(
                out,
                "ccswitch_provider_attempts_total{{{labels},result=\"success\"}} {}",
                c.success
            );
            let _ = writeln!(
                out,
                "ccswitch_provider_attempts_total{{{labels},result=\"failure\"}} {}",
                c.failure
            );
        }
        out.push_str("# HELP ccswitch_provider_latency_seconds Time to upstream response headers per attempt.\n# TYPE ccswitch_provider_latency_seconds histogram\n");
        for ((app, id), c) in providers.iter() {
            let labels = provider_labels(app, id, &c.name);
            let mut cumulative = 0u64;
            for (i, upper) in LATENCY_BUCKETS_SECS.iter().enumerate() {
                cumulative += c.buckets[i];
                let _ = writeln!(out, "ccswitch_provider_latency_seconds_bucket{{{labels},le=\"{upper}\"}} {cumulative}");
            }
            cumulative += c.buckets[LATENCY_BUCKETS_SECS.len()];
            let _ = writeln!(
                out,
                "ccswitch_provider_latency_seconds_bucket{{{labels},le=\"+Inf\"}} {cumulative}"
            );
            let _ = writeln!(
                out,
                "ccswitch_provider_latency_seconds_sum{{{labels}}} {}",
                c.latency_sum_secs
            );
            let _ = writeln!(
                out,
                "ccswitch_provider_latency_seconds_count{{{labels}}} {cumulative}"
            );
        }
        drop(providers);

        out.push_str("# HELP ccswitch_circuit_breaker_state Circuit breaker state (0=closed, 1=half-open, 2=open).\n# TYPE ccswitch_circuit_breaker_state gauge\n");
        for b in breakers {
            let _ = writeln!(
                out,
                "ccswitch_circuit_breaker_state{{app=\"{}\",provider_id=\"{}\"}} {}",
                escape_label(&b.app_type),
                escape_label(&b.provider_id),
                b.state
            );
        }
        out
    }

    pub fn connection_opened(&self) {
        self.active_connections.fetch_add(1, Ordering::Relaxed);
    }

    pub fn connection_closed(&self) {
        // Saturating decrement: never wrap below zero.
        let _ = self
            .active_connections
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
                Some(n.saturating_sub(1))
            });
    }

    #[cfg(test)]
    pub fn last_error(&self) -> Option<String> {
        self.detail().last_error.clone()
    }

    #[cfg(test)]
    pub fn active_connections(&self) -> usize {
        self.active_connections.load(Ordering::Relaxed)
    }

    /// Copy the counters into `status`, deriving `success_rate`.
    pub fn apply_to(&self, status: &mut ProxyStatus) {
        status.active_connections = self.active_connections.load(Ordering::Relaxed);
        status.total_requests = self.total_requests.load(Ordering::Relaxed);
        status.success_requests = self.success_requests.load(Ordering::Relaxed);
        status.failed_requests = self.failed_requests.load(Ordering::Relaxed);
        status.failover_count = self.failover_count.load(Ordering::Relaxed);
        status.success_rate = if status.total_requests > 0 {
            (status.success_requests as f32 / status.total_requests as f32) * 100.0
        } else {
            0.0
        };
        let detail = self.detail();
        status.current_provider = detail.current_provider.clone();
        status.current_provider_id = detail.current_provider_id.clone();
        status.last_request_at = detail.last_request_at.clone();
        status.last_error = detail.last_error.clone();
    }

    /// A status snapshot with only the counters filled in.
    #[cfg(test)]
    pub fn snapshot(&self) -> ProxyStatus {
        let mut status = ProxyStatus::default();
        self.apply_to(&mut status);
        status
    }
}

fn escape_label(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

fn provider_labels(app: &str, id: &str, name: &str) -> String {
    format!(
        "app=\"{}\",provider_id=\"{}\",provider=\"{}\"",
        escape_label(app),
        escape_label(id),
        escape_label(name)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prometheus_render_has_provider_histograms_and_breakers() {
        let metrics = ProxyMetrics::default();
        metrics.record_request_start();
        metrics.record_attempt_result(
            "claude",
            "p1",
            "Main \"A\"",
            true,
            Duration::from_millis(300),
        );
        metrics.record_attempt_result(
            "claude",
            "p1",
            "Main \"A\"",
            false,
            Duration::from_secs(400),
        );
        metrics.record_outcome(RequestOutcome::Served { failover: true });
        let text = metrics.render_prometheus(
            &[BreakerGauge {
                app_type: "claude".into(),
                provider_id: "p1".into(),
                state: 2,
            }],
            &[("ccswitch_test_gauge", "Test.", 7)],
        );
        assert!(text.contains("ccswitch_proxy_requests_total 1"), "{text}");
        assert!(text.contains("ccswitch_proxy_failovers_total 1"));
        assert!(text.contains(
            "ccswitch_provider_attempts_total{app=\"claude\",provider_id=\"p1\",provider=\"Main \\\"A\\\"\",result=\"success\"} 1"
        ), "{text}");
        assert!(text.contains("le=\"0.25\"} 0"));
        assert!(text.contains("le=\"0.5\"} 1"));
        assert!(text.contains("le=\"300\"} 1"));
        assert!(text.contains("le=\"+Inf\"} 2"));
        assert!(text.contains("ccswitch_provider_latency_seconds_count{app=\"claude\",provider_id=\"p1\",provider=\"Main \\\"A\\\"\"} 2"));
        assert!(
            text.contains("ccswitch_circuit_breaker_state{app=\"claude\",provider_id=\"p1\"} 2")
        );
        assert!(text.contains("ccswitch_test_gauge 7"));
        // Every non-comment line is `name{labels} value` or `name value`.
        for line in text.lines().filter(|l| !l.starts_with('#')) {
            assert_eq!(line.rsplitn(2, ' ').count(), 2, "{line}");
        }
    }

    #[test]
    fn record_outcome_updates_counters_and_derives_success_rate() {
        let metrics = ProxyMetrics::default();
        for _ in 0..4 {
            metrics.record_request_start();
        }
        metrics.record_attempt_target("p1", "Provider 1");
        metrics.record_outcome(RequestOutcome::Served { failover: false });
        metrics.record_outcome(RequestOutcome::Served { failover: true });
        metrics.record_outcome(RequestOutcome::Served { failover: false });
        metrics.record_outcome(RequestOutcome::Failed {
            error: "boom".into(),
        });

        let status = metrics.snapshot();
        assert_eq!(status.total_requests, 4);
        assert_eq!(status.success_requests, 3);
        assert_eq!(status.failed_requests, 1);
        assert_eq!(status.failover_count, 1);
        assert!((status.success_rate - 75.0).abs() < f32::EPSILON);
        assert_eq!(status.last_error.as_deref(), Some("boom"));
        assert_eq!(status.current_provider_id.as_deref(), Some("p1"));
        assert!(status.last_request_at.is_some());

        metrics.record_outcome(RequestOutcome::Served { failover: false });
        assert_eq!(metrics.last_error(), None, "success clears last_error");
    }

    #[test]
    fn active_connections_never_underflow() {
        let metrics = ProxyMetrics::default();
        metrics.connection_opened();
        metrics.connection_closed();
        metrics.connection_closed();
        assert_eq!(metrics.active_connections(), 0);
    }

    #[test]
    fn concurrent_updates_are_not_lost() {
        let metrics = ProxyMetrics::new();
        let threads: Vec<_> = (0..8)
            .map(|_| {
                let metrics = metrics.clone();
                std::thread::spawn(move || {
                    for _ in 0..1000 {
                        metrics.record_request_start();
                        metrics.record_outcome(RequestOutcome::Served { failover: false });
                    }
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
        let status = metrics.snapshot();
        assert_eq!(status.total_requests, 8000);
        assert_eq!(status.success_requests, 8000);
    }
}
