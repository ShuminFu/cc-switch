//! 已迁移到编解码的桥接入口
//!
//! 每个函数按 [`CodecEngine`] 在旧转换器与 IR 编解码之间选择，调用方不再直接调用
//! `providers::transform*` / `streaming*`。旧路径保留一个版本作为回退
//! （`codecEngine = legacy`），删除前须满足 golden 一致且一个版本无回归。

use std::pin::Pin;

use bytes::Bytes;
use futures::stream::Stream;
use serde_json::Value;

use super::anthropic::{AnthropicCodec, AnthropicEventEncoder};
use super::openai_chat::{OpenAiChatCodec, OpenAiChatEventDecoder};
use super::transcode::transcode_sse_stream;
use super::{CodecEngine, CodecError, RequestCodec, ResponseCodec};
use crate::proxy::error::ProxyError;
use crate::proxy::providers::{streaming, transform};

/// Anthropic ← OpenAI Chat 桥接已迁移（CCS-176）
pub const ANTHROPIC_OPENAI_CHAT_MIGRATED: bool = true;

/// 装箱后的客户端 SSE 字节流
pub type ClientSseStream = Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send>>;

/// 编解码错误 → 代理错误；格式错误沿用旧转换器的消息文本
fn transform_error(error: CodecError) -> ProxyError {
    match error {
        CodecError::Invalid { message, .. } => ProxyError::TransformError(message),
        other => ProxyError::TransformError(other.to_string()),
    }
}

/// Anthropic Messages 请求 → OpenAI Chat Completions 请求
///
/// `stream_options.include_usage` 与 `prompt_cache_key` 依赖调用方上下文，仍由调用方注入。
pub fn anthropic_to_openai_chat_request(
    body: Value,
    preserve_reasoning_content: bool,
    engine: CodecEngine,
) -> Result<Value, ProxyError> {
    if !engine.uses_ir(ANTHROPIC_OPENAI_CHAT_MIGRATED) {
        return transform::anthropic_to_openai_with_reasoning_content(
            body,
            preserve_reasoning_content,
        );
    }
    let request = AnthropicCodec
        .decode_request(&body)
        .map_err(transform_error)?;
    OpenAiChatCodec::new(preserve_reasoning_content)
        .encode_request(&request)
        .map_err(transform_error)
}

/// OpenAI Chat Completions 非流式响应 → Anthropic Messages 响应
pub fn openai_chat_to_anthropic_response(
    body: Value,
    engine: CodecEngine,
) -> Result<Value, ProxyError> {
    if !engine.uses_ir(ANTHROPIC_OPENAI_CHAT_MIGRATED) {
        return transform::openai_to_anthropic(body);
    }
    let response = OpenAiChatCodec::default()
        .decode_response(&body)
        .map_err(transform_error)?;
    AnthropicCodec
        .encode_response(&response)
        .map_err(transform_error)
}

/// OpenAI Chat Completions SSE → Anthropic Messages SSE
pub fn openai_chat_to_anthropic_sse<E: std::error::Error + Send + 'static>(
    upstream: impl Stream<Item = Result<Bytes, E>> + Send + 'static,
    engine: CodecEngine,
) -> ClientSseStream {
    if !engine.uses_ir(ANTHROPIC_OPENAI_CHAT_MIGRATED) {
        return Box::pin(streaming::create_anthropic_sse_stream(upstream));
    }
    Box::pin(transcode_sse_stream(
        upstream,
        Box::new(OpenAiChatEventDecoder::new()),
        Box::new(AnthropicEventEncoder::new()),
    ))
}

#[cfg(test)]
mod tests;
