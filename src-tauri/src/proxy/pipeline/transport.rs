//! Transport stage: send one finished upstream request over the right HTTP stack
//! under the request's [`TimeoutPolicy`], and read bodies with bounded memory.
//!
//! Two stacks exist: the pooled reqwest client (default) and the hyper raw-write
//! path that reproduces the client's exact header casing for native Anthropic
//! upstreams. Both get the same connect, response-header and body deadlines.

use super::timeout::{phase_timeout, remaining, TimeoutPolicy};
use crate::provider::Provider;
use crate::proxy::content_encoding::{decompress_body, get_content_encoding};
use crate::proxy::hyper_client::ProxyResponse;
use crate::proxy::ProxyError;
use bytes::{Bytes, BytesMut};
use futures::StreamExt;
use std::time::Duration;
use tokio::time::Instant;

/// Largest non-streaming success body buffered inside the retry loop (PROXY-45).
pub const MAX_BUFFERED_RESPONSE_BYTES: usize = 64 * 1024 * 1024;
/// Upstream error bodies are only needed for diagnostics and rectifier matching.
pub const MAX_ERROR_BODY_BYTES: usize = 1024 * 1024;

/// A fully prepared upstream request.
#[derive(Debug)]
pub struct UpstreamRequest {
    pub url: String,
    pub method: http::Method,
    pub headers: http::HeaderMap,
    pub extensions: http::Extensions,
    pub body: Vec<u8>,
    /// The client asked for a stream (governs which deadlines apply).
    pub streaming: bool,
    /// Native Anthropic upstream: keep the client's exact header casing.
    pub preserve_exact_header_case: bool,
}

/// Which HTTP stack carries an attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportKind {
    /// Pooled reqwest client (OpenAI-style backends, SOCKS5 upstream proxies).
    PooledReqwest,
    /// Raw HTTP/1.1 write with exact header casing, CONNECT tunnel through HTTP proxies.
    HyperRaw,
}

pub fn select_transport(
    preserve_exact_header_case: bool,
    upstream_proxy_url: Option<&str>,
) -> TransportKind {
    // SOCKS5 proxies cannot carry the raw CONNECT tunnel.
    let is_socks_proxy = upstream_proxy_url.is_some_and(|u| u.starts_with("socks5"));
    if is_socks_proxy || !preserve_exact_header_case {
        TransportKind::PooledReqwest
    } else {
        TransportKind::HyperRaw
    }
}

/// Only native Anthropic upstreams fingerprint header casing; everything else
/// goes through the pooled client and avoids a TCP+TLS handshake per request.
pub(crate) fn should_preserve_exact_header_case(
    adapter_name: &str,
    provider: &Provider,
    resolved_claude_api_format: Option<&str>,
    is_copilot: bool,
) -> bool {
    if matches!(adapter_name, "Codex" | "Gemini") {
        return false;
    }

    if is_copilot || provider.is_codex_oauth() {
        return false;
    }

    matches!(resolved_claude_api_format, None | Some("anthropic"))
}

pub(crate) fn map_reqwest_send_error(error: reqwest::Error) -> ProxyError {
    if error.is_timeout() {
        ProxyError::Timeout(format!("request timed out: {error}"))
    } else if error.is_connect() {
        ProxyError::ForwardFailed(format!("connect failed: {error}"))
    } else {
        ProxyError::ForwardFailed(error.to_string())
    }
}

/// Send `request` and wait for the response headers.
///
/// Returns the response and the instant the attempt started, from which the
/// first-byte / total deadlines are measured.
pub async fn send(
    request: UpstreamRequest,
    policy: &TimeoutPolicy,
    upstream_proxy_url: Option<&str>,
) -> Result<(ProxyResponse, Instant), ProxyError> {
    let started = Instant::now();
    let header_timeout = policy.response_headers(request.streaming);
    let kind = select_transport(request.preserve_exact_header_case, upstream_proxy_url);
    let response = match kind {
        TransportKind::PooledReqwest => {
            log::debug!(
                "[Forwarder] Using pooled reqwest client (preserve_exact_header_case={}, proxy={})",
                request.preserve_exact_header_case,
                upstream_proxy_url.is_some()
            );
            let client = crate::proxy::http_client::get();
            let mut builder = client
                .request(request.method, &request.url)
                .timeout(policy.reqwest_request_timeout(request.streaming));
            for (key, value) in &request.headers {
                builder = builder.header(key, value);
            }
            let send = builder.body(request.body).send();
            let response = tokio::time::timeout(header_timeout, send)
                .await
                .map_err(|_| phase_timeout("upstream response headers", header_timeout))?
                .map_err(map_reqwest_send_error)?;
            ProxyResponse::Reqwest(response)
        }
        TransportKind::HyperRaw => {
            let uri: http::Uri = request.url.parse().map_err(|e| {
                ProxyError::ForwardFailed(format!("Invalid URL '{}': {e}", request.url))
            })?;
            // The hyper client applies its own connect timeout (CONNECT_TIMEOUT,
            // bounded by this deadline) and fails fast on unreachable hosts.
            crate::proxy::hyper_client::send_request(
                uri,
                request.method,
                request.headers,
                request.extensions,
                request.body,
                header_timeout,
                upstream_proxy_url,
            )
            .await?
        }
    };
    Ok((response, started))
}

