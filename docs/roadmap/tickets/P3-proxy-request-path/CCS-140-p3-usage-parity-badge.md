# CCS-140: [usage] Show a parity badge when normalized usage disagrees with the raw upstream usage

**Status:** Open · **Kind:** feature · **Labels:** `feature`, `phase:P3`, `p3-low`, `area:usage`

> **Ticket key:** `p3-usage-parity-badge` · **Phase:** P3 · **Priority:** p3-low · **Effort:** S · **Depends on:** `p3-usage-ir`, `p3-stream-traits-taps`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Double-counted cache_read and zero-usage streams reach users' bills unnoticed.

## Proposed change
Compare the UsageIR billable totals with the raw upstream usage block. Store a mismatch flag on the log row (in an existing JSON metadata column, with no DDL) and show a badge with details.

## Acceptance criteria / verification
- [ ] A fixture with deliberately inconsistent usage sets the flag
- [ ] RTL: the badge renders
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml usage` and `pnpm test:unit`

## Dependencies
p3-usage-ir, p3-stream-traits-taps
