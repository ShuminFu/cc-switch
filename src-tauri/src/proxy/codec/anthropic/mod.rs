//! Anthropic Messages 编解码
//!
//! Anthropic 作为**客户端**（Claude Code）：
//! - 请求解码（Messages 请求体 → [`ChatRequest`]）
//! - 响应编码（[`ChatResponse`] → Messages 响应体）
//! - 流式编码（[`StreamEvent`] → Messages SSE）
//!
//! Anthropic 作为**上游**（Codex ← Anthropic 网关）：
//! - 请求编码（[`ChatRequest`] → Messages 请求体，含 Anthropic 对历史与 thinking 的约束）
//! - 响应解码与流式解码（[`AnthropicEventDecoder`]）

mod request;
mod response;
pub(crate) mod sse_aggregate;
mod stream;
pub(crate) mod thinking;
mod upstream_request;
mod upstream_response;
mod upstream_stream;

use serde_json::Value;

use super::{CodecError, EventDecoder, EventEncoder, RequestCodec, ResponseCodec, StreamCodec};
use crate::proxy::protocol::ir::{ChatRequest, ChatResponse};
use crate::proxy::protocol::WireFormat;

pub use stream::AnthropicEventEncoder;
pub(crate) use upstream_stream::message_events;
pub use upstream_stream::AnthropicEventDecoder;

/// 请求没有 `max_output_tokens` 时注入的 `max_tokens`（Anthropic 要求必填）
pub const DEFAULT_MAX_TOKENS: u64 = 8192;

/// Anthropic Messages 编解码
#[derive(Debug, Clone, Copy, Default)]
pub struct AnthropicCodec;

impl AnthropicCodec {
    /// 请求编码，请求没有 `max_tokens` 时使用 `default_max_tokens`
    pub fn encode_request_with_default_max_tokens(
        &self,
        request: &ChatRequest,
        default_max_tokens: u64,
    ) -> Result<Value, CodecError> {
        upstream_request::encode_request(request, default_max_tokens)
    }
}

impl RequestCodec for AnthropicCodec {
    fn format(&self) -> WireFormat {
        WireFormat::AnthropicMessages
    }

    fn decode_request(&self, body: &Value) -> Result<ChatRequest, CodecError> {
        Ok(request::decode_request(body))
    }

    fn encode_request(&self, request: &ChatRequest) -> Result<Value, CodecError> {
        self.encode_request_with_default_max_tokens(request, DEFAULT_MAX_TOKENS)
    }
}

impl ResponseCodec for AnthropicCodec {
    fn decode_response(&self, body: &Value) -> Result<ChatResponse, CodecError> {
        upstream_response::decode_response(body)
    }

    fn encode_response(&self, response: &ChatResponse) -> Result<Value, CodecError> {
        Ok(response::encode_response(response))
    }
}

impl StreamCodec for AnthropicCodec {
    fn stream_decoder(&self) -> Result<Box<dyn EventDecoder>, CodecError> {
        Ok(Box::new(AnthropicEventDecoder::new()))
    }

    fn stream_encoder(&self) -> Result<Box<dyn EventEncoder>, CodecError> {
        Ok(Box::new(AnthropicEventEncoder::new()))
    }
}

#[cfg(test)]
mod tests;