/// Collect a body, failing when it exceeds `limit` bytes or `deadline` passes.
pub async fn collect_body(
    response: ProxyResponse,
    limit: usize,
    deadline: Option<(Instant, Duration)>,
) -> Result<Bytes, ProxyError> {
    if let ProxyResponse::Buffered { body, .. } = response {
        return Ok(body);
    }
    let mut stream = Box::pin(response.bytes_stream());
    let mut body = BytesMut::new();
    loop {
        let next = match deadline {
            Some((at, limit_dur)) => {
                let left = remaining(at, "upstream response body", limit_dur)?;
                tokio::time::timeout(left, stream.next())
                    .await
                    .map_err(|_| phase_timeout("upstream response body", limit_dur))?
            }
            None => stream.next().await,
        };
        let Some(chunk) = next else {
            return Ok(body.freeze());
        };
        let chunk = chunk
            .map_err(|e| ProxyError::ForwardFailed(format!("Failed to read response body: {e}")))?;
        if body.len() + chunk.len() > limit {
            return Err(ProxyError::ForwardFailed(format!(
                "upstream response body exceeds the {limit}-byte limit"
            )));
        }
        body.extend_from_slice(&chunk);
    }
}

/// With failover, success cannot be judged from the response headers alone:
///
/// - non-streaming: buffer the whole body (bounded by the total deadline), so a
///   body that never arrives or breaks off still fails over;
/// - streaming: wait for the first chunk (bounded by the first-byte deadline), so a
///   200 that never emits data is not scored as success.
///
/// Phases without a configured limit pass the response through untouched.
pub async fn prepare_success_response(
    response: ProxyResponse,
    streaming: bool,
    policy: &TimeoutPolicy,
    started: Instant,
) -> Result<ProxyResponse, ProxyError> {
    if streaming {
        let Some(first_byte) = policy.first_byte else {
            return Ok(response);
        };
        return prime_first_chunk(response, started + first_byte, first_byte).await;
    }

    let Some(total) = policy.total else {
        return Ok(response);
    };
    let status = response.status();
    let headers = response.headers().clone();
    let body = collect_body(
        response,
        MAX_BUFFERED_RESPONSE_BYTES,
        Some((started + total, total)),
    )
    .await?;
    Ok(ProxyResponse::buffered(status, headers, body))
}

async fn prime_first_chunk(
    response: ProxyResponse,
    deadline: Instant,
    limit: Duration,
) -> Result<ProxyResponse, ProxyError> {
    let status = response.status();
    let headers = response.headers().clone();
    let mut stream = Box::pin(response.bytes_stream());
    let left = remaining(deadline, "stream first byte", limit)?;
    let first = tokio::time::timeout(left, stream.next())
        .await
        .map_err(|_| phase_timeout("stream first byte", limit))?;
    let Some(first) = first else {
        return Err(ProxyError::ForwardFailed(
            "stream ended before the first chunk".to_string(),
        ));
    };
    let first = first.map_err(|e| {
        ProxyError::ForwardFailed(format!("Failed to read first stream chunk: {e}"))
    })?;
    let replay = futures::stream::once(async move { Ok(first) }).chain(stream);
    Ok(ProxyResponse::streamed(status, headers, replay))
}

/// Read a (possibly compressed) body and decode it; undecodable bodies fall back
/// to the raw bytes so diagnostics are kept.
pub async fn read_decoded(
    response: ProxyResponse,
    limit: usize,
    deadline: Option<(Instant, Duration)>,
) -> Result<(http::StatusCode, http::HeaderMap, Bytes, Vec<u8>), ProxyError> {
    let status = response.status();
    let headers = response.headers().clone();
    let encoding = get_content_encoding(&headers);
    let raw = collect_body(response, limit, deadline).await?;
    let decoded = match encoding {
        Some(encoding) => match decompress_body(&encoding, &raw) {
            Ok(Some(decompressed)) => decompressed,
            _ => raw.to_vec(),
        },
        None => raw.to_vec(),
    };
    Ok((status, headers, raw, decoded))
}

