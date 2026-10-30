//! 上游响应校验（仍在故障转移循环内）
//!
//! 成功不能只看上游响应头：2xx 的语义失败（Anthropic error envelope、Responses
//! `status: failed`）与流式首包都要在 retry loop 内确认，才能把失败的尝试交给下一家
//! provider；非 2xx 响应在这里读出（解压后的）错误体并转成 [`ProxyError::UpstreamError`]。
//! 其余流式响应经 [`stream_start`](super::stream_start) 闸门确认首个内容前没有错误事件。

use crate::proxy::content_encoding::{decompress_body, get_content_encoding};
use crate::proxy::error::ProxyError;
use crate::proxy::handler_context::DEFAULT_NON_STREAMING_BODY_TIMEOUT;
use crate::proxy::hyper_client::ProxyResponse;
use crate::proxy::protocol::{ClaudeApiFormat, WireFormat};
use crate::proxy::stream::{SseDecoder, SseEvent};
use bytes::Bytes;
use futures::StreamExt;
use serde_json::Value;
use std::time::Duration;

/// 按本次请求的超时配置校验上游响应
#[derive(Debug, Clone, Copy)]
pub(crate) struct ResponseValidator {
    /// 非流式请求超时（0 表示不限）
    pub non_streaming_timeout: Duration,
    /// 流式请求首包超时（0 表示不限）
    pub streaming_first_byte_timeout: Duration,
}

/// 需要在 retry loop 内做的 2xx 语义校验
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SuccessCheck {
    /// 只做缓冲 / 首包预读
    None,
    /// Codex→Anthropic：识别 2xx Anthropic error envelope
    AnthropicEnvelope,
    /// Claude→Responses：识别 2xx Responses 失败对象 / 流式开头的失败事件
    ResponsesStatus,
}

impl SuccessCheck {
    pub(crate) fn for_route(
        codex_responses_to_anthropic: bool,
        claude_api_format: Option<ClaudeApiFormat>,
    ) -> Self {
        if codex_responses_to_anthropic {
            SuccessCheck::AnthropicEnvelope
        } else if claude_api_format == Some(ClaudeApiFormat::OpenAiResponses) {
            SuccessCheck::ResponsesStatus
        } else {
            SuccessCheck::None
        }
    }
}

impl ResponseValidator {
    /// 校验上游响应：2xx 返回可交给 handler 的响应，否则返回可分类的错误
    ///
    /// `upstream` 是本次尝试的上游线协议，流式闸门按它解码 SSE。
    pub(crate) async fn validate(
        &self,
        response: ProxyResponse,
        request_is_streaming: bool,
        check: SuccessCheck,
        upstream: WireFormat,
    ) -> Result<ProxyResponse, ProxyError> {
        let status = response.status();
        if !status.is_success() {
            return Err(self.upstream_error(response).await);
        }

        let mut response = self
            .prepare_success_response_for_failover(response, request_is_streaming)
            .await?;
        // Streaming requests normally return SSE. If a compatible gateway
        // explicitly returns JSON instead, buffer and validate it inside the retry
        // loop as well so a 2xx Anthropic error envelope can still fail over. Do
        // not buffer unknown content types: some gateways omit the SSE header.
        match check {
            SuccessCheck::AnthropicEnvelope if !request_is_streaming || response.is_json() => {
                response = self
                    .validate_codex_anthropic_success_response(response)
                    .await?;
            }
            SuccessCheck::ResponsesStatus => {
                if !request_is_streaming || response.is_json() {
                    // Claude→Responses gateways can also return a semantic failure in an
                    // HTTP 2xx Response object. Validate buffered/JSON bodies inside the
                    // retry loop so an early failure can still select another provider.
                    response = self.validate_responses_success_response(response).await?;
                } else {
                    // Delay committing the downstream stream until the upstream emits
                    // either productive output or a valid non-failure terminal event.
                    // A response.failed/error before output remains failover-safe.
                    response = self.validate_responses_stream_start(response).await?;
                }
            }
            // 其余 SSE：缓冲到首个内容，内容前的错误事件（200 后立即报错）同样可以故障转移
            _ if request_is_streaming && !response.is_json() => {
                response = self.gate_stream_start(response, upstream).await?;
            }
            _ => {}
        }
        Ok(response)
    }

