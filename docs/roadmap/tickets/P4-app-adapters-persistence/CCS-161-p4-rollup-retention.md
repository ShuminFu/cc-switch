# CCS-161: [db] Store rollup cost as integer micro-USD and add RetentionPolicy with Maintenance::run

**Status:** Done · **Kind:** refactor · **Labels:** `refactor`, `phase:P4`, `p3-low`, `area:database`

> **Ticket key:** `p4-rollup-retention` · **Phase:** P4 · **Priority:** p3-low · **Effort:** M · **Depends on:** `p4-usage-repo`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Evidence
- database/mod.rs:148 and backup.rs:271 hard-coded retention literals; floating-point rollups

## Proposed change
- Migration to micro-USD integers for rollups.
- RetentionPolicy {detail_days, rollup_days, backups_keep} in settings, with Maintenance::run on a schedule.

## Acceptance criteria / verification
- [x] Rollup totals equal the sum of the detail rows exactly (test)
- [x] Retention prunes per the policy (test)
- [x] `cargo test --manifest-path src-tauri/Cargo.toml maintenance`

## Dependencies
p4-usage-repo

## Implementation notes

**Summary**
- Schema v20 (`database/migrations/v020.rs`, additive, no `DB_COMPAT_VERSION` bump): adds
  `usage_daily_rollups.total_cost_micros INTEGER NOT NULL DEFAULT 0`, backfilled from the
  existing `total_cost_usd` text (rounded to micro-USD). The text column keeps its values.
- `rollup_and_prune` converts each detail row to integer micro-USD and sums integers
  (`sql_helpers::cost_micros_sql`), merging with existing rollups by integer addition, so a
  rollup equals the sum of its detail rows exactly (the old REAL `SUM` gave e.g.
  `0.9999999999999999` for 10 × `0.1`). `total_cost_usd` is still written, as the exact
  decimal text of the micros (`micros_to_usd_text_sql`).
- Dashboard / limit queries in `repo/usage.rs` read rollup cost from `total_cost_micros`
  (`rollup_cost_usd_sum_sql`, `PROVIDER_LIMIT_WINDOW_SQL`).
- `settings::RetentionPolicy { detail_days, rollup_days, backups_keep }`, resolved by
  `RetentionPolicy::from_settings` / `effective_retention_policy()` from new settings
  `usageDetailRetentionDays` (default 30, min 1), `usageRollupRetentionDays` (default / 0 =
  keep forever, never below `detail_days`) and the existing `backupRetainCount` (default 10,
  min 1; `effective_backup_retain_count` now delegates to the policy).
- `database::Maintenance::run(db, &policy)` (`database/maintenance.rs`) replaces the
  hard-coded literals in `Database::init` and `periodic_backup_if_needed` (the daily timer):
  stream-check log cleanup (7 days, now a named const), detail rollup per `detail_days`,
  rollup pruning per `rollup_days` (new `Database::prune_old_rollups`), backup pruning per
  `backups_keep` (`cleanup_db_backups` now takes the count), incremental vacuum when rows
  were deleted. Steps fail independently with a warning; it returns a `MaintenanceReport`.
- New v20 migration fixture `src-tauri/tests/fixtures/db/v20.sql` (generated with
  `scripts/gen-db-fixture.sh 595294d`, `CCS_FIXTURE_LOCKFILE=current`); the generator's
  sample rollup row now sets `total_cost_micros`.
- Not done here (frontend lane): no settings UI or TS type for the two new retention
  settings; they work from `settings.json` and keep defaults until a UI is added.

**Files**
- `src-tauri/src/database/{maintenance.rs (new), mod.rs, backup.rs}`
- `src-tauri/src/database/migrations/{v020.rs (new), mod.rs, tests.rs}`
- `src-tauri/src/database/dao/usage_rollup.rs`, `src-tauri/src/database/repo/usage.rs`
- `src-tauri/src/services/{sql_helpers.rs, usage_stats.rs}`, `src-tauri/src/settings.rs`
- `src-tauri/tests/fixtures/db/v20.sql` (new), `scripts/gen-db-fixture.sh`

**Commits**
- `595294d` refactor(db): integer micro-USD rollup cost and RetentionPolicy with Maintenance::run
- follow-up commit: v20 DB fixture and these notes

**Tests**
- `dao::usage_rollup::tests::rollup_cost_equals_sum_of_detail_rows_exactly` (regression:
  10 × `0.1` plus sub-cent rows; rollup micros == detail micros sum, text `2.567903`,
  merge adds exactly)
- `dao::usage_rollup::tests::rollup_cutoff_keeps_the_last_retain_days_local_days`
- `database::maintenance::tests::{maintenance_prunes_per_retention_policy,
  maintenance_keeps_rollups_forever_without_rollup_days, maintenance_honours_detail_days}`
- `migrations::tests::migrate_v19_to_v20_backfills_rollup_cost_micros`
- `settings::tests::retention_policy_defaults_and_bounds`
- `sql_helpers::tests::cost_micros_round_trip_through_text`; `usage_stats` rollup fixtures
  now set `total_cost_micros`, and the summary test asserts the rollup cost.

**Verification**
- `cargo fmt --manifest-path src-tauri/Cargo.toml --check`: clean
- `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` and `--all-targets`: clean
- `cargo test --manifest-path src-tauri/Cargo.toml maintenance`: 4 passed
- `cargo test --manifest-path src-tauri/Cargo.toml --test migrations`: 14 passed (includes
  the v20 fixture and `db_fixtures_cover_every_schema_version`)
- `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast`: 2615 passed, 0 failed

**Review**
- Re-checked both acceptance criteria against the code: every rollup reader in `repo/usage.rs`
  (5 dashboard queries + `PROVIDER_LIMIT_WINDOW_SQL`) reads `total_cost_micros`; the only rollup
  writer is `rollup_and_prune`; SQL import / sync import run migrations, so older dumps get the
  v20 backfill; unknown settings keys round-trip through the frontend save (spread payload).
- Fixed: the `maintenance_prunes_per_retention_policy` helper set backup mtimes through a
  read-only `File::open` handle, which Windows rejects (setting file times needs
  `FILE_WRITE_ATTRIBUTES`), so the test would fail on the `windows-latest` CI job. It now writes
  and stamps the file through one `File::create` handle (`OpenOptions` is banned by
  `tests/write_lint.rs`).
- Re-verified: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
  `cargo test maintenance` (4 passed), full `cargo test --no-fail-fast` (2615 passed, 0 failed).
