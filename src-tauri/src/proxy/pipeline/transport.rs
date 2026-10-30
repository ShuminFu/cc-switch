//! 上游发送
//!
//! 在 pooled reqwest 与 raw hyper 写（保持客户端原始 header 大小写）之间选择，
//! 并按请求是否流式确定等待响应头的超时。

use crate::provider::Provider;
use crate::proxy::error::ProxyError;
use crate::proxy::hyper_client::ProxyResponse;
use crate::proxy::protocol::ClaudeApiFormat;
use http::Extensions;
use serde_json::Value;
use std::time::Duration;

/// 一次待发送的上游请求
pub(crate) struct UpstreamRequest<'a> {
    pub method: &'a http::Method,
    pub url: &'a str,
    pub headers: http::HeaderMap,
    pub extensions: &'a Extensions,
    pub body: Vec<u8>,
    pub is_streaming: bool,
    /// 是否需要原样保留客户端 header 大小写（决定能否走 raw hyper 写）
    pub preserve_exact_header_case: bool,
}

/// 发送超时配置
#[derive(Debug, Clone, Copy)]
pub(crate) struct TransportTimeouts {
    /// 非流式请求超时（0 表示不限，等待响应头时回退为 600 秒）
    pub non_streaming: Duration,
    /// 流式请求首包超时（0 表示不限）
    pub streaming_first_byte: Duration,
}

impl TransportTimeouts {
    /// 等待响应头的超时：流式请求使用首包超时（非 0 时），两条发送路径共用，
    /// 避免 raw hyper 路径按非流式超时（默认 600s）等待，拖慢故障转移。
    fn header_timeout(&self, is_streaming: bool) -> Duration {
        let timeout = if self.non_streaming.is_zero() {
            Duration::from_secs(600) // 默认 600 秒
        } else {
            self.non_streaming
        };
        if is_streaming && !self.streaming_first_byte.is_zero() {
            self.streaming_first_byte
        } else {
            timeout
        }
    }
}

/// 发送请求，返回上游响应（不检查状态码）
pub(crate) async fn send(
    request: UpstreamRequest<'_>,
    timeouts: TransportTimeouts,
) -> Result<ProxyResponse, ProxyError> {
    let header_timeout = timeouts.header_timeout(request.is_streaming);

    // 获取全局代理 URL
    let upstream_proxy_url: Option<String> = crate::proxy::http_client::get_current_proxy_url();

    // SOCKS5 代理不支持 CONNECT 隧道，需要用 reqwest
    let is_socks_proxy = upstream_proxy_url
        .as_deref()
        .map(|u| u.starts_with("socks5"))
        .unwrap_or(false);
    let preserve_exact_header_case = request.preserve_exact_header_case;

    if is_socks_proxy || !preserve_exact_header_case {
        // OpenAI / Copilot / Codex 类后端不依赖原始 header 大小写；走 reqwest
        // 连接池，避免 raw TCP/TLS path 每次请求都重新握手。SOCKS5 也只能走 reqwest。
        log::debug!(
            "[Forwarder] Using pooled reqwest client (preserve_exact_header_case={preserve_exact_header_case}, socks_proxy={is_socks_proxy})"
        );
        let client = crate::proxy::http_client::get();
        let mut builder = client.request(request.method.clone(), request.url);
        if request.is_streaming {
            // reqwest 的 timeout 是整请求超时；流式请求交给 response_processor
            // 的首包/静默期超时控制，避免长流被总时长误杀。
            builder = builder.timeout(Duration::from_secs(24 * 60 * 60));
        } else if !timeouts.non_streaming.is_zero() {
            builder = builder.timeout(timeouts.non_streaming);
        }
        for (key, value) in &request.headers {
            builder = builder.header(key, value);
        }
        let send = builder.body(request.body).send();
        let send_result = if request.is_streaming {
            tokio::time::timeout(header_timeout, send)
                .await
                .map_err(|_| {
                    ProxyError::Timeout(format!(
                        "流式响应首包超时: {}s（上游未返回响应头）",
                        header_timeout.as_secs()
                    ))
                })?
        } else {
            send.await
        };
        let reqwest_resp = send_result.map_err(map_reqwest_send_error)?;
        Ok(ProxyResponse::Reqwest(reqwest_resp))
    } else {
        // HTTP 代理或直连：走 hyper raw write（保持 header 大小写）
        // 如果有 HTTP 代理，hyper_client 会用 CONNECT 隧道穿过代理
        let uri: http::Uri = request.url.parse().map_err(|e| {
            ProxyError::ForwardFailed(format!("Invalid URL '{}': {e}", request.url))
        })?;
        crate::proxy::hyper_client::send_request(
            uri,
            request.method.clone(),
            request.headers,
            request.extensions.clone(),
            request.body,
            header_timeout,
            upstream_proxy_url.as_deref(),
        )
        .await
    }
}

