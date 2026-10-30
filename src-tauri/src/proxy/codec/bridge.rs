//! 已迁移到编解码的桥接入口
//!
//! 每个函数按 [`CodecEngine`] 在旧转换器与 IR 编解码之间选择，调用方不再直接调用
//! `providers::transform*` / `streaming*`。旧路径保留一个版本作为回退
//! （`codecEngine = legacy`），删除前须满足 golden 一致且一个版本无回归。

use std::pin::Pin;

use bytes::Bytes;
use futures::stream::Stream;
use serde_json::Value;

use super::anthropic::{
    message_events, AnthropicCodec, AnthropicEventDecoder, AnthropicEventEncoder,
};
use super::gemini::{AnthropicToolSchemaHints, GeminiCodec, GeminiEventDecoder, GeminiShadowSink};
use super::openai_chat::{
    decode_responses_client_response, encode_responses_client_request, OpenAiChatCodec,
    OpenAiChatEventDecoder, OpenAiChatResponsesDecoder,
};
use super::responses::{
    CodexToolContext, ResponsesCodec, ResponsesEventDecoder, ResponsesEventEncoder,
    ResponsesFlavor, ResponsesUpstreamOptions,
};
use super::side_store::{SessionSideStore, SideStoreScope};
use super::transcode::{
    transcode_sse_or_json_stream, transcode_sse_stream, transcode_sse_stream_with,
    TranscodeOptions, TransportErrors,
};
use super::{CodecEngine, CodecError, EventEncoder, RequestCodec, ResponseCodec};
use crate::provider::CodexChatReasoningConfig;
use crate::proxy::error::ProxyError;
use crate::proxy::protocol::ir::StreamEvent;
use crate::proxy::providers::gemini_shadow::GeminiShadowStore;
use crate::proxy::providers::{
    streaming, streaming_codex_anthropic, streaming_codex_chat, streaming_gemini,
    streaming_responses, transform, transform_codex_anthropic, transform_codex_chat,
    transform_gemini, transform_responses,
};

/// Anthropic ← OpenAI Chat 桥接已迁移（CCS-176）
pub const ANTHROPIC_OPENAI_CHAT_MIGRATED: bool = true;

/// Anthropic ← Gemini Native 桥接已迁移（CCS-177）
pub const ANTHROPIC_GEMINI_MIGRATED: bool = true;

/// Anthropic ← OpenAI Responses 桥接已迁移（CCS-178）
pub const ANTHROPIC_RESPONSES_MIGRATED: bool = true;

/// OpenAI Responses（Codex）← Anthropic Messages 桥接已迁移（CCS-178）
pub const RESPONSES_ANTHROPIC_MIGRATED: bool = true;

/// OpenAI Responses（Codex）← OpenAI Chat 桥接已迁移（CCS-178）
pub const RESPONSES_CHAT_MIGRATED: bool = true;

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
    Box::pin(transcode_sse_or_json_stream(
        upstream,
        Box::new(|body: &[u8]| responses_json_to_anthropic_frames(body)),
        Box::new(ResponsesEventDecoder::new()),
        Box::new(AnthropicEventEncoder::with_start_output_tokens()),
        TranscodeOptions {
            transport_errors: TransportErrors::AsEvent,
            flush_final_event: true,
        },
    ))
}

/// 请求侧的编解码错误：客户端请求本身无法转换，是不可重试的无效请求
fn invalid_request_error(error: CodecError) -> ProxyError {
    match error {
        CodecError::Invalid { message, .. } => ProxyError::InvalidRequest(message),
        other => ProxyError::TransformError(other.to_string()),
    }
}

/// OpenAI Responses 请求（Codex）→ Anthropic Messages 请求
///
/// `default_max_tokens`：请求没有 `max_output_tokens` 时注入（Anthropic 要求必填）。
pub fn responses_to_anthropic_request(
    body: Value,
    default_max_tokens: u64,
    engine: CodecEngine,
) -> Result<Value, ProxyError> {
    if !engine.uses_ir(RESPONSES_ANTHROPIC_MIGRATED) {
        return transform_codex_anthropic::responses_request_to_anthropic(body, default_max_tokens);
    }
    let request =
        ResponsesCodec::for_client(ResponsesFlavor::AnthropicUpstream, Default::default())
            .decode_request(&body)
            .map_err(invalid_request_error)?;
    AnthropicCodec
        .encode_request_with_default_max_tokens(&request, default_max_tokens)
        .map_err(invalid_request_error)
}

/// Anthropic Messages 非流式响应 → OpenAI Responses 响应
pub fn anthropic_to_responses_response(
    body: Value,
    tool_context: &CodexToolContext,
    engine: CodecEngine,
) -> Result<Value, ProxyError> {
    if !engine.uses_ir(RESPONSES_ANTHROPIC_MIGRATED) {
        return transform_codex_anthropic::anthropic_response_to_responses_with_context(
            body,
            tool_context,
        );
    }
    let response = AnthropicCodec
        .decode_response(&body)
        .map_err(transform_error)?;
    ResponsesCodec::for_client(ResponsesFlavor::AnthropicUpstream, tool_context.clone())
        .encode_response(&response)
        .map_err(transform_error)
}

