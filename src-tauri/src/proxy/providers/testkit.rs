//! Golden bridge testkit.
//!
//! Drives the protocol bridges from on-disk fixtures so their behaviour can be
//! pinned by snapshots and compared before and after a refactor:
//!
//! - Anthropic Messages clients: `anthropic__openai_chat`, `anthropic__gemini`,
//!   `anthropic__responses`;
//! - OpenAI Responses (Codex) clients: `responses__chat`, `responses__anthropic`;
//! - Gemini native clients (Gemini CLI): `gemini__anthropic`.
//!
//! Fixture layout (one directory per case):
//!
//! ```text
//! tests/fixtures/proxy/<bridge>/<case>/
//!   meta.toml                 intent, source, source commit, options, invariants
//!   client_request.json       optional: request sent by the client (Anthropic,
//!                             Responses or Gemini, per the bridge's client protocol)
//!   upstream_response.sse     optional: raw upstream SSE body (stream case)
//!   upstream_response.json    optional: upstream JSON body (non-stream case)
//!   upstream_request.snap     snapshot: converted upstream request
//!   client_response.snap      snapshot: converted client response
//!   usage.snap                snapshot: usage the proxy would record
//! ```
//!
//! Besides producing the snapshot texts, the kit re-splits every stream input
//! into chunks (every byte offset for inputs up to [`EXHAUSTIVE_SPLIT_LIMIT`]
//! bytes, [`RANDOM_SPLIT_PLANS`] seeded random multi-splits, every UTF-8
//! continuation byte and every CRLF pair) and checks that each split yields the
//! same event sequence as the unsplit input and satisfies the structural
//! invariants. Every failure names the split offsets that produced it.

use super::transform::inject_openai_stream_include_usage;
use super::transform_codex_chat::{build_codex_tool_context_from_request, CodexToolContext};
use super::transform_gemini::{extract_anthropic_tool_schema_hints, AnthropicToolSchemaHints};
use crate::pricing::engine::{self, InputBasis, TokenCounts};
use crate::proxy::codec::bridge;
pub use crate::proxy::codec::CodecEngine;
use crate::proxy::sse::{strip_sse_field, take_sse_block};
use crate::proxy::usage::calculator::{CostCalculator, ModelPricing};
use crate::proxy::usage::ir::UsageIR;
use crate::proxy::usage::parity::{check_usage_parity, raw_usage_block, UsageParityIssue};
use crate::proxy::usage::parser::TokenUsage;
use bytes::Bytes;
use futures::StreamExt;
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

/// Stream inputs up to this size are split at every byte offset.
pub const EXHAUSTIVE_SPLIT_LIMIT: usize = 4 * 1024;
/// Number of seeded random multi-split plans per stream input.
pub const RANDOM_SPLIT_PLANS: usize = 200;
/// Upper bound on the number of cut points in one random plan.
const RANDOM_SPLIT_MAX_CUTS: usize = 24;
/// At most this many split failures are listed per fixture.
const MAX_REPORTED_FAILURES: usize = 8;

/// Event name used for an `Err` item yielded by a bridge stream.
pub const STREAM_ERROR_EVENT: &str = "<stream-error>";

// ---------------------------------------------------------------------------
// Bridges
// ---------------------------------------------------------------------------

/// Protocol bridge under test, named `<client protocol>__<upstream protocol>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Bridge {
    /// Anthropic Messages client -> OpenAI Chat Completions upstream.
    AnthropicOpenAiChat,
    /// Anthropic Messages client -> Gemini native upstream.
    AnthropicGemini,
    /// Anthropic Messages client -> OpenAI Responses upstream.
    AnthropicResponses,
    /// OpenAI Responses (Codex) client -> OpenAI Chat Completions upstream.
    ResponsesChat,
    /// OpenAI Responses (Codex) client -> Anthropic Messages upstream.
    ResponsesAnthropic,
    /// Gemini native client (Gemini CLI) -> Anthropic Messages upstream.
    GeminiAnthropic,
}

/// Wire protocol spoken on one side of a bridge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    Anthropic,
    OpenAiChat,
    Gemini,
    Responses,
}

impl Protocol {
    /// Whether the protocol's input token count already includes cache reads
    /// and writes (OpenAI, Gemini) or excludes them (Anthropic).
    fn input_includes_cache(self) -> bool {
        !matches!(self, Protocol::Anthropic)
    }
}

impl Bridge {
    pub const ALL: [Bridge; 6] = [
        Bridge::AnthropicOpenAiChat,
        Bridge::AnthropicGemini,
        Bridge::AnthropicResponses,
        Bridge::ResponsesChat,
        Bridge::ResponsesAnthropic,
        Bridge::GeminiAnthropic,
    ];

    pub fn dir_name(self) -> &'static str {
        match self {
            Bridge::AnthropicOpenAiChat => "anthropic__openai_chat",
            Bridge::AnthropicGemini => "anthropic__gemini",
            Bridge::AnthropicResponses => "anthropic__responses",
            Bridge::ResponsesChat => "responses__chat",
            Bridge::ResponsesAnthropic => "responses__anthropic",
            Bridge::GeminiAnthropic => "gemini__anthropic",
        }
    }

    /// Protocol of the client side (requests in, responses out).
    pub fn client(self) -> Protocol {
        match self {
            Bridge::AnthropicOpenAiChat | Bridge::AnthropicGemini | Bridge::AnthropicResponses => {
                Protocol::Anthropic
            }
            Bridge::ResponsesChat | Bridge::ResponsesAnthropic => Protocol::Responses,
            Bridge::GeminiAnthropic => Protocol::Gemini,
        }
    }

    /// Protocol of the upstream side.
    pub fn upstream(self) -> Protocol {
        match self {
            Bridge::AnthropicOpenAiChat | Bridge::ResponsesChat => Protocol::OpenAiChat,
            Bridge::AnthropicGemini => Protocol::Gemini,
            Bridge::AnthropicResponses => Protocol::Responses,
            Bridge::ResponsesAnthropic | Bridge::GeminiAnthropic => Protocol::Anthropic,
        }
    }

    pub fn from_dir_name(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|bridge| bridge.dir_name() == name)
    }
}

// ---------------------------------------------------------------------------
// Fixture metadata
// ---------------------------------------------------------------------------

/// Contents of `meta.toml`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaseMeta {
    /// What behaviour the case pins, in one or two sentences.
    pub intent: String,
    /// Commit whose behaviour the snapshots record (or that introduced it).
    pub source_commit: String,
    /// Inline test the case was ported from, if any.
    #[serde(default)]
    pub ported_from: Option<String>,
    #[serde(default)]
    pub options: CaseOptions,
    #[serde(default)]
    pub invariants: InvariantConfig,
    #[serde(default)]
    pub parity: ParityConfig,
    #[serde(default)]
    pub usage_parity: UsageParityConfig,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaseOptions {
    /// Chat bridge: keep `reasoning_content` on replayed assistant turns
    /// (the proxy enables this for reasoning vendors such as DeepSeek/Kimi).
    #[serde(default)]
    pub preserve_reasoning_content: bool,
    /// Append an upstream transport error with this message after the SSE body.
    #[serde(default)]
    pub upstream_error: Option<String>,
    /// Responses bridge: the upstream is the Codex OAuth backend (forces
    /// `store: false` and requests encrypted reasoning).
    #[serde(default)]
    pub codex_oauth: bool,
    /// Gemini client: model name from the request URL (the body has none).
    /// Defaults to [`GEMINI_CLIENT_DEFAULT_MODEL`].
    #[serde(default)]
    pub client_model: Option<String>,
    /// Gemini client: whether the URL is `:streamGenerateContent`. Defaults to
    /// whether the fixture ships an `upstream_response.sse`.
    #[serde(default)]
    pub client_stream: Option<bool>,
}

/// Model a Gemini client fixture requests when `client_model` is not set.
pub const GEMINI_CLIENT_DEFAULT_MODEL: &str = "claude-sonnet-4-5";

/// How the client stream is expected to end.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Terminal {
    /// Exactly one completion event as the last event: `message_stop` for
    /// Anthropic clients, `response.completed` / `response.incomplete` for
    /// Responses clients (also accepted as `completed`).
    #[default]
    #[serde(alias = "completed")]
    MessageStop,
    /// Exactly one `error` event (or stream `Err` item), as the last event;
    /// `response.failed` for Responses clients.
    Error,
    /// No terminal event at all (truncated upstream).
    None,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InvariantConfig {
    #[serde(default)]
    pub terminal: Terminal,
    /// Invariant names to skip for this case (see [`Invariant`]).
    #[serde(default)]
    pub skip: Vec<String>,
    /// Required when `skip` is non-empty: why the invariant does not hold.
    #[serde(default)]
    pub skip_reason: Option<String>,
}

/// Stream vs non-stream comparison, for cases that ship both upstream bodies.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParityConfig {
    /// Known differences between the stream and non-stream conversions, one
    /// line per field as produced by [`parity_diff`]. Empty means "identical".
    #[serde(default)]
    pub expected_diff: Vec<String>,
}

/// Usage parity against the raw upstream usage block (see
/// [`check_usage_parity_case`]).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UsageParityConfig {
    /// The parity flag the raw upstream usage is known to raise (an
    /// upstream whose own usage block is inconsistent). The checker then
    /// requires exactly this flag instead of no flag.
    #[serde(default)]
    pub expected_issue: Option<UsageParityIssue>,
    /// Required with `expected_issue`: why the upstream usage is inconsistent.
    #[serde(default)]
    pub reason: Option<String>,
}

/// Structural invariants checked on every client output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Invariant {
    /// SSE `event:` name equals the payload `type`.
    EventNames,
    /// At most one `message_start`, and it is the first event.
    MessageStart,
    /// Every content block is started once, receives deltas only while open
    /// and is stopped once before the message ends.
    BlockPairing,
    /// Content block indices are assigned 0, 1, 2, ... in emission order.
    IndexOrder,
    /// `tool_use` blocks carry a non-empty, unique id and a name.
    ToolIds,
    /// Concatenated `input_json_delta` fragments parse as JSON.
    ToolInputJson,
    /// Thinking blocks come before any text or tool_use block.
    ReasoningFirst,
    /// Exactly one terminal event, matching `meta.toml`, and nothing after it.
    Terminal,
    /// Client usage accounts for exactly the upstream tokens.
    UsageConservation,
}

impl Invariant {
    pub const ALL: [Invariant; 9] = [
        Invariant::EventNames,
        Invariant::MessageStart,
        Invariant::BlockPairing,
        Invariant::IndexOrder,
        Invariant::ToolIds,
        Invariant::ToolInputJson,
        Invariant::ReasoningFirst,
        Invariant::Terminal,
        Invariant::UsageConservation,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Invariant::EventNames => "event_names",
            Invariant::MessageStart => "message_start",
            Invariant::BlockPairing => "block_pairing",
            Invariant::IndexOrder => "index_order",
            Invariant::ToolIds => "tool_ids",
            Invariant::ToolInputJson => "tool_input_json",
            Invariant::ReasoningFirst => "reasoning_first",
            Invariant::Terminal => "terminal",
            Invariant::UsageConservation => "usage_conservation",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|inv| inv.name() == name)
    }
}

// ---------------------------------------------------------------------------
// Fixture loading
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Fixture {
    pub bridge: Bridge,
    pub name: String,
    pub dir: PathBuf,
    pub meta: CaseMeta,
    pub client_request: Option<Value>,
    pub upstream_sse: Option<Vec<u8>>,
    pub upstream_json: Option<Value>,
}

impl Fixture {
    /// `<bridge>/<case>`, used in every report.
    pub fn id(&self) -> String {
        format!("{}/{}", self.bridge.dir_name(), self.name)
    }

    pub fn has_stream(&self) -> bool {
        self.upstream_sse.is_some()
    }

    pub fn has_non_stream(&self) -> bool {
        self.upstream_json.is_some()
    }

    fn skipped(&self) -> HashSet<Invariant> {
        self.meta
            .invariants
            .skip
            .iter()
            .filter_map(|name| Invariant::from_name(name))
            .collect()
    }

    /// Tool schema hints the proxy derives from the client request.
    fn tool_schema_hints(&self) -> Option<AnthropicToolSchemaHints> {
        let hints = extract_anthropic_tool_schema_hints(self.client_request.as_ref()?);
        (!hints.is_empty()).then_some(hints)
    }

    /// Codex tool context (custom / namespace / tool_search tools) the proxy
    /// derives from the Responses client request.
    fn codex_tool_context(&self) -> CodexToolContext {
        self.client_request
            .as_ref()
            .map(build_codex_tool_context_from_request)
            .unwrap_or_default()
    }
}

/// Load every fixture of `bridge` under `root` (the `tests/fixtures/proxy` dir),
/// sorted by case name.
pub fn load_fixtures(root: &Path, bridge: Bridge) -> Result<Vec<Fixture>, String> {
    let bridge_dir = root.join(bridge.dir_name());
    let entries =
        fs::read_dir(&bridge_dir).map_err(|e| format!("read {}: {e}", bridge_dir.display()))?;
    let mut dirs: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.is_dir())
        .collect();
    dirs.sort();
    dirs.iter().map(|dir| load_fixture(bridge, dir)).collect()
}

