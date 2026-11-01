//! Response validation stage: semantic checks that run inside the retry loop, so a
//! 2xx response that is really an error can still fail over to the next provider.
//!
//! - buffered/JSON 2xx bodies: Anthropic and Responses error envelopes;
//! - streams: the start of the stream is primed until it is known to be productive
//!   (or a terminal failure), per upstream format (PROXY-41). The primed bytes are
//!   replayed unchanged.

use super::transport::{read_decoded, MAX_BUFFERED_RESPONSE_BYTES};
use crate::proxy::hyper_client::ProxyResponse;
use crate::proxy::ProxyError;
use bytes::Bytes;
use futures::StreamExt;
use serde_json::Value;
use std::time::Duration;

/// Bytes primed at most before a stream is committed without a verdict.
pub const MAX_PRIME_BYTES: usize = 256 * 1024;

/// Upstream stream protocol, for stream-start validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamFormat {
    Anthropic,
    OpenAiChat,
    Responses,
    Gemini,
}

/// Validate a buffered (or explicitly JSON) 2xx body for an error envelope.
pub async fn validate_buffered_success(
    response: ProxyResponse,
    format: StreamFormat,
) -> Result<ProxyResponse, ProxyError> {
    let (status, headers, raw, decoded) =
        read_decoded(response, MAX_BUFFERED_RESPONSE_BYTES, None).await?;
    let failure = match format {
        StreamFormat::Anthropic => anthropic_error_envelope_message(&decoded)
            .map(|m| format!("Anthropic upstream returned a 2xx error envelope: {m}")),
        StreamFormat::Responses => responses_error_envelope_message(&decoded)
            .map(|m| format!("Responses upstream returned a 2xx failure: {m}")),
        StreamFormat::OpenAiChat | StreamFormat::Gemini => None,
    };
    if let Some(message) = failure {
        return Err(ProxyError::TransformError(message));
    }
    Ok(ProxyResponse::buffered(status, headers, raw))
}

/// Prime a streaming 2xx response until its first productive event.
///
/// Each wait for the next chunk is bounded by `chunk_timeout` (the first-byte
/// limit). The stream is committed (replayed byte-identically) once a productive
/// event arrives, when it is not SSE at all, or after [`MAX_PRIME_BYTES`].
pub async fn validate_stream_start(
    response: ProxyResponse,
    format: StreamFormat,
    chunk_timeout: Option<Duration>,
) -> Result<ProxyResponse, ProxyError> {
    let status = response.status();
    let headers = response.headers().clone();
    let mut stream = Box::pin(response.bytes_stream());
    let mut replay_chunks: Vec<Bytes> = Vec::new();
    let mut parse_buffer = String::new();
    let mut utf8_remainder = Vec::new();
    let mut primed = 0usize;

    macro_rules! commit {
        () => {{
            let replay = futures::stream::iter(replay_chunks.into_iter().map(Ok)).chain(stream);
            return Ok(ProxyResponse::streamed(status, headers, replay));
        }};
    }

    loop {
        let next = match chunk_timeout {
            None => stream.next().await,
            Some(limit) => tokio::time::timeout(limit, stream.next())
                .await
                .map_err(|_| {
                    ProxyError::Timeout(format!(
                        "stream produced no semantic output within {}s",
                        limit.as_secs()
                    ))
                })?,
        };

        let Some(chunk) = next else {
            if let Some(outcome) = inspect_json_document(format, &parse_buffer) {
                outcome?;
                let replay = futures::stream::iter(replay_chunks.into_iter().map(Ok));
                return Ok(ProxyResponse::streamed(status, headers, replay));
            }
            let rest = parse_buffer.trim();
            if !rest.is_empty() {
                if let Some(outcome) = inspect_start_event(format, rest) {
                    outcome?;
                    let replay = futures::stream::iter(replay_chunks.into_iter().map(Ok));
                    return Ok(ProxyResponse::streamed(status, headers, replay));
                }
            }
            if format != StreamFormat::Responses && primed > 0 {
                // Lifecycle-only stream (e.g. just pings): nothing to fail over on.
                let replay = futures::stream::iter(replay_chunks.into_iter().map(Ok));
                return Ok(ProxyResponse::streamed(status, headers, replay));
            }
            return Err(ProxyError::ForwardFailed(
                "stream ended before producing output or a terminal event".to_string(),
            ));
        };
        let chunk = chunk.map_err(|error| {
            ProxyError::ForwardFailed(format!("Failed while validating stream start: {error}"))
        })?;
        crate::proxy::sse::append_utf8_safe(&mut parse_buffer, &mut utf8_remainder, &chunk);
        primed += chunk.len();
        replay_chunks.push(chunk);

        // Gateways that ignore `stream:true` may return one complete JSON document
        // without a JSON content type; recognize that before looking for SSE
        // delimiters (pretty-printed JSON can contain blank lines).
        if let Some(outcome) = inspect_json_document(format, &parse_buffer) {
            outcome?;
            commit!();
        }
        if format != StreamFormat::Responses && !looks_like_sse(&parse_buffer) {
            // Not SSE (e.g. Gemini's JSON-array stream): no event framing to wait for.
            commit!();
        }

        while let Some(block) = crate::proxy::sse::take_sse_block(&mut parse_buffer) {
            if let Some(outcome) = inspect_start_event(format, &block) {
                outcome?;
                commit!();
            }
        }

        if primed >= MAX_PRIME_BYTES {
            log::warn!(
                "[Forwarder] stream-start priming exceeded {MAX_PRIME_BYTES} bytes; committing buffered stream"
            );
            commit!();
        }
    }
}

