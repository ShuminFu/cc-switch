//! 上游流式响应的通用解码工具。

pub(crate) mod sse_decoder;

pub(crate) use sse_decoder::{SseDecoder, SseEvent};