    /// 非 2xx：读出错误体（失败时返回读取错误本身）
    async fn upstream_error(&self, response: ProxyResponse) -> ProxyError {
        let status_code = response.status().as_u16();
        // 错误响应同样可能被上游压缩（content-encoding）。reqwest 未启用任何
        // 自动解压 feature，这里拿到的是原始字节；不解压的话，压缩过的错误体会
        // 在 from_utf8 处变成非 UTF-8 而被丢弃，隐藏掉上游的限流/鉴权等详情。
        let encoding = get_content_encoding(response.headers());
        let raw = match self.read_body_bounded(response).await {
            Ok(raw) => raw,
            Err(error) => return error,
        };
        let body_text = String::from_utf8(decode_body(encoding, &raw)).ok();

        ProxyError::UpstreamError {
            status: status_code,
            body: body_text,
        }
    }

    /// 故障转移开启时，成功不能只看上游响应头。
    ///
    /// - 非流式：先把完整 body 读到内存，读超时/连接中断会回到 retry loop 尝试下一家。
    /// - 流式：至少等首个 chunk 到达，避免上游返回 200 后一直不吐 SSE 时被误记成功。
    pub(crate) async fn prepare_success_response_for_failover(
        &self,
        response: ProxyResponse,
        request_is_streaming: bool,
    ) -> Result<ProxyResponse, ProxyError> {
        if request_is_streaming {
            return self.prime_streaming_response(response).await;
        }

        if self.non_streaming_timeout.is_zero() {
            return Ok(response);
        }

        let status = response.status();
        let headers = response.headers().clone();
        let body_timeout = self.non_streaming_timeout;
        let body = tokio::time::timeout(body_timeout, response.bytes())
            .await
            .map_err(|_| {
                ProxyError::Timeout(format!(
                    "响应体读取超时: {}s（上游发完响应头后 body 未到达）",
                    body_timeout.as_secs()
                ))
            })??;

        Ok(ProxyResponse::buffered(status, headers, body))
    }

    /// 在 retry loop 内整包读取响应体（错误体 / 2xx 语义校验），始终带上限。
    ///
    /// 故障转移关闭时 `non_streaming_timeout` 为 0，raw hyper 路径的 body 不会被
    /// 预先缓冲；这里回退到 [`DEFAULT_NON_STREAMING_BODY_TIMEOUT`]，避免上游发完
    /// 响应头后卡住 body 时请求永久挂起。
    pub(crate) async fn read_body_bounded(
        &self,
        response: ProxyResponse,
    ) -> Result<Bytes, ProxyError> {
        let body_timeout = if self.non_streaming_timeout.is_zero() {
            DEFAULT_NON_STREAMING_BODY_TIMEOUT
        } else {
            self.non_streaming_timeout
        };
        tokio::time::timeout(body_timeout, response.bytes())
            .await
            .map_err(|_| {
                ProxyError::Timeout(format!(
                    "响应体读取超时: {}s（上游发完响应头后 body 未到达）",
                    body_timeout.as_secs()
                ))
            })?
    }

    /// Some Anthropic-compatible gateways return an Anthropic error envelope with
    /// HTTP 2xx. Validate it inside the retry loop so the request can fail over to
    /// the next provider; the response transformer runs too late for that.
    pub(crate) async fn validate_codex_anthropic_success_response(
        &self,
        response: ProxyResponse,
    ) -> Result<ProxyResponse, ProxyError> {
        let status = response.status();
        let headers = response.headers().clone();
        let encoding = get_content_encoding(&headers);
        let raw = self.read_body_bounded(response).await?;
        let decoded = decode_body(encoding, &raw);

        if let Some(message) = codex_anthropic_error_envelope_message(&decoded) {
            return Err(ProxyError::TransformError(format!(
                "Anthropic upstream returned a 2xx error envelope: {message}"
            )));
        }

        Ok(ProxyResponse::buffered(status, headers, raw))
    }

