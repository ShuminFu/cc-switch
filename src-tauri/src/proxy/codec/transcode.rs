//! 流式桥接驱动：上游 SSE 字节流 → [`EventDecoder`] → IR 事件 → [`EventEncoder`] → 客户端 SSE
//!
//! SSE 分帧统一交给 [`SseDecoder`]；解码端产生 [`StreamEvent::Error`]、解码失败或上游
//! 传输出错时，编码出错误事件后立即停止读取上游，不再补发收尾事件（与旧转换器一致：
//! 失败不能伪装成正常完成）。未以空行结束的最后一个事件按 SSE 规范丢弃。
//! 上游传输错误默认编码为错误事件；[`TransportErrors::Propagate`] 改为把它作为
//! 流的 `Err` 项原样交给客户端连接（旧 Gemini 转换器的行为）。
//! [`TranscodeOptions::flush_final_event`] 让上游 EOF 时未以空行结束的最后一个事件
//! 仍被处理（缓冲型反向代理常见，旧 Responses 转换器的行为）。

use bytes::Bytes;
use futures::stream::{Stream, StreamExt};

use super::{EventDecoder, EventEncoder};
use crate::proxy::protocol::ir::StreamEvent;
use crate::proxy::stream::SseDecoder;

/// 上游传输错误的处理方式
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TransportErrors {
    /// 编码为一个 [`StreamEvent::Error`] 后正常结束客户端流
    #[default]
    AsEvent,
    /// 作为客户端流的 `Err` 项输出，客户端连接随之中断
    Propagate,
}

/// 流式桥接驱动的选项
#[derive(Debug, Clone, Copy, Default)]
pub struct TranscodeOptions {
    pub transport_errors: TransportErrors,
    /// EOF 时处理未以空行结束的最后一个事件
    pub flush_final_event: bool,
}

/// 把上游字节流经一对编解码状态机转换为客户端 SSE 字节流
pub fn transcode_sse_stream<E: std::error::Error + Send + 'static>(
    upstream: impl Stream<Item = Result<Bytes, E>> + Send + 'static,
    decoder: Box<dyn EventDecoder>,
    encoder: Box<dyn EventEncoder>,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> + Send {
    transcode_sse_stream_with(upstream, decoder, encoder, TransportErrors::AsEvent)
}

/// [`transcode_sse_stream`]，并指定上游传输错误的处理方式
pub fn transcode_sse_stream_with<E: std::error::Error + Send + 'static>(
    upstream: impl Stream<Item = Result<Bytes, E>> + Send + 'static,
    decoder: Box<dyn EventDecoder>,
    encoder: Box<dyn EventEncoder>,
    transport_errors: TransportErrors,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> + Send {
    transcode_sse_stream_opts(
        upstream,
        decoder,
        encoder,
        TranscodeOptions {
            transport_errors,
            ..TranscodeOptions::default()
        },
    )
}

/// [`transcode_sse_stream`]，并指定全部选项
pub fn transcode_sse_stream_opts<E: std::error::Error + Send + 'static>(
    upstream: impl Stream<Item = Result<Bytes, E>> + Send + 'static,
    mut decoder: Box<dyn EventDecoder>,
    mut encoder: Box<dyn EventEncoder>,
    options: TranscodeOptions,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> + Send {
    let transport_errors = options.transport_errors;
    async_stream::stream! {
        let mut sse = SseDecoder::new();
        let mut failed = false;
        tokio::pin!(upstream);

        'upstream: while let Some(chunk) = upstream.next().await {
            let events = match chunk {
                Ok(bytes) => sse.feed(&bytes),
                Err(error) => {
                    log::error!("Stream error: {error}");
                    if transport_errors == TransportErrors::Propagate {
                        yield Err(std::io::Error::other(error.to_string()));
                        failed = true;
                        break;
                    }
                    let event = StreamEvent::Error {
                        message: format!("Stream error: {error}"),
                        code: Some("stream_error".to_string()),
                    };
                    for frame in encoder.encode(&event) {
                        yield Ok(frame);
                    }
                    failed = true;
                    break;
                }
            };
            for event in events {
                let decoded = decoder
                    .decode(event.event_name(), &event.data)
                    .unwrap_or_else(|error| vec![decode_error(error)]);
                for ir_event in decoded {
                    let is_error = matches!(ir_event, StreamEvent::Error { .. });
                    for frame in encoder.encode(&ir_event) {
                        yield Ok(frame);
                    }
                    if is_error {
                        failed = true;
                        break 'upstream;
                    }
                }
            }
        }

        if !failed && options.flush_final_event {
            if let Some(event) = sse.finish() {
                let decoded = decoder
                    .decode(event.event_name(), &event.data)
                    .unwrap_or_else(|error| vec![decode_error(error)]);
                for ir_event in decoded {
                    let is_error = matches!(ir_event, StreamEvent::Error { .. });
                    for frame in encoder.encode(&ir_event) {
                        yield Ok(frame);
                    }
                    if is_error {
                        failed = true;
                        break;
                    }
                }
            }
        }
        if !failed {
            let tail = decoder
                .finish()
                .unwrap_or_else(|error| vec![decode_error(error)]);
            for ir_event in tail {
                let is_error = matches!(ir_event, StreamEvent::Error { .. });
                for frame in encoder.encode(&ir_event) {
                    yield Ok(frame);
                }
                if is_error {
                    failed = true;
                    break;
                }
            }
        }
        if !failed {
            for frame in encoder.finish() {
                yield Ok(frame);
            }
        }
    }
}

