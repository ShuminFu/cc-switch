# CCS-185: [proxy] Fail over on errors that arrive before any content is streamed

**Status:** Open · **Kind:** feature · **Labels:** `feature`, `phase:P5`, `p2-medium`, `area:proxy`

> **Ticket key:** `p5-midstream-failover` · **Phase:** P5 · **Priority:** p2-medium · **Effort:** M · **Depends on:** `p5-ir-codec-core`, `p3-attempt-recorder`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Some gateways return 200 and then an immediate error or response.failed.

## Proposed change
- Buffer IR events until the first TextDelta or ToolCall.
- On an early error, discard the buffer and retry the next provider through AttemptRecorder.

## Acceptance criteria / verification
- [ ] E2E: P1 returns 200 then an error event before content, so the client receives P2's response with no error
- [ ] An error after content is passed through
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --test proxy_e2e`

## Dependencies
p5-ir-codec-core, p3-attempt-recorder
