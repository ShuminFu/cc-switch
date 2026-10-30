# CCS-185: [proxy] Fail over on errors that arrive before any content is streamed

**Status:** Done · **Kind:** feature · **Labels:** `feature`, `phase:P5`, `p2-medium`, `area:proxy`

> **Ticket key:** `p5-midstream-failover` · **Phase:** P5 · **Priority:** p2-medium · **Effort:** M · **Depends on:** `p5-ir-codec-core`, `p3-attempt-recorder`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Some gateways return 200 and then an immediate error or response.failed.

## Proposed change
- Buffer IR events until the first TextDelta or ToolCall.
- On an early error, discard the buffer and retry the next provider through AttemptRecorder.

## Acceptance criteria / verification
- [x] E2E: P1 returns 200 then an error event before content, so the client receives P2's response with no error
- [x] An error after content is passed through
- [x] `cargo test --manifest-path src-tauri/Cargo.toml --test proxy_e2e`

## Dependencies
p5-ir-codec-core, p3-attempt-recorder

## Implementation notes

**Summary.** I added a stream-start gate to the retry loop (`ResponseValidator::gate_stream_start`). It handles every streaming SSE response except Claude→Responses, which already has its own `validate_responses_stream_start`. The gate works as follows:
- It picks the IR `EventDecoder` for the attempt's upstream wire format (`RoutePlan.upstream`): Anthropic, Chat, Responses or Gemini. The decoder is used only to inspect events, so no side store is written.
- It buffers the raw upstream bytes until one of these events arrives:
  - **First content event** (non-empty text or reasoning, or a tool call): the stream is committed. The buffered bytes are replayed unchanged and the rest streams straight through. Errors after this point reach the client as before, with no failover.
  - **`MessageStop` with no content:** the stream is committed.
  - **`StreamEvent::Error`:** the buffer is dropped and the attempt fails with a retryable `TransformError`. This covers the decoders' "ended before terminal event" errors at EOF. `settle_failure`/`AttemptRecorder` then record a provider failure and the loop tries the next provider.
- It does not interpret JSON bodies (gateways that ignore `stream: true`), events it cannot decode, or anything past 256 KiB. In those cases it commits and leaves the response converters to handle the stream as before.
- It is skipped when failover is off (first-byte timeout 0). With nothing to fail over to, passthrough stays unchanged.
- Each read uses the first-byte timeout.

Reasoning deltas count as content, which differs slightly from "first TextDelta or ToolCall" in the ticket. Buffering long reasoning would leave the client with no output for the whole thinking phase.

**Files:**
- `src-tauri/src/proxy/pipeline/stream_start.rs` (new: the gate plus 9 unit tests)
- `src-tauri/src/proxy/pipeline/validate.rs` (`validate` takes the upstream `WireFormat` and calls the gate)
- `src-tauri/src/proxy/pipeline/mod.rs`
- `src-tauri/src/proxy/forwarder.rs` (passes `plan.upstream`)
- `src-tauri/tests/proxy_e2e.rs` (2 new E2E tests)

**Commits:**
- f7a6206: the change
- the follow-up commit with this note

**Verification:**
- New E2E test `stream_error_before_content_fails_over_to_the_next_provider`. P1 returns 200, then `message_start`, an empty text block and an `error` event. The client receives exactly P2's SSE bytes with no error. Both hits are 1, `failover_count` is 1, the log row is for P2 and P1's health shows 1 failure. With the gate disabled the test fails (checked locally), so it is a real regression test.
- New E2E test `stream_error_after_content_is_passed_through`. The client gets P1's bytes including the error, and P2 is never hit.
- `cargo test --test proxy_e2e`: 29 passed.
- `cargo test --lib stream_start`: 13 passed. These are the 9 new tests for Anthropic, Chat, Responses, split events, EOF before content, JSON body and failover off, plus the existing Responses stream-start tests.
- `cargo test --no-fail-fast`: 2866 passed, 0 failed (lib 2641). `proxy_golden` passed 16/16 with no fixture diffs.
- `cargo fmt --check`: clean.
- `cargo clippy --all-targets -- -D warnings`: clean.
- No frontend changes.
