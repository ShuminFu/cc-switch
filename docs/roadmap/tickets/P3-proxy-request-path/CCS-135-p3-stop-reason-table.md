# CCS-135: [proxy] Add a canonical StopReason table and unify the Gemini mapping

**Status:** Open · **Kind:** refactor · **Labels:** `refactor`, `phase:P3`, `p2-medium`, `area:proxy`

> **Ticket key:** `p3-stop-reason-table` · **Phase:** P3 · **Priority:** p2-medium · **Effort:** S · **Depends on:** `p3-usage-ir`, `p1-bridge-stop-reason-tool-use`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
The stream and non-stream paths map stop reasons differently. The Gemini finishReason drift is pinned as an expected diff.

## Evidence
- streaming_gemini.rs:18 vs transform_gemini.rs:1142; streaming.rs:701-715; transform.rs:645-659

## Proposed change
- One StopReason enum with a table per format, including the tool_use override from P1.
- The Gemini unification ships as its own PR and updates the expected-diff fixture.

## Acceptance criteria / verification
- [ ] For every bridge, the golden stream and non-stream outputs return the same stop_reason
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --test proxy_golden`

## Dependencies
p3-usage-ir, p1-bridge-stop-reason-tool-use
