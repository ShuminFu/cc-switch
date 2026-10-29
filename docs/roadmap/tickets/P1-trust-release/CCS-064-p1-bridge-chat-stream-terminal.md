# CCS-064: [bridge] Port the terminal state machine to the Chat->Anthropic stream converter

**Status:** Done · **Kind:** bug · **Labels:** `bug`, `phase:P1`, `p2-medium`, `area:proxy-bridge`

> **Ticket key:** `p1-bridge-chat-stream-terminal` · **Phase:** P1 · **Priority:** p2-medium · **Effort:** M · **Depends on:** `p0-golden-testkit-chat-gemini`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding proxy-chat-stream-terminal.
- A null or missing delta drops the whole chunk, along with its finish_reason and usage.
- In-band `{error}` payloads are swallowed.
- An early EOF emits nothing.
- A bare [DONE] emits message_stop without closing open blocks, possibly before message_start.
- A repeated [DONE] emits message_stop twice.

## Evidence
- src-tauri/src/proxy/providers/streaming.rs:26-30 (delta not default), 187-209, 228, 649-670; production use handlers.rs:422

## Proposed change
- Add #[serde(default)] to delta.
- Detect a top-level error or `event: error` and emit an Anthropic error event.
- Add a single finalize helper: close open blocks, send one message_delta, then one message_stop guarded by has_sent_message_stop. Use it on [DONE] and EOF, marking EOF without a finish as truncation (as streaming_responses.rs does).
- Never emit message_stop before message_start.

## Acceptance criteria / verification
- [x] Tests: (a) no-delta finish_stop gives stop_reason end_turn; (b) an error payload gives event: error; (c) EOF after text gives content_block_stop plus a truncation signal; (d) [DONE] closes blocks; (e) a double [DONE] gives exactly one message_stop
- [x] Golden fixtures added for each
- [x] `cargo test --manifest-path src-tauri/Cargo.toml proxy::providers::streaming`

## Dependencies
p0-golden-testkit-chat-gemini

## Implementation notes

**Summary.** `create_anthropic_sse_stream` (`src-tauri/src/proxy/providers/streaming.rs`) now follows the same terminal state machine as the Responses converter:
- `StreamChoice.delta` is `#[serde(default)]` with a null-as-default deserializer, so a finish chunk with a missing or `null` delta keeps its `finish_reason` and `usage`.
- A data payload with a non-null top-level `error`, or any SSE block with `event: error`, emits one Anthropic `error` event (message/type taken from `error.message` and `error.type`/`error.code`, falling back to `upstream_error`), and the stream stops there with no success terminal.
- New helpers: `close_open_blocks` (closes the text/thinking block, late-starts tool blocks that have pending args and closes all tool blocks; the old inline finish_reason code moved here) and `finalize_message` (synthesizes `message_start` if it was never sent, then emits one `message_delta` and one `message_stop`, guarded by `has_sent_message_stop`). Both are used for `[DONE]` and for EOF.
- `[DONE]` without a prior finish_reason closes the open blocks and finishes with `end_turn` (or `tool_use` if a tool block was started). Data after `message_stop`, such as a repeated `[DONE]`, is ignored.
- EOF without a finish_reason: text/thinking-only output closes its blocks and finishes with `stop_reason: max_tokens` as the truncation signal. If there was no output, or tool arguments were in flight, it emits an `error` event of type `stream_truncated`. Before this change it emitted nothing.

**Behaviour change.** The golden fixture `stream_truncated_without_finish_reason` and its inline test used to require "no terminal events" on EOF. They now pin the truncation finalization (`content_block_stop` + `message_delta` with `max_tokens` + `message_stop`).

**Files.**
- `src-tauri/src/proxy/providers/streaming.rs` (converter + 10 new/updated inline tests)
- `src-tauri/tests/fixtures/proxy/anthropic__openai_chat/` new golden fixtures: `stream_finish_without_delta` (a), `stream_in_band_error_payload` (b), `stream_sse_error_event` (b), `stream_truncated_without_finish_reason` (c, updated), `stream_eof_before_output` (c, early EOF), `stream_done_closes_open_blocks` (d), `stream_bare_done` (d, no message_stop before message_start), `stream_double_done` (e)

**Commits.** `06ff23c` (code + fixtures); follow-up commit sets the fixtures' `source_commit` and records these notes.

**Verification.**
- `cargo test --manifest-path src-tauri/Cargo.toml --lib proxy::providers::streaming::`: 21 passed, 0 failed. The new tests cover (a) no-delta/null-delta finish gives end_turn plus usage, (b) in-band `{error}` and `event: error` give `event: error`, (c) EOF after text gives content_block_stop plus max_tokens, (d) `[DONE]` closes blocks and a bare `[DONE]` emits message_start first, (e) a double `[DONE]` gives exactly one message_stop. The EOF test inverts the old assertion, and the other regression tests exercise paths that were broken before (delta parse failure, swallowed error, unguarded `[DONE]`).
- `INSTA_UPDATE=always cargo test --test proxy_golden`, then reviewed the snapshots and re-ran: 6 passed (all invariants hold, including terminal/block_pairing/message_start).
- `cargo fmt --check`: clean. `cargo clippy -- -D warnings`: clean.
- `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast`: 2309 passed, 0 failed (lib: 2153 passed, 3 ignored).
