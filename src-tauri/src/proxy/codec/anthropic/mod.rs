//! Anthropic Messages 编解码
//!
//! 目前只实现 Anthropic 作为**客户端**一侧所需的方向：
//! - 请求解码（Messages 请求体 → [`ChatRequest`]）
//! - 响应编码（[`ChatResponse`] → Messages 响应体）
//! - 流式编码（[`StreamEvent`] → Messages SSE）
//!
//! Anthropic 作为上游的方向（请求编码、响应 / 流式解码）随 Responses→Anthropic
//! 桥接迁移时补齐，在此之前返回 [`CodecError::Unsupported`]。

mod request;
mod response;
mod stream;

use serde_json::Value;

use super::{CodecError, EventDecoder, EventEncoder, RequestCodec, ResponseCodec, StreamCodec};
use crate::proxy::protocol::ir::{ChatRequest, ChatResponse};
use crate::proxy::protocol::WireFormat;

pub use stream::AnthropicEventEncoder;

/// Anthropic Messages 编解码
#[derive(Debug, Clone, Copy, Default)]
pub struct AnthropicCodec;

fn unsupported(what: &str) -> CodecError {
    CodecError::Unsupported {
        format: WireFormat::AnthropicMessages,
        what: what.to_string(),
    }
}

impl RequestCodec for AnthropicCodec {
    fn format(&self) -> WireFormat {
        WireFormat::AnthropicMessages
    }

    fn decode_request(&self, body: &Value) -> Result<ChatRequest, CodecError> {
        Ok(request::decode_request(body))
    }

    fn encode_request(&self, _request: &ChatRequest) -> Result<Value, CodecError> {
        Err(unsupported("request encoding (Anthropic as upstream)"))
    }
}

impl ResponseCodec for AnthropicCodec {
    fn decode_response(&self, _body: &Value) -> Result<ChatResponse, CodecError> {
        Err(unsupported("response decoding (Anthropic as upstream)"))
    }

    fn encode_response(&self, response: &ChatResponse) -> Result<Value, CodecError> {
        Ok(response::encode_response(response))
    }
}

impl StreamCodec for AnthropicCodec {
    fn stream_decoder(&self) -> Result<Box<dyn EventDecoder>, CodecError> {
        Err(unsupported("stream decoding (Anthropic as upstream)"))
    }

    fn stream_encoder(&self) -> Result<Box<dyn EventEncoder>, CodecError> {
        Ok(Box::new(AnthropicEventEncoder::new()))
    }
}

#[cfg(test)]
mod tests;
