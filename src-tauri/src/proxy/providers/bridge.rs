//! Bridge registry (PROV-21): resolve a `(client, upstream)` wire-protocol pair
//! once and convert through the canonical IR (`super::ir`).
//!
//! `api_format` strings from provider config are parsed in exactly one place
//! ([`WireProtocol::parse`] / [`WireProtocol::from_api_format`]); everything
//! else matches on the enum.

use super::ir;
use super::transform_codex_chat::CodexToolContext;
use crate::proxy::error::ProxyError;
use serde_json::Value;

/// A wire protocol spoken by a client or an upstream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum WireProtocol {
    /// Anthropic Messages (`/v1/messages`), `api_format = "anthropic"`.
    AnthropicMessages,
    /// OpenAI Chat Completions, `api_format = "openai_chat"`.
    OpenAiChat,
    /// OpenAI Responses, `api_format = "openai_responses"`.
    OpenAiResponses,
    /// Gemini `generateContent`, `api_format = "gemini_native"`.
    GeminiNative,
}

impl WireProtocol {
    /// Parse a known `api_format` value (exact match).
    pub(crate) fn parse(api_format: &str) -> Option<Self> {
        match api_format {
            "anthropic" => Some(Self::AnthropicMessages),
            "openai_chat" => Some(Self::OpenAiChat),
            "openai_responses" => Some(Self::OpenAiResponses),
            "gemini_native" => Some(Self::GeminiNative),
            _ => None,
        }
    }

    /// Parse a configured `api_format`; unknown values mean native Anthropic
    /// passthrough, as they always have.
    pub(crate) fn from_api_format(api_format: &str) -> Self {
        Self::parse(api_format).unwrap_or(Self::AnthropicMessages)
    }

    /// The canonical `api_format` string.
    pub(crate) fn api_format(self) -> &'static str {
        match self {
            Self::AnthropicMessages => "anthropic",
            Self::OpenAiChat => "openai_chat",
            Self::OpenAiResponses => "openai_responses",
            Self::GeminiNative => "gemini_native",
        }
    }
}

/// Per-request bridge policy: request-derived state an encoder needs.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct BridgePolicy<'a> {
    /// Restores Codex tool names/kinds when encoding for a Responses client.
    pub(crate) tool_context: Option<&'a CodexToolContext>,
}

/// A resolved `(client, upstream)` protocol pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Bridge {
    pub(crate) client: WireProtocol,
    pub(crate) upstream: WireProtocol,
}

impl Bridge {
    pub(crate) fn new(client: WireProtocol, upstream: WireProtocol) -> Self {
        Self { client, upstream }
    }

    /// Bridge for a Claude (Anthropic Messages) client and a provider
    /// `api_format`.
    pub(crate) fn for_claude_api_format(api_format: &str) -> Self {
        Self::new(
            WireProtocol::AnthropicMessages,
            WireProtocol::from_api_format(api_format),
        )
    }

    /// Whether requests/responses must be converted at all.
    pub(crate) fn needs_transform(&self) -> bool {
        self.client != self.upstream
    }

    /// Decode a non-streaming upstream response into the IR.
    fn decode_upstream_response(&self, body: &Value) -> Result<ir::Response, ProxyError> {
        match self.upstream {
            WireProtocol::AnthropicMessages => Ok(ir::anthropic::decode_response(body)),
            WireProtocol::OpenAiChat => ir::chat::decode_response(
                body,
                ir::chat::DecodeOptions {
                    // Reasoning models behind plain Chat gateways inline
                    // `<think>`; Responses clients get it as a reasoning item.
                    split_think_tags: self.client == WireProtocol::OpenAiResponses,
                },
            ),
            WireProtocol::OpenAiResponses => ir::responses::decode_response(body),
            WireProtocol::GeminiNative => {
                let parts = body
                    .pointer("/candidates/0/content/parts")
                    .and_then(Value::as_array)
                    .map(Vec::as_slice);
                if parts.is_none() && body.pointer("/promptFeedback/blockReason").is_none() {
                    if body.pointer("/candidates/0").is_none() {
                        return Err(ProxyError::TransformError(
                            "No candidates in Gemini response".to_string(),
                        ));
                    }
                }
                Ok(ir::gemini::decode_response(body, parts))
            }
        }
    }

