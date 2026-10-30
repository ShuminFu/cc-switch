# CCS-177: [proxy] Migrate the Gemini bridge to codecs

**Status:** Done · **Kind:** refactor · **Labels:** `refactor`, `phase:P5`, `p2-medium`, `area:proxy`

> **Ticket key:** `p5-codec-gemini` · **Phase:** P5 · **Priority:** p2-medium · **Effort:** L · **Depends on:** `p5-codec-openai-chat`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Proposed change
Add codec/gemini, then delete transform_gemini.rs and streaming_gemini.rs after parity and one release.

## Acceptance criteria / verification
- [x] anthropic__gemini fixtures pass, including the P1 delta-mode fixtures
- [x] `cargo test --manifest-path src-tauri/Cargo.toml --test proxy_golden`

## Dependencies
p5-codec-openai-chat

## Implementation notes

**Summary.** When `codecEngine = ir`, the Anthropic client ← Gemini Native upstream bridge now runs through the IR codecs. The default stays `legacy` for one release. As the ticket asks, `transform_gemini.rs` and `streaming_gemini.rs` are kept as the fallback. They should be deleted only after a release ships with no regressions.

- `codec/gemini` (Gemini as upstream only; the client-side directions return `CodecError::Unsupported`):
  - Request encode (`ChatRequest` → `generateContent`), with the same field order as the legacy converter. It replays shadow assistant turns, matching them by tool-call id, then by name, then by position. It re-attaches `thoughtSignature`, strips synthesized ids and resolves `functionResponse.name` from the shadow, the request history and the same message.
  - Response decode: the first candidate only. Thought parts are skipped, missing ids are synthesized, schema-hint argument rectification runs, and a blocked prompt becomes refusal text.
  - `GeminiEventDecoder` for SSE: text is sent live through the cumulative/incremental `GeminiTextDeltaTracker` (the P1 delta-mode logic). Tool-call snapshots are merged and sent at the end. Everything after `[DONE]` is ignored.
- Shadow recording goes through `GeminiShadowSink`, which needs a store, a provider and a session (the same rule as before), via `SessionSideStore`. A non-stream response records its turn when it is decoded. A stream records at a clean end of stream, and records nothing on a transport error.
- Shared helpers moved into `codec/gemini/{tool_calls,text}.rs` (synthesized ids, schema hints and rectification, `extract_tool_call_meta`, `shadow_parts`, snapshot merging, the text tracker). The legacy converters import them, and `transform_gemini` re-exports the hint API, so `handlers.rs` and `stream/codec.rs` imports are unchanged. Deleting the legacy files later needs no further moves.
- `codec/bridge.rs` provides `anthropic_to_gemini_request`, `gemini_to_anthropic_response` and `gemini_to_anthropic_sse`. `claude.rs`, `handlers.rs` (non-stream), `stream::codec::GeminiToAnthropic` and the testkit call them with `settings::codec_engine()`.
- Two shared codec additions keep the legacy stream byte for byte:
  - `AnthropicEventEncoder::with_start_output_tokens()`: the legacy `message_start` carries the first chunk's full usage.
  - `transcode_sse_stream_with(.., TransportErrors::Propagate)`: a transport error ends the stream with an `Err` item, not an SSE `error` event.
- `json_number` moved from `openai_chat/request.rs` to `codec/mod.rs` so both encoders share it.

**Parity checker.** `tests/proxy_golden.rs::codec_parity_anthropic_gemini` runs every `anthropic__gemini` fixture under both engines. That includes `stream_text_cumulative`, `stream_text_incremental`, the synthesized-id, `[DONE]`, CRLF, CJK and upstream-error cases. For each one it compares the upstream request, the raw client SSE bytes, the client response, the usage and the check failures. Raw bytes are compared after random `gemini_synth_*` ids are numbered by first appearance, since two runs never share them. Result: 0 mismatches. `golden_anthropic_gemini_ir` checks the IR engine against the unchanged snapshots, including split fuzzing and invariants. No fixture or snapshot changed.

`proxy::codec::gemini::tests` adds legacy-vs-IR differential tests on hand-written inputs beyond the corpus:
- request content forms, every tool_choice and the error branches
- shadow replay by id, name and position, synthesized-id stripping and thoughtSignature re-attachment
- responses with thoughts, empty and synthesized ids, rectification, blocking and missing candidates
- shadow recording on both paths, including no recording on a block or a transport error
- streams with blocking, garbage and empty input, tools, signatures and transport errors

**Intentional IR-engine differences** (malformed or equivalent inputs only, asserted in `request_intentional_differences_from_legacy` and `stream_decoder_ignores_events_after_done`):
- `functionCall.args` replayed from a client `tool_use` has its keys sorted (canonical JSON, as in CCS-176). Shadow-replayed parts and response arguments keep their original order.
- An integral float is written as an integer (`1.0` → `1`). An empty `stop_sequences: []` is no longer forwarded.
- `tool_result` content `[]` or `null` is sent as `""`. In a mixed array, unknown blocks are kept as JSON text and joined with the text blocks. An array of only images or documents is rebuilt from the IR in Anthropic block shape.
- The string `tool_choice` `"any"`/`"required"` maps to `ANY`, and unknown `tool_choice` values are dropped. The legacy converter returned an error for both.
- Text from a `role: system` message is folded into the system prompt only from text blocks. An image without a source type is treated as base64. Blocks the Anthropic decoder cannot represent are skipped, not rejected.
- Events after `[DONE]` are always ignored. The legacy converter ignored only those in the same network chunk.
- SSE framing follows the WHATWG rules through `SseDecoder`.

**Files:**
- `src-tauri/src/proxy/codec/gemini/{mod,request,response,stream,text,tool_calls,tests}.rs` (new)
- `src-tauri/src/proxy/codec/{bridge.rs,mod.rs,transcode.rs,anthropic/stream.rs,openai_chat/request.rs}`
- `src-tauri/src/proxy/providers/{transform_gemini.rs,streaming_gemini.rs,claude.rs,testkit.rs}`
- `src-tauri/src/proxy/handlers.rs`
- `src-tauri/src/proxy/stream/codec.rs`
- `src-tauri/tests/proxy_golden.rs`

**Commits:** 4217f8d (code), plus the follow-up commit with this note.

**Verification:**
- `cargo fmt --check`: clean
- `cargo clippy -- -D warnings` and `cargo clippy --all-targets -- -D warnings`: clean
- `cargo test --lib codec`: 61 passed
- `cargo test --lib gemini`: 195 passed
- `cargo test --manifest-path src-tauri/Cargo.toml --test proxy_golden`: 10 passed (adds `golden_anthropic_gemini_ir` and `codec_parity_anthropic_gemini`)
- `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast`: 2815 passed, 0 failed
- No frontend changes.
