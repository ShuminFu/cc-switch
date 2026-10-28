# CCS-010: [usage] Add a cost golden table that runs every cost path

**Status:** Open · **Kind:** chore · **Labels:** `chore`, `phase:P0`, `p1-high`, `area:usage-tests`

> **Ticket key:** `p0-cost-golden-table` · **Phase:** P0 · **Priority:** p1-high · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
The cache-inclusive/exclusive input rule is copied in 5 places that disagree. The grokbuild backfill computes a different cost from ingest. The 1-hour TTL accounting was shipped and reverted (13e7c1f/6eb217b).

## Evidence
- src-tauri/src/proxy/usage/calculator.rs:62, proxy/usage/logger.rs:75, database sql_helpers.rs:19 (fresh_input_sql), usage_stats.rs:377 and :1783-1871 (backfill)

## Proposed change
Add a table-driven test with rows of (app_type x {LEGACY, cache-inclusive, cache-exclusive} x usage x pricing row) -> expected cost, run through all four paths. Mark the failing grokbuild backfill rows #[ignore] with an issue link.

## Acceptance criteria / verification
- [ ] Covers all 8 apps x 3 semantics
- [ ] The only failures are the #[ignore] grokbuild rows
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml cost_golden`

## Dependencies
None.
