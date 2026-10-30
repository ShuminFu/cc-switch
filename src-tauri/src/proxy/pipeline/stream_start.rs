//! 流式首个内容前的故障转移闸门
//!
//! 有些网关先返回 200 与 SSE 响应头，紧接着发出错误事件（Anthropic `error`、Chat
//! in-band `error`、Responses `response.failed`）而没有任何内容。只看首包时这类尝试会被
//! 记为成功，客户端随后收到一个只有错误的流，故障转移不再有机会接手。
//!
//! 闸门在 retry loop 内按上游线协议把 SSE 解码为 IR 事件，并缓冲原始字节，直到：
//! - 出现第一个内容事件（非空文本 / reasoning 增量，或工具调用）→ 提交：缓冲字节原样回放，
//!   后续字节直通；此后的错误随流透传给客户端，不再切换 provider；
//! - 出现 [`StreamEvent::MessageStop`]（没有内容的正常结束）→ 提交；
//! - 出现 [`StreamEvent::Error`]（含上游流在内容前结束）→ 丢弃缓冲，返回可重试错误，
//!   由 [`AttemptRecorder`](super::failover::AttemptRecorder) 记一次 provider 失败并换下一家。
//!
//! reasoning 增量同样视为内容：思考模型的 reasoning 可能持续很久，缓冲它会让客户端
//! 长时间看不到任何输出。上游忽略 `stream: true` 直接返回 JSON、解码出错或缓冲超过上限时
//! 直接提交，交给响应转换按原有方式处理（闸门只决定是否切换，不改写字节）。

use super::validate::ResponseValidator;
use crate::proxy::codec::anthropic::AnthropicEventDecoder;
use crate::proxy::codec::gemini::GeminiEventDecoder;
use crate::proxy::codec::openai_chat::OpenAiChatEventDecoder;
use crate::proxy::codec::responses::ResponsesEventDecoder;
use crate::proxy::codec::EventDecoder;
use crate::proxy::content_encoding::get_content_encoding;
use crate::proxy::error::ProxyError;
use crate::proxy::hyper_client::ProxyResponse;
use crate::proxy::protocol::ir::{Part, PartDelta, StreamEvent};
use crate::proxy::protocol::WireFormat;
use crate::proxy::stream::{SseDecoder, SseEvent};
use bytes::Bytes;
use futures::StreamExt;

/// 内容出现前最多缓冲的字节数；超过后直接提交，避免无界缓冲
const MAX_GATE_BYTES: usize = 256 * 1024;

/// 一批 IR 事件对闸门的结论
#[derive(Debug)]
enum Verdict {
    /// 仍只有生命周期事件（开始、usage、空块），继续缓冲
    Pending,
    /// 可以把流交给客户端
    Commit,
    /// 内容前的上游错误：换下一家 provider
    Fail(ProxyError),
}

/// 上游线协议对应的 SSE 解码状态机（只用于检查，不写任何旁路存储）
fn upstream_decoder(upstream: WireFormat) -> Box<dyn EventDecoder> {
    match upstream {
        WireFormat::AnthropicMessages => Box::new(AnthropicEventDecoder::new()),
        WireFormat::OpenAiChat => Box::new(OpenAiChatEventDecoder::new()),
        WireFormat::OpenAiResponses => Box::new(ResponsesEventDecoder::new()),
        WireFormat::GeminiNative => Box::new(GeminiEventDecoder::new(None, None)),
    }
}

/// 事件是否带有客户端可见的内容
fn is_content(event: &StreamEvent) -> bool {
    match event {
        StreamEvent::PartStart { part, .. } => match part {
            Part::Text { text } | Part::Reasoning { text, .. } => !text.is_empty(),
            // 工具调用、加密 reasoning、图片等块在开始事件里就是完整内容
            _ => true,
        },
        StreamEvent::PartDelta { delta, .. } => match delta {
            PartDelta::Text(text) | PartDelta::Reasoning(text) => !text.is_empty(),
            PartDelta::ToolArguments(_) => true,
            PartDelta::Signature(_) => false,
        },
        _ => false,
    }
}