/// Whether the buffered prefix is (still) plausibly SSE.
fn looks_like_sse(buffer: &str) -> bool {
    let trimmed = buffer.trim_start_matches(['\u{feff}', ' ', '\t', '\r', '\n']);
    if trimmed.is_empty() {
        return true;
    }
    ["event:", "data:", ":", "id:", "retry:"]
        .iter()
        .any(|prefix| trimmed.starts_with(prefix) || prefix.starts_with(trimmed))
}

fn inspect_json_document(format: StreamFormat, buffer: &str) -> Option<Result<(), ProxyError>> {
    let trimmed = buffer.trim();
    if !matches!(trimmed.as_bytes().first(), Some(b'{') | Some(b'[')) {
        return None;
    }
    let value: Value = serde_json::from_str(trimmed).ok()?;
    let failure = match format {
        StreamFormat::Responses => responses_error_envelope_message(trimmed.as_bytes())
            .map(|m| format!("Responses upstream returned a 2xx failure: {m}")),
        StreamFormat::Anthropic => anthropic_error_envelope_message(trimmed.as_bytes())
            .map(|m| format!("Anthropic upstream returned a 2xx error envelope: {m}")),
        StreamFormat::OpenAiChat | StreamFormat::Gemini => {
            let first = match &value {
                Value::Array(items) => items.first().cloned().unwrap_or(Value::Null),
                other => other.clone(),
            };
            reportable_error(&first).map(|m| format!("upstream returned a 2xx error: {m}"))
        }
    };
    Some(match failure {
        Some(message) => Err(ProxyError::TransformError(message)),
        None => Ok(()),
    })
}

/// An `{"error": {...}}` object with something to report (null / empty
/// placeholders some gateways emit on every chunk do not count).
fn reportable_error(value: &Value) -> Option<String> {
    let error = value.get("error")?;
    if error.is_null() {
        return None;
    }
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .or_else(|| error.as_str())
        .map(str::trim)
        .filter(|m| !m.is_empty())?;
    let kind = error
        .get("type")
        .and_then(Value::as_str)
        .or_else(|| error.get("status").and_then(Value::as_str))
        .or_else(|| error.get("code").and_then(Value::as_str))
        .unwrap_or("error");
    Some(format!("{kind}: {message}"))
}