fn decode_error(error: super::CodecError) -> StreamEvent {
    log::error!("[Codec] failed to decode upstream SSE event: {error}");
    StreamEvent::Error {
        message: error.to_string(),
        code: Some("invalid_upstream_event".to_string()),
    }
}

/// 上游流的开头是否为 JSON 文档（`{` / `[`）；全为空白（或 BOM）时还无法判断
fn sniff_json_document(head: &[u8]) -> Option<bool> {
    let text = match std::str::from_utf8(head) {
        Ok(text) => text,
        // 末尾不完整的多字节字符留到下一块再判断
        Err(error) => std::str::from_utf8(&head[..error.valid_up_to()]).unwrap_or_default(),
    };
    text.trim_start_matches(|ch: char| ch.is_whitespace() || ch == '\u{feff}')
        .bytes()
        .next()
        .map(|first| matches!(first, b'{' | b'['))
}

/// 整段 JSON 响应体 → 客户端 SSE 帧
pub type JsonBodyFrames = Box<dyn FnOnce(&[u8]) -> Vec<Bytes> + Send>;

/// [`transcode_sse_stream_opts`]，并兼容忽略 `stream: true`、直接返回整个 JSON 的网关
///
/// 看到第一个非空白字符前保留原始字节：是 `{` / `[` 时（含其中的空行）缓冲到 EOF 后交给
/// `json_frames`，否则按 SSE 转换。缓冲期间的传输错误由 `encoder` 编码为 `stream_error`。
pub fn transcode_sse_or_json_stream<E: std::error::Error + Send + 'static>(
    upstream: impl Stream<Item = Result<Bytes, E>> + Send + 'static,
    json_frames: JsonBodyFrames,
    decoder: Box<dyn EventDecoder>,
    mut encoder: Box<dyn EventEncoder>,
    options: TranscodeOptions,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> + Send {
    async_stream::stream! {
        let mut upstream = Box::pin(upstream);
        let mut head: Vec<u8> = Vec::new();
        let mut is_json = false;
        let mut transport_error = None;
        loop {
            match upstream.next().await {
                Some(Ok(bytes)) => {
                    head.extend_from_slice(&bytes);
                    if let Some(json) = sniff_json_document(&head) {
                        is_json = json;
                        break;
                    }
                }
                Some(Err(error)) => {
                    transport_error = Some(error.to_string());
                    break;
                }
                None => break,
            }
        }
        if is_json {
            while let Some(chunk) = upstream.next().await {
                match chunk {
                    Ok(bytes) => head.extend_from_slice(&bytes),
                    Err(error) => {
                        transport_error = Some(error.to_string());
                        break;
                    }
                }
            }
        }
        if let Some(error) = transport_error {
            log::error!("Stream error: {error}");
            let event = StreamEvent::Error {
                message: format!("Stream error: {error}"),
                code: Some("stream_error".to_string()),
            };
            for frame in encoder.encode(&event) {
                yield Ok(frame);
            }
            return;
        }
        if is_json {
            for frame in json_frames(&head) {
                yield Ok(frame);
            }
            return;
        }

        let rest = futures::stream::iter(std::iter::once(Ok::<Bytes, E>(Bytes::from(head))))
            .chain(upstream);
        let mut converted = Box::pin(transcode_sse_stream_opts(rest, decoder, encoder, options));
        while let Some(item) = converted.next().await {
            yield item;
        }
    }
}
