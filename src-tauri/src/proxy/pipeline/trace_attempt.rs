//! 单次尝试的追踪记录
//!
//! 把一次 provider 尝试的结果（上游状态码 / 失败原因）与已应用的恢复策略写入
//! [`TraceHandle`] 的当前尝试。

use super::recovery::RecoveryStrategy;
use crate::proxy::error::ProxyError;
use crate::proxy::error_mapper::get_error_message;
use crate::proxy::trace::TraceHandle;

/// 记录当前尝试的结果：`Ok` 为上游响应状态码，`Err` 为该次尝试的最终错误
pub(crate) fn record_attempt_outcome(
    trace: &TraceHandle,
    outcome: Result<u16, &ProxyError>,
    recovery: &[RecoveryStrategy],
) {
    let applied: Vec<&str> = recovery.iter().map(|strategy| strategy.as_str()).collect();
    match outcome {
        Ok(status) => trace.succeed_attempt(status, &applied),
        Err(error) => {
            let upstream_status = match error {
                ProxyError::UpstreamError { status, .. } => Some(*status),
                _ => None,
            };
            trace.fail_attempt(&get_error_message(error), upstream_status, &applied);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proxy::trace::{AttemptStatus, TraceStore};

    fn begin_attempt(store: &TraceStore) -> TraceHandle {
        let trace = store.begin(
            "claude",
            &http::Method::POST,
            &"/v1/messages".parse().unwrap(),
            false,
        );
        trace.begin_attempt("p1", "Provider 1");
        trace
    }

    #[test]
    fn success_records_status_and_applied_recovery() {
        let store = TraceStore::default();
        let trace = begin_attempt(&store);

        record_attempt_outcome(
            &trace,
            Ok(200),
            &[
                RecoveryStrategy::MediaDowngrade,
                RecoveryStrategy::ThinkingBudgetRectifier,
            ],
        );

        let attempt = &trace.snapshot().attempts[0];
        assert_eq!(attempt.status, AttemptStatus::Succeeded);
        assert_eq!(attempt.upstream_status, Some(200));
        assert_eq!(
            attempt.recovery,
            vec![
                RecoveryStrategy::MediaDowngrade.as_str(),
                RecoveryStrategy::ThinkingBudgetRectifier.as_str(),
            ]
        );
        assert!(attempt.reason.is_none());
    }

    #[test]
    fn upstream_error_records_its_status_and_reason() {
        let store = TraceStore::default();
        let trace = begin_attempt(&store);
        let error = ProxyError::UpstreamError {
            status: 503,
            body: Some("overloaded".to_string()),
        };

        record_attempt_outcome(&trace, Err(&error), &[]);

        let attempt = &trace.snapshot().attempts[0];
        assert_eq!(attempt.status, AttemptStatus::Failed);
        assert_eq!(attempt.upstream_status, Some(503));
        assert_eq!(
            attempt.reason.as_deref(),
            Some(get_error_message(&error).as_str())
        );
        assert!(attempt.recovery.is_empty());
    }

    #[test]
    fn transport_error_has_no_upstream_status() {
        let store = TraceStore::default();
        let trace = begin_attempt(&store);
        let error = ProxyError::Timeout("请求超时".to_string());

        record_attempt_outcome(
            &trace,
            Err(&error),
            &[RecoveryStrategy::ThinkingSignatureRectifier],
        );

        let attempt = &trace.snapshot().attempts[0];
        assert_eq!(attempt.status, AttemptStatus::Failed);
        assert_eq!(attempt.upstream_status, None);
        assert_eq!(
            attempt.recovery,
            vec![RecoveryStrategy::ThinkingSignatureRectifier.as_str()]
        );
    }
}