pub fn load_fixture(bridge: Bridge, dir: &Path) -> Result<Fixture, String> {
    let name = dir
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| format!("bad fixture dir {}", dir.display()))?
        .to_string();
    let id = format!("{}/{name}", bridge.dir_name());

    let meta_text = fs::read_to_string(dir.join("meta.toml"))
        .map_err(|e| format!("{id}: read meta.toml: {e}"))?;
    let meta: CaseMeta =
        toml::from_str(&meta_text).map_err(|e| format!("{id}: parse meta.toml: {e}"))?;
    for skipped in &meta.invariants.skip {
        if Invariant::from_name(skipped).is_none() {
            return Err(format!("{id}: unknown invariant `{skipped}` in skip"));
        }
    }
    if !meta.invariants.skip.is_empty() && meta.invariants.skip_reason.is_none() {
        return Err(format!("{id}: invariants.skip requires skip_reason"));
    }
    if meta.usage_parity.expected_issue.is_some() && meta.usage_parity.reason.is_none() {
        return Err(format!(
            "{id}: usage_parity.expected_issue requires a reason"
        ));
    }

    let read_json = |file: &str| -> Result<Option<Value>, String> {
        let path = dir.join(file);
        if !path.exists() {
            return Ok(None);
        }
        let text = fs::read_to_string(&path).map_err(|e| format!("{id}: read {file}: {e}"))?;
        serde_json::from_str(&text)
            .map(Some)
            .map_err(|e| format!("{id}: parse {file}: {e}"))
    };
    let client_request = read_json("client_request.json")?;
    let upstream_json = read_json("upstream_response.json")?;
    let sse_path = dir.join("upstream_response.sse");
    let upstream_sse = if sse_path.exists() {
        Some(fs::read(&sse_path).map_err(|e| format!("{id}: read upstream_response.sse: {e}"))?)
    } else {
        None
    };

    if client_request.is_none() && upstream_json.is_none() && upstream_sse.is_none() {
        return Err(format!("{id}: fixture has no input files"));
    }
    if !meta.parity.expected_diff.is_empty() && (upstream_json.is_none() || upstream_sse.is_none())
    {
        return Err(format!(
            "{id}: parity.expected_diff needs both upstream_response.sse and .json"
        ));
    }

    Ok(Fixture {
        bridge,
        name,
        dir: dir.to_path_buf(),
        meta,
        client_request,
        upstream_sse,
        upstream_json,
    })
}

// ---------------------------------------------------------------------------
// Running the bridges
// ---------------------------------------------------------------------------

/// One parsed client SSE event (or a stream `Err` item, see [`STREAM_ERROR_EVENT`]).
#[derive(Debug, Clone, PartialEq)]
pub struct ClientEvent {
    pub event: Option<String>,
    pub data: Value,
}

impl ClientEvent {
    pub fn kind(&self) -> &str {
        self.data
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default()
    }

    fn is_terminal(&self) -> bool {
        self.is_completion()
            || matches!(self.kind(), "error" | "response.failed")
            || self.is_gemini_error()
            || self.event.as_deref() == Some(STREAM_ERROR_EVENT)
    }

    /// Successful end of the message (possibly truncated by a token limit);
    /// for Gemini clients a frame carrying a `finishReason`.
    fn is_completion(&self) -> bool {
        matches!(
            self.kind(),
            "message_stop" | "response.completed" | "response.incomplete"
        ) || gemini_finish_reason(&self.data).is_some()
    }

    /// Gemini error frame: `{"error": {...}}` without a `type`.
    fn is_gemini_error(&self) -> bool {
        self.kind().is_empty() && self.data.get("error").is_some()
    }
}

/// Convert the client request into the upstream request, as the proxy does
/// with the default (legacy) conversion engine.
pub fn convert_request(fixture: &Fixture) -> Option<Result<Value, String>> {
    convert_request_with(fixture, CodecEngine::Legacy)
}

/// [`convert_request`] with an explicit conversion engine. Bridges that are
/// not migrated to codecs ignore `engine`.
pub fn convert_request_with(
    fixture: &Fixture,
    engine: CodecEngine,
) -> Option<Result<Value, String>> {
    let body = fixture.client_request.clone()?;
    let result = match fixture.bridge {
        Bridge::AnthropicOpenAiChat => bridge::anthropic_to_openai_chat_request(
            body,
            fixture.meta.options.preserve_reasoning_content,
            engine,
        )
        .map(|mut converted| {
            inject_openai_stream_include_usage(&mut converted);
            converted
        }),
        // No shadow store: fixtures carry their full history.
        Bridge::AnthropicGemini => {
            bridge::anthropic_to_gemini_request(body, None, None, None, engine)
        }
        // No prompt_cache_key and no fast mode: both depend on provider config.
        Bridge::AnthropicResponses => bridge::anthropic_to_responses_request(
            body,
            crate::proxy::codec::responses::ResponsesUpstreamOptions {
                codex_oauth: fixture.meta.options.codex_oauth,
                ..Default::default()
            },
            engine,
        ),
        // The proxy first restores cached tool calls from the session history
        // (`codex_chat_history`) and applies provider reasoning config; fixtures
        // carry their full history and run without provider config.
        Bridge::ResponsesChat => bridge::responses_to_chat_request(body, None, engine),
        // Same default max_tokens as the forwarder. The forwarder's later cache
        // breakpoint injection and Claude Code system prompt are provider config.
        Bridge::ResponsesAnthropic => {
            bridge::responses_to_anthropic_request(body, CODEX_ANTHROPIC_DEFAULT_MAX_TOKENS, engine)
        }
        // A new combination without a legacy converter: always the IR codecs.
        Bridge::GeminiAnthropic => {
            let options = &fixture.meta.options;
            bridge::gemini_client_to_anthropic_request(
                body,
                options
                    .client_model
                    .as_deref()
                    .unwrap_or(GEMINI_CLIENT_DEFAULT_MODEL),
                options.client_stream.unwrap_or(fixture.has_stream()),
                CODEX_ANTHROPIC_DEFAULT_MAX_TOKENS,
            )
        }
    };
    Some(result.map_err(|e| e.to_string()))
}

/// `DEFAULT_CODEX_ANTHROPIC_MAX_TOKENS` in the forwarder (also the Gemini ->
/// Anthropic default).
const CODEX_ANTHROPIC_DEFAULT_MAX_TOKENS: u64 = 8192;

/// Convert the non-stream upstream body into the client response.
pub fn convert_non_stream(fixture: &Fixture) -> Option<Result<Value, String>> {
    convert_non_stream_with(fixture, CodecEngine::Legacy)
}

/// [`convert_non_stream`] with an explicit conversion engine.
pub fn convert_non_stream_with(
    fixture: &Fixture,
    engine: CodecEngine,
) -> Option<Result<Value, String>> {
    let body = fixture.upstream_json.clone()?;
    let result = match fixture.bridge {
        Bridge::AnthropicOpenAiChat => bridge::openai_chat_to_anthropic_response(body, engine),
        Bridge::AnthropicGemini => bridge::gemini_to_anthropic_response(
            body,
            None,
            fixture.tool_schema_hints().as_ref(),
            engine,
        ),
        Bridge::AnthropicResponses => bridge::responses_to_anthropic_response(body, engine),
        Bridge::ResponsesChat => {
            bridge::chat_to_responses_response(body, &fixture.codex_tool_context(), engine)
        }
        Bridge::ResponsesAnthropic => {
            bridge::anthropic_to_responses_response(body, &fixture.codex_tool_context(), engine)
        }
        Bridge::GeminiAnthropic => bridge::anthropic_to_gemini_client_response(body),
    };
    Some(result.map_err(|e| e.to_string()))
}

/// Feed `chunks` through the bridge's stream converter and parse the output.
pub fn run_stream(fixture: &Fixture, chunks: &[&[u8]]) -> Result<Vec<ClientEvent>, String> {
    run_stream_with(fixture, chunks, CodecEngine::Legacy)
}

/// [`run_stream`] with an explicit conversion engine.
pub fn run_stream_with(
    fixture: &Fixture,
    chunks: &[&[u8]],
    engine: CodecEngine,
) -> Result<Vec<ClientEvent>, String> {
    parse_client_stream(stream_output_with(fixture, chunks, engine))
}

/// Raw items the bridge's stream converter yields for `chunks`, before parsing.
pub fn stream_output_with(
    fixture: &Fixture,
    chunks: &[&[u8]],
    engine: CodecEngine,
) -> Vec<Result<Bytes, std::io::Error>> {
    let mut items: Vec<Result<Bytes, std::io::Error>> = chunks
        .iter()
        .map(|chunk| Ok(Bytes::copy_from_slice(chunk)))
        .collect();
    if let Some(message) = &fixture.meta.options.upstream_error {
        items.push(Err(std::io::Error::other(message.clone())));
    }
    let upstream = futures::stream::iter(items);

    match fixture.bridge {
        Bridge::AnthropicOpenAiChat => futures::executor::block_on(
            bridge::openai_chat_to_anthropic_sse(upstream, engine).collect(),
        ),
        Bridge::AnthropicGemini => futures::executor::block_on(
            bridge::gemini_to_anthropic_sse(upstream, None, fixture.tool_schema_hints(), engine)
                .collect(),
        ),
        Bridge::AnthropicResponses => futures::executor::block_on(
            bridge::responses_to_anthropic_sse(upstream, engine).collect(),
        ),
        Bridge::ResponsesChat => futures::executor::block_on(
            bridge::chat_to_responses_sse(upstream, fixture.codex_tool_context(), engine).collect(),
        ),
        Bridge::ResponsesAnthropic => futures::executor::block_on(
            bridge::anthropic_to_responses_sse(upstream, fixture.codex_tool_context(), engine)
                .collect(),
        ),
        Bridge::GeminiAnthropic => {
            futures::executor::block_on(bridge::anthropic_to_gemini_client_sse(upstream).collect())
        }
    }
}

/// Parse bridge output items into events. Output must be valid UTF-8 and made
/// of complete SSE blocks.
pub fn parse_client_stream(
    items: Vec<Result<Bytes, std::io::Error>>,
) -> Result<Vec<ClientEvent>, String> {
    let mut text = Vec::new();
    let mut events = Vec::new();
    let flush = |text: &mut Vec<u8>, events: &mut Vec<ClientEvent>| -> Result<(), String> {
        let mut buffer = String::from_utf8(std::mem::take(text))
            .map_err(|e| format!("client stream is not valid UTF-8: {e}"))?;
        while let Some(block) = take_sse_block(&mut buffer) {
            if block.trim().is_empty() {
                continue;
            }
            let mut event = None;
            let mut data = Vec::new();
            for line in block.lines() {
                if let Some(name) = strip_sse_field(line, "event") {
                    event = Some(name.to_string());
                } else if let Some(payload) = strip_sse_field(line, "data") {
                    data.push(payload);
                }
            }
            let data = data.join("\n");
            let data: Value = serde_json::from_str(&data)
                .map_err(|e| format!("client SSE data is not JSON ({e}): {data}"))?;
            events.push(ClientEvent { event, data });
        }
        if !buffer.trim().is_empty() {
            return Err(format!(
                "client stream has an unterminated SSE block: {buffer:?}"
            ));
        }
        Ok(())
    };
    for item in items {
        match item {
            Ok(bytes) => text.extend_from_slice(&bytes),
            Err(error) => {
                flush(&mut text, &mut events)?;
                events.push(ClientEvent {
                    event: Some(STREAM_ERROR_EVENT.to_string()),
                    data: json!({ "type": STREAM_ERROR_EVENT, "message": error.to_string() }),
                });
            }
        }
    }
    flush(&mut text, &mut events)?;
    Ok(events)
}

/// Data payloads of the upstream SSE body (non-JSON payloads such as `[DONE]`
/// are skipped).
fn upstream_sse_payloads(input: &[u8]) -> Vec<Value> {
    let mut buffer = String::from_utf8_lossy(input).into_owned();
    buffer.push_str("\n\n");
    let mut payloads = Vec::new();
    while let Some(block) = take_sse_block(&mut buffer) {
        let data: Vec<&str> = block
            .lines()
            .filter_map(|line| strip_sse_field(line, "data"))
            .collect();
        if let Ok(value) = serde_json::from_str::<Value>(&data.join("\n")) {
            payloads.push(value);
        }
    }
    payloads
}

// ---------------------------------------------------------------------------
// Usage
// ---------------------------------------------------------------------------

fn usage_to_value(usage: Option<TokenUsage>) -> Value {
    match usage {
        Some(usage) => json!({
            "input_tokens": usage.input_tokens,
            "output_tokens": usage.output_tokens,
            "cache_read_tokens": usage.cache_read_tokens,
            "cache_creation_tokens": usage.cache_creation_tokens,
            "model": usage.model,
            "message_id": usage.message_id,
            "billable": usage.has_billable_tokens(),
        }),
        None => Value::Null,
    }
}

