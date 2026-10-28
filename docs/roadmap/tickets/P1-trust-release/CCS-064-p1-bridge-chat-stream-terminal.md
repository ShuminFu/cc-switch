# CCS-064: [bridge] Port the terminal state machine to the Chat->Anthropic stream converter

**Status:** Open · **Kind:** bug · **Labels:** `bug`, `phase:P1`, `p2-medium`, `area:proxy-bridge`

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
- [ ] Tests: (a) no-delta finish_stop gives stop_reason end_turn; (b) an error payload gives event: error; (c) EOF after text gives content_block_stop plus a truncation signal; (d) [DONE] closes blocks; (e) a double [DONE] gives exactly one message_stop
- [ ] Golden fixtures added for each
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml proxy::providers::streaming`

## Dependencies
p0-golden-testkit-chat-gemini
