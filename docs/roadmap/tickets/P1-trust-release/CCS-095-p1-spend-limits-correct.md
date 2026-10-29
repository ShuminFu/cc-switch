# CCS-095: [usage] Fix spend-limit windows, add an index and surface DB errors

**Status:** Done · **Kind:** bug · **Labels:** `bug`, `phase:P1`, `p2-medium`, `area:usage`

> **Ticket key:** `p1-spend-limits-correct` · **Phase:** P1 · **Priority:** p2-medium · **Effort:** S · **Depends on:** `p1-db-compat-policy`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
check_provider_limits computes its day/month windows in SQL and returns $0 when a DB error occurs. The critique notes this is a UI-only check (it is never enforced in the proxy; see p4-spend-cap-enforcement).

## Evidence
- src-tauri/src/services/usage_stats.rs:1599-1680 (:1648, :1667 unwrap_or(0.0)); commands/usage.rs:257

## Proposed change
- Compute [start,end) in Rust with compute_local_midnight_cutoff.
- Add `CREATE INDEX IF NOT EXISTS` on proxy_request_logs(provider_id, app_type, created_at). It is additive, with no version bump, per the compatibility policy.
- Propagate errors.

## Acceptance criteria / verification
- [x] EXPLAIN QUERY PLAN shows an index seek
- [x] Test: a DB error produces an Err, not 0.0
- [x] Test: a boundary row at midnight lands in the correct day
- [x] `cargo test --manifest-path src-tauri/Cargo.toml usage_stats`

## Dependencies
p1-db-compat-policy

## Implementation notes

**Summary**
- `Database::check_provider_limits` now delegates to `check_provider_limits_at(provider_id, app_type, now)`. The day window is `[local_midnight(today), local_midnight(tomorrow))` and the month window is `[local_midnight(1st of month), local_midnight(1st of next month))`, both computed in Rust. The SQL only compares `created_at >= ? AND created_at < ?` on detail rows and `date >= ? AND date < ?` on rollup rows (shared `PROVIDER_LIMIT_WINDOW_SQL`).
- The midnight logic of `compute_local_midnight_cutoff` was split out into `local_midnight_timestamp(day)` (same DST handling: earlier instant when ambiguous, +1h in a gap). `compute_local_midnight_cutoff` calls it, and it is re-exported as `crate::database::local_midnight_timestamp`. Windows are built from calendar dates, not `now - N days`, so a 23h/25h DST day cannot shift them.
- An additive `CREATE INDEX IF NOT EXISTS idx_request_logs_provider_created_at ON proxy_request_logs(provider_id, app_type, created_at)` was added in `create_tables_on_conn`, which runs on every startup, so existing databases get it too. Per the ticket there is no `SCHEMA_VERSION` bump.
- Errors are propagated: a failure in either sum query, or in the provider meta lookup, now returns `AppError::Database` instead of `0.0`. A missing provider row, or meta that is not valid JSON, still means "no limits configured".
- Unchanged: this is still a UI-only check. Proxy enforcement is tracked in p4-spend-cap-enforcement.

**Files**
- `src-tauri/src/services/usage_stats.rs`: windows, error propagation, tests
- `src-tauri/src/database/dao/usage_rollup.rs`: `local_midnight_timestamp` helper
- `src-tauri/src/database/mod.rs`: re-export
- `src-tauri/src/database/schema.rs`: new index

**Commits:** `540b2b7` (code + tests); this ticket update follows it.

**Tests added** (`services::usage_stats::tests`)
- `test_check_provider_limits_midnight_boundaries`: a row at exactly 00:00:00 counts toward today, a row at 23:59:59 the day before does not, the next midnight is excluded, and the month edges (1st 00:00:00, previous month's last second, next month's 1st) plus rollup dates are bucketed correctly. It also re-evaluates at 23:59:59 the day before.
- `test_check_provider_limits_propagates_db_errors`: dropping `usage_daily_rollups` gives `Err(AppError::Database)`. The old code returned `Ok` with `0.000000`.
- `test_check_provider_limits_query_uses_provider_created_at_index`: `EXPLAIN QUERY PLAN` on the real query shows `SEARCH proxy_request_logs USING INDEX idx_request_logs_provider_created_at (provider_id=? AND app_type=? AND created_at>? AND created_at<?)` and no table scan. Without the new index, SQLite picks `idx_request_logs_provider (provider_id=? AND app_type=?)`, so the test fails.

**Verification**
- `cargo fmt --manifest-path src-tauri/Cargo.toml --check`: clean
- `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`: clean
- `cargo test --manifest-path src-tauri/Cargo.toml check_provider_limits`: 3 passed. Also passed with `TZ=America/New_York`, `TZ=Asia/Kolkata` and `TZ=Pacific/Auckland`.
- `cargo test --manifest-path src-tauri/Cargo.toml usage_stats`: 32 passed
- `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast`: everything passed (lib 2266 passed, 2 ignored; all integration test binaries ok; 0 failed)