/// Usage the proxy records for a converted client stream.
pub fn client_stream_usage(client: Protocol, events: &[ClientEvent]) -> Value {
    usage_to_value(recorded_stream_usage(client, events))
}

/// Usage the proxy records for a converted non-stream client response.
pub fn client_response_usage(client: Protocol, response: &Value) -> Value {
    usage_to_value(recorded_response_usage(client, response))
}

fn recorded_stream_usage(client: Protocol, events: &[ClientEvent]) -> Option<TokenUsage> {
    let data: Vec<Value> = events.iter().map(|event| event.data.clone()).collect();
    match client {
        Protocol::Responses => TokenUsage::from_codex_stream_events_auto(&data),
        Protocol::Gemini => TokenUsage::from_gemini_stream_chunks(&data),
        _ => TokenUsage::from_claude_stream_events(&data),
    }
}

fn recorded_response_usage(client: Protocol, response: &Value) -> Option<TokenUsage> {
    match client {
        Protocol::Responses => TokenUsage::from_codex_response_auto(response),
        Protocol::Gemini => TokenUsage::from_gemini_response(response),
        _ => TokenUsage::from_claude_response(response),
    }
}

/// Usage as reported by the upstream itself, parsed in its native format.
fn upstream_stream_usage(bridge: Bridge, input: &[u8]) -> Option<TokenUsage> {
    let payloads = upstream_sse_payloads(input);
    if payloads.is_empty() {
        // Some gateways answer a stream request with one whole JSON body.
        if let Ok(body) = serde_json::from_slice::<Value>(input) {
            return upstream_response_usage(bridge, &body);
        }
    }
    match bridge.upstream() {
        Protocol::OpenAiChat => TokenUsage::from_openai_stream_events(&payloads),
        Protocol::Gemini => TokenUsage::from_gemini_stream_chunks(&payloads),
        Protocol::Anthropic => TokenUsage::from_claude_stream_events(&payloads),
        // The usage of the last response object that carries one (completed,
        // incomplete or failed), counted as reported (input includes cache).
        Protocol::Responses => payloads.iter().rev().find_map(|payload| {
            let response = payload.get("response").unwrap_or(payload);
            response.get("usage")?;
            TokenUsage::from_codex_response(response)
        }),
    }
}

fn upstream_response_usage(bridge: Bridge, body: &Value) -> Option<TokenUsage> {
    match bridge.upstream() {
        Protocol::OpenAiChat => TokenUsage::from_openai_response(body),
        Protocol::Gemini => TokenUsage::from_gemini_response(body),
        Protocol::Anthropic => TokenUsage::from_claude_response(body),
        Protocol::Responses => TokenUsage::from_codex_response(body),
    }
}

/// OpenAI, Gemini and Responses count cache tokens inside the prompt total,
/// while Anthropic splits them out. Whatever the two sides' conventions, the
/// client must account for the same prompt total, output, cache read and
/// cache creation tokens as the upstream reported.
fn check_usage_conservation(
    bridge: Bridge,
    upstream: Option<&TokenUsage>,
    client: &Value,
) -> Result<(), String> {
    let Some(upstream) = upstream else {
        let billable = client.get("billable").and_then(Value::as_bool);
        return match billable {
            None | Some(false) => Ok(()),
            Some(true) => Err(format!(
                "upstream reported no usage but client usage is billable: {client}"
            )),
        };
    };
    let field = |name: &str| client.get(name).and_then(Value::as_u64).unwrap_or(0);
    let client_prompt = if bridge.client().input_includes_cache() {
        field("input_tokens")
    } else {
        field("input_tokens") + field("cache_read_tokens") + field("cache_creation_tokens")
    };
    let upstream_prompt = if bridge.upstream().input_includes_cache() {
        u64::from(upstream.input_tokens)
    } else {
        u64::from(upstream.input_tokens)
            + u64::from(upstream.cache_read_tokens)
            + u64::from(upstream.cache_creation_tokens)
    };
    let mut problems = Vec::new();
    if client_prompt != upstream_prompt {
        problems.push(format!(
            "client prompt total={client_prompt} but upstream prompt total={upstream_prompt}"
        ));
    }
    for (name, upstream_value) in [
        ("output_tokens", upstream.output_tokens),
        ("cache_read_tokens", upstream.cache_read_tokens),
        ("cache_creation_tokens", upstream.cache_creation_tokens),
    ] {
        if field(name) != u64::from(upstream_value) {
            problems.push(format!(
                "{name}={} but upstream {name}={upstream_value}",
                field(name)
            ));
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems.join("; "))
    }
}

// ---------------------------------------------------------------------------
// Usage parity (CCS-186)
// ---------------------------------------------------------------------------

/// Per-million prices of the usage parity cost table. Every bucket has its
/// own price, so a token billed in the wrong bucket changes the total.
const PARITY_PRICING: [&str; 4] = ["3", "15", "0.3", "3.75"];

/// Usage parity results for the fixtures of a corpus.
#[derive(Debug, Default)]
pub struct UsageParityOutcome {
    /// Upstream bodies whose raw usage block was checked.
    pub checked: usize,
    /// One line per mismatch, prefixed with the fixture id and body kind.
    pub failures: Vec<String>,
}

/// The raw usage block of a non-stream upstream body, as the upstream sent it.
fn raw_response_usage_block(body: &Value) -> Option<Value> {
    raw_usage_block(body).cloned()
}

/// The raw usage block of a stream upstream body: the last block the proxy
/// would bill from (last Chat usage chunk, last Gemini `usageMetadata`, last
/// Responses object with usage). Anthropic splits usage across
/// `message_start` and `message_delta`, so their fields are merged with the
/// later value winning.
fn raw_stream_usage_block(protocol: Protocol, input: &[u8]) -> Option<Value> {
    let payloads = upstream_sse_payloads(input);
    if payloads.is_empty() {
        // Some gateways answer a stream request with one whole JSON body.
        let body = serde_json::from_slice::<Value>(input).ok()?;
        return raw_response_usage_block(&body);
    }
    if protocol != Protocol::Anthropic {
        return payloads
            .iter()
            .rev()
            .find_map(|payload| raw_usage_block(payload).cloned());
    }
    let mut merged = Map::new();
    for payload in &payloads {
        let usage = match payload.get("type").and_then(Value::as_str) {
            Some("message_start") => payload.pointer("/message/usage"),
            Some("message_delta") => payload.get("usage"),
            _ => None,
        };
        if let Some(Value::Object(fields)) = usage {
            for (key, value) in fields {
                if !value.is_null() {
                    merged.insert(key.clone(), value.clone());
                }
            }
        }
    }
    (!merged.is_empty()).then_some(Value::Object(merged))
}

/// The upstream's raw usage block read into the canonical IR.
fn upstream_usage_ir(protocol: Protocol, raw: &Value) -> UsageIR {
    match protocol {
        Protocol::Anthropic => UsageIR::from_anthropic(raw),
        Protocol::OpenAiChat => UsageIR::from_chat_completions(raw),
        Protocol::Gemini => UsageIR::from_gemini(raw),
        Protocol::Responses => UsageIR::from_responses(raw),
    }
}

/// App type whose input semantics match how `protocol` counts input tokens.
fn parity_app_type(protocol: Protocol) -> &'static str {
    match protocol {
        Protocol::Anthropic => "claude",
        Protocol::Gemini => "gemini",
        Protocol::OpenAiChat | Protocol::Responses => "codex",
    }
}

/// Compare one upstream body's usage across every path that bills it.
///
/// - `raw` is the upstream's raw usage block, `recorded` the usage the proxy
///   logs for the converted client output.
/// - The UsageIR-normalized usage and the recorded usage must both re-sum to
///   the raw block's own totals ([`check_usage_parity`]); an all-zero usage is
///   fine only when the raw block is all zero too. With `expected_issue` both
///   must raise exactly that flag instead.
/// - The recorded usage must hold the same prompt total, cache read, cache
///   write and output buckets as the IR.
/// - Cost table: the IR priced bucket by bucket must equal the pricing
///   engine's cost of the IR under every stored input semantics (fresh,
///   total, legacy cache-read-only), of the IR's usage under the upstream's
///   app type, and of the recorded usage under the client's app type.
///
/// A client stream that never completed (`complete == false`) is billed for
/// what reached the client: Anthropic usage takes the output from
/// `message_delta`, which an aborted stream never sends. Its recorded usage
/// only has to stay within the IR bucket by bucket; the IR checks still apply.
pub fn check_usage_parity_case(
    bridge: Bridge,
    raw: Option<&Value>,
    recorded: Option<&TokenUsage>,
    complete: bool,
    expected_issue: Option<UsageParityIssue>,
) -> Vec<String> {
    let mut problems = Vec::new();
    let Some(raw) = raw else {
        if recorded.is_some_and(TokenUsage::has_billable_tokens) {
            problems.push(format!(
                "upstream sent no usage block but the recorded usage is billable: {recorded:?}"
            ));
        }
        return problems;
    };

    let (client, upstream) = (bridge.client(), bridge.upstream());
    let ir = upstream_usage_ir(upstream, raw);
    let ir_usage = ir.to_token_usage();
    let recorded = recorded.cloned().unwrap_or_default();

    let mut paths = vec![("usage_ir", &ir_usage, upstream.input_includes_cache())];
    if complete {
        paths.push(("recorded", &recorded, client.input_includes_cache()));
    }
    for (path, usage, input_includes_cache) in paths {
        let parity = check_usage_parity(usage, input_includes_cache, Some(raw));
        if let Some(expected) = expected_issue {
            if parity.map(|parity| parity.issue) != Some(expected) {
                problems.push(format!(
                    "{path}: expected parity flag {expected:?} but got {parity:?}; raw usage {raw}"
                ));
            }
            continue;
        }
        match parity {
            None => {}
            Some(parity)
                if parity.issue == UsageParityIssue::ZeroUsage && parity.raw_total == Some(0) => {}
            Some(parity) => problems.push(format!(
                "{path}: {:?}, normalized total {} vs raw total {:?} (raw input {:?}, raw cache {:?}); raw usage {raw}",
                parity.issue,
                parity.normalized_total,
                parity.raw_total,
                parity.raw_input,
                parity.raw_cache
            )),
        }
    }

    let recorded_prompt = if client.input_includes_cache() {
        u64::from(recorded.input_tokens)
    } else {
        u64::from(recorded.input_tokens)
            + u64::from(recorded.cache_read_tokens)
            + u64::from(recorded.cache_creation_tokens)
    };
    for (bucket, recorded_value, ir_value) in [
        ("prompt total", recorded_prompt, ir.total_input()),
        (
            "cache_read",
            u64::from(recorded.cache_read_tokens),
            ir.cache_read,
        ),
        (
            "cache_creation",
            u64::from(recorded.cache_creation_tokens),
            ir.cache_write,
        ),
        ("output", u64::from(recorded.output_tokens), ir.output),
    ] {
        let mismatch = if complete {
            recorded_value != ir_value
        } else {
            recorded_value > ir_value
        };
        if mismatch {
            problems.push(format!(
                "recorded {bucket}={recorded_value} but UsageIR {bucket}={ir_value}"
            ));
        }
    }

    let [input, output, cache_read, cache_creation] = PARITY_PRICING;
    let pricing = ModelPricing::from_strings(input, output, cache_read, cache_creation)
        .expect("valid parity pricing");
    // Priced here from the price strings, independently of the engine.
    let price = |text: &str| Decimal::from_str(text).expect("valid parity price");
    let expected = (Decimal::from(ir.fresh_input) * price(input)
        + Decimal::from(ir.output) * price(output)
        + Decimal::from(ir.cache_read) * price(cache_read)
        + Decimal::from(ir.cache_write) * price(cache_creation))
        / Decimal::from(1_000_000u64);
    let counts = |input| TokenCounts {
        input,
        output: ir.output,
        cache_read: ir.cache_read,
        cache_creation: ir.cache_write,
    };
    let mut rows = vec![
        (
            "fresh".to_string(),
            engine::cost(
                &counts(ir.fresh_input),
                InputBasis::Fresh,
                &pricing,
                Decimal::ONE,
            ),
        ),
        (
            "total".to_string(),
            engine::cost(
                &counts(ir.total_input()),
                InputBasis::IncludesCache,
                &pricing,
                Decimal::ONE,
            ),
        ),
        (
            "legacy".to_string(),
            engine::cost(
                &counts(ir.fresh_input + ir.cache_read),
                InputBasis::IncludesCacheRead,
                &pricing,
                Decimal::ONE,
            ),
        ),
        (
            format!("usage_ir as {}", parity_app_type(upstream)),
            CostCalculator::calculate_for_app(
                parity_app_type(upstream),
                &ir_usage,
                &pricing,
                Decimal::ONE,
            ),
        ),
    ];
    if complete {
        rows.push((
            format!("recorded as {}", parity_app_type(client)),
            CostCalculator::calculate_for_app(
                parity_app_type(client),
                &recorded,
                &pricing,
                Decimal::ONE,
            ),
        ));
    }
    for (row, cost) in rows {
        if cost.total_cost != expected {
            problems.push(format!(
                "cost table row `{row}`: {} but UsageIR prices at {expected}",
                cost.total_cost
            ));
        }
    }
    problems
}

