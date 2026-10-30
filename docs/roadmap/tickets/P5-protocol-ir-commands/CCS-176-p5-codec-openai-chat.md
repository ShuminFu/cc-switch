# CCS-176: [proxy] Migrate the Anthropic<-OpenAI Chat bridge to codecs

**Status:** Done · **Kind:** refactor · **Labels:** `refactor`, `phase:P5`, `p2-medium`, `area:proxy`

> **Ticket key:** `p5-codec-openai-chat` · **Phase:** P5 · **Priority:** p2-medium · **Effort:** L · **Depends on:** `p5-session-side-store`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Proposed change
Add codec/openai_chat and codec/anthropic. Delete transform.rs and streaming.rs only after golden parity and one release with no regressions.

## Acceptance criteria / verification
- [x] Golden fixtures for anthropic__openai_chat pass, including the P1 fixes
- [x] The parity checker reports 0 mismatches
- [x] `cargo test --manifest-path src-tauri/Cargo.toml --test proxy_golden`

## Dependencies
p5-session-side-store

## Implementation notes

**Summary.** The Anthropic client ← OpenAI Chat upstream bridge now runs through the IR codecs when `codecEngine = ir`. The default stays `legacy` for one release. As the ticket asks, `transform.rs` and `streaming.rs` are kept as the fallback. They should be deleted only after a release ships with no regressions.

- `codec/anthropic`: request decode (Messages → `ChatRequest`), response encode, and the SSE `EventEncoder`.
- `codec/openai_chat`: request encode, response decode, and the SSE `EventDecoder`. The decoder follows every legacy rule: deferred tool starts, late fallback starts (`tool_call_N` / `unknown_tool`), first `finish_reason` only, end deferred to `[DONE]`/EOF, in-band errors, `max_tokens` vs `stream_truncated` truncation, and the Copilot infinite-whitespace abort.
- Codec directions no bridge uses yet (Anthropic as upstream, Chat as client) return `CodecError::Unsupported`. They will be added when the Responses bridges migrate.
- `codec/transcode.rs` is a generic upstream SSE → decoder → encoder driver built on `SseDecoder`.
- `codec/bridge.rs` provides the engine-switching entry points: `anthropic_to_openai_chat_request`, `openai_chat_to_anthropic_response` and `openai_chat_to_anthropic_sse`. `claude.rs`, `handlers.rs` (non-stream) and `stream::codec::ChatToAnthropic` call these with `settings::codec_engine()`.
- IR changes:
  - `MessageStart.usage`: the legacy `message_start` carries usage from the first chunk only.
  - `StreamEvent::Error.code`: the error type shown to clients.
  - `MediaSource::Text`: for text/content document sources.
  - `ReasoningConfig.adaptive`.
- `StreamCodec` constructors now return `Result`.
- Testkit: `*_with(fixture, CodecEngine)` variants and `stream_output_with` (raw bytes).

**Parity checker.** `tests/proxy_golden.rs::codec_parity_anthropic_openai_chat` runs every `anthropic__openai_chat` fixture under both engines. For each one it compares the upstream request, the raw client SSE bytes, the client response, the usage and the check failures. Result: 0 mismatches. `golden_anthropic_openai_chat_ir` checks the IR engine against the unchanged snapshots, including split fuzzing and invariants. No fixture or snapshot changed.

The usage-parity CI checker over the whole corpus is a separate ticket (CCS-186). `proxy::codec::bridge::tests` adds legacy-vs-IR differential tests on hand-written inputs beyond the corpus: system and billing header, every document and image source, tool history, tool_result content forms, tools and tool_choice, the reasoning-effort matrix, response variants and errors, stream tools, whitespace abort, errors, truncation and framing. Each stream case also runs with a transport error appended.

**Intentional IR-engine differences** (malformed or equivalent inputs only, asserted in tests):
- An integral float is written as an integer (`1.0` → `1`).
- `stream: false` and `stop_sequences: []` are no longer forwarded. Both are the defaults.
- Messages with a role other than user, assistant or system are sent as user. Messages without `content` are dropped. Text from `role: system` messages is folded into the system prompt.
- Unknown `tool_choice` values are dropped. Non-numeric sampling fields are dropped.
- A transport error that arrives after `message_stop` no longer adds an `error` event after the terminal event.
- SSE framing follows the WHATWG rules through `SseDecoder`.

**Files:**
- `src-tauri/src/proxy/codec/{anthropic,openai_chat}/{mod,request,response,stream,tests}.rs` (new)
- `src-tauri/src/proxy/codec/{bridge.rs,bridge/tests.rs,transcode.rs}` (new)
- `src-tauri/src/proxy/codec/{mod.rs,tests.rs}`
- `src-tauri/src/proxy/protocol/ir.rs`
- `src-tauri/src/proxy/providers/{claude.rs,testkit.rs}`
- `src-tauri/src/proxy/handlers.rs`
- `src-tauri/src/proxy/stream/codec.rs`
- `src-tauri/src/settings.rs`
- `src-tauri/tests/proxy_golden.rs`

**Commits:** 86c1d88 (code), plus the follow-up commit with this note.

**Verification:**
- `cargo fmt --check`: clean
- `cargo clippy --all-targets -- -D warnings`: clean
- `cargo test --lib codec`: 44 passed
- `cargo test --manifest-path src-tauri/Cargo.toml --test proxy_golden`: 8 passed (adds `golden_anthropic_openai_chat_ir` and `codec_parity_anthropic_openai_chat`)
- `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast`: 2796 passed, 0 failed
- `pnpm typecheck`: clean (no frontend changes)