pub(crate) fn should_preserve_exact_header_case(
    adapter_name: &str,
    provider: &Provider,
    resolved_claude_api_format: Option<ClaudeApiFormat>,
    is_copilot: bool,
) -> bool {
    if matches!(adapter_name, "Codex" | "Gemini") {
        return false;
    }

    if is_copilot || provider.is_codex_oauth() {
        return false;
    }

    matches!(
        resolved_claude_api_format,
        None | Some(ClaudeApiFormat::Anthropic)
    )
}

/// 输出请求信息日志
pub(crate) fn log_outbound_request(tag: &str, url: &str, body: &Value) {
    let request_model = body
        .get("model")
        .and_then(|v| v.as_str())
        .unwrap_or("<none>");
    log::info!("[{tag}] >>> 请求 URL: {url} (model={request_model})");
    if log::log_enabled!(log::Level::Debug) {
        if let Ok(body_str) = serde_json::to_string(body) {
            log::debug!(
                "[{tag}] >>> 请求体内容 ({}字节): {}",
                body_str.len(),
                body_str
            );
        }
    }
}

fn map_reqwest_send_error(error: reqwest::Error) -> ProxyError {
    if error.is_timeout() {
        ProxyError::Timeout(format!("请求超时: {error}"))
    } else if error.is_connect() {
        ProxyError::ForwardFailed(format!("连接失败: {error}"))
    } else {
        ProxyError::ForwardFailed(error.to_string())
    }
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
    fn streaming_header_wait_uses_first_byte_timeout_when_set() {
        let timeouts = TransportTimeouts {
            non_streaming: Duration::ZERO,
            streaming_first_byte: Duration::from_secs(5),
        };
        assert_eq!(timeouts.header_timeout(true), Duration::from_secs(5));
        assert_eq!(timeouts.header_timeout(false), Duration::from_secs(600));

        let timeouts = TransportTimeouts {
            non_streaming: Duration::from_secs(30),
            streaming_first_byte: Duration::ZERO,
        };
        assert_eq!(timeouts.header_timeout(true), Duration::from_secs(30));
        assert_eq!(timeouts.header_timeout(false), Duration::from_secs(30));
    }

    #[test]
    fn exact_header_case_preserved_for_native_claude_only() {
        let provider = test_provider_with_type(None);

        assert!(should_preserve_exact_header_case(
            "Claude",
            &provider,
            Some(ClaudeApiFormat::Anthropic),
            false
        ));
        assert!(!should_preserve_exact_header_case(
            "Claude",
            &provider,
            Some(ClaudeApiFormat::OpenAiResponses),
            false
        ));
        assert!(!should_preserve_exact_header_case(
            "Codex", &provider, None, false
        ));
        assert!(!should_preserve_exact_header_case(
            "Gemini", &provider, None, false
        ));
    }

    #[test]
    fn exact_header_case_skipped_for_codex_oauth_and_copilot() {
        let codex_oauth = test_provider_with_type(Some("codex_oauth"));
        let copilot = test_provider_with_type(Some("github_copilot"));

        assert!(!should_preserve_exact_header_case(
            "Claude",
            &codex_oauth,
            Some(ClaudeApiFormat::OpenAiResponses),
            false
        ));
        assert!(!should_preserve_exact_header_case(
            "Claude",
            &copilot,
            Some(ClaudeApiFormat::OpenAiChat),
            true
        ));
    }
}