/// Run [`check_usage_parity_case`] on every upstream body of `fixture`: the
/// unsplit stream body and the non-stream body. A body whose conversion fails
/// is left to the golden tests, which already report it.
pub fn usage_parity_with(fixture: &Fixture, engine: CodecEngine) -> UsageParityOutcome {
    let mut outcome = UsageParityOutcome::default();
    let (bridge, client) = (fixture.bridge, fixture.bridge.client());
    let expected_issue = fixture.meta.usage_parity.expected_issue;
    let mut check = |kind: &str, raw: Option<Value>, recorded: Option<TokenUsage>, complete| {
        if raw.is_some() {
            outcome.checked += 1;
        }
        for problem in check_usage_parity_case(
            bridge,
            raw.as_ref(),
            recorded.as_ref(),
            complete,
            expected_issue,
        ) {
            outcome
                .failures
                .push(format!("{} ({kind}): {problem}", fixture.id()));
        }
    };

    if let Some(input) = &fixture.upstream_sse {
        if let Ok(events) = run_stream_with(fixture, &[input.as_slice()], engine) {
            check(
                "stream",
                raw_stream_usage_block(bridge.upstream(), input),
                recorded_stream_usage(client, &events),
                events.iter().any(ClientEvent::is_completion),
            );
        }
    }
    if let (Some(body), Some(Ok(response))) = (
        &fixture.upstream_json,
        convert_non_stream_with(fixture, engine),
    ) {
        check(
            "non-stream",
            raw_response_usage_block(body),
            recorded_response_usage(client, &response),
            true,
        );
    }
    outcome
}

// ---------------------------------------------------------------------------
// Redaction and rendering
// ---------------------------------------------------------------------------

/// Keys whose string values are identifiers.
const ID_KEYS: [&str; 7] = [
    "id",
    "tool_use_id",
    "tool_call_id",
    "responseId",
    "call_id",
    "item_id",
    "previous_response_id",
];
/// Keys whose values are timestamps.
const TIMESTAMP_KEYS: [&str; 4] = ["created", "created_at", "createTime", "timestamp"];

/// Replaces identifiers that the bridge generated (i.e. that appear nowhere in
/// the fixture inputs) with `[generated-id:N]`, numbered by first appearance so
/// equal ids stay equal. Ids copied from the inputs are kept verbatim so the
/// snapshots still show pass-through. Timestamps are always redacted.
#[derive(Debug, Clone, Default)]
pub struct Redactor {
    input_text: String,
    generated: HashMap<String, String>,
}

impl Redactor {
    pub fn new(fixture: &Fixture) -> Self {
        let mut input_text = String::new();
        if let Some(request) = &fixture.client_request {
            input_text.push_str(&request.to_string());
        }
        if let Some(body) = &fixture.upstream_json {
            input_text.push_str(&body.to_string());
        }
        if let Some(sse) = &fixture.upstream_sse {
            input_text.push_str(&String::from_utf8_lossy(sse));
        }
        Self {
            input_text,
            generated: HashMap::new(),
        }
    }

    fn redact_id(&mut self, id: &str) -> Option<String> {
        if id.is_empty() || (self.input_text.contains(&format!("\"{id}\""))) {
            return None;
        }
        let next = self.generated.len() + 1;
        Some(
            self.generated
                .entry(id.to_string())
                .or_insert_with(|| format!("[generated-id:{next}]"))
                .clone(),
        )
    }

    pub fn redact(&mut self, value: &mut Value) {
        match value {
            Value::Object(map) => {
                for (key, child) in map.iter_mut() {
                    if TIMESTAMP_KEYS.contains(&key.as_str()) && !child.is_null() {
                        *child = json!("[timestamp]");
                        continue;
                    }
                    if ID_KEYS.contains(&key.as_str()) {
                        if let Some(id) = child.as_str() {
                            if let Some(redacted) = self.redact_id(id) {
                                *child = Value::String(redacted);
                            }
                            continue;
                        }
                    }
                    self.redact(child);
                }
            }
            Value::Array(items) => items.iter_mut().for_each(|item| self.redact(item)),
            _ => {}
        }
    }

    pub fn redact_events(&mut self, events: &[ClientEvent]) -> Vec<ClientEvent> {
        events
            .iter()
            .map(|event| {
                let mut data = event.data.clone();
                self.redact(&mut data);
                ClientEvent {
                    event: event.event.clone(),
                    data,
                }
            })
            .collect()
    }
}

/// One line per event: `<event name> <compact JSON>`.
pub fn render_events(events: &[ClientEvent]) -> String {
    let mut out = String::new();
    for event in events {
        let name = event.event.as_deref().unwrap_or("<no-event-name>");
        let _ = writeln!(out, "{name} {}", event.data);
    }
    out
}

pub fn render_json(value: &Value) -> String {
    let mut text = serde_json::to_string_pretty(value).unwrap_or_default();
    text.push('\n');
    text
}

// ---------------------------------------------------------------------------
// Invariants
// ---------------------------------------------------------------------------

/// Check the structural invariants of a client event stream. Returns one
/// message per violation, prefixed with the invariant name.
pub fn check_stream_invariants(
    bridge: Bridge,
    events: &[ClientEvent],
    terminal: Terminal,
    skip: &HashSet<Invariant>,
    upstream_usage: Option<&TokenUsage>,
) -> Vec<String> {
    if bridge.client() == Protocol::Responses {
        return check_responses_stream_invariants(bridge, events, terminal, skip, upstream_usage);
    }
    if bridge.client() == Protocol::Gemini {
        return check_gemini_stream_invariants(bridge, events, terminal, skip, upstream_usage);
    }
    let mut violations: Vec<(Invariant, String)> = Vec::new();
    let mut fail = |inv: Invariant, message: String| violations.push((inv, message));
    let describe = |i: usize, event: &ClientEvent| format!("event #{i} ({})", event.data);

    // event_names
    for (i, event) in events.iter().enumerate() {
        if event.event.as_deref() != Some(event.kind()) {
            fail(
                Invariant::EventNames,
                format!(
                    "{} has event name {:?}",
                    describe(i, event),
                    event.event.as_deref()
                ),
            );
        }
    }

    // message_start
    let starts: Vec<usize> = events
        .iter()
        .enumerate()
        .filter(|(_, event)| event.kind() == "message_start")
        .map(|(i, _)| i)
        .collect();
    if starts.len() > 1 {
        fail(
            Invariant::MessageStart,
            format!("{} message_start events at {starts:?}", starts.len()),
        );
    }
    if let Some(&first) = starts.first() {
        if first != 0 {
            fail(
                Invariant::MessageStart,
                format!("message_start is event #{first}, not the first event"),
            );
        }
    } else if terminal == Terminal::MessageStop {
        fail(
            Invariant::MessageStart,
            "stream completes without message_start".to_string(),
        );
    }

    // block_pairing / index_order / tool_ids / tool_input_json / reasoning_first
    let mut open: HashMap<u64, (String, String)> = HashMap::new();
    let mut closed: HashSet<u64> = HashSet::new();
    let mut next_index = 0u64;
    let mut tool_ids: HashSet<String> = HashSet::new();
    let mut seen_non_thinking = false;
    for (i, event) in events.iter().enumerate() {
        let index = event.data.get("index").and_then(Value::as_u64);
        match event.kind() {
            "content_block_start" => {
                let Some(index) = index else {
                    fail(
                        Invariant::BlockPairing,
                        format!("{} has no index", describe(i, event)),
                    );
                    continue;
                };
                let block_type = event
                    .data
                    .pointer("/content_block/type")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                if open.contains_key(&index) || closed.contains(&index) {
                    fail(
                        Invariant::BlockPairing,
                        format!("{} restarts block {index}", describe(i, event)),
                    );
                }
                if index != next_index {
                    fail(
                        Invariant::IndexOrder,
                        format!(
                            "{} starts block {index}, expected {next_index}",
                            describe(i, event)
                        ),
                    );
                }
                next_index = next_index.max(index + 1);
                match block_type.as_str() {
                    "thinking" | "redacted_thinking" => {
                        if seen_non_thinking {
                            fail(
                                Invariant::ReasoningFirst,
                                format!("{} starts thinking after content", describe(i, event)),
                            );
                        }
                    }
                    _ => seen_non_thinking = true,
                }
                if block_type == "tool_use" {
                    let id = event
                        .data
                        .pointer("/content_block/id")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    let name = event
                        .data
                        .pointer("/content_block/name")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    if id.is_empty() || name.is_empty() {
                        fail(
                            Invariant::ToolIds,
                            format!("{} has empty tool id or name", describe(i, event)),
                        );
                    } else if !tool_ids.insert(id.to_string()) {
                        fail(
                            Invariant::ToolIds,
                            format!("{} reuses tool id {id}", describe(i, event)),
                        );
                    }
                }
                open.insert(index, (block_type, String::new()));
            }
            "content_block_delta" => match index.and_then(|index| open.get_mut(&index)) {
                Some((_, json_buf)) => {
                    if let Some(fragment) = event
                        .data
                        .pointer("/delta/partial_json")
                        .and_then(Value::as_str)
                    {
                        json_buf.push_str(fragment);
                    }
                }
                None => fail(
                    Invariant::BlockPairing,
                    format!("{} targets a block that is not open", describe(i, event)),
                ),
            },
            "content_block_stop" => {
                match index.and_then(|index| open.remove(&index).map(|b| (index, b))) {
                    Some((index, (block_type, json_buf))) => {
                        closed.insert(index);
                        if block_type == "tool_use"
                            && !json_buf.is_empty()
                            && serde_json::from_str::<Value>(&json_buf).is_err()
                        {
                            fail(
                                Invariant::ToolInputJson,
                                format!("block {index} input is not JSON: {json_buf:?}"),
                            );
                        }
                    }
                    None => fail(
                        Invariant::BlockPairing,
                        format!("{} stops a block that is not open", describe(i, event)),
                    ),
                }
            }
            "message_delta" | "message_stop" if !open.is_empty() => {
                let mut indices: Vec<u64> = open.keys().copied().collect();
                indices.sort_unstable();
                fail(
                    Invariant::BlockPairing,
                    format!(
                        "{} while blocks {indices:?} are still open",
                        describe(i, event)
                    ),
                );
                open.clear();
            }
            _ => {}
        }
    }

    // terminal
    let terminals: Vec<usize> = events
        .iter()
        .enumerate()
        .filter(|(_, event)| event.is_terminal())
        .map(|(i, _)| i)
        .collect();
    let deltas: Vec<usize> = events
        .iter()
        .enumerate()
        .filter(|(_, event)| event.kind() == "message_delta")
        .map(|(i, _)| i)
        .collect();
    match terminal {
        Terminal::None => {
            if !terminals.is_empty() {
                fail(
                    Invariant::Terminal,
                    format!("expected no terminal event, found them at {terminals:?}"),
                );
            }
        }
        Terminal::MessageStop | Terminal::Error => {
            if terminals.len() != 1 {
                fail(
                    Invariant::Terminal,
                    format!("expected exactly one terminal event, found {terminals:?}"),
                );
            } else {
                let at = terminals[0];
                let event = &events[at];
                let is_stop = event.is_completion();
                if is_stop != (terminal == Terminal::MessageStop) {
                    fail(
                        Invariant::Terminal,
                        format!(
                            "expected {terminal:?} terminal, got {}",
                            describe(at, event)
                        ),
                    );
                }
                if at + 1 != events.len() {
                    fail(
                        Invariant::Terminal,
                        format!(
                            "{} events follow the terminal event #{at}",
                            events.len() - at - 1
                        ),
                    );
                }
            }
        }
    }
    if deltas.len() > 1 {
        fail(
            Invariant::Terminal,
            format!("{} message_delta events at {deltas:?}", deltas.len()),
        );
    }
    if let Some(&delta) = deltas.first() {
        if events[delta + 1..].iter().any(|event| !event.is_terminal()) {
            fail(
                Invariant::Terminal,
                format!("non-terminal events follow message_delta #{delta}"),
            );
        }
    }

    // usage_conservation (only meaningful for a completed message)
    if terminal == Terminal::MessageStop {
        if let Err(problem) = check_usage_conservation(
            bridge,
            upstream_usage,
            &client_stream_usage(bridge.client(), events),
        ) {
            fail(Invariant::UsageConservation, problem);
        }
    }

    report_violations(violations, skip)
}

fn report_violations(
    violations: Vec<(Invariant, String)>,
    skip: &HashSet<Invariant>,
) -> Vec<String> {
    violations
        .into_iter()
        .filter(|(inv, _)| !skip.contains(inv))
        .map(|(inv, message)| format!("[{}] {message}", inv.name()))
        .collect()
}

