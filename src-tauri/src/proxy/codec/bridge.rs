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
use super::gemini::{AnthropicToolSchemaHints, GeminiCodec, GeminiEventDecoder, GeminiShadowSink};
use super::openai_chat::{OpenAiChatCodec, OpenAiChatEventDecoder};
use super::side_store::{SessionSideStore, SideStoreScope};
use super::transcode::{transcode_sse_stream, transcode_sse_stream_with, TransportErrors};
use super::{CodecEngine, CodecError, RequestCodec, ResponseCodec};
use crate::proxy::error::ProxyError;
use crate::proxy::providers::gemini_shadow::GeminiShadowStore;
use crate::proxy::providers::{streaming, streaming_gemini, transform, transform_gemini};

/// Anthropic ← OpenAI Chat 桥接已迁移（CCS-176）
pub const ANTHROPIC_OPENAI_CHAT_MIGRATED: bool = true;

/// Anthropic ← Gemini Native 桥接已迁移（CCS-177）
pub const ANTHROPIC_GEMINI_MIGRATED: bool = true;

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

/// Anthropic Messages 请求 → Gemini `generateContent` 请求
///
/// `shadow_store` 与 `provider_id` 齐全时回放该 (provider, 会话) 记录的助手轮次。
pub fn anthropic_to_gemini_request(
    body: Value,
    shadow_store: Option<&GeminiShadowStore>,
    provider_id: Option<&str>,
    session_id: Option<&str>,
    engine: CodecEngine,
) -> Result<Value, ProxyError> {
    if !engine.uses_ir(ANTHROPIC_GEMINI_MIGRATED) {
        return transform_gemini::anthropic_to_gemini_with_shadow(
            body,
            shadow_store,
            provider_id,
            session_id,
        );
    }
    let mut shadow_turns = Vec::new();
    if let (Some(store), Some(provider_id)) = (shadow_store, provider_id) {
        store.restore(
            &SideStoreScope::new(provider_id, session_id),
            &mut shadow_turns,
        );
    }
    let request = AnthropicCodec
        .decode_request(&body)
        .map_err(transform_error)?;
    GeminiCodec::new()
        .with_shadow_turns(shadow_turns)
        .encode_request(&request)
        .map_err(transform_error)
}

/// Gemini 非流式响应 → Anthropic Messages 响应；`shadow` 存在时记录本轮助手回复
pub fn gemini_to_anthropic_response(
    body: Value,
    shadow: Option<GeminiShadowSink>,
    tool_schema_hints: Option<&AnthropicToolSchemaHints>,
    engine: CodecEngine,
) -> Result<Value, ProxyError> {
    if !engine.uses_ir(ANTHROPIC_GEMINI_MIGRATED) {
        return transform_gemini::gemini_to_anthropic_with_shadow_and_hints(
            body,
            shadow.as_ref().map(GeminiShadowSink::store),
            shadow.as_ref().map(GeminiShadowSink::provider_id),
            shadow.as_ref().map(GeminiShadowSink::session_id),
            tool_schema_hints,
        );
    }
    let response = GeminiCodec::new()
        .with_tool_schema_hints(tool_schema_hints.cloned())
        .with_shadow_sink(shadow)
        .decode_response(&body)
        .map_err(transform_error)?;
    AnthropicCodec
        .encode_response(&response)
        .map_err(transform_error)
}

/// Gemini SSE → Anthropic Messages SSE；`shadow` 存在时在流正常结束后记录本轮助手回复
///
/// 与旧转换器一致，上游传输错误作为客户端流的 `Err` 项输出（不编码为 SSE 错误事件）。
pub fn gemini_to_anthropic_sse<E: std::error::Error + Send + 'static>(
    upstream: impl Stream<Item = Result<Bytes, E>> + Send + 'static,
    shadow: Option<GeminiShadowSink>,
    tool_schema_hints: Option<AnthropicToolSchemaHints>,
    engine: CodecEngine,
) -> ClientSseStream {
    if !engine.uses_ir(ANTHROPIC_GEMINI_MIGRATED) {
        let (store, provider_id, session_id) = match shadow {
            Some(sink) => (
                Some(sink.store_arc()),
                Some(sink.provider_id().to_string()),
                Some(sink.session_id().to_string()),
            ),
            None => (None, None, None),
        };
        return Box::pin(streaming_gemini::create_anthropic_sse_stream_from_gemini(
            upstream,
            store,
            provider_id,
            session_id,
            tool_schema_hints,
        ));
    }
    Box::pin(transcode_sse_stream_with(
        upstream,
        Box::new(GeminiEventDecoder::new(tool_schema_hints, shadow)),
        Box::new(AnthropicEventEncoder::with_start_output_tokens()),
        TransportErrors::Propagate,
    ))
}

#[cfg(test)]
mod tests;