/// Split one SSE block into its event name and joined data payload.
fn sse_event(block: &str) -> Option<(Option<String>, String)> {
    let mut named_event = None;
    let mut data_lines = Vec::new();
    for line in block.lines() {
        if let Some(event) = crate::proxy::sse::strip_sse_field(line, "event") {
            named_event = Some(event.trim().to_string());
        } else if let Some(data) = crate::proxy::sse::strip_sse_field(line, "data") {
            data_lines.push(data);
        }
    }
    if data_lines.is_empty() {
        return None;
    }
    Some((named_event, data_lines.join("\n")))
}

/// Inspect one complete SSE block. `None` = lifecycle-only, keep priming;
/// `Some(Ok)` = safe to commit; `Some(Err)` = failure before output (retryable).
fn inspect_start_event(format: StreamFormat, block: &str) -> Option<Result<(), ProxyError>> {
    match format {
        StreamFormat::Responses => inspect_responses_start_event(block),
        StreamFormat::Anthropic => inspect_anthropic_start_event(block),
        StreamFormat::OpenAiChat | StreamFormat::Gemini => {
            let (_, data) = sse_event(block)?;
            let Ok(value) = serde_json::from_str::<Value>(&data) else {
                // `[DONE]` or another non-JSON payload: nothing to judge.
                return Some(Ok(()));
            };
            match reportable_error(&value) {
                Some(message) => Some(Err(ProxyError::TransformError(format!(
                    "upstream stream started with an error: {message}"
                )))),
                None => Some(Ok(())),
            }
        }
    }
}

fn inspect_anthropic_start_event(block: &str) -> Option<Result<(), ProxyError>> {
    let (named_event, data) = sse_event(block)?;
    let value: Value = serde_json::from_str(&data).ok().unwrap_or(Value::Null);
    let event = named_event
        .as_deref()
        .filter(|e| !e.is_empty())
        .or_else(|| value.get("type").and_then(Value::as_str))
        .unwrap_or("");
    match event {
        "error" => {
            let error = value.get("error").unwrap_or(&value);
            let kind = error.get("type").and_then(Value::as_str).unwrap_or("error");
            let message = error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("upstream emitted an error event before output");
            Some(Err(ProxyError::TransformError(format!(
                "Anthropic stream started with {kind}: {message}"
            ))))
        }
        "ping" => None,
        _ => Some(Ok(())),
    }
}