/// Invariants for a non-stream client response.
pub fn check_response_invariants(
    bridge: Bridge,
    response: &Value,
    skip: &HashSet<Invariant>,
    upstream_usage: Option<&TokenUsage>,
) -> Vec<String> {
    if bridge.client() == Protocol::Responses {
        return check_responses_response_invariants(bridge, response, skip, upstream_usage);
    }
    if bridge.client() == Protocol::Gemini {
        return check_gemini_response_invariants(bridge, response, skip, upstream_usage);
    }
    let mut violations: Vec<(Invariant, String)> = Vec::new();
    let empty = Vec::new();
    let content = response
        .get("content")
        .and_then(Value::as_array)
        .unwrap_or(&empty);
    let mut tool_ids = HashSet::new();
    let mut seen_non_thinking = false;
    for (i, block) in content.iter().enumerate() {
        match block
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default()
        {
            "thinking" | "redacted_thinking" => {
                if seen_non_thinking {
                    violations.push((
                        Invariant::ReasoningFirst,
                        format!("content[{i}] is thinking after content"),
                    ));
                }
            }
            "tool_use" => {
                seen_non_thinking = true;
                let id = block.get("id").and_then(Value::as_str).unwrap_or_default();
                let name = block
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if id.is_empty() || name.is_empty() {
                    violations.push((
                        Invariant::ToolIds,
                        format!("content[{i}] has empty tool id or name"),
                    ));
                } else if !tool_ids.insert(id.to_string()) {
                    violations.push((Invariant::ToolIds, format!("content[{i}] reuses id {id}")));
                }
                if !block.get("input").is_some_and(Value::is_object) {
                    violations.push((
                        Invariant::ToolInputJson,
                        format!("content[{i}] input is not an object"),
                    ));
                }
            }
            _ => seen_non_thinking = true,
        }
    }
    if response.get("stop_reason").is_none_or(Value::is_null) {
        violations.push((
            Invariant::Terminal,
            "response has no stop_reason".to_string(),
        ));
    }
    if let Err(problem) = check_usage_conservation(
        bridge,
        upstream_usage,
        &client_response_usage(bridge.client(), response),
    ) {
        violations.push((Invariant::UsageConservation, problem));
    }
    report_violations(violations, skip)
}

/// Item types that carry a tool call the client must answer.
fn is_tool_call_item(item: &Value) -> bool {
    matches!(
        item.get("type").and_then(Value::as_str),
        Some("function_call" | "custom_tool_call")
    )
}

/// `tool_ids` / `tool_input_json` / `reasoning_first` checks shared by the
/// stream and non-stream Responses outputs. `items` are in output order.
fn check_responses_output_items(items: &[&Value], violations: &mut Vec<(Invariant, String)>) {
    let mut call_ids = HashSet::new();
    let mut seen_non_reasoning = false;
    for (i, item) in items.iter().enumerate() {
        let item_type = item.get("type").and_then(Value::as_str).unwrap_or_default();
        if item_type == "reasoning" {
            if seen_non_reasoning {
                violations.push((
                    Invariant::ReasoningFirst,
                    format!("output[{i}] is reasoning after content"),
                ));
            }
            continue;
        }
        seen_non_reasoning = true;
        if !is_tool_call_item(item) {
            continue;
        }
        let call_id = item
            .get("call_id")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let name = item.get("name").and_then(Value::as_str).unwrap_or_default();
        if call_id.is_empty() || name.is_empty() {
            violations.push((
                Invariant::ToolIds,
                format!("output[{i}] ({item}) has empty call_id or name"),
            ));
        } else if !call_ids.insert(call_id.to_string()) {
            violations.push((
                Invariant::ToolIds,
                format!("output[{i}] reuses call_id {call_id}"),
            ));
        }
        // A call cut off by the token limit (status incomplete) may carry
        // partial arguments; the client must not run it anyway.
        let incomplete = item.get("status").and_then(Value::as_str) == Some("incomplete");
        if item_type == "function_call" && !incomplete {
            let arguments = item.get("arguments").and_then(Value::as_str);
            if !arguments
                .and_then(|text| serde_json::from_str::<Value>(text).ok())
                .is_some_and(|value| value.is_object())
            {
                violations.push((
                    Invariant::ToolInputJson,
                    format!("output[{i}] arguments are not a JSON object: {arguments:?}"),
                ));
            }
        }
    }
}

/// Stream invariants for a Responses client:
/// - `event_names`: SSE event name equals the payload `type`;
/// - `message_start`: one `response.created`, as the first event;
/// - `block_pairing`: every output item is added once and done once, deltas
///   only target open items, all items are done before the response completes,
///   and the completed response's `output` equals the done items;
/// - `index_order`: output indices are assigned 0, 1, 2, ... as items are added;
/// - `tool_ids` / `tool_input_json` (except for `incomplete` calls) /
///   `reasoning_first` on the done items;
/// - `terminal`: exactly one terminal event (`response.completed` /
///   `response.incomplete`, or `response.failed` / stream error), last;
/// - `usage_conservation` against the upstream usage.
fn check_responses_stream_invariants(
    bridge: Bridge,
    events: &[ClientEvent],
    terminal: Terminal,
    skip: &HashSet<Invariant>,
    upstream_usage: Option<&TokenUsage>,
) -> Vec<String> {
    let mut violations: Vec<(Invariant, String)> = Vec::new();
    let describe = |i: usize, event: &ClientEvent| format!("event #{i} ({})", event.data);

    for (i, event) in events.iter().enumerate() {
        if event.event.as_deref() != Some(event.kind()) {
            violations.push((
                Invariant::EventNames,
                format!(
                    "{} has event name {:?}",
                    describe(i, event),
                    event.event.as_deref()
                ),
            ));
        }
    }

    let created: Vec<usize> = events
        .iter()
        .enumerate()
        .filter(|(_, event)| event.kind() == "response.created")
        .map(|(i, _)| i)
        .collect();
    if created.len() > 1 {
        violations.push((
            Invariant::MessageStart,
            format!("{} response.created events at {created:?}", created.len()),
        ));
    }
    match created.first() {
        Some(&0) => {}
        Some(&first) => violations.push((
            Invariant::MessageStart,
            format!("response.created is event #{first}, not the first event"),
        )),
        None if terminal == Terminal::MessageStop => violations.push((
            Invariant::MessageStart,
            "stream completes without response.created".to_string(),
        )),
        None => {}
    }

    let mut open: HashSet<u64> = HashSet::new();
    let mut done: Vec<(u64, Value)> = Vec::new();
    let mut next_index = 0u64;
    for (i, event) in events.iter().enumerate() {
        let index = event.data.get("output_index").and_then(Value::as_u64);
        let kind = event.kind();
        match kind {
            "response.output_item.added" => {
                let Some(index) = index else {
                    violations.push((
                        Invariant::BlockPairing,
                        format!("{} has no output_index", describe(i, event)),
                    ));
                    continue;
                };
                if open.contains(&index) || done.iter().any(|(at, _)| *at == index) {
                    violations.push((
                        Invariant::BlockPairing,
                        format!("{} re-adds output item {index}", describe(i, event)),
                    ));
                }
                if index != next_index {
                    violations.push((
                        Invariant::IndexOrder,
                        format!(
                            "{} adds output item {index}, expected {next_index}",
                            describe(i, event)
                        ),
                    ));
                }
                next_index = next_index.max(index + 1);
                open.insert(index);
            }
            "response.output_item.done" => match index {
                Some(index) if open.remove(&index) => {
                    done.push((
                        index,
                        event.data.get("item").cloned().unwrap_or(Value::Null),
                    ));
                }
                _ => violations.push((
                    Invariant::BlockPairing,
                    format!("{} finishes an item that is not open", describe(i, event)),
                )),
            },
            "response.completed" | "response.incomplete" | "response.failed" => {
                // Like an Anthropic `error`, a failed response may abandon open items.
                if kind != "response.failed" && !open.is_empty() {
                    let mut indices: Vec<u64> = open.iter().copied().collect();
                    indices.sort_unstable();
                    violations.push((
                        Invariant::BlockPairing,
                        format!(
                            "{} while output items {indices:?} are still open",
                            describe(i, event)
                        ),
                    ));
                    open.clear();
                }
                if kind != "response.failed" {
                    let mut finished = done.clone();
                    finished.sort_by_key(|(index, _)| *index);
                    let finished: Vec<Value> = finished.into_iter().map(|(_, item)| item).collect();
                    let output = event
                        .data
                        .pointer("/response/output")
                        .cloned()
                        .unwrap_or_else(|| json!([]));
                    if output != Value::Array(finished.clone()) {
                        violations.push((
                            Invariant::BlockPairing,
                            format!(
                                "{} output differs from the done items {}",
                                describe(i, event),
                                Value::Array(finished)
                            ),
                        ));
                    }
                }
            }
            _ if kind.starts_with("response.") && index.is_some() => {
                let index = index.unwrap_or_default();
                if !open.contains(&index) {
                    violations.push((
                        Invariant::BlockPairing,
                        format!("{} targets an item that is not open", describe(i, event)),
                    ));
                }
            }
            _ => {}
        }
    }
    let mut finished = done;
    finished.sort_by_key(|(index, _)| *index);
    let items: Vec<&Value> = finished.iter().map(|(_, item)| item).collect();
    check_responses_output_items(&items, &mut violations);

    let terminals: Vec<usize> = events
        .iter()
        .enumerate()
        .filter(|(_, event)| event.is_terminal())
        .map(|(i, _)| i)
        .collect();
    match terminal {
        Terminal::None => {
            if !terminals.is_empty() {
                violations.push((
                    Invariant::Terminal,
                    format!("expected no terminal event, found them at {terminals:?}"),
                ));
            }
        }
        Terminal::MessageStop | Terminal::Error => {
            if terminals.len() != 1 {
                violations.push((
                    Invariant::Terminal,
                    format!("expected exactly one terminal event, found {terminals:?}"),
                ));
            } else {
                let at = terminals[0];
                let event = &events[at];
                if event.is_completion() != (terminal == Terminal::MessageStop) {
                    violations.push((
                        Invariant::Terminal,
                        format!(
                            "expected {terminal:?} terminal, got {}",
                            describe(at, event)
                        ),
                    ));
                }
                if at + 1 != events.len() {
                    violations.push((
                        Invariant::Terminal,
                        format!(
                            "{} events follow the terminal event #{at}",
                            events.len() - at - 1
                        ),
                    ));
                }
            }
        }
    }

    if terminal == Terminal::MessageStop {
        if let Err(problem) = check_usage_conservation(
            bridge,
            upstream_usage,
            &client_stream_usage(bridge.client(), events),
        ) {
            violations.push((Invariant::UsageConservation, problem));
        }
    }

    report_violations(violations, skip)
}

/// Non-stream invariants for a Responses client response.
fn check_responses_response_invariants(
    bridge: Bridge,
    response: &Value,
    skip: &HashSet<Invariant>,
    upstream_usage: Option<&TokenUsage>,
) -> Vec<String> {
    let mut violations: Vec<(Invariant, String)> = Vec::new();
    let empty = Vec::new();
    let output = response
        .get("output")
        .and_then(Value::as_array)
        .unwrap_or(&empty);
    let items: Vec<&Value> = output.iter().collect();
    check_responses_output_items(&items, &mut violations);
    if response.get("status").and_then(Value::as_str).is_none() {
        violations.push((Invariant::Terminal, "response has no status".to_string()));
    }
    if let Err(problem) = check_usage_conservation(
        bridge,
        upstream_usage,
        &client_response_usage(bridge.client(), response),
    ) {
        violations.push((Invariant::UsageConservation, problem));
    }
    report_violations(violations, skip)
}

/// `finishReason` of the first candidate of a Gemini response or stream frame.
fn gemini_finish_reason(frame: &Value) -> Option<&str> {
    frame.pointer("/candidates/0/finishReason")?.as_str()
}

/// Content parts of the first candidate of a Gemini response or stream frame.
fn gemini_parts(frame: &Value) -> &[Value] {
    frame
        .pointer("/candidates/0/content/parts")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
}

