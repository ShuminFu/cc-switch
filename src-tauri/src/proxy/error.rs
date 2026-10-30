use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ProxyError {
    #[error("proxy server is already running")]
    AlreadyRunning,

    #[error("proxy server is not running")]
    NotRunning,

    #[error("failed to bind the listen address: {0}")]
    BindFailed(String),

    #[error("timed out waiting for the proxy server to stop")]
    StopTimeout,

    #[error("failed to stop the proxy server: {0}")]
    StopFailed(String),

    #[error("failed to forward the request upstream: {0}")]
    ForwardFailed(String),

    #[error("no provider is available for this request")]
    NoAvailableProvider,

    #[error("all providers are temporarily unavailable (circuit breakers open)")]
    AllProvidersCircuitOpen,

    #[error("no provider is configured for this app")]
    NoProvidersConfigured,

    #[allow(dead_code)]
    #[error("provider is unhealthy: {0}")]
    ProviderUnhealthy(String),

    #[error("upstream returned HTTP {status}: {body:?}")]
    UpstreamError { status: u16, body: Option<String> },

    #[error("maximum number of attempts exceeded")]
    MaxRetriesExceeded,

    #[error("database error: {0}")]
    DatabaseError(String),

    #[error("provider configuration error: {0}")]
    ConfigError(String),

    #[error("protocol conversion error: {0}")]
    TransformError(String),

    #[error("invalid request: {0}")]
    InvalidRequest(String),

    #[error("timeout: {0}")]
    Timeout(String),

    /// 流式响应空闲超时
    #[allow(dead_code)]
    #[error("stream idle timeout: no data for {0}s")]
    StreamIdleTimeout(u64),

    /// 认证错误
    #[error("authentication failed: {0}")]
    AuthError(String),

    #[error("internal proxy error: {0}")]
    Internal(String),
}

impl ProxyError {
    /// Stable machine-readable error code used in proxy error bodies.
    pub fn code(&self) -> &'static str {
        match self {
            ProxyError::ForwardFailed(_) => "cc_switch_forward_failed",
            ProxyError::Timeout(_) | ProxyError::StreamIdleTimeout(_) => "cc_switch_timeout",
            ProxyError::NoAvailableProvider => "cc_switch_no_available_provider",
            ProxyError::AllProvidersCircuitOpen => "cc_switch_all_providers_circuit_open",
            ProxyError::NoProvidersConfigured => "cc_switch_no_providers_configured",
            ProxyError::MaxRetriesExceeded => "cc_switch_max_retries_exceeded",
            ProxyError::ProviderUnhealthy(_) => "cc_switch_provider_unhealthy",
            ProxyError::ConfigError(_) => "cc_switch_config_error",
            ProxyError::TransformError(_) => "cc_switch_transform_error",
            ProxyError::InvalidRequest(_) => "cc_switch_invalid_request",
            ProxyError::AuthError(_) => "cc_switch_auth_error",
            ProxyError::UpstreamError { .. } => "cc_switch_upstream_error",
            ProxyError::DatabaseError(_) => "cc_switch_database_error",
            ProxyError::Internal(_) => "cc_switch_internal_error",
            ProxyError::AlreadyRunning
            | ProxyError::NotRunning
            | ProxyError::BindFailed(_)
            | ProxyError::StopTimeout
            | ProxyError::StopFailed(_) => "cc_switch_proxy_error",
        }
    }

    /// Map a client request-body JSON parse failure to a 400 client error.
    ///
    /// The serde error carries line/column, which is enough for the client to
    /// locate the problem. Body *read* failures stay `Internal`.
    pub fn invalid_request_json(err: &serde_json::Error) -> Self {
        ProxyError::InvalidRequest(format!("Request body is not valid JSON: {err}"))
    }
}

