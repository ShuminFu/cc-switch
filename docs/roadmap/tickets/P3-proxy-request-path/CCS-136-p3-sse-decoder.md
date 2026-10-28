# CCS-136: [proxy] Replace the three SSE sniffers with one SseDecoder

**Status:** Open · **Kind:** refactor · **Labels:** `refactor`, `phase:P3`, `p2-medium`, `area:proxy`

> **Ticket key:** `p3-sse-decoder` · **Phase:** P3 · **Priority:** p2-medium · **Effort:** M · **Depends on:** `p3-route-plan`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
SSE framing is implemented three times, with different chunk-boundary bugs.

## Evidence
- streaming_responses.rs:336, forwarder.rs:2882, handlers.rs:1910

## Proposed change
Add proxy/stream/sse_decoder.rs (CRLF, multi-line data, comments, event:) and a proptest that output is identical under any chunking.

## Acceptance criteria / verification
- [ ] The three sites use SseDecoder; the proptest passes; golden fixtures are unchanged
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml stream::`

## Dependencies
p3-route-plan
