# CCS-137: [proxy] Wrap converters behind StreamDecoder/Encoder, and add UsageTap, TimingTap and aggregate

**Status:** Open · **Kind:** refactor · **Labels:** `refactor`, `phase:P3`, `p2-medium`, `area:proxy`

> **Ticket key:** `p3-stream-traits-taps` · **Phase:** P3 · **Priority:** p2-medium · **Effort:** L · **Depends on:** `p3-sse-decoder`, `p3-usage-ir`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Usage is re-parsed from a growing Vec of events, and non-stream aggregation is duplicated.

## Evidence
- response_processor.rs:677-790; handlers.rs:1814, 2022

## Proposed change
- Put the existing converters behind StreamDecoder/StreamEncoder traits without rewriting them.
- UsageTap keeps running totals; TimingTap records TTFB and first token.
- Add stream/aggregate.rs.

## Acceptance criteria / verification
- [ ] UsageTap memory is constant in the event count (test with 100k events)
- [ ] Golden fixtures are identical; handlers.rs is 1200 lines or fewer
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --test proxy_golden --test proxy_e2e`

## Dependencies
p3-sse-decoder, p3-usage-ir
