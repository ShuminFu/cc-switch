//! Lock-free proxy counters (PROXY-34).
//!
//! The request pipeline used to mutate `ProxyStatus` under an async `RwLock` in a
//! dozen places, recomputing `success_rate` by hand each time. Counters now live in
//! atomics updated through a handful of `record_*` calls, the few string fields sit
//! behind a short synchronous mutex (never held across `.await`), and
//! `success_rate` is derived when the status is read.

use crate::proxy::types::ProxyStatus;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

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

#[cfg(test)]
mod tests {
    use super::*;

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
