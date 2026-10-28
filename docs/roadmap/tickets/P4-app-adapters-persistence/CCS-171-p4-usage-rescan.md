# CCS-171: [usage] Add a usage re-scan action and a per-source data-quality report

**Status:** Open · **Kind:** feature · **Labels:** `feature`, `phase:P4`, `p3-low`, `area:usage`

> **Ticket key:** `p4-usage-rescan` · **Phase:** P4 · **Priority:** p3-low · **Effort:** M · **Depends on:** `p4-usage-repo`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Backend idea 7. Existing rows that were undercounted before the P1 fixes cannot be repaired.

## Proposed change
- Add a "re-scan session logs" action that resets the cursors and re-imports from offset 0 with upsert semantics, in the background with progress.
- Add a data-quality report via get_data_source_breakdown.

## Acceptance criteria / verification
- [ ] Test: a DB with an undercounted row is corrected after a re-scan, with no duplicates
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml session_usage`

## Dependencies
p4-usage-repo