fn inspect(events: &[StreamEvent]) -> Verdict {
    for event in events {
        if is_content(event) {
            return Verdict::Commit;
        }
        match event {
            StreamEvent::Error { message, code } => {
                let code = code.as_deref().unwrap_or("upstream_error");
                return Verdict::Fail(ProxyError::TransformError(format!(
                    "Upstream stream failed before any content: {code}: {message}"
                )));
            }
            StreamEvent::MessageStop { .. } => return Verdict::Commit,
            _ => {}
        }
    }
    Verdict::Pending
}

/// 把一个 SSE 事件交给解码器并给出结论；解码失败时提交（交给响应转换处理）
fn inspect_sse_event(decoder: &mut dyn EventDecoder, event: &SseEvent) -> Verdict {
    match decoder.decode(event.event_name(), &event.data) {
        Ok(events) => inspect(&events),
        Err(error) => {
            log::debug!("[Failover] stream gate could not decode upstream event: {error}");
            Verdict::Commit
        }
    }
}

/// 上游流的开头是否为 JSON 文档；全为空白（或 BOM）时还无法判断
fn starts_like_json(head: &[u8]) -> Option<bool> {
    let text = match std::str::from_utf8(head) {
        Ok(text) => text,
        Err(error) => std::str::from_utf8(&head[..error.valid_up_to()]).unwrap_or_default(),
    };
    text.trim_start_matches(|ch: char| ch.is_whitespace() || ch == '\u{feff}')
        .bytes()
        .next()
        .map(|first| matches!(first, b'{' | b'['))
}