    pub(crate) async fn validate_responses_success_response(
        &self,
        response: ProxyResponse,
    ) -> Result<ProxyResponse, ProxyError> {
        let status = response.status();
        let headers = response.headers().clone();
        let encoding = get_content_encoding(&headers);
        let raw = self.read_body_bounded(response).await?;
        let decoded = decode_body(encoding, &raw);

        if let Some(message) = responses_error_envelope_message(&decoded) {
            return Err(ProxyError::TransformError(format!(
                "Responses upstream returned a 2xx failure: {message}"
            )));
        }

        Ok(ProxyResponse::buffered(status, headers, raw))
    }

    async fn validate_responses_stream_start(
        &self,
        response: ProxyResponse,
    ) -> Result<ProxyResponse, ProxyError> {
        const MAX_PRIME_BYTES: usize = 256 * 1024;

        let status = response.status();
        let headers = response.headers().clone();
        let mut stream = Box::pin(response.bytes_stream());
        let mut replay_chunks: Vec<Bytes> = Vec::new();
        // Some compatible gateways ignore `stream:true` and return a complete
        // Responses JSON document without a JSON content-type. Until the first
        // non-whitespace character rules that out, keep the raw text: pretty-printed
        // JSON may itself contain blank lines and must not be framed as SSE.
        let mut json_probe: Option<String> = Some(String::new());
        let mut utf8_remainder = Vec::new();
        let mut decoder = SseDecoder::new();

        loop {
            let next = if self.streaming_first_byte_timeout.is_zero() {
                stream.next().await
            } else {
                tokio::time::timeout(self.streaming_first_byte_timeout, stream.next())
                    .await
                    .map_err(|_| {
                        ProxyError::Timeout(format!(
                            "Responses stream produced no semantic output within {}s",
                            self.streaming_first_byte_timeout.as_secs()
                        ))
                    })?
            };

            let Some(chunk) = next else {
                let mut events = Vec::new();
                if let Some(text) = json_probe.take() {
                    if let Some(outcome) = inspect_responses_json_document(&text) {
                        outcome?;
                        let replay = futures::stream::iter(replay_chunks.into_iter().map(Ok));
                        return Ok(ProxyResponse::streamed(status, headers, replay));
                    }
                    events = decoder.feed_str(&text);
                    events.extend(decoder.feed(&utf8_remainder));
                }
                events.extend(decoder.finish());
                for event in &events {
                    if let Some(outcome) = inspect_responses_start_event(event) {
                        outcome?;
                        let replay = futures::stream::iter(replay_chunks.into_iter().map(Ok));
                        return Ok(ProxyResponse::streamed(status, headers, replay));
                    }
                }
                return Err(ProxyError::ForwardFailed(
                    "Responses stream ended before producing output or a terminal event"
                        .to_string(),
                ));
            };
            let chunk = chunk.map_err(|error| {
                ProxyError::ForwardFailed(format!(
                    "Failed while validating Responses stream start: {error}"
                ))
            })?;

            let events = match json_probe.as_mut() {
                Some(text) => {
                    crate::proxy::sse::append_utf8_safe(text, &mut utf8_remainder, &chunk);
                    let first = text
                        .trim_start_matches(|ch: char| ch.is_whitespace() || ch == '\u{feff}')
                        .as_bytes()
                        .first()
                        .copied();
                    match first {
                        Some(b'{') | Some(b'[') => {
                            if let Some(outcome) = inspect_responses_json_document(text) {
                                outcome?;
                                replay_chunks.push(chunk);
                                let replay =
                                    futures::stream::iter(replay_chunks.into_iter().map(Ok))
                                        .chain(stream);
                                return Ok(ProxyResponse::streamed(status, headers, replay));
                            }
                            Vec::new()
                        }
                        None => Vec::new(),
                        Some(_) => {
                            let text = json_probe.take().unwrap_or_default();
                            let mut events = decoder.feed_str(&text);
                            events.extend(decoder.feed(&std::mem::take(&mut utf8_remainder)));
                            events
                        }
                    }
                }
                None => decoder.feed(&chunk),
            };
            replay_chunks.push(chunk);

            for event in &events {
                if let Some(outcome) = inspect_responses_start_event(event) {
                    outcome?;
                    let replay =
                        futures::stream::iter(replay_chunks.into_iter().map(Ok)).chain(stream);
                    return Ok(ProxyResponse::streamed(status, headers, replay));
                }
            }

            if replay_chunks.iter().map(Bytes::len).sum::<usize>() >= MAX_PRIME_BYTES {
                log::warn!(
                    "[Claude/Responses] semantic stream priming exceeded {MAX_PRIME_BYTES} bytes; committing buffered stream"
                );
                let replay = futures::stream::iter(replay_chunks.into_iter().map(Ok)).chain(stream);
                return Ok(ProxyResponse::streamed(status, headers, replay));
            }
        }
    }

