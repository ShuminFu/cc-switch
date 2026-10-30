//! OpenAI Responses 编解码
//!
//! Responses 在两种桥接中出现：
//! - 作为**上游**（Anthropic 客户端 ← Responses）：请求编码、响应解码、流式解码；
//! - 作为**客户端**（Codex ← Anthropic / Chat）：请求解码、响应编码、流式编码。
//!
//! 上游一侧的 reasoning 条目经 [`crate::proxy::providers::reasoning_bridge`] 的信封
//! 装进 IR 的 reasoning 签名，由 Anthropic 客户端原样带回后还原；客户端一侧的
//! Anthropic 签名 thinking 经 [`reasoning`] 的信封装进 `encrypted_content`。

mod client_request;
mod client_response;
mod client_stream;
pub(crate) mod errors;
pub(crate) mod reasoning;
mod request;
mod response;
mod stream;
pub(crate) mod tools;
pub(crate) mod usage;

use serde_json::Value;

use super::{CodecError, EventDecoder, EventEncoder, RequestCodec, ResponseCodec, StreamCodec};
use crate::proxy::protocol::ir::{ChatRequest, ChatResponse};
use crate::proxy::protocol::WireFormat;

pub use client_stream::ResponsesEventEncoder;
pub use stream::ResponsesEventDecoder;
pub(crate) use tools::CodexToolContext;

/// 工具结果为错误时，Responses `output` 数组开头的标记（Responses 没有 `is_error`）
pub(crate) const TOOL_RESULT_ERROR_MARKER: &str = "[cc-switch:tool-result-error]";

/// Claude Code 的 `Read` 工具不接受空字符串 `pages`，转给客户端前去掉
pub(crate) fn sanitize_anthropic_tool_use_input(name: &str, input: Value) -> Value {
    if name != "Read" {
        return input;
    }

    match input {
        Value::Object(mut object) => {
            if matches!(object.get("pages"), Some(Value::String(value)) if value.is_empty()) {
                object.remove("pages");
            }
            Value::Object(object)
        }
        other => other,
    }
}

pub(crate) fn sanitize_anthropic_tool_use_input_json(name: &str, raw: &str) -> String {
    if name != "Read" || raw.is_empty() {
        return raw.to_string();
    }

    let Ok(input) = serde_json::from_str::<Value>(raw) else {
        return raw.to_string();
    };

    serde_json::to_string(&sanitize_anthropic_tool_use_input(name, input))
        .unwrap_or_else(|_| raw.to_string())
}

/// 上游请求编码选项（依赖供应商配置）
#[derive(Debug, Clone, Default)]
pub struct ResponsesUpstreamOptions {
    /// 注入的 `prompt_cache_key`
    pub cache_key: Option<String>,
    /// 目标是 Codex OAuth（ChatGPT Plus/Pro 反代）后端：强制 `store: false` 等协议约束
    pub codex_oauth: bool,
    /// Codex OAuth 的 FAST 模式：注入 `service_tier = "priority"`
    pub codex_fast_mode: bool,
}

/// Responses 作为客户端时上游的协议
///
/// 两个旧转换器（Codex ← Anthropic、Codex ← Chat）对同一 Responses 请求的归一化规则
/// 与回复的条目布局不同；迁移期间按上游保留各自的形状，统一留待旧转换器删除之后。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ResponsesFlavor {
    #[default]
    AnthropicUpstream,
    ChatUpstream,
}

/// OpenAI Responses 编解码
#[derive(Debug, Clone, Default)]
pub struct ResponsesCodec {
    upstream: ResponsesUpstreamOptions,
    flavor: ResponsesFlavor,
    tool_context: CodexToolContext,
}

impl ResponsesCodec {
    pub fn new() -> Self {
        Self::default()
    }

    /// 作为客户端时的上游协议与工具上下文（由客户端请求构建）
    pub(crate) fn for_client(flavor: ResponsesFlavor, tool_context: CodexToolContext) -> Self {
        Self {
            flavor,
            tool_context,
            ..Self::default()
        }
    }

    /// 作为上游时的请求编码选项
    pub fn with_upstream_options(mut self, options: ResponsesUpstreamOptions) -> Self {
        self.upstream = options;
        self
    }
}

fn unsupported(what: &str) -> CodecError {
    CodecError::Unsupported {
        format: WireFormat::OpenAiResponses,
        what: what.to_string(),
    }
}

fn invalid(message: impl Into<String>) -> CodecError {
    CodecError::Invalid {
        format: WireFormat::OpenAiResponses,
        message: message.into(),
    }
}

impl RequestCodec for ResponsesCodec {
    fn format(&self) -> WireFormat {
        WireFormat::OpenAiResponses
    }

    fn decode_request(&self, body: &Value) -> Result<ChatRequest, CodecError> {
        client_request::decode_request(body, self.flavor)
    }

    fn encode_request(&self, request: &ChatRequest) -> Result<Value, CodecError> {
        Ok(request::encode_request(request, &self.upstream))
    }
}

impl ResponseCodec for ResponsesCodec {
    fn decode_response(&self, body: &Value) -> Result<ChatResponse, CodecError> {
        response::decode_response(body)
    }

    fn encode_response(&self, response: &ChatResponse) -> Result<Value, CodecError> {
        Ok(client_response::encode_response(
            response,
            self.flavor,
            &self.tool_context,
        ))
    }
}

impl StreamCodec for ResponsesCodec {
    fn stream_decoder(&self) -> Result<Box<dyn EventDecoder>, CodecError> {
        Ok(Box::new(ResponsesEventDecoder::new()))
    }

    fn stream_encoder(&self) -> Result<Box<dyn EventEncoder>, CodecError> {
        Ok(Box::new(ResponsesEventEncoder::new(
            self.flavor,
            self.tool_context.clone(),
        )))
    }
}

#[cfg(test)]
mod client_anthropic_tests;
#[cfg(test)]
mod client_chat_tests;
#[cfg(test)]
mod tests;
