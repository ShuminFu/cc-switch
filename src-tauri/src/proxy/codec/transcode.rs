//! 流式桥接驱动：上游 SSE 字节流 → [`EventDecoder`] → IR 事件 → [`EventEncoder`] → 客户端 SSE
//!
//! SSE 分帧统一交给 [`SseDecoder`]；解码端产生 [`StreamEvent::Error`]、解码失败或上游
//! 传输出错时，编码出错误事件后立即停止读取上游，不再补发收尾事件（与旧转换器一致：
//! 失败不能伪装成正常完成）。未以空行结束的最后一个事件按 SSE 规范丢弃。

use bytes::Bytes;
use futures::stream::{Stream, StreamExt};

use super::{EventDecoder, EventEncoder};
use crate::proxy::protocol::ir::StreamEvent;
use crate::proxy::stream::SseDecoder;

/// 把上游字节流经一对编解码状态机转换为客户端 SSE 字节流
pub fn transcode_sse_stream<E: std::error::Error + Send + 'static>(
    upstream: impl Stream<Item = Result<Bytes, E>> + Send + 'static,
    mut decoder: Box<dyn EventDecoder>,
    mut encoder: Box<dyn EventEncoder>,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> + Send {
    async_stream::stream! {
        let mut sse = SseDecoder::new();
        let mut failed = false;
        tokio::pin!(upstream);

        'upstream: while let Some(chunk) = upstream.next().await {
            let events = match chunk {
                Ok(bytes) => sse.feed(&bytes),
                Err(error) => {
                    log::error!("Stream error: {error}");
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