impl ResponseValidator {
    /// 缓冲上游 SSE 直到第一个内容事件；内容前的错误返回可重试错误（见模块文档）
    ///
    /// 故障转移关闭（首包超时为 0）时没有下一家可切换，直接返回原响应。
    pub(crate) async fn gate_stream_start(
        &self,
        response: ProxyResponse,
        upstream: WireFormat,
    ) -> Result<ProxyResponse, ProxyError> {
        if self.streaming_first_byte_timeout.is_zero() {
            return Ok(response);
        }
        // 上游无视 `accept-encoding: identity` 压缩了 SSE：无法解码检查，原样直通
        if get_content_encoding(response.headers()).is_some() {
            return Ok(response);
        }

        let status = response.status();
        let headers = response.headers().clone();
        let mut stream = Box::pin(response.bytes_stream());
        let mut buffered: Vec<Bytes> = Vec::new();
        let mut buffered_len = 0usize;
        let mut sniffed = false;
        let mut sse = SseDecoder::new();
        let mut decoder = upstream_decoder(upstream);

        macro_rules! commit {
            () => {{
                let replay = futures::stream::iter(buffered.into_iter().map(Ok)).chain(stream);
                return Ok(ProxyResponse::streamed(status, headers, replay));
            }};
        }

        loop {
            let next = tokio::time::timeout(self.streaming_first_byte_timeout, stream.next())
                .await
                .map_err(|_| {
                    ProxyError::Timeout(format!(
                        "流式响应在 {}s 内未产生任何内容",
                        self.streaming_first_byte_timeout.as_secs()
                    ))
                })?;

            let Some(chunk) = next else {
                // EOF：处理未以空行结束的最后一个事件，再让解码器收尾
                // （内容前就结束的流会在这里以错误事件出现）
                let mut verdict = Verdict::Pending;
                if let Some(event) = sse.finish() {
                    verdict = inspect_sse_event(decoder.as_mut(), &event);
                }
                if matches!(verdict, Verdict::Pending) {
                    verdict = match decoder.finish() {
                        Ok(events) => inspect(&events),
                        Err(_) => Verdict::Commit,
                    };
                }
                if let Verdict::Fail(error) = verdict {
                    return Err(error);
                }
                commit!();
            };
            let chunk = chunk.map_err(|error| {
                ProxyError::ForwardFailed(format!("读取流式响应内容前失败: {error}"))
            })?;
            buffered_len += chunk.len();
            buffered.push(chunk);

            if !sniffed {
                let head: Vec<u8> = buffered.concat();
                match starts_like_json(&head) {
                    // 忽略 `stream: true` 的整段 JSON：交给响应转换按原有方式处理
                    Some(true) => commit!(),
                    Some(false) => {
                        sniffed = true;
                        for event in sse.feed(&head) {
                            match inspect_sse_event(decoder.as_mut(), &event) {
                                Verdict::Pending => {}
                                Verdict::Commit => commit!(),
                                Verdict::Fail(error) => return Err(error),
                            }
                        }
                    }
                    None => {}
                }
            } else {
                let last = buffered.last().cloned().unwrap_or_default();
                for event in sse.feed(&last) {
                    match inspect_sse_event(decoder.as_mut(), &event) {
                        Verdict::Pending => {}
                        Verdict::Commit => commit!(),
                        Verdict::Fail(error) => return Err(error),
                    }
                }
            }

            if buffered_len >= MAX_GATE_BYTES {
                log::warn!(
                    "[Failover] stream gate buffered {MAX_GATE_BYTES} bytes without content; committing stream"
                );
                commit!();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderMap;
    use http::StatusCode;
    use std::time::Duration;

    fn validator() -> ResponseValidator {
        ResponseValidator {
            non_streaming_timeout: Duration::from_secs(1),
            streaming_first_byte_timeout: Duration::from_secs(1),
        }
    }

    fn chunked(chunks: Vec<String>) -> ProxyResponse {
        ProxyResponse::streamed(
            StatusCode::OK,
            HeaderMap::new(),
            futures::stream::iter(
                chunks
                    .into_iter()
                    .map(|chunk| Ok::<Bytes, std::io::Error>(Bytes::from(chunk)))
                    .collect::<Vec<_>>(),
            ),
        )
    }

    fn sse(event: &str, data: &str) -> String {
        format!("event: {event}\ndata: {data}\n\n")
    }

    fn anthropic_start() -> String {
        sse(
            "message_start",
            r#"{"type":"message_start","message":{"id":"msg_1","type":"message","role":"assistant","model":"m","content":[],"usage":{"input_tokens":1,"output_tokens":0}}}"#,
        )
    }

    fn anthropic_error() -> String {
        sse(
            "error",
            r#"{"type":"error","error":{"type":"overloaded_error","message":"busy"}}"#,
        )
    }

    #[tokio::test]
    async fn anthropic_error_before_content_is_retryable() {
        let err = validator()
            .gate_stream_start(
                chunked(vec![
                    anthropic_start(),
                    sse(
                        "content_block_start",
                        r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#,
                    ),
                    anthropic_error(),
                ]),
                WireFormat::AnthropicMessages,
            )
            .await
            .err()
            .expect("error before content should fail over");
        assert!(
            matches!(&err, ProxyError::TransformError(message) if message.contains("overloaded_error") && message.contains("busy")),
            "got {err:?}"
        );
    }

    #[tokio::test]
    async fn anthropic_error_after_content_is_passed_through() {
        let chunks = vec![
            anthropic_start(),
            sse(
                "content_block_start",
                r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#,
            ),
            sse(
                "content_block_delta",
                r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hi"}}"#,
            ),
            anthropic_error(),
        ];
        let prepared = validator()
            .gate_stream_start(chunked(chunks.clone()), WireFormat::AnthropicMessages)
            .await
            .expect("content before the error commits the stream");
        assert_eq!(
            prepared.bytes().await.unwrap(),
            Bytes::from(chunks.concat()),
            "committed bytes are replayed unchanged"
        );
    }

    #[tokio::test]
    async fn event_split_across_chunks_is_reassembled() {
        let error = anthropic_error();
        let (head, tail) = error.split_at(12);
        let err = validator()
            .gate_stream_start(
                chunked(vec![anthropic_start(), head.to_string(), tail.to_string()]),
                WireFormat::AnthropicMessages,
            )
            .await
            .err()
            .expect("split error event should still fail over");
        assert!(matches!(err, ProxyError::TransformError(_)), "got {err:?}");
    }

    #[tokio::test]
    async fn chat_in_band_error_before_content_is_retryable() {
        let err = validator()
            .gate_stream_start(
                chunked(vec![format!(
                    "data: {}\n\n",
                    r#"{"error":{"message":"upstream overloaded","type":"server_error"}}"#
                )]),
                WireFormat::OpenAiChat,
            )
            .await
            .err()
            .expect("Chat error before content should fail over");
        assert!(
            matches!(&err, ProxyError::TransformError(message) if message.contains("upstream overloaded")),
            "got {err:?}"
        );
    }

    #[tokio::test]
    async fn chat_tool_call_commits_the_stream() {
        let chunk = r#"{"id":"c1","model":"m","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_1","type":"function","function":{"name":"lookup","arguments":""}}]}}]}"#;
        let chunks = vec![format!("data: {chunk}\n\n")];
        let prepared = validator()
            .gate_stream_start(chunked(chunks.clone()), WireFormat::OpenAiChat)
            .await
            .expect("a tool call is content");
        assert_eq!(
            prepared.bytes().await.unwrap(),
            Bytes::from(chunks.concat())
        );
    }

    #[tokio::test]
    async fn responses_failed_before_output_is_retryable() {
        let err = validator()
            .gate_stream_start(
                chunked(vec![
                    sse(
                        "response.created",
                        r#"{"type":"response.created","response":{"id":"resp_1","status":"in_progress","output":[]}}"#,
                    ),
                    sse(
                        "response.failed",
                        r#"{"type":"response.failed","response":{"id":"resp_1","status":"failed","error":{"code":"server_error","message":"boom"}}}"#,
                    ),
                ]),
                WireFormat::OpenAiResponses,
            )
            .await
            .err()
            .expect("response.failed before output should fail over");
        assert!(
            matches!(&err, ProxyError::TransformError(message) if message.contains("boom")),
            "got {err:?}"
        );
    }

    #[tokio::test]
    async fn stream_ending_before_content_is_retryable() {
        let err = validator()
            .gate_stream_start(
                chunked(vec![anthropic_start()]),
                WireFormat::AnthropicMessages,
            )
            .await
            .err()
            .expect("a stream without content or terminal event should fail over");
        assert!(matches!(err, ProxyError::TransformError(_)), "got {err:?}");
    }

    #[tokio::test]
    async fn whole_json_body_is_committed_unchanged() {
        let body = r#"{"type":"message","content":[]}"#.to_string();
        let prepared = validator()
            .gate_stream_start(chunked(vec![body.clone()]), WireFormat::AnthropicMessages)
            .await
            .expect("JSON bodies are left to the response converter");
        assert_eq!(prepared.bytes().await.unwrap(), Bytes::from(body));
    }

    #[tokio::test]
    async fn redacted_reasoning_block_commits_the_stream() {
        let chunks = vec![
            anthropic_start(),
            sse(
                "content_block_start",
                r#"{"type":"content_block_start","index":0,"content_block":{"type":"redacted_thinking","data":"opaque"}}"#,
            ),
            anthropic_error(),
        ];
        let prepared = validator()
            .gate_stream_start(chunked(chunks.clone()), WireFormat::AnthropicMessages)
            .await
            .expect("a redacted thinking block is content");
        assert_eq!(
            prepared.bytes().await.unwrap(),
            Bytes::from(chunks.concat())
        );
    }

    #[tokio::test]
    async fn compressed_stream_is_passed_through_unchecked() {
        let mut headers = HeaderMap::new();
        headers.insert("content-encoding", "gzip".parse().unwrap());
        let body = Bytes::from_static(b"\x1f\x8b\x08\x00not-sse");
        let response = ProxyResponse::streamed(
            StatusCode::OK,
            headers,
            futures::stream::iter(vec![Ok::<Bytes, std::io::Error>(body.clone())]),
        );
        let prepared = validator()
            .gate_stream_start(response, WireFormat::AnthropicMessages)
            .await
            .expect("compressed SSE cannot be inspected and is passed through");
        assert_eq!(prepared.bytes().await.unwrap(), body);
    }

    #[tokio::test]
    async fn gate_is_skipped_when_failover_is_off() {
        let off = ResponseValidator {
            non_streaming_timeout: Duration::ZERO,
            streaming_first_byte_timeout: Duration::ZERO,
        };
        let chunks = vec![anthropic_start(), anthropic_error()];
        let prepared = off
            .gate_stream_start(chunked(chunks.clone()), WireFormat::AnthropicMessages)
            .await
            .expect("without failover the stream is passed through");
        assert_eq!(
            prepared.bytes().await.unwrap(),
            Bytes::from(chunks.concat())
        );
    }
}
