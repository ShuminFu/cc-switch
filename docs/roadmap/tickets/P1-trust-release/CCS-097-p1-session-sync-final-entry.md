# CCS-097: [usage] Upgrade imported partial streaming snapshots when the final session entry arrives

**Status:** Open · **Kind:** bug · **Labels:** `bug`, `phase:P1`, `p2-medium`, `area:usage`

> **Ticket key:** `p1-session-sync-final-entry` · **Phase:** P1 · **Priority:** p2-medium · **Effort:** S · **Depends on:** `p1-session-sync-partial-line`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding backend-session-usage-dedup. Dedup by message.id happens only within one pass. If a periodic sync imported an early snapshot (stop_reason null, partial output_tokens), the final entry is dropped by should_skip_session_insert and INSERT OR IGNORE. Output tokens and cost stay undercounted permanently.

## Evidence
- src-tauri/src/services/session_usage.rs:186-211, 297-316, 323-330, 345-353, 432-443, ~479; usage_stats.rs:352-361

## Proposed change
- When a `session:<id>` row exists with data_source=session_log, UPDATE the token and cost columns if the new entry has a stop_reason and the old one did not, or if it has more output_tokens.
- Detect a non-final row by stop_reason stored in an existing metadata column if one exists. Otherwise compare output_tokens only (no DDL in P1).

## Acceptance criteria / verification
- [ ] Test: msg_1 with output 1 and stop null, synced; then output 500 and end_turn, synced. The row has output_tokens=500, recomputed cost, and count 1.
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml session_usage`

## Dependencies
p1-session-sync-partial-line
