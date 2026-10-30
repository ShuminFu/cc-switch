# CCS-097: [usage] Upgrade imported partial streaming snapshots when the final session entry arrives

**Status:** Done · **Kind:** bug · **Labels:** `bug`, `phase:P1`, `p2-medium`, `area:usage`

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
- [x] Test: msg_1 with output 1 and stop null, synced; then output 500 and end_turn, synced. The row has output_tokens=500, recomputed cost, and count 1.
- [x] `cargo test --manifest-path src-tauri/Cargo.toml session_usage`

## Dependencies
p1-session-sync-partial-line

## Implementation notes
- **Summary:** `insert_session_log_entry` (`src-tauri/src/services/session_usage.rs`) now looks up an existing `session:<id>` row with `data_source = 'session_log'` before the dedup check. If the new entry has more `output_tokens`, it UPDATEs the token columns and recomputed cost columns in place. The UPDATE is guarded by `output_tokens < new`, so the row count stays at 1. A snapshot with equal or smaller output is skipped as before, so rows never get downgraded. `proxy_request_logs` has no column that stores stop_reason, so as the ticket allows, "is this the newer snapshot" is decided by output_tokens alone (no DDL). `created_at` is kept as originally stored. A proxy row that duplicates the upgraded row is still hidden at query time by `effective_usage_log_filter`.
- **Files:** `src-tauri/src/services/session_usage.rs` (fix plus regression test `test_sync_upgrades_partial_snapshot_when_final_entry_arrives`).
- **Commits:** da380f3 (code + test); the ticket update is a follow-up docs commit.
- **Verification:**
  - Regression test: msg_1 synced with output 1 and stop null, then with output 500 and end_turn. Result: output_tokens=500, output and total cost equal `CostCalculator` for the final usage, and 1 row. A stale snapshot inserted afterwards does not downgrade the row. With the upgrade branch disabled the test fails, and it passes with the fix.
  - `cargo test --manifest-path src-tauri/Cargo.toml --lib session_usage`: 51 passed / 0 failed.
  - `cargo fmt --check`: clean. `cargo clippy -- -D warnings`: clean.
  - `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast`: 2436 passed / 0 failed.
