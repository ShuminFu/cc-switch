//! 流式格式转换器的统一接口。
//!
//! 既有转换器（`providers::streaming*`）各自是一个 `fn(stream, ..) -> impl Stream`，
//! handler 里按 api_format 手写 if/else 选择并装箱。这里把它们原样包在
//! [`StreamDecoder`]（上游流 → 客户端 SSE 流）与 [`StreamEncoder`]（完整上游
//! 消息 → 客户端 SSE 事件）两个 trait 之后，转换逻辑本身不做改动；
//! handler 只需按路由选出实现，再接上 usage/timing tap。

use std::pin::Pin;
use std::sync::Arc;

use bytes::Bytes;
use futures::Stream;
use serde_json::Value;

use crate::proxy::codec::bridge::{
    anthropic_message_to_responses_sse, anthropic_to_gemini_client_sse, anthropic_to_responses_sse,
    chat_to_responses_sse, gemini_to_anthropic_sse, openai_chat_to_anthropic_sse,
    responses_to_anthropic_sse,
};
use crate::proxy::codec::gemini::{AnthropicToolSchemaHints, GeminiShadowSink};
use crate::proxy::codec::responses::CodexToolContext;
use crate::proxy::protocol::{ClaudeApiFormat, WireFormat};
use crate::proxy::providers::{
    codex_chat_history::{record_responses_sse_stream, CodexChatHistoryStore},
    gemini_shadow::GeminiShadowStore,
};

/// 装箱后的 SSE 字节流
pub(crate) type ByteStream = Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send>>;

/// 把上游的流式响应逐块转换为客户端线协议的 SSE 字节流
pub(crate) trait StreamDecoder: Send {
    /// 上游响应的线协议
    fn upstream_format(&self) -> WireFormat;
    /// 输出给客户端的线协议
    fn client_format(&self) -> WireFormat;
    /// 接管上游字节流，返回转换后的 SSE 字节流
    fn decode(self: Box<Self>, upstream: ByteStream) -> ByteStream;
}

/// 把一个完整的（已缓冲的）上游消息编码为客户端线协议的 SSE 事件序列
///
/// 输出线协议与同名 [`StreamDecoder`] 实现的 `client_format` 一致。
pub(crate) trait StreamEncoder: Send {
    /// 编码完整消息为 SSE 事件
    fn encode(self: Box<Self>, message: &Value) -> Vec<Bytes>;

    /// 编码为可直接作为响应体的 SSE 字节流
    fn encode_stream(self: Box<Self>, message: &Value) -> ByteStream {
        let events = self.encode(message);
        Box::pin(futures::stream::iter(
            events.into_iter().map(Ok::<Bytes, std::io::Error>),
        ))
    }
}

/// OpenAI Chat Completions 流 → Anthropic Messages SSE
pub(crate) struct ChatToAnthropic;

impl StreamDecoder for ChatToAnthropic {
    fn upstream_format(&self) -> WireFormat {
        WireFormat::OpenAiChat
    }
    fn client_format(&self) -> WireFormat {
        WireFormat::AnthropicMessages
    }
    fn decode(self: Box<Self>, upstream: ByteStream) -> ByteStream {
        openai_chat_to_anthropic_sse(upstream, crate::settings::codec_engine())
    }
}

/// OpenAI Responses 流 → Anthropic Messages SSE
pub(crate) struct ResponsesToAnthropic;

impl StreamDecoder for ResponsesToAnthropic {
    fn upstream_format(&self) -> WireFormat {
        WireFormat::OpenAiResponses
    }
    fn client_format(&self) -> WireFormat {
        WireFormat::AnthropicMessages
    }
    fn decode(self: Box<Self>, upstream: ByteStream) -> ByteStream {
        responses_to_anthropic_sse(upstream, crate::settings::codec_engine())
    }
}

/// Gemini Native 流 → Anthropic Messages SSE
pub(crate) struct GeminiToAnthropic {
    pub shadow_store: Option<Arc<GeminiShadowStore>>,
    pub provider_id: Option<String>,
    pub session_id: Option<String>,
    pub tool_schema_hints: Option<AnthropicToolSchemaHints>,
}

