# CCS-008: [proxy-tests] Add golden bridge testkit with chunk-split fuzzing, plus Chat and Gemini fixtures

**Status:** Open · **Kind:** chore · **Labels:** `chore`, `phase:P0`, `p1-high`, `area:proxy-tests`

> **Ticket key:** `p0-golden-testkit-chat-gemini` · **Phase:** P0 · **Priority:** p1-high · **Effort:** M
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Most recent fix commits (6d316c0, 27ce0a5, 650905a, 613fef7, f991726) are in the protocol bridges. Bridge behaviour is pinned only by ad-hoc inline concat!() tests, so behaviour cannot be compared before and after a refactor, and chunk-boundary bugs go unseen.

## Evidence
- Inline cases in src-tauri/src/proxy/providers/streaming.rs and streaming_gemini.rs
- Gemini finishReason mapping drift: streaming_gemini.rs:18 vs transform_gemini.rs:1142

## Proposed change
- Add a `proxy::providers::testkit` module under `cfg(any(test, feature="testkit"))`, plus src-tauri/tests/proxy_golden.rs.
- Fixture layout: tests/fixtures/proxy/<bridge>/<case>/{client_request.json, upstream_request.expected, upstream_response.sse|json, client_response.expected, usage.expected, meta.toml (intent and source commit)}.
- Snapshot with insta, redacting ids and timestamps.
- Re-split every stream input at every byte offset (inputs of 4 KB or less), at 200 random splits, and at UTF-8 and CRLF boundaries.
- Assert structural invariants separately from the snapshots: start/stop pairing, stable tool ids, Chat index order, reasoning before message, exactly one terminal event, usage conservation.
- Port the existing cases for anthropic__openai_chat and anthropic__gemini. Record the Gemini finishReason drift as an expected-diff fixture.

## Acceptance criteria / verification
- [ ] At least 30 fixtures across anthropic__openai_chat and anthropic__gemini, covering stream and non-stream
- [ ] Invariant failures report the split offset
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --test proxy_golden` passes
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml proxy::` still passes (the ~1046 inline tests)
- [ ] `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`

## Dependencies
None.
