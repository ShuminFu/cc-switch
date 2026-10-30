# CCS-136: [proxy] Replace the three SSE sniffers with one SseDecoder

**Status:** Done · **Kind:** refactor · **Labels:** `refactor`, `phase:P3`, `p2-medium`, `area:proxy`

> **Ticket key:** `p3-sse-decoder` · **Phase:** P3 · **Priority:** p2-medium · **Effort:** M · **Depends on:** `p3-route-plan`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
SSE framing is implemented three times, with different chunk-boundary bugs.

## Evidence
- streaming_responses.rs:336, forwarder.rs:2882, handlers.rs:1910

## Proposed change
Add proxy/stream/sse_decoder.rs (CRLF, multi-line data, comments, event:) and a proptest that output is identical under any chunking.

## Acceptance criteria / verification
- [x] The three sites use SseDecoder; the proptest passes; golden fixtures are unchanged
- [x] `cargo test --manifest-path src-tauri/Cargo.toml stream::`

## Dependencies
p3-route-plan

## Implementation notes

**Summary.** Added `proxy::stream::SseDecoder` (`src-tauri/src/proxy/stream/sse_decoder.rs`), an incremental SSE decoder that follows the WHATWG `text/event-stream` rules. It handles LF, CRLF and lone-CR line endings; a `\r` at the end of a chunk waits for the next chunk. It also handles a blank line that dispatches only events with data, `data` lines joined with `\n`, `:` comments, `event`/`id`/`retry`, a single space stripped after the colon, a field with no colon, a leading BOM, and UTF-8 split across chunks (through `append_utf8_safe`). Two leniencies the proxy already had are kept: indented field names (C4) and trimmed `event` values. `feed(&[u8])`/`feed_str(&str)` return the events that are complete; `finish()` flushes an unterminated trailing event. The callers use that flush as their non-strict tail path.

The three sites from the ticket's evidence now use it:
- `providers/streaming_responses.rs`: the Responses-to-Anthropic stream converter. The whole-JSON fallback is now decided once, from the first non-whitespace character, before SSE decoding starts.
- `pipeline/validate.rs`, `validate_responses_stream_start` / `inspect_responses_start_event`: this is the old `forwarder.rs:2882` site, which moved there in CCS-130. It now keeps a raw JSON probe until the body is ruled out as JSON, so a pretty-printed JSON body is no longer framed as SSE when a chunk ends right after a blank line.
- `handlers.rs`, `responses_sse_to_response_value` and `chat_sse_to_response_value`: `sse_block_parts` was removed.

Bugs fixed: events were silently lost under lone-CR or `\n\r\n` (LF line followed by a CRLF blank line) framing, and validate dropped a JSON document that was split at a blank line.

`proxy::sse::{take_sse_block, strip_sse_field}` stay in place for the other stream converters (`streaming.rs`, `streaming_gemini.rs`, `streaming_codex_*`, `codex_chat_history.rs`, `response_processor.rs`, `testkit.rs`). They are outside this ticket's three sites; migrating them is a mechanical follow-up.

**Files.** `src-tauri/src/proxy/stream/{mod.rs,sse_decoder.rs}` (new), `src-tauri/src/proxy/mod.rs`, `src-tauri/src/proxy/handlers.rs`, `src-tauri/src/proxy/pipeline/validate.rs`, `src-tauri/src/proxy/providers/streaming_responses.rs`.

**Commits.** `4da7928` (code and tests); the follow-up commit records these notes.

**Tests.**
- `stream::sse_decoder::tests` (12 tests), including the proptest `output_is_identical_under_any_chunking`. It generates random events (comments, event/id fields, multi-line data with CJK, emoji, BOM, colons and spaces; LF/CRLF/CR endings; optional BOM; optionally unterminated last event) and checks that decoding in one chunk, at random cut points, and byte by byte all equal the expected events.
- Regression tests that fail with the old `take_sse_block` framing: `handlers::tests::sse_aggregators_handle_cr_only_and_mixed_line_endings`, `streaming_responses::tests::test_streaming_responses_mixed_line_endings_split_byte_by_byte`, `validate::tests::responses_stream_start_keeps_pretty_json_split_at_blank_line` and `validate::tests::responses_stream_start_decodes_cr_framed_events_across_chunks`.
- The golden fixtures (`tests/fixtures/**`, wire golden) are unchanged and still pass.

**Verification.**
- `cargo fmt --manifest-path src-tauri/Cargo.toml --check`: clean.
- `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` and `--all-targets -- -D warnings`: clean.
- `cargo test --manifest-path src-tauri/Cargo.toml stream::`: 12 passed.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib -- stream:: validate:: handlers:: streaming_responses`: 96 passed.
- `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast`: 2482 passed, 0 failed (lib 2316 passed, 3 ignored).
