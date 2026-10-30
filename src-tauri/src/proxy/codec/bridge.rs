//! 已迁移到编解码的桥接入口
//!
//! 每个函数按 [`CodecEngine`] 在旧转换器与 IR 编解码之间选择，调用方不再直接调用
//! `providers::transform*` / `streaming*`。旧路径保留一个版本作为回退
//! （`codecEngine = legacy`），删除前须满足 golden 一致且一个版本无回归。

use std::pin::Pin;

use bytes::Bytes;
use futures::stream::{Stream, StreamExt};
use serde_json::Value;

use super::anthropic::{AnthropicCodec, AnthropicEventEncoder};
use super::gemini::{AnthropicToolSchemaHints, GeminiCodec, GeminiEventDecoder, GeminiShadowSink};
use super::openai_chat::{OpenAiChatCodec, OpenAiChatEventDecoder};
use super::responses::{ResponsesCodec, ResponsesEventDecoder, ResponsesUpstreamOptions};
use super::side_store::{SessionSideStore, SideStoreScope};
use super::transcode::{
    transcode_sse_stream, transcode_sse_stream_opts, transcode_sse_stream_with, TranscodeOptions,
    TransportErrors,
};
use super::{CodecEngine, CodecError, EventEncoder, RequestCodec, ResponseCodec};
use crate::proxy::error::ProxyError;
use crate::proxy::protocol::ir::StreamEvent;
use crate::proxy::providers::gemini_shadow::GeminiShadowStore;
use crate::proxy::providers::{
    streaming, streaming_gemini, streaming_responses, transform, transform_gemini,
    transform_responses,
};

/// Anthropic ← OpenAI Chat 桥接已迁移（CCS-176）
pub const ANTHROPIC_OPENAI_CHAT_MIGRATED: bool = true;

/// Anthropic ← Gemini Native 桥接已迁移（CCS-177）
pub const ANTHROPIC_GEMINI_MIGRATED: bool = true;

/// Anthropic ← OpenAI Responses 桥接已迁移（CCS-178）
pub const ANTHROPIC_RESPONSES_MIGRATED: bool = true;

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

/// Anthropic Messages 请求 → OpenAI Responses 请求
pub fn anthropic_to_responses_request(
    body: Value,
    options: ResponsesUpstreamOptions,
    engine: CodecEngine,
) -> Result<Value, ProxyError> {
    if !engine.uses_ir(ANTHROPIC_RESPONSES_MIGRATED) {
        return transform_responses::anthropic_to_responses(
            body,
            options.cache_key.as_deref(),
            options.codex_oauth,
            options.codex_fast_mode,
        );
    }
    let request = AnthropicCodec
        .decode_request(&body)
        .map_err(transform_error)?;
    ResponsesCodec::new()
        .with_upstream_options(options)
        .encode_request(&request)
        .map_err(transform_error)
}

/// OpenAI Responses 非流式响应 → Anthropic Messages 响应
pub fn responses_to_anthropic_response(
    body: Value,
    engine: CodecEngine,
) -> Result<Value, ProxyError> {
    if !engine.uses_ir(ANTHROPIC_RESPONSES_MIGRATED) {
        return transform_responses::responses_to_anthropic(body);
    }
    let response = ResponsesCodec::new()
        .decode_response(&body)
        .map_err(transform_error)?;
    AnthropicCodec
        .encode_response(&response)
        .map_err(transform_error)
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

/// 忽略 `stream: true`、直接返回整个 Responses JSON 的网关：转换为完整的 Anthropic 流
fn responses_json_to_anthropic_frames(body: &[u8]) -> Vec<Bytes> {
    let text = String::from_utf8_lossy(body);
    let mut encoder = AnthropicEventEncoder::new().with_tool_start_input();
    let error = |message: String, code: &str| StreamEvent::Error {
        message,
        code: Some(code.to_string()),
    };
    let events = match serde_json::from_str::<Value>(text.trim()) {
        Err(parse_error) => vec![error(
            format!("Invalid JSON response from Responses upstream: {parse_error}"),
            "response_parse_error",
        )],
        Ok(body) => match ResponsesCodec::new().decode_response(&body) {
            Err(codec_error) => vec![error(
                transform_error(codec_error).to_string(),
                "response_transform_error",
            )],
            Ok(response) => {
                let mut events = response.to_events();
                // message_start 报告完整的输入侧 usage（输出 token 由编码端置 0）
                if let Some(StreamEvent::MessageStart { usage, .. }) = events.first_mut() {
                    *usage = response.usage;
                }
                events
            }
        },
    };
    let mut frames: Vec<Bytes> = events
        .iter()
        .flat_map(|event| encoder.encode(event))
        .collect();
    frames.extend(encoder.finish());
    frames
}

/// OpenAI Responses SSE → Anthropic Messages SSE
///
/// 与旧转换器一致：上游忽略 `stream: true` 返回整个 JSON 时（开头为 `{` / `[`）缓冲到
/// EOF 后整体转换；未以空行结束的最后一个事件仍被处理；传输错误编码为 `stream_error`。
pub fn responses_to_anthropic_sse<E: std::error::Error + Send + 'static>(
    upstream: impl Stream<Item = Result<Bytes, E>> + Send + 'static,
    engine: CodecEngine,
) -> ClientSseStream {
    if !engine.uses_ir(ANTHROPIC_RESPONSES_MIGRATED) {
        return Box::pin(streaming_responses::create_anthropic_sse_stream_from_responses(upstream));
    }
    Box::pin(async_stream::stream! {
        let mut upstream = Box::pin(upstream);
        let mut head: Vec<u8> = Vec::new();
        let mut is_json = false;
        // 嗅探：看到第一个非空白字符前保留原始字节
        loop {
            match upstream.next().await {
                Some(Ok(bytes)) => {
                    head.extend_from_slice(&bytes);
                    match sniff_json_document(&head) {
                        None => continue,
                        Some(json) => {
                            is_json = json;
                            break;
                        }
                    }
                }
                Some(Err(error)) => {
                    log::error!("Responses stream error: {error}");
                    let mut encoder = AnthropicEventEncoder::new();
                    for frame in encoder.encode(&StreamEvent::Error {
                        message: format!("Stream error: {error}"),
                        code: Some("stream_error".to_string()),
                    }) {
                        yield Ok(frame);
                    }
                    return;
                }
                None => break,
            }
        }

        if is_json {
            // JSON 文档（含其中的空行）原样缓冲到 EOF
            while let Some(chunk) = upstream.next().await {
                match chunk {
                    Ok(bytes) => head.extend_from_slice(&bytes),
                    Err(error) => {
                        log::error!("Responses stream error: {error}");
                        let mut encoder = AnthropicEventEncoder::new();
                        for frame in encoder.encode(&StreamEvent::Error {
                            message: format!("Stream error: {error}"),
                            code: Some("stream_error".to_string()),
                        }) {
                            yield Ok(frame);
                        }
                        return;
                    }
                }
            }
            for frame in responses_json_to_anthropic_frames(&head) {
                yield Ok(frame);
            }
            return;
        }

        let rest = futures::stream::iter(std::iter::once(Ok::<Bytes, E>(Bytes::from(head))))
            .chain(upstream);
        let mut converted = Box::pin(transcode_sse_stream_opts(
            rest,
            Box::new(ResponsesEventDecoder::new()),
            Box::new(AnthropicEventEncoder::with_start_output_tokens()),
            TranscodeOptions {
                transport_errors: TransportErrors::AsEvent,
                flush_final_event: true,
            },
        ));
        while let Some(item) = converted.next().await {
            yield item;
        }
    })
}

#[cfg(test)]
mod tests;
