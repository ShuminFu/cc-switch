//! Canonical message / stream IR shared by every protocol bridge (PROV-19).
//!
//! The proxy speaks four wire protocols: Anthropic Messages, OpenAI Chat
//! Completions, OpenAI Responses and Gemini `generateContent`. Instead of one
//! hand-written converter per (client, upstream) pair, a bridge is expressed as
//!
//! ```text
//! decode_<upstream>  →  ir::Response / ir::StreamEvent  →  encode_<client>
//! ```
//!
//! Each wire concern (usage buckets, stop reasons, content parts, opaque
//! reasoning envelopes, stream lifecycle) is implemented exactly once per
//! protocol, in that protocol's codec module:
//!
//! | module                  | decode                                   | encode                                  |
//! |-------------------------|------------------------------------------|-----------------------------------------|
//! | [`anthropic`]           | usage, stop reason, content blocks, response, SSE events | usage, stop reason, content blocks, response, SSE events |
//! | [`chat`]                | usage, `finish_reason`, response         | —                                       |
//! | [`responses`]           | usage, `status`, response                | usage, `status`, response               |
//! | [`gemini`]              | `usageMetadata`, `finishReason`, response | —                                       |
//! | [`stream`]              | —                                        | response → canonical event lifecycle; events → response (collector) |
//!
//! Bridge-specific behaviour (Codex tool-name restoration, the Chat-compatible
//! Responses item layout, Gemini shadow-store recording) stays outside the codecs
//! as explicit policy arguments, so the codecs themselves are pure functions.
//!
//! ## Usage semantics
//!
//! [`Usage::input`] is always the **fresh** (cache-exclusive) input. Decoders of
//! protocols whose input count includes cache hits (Chat, Responses, Gemini)
//! subtract the cache buckets once, here; encoders of such protocols add them
//! back. The identity `input + cache_read + cache_write == upstream input`
//! therefore holds on every bridge (PROV-17).
//!
//! ## Lossy fields
//!
//! Content the target protocol cannot represent is dropped by the encoder with a
//! `log::debug!` (see each encoder). Unknown Anthropic blocks travel as
//! [`Part::Other`] so Anthropic→Anthropic aggregation stays lossless.

pub(crate) mod anthropic;
pub(crate) mod chat;
pub(crate) mod gemini;
pub(crate) mod responses;
pub(crate) mod stream;

use serde_json::Value;

/// Why the model stopped producing output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum StopReason {
    EndTurn,
    MaxTokens,
    ToolUse,
    StopSequence,
    /// Safety / content filtering (PROV-24).
    Refusal,
    PauseTurn,
    ContextWindowExceeded,
    /// A stop reason the IR does not model; carried verbatim.
    Other(String),
}

/// Tool-call arguments. Chat upstreams send a raw JSON string that may be
/// truncated; other protocols send structured JSON. The client encoder decides
/// how strictly to parse a raw string.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ToolInput {
    Json(Value),
    Raw(Option<Value>),
}

/// One canonical content part of an assistant response.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Part {
    Text(String),
    /// Model refusal text (Chat/Responses `refusal`). Anthropic has no refusal
    /// block, so its encoder renders it as text.
    Refusal(String),
    Thinking {
        text: String,
        /// Opaque replay state (Anthropic signature or a cc-switch envelope).
        signature: Option<String>,
    },
    RedactedThinking {
        data: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: ToolInput,
    },
    /// A protocol block the IR does not model (e.g. Anthropic server tool
    /// blocks). Encoders of the same protocol re-emit it verbatim; others drop it.
    Other(Value),
}

/// Token accounting with cache-exclusive input (see module docs).
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Usage {
    /// Fresh input tokens (excludes cache reads and writes).
    pub input: u64,
    pub output: u64,
    /// `Some` when the upstream reported the bucket.
    pub cache_read: Option<u64>,
    pub cache_write: Option<u64>,
    /// Anthropic `cache_creation` TTL breakdown, passed through verbatim.
    pub cache_write_breakdown: Option<Value>,
    /// Reasoning / thinking output tokens (a subset of `output`).
    pub reasoning: Option<u64>,
    /// Upstream-reported total, when it sent one.
    pub total: Option<u64>,
}

impl Usage {
    /// `input + cache_read + cache_write`: the cache-inclusive input count used
    /// by OpenAI-style protocols.
    pub(crate) fn inclusive_input(&self) -> u64 {
        self.input
            .saturating_add(self.cache_read.unwrap_or(0))
            .saturating_add(self.cache_write.unwrap_or(0))
    }
}

/// A complete (non-streaming) assistant response.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Response {
    /// Upstream message/response id; `None` or empty means "synthesize one".
    pub id: Option<String>,
    pub model: String,
    /// Unix seconds, when the upstream reported a creation time.
    pub created: Option<u64>,
    pub content: Vec<Part>,
    pub stop: Option<StopReason>,
    pub usage: Usage,
}

impl Response {
    pub(crate) fn has_tool_use(&self) -> bool {
        self.content
            .iter()
            .any(|part| matches!(part, Part::ToolUse { .. }))
    }
}
