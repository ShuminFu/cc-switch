# CCS-092: [usage] Make AppType::input_semantics() the single source and fix the grokbuild backfill cost

**Status:** Open · **Kind:** bug · **Labels:** `bug`, `phase:P1`, `p1-high`, `area:usage`

> **Ticket key:** `p1-usage-input-semantics` · **Phase:** P1 · **Priority:** p1-high · **Effort:** S · **Depends on:** `p0-cost-golden-table`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
The cache-inclusive/exclusive input rule is copied in 5 places that disagree, so the grokbuild backfill computes a different cost from ingest.

## Evidence
- proxy/usage/calculator.rs:62, proxy/usage/logger.rs:75, sql_helpers.rs:19, usage_stats.rs:377 and :1820

## Proposed change
Add `AppType::input_semantics()` and have all 5 sites call it. The SQL helper generates its CASE from the enum.

## Acceptance criteria / verification
- [ ] The cost golden table is fully green, with the #[ignore] removed from the grokbuild rows
- [ ] `grep -rn 'cache-inclusive\|CACHE_INCLUSIVE' src-tauri/src` shows a single definition
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml cost_golden`

## Dependencies
p0-cost-golden-table
