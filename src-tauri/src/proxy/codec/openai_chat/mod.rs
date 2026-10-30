//! OpenAI Chat Completions 编解码
//!
//! 目前只实现 Chat 作为**上游**一侧所需的方向：
//! - 请求编码（[`ChatRequest`] → Chat Completions 请求体）
//! - 响应解码（Chat Completions 响应体 → [`ChatResponse`]）
//! - 流式解码（Chat Completions SSE → [`StreamEvent`]）
//!
//! 没有客户端直接说 Chat 协议，反方向返回 [`CodecError::Unsupported`]；
//! Responses→Chat 桥接迁移时只会用到同一侧的方向。

pub(crate) mod request;
mod response;
mod stream;

use serde_json::Value;

use super::{CodecError, EventDecoder, EventEncoder, RequestCodec, ResponseCodec, StreamCodec};
use crate::proxy::protocol::ir::{ChatRequest, ChatResponse};
use crate::proxy::protocol::WireFormat;

pub use stream::OpenAiChatEventDecoder;

/// OpenAI Chat Completions 编解码
#[derive(Debug, Clone, Copy, Default)]
pub struct OpenAiChatCodec {
    /// 在带工具调用的 assistant 历史消息上回填 `reasoning_content`
    /// （Moonshot/Kimi/DeepSeek 等 thinking 模型要求；通用上游不发送该非标准字段）
    pub preserve_reasoning_content: bool,
}

impl OpenAiChatCodec {
    pub fn new(preserve_reasoning_content: bool) -> Self {
        Self {
            preserve_reasoning_content,
        }
    }
}

fn unsupported(what: &str) -> CodecError {
    CodecError::Unsupported {
        format: WireFormat::OpenAiChat,
        what: what.to_string(),
    }
}

fn invalid(message: &str) -> CodecError {
    CodecError::Invalid {
        format: WireFormat::OpenAiChat,
        message: message.to_string(),
    }
}

impl RequestCodec for OpenAiChatCodec {
    fn format(&self) -> WireFormat {
        WireFormat::OpenAiChat
    }

    fn decode_request(&self, _body: &Value) -> Result<ChatRequest, CodecError> {
        Err(unsupported("request decoding (Chat as client)"))
    }

    fn encode_request(&self, request: &ChatRequest) -> Result<Value, CodecError> {
        Ok(request::encode_request(
            request,
            self.preserve_reasoning_content,
        ))
    }
}

impl ResponseCodec for OpenAiChatCodec {
    fn decode_response(&self, body: &Value) -> Result<ChatResponse, CodecError> {
        response::decode_response(body)
    }

    fn encode_response(&self, _response: &ChatResponse) -> Result<Value, CodecError> {
        Err(unsupported("response encoding (Chat as client)"))
    }
}

impl StreamCodec for OpenAiChatCodec {
    fn stream_decoder(&self) -> Result<Box<dyn EventDecoder>, CodecError> {
        Ok(Box::new(OpenAiChatEventDecoder::new()))
    }

    fn stream_encoder(&self) -> Result<Box<dyn EventEncoder>, CodecError> {
        Err(unsupported("stream encoding (Chat as client)"))
    }
}

#[cfg(test)]
mod tests;
