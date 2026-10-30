//! 尝试失败的分类与日志
//!
//! 决定一次失败是换下一家 provider 重试，还是直接把错误返回给客户端；
//! 并生成故障转移循环的失败日志。

use crate::provider::Provider;
use crate::proxy::error::{ErrorCategory, ProxyError};
use crate::proxy::log_codes::fwd as log_fwd;
use serde_json::Value;

pub(crate) fn categorize_proxy_error(error: &ProxyError, provider: &Provider) -> ErrorCategory {
    // Authentication belongs to the Codex client for the built-in official
    // route. Retrying another provider would silently move the conversation
    // away from the selected official account and poison its health state.
    if crate::proxy::providers::is_codex_official_provider(provider)
        && (matches!(error, ProxyError::AuthError(_))
            || matches!(
                error,
                ProxyError::UpstreamError {
                    status: 401 | 403,
                    ..
                }
            ))
    {
        return ErrorCategory::NonRetryable;
    }

    match error {
        // 网络和上游错误：都应该尝试下一个供应商
        ProxyError::Timeout(_) => ErrorCategory::Retryable,
        ProxyError::ForwardFailed(_) => ErrorCategory::Retryable,
        ProxyError::ProviderUnhealthy(_) => ErrorCategory::Retryable,
        // 上游 HTTP 错误：按状态码分桶。
        //
        // 客户端请求自身有问题的状态码无论换哪个 provider 都会被拒绝，
        // 继续轮询只会放大错误率、污染熔断器健康度、浪费配额：
        //   400 Bad Request / 422 Unprocessable Entity   ← 请求体格式或语义错误
        //   405 Method Not Allowed / 406 Not Acceptable  ← 方法或 Accept 错误
        //   413 Payload Too Large / 414 URI Too Long     ← 客户端构造超限
        //   415 Unsupported Media Type                    ← Content-Type 错误
        //   501 Not Implemented                           ← 上游协议确实不支持
        //
        // 其他 4xx（401/403/404/408/409/429/451 等）和全部 5xx 都保留
        // Retryable —— 换一家 provider 可能持有不同的 key、配额、地域或模型映射。
        ProxyError::UpstreamError { status, .. } => match *status {
            400 | 405 | 406 | 413 | 414 | 415 | 422 | 501 => ErrorCategory::NonRetryable,
            _ => ErrorCategory::Retryable,
        },
        // Provider 级配置/转换问题：换一个 Provider 可能就能成功
        ProxyError::ConfigError(_) => ErrorCategory::Retryable,
        ProxyError::TransformError(_) => ErrorCategory::Retryable,
        ProxyError::AuthError(_) => ErrorCategory::Retryable,
        ProxyError::StreamIdleTimeout(_) => ErrorCategory::Retryable,
        // 无可用供应商：所有供应商都试过了，无法重试
        ProxyError::NoAvailableProvider => ErrorCategory::NonRetryable,
        // 其他错误（数据库/内部错误等）：不是换供应商能解决的问题
        _ => ErrorCategory::NonRetryable,
    }
}

pub(crate) fn build_retryable_failure_log(
    provider_name: &str,
    attempted_providers: usize,
    total_providers: usize,
    error: &ProxyError,
) -> (&'static str, String) {
    let error_summary = summarize_proxy_error(error);

    if total_providers <= 1 {
        (
            log_fwd::SINGLE_PROVIDER_FAILED,
            format!("Provider {provider_name} 请求失败: {error_summary}"),
        )
    } else {
        (
            log_fwd::PROVIDER_FAILED_RETRY,
            format!(
                "Provider {provider_name} 失败，继续尝试下一个 ({attempted_providers}/{total_providers}): {error_summary}"
            ),
        )
    }
}

pub(crate) fn build_terminal_failure_log(
    attempted_providers: usize,
    total_providers: usize,
    last_error: Option<&ProxyError>,
) -> Option<(&'static str, String)> {
    if total_providers <= 1 {
        return None;
    }

    let error_summary = last_error
        .map(summarize_proxy_error)
        .unwrap_or_else(|| "未知错误".to_string());

    Some((
        log_fwd::ALL_PROVIDERS_FAILED,
        format!(
            "已尝试 {attempted_providers}/{total_providers} 个 Provider，均失败。最后错误: {error_summary}"
        ),
    ))
}

fn summarize_proxy_error(error: &ProxyError) -> String {
    match error {
        ProxyError::UpstreamError { status, body } => {
            let body_summary = body
                .as_deref()
                .map(summarize_upstream_body)
                .filter(|summary| !summary.is_empty());

            match body_summary {
                Some(summary) => format!("上游 HTTP {status}: {summary}"),
                None => format!("上游 HTTP {status}"),
            }
        }
        ProxyError::Timeout(message) => {
            format!("请求超时: {}", summarize_text_for_log(message, 180))
        }
        ProxyError::ForwardFailed(message) => {
            format!("请求转发失败: {}", summarize_text_for_log(message, 180))
        }
        ProxyError::TransformError(message) => {
            format!("响应转换失败: {}", summarize_text_for_log(message, 180))
        }
        ProxyError::ConfigError(message) => {
            format!("配置错误: {}", summarize_text_for_log(message, 180))
        }
        ProxyError::AuthError(message) => {
            format!("认证失败: {}", summarize_text_for_log(message, 180))
        }
        _ => summarize_text_for_log(&error.to_string(), 180),
    }
}