impl IntoResponse for ProxyError {
    fn into_response(self) -> Response {
        let (status, body) = match &self {
            ProxyError::UpstreamError {
                status: upstream_status,
                body: upstream_body,
            } => {
                let http_status =
                    StatusCode::from_u16(*upstream_status).unwrap_or(StatusCode::BAD_GATEWAY);

                // 尝试解析上游响应体为 JSON，如果失败则包装为字符串
                let error_body = if let Some(body_str) = upstream_body {
                    if let Ok(json_body) = serde_json::from_str::<serde_json::Value>(body_str) {
                        // 上游返回的是 JSON，直接透传
                        json_body
                    } else {
                        // 上游返回的不是 JSON，包装为错误消息
                        json!({
                            "error": {
                                "message": body_str,
                                "type": "upstream_error",
                            }
                        })
                    }
                } else {
                    json!({
                        "error": {
                            "message": format!("Upstream error (status {})", upstream_status),
                            "type": "upstream_error",
                        }
                    })
                };

                (http_status, error_body)
            }
            _ => {
                // One status table for responses and logs (PROXY-28).
                let http_status = StatusCode::from_u16(
                    crate::proxy::error_mapper::map_proxy_error_to_status(&self),
                )
                .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
                let message = self.to_string();

                // Anthropic-style envelope (`type: "error"`) with a stable
                // machine-readable `code` so clients and the error-rate statistics
                // can tell proxy-side failures from client mistakes.
                let error_body = json!({
                    "type": "error",
                    "error": {
                        "message": message,
                        "type": "proxy_error",
                        "code": self.code(),
                    }
                });

                (http_status, error_body)
            }
        };

        (status, Json(body)).into_response()
    }
}

/// 错误分类
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCategory {
    /// 可重试错误（网络问题、5xx）
    Retryable, // 网络超时、5xx 错误
    /// 不可重试错误（4xx、认证失败）
    NonRetryable, // 认证失败、参数错误、4xx 错误
    #[allow(dead_code)]
    ClientAbort, // 客户端主动中断
}

#[cfg(test)]
mod tests {
    use super::*;
    use http_body_util::BodyExt;

    #[tokio::test]
    async fn proxy_error_body_is_anthropic_style_with_code() {
        let response = ProxyError::InvalidRequest("bad".to_string()).into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["type"], "error");
        assert_eq!(body["error"]["type"], "proxy_error");
        assert_eq!(body["error"]["code"], "cc_switch_invalid_request");
    }

    #[test]
    fn invalid_request_json_keeps_serde_position() {
        let err = serde_json::from_slice::<serde_json::Value>(b"{").unwrap_err();
        let proxy_error = ProxyError::invalid_request_json(&err);
        assert!(matches!(proxy_error, ProxyError::InvalidRequest(_)));
        assert!(proxy_error.to_string().contains("column"));
    }

    #[test]
    fn display_strings_are_ascii_for_api_clients() {
        // PROXY-31: these messages reach API clients verbatim.
        let errors = [
            ProxyError::AlreadyRunning,
            ProxyError::NotRunning,
            ProxyError::BindFailed("x".into()),
            ProxyError::StopTimeout,
            ProxyError::StopFailed("x".into()),
            ProxyError::ForwardFailed("x".into()),
            ProxyError::NoAvailableProvider,
            ProxyError::AllProvidersCircuitOpen,
            ProxyError::NoProvidersConfigured,
            ProxyError::ProviderUnhealthy("x".into()),
            ProxyError::UpstreamError {
                status: 500,
                body: None,
            },
            ProxyError::MaxRetriesExceeded,
            ProxyError::DatabaseError("x".into()),
            ProxyError::ConfigError("x".into()),
            ProxyError::TransformError("x".into()),
            ProxyError::InvalidRequest("x".into()),
            ProxyError::Timeout("x".into()),
            ProxyError::StreamIdleTimeout(5),
            ProxyError::AuthError("x".into()),
            ProxyError::Internal("x".into()),
        ];
        for error in errors {
            assert!(error.to_string().is_ascii(), "{error}");
            assert!(error.code().starts_with("cc_switch_"));
        }
    }

    #[tokio::test]
    async fn messages_route_error_body_carries_status_and_code() {
        let response = ProxyError::AllProvidersCircuitOpen.into_response();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            body["error"]["code"],
            "cc_switch_all_providers_circuit_open"
        );
    }
}
