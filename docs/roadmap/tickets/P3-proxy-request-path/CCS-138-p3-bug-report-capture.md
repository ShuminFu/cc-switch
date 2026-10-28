# CCS-138: [proxy] Capture and export a redacted bug report in the golden-fixture layout

**Status:** Open · **Kind:** feature · **Labels:** `feature`, `phase:P3`, `p2-medium`, `area:proxy`

> **Ticket key:** `p3-bug-report-capture` · **Phase:** P3 · **Priority:** p2-medium · **Effort:** M · **Depends on:** `p3-stream-traits-taps`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Bridge bugs reported by users cannot be replayed. The critique moved this from P2 to P3 because it needs the stream taps.

## Proposed change
- Add an opt-in, time-boxed CaptureTap that records the client request, the upstream request, the upstream SSE with timings and the client SSE, all redacted.
- Add an "Export bug report" button in the request log that writes a zip matching tests/fixtures/proxy/<bridge>/<case>.

## Acceptance criteria / verification
- [ ] Redaction test over a corpus of key patterns (sk-, sk-ant-, AIza, ghu_, Bearer, x-api-key, cookies): zero hits in the export
- [ ] An exported capture replays green through proxy_golden
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml capture` and `pnpm test:unit`

## Dependencies
p3-stream-traits-taps