impl StreamDecoder for GeminiToAnthropic {
    fn upstream_format(&self) -> WireFormat {
        WireFormat::GeminiNative
    }
    fn client_format(&self) -> WireFormat {
        WireFormat::AnthropicMessages
    }
    fn decode(self: Box<Self>, upstream: ByteStream) -> ByteStream {
        let this = *self;
        gemini_to_anthropic_sse(
            upstream,
            GeminiShadowSink::new(this.shadow_store, this.provider_id, this.session_id),
            this.tool_schema_hints,
            crate::settings::codec_engine(),
        )
    }
}

/// OpenAI Chat Completions 流 → OpenAI Responses SSE（并把完成的响应记入 Chat 历史）
pub(crate) struct ChatToResponses {
    pub tool_context: CodexToolContext,
    pub history: Arc<CodexChatHistoryStore>,
}

impl StreamDecoder for ChatToResponses {
    fn upstream_format(&self) -> WireFormat {
        WireFormat::OpenAiChat
    }
    fn client_format(&self) -> WireFormat {
        WireFormat::OpenAiResponses
    }
    fn decode(self: Box<Self>, upstream: ByteStream) -> ByteStream {
        let this = *self;
        let converted =
            chat_to_responses_sse(upstream, this.tool_context, crate::settings::codec_engine());
        Box::pin(record_responses_sse_stream(converted, this.history))
    }
}

/// Anthropic Messages 流 → OpenAI Responses SSE
pub(crate) struct AnthropicToResponses {
    pub tool_context: CodexToolContext,
}

impl StreamDecoder for AnthropicToResponses {
    fn upstream_format(&self) -> WireFormat {
        WireFormat::AnthropicMessages
    }
    fn client_format(&self) -> WireFormat {
        WireFormat::OpenAiResponses
    }
    fn decode(self: Box<Self>, upstream: ByteStream) -> ByteStream {
        anthropic_to_responses_sse(upstream, self.tool_context, crate::settings::codec_engine())
    }
}

impl StreamEncoder for AnthropicToResponses {
    fn encode(self: Box<Self>, message: &Value) -> Vec<Bytes> {
        anthropic_message_to_responses_sse(
            message,
            self.tool_context,
            crate::settings::codec_engine(),
        )
    }
}

/// Anthropic Messages 流 → Gemini `streamGenerateContent?alt=sse`（Gemini CLI 客户端）
pub(crate) struct AnthropicToGemini;

impl StreamDecoder for AnthropicToGemini {
    fn upstream_format(&self) -> WireFormat {
        WireFormat::AnthropicMessages
    }
    fn client_format(&self) -> WireFormat {
        WireFormat::GeminiNative
    }
    fn decode(self: Box<Self>, upstream: ByteStream) -> ByteStream {
        anthropic_to_gemini_client_sse(upstream)
    }
}

/// 用 `decoder` 转换上游字节流，返回客户端线协议与转换后的 SSE 流
pub(crate) fn decode_upstream(
    decoder: Box<dyn StreamDecoder>,
    tag: &str,
    upstream: ByteStream,
) -> (WireFormat, ByteStream) {
    let client_format = decoder.client_format();
    log::debug!(
        "[{tag}] 流式转换: {:?} → {client_format:?}",
        decoder.upstream_format()
    );
    (client_format, decoder.decode(upstream))
}