fn summarize_upstream_body(body: &str) -> String {
    if let Ok(json_body) = serde_json::from_str::<Value>(body) {
        if let Some(message) = extract_json_error_message(&json_body) {
            return summarize_text_for_log(&message, 180);
        }

        if let Ok(compact_json) = serde_json::to_string(&json_body) {
            return summarize_text_for_log(&compact_json, 180);
        }
    }

    summarize_text_for_log(body, 180)
}

fn extract_json_error_message(body: &Value) -> Option<String> {
    let candidates = [
        body.pointer("/error/message"),
        body.pointer("/message"),
        body.pointer("/detail"),
        body.pointer("/error"),
    ];

    candidates
        .into_iter()
        .flatten()
        .find_map(|value| value.as_str().map(ToString::to_string))
}

fn summarize_text_for_log(text: &str, max_chars: usize) -> String {
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let trimmed = normalized.trim();

    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }

    let truncated: String = trimmed.chars().take(max_chars).collect();
    let truncated = truncated.trim_end();
    format!("{truncated}...")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn test_provider_with_type(provider_type: Option<&str>) -> Provider {
        let mut provider = Provider::with_id(
            "provider-1".to_string(),
            "Provider 1".to_string(),
            json!({}),
            None,
        );
        provider.meta = provider_type.map(|value| crate::provider::ProviderMeta {
            provider_type: Some(value.to_string()),
            ..Default::default()
        });
        provider
    }

    #[test]
    fn single_provider_retryable_log_uses_single_provider_code() {
        let error = ProxyError::UpstreamError {
            status: 429,
            body: Some(r#"{"error":{"message":"rate limit exceeded"}}"#.to_string()),
        };

        let (code, message) = build_retryable_failure_log("PackyCode-response", 1, 1, &error);

        assert_eq!(code, log_fwd::SINGLE_PROVIDER_FAILED);
        assert!(message.contains("Provider PackyCode-response 请求失败"));
        assert!(message.contains("上游 HTTP 429"));
        assert!(message.contains("rate limit exceeded"));
        assert!(!message.contains("切换下一个"));
    }

    #[test]
    fn multi_provider_retryable_log_keeps_failover_wording() {
        let error = ProxyError::Timeout("upstream timed out after 30s".to_string());

        let (code, message) = build_retryable_failure_log("primary", 1, 3, &error);

        assert_eq!(code, log_fwd::PROVIDER_FAILED_RETRY);
        assert!(message.contains("继续尝试下一个 (1/3)"));
        assert!(message.contains("请求超时"));
    }

    #[test]
    fn single_provider_has_no_terminal_all_failed_log() {
        assert!(build_terminal_failure_log(1, 1, None).is_none());
    }

    #[test]
    fn multi_provider_terminal_log_contains_last_error_summary() {
        let error = ProxyError::ForwardFailed("connection reset by peer".to_string());

        let (code, message) =
            build_terminal_failure_log(2, 2, Some(&error)).expect("expected terminal log");

        assert_eq!(code, log_fwd::ALL_PROVIDERS_FAILED);
        assert!(message.contains("已尝试 2/2 个 Provider，均失败"));
        assert!(message.contains("connection reset by peer"));
    }

    #[test]
    fn summarize_upstream_body_prefers_json_message() {
        let body = json!({
            "error": {
                "message": "invalid_request_error: unsupported field"
            },
            "request_id": "req_123"
        });

        let summary = summarize_upstream_body(&body.to_string());

        assert_eq!(summary, "invalid_request_error: unsupported field");
    }

    #[test]
    fn summarize_text_for_log_collapses_whitespace_and_truncates() {
        let summary = summarize_text_for_log("line1\n\n line2   line3", 12);

        assert_eq!(summary, "line1 line2...");
    }

    #[test]
    fn invalid_client_history_is_not_retryable() {
        let provider = test_provider_with_type(None);
        assert_eq!(
            categorize_proxy_error(
                &ProxyError::InvalidRequest("invalid historical tool arguments".to_string()),
                &provider,
            ),
            ErrorCategory::NonRetryable
        );
    }

    #[test]
    fn official_codex_auth_failures_are_not_retryable() {
        let mut provider = test_provider_with_type(None);
        provider.id = "codex-official".to_string();
        provider.category = Some("official".to_string());

        for error in [
            ProxyError::AuthError("restart Codex".to_string()),
            ProxyError::UpstreamError {
                status: 401,
                body: None,
            },
            ProxyError::UpstreamError {
                status: 403,
                body: None,
            },
        ] {
            assert_eq!(
                categorize_proxy_error(&error, &provider),
                ErrorCategory::NonRetryable
            );
        }
    }
}