/// Turn a non-2xx response into `UpstreamError`, keeping a bounded, decoded body.
///
/// Error bodies may be compressed (reqwest has no automatic decompression), so
/// they are decoded before the UTF-8 check that would otherwise hide rate-limit
/// or auth details.
pub async fn upstream_error(response: ProxyResponse, read_timeout: Duration) -> ProxyError {
    let status = response.status().as_u16();
    let deadline = Some((Instant::now() + read_timeout, read_timeout));
    let body = match read_decoded(response, usize::MAX, deadline).await {
        Ok((_, _, _, decoded)) => {
            let mut decoded = decoded;
            if decoded.len() > MAX_ERROR_BODY_BYTES {
                decoded.truncate(MAX_ERROR_BODY_BYTES);
            }
            match String::from_utf8(decoded) {
                Ok(text) => Some(text),
                Err(e) => {
                    // Truncation may split a code point; keep the valid prefix.
                    let valid = e.utf8_error().valid_up_to();
                    let mut bytes = e.into_bytes();
                    bytes.truncate(valid);
                    String::from_utf8(bytes).ok().filter(|s| !s.is_empty())
                }
            }
        }
        Err(e) => {
            log::debug!("[Forwarder] failed to read upstream error body: {e}");
            None
        }
    };
    ProxyError::UpstreamError { status, body }
}

#[cfg(test)]
mod tests {
    use super::*;
    use http::{HeaderMap, StatusCode};

    fn streamed(chunks: Vec<Result<Bytes, std::io::Error>>, delay: Duration) -> ProxyResponse {
        let stream = futures::stream::iter(chunks).then(move |c| async move {
            tokio::time::sleep(delay).await;
            c
        });
        ProxyResponse::streamed(StatusCode::OK, HeaderMap::new(), stream)
    }

    #[test]
    fn transport_selection() {
        assert_eq!(select_transport(true, None), TransportKind::HyperRaw);
        assert_eq!(
            select_transport(true, Some("http://127.0.0.1:7890")),
            TransportKind::HyperRaw
        );
        assert_eq!(
            select_transport(true, Some("socks5://127.0.0.1:1080")),
            TransportKind::PooledReqwest
        );
        assert_eq!(select_transport(false, None), TransportKind::PooledReqwest);
    }

    #[tokio::test]
    async fn non_streaming_body_shares_the_total_deadline() {
        // Headers "took" 900ms of a 1s budget: a body arriving 300ms later fails.
        let policy = TimeoutPolicy::from_secs(1, 0, 0);
        let started = Instant::now() - Duration::from_millis(900);
        let response = streamed(
            vec![Ok(Bytes::from_static(b"{}"))],
            Duration::from_millis(300),
        );
        let err = prepare_success_response(response, false, &policy, started)
            .await
            .err()
            .expect("must time out");
        assert!(matches!(err, ProxyError::Timeout(_)), "{err}");
    }

    #[tokio::test]
    async fn non_streaming_without_total_is_not_buffered() {
        let policy = TimeoutPolicy::default();
        let response = streamed(vec![Ok(Bytes::from_static(b"{}"))], Duration::ZERO);
        let prepared = prepare_success_response(response, false, &policy, Instant::now())
            .await
            .unwrap();
        assert!(matches!(prepared, ProxyResponse::Streamed { .. }));
    }

    #[tokio::test]
    async fn streaming_first_chunk_is_primed_and_replayed() {
        let policy = TimeoutPolicy::from_secs(0, 1, 0);
        let response = streamed(
            vec![Ok(Bytes::from_static(b"a")), Ok(Bytes::from_static(b"b"))],
            Duration::ZERO,
        );
        let prepared = prepare_success_response(response, true, &policy, Instant::now())
            .await
            .unwrap();
        assert_eq!(prepared.bytes().await.unwrap(), Bytes::from_static(b"ab"));
    }

    #[tokio::test]
    async fn streaming_first_chunk_timeout_is_a_timeout_error() {
        let policy = TimeoutPolicy::from_secs(0, 1, 0);
        let response = ProxyResponse::streamed(
            StatusCode::OK,
            HeaderMap::new(),
            futures::stream::pending::<Result<Bytes, std::io::Error>>(),
        );
        let err = prepare_success_response(response, true, &policy, Instant::now())
            .await
            .err()
            .unwrap();
        assert!(err.to_string().contains("stream first byte timed out"));
    }

    #[tokio::test]
    async fn oversized_bodies_are_rejected() {
        let response = streamed(
            vec![
                Ok(Bytes::from(vec![b'x'; 10])),
                Ok(Bytes::from(vec![b'y'; 10])),
            ],
            Duration::ZERO,
        );
        let err = collect_body(response, 15, None).await.unwrap_err();
        assert!(matches!(err, ProxyError::ForwardFailed(_)));
    }

    #[tokio::test]
    async fn error_bodies_are_truncated_on_a_char_boundary() {
        let mut big = "错".repeat(MAX_ERROR_BODY_BYTES / 3 + 10);
        big.push('!');
        let response = ProxyResponse::buffered(
            StatusCode::TOO_MANY_REQUESTS,
            HeaderMap::new(),
            Bytes::from(big),
        );
        match upstream_error(response, Duration::from_secs(1)).await {
            ProxyError::UpstreamError { status, body } => {
                assert_eq!(status, 429);
                let body = body.unwrap();
                assert!(body.len() <= MAX_ERROR_BODY_BYTES);
                assert!(body.starts_with('错'));
            }
            other => panic!("unexpected {other:?}"),
        }
    }
}