/// 按 Claude 供应商的 api_format 选择上游 → Anthropic 的流式解码器
///
/// `gemini` 仅在 `GeminiNative` 时使用；其余非 Responses 格式沿用 Chat 解码器。
pub(crate) fn anthropic_decoder(
    api_format: ClaudeApiFormat,
    gemini: GeminiToAnthropic,
) -> Box<dyn StreamDecoder> {
    match api_format {
        ClaudeApiFormat::OpenAiResponses => Box::new(ResponsesToAnthropic),
        ClaudeApiFormat::GeminiNative => Box::new(gemini),
        ClaudeApiFormat::OpenAiChat | ClaudeApiFormat::Anthropic => Box::new(ChatToAnthropic),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proxy::providers::streaming::create_anthropic_sse_stream;
    use crate::proxy::providers::streaming_codex_anthropic::{
        create_responses_sse_stream_from_anthropic_with_context,
        responses_sse_events_from_anthropic_message,
    };
    use futures::StreamExt;

    fn upstream(chunks: &[&str]) -> ByteStream {
        let chunks: Vec<Result<Bytes, std::io::Error>> = chunks
            .iter()
            .map(|c| Ok(Bytes::copy_from_slice(c.as_bytes())))
            .collect();
        Box::pin(futures::stream::iter(chunks))
    }

    async fn collect(stream: ByteStream) -> String {
        let chunks: Vec<_> = stream.collect().await;
        chunks
            .into_iter()
            .map(|c| String::from_utf8(c.unwrap().to_vec()).unwrap())
            .collect()
    }

    fn no_gemini() -> GeminiToAnthropic {
        GeminiToAnthropic {
            shadow_store: None,
            provider_id: None,
            session_id: None,
            tool_schema_hints: None,
        }
    }

    #[test]
    fn anthropic_decoder_selects_by_api_format() {
        let cases = [
            (
                ClaudeApiFormat::OpenAiResponses,
                WireFormat::OpenAiResponses,
            ),
            (ClaudeApiFormat::GeminiNative, WireFormat::GeminiNative),
            (ClaudeApiFormat::OpenAiChat, WireFormat::OpenAiChat),
        ];
        for (api_format, upstream_format) in cases {
            let decoder = anthropic_decoder(api_format, no_gemini());
            assert_eq!(decoder.upstream_format(), upstream_format);
            assert_eq!(decoder.client_format(), WireFormat::AnthropicMessages);
        }
    }

    /// trait 包装只做转发：输出与直接调用原转换器逐字节一致
    #[tokio::test]
    async fn decoders_are_byte_identical_to_the_wrapped_converters() {
        let chat = [
            "data: {\"id\":\"c1\",\"model\":\"gpt\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"Hi\"}}]}\n\n",
            "data: {\"id\":\"c1\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":3,\"completion_tokens\":1}}\n\n",
            "data: [DONE]\n\n",
        ];
        let via_trait = collect(Box::new(ChatToAnthropic).decode(upstream(&chat))).await;
        let direct = collect(Box::pin(create_anthropic_sse_stream(upstream(&chat)))).await;
        assert_eq!(via_trait, direct);
        assert!(via_trait.contains("message_start"));

        let anthropic = [
            "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"model\":\"claude\",\"usage\":{\"input_tokens\":3}}}\n\n",
            "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":1}}\n\n",
            "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
        ];
        let decoder = AnthropicToResponses {
            tool_context: CodexToolContext::default(),
        };
        let via_trait = collect(Box::new(decoder).decode(upstream(&anthropic))).await;
        let direct = collect(Box::pin(
            create_responses_sse_stream_from_anthropic_with_context(
                upstream(&anthropic),
                CodexToolContext::default(),
            ),
        ))
        .await;
        assert_eq!(via_trait, direct);
    }

    #[tokio::test]
    async fn encoder_matches_wrapped_message_converter() {
        let message = serde_json::json!({
            "id": "msg_1",
            "type": "message",
            "role": "assistant",
            "model": "claude",
            "content": [{"type": "text", "text": "hi"}],
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 1, "output_tokens": 1}
        });
        let encoder = Box::new(AnthropicToResponses {
            tool_context: CodexToolContext::default(),
        });
        assert_eq!(
            StreamDecoder::client_format(&*encoder),
            WireFormat::OpenAiResponses
        );
        let via_trait = collect(encoder.encode_stream(&message)).await;
        let direct: String =
            responses_sse_events_from_anthropic_message(&message, CodexToolContext::default())
                .into_iter()
                .map(|b| String::from_utf8(b.to_vec()).unwrap())
                .collect();
        assert_eq!(via_trait, direct);
        assert!(via_trait.contains("response.completed"));
    }
}
