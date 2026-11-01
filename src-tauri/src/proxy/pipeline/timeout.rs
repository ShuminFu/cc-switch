//! One timeout policy for every upstream attempt (PROXY-08).
//!
//! The policy is resolved once per client request from the per-app
//! [`AppProxyConfig`] and applied identically by both HTTP stacks (the pooled
//! reqwest client and the hyper raw-write path), by the in-loop response priming
//! and by the passthrough stream.
//!
//! | phase                          | failover on, value > 0         | value = 0, or failover off              |
//! |--------------------------------|--------------------------------|-----------------------------------------|
//! | TCP connect (+ proxy tunnel)   | [`CONNECT_TIMEOUT`] (10 s)     | [`CONNECT_TIMEOUT`] (10 s)              |
//! | streaming: headers + 1st chunk | `streaming_first_byte_timeout` | headers: [`FALLBACK_RESPONSE_TIMEOUT`], first chunk: unbounded |
//! | streaming: gap between chunks  | `streaming_idle_timeout`       | unbounded                               |
//! | non-streaming: headers + body  | `non_streaming_timeout`, one deadline (never doubled) | headers: [`FALLBACK_RESPONSE_TIMEOUT`], body: unbounded |
//!
//! "First byte" is measured from the moment the attempt starts sending, so the
//! header wait and the first-chunk wait share one deadline. Turning failover off
//! keeps its historical meaning (the configured phase limits are bypassed and only
//! the fallback ceiling applies) and limits the request to one provider.

use crate::proxy::types::AppProxyConfig;
use crate::proxy::ProxyError;
use std::time::Duration;

/// Connect timeout shared by both stacks (the reqwest client is built with it too).
pub const CONNECT_TIMEOUT: Duration = crate::proxy::hyper_client::CONNECT_TIMEOUT;

/// Ceiling for the response-header wait when no phase limit is configured.
pub const FALLBACK_RESPONSE_TIMEOUT: Duration = Duration::from_secs(600);

/// Per-request reqwest timeout for streaming requests. reqwest only offers a
/// whole-request timeout, which would cut long streams; streams are bounded by the
/// first-byte deadline and the idle timeout instead.
pub const STREAMING_REQUEST_CEILING: Duration = Duration::from_secs(24 * 60 * 60);

/// Effective timeouts for one client request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeoutPolicy {
    pub connect: Duration,
    /// Streaming: deadline for the response headers and the first body chunk.
    pub first_byte: Option<Duration>,
    /// Streaming: maximum silence between two chunks after the first one.
    pub idle: Option<Duration>,
    /// Non-streaming: deadline for the response headers and the complete body.
    pub total: Option<Duration>,
}

impl Default for TimeoutPolicy {
    /// No phase limits (the policy used when failover is off).
    fn default() -> Self {
        Self {
            connect: CONNECT_TIMEOUT,
            first_byte: None,
            idle: None,
            total: None,
        }
    }
}

fn opt_secs(value: u64) -> Option<Duration> {
    (value > 0).then(|| Duration::from_secs(value))
}

impl TimeoutPolicy {
    /// Resolve the policy for one request from the per-app proxy config.
    pub fn from_app_config(config: &AppProxyConfig) -> Self {
        if !config.auto_failover_enabled {
            return Self::default();
        }
        Self::from_secs(
            u64::from(config.non_streaming_timeout),
            u64::from(config.streaming_first_byte_timeout),
            u64::from(config.streaming_idle_timeout),
        )
    }

    /// Build a policy from raw seconds (0 = phase disabled).
    pub fn from_secs(total: u64, first_byte: u64, idle: u64) -> Self {
        Self {
            connect: CONNECT_TIMEOUT,
            first_byte: opt_secs(first_byte),
            idle: opt_secs(idle),
            total: opt_secs(total),
        }
    }

    /// Deadline for receiving the response headers of one attempt.
    pub fn response_headers(&self, streaming: bool) -> Duration {
        let phase = if streaming {
            self.first_byte
        } else {
            self.total
        };
        phase.unwrap_or(FALLBACK_RESPONSE_TIMEOUT)
    }

    /// Whole-request timeout handed to reqwest for one attempt.
    pub fn reqwest_request_timeout(&self, streaming: bool) -> Duration {
        if streaming {
            STREAMING_REQUEST_CEILING
        } else {
            self.total.unwrap_or(FALLBACK_RESPONSE_TIMEOUT)
        }
    }