pub(crate) fn anthropic_error_envelope_message(body: &[u8]) -> Option<String> {
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

pub(crate) fn responses_error_envelope_message(body: &[u8]) -> Option<String> {
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
/// omits `application/json`. `None` means either "not JSON" or "not complete yet".
#[cfg(test)]
pub(crate) fn inspect_responses_json_document(buffer: &str) -> Option<Result<(), ProxyError>> {
    inspect_json_document(StreamFormat::Responses, buffer)
}

/// Inspect one complete Responses SSE block while the response is still inside
/// the retry loop. `None` means the event is lifecycle-only and priming should
/// continue; `Some(Ok(()))` means it is safe to commit/replay the stream.
pub(crate) fn inspect_responses_start_event(block: &str) -> Option<Result<(), ProxyError>> {
    let (named_event, data) = sse_event(block)?;
    let value: Value = serde_json::from_str(&data).ok()?;
    let event = named_event
        .as_deref()
        .filter(|event| !event.is_empty())
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
        // expose. Mid-stream failures after this point are reported by the stream
        // health reporter (PROXY-09) but do not switch providers.
        _ => Some(Ok(())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use http::{HeaderMap, StatusCode};

    fn sse(chunks: &[&'static str]) -> ProxyResponse {
        let mut headers = HeaderMap::new();
        headers.insert("content-type", "text/event-stream".parse().unwrap());
        let items: Vec<Result<Bytes, std::io::Error>> = chunks
            .iter()
            .map(|c| Ok(Bytes::from_static(c.as_bytes())))
            .collect();
        ProxyResponse::streamed(StatusCode::OK, headers, futures::stream::iter(items))
    }

    async fn run(format: StreamFormat, chunks: &[&'static str]) -> Result<Bytes, ProxyError> {
        let response =
            validate_stream_start(sse(chunks), format, Some(Duration::from_secs(1))).await?;
        response.bytes().await
    }

    #[tokio::test]
    async fn anthropic_error_event_before_output_is_retryable() {
        let err = run(
            StreamFormat::Anthropic,
            &["event: error\ndata: {\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\",\"message\":\"Overloaded\"}}\n\n"],
        )
        .await
        .unwrap_err();
        assert!(matches!(err, ProxyError::TransformError(_)));
        assert!(err.to_string().contains("overloaded_error"));
    }

    #[tokio::test]
    async fn anthropic_message_start_is_replayed_byte_identically() {
        let chunks = [
            "event: ping\ndata: {\"type\":\"ping\"}\n\n",
            "event: message_start\ndata: {\"type\":\"message_start\"}\n\n",
            "event: content_block_start\ndata: {}\n\n",
        ];
        let body = run(StreamFormat::Anthropic, &chunks).await.unwrap();
        assert_eq!(body, Bytes::from(chunks.concat()));
    }

    #[tokio::test]
    async fn chat_error_chunk_is_retryable_and_choices_commit() {
        let err = run(
            StreamFormat::OpenAiChat,
            &["data: {\"error\":{\"message\":\"rate limited\",\"type\":\"rate_limit\"}}\n\n"],
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("rate limited"));

        let ok = run(
            StreamFormat::OpenAiChat,
            &["data: {\"error\":null,\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\n"],
        )
        .await;
        assert!(ok.is_ok());
    }

    #[tokio::test]
    async fn gemini_error_chunk_is_retryable_and_json_array_streams_commit() {
        let err = run(
            StreamFormat::Gemini,
            &["data: {\"error\":{\"code\":429,\"message\":\"quota\",\"status\":\"RESOURCE_EXHAUSTED\"}}\n\n"],
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("RESOURCE_EXHAUSTED"));

        // Non-SSE JSON-array stream: committed at once, nothing is held back.
        let body = run(StreamFormat::Gemini, &["[{\"candidates\":[", "]}]"])
            .await
            .unwrap();
        assert_eq!(body, Bytes::from_static(b"[{\"candidates\":[]}]"));
    }

    #[tokio::test]
    async fn responses_lifecycle_events_keep_priming() {
        let err = run(
            StreamFormat::Responses,
            &[
                "event: response.created\ndata: {\"type\":\"response.created\",\"response\":{}}\n\n",
                "event: response.failed\ndata: {\"type\":\"response.failed\",\"response\":{\"status\":\"failed\",\"error\":{\"code\":\"server_error\",\"message\":\"boom\"}}}\n\n",
            ],
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("server_error"));
    }

    #[tokio::test]
    async fn buffered_anthropic_error_envelope_is_detected() {
        let response = ProxyResponse::buffered(
            StatusCode::OK,
            HeaderMap::new(),
            Bytes::from_static(
                b"{\"type\":\"error\",\"error\":{\"type\":\"api_error\",\"message\":\"x\"}}",
            ),
        );
        let err = validate_buffered_success(response, StreamFormat::Anthropic)
            .await
            .err()
            .unwrap();
        assert!(matches!(err, ProxyError::TransformError(_)));
    }

    #[test]
    fn sse_sniffing() {
        assert!(looks_like_sse(""));
        assert!(looks_like_sse("da"));
        assert!(looks_like_sse("\n: keep-alive\n"));
        assert!(!looks_like_sse("[{\"candidates\""));
        assert!(!looks_like_sse("{\"id\":1}"));
    }
}
