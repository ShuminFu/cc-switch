# CCS-174: [proxy] Define the canonical protocol IR and codec traits

**Status:** Open · **Kind:** refactor · **Labels:** `refactor`, `phase:P5`, `p2-medium`, `area:proxy`

> **Ticket key:** `p5-ir-codec-core` · **Phase:** P5 · **Priority:** p2-medium · **Effort:** M · **Depends on:** `p3-stream-traits-taps`, `p3-stop-reason-table`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Proposed change
- Add proxy/protocol/ir.rs (ChatRequest, Part, StreamEvent), reusing UsageIR and StopReason.
- Add codec traits per format with request, response and stream parts. Non-stream is stream plus aggregate.
- Add a `proxy.codecEngine=legacy` flag.

## Acceptance criteria / verification
- [ ] IR unit tests; proptest for round-trip where the formats are lossless
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml protocol::ir`

## Dependencies
p3-stream-traits-taps, p3-stop-reason-table