/// 完整的 Anthropic 消息（或错误信封）→ Responses SSE 帧（非流式上游 → 流式客户端）
pub fn anthropic_message_to_responses_sse(
    body: &Value,
    tool_context: CodexToolContext,
    engine: CodecEngine,
) -> Vec<Bytes> {
    if !engine.uses_ir(RESPONSES_ANTHROPIC_MIGRATED) {
        return streaming_codex_anthropic::responses_sse_events_from_anthropic_message(
            body,
            tool_context,
        );
    }
    let mut encoder = ResponsesEventEncoder::new(ResponsesFlavor::AnthropicUpstream, tool_context);
    let mut frames: Vec<Bytes> = message_events(body)
        .iter()
        .flat_map(|event| encoder.encode(event))
        .collect();
    frames.extend(encoder.finish());
    frames
}

/// Anthropic Messages SSE → OpenAI Responses SSE
///
/// 与旧转换器一致：上游返回整个 JSON 消息时缓冲到 EOF 后整体转换（解析失败按没有
/// 终止事件处理）；未以空行结束的最后一个事件仍被处理；传输错误编码为 `response.failed`。
pub fn anthropic_to_responses_sse<E: std::error::Error + Send + 'static>(
    upstream: impl Stream<Item = Result<Bytes, E>> + Send + 'static,
    tool_context: CodexToolContext,
    engine: CodecEngine,
) -> ClientSseStream {
    if !engine.uses_ir(RESPONSES_ANTHROPIC_MIGRATED) {
        return Box::pin(
            streaming_codex_anthropic::create_responses_sse_stream_from_anthropic_with_context(
                upstream,
                tool_context,
            ),
        );
    }
    let json_context = tool_context.clone();
    Box::pin(transcode_sse_or_json_stream(
        upstream,
        Box::new(move |body: &[u8]| {
            let text = String::from_utf8_lossy(body);
            let candidate =
                text.trim_start_matches(|ch: char| ch.is_whitespace() || ch == '\u{feff}');
            match serde_json::from_str::<Value>(candidate) {
                Ok(message) => {
                    anthropic_message_to_responses_sse(&message, json_context, CodecEngine::Ir)
                }
                Err(_) => {
                    let mut encoder = ResponsesEventEncoder::new(
                        ResponsesFlavor::AnthropicUpstream,
                        json_context,
                    );
                    encoder.encode(&StreamEvent::Error {
                        message: "Upstream Anthropic stream ended before message_stop".to_string(),
                        code: Some("stream_truncated".to_string()),
                    })
                }
            }
        }),
        Box::new(AnthropicEventDecoder::new()),
        Box::new(ResponsesEventEncoder::new(
            ResponsesFlavor::AnthropicUpstream,
            tool_context,
        )),
        TranscodeOptions {
            transport_errors: TransportErrors::AsEvent,
            flush_final_event: true,
        },
    ))
}

/// OpenAI Responses 请求（Codex）→ OpenAI Chat Completions 请求
///
/// `reasoning_config`：供应商声明的 thinking / effort 能力；`stream_options.include_usage`
/// 在流式请求中注入。会话历史的恢复与 `prompt_cache_key` 依赖调用方上下文，仍由调用方处理。
pub fn responses_to_chat_request(
    body: Value,
    reasoning_config: Option<&CodexChatReasoningConfig>,
    engine: CodecEngine,
) -> Result<Value, ProxyError> {
    if !engine.uses_ir(RESPONSES_CHAT_MIGRATED) {
        return transform_codex_chat::responses_to_chat_completions_with_reasoning(
            body,
            reasoning_config,
        );
    }
    let request = ResponsesCodec::for_client(ResponsesFlavor::ChatUpstream, Default::default())
        .decode_request(&body)
        .map_err(invalid_request_error)?;
    Ok(encode_responses_client_request(&request, reasoning_config))
}

/// OpenAI Chat Completions 非流式响应 → OpenAI Responses 响应
pub fn chat_to_responses_response(
    body: Value,
    tool_context: &CodexToolContext,
    engine: CodecEngine,
) -> Result<Value, ProxyError> {
    if !engine.uses_ir(RESPONSES_CHAT_MIGRATED) {
        return transform_codex_chat::chat_completion_to_response_with_context(body, tool_context);
    }
    let response = decode_responses_client_response(&body).map_err(transform_error)?;
    ResponsesCodec::for_client(ResponsesFlavor::ChatUpstream, tool_context.clone())
        .encode_response(&response)
        .map_err(transform_error)
}

/// OpenAI Chat Completions SSE → OpenAI Responses SSE
pub fn chat_to_responses_sse<E: std::error::Error + Send + 'static>(
    upstream: impl Stream<Item = Result<Bytes, E>> + Send + 'static,
    tool_context: CodexToolContext,
    engine: CodecEngine,
) -> ClientSseStream {
    if !engine.uses_ir(RESPONSES_CHAT_MIGRATED) {
        return Box::pin(
            streaming_codex_chat::create_responses_sse_stream_from_chat_with_context(
                upstream,
                tool_context,
            ),
        );
    }
    Box::pin(transcode_sse_stream(
        upstream,
        Box::new(OpenAiChatResponsesDecoder::new()),
        Box::new(ResponsesEventEncoder::new(
            ResponsesFlavor::ChatUpstream,
            tool_context,
        )),
    ))
}

#[cfg(test)]
mod tests;
