//! Gemini Native 编解码
//!
//! Gemini 作为**上游**（Anthropic 客户端 ← Gemini 上游），[`GeminiCodec`]：
//! - 请求编码（[`ChatRequest`] → `generateContent` 请求体）
//! - 响应解码（`GenerateContentResponse` → [`ChatResponse`]）
//! - 流式解码（`streamGenerateContent?alt=sse` → [`StreamEvent`]）
//!
//! Gemini 作为**客户端**（Gemini CLI → Anthropic 上游，CCS-187），[`GeminiClientCodec`]：
//! - 请求解码（`generateContent` 请求体 → [`ChatRequest`]，模型名与流式标志来自 URL）
//! - 响应编码（[`ChatResponse`] → `GenerateContentResponse`）
//! - 流式编码（[`StreamEvent`] → `streamGenerateContent?alt=sse`，[`GeminiEventEncoder`]）
//!
//! Gemini 客户端直连 Gemini 上游时是透传，不经过 IR；两个编解码未实现的方向返回
//! [`CodecError::Unsupported`]。
//!
//! Gemini 有两类 IR 之外的状态：
//! - assistant 历史中的原始 parts 与 functionCall 的 `thoughtSignature`，Anthropic
//!   客户端不会带回。回复侧由 [`GeminiShadowSink`] 记入
//!   [`GeminiShadowStore`]，请求侧由调用方取出后经 [`GeminiCodec::with_shadow_turns`]
//!   交给编码端回放。
//! - 客户端工具 schema（[`AnthropicToolSchemaHints`]），解码端据此纠正 Gemini
//!   偶尔放错位置的工具参数。

mod client_request;
pub(crate) mod client_response;
mod client_stream;
mod request;
mod response;
mod stream;
pub mod text;
pub mod tool_calls;

use std::sync::Arc;

use serde_json::Value;

use super::side_store::{SessionSideStore, SideStoreScope};
use super::{CodecError, EventDecoder, EventEncoder, RequestCodec, ResponseCodec, StreamCodec};
use crate::proxy::protocol::ir::{ChatRequest, ChatResponse};
use crate::proxy::protocol::WireFormat;
use crate::proxy::providers::gemini_shadow::{GeminiAssistantTurn, GeminiShadowStore};

pub use client_stream::GeminiEventEncoder;
pub use stream::GeminiEventDecoder;
pub use tool_calls::AnthropicToolSchemaHints;

/// 回复侧把助手轮次记入 shadow 存储的目标：存储与 (provider, 会话) 作用域
///
/// 没有会话 ID 时不记录（与旧转换器一致），因此只能经 [`GeminiShadowSink::new`]
/// 在三者齐全时构造。
#[derive(Debug, Clone)]
pub struct GeminiShadowSink {
    store: Arc<GeminiShadowStore>,
    provider_id: String,
    session_id: String,
}

impl GeminiShadowSink {
    /// 存储、provider 与会话 ID 任一缺失时返回 `None`
    pub fn new(
        store: Option<Arc<GeminiShadowStore>>,
        provider_id: Option<String>,
        session_id: Option<String>,
    ) -> Option<Self> {
        Some(Self {
            store: store?,
            provider_id: provider_id?,
            session_id: session_id?,
        })
    }

    pub fn store(&self) -> &GeminiShadowStore {
        &self.store
    }

    pub fn store_arc(&self) -> Arc<GeminiShadowStore> {
        Arc::clone(&self.store)
    }

    pub fn provider_id(&self) -> &str {
        &self.provider_id
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    fn record(&self, turn: &mut GeminiAssistantTurn) {
        self.store.record(
            &SideStoreScope::new(&self.provider_id, Some(&self.session_id)),
            turn,
        );
    }
}

/// Gemini Native 编解码
#[derive(Debug, Clone, Default)]
pub struct GeminiCodec {
    /// 请求侧：从 shadow 存储取出的历史助手轮次（旧 → 新）
    shadow_turns: Vec<GeminiAssistantTurn>,
    /// 回复侧：客户端工具 schema，用于纠正工具参数
    tool_schema_hints: Option<AnthropicToolSchemaHints>,
    /// 回复侧：记录助手轮次的 shadow 存储
    shadow_sink: Option<GeminiShadowSink>,
}

impl GeminiCodec {
    pub fn new() -> Self {
        Self::default()
    }

    /// 请求编码时回放的历史助手轮次
    pub fn with_shadow_turns(mut self, turns: Vec<GeminiAssistantTurn>) -> Self {
        self.shadow_turns = turns;
        self
    }