    /// Convert a non-streaming upstream response into the client protocol:
    /// `decode_<upstream>` → IR → `encode_<client>`.
    pub(crate) fn response_to_client(
        &self,
        body: Value,
        policy: BridgePolicy<'_>,
    ) -> Result<Value, ProxyError> {
        if !self.needs_transform() {
            return Ok(body);
        }
        let response = self.decode_upstream_response(&body)?;
        match self.client {
            WireProtocol::AnthropicMessages => ir::anthropic::encode_response(&response),
            WireProtocol::OpenAiResponses => {
                let default_context = CodexToolContext::default();
                let layout = if self.upstream == WireProtocol::OpenAiChat {
                    ir::responses::Layout::ChatCompat
                } else {
                    ir::responses::Layout::Interleaved
                };
                Ok(ir::responses::encode_response(
                    &response,
                    policy.tool_context.unwrap_or(&default_context),
                    layout,
                ))
            }
            WireProtocol::OpenAiChat | WireProtocol::GeminiNative => {
                Err(ProxyError::TransformError(format!(
                    "No response encoder for {} clients",
                    self.client.api_format()
                )))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn api_format_round_trips_and_unknown_is_anthropic() {
        for protocol in [
            WireProtocol::AnthropicMessages,
            WireProtocol::OpenAiChat,
            WireProtocol::OpenAiResponses,
            WireProtocol::GeminiNative,
        ] {
            assert_eq!(WireProtocol::parse(protocol.api_format()), Some(protocol));
        }
        assert_eq!(WireProtocol::parse("bogus"), None);
        assert_eq!(
            WireProtocol::from_api_format("bogus"),
            WireProtocol::AnthropicMessages
        );
        assert!(!Bridge::for_claude_api_format("anthropic").needs_transform());
        assert!(Bridge::for_claude_api_format("gemini_native").needs_transform());
    }

    #[test]
    fn passthrough_bridge_returns_body_unchanged() {
        let body = json!({"anything": true});
        let bridge = Bridge::for_claude_api_format("anthropic");
        assert_eq!(
            bridge
                .response_to_client(body.clone(), BridgePolicy::default())
                .unwrap(),
            body
        );
    }

    /// The same assistant turn arriving over every upstream protocol yields the
    /// same Anthropic message (modulo ids): one IR, one encoder.
    #[test]
    fn every_upstream_decodes_to_the_same_anthropic_message() {
        let chat = json!({
            "id": "x", "model": "m",
            "choices": [{"message": {"role": "assistant", "content": "hi",
                "tool_calls": [{"id": "call_1", "type": "function",
                    "function": {"name": "get", "arguments": "{\"a\":1}"}}]},
                "finish_reason": "tool_calls"}],
            "usage": {"prompt_tokens": 10, "completion_tokens": 5,
                "prompt_tokens_details": {"cached_tokens": 4}}
        });
        let responses = json!({
            "id": "x", "model": "m", "status": "completed",
            "output": [
                {"type": "message", "role": "assistant",
                 "content": [{"type": "output_text", "text": "hi"}]},
                {"type": "function_call", "call_id": "call_1", "name": "get",
                 "arguments": "{\"a\":1}"}
            ],
            "usage": {"input_tokens": 10, "output_tokens": 5,
                "input_tokens_details": {"cached_tokens": 4}}
        });
        let gemini = json!({
            "responseId": "x", "modelVersion": "m",
            "candidates": [{"content": {"parts": [
                {"text": "hi"},
                {"functionCall": {"id": "call_1", "name": "get", "args": {"a": 1}}}
            ]}, "finishReason": "STOP"}],
            "usageMetadata": {"promptTokenCount": 10, "totalTokenCount": 15,
                "cachedContentTokenCount": 4}
        });
        let expected = json!({
            "id": "x", "type": "message", "role": "assistant",
            "content": [
                {"type": "text", "text": "hi"},
                {"type": "tool_use", "id": "call_1", "name": "get", "input": {"a": 1}}
            ],
            "model": "m", "stop_reason": "tool_use", "stop_sequence": null,
            "usage": {"input_tokens": 6, "output_tokens": 5, "cache_read_input_tokens": 4}
        });
        for (format, body) in [
            ("openai_chat", chat),
            ("openai_responses", responses),
            ("gemini_native", gemini),
        ] {
            let actual = Bridge::for_claude_api_format(format)
                .response_to_client(body, BridgePolicy::default())
                .unwrap();
            assert_eq!(actual, expected, "{format}");
        }
    }

    #[test]
    fn responses_client_round_trips_usage_through_anthropic_upstream() {
        let anthropic = json!({
            "id": "msg_1", "type": "message", "model": "claude",
            "content": [{"type": "text", "text": "ok"}],
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 6, "output_tokens": 5, "cache_read_input_tokens": 4}
        });
        let out = Bridge::new(
            WireProtocol::OpenAiResponses,
            WireProtocol::AnthropicMessages,
        )
        .response_to_client(anthropic, BridgePolicy::default())
        .unwrap();
        assert_eq!(out["status"], "completed");
        assert_eq!(out["usage"]["input_tokens"], 10);
        assert_eq!(out["usage"]["input_tokens_details"]["cached_tokens"], 4);
    }
}