/// `tool_ids` / `tool_input_json` / `reasoning_first` checks on Gemini parts in
/// output order (stream frames concatenated).
fn check_gemini_parts(parts: &[&Value], violations: &mut Vec<(Invariant, String)>) {
    let mut tool_ids = HashSet::new();
    let mut seen_non_thought = false;
    for (i, part) in parts.iter().enumerate() {
        if let Some(call) = part.get("functionCall") {
            seen_non_thought = true;
            let id = call.get("id").and_then(Value::as_str).unwrap_or_default();
            let name = call.get("name").and_then(Value::as_str).unwrap_or_default();
            if id.is_empty() || name.is_empty() {
                violations.push((
                    Invariant::ToolIds,
                    format!("part #{i} has an empty functionCall id or name"),
                ));
            } else if !tool_ids.insert(id.to_string()) {
                violations.push((Invariant::ToolIds, format!("part #{i} reuses id {id}")));
            }
            if !call.get("args").is_some_and(Value::is_object) {
                violations.push((
                    Invariant::ToolInputJson,
                    format!("part #{i} functionCall args is not an object"),
                ));
            }
        } else if part.get("thought").and_then(Value::as_bool) == Some(true) {
            if seen_non_thought {
                violations.push((
                    Invariant::ReasoningFirst,
                    format!("part #{i} is a thought after content"),
                ));
            }
        } else if part
            .get("text")
            .and_then(Value::as_str)
            .is_some_and(|text| !text.is_empty())
        {
            seen_non_thought = true;
        }
    }
}

/// Stream invariants for a Gemini client: data-only frames, one `finishReason`
/// on the last frame (or one error frame), valid tool calls and conserved usage.
fn check_gemini_stream_invariants(
    bridge: Bridge,
    events: &[ClientEvent],
    terminal: Terminal,
    skip: &HashSet<Invariant>,
    upstream_usage: Option<&TokenUsage>,
) -> Vec<String> {
    let mut violations: Vec<(Invariant, String)> = Vec::new();
    for (i, event) in events.iter().enumerate() {
        if event.event.is_some() && event.event.as_deref() != Some(STREAM_ERROR_EVENT) {
            violations.push((
                Invariant::EventNames,
                format!("frame #{i} has SSE event name {:?}", event.event),
            ));
        }
    }
    let parts: Vec<&Value> = events
        .iter()
        .flat_map(|event| gemini_parts(&event.data))
        .collect();
    check_gemini_parts(&parts, &mut violations);

    let terminals: Vec<usize> = events
        .iter()
        .enumerate()
        .filter(|(_, event)| event.is_terminal())
        .map(|(i, _)| i)
        .collect();
    match terminal {
        Terminal::None => {
            if !terminals.is_empty() {
                violations.push((
                    Invariant::Terminal,
                    format!("expected no terminal frame, found them at {terminals:?}"),
                ));
            }
        }
        Terminal::MessageStop | Terminal::Error => {
            let last = events.len().checked_sub(1);
            if terminals.len() != 1 || terminals.first().copied() != last {
                violations.push((
                    Invariant::Terminal,
                    format!(
                        "expected exactly one terminal frame as the last of {} frames, found {terminals:?}",
                        events.len()
                    ),
                ));
            } else if events[terminals[0]].is_completion() != (terminal == Terminal::MessageStop) {
                violations.push((
                    Invariant::Terminal,
                    format!(
                        "expected {terminal:?} terminal, got {}",
                        events[terminals[0]].data
                    ),
                ));
            }
        }
    }

    if terminal == Terminal::MessageStop {
        if let Err(problem) = check_usage_conservation(
            bridge,
            upstream_usage,
            &client_stream_usage(bridge.client(), events),
        ) {
            violations.push((Invariant::UsageConservation, problem));
        }
    }
    report_violations(violations, skip)
}

/// Non-stream invariants for a Gemini client response.
fn check_gemini_response_invariants(
    bridge: Bridge,
    response: &Value,
    skip: &HashSet<Invariant>,
    upstream_usage: Option<&TokenUsage>,
) -> Vec<String> {
    let mut violations: Vec<(Invariant, String)> = Vec::new();
    let parts: Vec<&Value> = gemini_parts(response).iter().collect();
    check_gemini_parts(&parts, &mut violations);
    if gemini_finish_reason(response).is_none() {
        violations.push((
            Invariant::Terminal,
            "response has no finishReason".to_string(),
        ));
    }
    if let Err(problem) = check_usage_conservation(
        bridge,
        upstream_usage,
        &client_response_usage(bridge.client(), response),
    ) {
        violations.push((Invariant::UsageConservation, problem));
    }
    report_violations(violations, skip)
}

// ---------------------------------------------------------------------------
// Chunk-split fuzzing
// ---------------------------------------------------------------------------

/// A way of cutting one stream input into chunks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SplitPlan {
    /// Strategy label, e.g. `byte-offset`, `random#17`, `utf8`, `crlf`.
    pub label: String,
    /// Sorted, distinct cut offsets in `1..len`.
    pub offsets: Vec<usize>,
}

impl SplitPlan {
    fn new(label: impl Into<String>, offsets: impl IntoIterator<Item = usize>) -> Self {
        let offsets: BTreeSet<usize> = offsets.into_iter().collect();
        Self {
            label: label.into(),
            offsets: offsets.into_iter().collect(),
        }
    }

    pub fn chunks<'a>(&self, input: &'a [u8]) -> Vec<&'a [u8]> {
        let mut chunks = Vec::with_capacity(self.offsets.len() + 1);
        let mut start = 0;
        for &offset in &self.offsets {
            chunks.push(&input[start..offset]);
            start = offset;
        }
        chunks.push(&input[start..]);
        chunks
    }

    /// Human-readable description used in failure reports.
    pub fn describe(&self) -> String {
        const SHOWN: usize = 32;
        if self.offsets.len() <= SHOWN {
            format!("split {} at offsets {:?}", self.label, self.offsets)
        } else {
            format!(
                "split {} at {} offsets {:?}...",
                self.label,
                self.offsets.len(),
                &self.offsets[..SHOWN]
            )
        }
    }
}

/// SplitMix64: tiny deterministic PRNG so random plans are reproducible from
/// the fixture name alone.
struct SplitMix64(u64);

impl SplitMix64 {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, bound: usize) -> usize {
        (self.next() % bound as u64) as usize
    }
}