    /// Body read limit used by response handlers as a safety net (zero = none).
    /// Bodies are normally already buffered inside the retry loop.
    pub fn body_read_timeout(&self) -> Duration {
        self.total.unwrap_or(Duration::ZERO)
    }

    /// The streaming limits in the shape the passthrough stream expects.
    pub fn streaming_config(&self) -> crate::proxy::handler_context::StreamingTimeoutConfig {
        crate::proxy::handler_context::StreamingTimeoutConfig {
            first_byte_timeout: self.first_byte.map_or(0, |d| d.as_secs()),
            idle_timeout: self.idle.map_or(0, |d| d.as_secs()),
        }
    }
}

/// Time left until `deadline`, or a timeout error naming the phase.
pub fn remaining(
    deadline: tokio::time::Instant,
    phase: &str,
    limit: Duration,
) -> Result<Duration, ProxyError> {
    let now = tokio::time::Instant::now();
    if now >= deadline {
        return Err(phase_timeout(phase, limit));
    }
    Ok(deadline - now)
}

/// The timeout error for one phase.
pub fn phase_timeout(phase: &str, limit: Duration) -> ProxyError {
    ProxyError::Timeout(format!("{phase} timed out after {}s", limit.as_secs()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(failover: bool, total: u32, first_byte: u32, idle: u32) -> AppProxyConfig {
        AppProxyConfig {
            app_type: "claude".into(),
            enabled: true,
            auto_failover_enabled: failover,
            max_retries: 3,
            streaming_first_byte_timeout: first_byte,
            streaming_idle_timeout: idle,
            non_streaming_timeout: total,
            circuit_failure_threshold: 4,
            circuit_success_threshold: 2,
            circuit_timeout_seconds: 60,
            circuit_error_rate_threshold: 0.6,
            circuit_min_requests: 10,
        }
    }

    /// The table in the module docs, row by row.
    #[test]
    fn effective_timeout_table() {
        let on = TimeoutPolicy::from_app_config(&config(true, 300, 45, 90));
        assert_eq!(on.connect, CONNECT_TIMEOUT);
        assert_eq!(on.response_headers(true), Duration::from_secs(45));
        assert_eq!(on.response_headers(false), Duration::from_secs(300));
        assert_eq!(on.idle, Some(Duration::from_secs(90)));
        assert_eq!(on.reqwest_request_timeout(false), Duration::from_secs(300));
        assert_eq!(on.reqwest_request_timeout(true), STREAMING_REQUEST_CEILING);
        assert_eq!(on.body_read_timeout(), Duration::from_secs(300));
        assert_eq!(on.streaming_config().first_byte_timeout, 45);
        assert_eq!(on.streaming_config().idle_timeout, 90);

        let zeros = TimeoutPolicy::from_app_config(&config(true, 0, 0, 0));
        assert_eq!(zeros.response_headers(true), FALLBACK_RESPONSE_TIMEOUT);
        assert_eq!(zeros.response_headers(false), FALLBACK_RESPONSE_TIMEOUT);
        assert_eq!(zeros.idle, None);
        assert_eq!(zeros.body_read_timeout(), Duration::ZERO);

        let off = TimeoutPolicy::from_app_config(&config(false, 300, 45, 90));
        assert_eq!(off, TimeoutPolicy::default());
        assert_eq!(off.connect, CONNECT_TIMEOUT);
        assert_eq!(off.response_headers(true), FALLBACK_RESPONSE_TIMEOUT);
        assert_eq!(
            off.reqwest_request_timeout(false),
            FALLBACK_RESPONSE_TIMEOUT
        );
        assert_eq!(off.streaming_config().idle_timeout, 0);
    }

    #[test]
    fn streaming_header_wait_uses_first_byte_not_total() {
        // Before PROXY-08 the hyper path waited `non_streaming_timeout` for the
        // headers of a streaming request while reqwest used the first-byte limit.
        let policy = TimeoutPolicy::from_secs(600, 30, 0);
        assert_eq!(policy.response_headers(true), Duration::from_secs(30));
        assert_eq!(policy.response_headers(false), Duration::from_secs(600));
    }

    #[tokio::test]
    async fn remaining_reports_the_phase_once_the_deadline_passed() {
        let deadline = tokio::time::Instant::now();
        let err = remaining(deadline, "response body", Duration::from_secs(5)).unwrap_err();
        assert!(err.to_string().contains("response body timed out after 5s"));
        let later = tokio::time::Instant::now() + Duration::from_secs(5);
        assert!(remaining(later, "x", Duration::from_secs(5)).unwrap() > Duration::ZERO);
    }
}