    async fn prime_streaming_response(
        &self,
        response: ProxyResponse,
    ) -> Result<ProxyResponse, ProxyError> {
        if self.streaming_first_byte_timeout.is_zero() {
            return Ok(response);
        }

        let status = response.status();
        let headers = response.headers().clone();
        let timeout = self.streaming_first_byte_timeout;
        let mut stream = Box::pin(response.bytes_stream());

        let first = tokio::time::timeout(timeout, stream.next())
            .await
            .map_err(|_| {
                ProxyError::Timeout(format!(
                    "流式响应首包超时: {}s（上游已返回响应头但未返回数据）",
                    timeout.as_secs()
                ))
            })?;

        let Some(first) = first else {
            return Err(ProxyError::ForwardFailed(
                "流式响应在首包到达前结束".to_string(),
            ));
        };

        let first =
            first.map_err(|e| ProxyError::ForwardFailed(format!("读取流式响应首包失败: {e}")))?;

        let replay = futures::stream::once(async move { Ok(first) }).chain(stream);
        Ok(ProxyResponse::streamed(status, headers, replay))
    }
}

/// 按 content-encoding 解压；不支持的编码 / 解压失败时退回原始字节，尽量保留可读信息
fn decode_body(encoding: Option<String>, raw: &Bytes) -> Vec<u8> {
    match encoding {
        Some(encoding) => match decompress_body(&encoding, raw) {
            Ok(Some(decompressed)) => decompressed,
            _ => raw.to_vec(),
        },
        None => raw.to_vec(),
    }
}

fn codex_anthropic_error_envelope_message(body: &[u8]) -> Option<String> {
    let value: Value = serde_json::from_slice(body).ok()?;
    if value.get("type").and_then(Value::as_str) != Some("error") && value.get("error").is_none() {
        return None;
    }
    let error = value.get("error").unwrap_or(&value);
    let error_type = error.get("type").and_then(Value::as_str).unwrap_or("error");
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .or_else(|| error.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| error.to_string());
    Some(format!("{error_type}: {message}"))
}

fn responses_error_envelope_message(body: &[u8]) -> Option<String> {
    let value: Value = serde_json::from_slice(body).ok()?;
    let status = value.get("status").and_then(Value::as_str);
    let has_error = value.get("error").is_some_and(|error| !error.is_null());
    if !matches!(status, Some("failed" | "cancelled")) && !has_error {
        return None;
    }

    let error = value.get("error").unwrap_or(&value);
    let error_type = error
        .get("type")
        .and_then(Value::as_str)
        .or_else(|| error.get("code").and_then(Value::as_str))
        .unwrap_or_else(|| status.unwrap_or("error"));
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .or_else(|| error.as_str())
        .filter(|message| !message.trim().is_empty())
        .unwrap_or(match status {
            Some("cancelled") => "response generation was cancelled",
            _ => "response generation failed",
        });
    Some(format!("{error_type}: {message}"))
}

/// A streaming request may receive a whole JSON document even when the gateway
/// omits `application/json`. `None` means either "not JSON" or "not complete yet";
/// a parsed document is safe to commit unless it is a semantic failure envelope.
fn inspect_responses_json_document(buffer: &str) -> Option<Result<(), ProxyError>> {
    let trimmed = buffer.trim();
    if !matches!(trimmed.as_bytes().first(), Some(b'{') | Some(b'[')) {
        return None;
    }
    let _: Value = serde_json::from_str(trimmed).ok()?;
    if let Some(message) = responses_error_envelope_message(trimmed.as_bytes()) {
        return Some(Err(ProxyError::TransformError(format!(
            "Responses upstream returned a 2xx failure: {message}"
        ))));
    }
    Some(Ok(()))
}