    /// 解码回复时用于纠正工具参数的客户端工具 schema
    pub fn with_tool_schema_hints(mut self, hints: Option<AnthropicToolSchemaHints>) -> Self {
        self.tool_schema_hints = hints;
        self
    }

    /// 解码回复后记录助手轮次的 shadow 存储
    pub fn with_shadow_sink(mut self, sink: Option<GeminiShadowSink>) -> Self {
        self.shadow_sink = sink;
        self
    }
}

fn unsupported(what: &str) -> CodecError {
    CodecError::Unsupported {
        format: WireFormat::GeminiNative,
        what: what.to_string(),
    }
}

fn invalid(message: impl Into<String>) -> CodecError {
    CodecError::Invalid {
        format: WireFormat::GeminiNative,
        message: message.into(),
    }
}

impl RequestCodec for GeminiCodec {
    fn format(&self) -> WireFormat {
        WireFormat::GeminiNative
    }

    fn decode_request(&self, _body: &Value) -> Result<ChatRequest, CodecError> {
        Err(unsupported("request decoding (Gemini as client)"))
    }

    fn encode_request(&self, request: &ChatRequest) -> Result<Value, CodecError> {
        request::encode_request(request, &self.shadow_turns)
    }
}

impl ResponseCodec for GeminiCodec {
    fn decode_response(&self, body: &Value) -> Result<ChatResponse, CodecError> {
        let (response, turn) = response::decode_response(body, self.tool_schema_hints.as_ref())?;
        if let (Some(sink), Some(mut turn)) = (&self.shadow_sink, turn) {
            sink.record(&mut turn);
        }
        Ok(response)
    }

    fn encode_response(&self, _response: &ChatResponse) -> Result<Value, CodecError> {
        Err(unsupported("response encoding (Gemini as client)"))
    }
}

impl StreamCodec for GeminiCodec {
    fn stream_decoder(&self) -> Result<Box<dyn EventDecoder>, CodecError> {
        Ok(Box::new(GeminiEventDecoder::new(
            self.tool_schema_hints.clone(),
            self.shadow_sink.clone(),
        )))
    }

    fn stream_encoder(&self) -> Result<Box<dyn EventEncoder>, CodecError> {
        Err(unsupported("stream encoding (Gemini as client)"))
    }
}

/// Gemini 作为客户端时的编解码（Gemini CLI 等 → 其他格式的上游）
///
/// Gemini 请求体不含模型名与流式标志（二者在 URL 中），由调用方从端点解析后传入。
#[derive(Debug, Clone, Default)]
pub struct GeminiClientCodec {
    model: String,
    stream: bool,
}

impl GeminiClientCodec {
    pub fn new(model: impl Into<String>, stream: bool) -> Self {
        Self {
            model: model.into(),
            stream,
        }
    }
}

impl RequestCodec for GeminiClientCodec {
    fn format(&self) -> WireFormat {
        WireFormat::GeminiNative
    }

    fn decode_request(&self, body: &Value) -> Result<ChatRequest, CodecError> {
        if !body.is_object() {
            return Err(invalid(
                "generateContent request body must be a JSON object",
            ));
        }
        Ok(client_request::decode_request(
            body,
            &self.model,
            self.stream,
        ))
    }

    fn encode_request(&self, _request: &ChatRequest) -> Result<Value, CodecError> {
        Err(unsupported("request encoding (Gemini as client)"))
    }
}

impl ResponseCodec for GeminiClientCodec {
    fn decode_response(&self, _body: &Value) -> Result<ChatResponse, CodecError> {
        Err(unsupported("response decoding (Gemini as client)"))
    }

    fn encode_response(&self, response: &ChatResponse) -> Result<Value, CodecError> {
        Ok(client_response::encode_response(response))
    }
}

impl StreamCodec for GeminiClientCodec {
    fn stream_decoder(&self) -> Result<Box<dyn EventDecoder>, CodecError> {
        Err(unsupported("stream decoding (Gemini as client)"))
    }

    fn stream_encoder(&self) -> Result<Box<dyn EventEncoder>, CodecError> {
        Ok(Box::new(GeminiEventEncoder::new()))
    }
}

#[cfg(test)]
mod client_tests;
#[cfg(test)]
mod tests;

/// 请求体中的模型名（Gemini 路由用）
pub fn extract_gemini_model(body: &Value) -> Option<&str> {
    body.get("model").and_then(|value| value.as_str())
}