/// FNV-1a, used to derive a per-fixture seed.
pub fn seed_for(name: &str) -> u64 {
    name.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// All split plans for `input`, in a fixed order.
pub fn split_plans(input: &[u8], seed: u64) -> Vec<SplitPlan> {
    let len = input.len();
    let mut plans = Vec::new();
    if len < 2 {
        return plans;
    }

    let utf8: Vec<usize> = (1..len).filter(|&i| (input[i] & 0xC0) == 0x80).collect();
    let crlf: Vec<usize> = (1..len)
        .filter(|&i| input[i - 1] == b'\r' && input[i] == b'\n')
        .collect();

    if len <= EXHAUSTIVE_SPLIT_LIMIT {
        plans.extend((1..len).map(|offset| SplitPlan::new("byte-offset", [offset])));
    } else {
        plans.extend(utf8.iter().map(|&offset| SplitPlan::new("utf8", [offset])));
        plans.extend(crlf.iter().map(|&offset| SplitPlan::new("crlf", [offset])));
    }
    if !utf8.is_empty() {
        plans.push(SplitPlan::new("utf8-all", utf8.iter().copied()));
    }
    if !crlf.is_empty() {
        plans.push(SplitPlan::new("crlf-all", crlf.iter().copied()));
    }
    plans.push(SplitPlan::new("every-byte", 1..len));

    let mut rng = SplitMix64(seed);
    for n in 0..RANDOM_SPLIT_PLANS {
        let cuts = 1 + rng.below(RANDOM_SPLIT_MAX_CUTS.min(len - 1));
        let offsets: Vec<usize> = (0..cuts).map(|_| 1 + rng.below(len - 1)).collect();
        plans.push(SplitPlan::new(
            format!("random#{n} seed={seed:#x}"),
            offsets,
        ));
    }
    plans
}

/// Run `run` on every split of `input` and compare against the unsplit run.
///
/// `run` converts a list of chunks into client events; `check` returns the
/// invariant violations of one output. Returns one report line per failing
/// split (each naming its offsets), capped at [`MAX_REPORTED_FAILURES`] plus a
/// summary line.
pub fn fuzz_splits<R, C>(input: &[u8], seed: u64, run: R, check: C) -> Vec<String>
where
    R: Fn(&[&[u8]]) -> Result<Vec<ClientEvent>, String>,
    C: Fn(&[ClientEvent]) -> (Vec<ClientEvent>, Vec<String>),
{
    let mut failures = Vec::new();
    let reference = match run(&[input]) {
        Ok(events) => check(&events).0,
        Err(error) => return vec![format!("unsplit: {error}")],
    };

    let mut failed = 0usize;
    let mut record = |line: String, failures: &mut Vec<String>| {
        failed += 1;
        if failures.len() < MAX_REPORTED_FAILURES {
            failures.push(line);
        }
    };
    for plan in split_plans(input, seed) {
        let chunks = plan.chunks(input);
        let events = match run(&chunks) {
            Ok(events) => events,
            Err(error) => {
                record(format!("{}: {error}", plan.describe()), &mut failures);
                continue;
            }
        };
        let (normalized, violations) = check(&events);
        if let Some(violation) = violations.first() {
            record(
                format!("{}: invariant violated: {violation}", plan.describe()),
                &mut failures,
            );
        } else if normalized != reference {
            record(
                format!(
                    "{}: output differs from unsplit input: {}",
                    plan.describe(),
                    first_difference(&reference, &normalized)
                ),
                &mut failures,
            );
        }
    }
    if failed > failures.len() {
        failures.push(format!(
            "... {} more failing splits not shown",
            failed - failures.len()
        ));
    }
    failures
}

fn first_difference(expected: &[ClientEvent], actual: &[ClientEvent]) -> String {
    for (i, (want, got)) in expected.iter().zip(actual).enumerate() {
        if want != got {
            return format!("event #{i}: expected {} got {}", want.data, got.data);
        }
    }
    format!("expected {} events, got {}", expected.len(), actual.len())
}

// ---------------------------------------------------------------------------
// Stream vs non-stream parity
// ---------------------------------------------------------------------------

/// Fold a client event stream into one message: for Anthropic clients the
/// content blocks in index order, stop reason and usage; for Responses
/// clients the done output items in output order, final status and usage.
pub fn aggregate_stream_message(client: Protocol, events: &[ClientEvent]) -> Value {
    if client == Protocol::Responses {
        return aggregate_responses_stream(events);
    }
    if client == Protocol::Gemini {
        let parts: Vec<&Value> = events
            .iter()
            .flat_map(|event| gemini_parts(&event.data))
            .collect();
        return json!({
            "parts": merge_gemini_parts(&parts),
            "finishReason": events
                .iter()
                .rev()
                .find_map(|event| gemini_finish_reason(&event.data)),
            "usage": client_stream_usage(client, events),
        });
    }
    let mut blocks: Vec<(u64, Map<String, Value>, String)> = Vec::new();
    let mut stop_reason = Value::Null;
    for event in events {
        let index = event.data.get("index").and_then(Value::as_u64);
        match event.kind() {
            "content_block_start" => {
                if let (Some(index), Some(Value::Object(block))) =
                    (index, event.data.get("content_block"))
                {
                    blocks.push((index, block.clone(), String::new()));
                }
            }
            "content_block_delta" => {
                let Some((_, block, json_buf)) =
                    blocks.iter_mut().find(|(i, _, _)| Some(*i) == index)
                else {
                    continue;
                };
                let delta = &event.data["delta"];
                for (delta_key, block_key) in [("text", "text"), ("thinking", "thinking")] {
                    if let Some(piece) = delta.get(delta_key).and_then(Value::as_str) {
                        let entry = block.entry(block_key).or_insert_with(|| json!(""));
                        let mut text = entry.as_str().unwrap_or_default().to_string();
                        text.push_str(piece);
                        *entry = json!(text);
                    }
                }
                if let Some(piece) = delta.get("partial_json").and_then(Value::as_str) {
                    json_buf.push_str(piece);
                }
            }
            "message_delta" => {
                stop_reason = event.data["delta"]["stop_reason"].clone();
            }
            _ => {}
        }
    }
    blocks.sort_by_key(|(index, _, _)| *index);
    let content: Vec<Value> = blocks
        .into_iter()
        .map(|(_, mut block, json_buf)| {
            if block.get("type").and_then(Value::as_str) == Some("tool_use") {
                let input = if json_buf.is_empty() {
                    json!({})
                } else {
                    serde_json::from_str(&json_buf).unwrap_or(Value::String(json_buf))
                };
                block.insert("input".to_string(), input);
            }
            Value::Object(block)
        })
        .collect();
    json!({
        "content": content,
        "stop_reason": stop_reason,
        "usage": client_stream_usage(client, events),
    })
}

/// Gemini parts with adjacent text pieces of the same kind (thought or not)
/// joined, functionCall ids and thought signatures dropped: the shape both the
/// stream and non-stream paths must agree on.
fn merge_gemini_parts(parts: &[&Value]) -> Vec<Value> {
    let mut merged: Vec<Value> = Vec::new();
    for part in parts {
        if let Some(text) = part.get("text").and_then(Value::as_str) {
            if text.is_empty() {
                continue;
            }
            let thought = part.get("thought").and_then(Value::as_bool) == Some(true);
            if let Some(last) = merged.last_mut() {
                let last_thought = last.get("thought").and_then(Value::as_bool) == Some(true);
                if let (Some(previous), true) = (
                    last.get("text").and_then(Value::as_str),
                    last_thought == thought,
                ) {
                    last["text"] = json!(format!("{previous}{text}"));
                    continue;
                }
            }
            merged.push(if thought {
                json!({ "text": text, "thought": true })
            } else {
                json!({ "text": text })
            });
        } else if let Some(call) = part.get("functionCall") {
            let mut call = call.clone();
            if let Some(map) = call.as_object_mut() {
                map.remove("id");
            }
            merged.push(json!({ "functionCall": call }));
        } else {
            merged.push((*part).clone());
        }
    }
    merged
}

fn aggregate_responses_stream(events: &[ClientEvent]) -> Value {
    let mut items: Vec<(u64, Value)> = Vec::new();
    let mut status = Value::Null;
    for event in events {
        match event.kind() {
            "response.output_item.done" => {
                if let (Some(index), Some(item)) = (
                    event.data.get("output_index").and_then(Value::as_u64),
                    event.data.get("item"),
                ) {
                    items.push((index, item.clone()));
                }
            }
            "response.completed" | "response.incomplete" | "response.failed" => {
                status = event.data["response"]["status"].clone();
            }
            _ => {}
        }
    }
    items.sort_by_key(|(index, _)| *index);
    json!({
        "output": items.into_iter().map(|(_, item)| item).collect::<Vec<_>>(),
        "status": status,
        "usage": client_stream_usage(Protocol::Responses, events),
    })
}

/// Compare the stream and non-stream conversions of the same upstream reply on
/// content (ids ignored), stop reason (status for Responses clients) and
/// recorded token counts. Returns one `field: stream=<..> non_stream=<..>`
/// line per difference.
pub fn parity_diff(
    client: Protocol,
    stream_message: &Value,
    non_stream_response: &Value,
) -> Vec<String> {
    fn content_summary(message: &Value) -> Value {
        let blocks = message
            .get("content")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        Value::Array(
            blocks
                .into_iter()
                .filter(|block| {
                    // Empty text blocks carry no content in either path.
                    !(block.get("type").and_then(Value::as_str) == Some("text")
                        && block.get("text").and_then(Value::as_str) == Some(""))
                })
                .map(|mut block| {
                    if let Some(map) = block.as_object_mut() {
                        map.remove("id");
                        map.remove("signature");
                    }
                    block
                })
                .collect(),
        )
    }
    fn tokens(usage: &Value) -> Value {
        json!({
            "input_tokens": usage.get("input_tokens"),
            "output_tokens": usage.get("output_tokens"),
            "cache_read_tokens": usage.get("cache_read_tokens"),
            "cache_creation_tokens": usage.get("cache_creation_tokens"),
        })
    }

    fn output_summary(message: &Value) -> Value {
        let items = message
            .get("output")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        Value::Array(
            items
                .into_iter()
                .map(|mut item| {
                    if let Some(map) = item.as_object_mut() {
                        map.remove("id");
                    }
                    item
                })
                .collect(),
        )
    }

    let non_stream_usage = client_response_usage(client, non_stream_response);
    let gemini_non_stream;
    let non_stream_response = if client == Protocol::Gemini {
        let parts: Vec<&Value> = gemini_parts(non_stream_response).iter().collect();
        gemini_non_stream = json!({
            "parts": merge_gemini_parts(&parts),
            "finishReason": gemini_finish_reason(non_stream_response),
        });
        &gemini_non_stream
    } else {
        non_stream_response
    };
    let (stop_field, content_field, content) = match client {
        Protocol::Gemini => (
            "finishReason",
            "parts",
            (
                stream_message["parts"].clone(),
                non_stream_response["parts"].clone(),
            ),
        ),
        Protocol::Responses => (
            "status",
            "output",
            (
                output_summary(stream_message),
                output_summary(non_stream_response),
            ),
        ),
        _ => (
            "stop_reason",
            "content",
            (
                content_summary(stream_message),
                content_summary(non_stream_response),
            ),
        ),
    };
    let pairs = [
        (
            stop_field,
            stream_message[stop_field].clone(),
            non_stream_response
                .get(stop_field)
                .cloned()
                .unwrap_or(Value::Null),
        ),
        (content_field, content.0, content.1),
        (
            "usage",
            tokens(&stream_message["usage"]),
            tokens(&non_stream_usage),
        ),
    ];
    pairs
        .into_iter()
        .filter(|(_, stream, non_stream)| stream != non_stream)
        .map(|(field, stream, non_stream)| {
            format!("{field}: stream={stream} non_stream={non_stream}")
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Whole-fixture driver
// ---------------------------------------------------------------------------

/// Everything a fixture run produces: snapshot texts and failures.
#[derive(Debug, Default)]
pub struct FixtureOutcome {
    /// `upstream_request` snapshot text, if the fixture has a client request.
    pub upstream_request: Option<String>,
    /// `client_response` snapshot text (stream events and/or JSON response).
    pub client_response: Option<String>,
    /// `usage` snapshot text.
    pub usage: Option<String>,
    /// Invariant, split-fuzz, parity and conversion failures.
    pub failures: Vec<String>,
}

/// Run every check for one fixture with the default (legacy) conversion engine.
pub fn run_fixture(fixture: &Fixture) -> FixtureOutcome {
    run_fixture_with(fixture, CodecEngine::Legacy)
}

/// [`run_fixture`] with an explicit conversion engine; the snapshots of a
/// bridge migrated to codecs must be identical under both engines.
pub fn run_fixture_with(fixture: &Fixture, engine: CodecEngine) -> FixtureOutcome {
    let mut outcome = FixtureOutcome::default();
    let mut redactor = Redactor::new(fixture);
    let skip = fixture.skipped();
    let client = fixture.bridge.client();

    if let Some(result) = convert_request_with(fixture, engine) {
        match result {
            Ok(mut request) => {
                redactor.redact(&mut request);
                outcome.upstream_request = Some(render_json(&request));
            }
            Err(error) => outcome.upstream_request = Some(format!("ERROR: {error}\n")),
        }
    }

    let mut client_response = String::new();
    let mut usage = String::new();
    let mut stream_message = None;

    if let Some(input) = &fixture.upstream_sse {
        let upstream_usage = upstream_stream_usage(fixture.bridge, input);
        let terminal = fixture.meta.invariants.terminal;
        let fresh_redactor = redactor.clone();
        let check = |events: &[ClientEvent]| {
            // A fresh redactor per run: generated ids must line up by first
            // appearance, not by literal value (synthesized ids are random).
            let normalized = fresh_redactor.clone().redact_events(events);
            let violations = check_stream_invariants(
                fixture.bridge,
                events,
                terminal,
                &skip,
                upstream_usage.as_ref(),
            );
            (normalized, violations)
        };
        let run = |chunks: &[&[u8]]| run_stream_with(fixture, chunks, engine);

        match run(&[input.as_slice()]) {
            Ok(events) => {
                for violation in check(&events).1 {
                    outcome.failures.push(format!("unsplit: {violation}"));
                }
                let redacted = redactor.redact_events(&events);
                client_response.push_str("# stream\n");
                client_response.push_str(&render_events(&redacted));
                let mut stream_usage = client_stream_usage(client, &events);
                redactor.redact(&mut stream_usage);
                usage.push_str("# stream\n");
                usage.push_str(&render_json(&stream_usage));
                stream_message = Some(aggregate_stream_message(client, &events));
            }
            Err(error) => outcome.failures.push(format!("unsplit: {error}")),
        }
        outcome
            .failures
            .extend(fuzz_splits(input, seed_for(&fixture.id()), run, check));
    }

    if let Some(result) = convert_non_stream_with(fixture, engine) {
        if !client_response.is_empty() {
            client_response.push('\n');
            usage.push('\n');
        }
        client_response.push_str("# non-stream\n");
        usage.push_str("# non-stream\n");
        match result {
            Ok(response) => {
                let upstream_usage = fixture
                    .upstream_json
                    .as_ref()
                    .and_then(|body| upstream_response_usage(fixture.bridge, body));
                for violation in check_response_invariants(
                    fixture.bridge,
                    &response,
                    &skip,
                    upstream_usage.as_ref(),
                ) {
                    outcome.failures.push(format!("non-stream: {violation}"));
                }
                let mut response_usage = client_response_usage(client, &response);
                if let Some(stream_message) = &stream_message {
                    let diff = parity_diff(client, stream_message, &response);
                    if diff != fixture.meta.parity.expected_diff {
                        outcome.failures.push(format!(
                            "parity: stream vs non-stream diff changed\n  expected: {:#?}\n  actual:   {diff:#?}",
                            fixture.meta.parity.expected_diff
                        ));
                    }
                }
                let mut response = response;
                redactor.redact(&mut response);
                redactor.redact(&mut response_usage);
                client_response.push_str(&render_json(&response));
                usage.push_str(&render_json(&response_usage));
            }
            Err(error) => {
                client_response.push_str(&format!("ERROR: {error}\n"));
                usage.push_str("null\n");
            }
        }
    }

    if !client_response.is_empty() {
        outcome.client_response = Some(client_response);
        outcome.usage = Some(usage);
    }
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token_usage(input: u32, output: u32, cache_read: u32, cache_creation: u32) -> TokenUsage {
        TokenUsage {
            input_tokens: input,
            output_tokens: output,
            cache_read_tokens: cache_read,
            cache_creation_tokens: cache_creation,
            model: None,
            message_id: None,
        }
    }

    #[test]
    fn usage_parity_case_accepts_consistent_usage() {
        // Chat upstream (input includes cache) -> Anthropic client (fresh input).
        let raw = json!({
            "prompt_tokens": 100, "completion_tokens": 20, "total_tokens": 120,
            "prompt_tokens_details": {"cached_tokens": 40}
        });
        let recorded = token_usage(60, 20, 40, 0);
        assert_eq!(
            check_usage_parity_case(
                Bridge::AnthropicOpenAiChat,
                Some(&raw),
                Some(&recorded),
                true,
                None
            ),
            Vec::<String>::new()
        );
        // No usage block and nothing billed.
        assert!(check_usage_parity_case(Bridge::ResponsesChat, None, None, true, None).is_empty());
    }

    #[test]
    fn usage_parity_case_flags_cache_double_count() {
        // cached_tokens exceeds prompt_tokens: the normalized total overshoots
        // the upstream total.
        let raw = json!({
            "prompt_tokens": 100, "completion_tokens": 20, "total_tokens": 120,
            "prompt_tokens_details": {"cached_tokens": 150}
        });
        let recorded = token_usage(0, 20, 150, 0);
        let problems = check_usage_parity_case(
            Bridge::AnthropicOpenAiChat,
            Some(&raw),
            Some(&recorded),
            true,
            None,
        );
        assert!(
            problems
                .iter()
                .any(|p| p.starts_with("usage_ir: CacheExceedsInput")),
            "{problems:#?}"
        );
        assert!(
            problems
                .iter()
                .any(|p| p.starts_with("recorded: CacheExceedsInput")),
            "{problems:#?}"
        );
        // Declared as a known upstream inconsistency, the same case passes,
        // and a missing flag would fail.
        assert!(check_usage_parity_case(
            Bridge::AnthropicOpenAiChat,
            Some(&raw),
            Some(&recorded),
            true,
            Some(UsageParityIssue::CacheExceedsInput)
        )
        .is_empty());
        assert!(!check_usage_parity_case(
            Bridge::AnthropicOpenAiChat,
            Some(&json!({"prompt_tokens": 1, "completion_tokens": 1})),
            Some(&token_usage(1, 1, 0, 0)),
            true,
            Some(UsageParityIssue::CacheExceedsInput)
        )
        .is_empty());
    }

    #[test]
    fn usage_parity_case_flags_bucket_and_cost_drift() {
        // Totals agree, but the cache read was recorded as cache creation:
        // the bucket check and the client's cost row both catch it.
        let raw = json!({
            "input_tokens": 100, "output_tokens": 10,
            "input_tokens_details": {"cached_tokens": 30}
        });
        let recorded = token_usage(70, 10, 0, 30);
        let problems = check_usage_parity_case(
            Bridge::AnthropicResponses,
            Some(&raw),
            Some(&recorded),
            true,
            None,
        );
        assert!(
            problems
                .iter()
                .any(|p| p.starts_with("recorded cache_read=0 but UsageIR cache_read=30")),
            "{problems:#?}"
        );
        assert!(
            problems
                .iter()
                .any(|p| p.starts_with("cost table row `recorded as claude`")),
            "{problems:#?}"
        );
    }

    #[test]
    fn usage_parity_case_flags_missing_or_invented_usage() {
        let raw = json!({"promptTokenCount": 3, "candidatesTokenCount": 2, "totalTokenCount": 5});
        let problems =
            check_usage_parity_case(Bridge::AnthropicGemini, Some(&raw), None, true, None);
        assert!(
            problems
                .iter()
                .any(|p| p.starts_with("recorded: ZeroUsage")),
            "{problems:#?}"
        );
        let problems = check_usage_parity_case(
            Bridge::AnthropicGemini,
            None,
            Some(&token_usage(3, 2, 0, 0)),
            true,
            None,
        );
        assert_eq!(problems.len(), 1, "{problems:#?}");
    }

    #[test]
    fn usage_parity_case_bounds_aborted_streams_by_the_ir() {
        let raw = json!({"promptTokenCount": 3, "totalTokenCount": 4});
        // The output never reached the client: within the IR, so accepted.
        assert!(check_usage_parity_case(
            Bridge::AnthropicGemini,
            Some(&raw),
            Some(&token_usage(3, 0, 0, 0)),
            false,
            None
        )
        .is_empty());
        // Recording more than the upstream reported is still a mismatch.
        assert!(!check_usage_parity_case(
            Bridge::AnthropicGemini,
            Some(&raw),
            Some(&token_usage(3, 5, 0, 0)),
            false,
            None
        )
        .is_empty());
    }

    #[test]
    fn usage_parity_merges_anthropic_stream_usage() {
        let sse = concat!(
            "event: message_start\n",
            "data: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":7,\"cache_read_input_tokens\":5,\"output_tokens\":1}}}\n\n",
            "event: message_delta\n",
            "data: {\"type\":\"message_delta\",\"usage\":{\"output_tokens\":9}}\n\n",
        );
        assert_eq!(
            raw_stream_usage_block(Protocol::Anthropic, sse.as_bytes()),
            Some(json!({"input_tokens": 7, "cache_read_input_tokens": 5, "output_tokens": 9}))
        );
    }

    fn event(name: &str, data: Value) -> ClientEvent {
        ClientEvent {
            event: Some(name.to_string()),
            data,
        }
    }

    fn text_stream() -> Vec<ClientEvent> {
        vec![
            event(
                "message_start",
                json!({"type":"message_start","message":{"id":"m","usage":{"input_tokens":3,"output_tokens":0}}}),
            ),
            event(
                "content_block_start",
                json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
            ),
            event(
                "content_block_delta",
                json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"hi"}}),
            ),
            event(
                "content_block_stop",
                json!({"type":"content_block_stop","index":0}),
            ),
            event(
                "message_delta",
                json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"input_tokens":3,"output_tokens":1}}),
            ),
            event("message_stop", json!({"type":"message_stop"})),
        ]
    }

    #[test]
    fn well_formed_stream_has_no_violations() {
        let upstream = TokenUsage {
            input_tokens: 3,
            output_tokens: 1,
            ..Default::default()
        };
        let violations = check_stream_invariants(
            Bridge::AnthropicOpenAiChat,
            &text_stream(),
            Terminal::MessageStop,
            &HashSet::new(),
            Some(&upstream),
        );
        assert!(violations.is_empty(), "{violations:?}");
    }

    #[test]
    fn invariants_flag_unpaired_blocks_and_double_terminal() {
        let mut events = text_stream();
        events.remove(3); // drop content_block_stop
        events.push(event("message_stop", json!({"type":"message_stop"})));
        let violations = check_stream_invariants(
            Bridge::AnthropicOpenAiChat,
            &events,
            Terminal::MessageStop,
            &HashSet::new(),
            None,
        );
        assert!(
            violations.iter().any(|v| v.starts_with("[block_pairing]")),
            "{violations:?}"
        );
        assert!(
            violations.iter().any(|v| v.starts_with("[terminal]")),
            "{violations:?}"
        );
    }

    #[test]
    fn invariants_flag_usage_drift_and_late_thinking() {
        let mut events = text_stream();
        events.insert(
            4,
            event(
                "content_block_start",
                json!({"type":"content_block_start","index":1,"content_block":{"type":"thinking","thinking":""}}),
            ),
        );
        events.insert(
            5,
            event(
                "content_block_stop",
                json!({"type":"content_block_stop","index":1}),
            ),
        );
        let upstream = TokenUsage {
            input_tokens: 5,
            output_tokens: 1,
            ..Default::default()
        };
        let violations = check_stream_invariants(
            Bridge::AnthropicOpenAiChat,
            &events,
            Terminal::MessageStop,
            &HashSet::new(),
            Some(&upstream),
        );
        assert!(violations
            .iter()
            .any(|v| v.starts_with("[reasoning_first]")));
        assert!(violations
            .iter()
            .any(|v| v.starts_with("[usage_conservation]")));

        let skip: HashSet<Invariant> =
            [Invariant::ReasoningFirst, Invariant::UsageConservation].into();
        assert!(check_stream_invariants(
            Bridge::AnthropicOpenAiChat,
            &events,
            Terminal::MessageStop,
            &skip,
            Some(&upstream)
        )
        .is_empty());
    }

    fn responses_text_stream() -> Vec<ClientEvent> {
        let item = json!({"id":"msg_1","type":"message","role":"assistant","content":[{"type":"output_text","text":"hi"}]});
        vec![
            event(
                "response.created",
                json!({"type":"response.created","response":{"id":"r","status":"in_progress","output":[]}}),
            ),
            event(
                "response.output_item.added",
                json!({"type":"response.output_item.added","output_index":0,"item":{"id":"msg_1","type":"message","content":[]}}),
            ),
            event(
                "response.output_text.delta",
                json!({"type":"response.output_text.delta","item_id":"msg_1","output_index":0,"delta":"hi"}),
            ),
            event(
                "response.output_item.done",
                json!({"type":"response.output_item.done","output_index":0,"item":item}),
            ),
            event(
                "response.completed",
                json!({"type":"response.completed","response":{"id":"r","status":"completed","output":[item],"usage":{"input_tokens":10,"output_tokens":1,"input_tokens_details":{"cached_tokens":4,"cache_write_tokens":2}}}}),
            ),
        ]
    }

    #[test]
    fn responses_invariants_accept_well_formed_stream_and_conserve_usage() {
        // Anthropic upstream: fresh 4 + cache read 4 + cache creation 2 = total 10.
        let upstream = TokenUsage {
            input_tokens: 4,
            output_tokens: 1,
            cache_read_tokens: 4,
            cache_creation_tokens: 2,
            ..Default::default()
        };
        let violations = check_stream_invariants(
            Bridge::ResponsesAnthropic,
            &responses_text_stream(),
            Terminal::MessageStop,
            &HashSet::new(),
            Some(&upstream),
        );
        assert!(violations.is_empty(), "{violations:?}");

        // Chat upstream reporting the same prompt total without cache writes.
        let upstream = TokenUsage {
            input_tokens: 10,
            output_tokens: 1,
            cache_read_tokens: 4,
            ..Default::default()
        };
        let violations = check_stream_invariants(
            Bridge::ResponsesChat,
            &responses_text_stream(),
            Terminal::MessageStop,
            &HashSet::new(),
            Some(&upstream),
        );
        assert!(
            violations
                .iter()
                .any(|v| v.starts_with("[usage_conservation]") && v.contains("cache_creation")),
            "{violations:?}"
        );
    }

    #[test]
    fn responses_invariants_flag_open_items_and_bad_tool_calls() {
        let mut events = responses_text_stream();
        events.remove(3); // drop output_item.done
        let call = json!({"type":"function_call","call_id":"","name":"f","arguments":"{"});
        events.insert(
            3,
            event(
                "response.output_item.added",
                json!({"type":"response.output_item.added","output_index":2,"item":call}),
            ),
        );
        events.insert(
            4,
            event(
                "response.output_item.done",
                json!({"type":"response.output_item.done","output_index":2,"item":call}),
            ),
        );
        let violations = check_stream_invariants(
            Bridge::ResponsesChat,
            &events,
            Terminal::MessageStop,
            &HashSet::new(),
            None,
        );
        for name in [
            "block_pairing",
            "index_order",
            "tool_ids",
            "tool_input_json",
        ] {
            assert!(
                violations
                    .iter()
                    .any(|v| v.starts_with(&format!("[{name}]"))),
                "missing {name}: {violations:?}"
            );
        }
    }

    #[test]
    fn split_plans_cover_every_offset_utf8_and_crlf() {
        let input = "data: 你\r\n\r\n".as_bytes();
        let plans = split_plans(input, 7);
        for offset in 1..input.len() {
            assert!(plans
                .iter()
                .any(|plan| plan.label == "byte-offset" && plan.offsets == [offset]));
        }
        let utf8 = plans.iter().find(|plan| plan.label == "utf8-all").unwrap();
        assert_eq!(utf8.offsets, vec![7, 8]);
        let crlf = plans.iter().find(|plan| plan.label == "crlf-all").unwrap();
        assert_eq!(crlf.offsets, vec![10, 12]);
        assert_eq!(
            plans
                .iter()
                .filter(|plan| plan.label.starts_with("random#"))
                .count(),
            RANDOM_SPLIT_PLANS
        );
        // Deterministic for a given seed.
        assert_eq!(plans, split_plans(input, 7));
        for plan in &plans {
            assert_eq!(plan.chunks(input).concat(), input);
        }
    }

    /// A deliberately broken converter that decodes each chunk on its own (the
    /// bug fixed by `append_utf8_safe`). The fuzzer must catch it and name the
    /// offending offsets.
    #[test]
    fn fuzzer_reports_split_offset_of_chunk_boundary_bug() {
        let input = "data: {\"t\":\"你好\"}\n\n".as_bytes();
        let ni = input.windows(3).position(|w| w == "你".as_bytes()).unwrap();
        let run = |chunks: &[&[u8]]| {
            let text: String = chunks
                .iter()
                .map(|chunk| String::from_utf8_lossy(chunk).into_owned())
                .collect();
            let data = text.trim().trim_start_matches("data: ").to_string();
            Ok(vec![event(
                "content_block_delta",
                json!({"type":"content_block_delta","text":data}),
            )])
        };
        let check = |events: &[ClientEvent]| (events.to_vec(), Vec::new());
        let failures = fuzz_splits(input, 1, run, check);
        assert!(!failures.is_empty());
        let expected = format!("split byte-offset at offsets [{}]", ni + 1);
        assert!(
            failures.iter().any(|line| line.starts_with(&expected)),
            "expected a failure for {expected}: {failures:#?}"
        );
    }

    #[test]
    fn fuzzer_reports_split_offset_of_invariant_violation() {
        let input = b"0123456789";
        // Emits a second message_stop whenever a chunk boundary falls at 4.
        let run = |chunks: &[&[u8]]| {
            let mut events = vec![event("message_stop", json!({"type":"message_stop"}))];
            let mut at = 0;
            for chunk in &chunks[..chunks.len() - 1] {
                at += chunk.len();
                if at == 4 {
                    events.push(event("message_stop", json!({"type":"message_stop"})));
                }
            }
            Ok(events)
        };
        let check = |events: &[ClientEvent]| {
            let violations = check_stream_invariants(
                Bridge::AnthropicOpenAiChat,
                events,
                Terminal::MessageStop,
                &HashSet::new(),
                None,
            )
            .into_iter()
            .filter(|v| v.starts_with("[terminal]"))
            .collect();
            (events.to_vec(), violations)
        };
        let failures = fuzz_splits(input, 3, run, check);
        assert!(
            failures.iter().any(|line| line
                .starts_with("split byte-offset at offsets [4]: invariant violated: [terminal]")),
            "{failures:#?}"
        );
    }

    #[test]
    fn redactor_keeps_input_ids_and_numbers_generated_ones() {
        let fixture = Fixture {
            bridge: Bridge::AnthropicGemini,
            name: "t".into(),
            dir: PathBuf::new(),
            meta: toml::from_str("intent = \"x\"\nsource_commit = \"abc\"").unwrap(),
            client_request: None,
            upstream_sse: Some(b"data: {\"id\":\"call_1\"}\n\n".to_vec()),
            upstream_json: None,
        };
        let mut redactor = Redactor::new(&fixture);
        let mut value = json!([
            {"id": "call_1"},
            {"id": "gemini_synth_aaa", "created": 123},
            {"tool_use_id": "gemini_synth_aaa"},
            {"id": "gemini_synth_bbb"}
        ]);
        redactor.redact(&mut value);
        assert_eq!(
            value,
            json!([
                {"id": "call_1"},
                {"id": "[generated-id:1]", "created": "[timestamp]"},
                {"tool_use_id": "[generated-id:1]"},
                {"id": "[generated-id:2]"}
            ])
        );
    }

    #[test]
    fn parity_diff_reports_field_level_differences() {
        let stream = json!({
            "content": [{"type": "tool_use", "id": "a", "name": "f", "input": {}}],
            "stop_reason": "tool_use",
            "usage": {"input_tokens": 1, "output_tokens": 2, "cache_read_tokens": 0, "cache_creation_tokens": 0}
        });
        let non_stream = json!({
            "content": [{"type": "tool_use", "id": "b", "name": "f", "input": {}}],
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 1, "output_tokens": 2}
        });
        assert_eq!(
            parity_diff(Protocol::Anthropic, &stream, &non_stream),
            vec!["stop_reason: stream=\"tool_use\" non_stream=\"end_turn\"".to_string()]
        );
    }
}
