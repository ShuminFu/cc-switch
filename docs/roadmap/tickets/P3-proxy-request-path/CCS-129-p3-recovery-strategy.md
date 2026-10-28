# CCS-129: [proxy] Replace nested forward() retries with a RecoveryStrategy list

**Status:** Open · **Kind:** refactor · **Labels:** `refactor`, `phase:P3`, `p2-medium`, `area:proxy`

> **Ticket key:** `p3-recovery-strategy` · **Phase:** P3 · **Priority:** p2-medium · **Effort:** M · **Depends on:** `p3-attempt-recorder`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
The rectifiers retry by calling forward() again from inside it, so forward() has several call sites.

## Evidence
- forwarder.rs:579, 725, 891

## Proposed change
Add pipeline/recovery.rs with RecoveryStrategy {MediaDowngrade, ThinkingSignatureRectifier, ThinkingBudgetRectifier}. Each is applied at most once and is visible in the trace.

## Acceptance criteria / verification
- [ ] forward() has exactly one call site
- [ ] A proxy_e2e scenario for each rectifier passes
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --test proxy_e2e`

## Dependencies
p3-attempt-recorder
