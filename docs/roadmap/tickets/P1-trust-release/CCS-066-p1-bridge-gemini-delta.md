# CCS-066: [bridge] Decide Gemini cumulative vs incremental mode once per stream

**Status:** Done · **Kind:** bug · **Labels:** `bug`, `phase:P1`, `p3-low`, `area:proxy-bridge`

> **Ticket key:** `p1-bridge-gemini-delta` · **Phase:** P1 · **Priority:** p3-low · **Effort:** S · **Depends on:** `p0-golden-testkit-chat-gemini`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding proxy-gemini-delta-drop. Classification runs on every chunk, so incremental chunks that start with all prior text ("ha" then "haha") lose text, and identical chunks are dropped.

## Evidence
- src-tauri/src/proxy/providers/streaming_gemini.rs:356-365, 394-398

## Proposed change
Track `mode: Option<bool>` per stream. Set it at the first unambiguous chunk and default to incremental.

## Acceptance criteria / verification
- [x] Test: incremental "ha","haha","ha" gives "hahahaha"; cumulative "ha","haha","hahaha" gives "hahaha"
- [x] `cargo test --manifest-path src-tauri/Cargo.toml streaming_gemini`

## Dependencies
p0-golden-testkit-chat-gemini

## Implementation notes

**Summary.** `create_anthropic_sse_stream_from_gemini` no longer classifies each
chunk as cumulative/incremental on its own. A new per-stream
`GeminiTextDeltaTracker` (`mode: Option<GeminiTextMode>`) decides once:

- a chunk that does not start with the text seen so far locks `Incremental`;
- a chunk that does (longer or identical snapshot) is ambiguous and is held
  back; if the next chunk starts with the held snapshot the stream locks
  `Cumulative`, otherwise it locks `Incremental` and the held chunk is
  released verbatim;
- a chunk still held at end of stream resolves as a cumulative snapshot.

Deviation from the proposed "default to incremental": a pure default would
break real cumulative relays (`"Hel","Hello"`, and the golden fixture
`anthropic__gemini/stream_text_then_function_call`, which repeats the final
snapshot with `finishReason`). Identical chunks are kept verbatim once
incremental mode is locked; a stream whose only evidence is "every chunk
starts with all prior text" is treated as cumulative. Text-block SSE emission
was factored into `text_delta_events` so the end-of-stream flush shares it.

**Files.** `src-tauri/src/proxy/providers/streaming_gemini.rs` (tracker,
emission helper, 6 new tests).

**Commits.** `8584c40` (code + tests); ticket notes in the follow-up commit.

**Verification.**
- New tests: `incremental_chunks_that_repeat_prior_text_are_not_dropped`
  (`"ha","haha","ha"` -> `"hahahaha"`, fails on the old per-chunk logic which
  produced `"hahaha"`), `cumulative_snapshots_emit_only_new_text`
  (`"ha","haha","hahaha"` -> `"hahaha"`),
  `identical_chunks_are_kept_once_incremental_mode_is_locked`,
  `repeated_final_cumulative_snapshot_is_not_duplicated`,
  `mode_is_locked_after_first_unambiguous_chunk`,
  `held_snapshot_is_released_at_end_of_stream`.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib streaming_gemini`: 20 passed, 0 failed.
- `cargo test --manifest-path src-tauri/Cargo.toml --test proxy_golden`: 6 passed (golden snapshots unchanged).
- `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast`: 2320 passed, 0 failed.
- `cargo fmt` clean; `cargo clippy --all-targets -- -D warnings` clean.