/// Inspect one complete Responses SSE event while the response is still inside
/// the retry loop. `None` means the event is lifecycle-only and priming should
/// continue; `Some(Ok(()))` means it is safe to commit/replay the stream.
fn inspect_responses_start_event(sse_event: &SseEvent) -> Option<Result<(), ProxyError>> {
    let value: Value = match serde_json::from_str(&sse_event.data) {
        Ok(value) => value,
        Err(_) => return None,
    };
    let event = sse_event
        .event
        .as_deref()
        .or_else(|| value.get("type").and_then(Value::as_str))
        .unwrap_or("");

    let response = value.get("response").unwrap_or(&value);
    if matches!(
        response.get("status").and_then(Value::as_str),
        Some("failed" | "cancelled")
    ) || response.get("error").is_some_and(|error| !error.is_null())
    {
        let error = response.get("error").unwrap_or(response);
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .or_else(|| error.as_str())
            .unwrap_or("Responses upstream failed before output");
        let error_type = error
            .get("type")
            .and_then(Value::as_str)
            .or_else(|| error.get("code").and_then(Value::as_str))
            .or_else(|| response.get("status").and_then(Value::as_str))
            .unwrap_or("upstream_error");
        return Some(Err(ProxyError::TransformError(format!(
            "Responses upstream {error_type}: {message}"
        ))));
    }

    match event {
        "response.failed" | "error" => {
            let error = response.get("error").unwrap_or(response);
            let message = error
                .get("message")
                .and_then(Value::as_str)
                .or_else(|| error.as_str())
                .unwrap_or("Responses upstream emitted an error before output");
            let error_type = error
                .get("type")
                .and_then(Value::as_str)
                .or_else(|| error.get("code").and_then(Value::as_str))
                .unwrap_or("upstream_error");
            Some(Err(ProxyError::TransformError(format!(
                "Responses upstream {error_type}: {message}"
            ))))
        }
        "response.created" | "response.in_progress" | "response.queued" => None,
        "" => None,
        // Productive output, incomplete, and completed terminals are all safe to
        // expose. Mid-stream failures after this point are surfaced by the converter
        // but intentionally do not switch providers.
        _ => Some(Ok(())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderMap;
    use http::StatusCode;

    fn validator(
        non_streaming_timeout: Duration,
        streaming_first_byte_timeout: Duration,
    ) -> ResponseValidator {
        ResponseValidator {
            non_streaming_timeout,
            streaming_first_byte_timeout,
        }
    }

    #[tokio::test]
    async fn non_streaming_success_is_buffered_before_marking_provider_successful() {
        let validator = validator(Duration::from_secs(1), Duration::from_secs(1));
        let response = ProxyResponse::streamed(
            StatusCode::OK,
            HeaderMap::new(),
            futures::stream::once(async {
                tokio::time::sleep(Duration::from_millis(10)).await;
                Ok::<Bytes, std::io::Error>(Bytes::from_static(b"{\"ok\":true}"))
            }),
        );

        let prepared = validator
            .prepare_success_response_for_failover(response, false)
            .await
            .expect("response should be buffered");

        assert_eq!(
            prepared.bytes().await.unwrap(),
            Bytes::from_static(b"{\"ok\":true}")
        );
    }

    /// 故障转移关闭（non_streaming_timeout = 0）时，retry loop 内的整包读取
    /// （2xx 语义校验、错误体）也必须有上限，不能在上游卡住 body 时永久挂起。
    #[tokio::test]
    async fn bounded_body_reads_time_out_on_stalled_body_with_failover_off() {
        let validator = validator(Duration::ZERO, Duration::ZERO);
        let stalled = || {
            ProxyResponse::streamed(
                StatusCode::OK,
                HeaderMap::new(),
                futures::stream::once(async {
                    Ok::<Bytes, std::io::Error>(Bytes::from_static(b"{"))
                })
                .chain(futures::stream::pending()),
            )
        };

        tokio::time::pause();
        let started = tokio::time::Instant::now();
        let guard = Duration::from_secs(3600);

        let err = tokio::time::timeout(guard, validator.read_body_bounded(stalled()))
            .await
            .expect("error-body read must not hang when failover is off")
            .expect_err("stalled body should time out");
        assert!(matches!(err, ProxyError::Timeout(_)), "got {err:?}");

        let err = tokio::time::timeout(
            guard,
            validator.validate_responses_success_response(stalled()),
        )
        .await
        .expect("responses validation must not hang when failover is off")
        .err()
        .expect("stalled body should time out");
        assert!(matches!(err, ProxyError::Timeout(_)), "got {err:?}");

        let err = tokio::time::timeout(
            guard,
            validator.validate_codex_anthropic_success_response(stalled()),
        )
        .await
        .expect("anthropic validation must not hang when failover is off")
        .err()
        .expect("stalled body should time out");
        assert!(matches!(err, ProxyError::Timeout(_)), "got {err:?}");

        assert!(started.elapsed() >= DEFAULT_NON_STREAMING_BODY_TIMEOUT * 3);
        assert!(started.elapsed() < guard);
    }

    #[tokio::test]
    async fn non_streaming_body_read_error_is_retryable_before_success_record() {
        let validator = validator(Duration::from_secs(1), Duration::from_secs(1));
        let response = ProxyResponse::streamed(
            StatusCode::OK,
            HeaderMap::new(),
            futures::stream::once(async {
                Err::<Bytes, std::io::Error>(std::io::Error::other("body boom"))
            }),
        );

        let err = match validator
            .prepare_success_response_for_failover(response, false)
            .await
        {
            Ok(_) => panic!("body read errors should fail the attempt"),
            Err(err) => err,
        };

        assert!(matches!(err, ProxyError::ForwardFailed(_)));
    }

    #[tokio::test]
    async fn streaming_success_primes_first_chunk_and_replays_it() {
        let validator = validator(Duration::from_secs(1), Duration::from_secs(1));
        let response = ProxyResponse::streamed(
            StatusCode::OK,
            HeaderMap::new(),
            futures::stream::iter(vec![
                Ok::<Bytes, std::io::Error>(Bytes::from_static(b"first")),
                Ok::<Bytes, std::io::Error>(Bytes::from_static(b"second")),
            ]),
        );

        let prepared = validator
            .prepare_success_response_for_failover(response, true)
            .await
            .expect("stream should be primed");

        assert_eq!(
            prepared.bytes().await.unwrap(),
            Bytes::from_static(b"firstsecond")
        );
    }

    #[tokio::test]
    async fn streaming_first_chunk_error_is_retryable_before_success_record() {
        let validator = validator(Duration::from_secs(1), Duration::from_secs(1));
        let response = ProxyResponse::streamed(
            StatusCode::OK,
            HeaderMap::new(),
            futures::stream::once(async {
                Err::<Bytes, std::io::Error>(std::io::Error::other("first chunk boom"))
            }),
        );

        let err = match validator
            .prepare_success_response_for_failover(response, true)
            .await
        {
            Ok(_) => panic!("first chunk errors should fail the attempt"),
            Err(err) => err,
        };

        assert!(matches!(err, ProxyError::ForwardFailed(_)));
    }

    #[test]
    fn codex_anthropic_2xx_error_envelope_is_detected_for_failover() {
        let body = br#"{"type":"error","error":{"type":"overloaded_error","message":"busy"}}"#;
        assert_eq!(
            codex_anthropic_error_envelope_message(body).as_deref(),
            Some("overloaded_error: busy")
        );
        assert!(
            codex_anthropic_error_envelope_message(br#"{"type":"message","content":[]}"#).is_none()
        );
    }

    #[test]
    fn responses_2xx_failure_is_detected_for_failover() {
        assert_eq!(
            responses_error_envelope_message(
                br#"{"status":"failed","error":{"type":"server_error","message":"busy"},"output":[]}"#
            )
            .as_deref(),
            Some("server_error: busy")
        );
        assert_eq!(
            responses_error_envelope_message(br#"{"status":"cancelled","output":[]}"#).as_deref(),
            Some("cancelled: response generation was cancelled")
        );
        assert!(responses_error_envelope_message(
            br#"{"status":"incomplete","incomplete_details":{"reason":"max_output_tokens"},"output":[]}"#
        )
        .is_none());
        assert!(responses_error_envelope_message(
            br#"{"status":"completed","error":null,"output":[]}"#
        )
        .is_none());
    }

    fn decode_event(block: &str) -> SseEvent {
        let mut decoder = SseDecoder::new();
        let mut events = decoder.feed_str(block);
        events.extend(decoder.finish());
        assert_eq!(events.len(), 1, "{block:?}");
        events.remove(0)
    }

    fn chunked_response(chunks: Vec<&'static str>) -> ProxyResponse {
        ProxyResponse::streamed(
            StatusCode::OK,
            HeaderMap::new(),
            futures::stream::iter(
                chunks
                    .into_iter()
                    .map(|chunk| Ok::<Bytes, std::io::Error>(Bytes::from_static(chunk.as_bytes())))
                    .collect::<Vec<_>>(),
            ),
        )
    }

    #[tokio::test]
    async fn responses_stream_start_keeps_pretty_json_split_at_blank_line() {
        // The first chunk ends inside a pretty-printed JSON document right after a
        // blank line; it must not be framed as an SSE block (which would drop the
        // opening brace and hide the document from the JSON probe).
        let validator = validator(Duration::from_secs(1), Duration::from_secs(1));
        let prepared = validator
            .validate_responses_stream_start(chunked_response(vec![
                "{\n\n  \"status\": \"completed\",",
                "\n  \"output\": []\n}",
            ]))
            .await
            .expect("whole JSON document should be accepted");
        assert_eq!(
            prepared.bytes().await.unwrap(),
            Bytes::from_static(b"{\n\n  \"status\": \"completed\",\n  \"output\": []\n}")
        );

        let err = validator
            .validate_responses_stream_start(chunked_response(vec![
                "{\n\n  \"status\": \"failed\",",
                "\n  \"error\": {\"message\": \"backend unavailable\"}\n}",
            ]))
            .await
            .err()
            .expect("failed JSON document should be retryable");
        assert!(
            matches!(&err, ProxyError::TransformError(message) if message.contains("backend unavailable")),
            "got {err:?}"
        );
    }

    #[tokio::test]
    async fn responses_stream_start_decodes_cr_framed_events_across_chunks() {
        let validator = validator(Duration::from_secs(1), Duration::from_secs(1));
        let err = validator
            .validate_responses_stream_start(chunked_response(vec![
                "event: response.created\rdata: {\"type\":\"response.created\"}\r",
                "\revent: response.failed\r",
                "data: {\"type\":\"response.failed\",\"response\":{\"error\":{\"message\":\"boom\"}}}\r",
            ]))
            .await
            .err()
            .expect("response.failed before output should be retryable");
        assert!(
            matches!(&err, ProxyError::TransformError(message) if message.contains("boom")),
            "got {err:?}"
        );
    }

    #[test]
    fn responses_stream_start_semantic_failure_is_retryable() {
        let created = concat!(
            "event: response.created\n",
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_1\"}}"
        );
        assert!(inspect_responses_start_event(&decode_event(created)).is_none());

        let failed = concat!(
            "event: response.failed\n",
            "data: {\"type\":\"response.failed\",\"response\":{\"error\":{\"type\":\"server_error\",\"message\":\"boom\"}}}"
        );
        assert!(matches!(
            inspect_responses_start_event(&decode_event(failed)),
            Some(Err(ProxyError::TransformError(message))) if message.contains("boom")
        ));

        let delta = concat!(
            "event: response.output_text.delta\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"hi\"}"
        );
        assert!(matches!(
            inspect_responses_start_event(&decode_event(delta)),
            Some(Ok(()))
        ));
    }

    #[test]
    fn responses_stream_start_accepts_unlabelled_whole_json() {
        assert!(matches!(
            inspect_responses_json_document(
                r#"{
                    "status": "completed",

                    "output": []
                }"#
            ),
            Some(Ok(()))
        ));
        assert!(inspect_responses_json_document(r#"{"status":"completed""#).is_none());

        let failed = inspect_responses_json_document(
            r#"{"status":"failed","error":{"message":"backend unavailable"}}"#,
        );
        assert!(
            matches!(failed, Some(Err(ProxyError::TransformError(message))) if message.contains("backend unavailable"))
        );
    }
}
