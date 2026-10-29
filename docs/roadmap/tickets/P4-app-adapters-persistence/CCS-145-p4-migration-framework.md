# CCS-145: [db] Add a versioned migration framework with a frozen baseline and audit table

**Status:** Done · **Kind:** refactor · **Labels:** `refactor`, `phase:P4`, `p1-high`, `area:database`

> **Ticket key:** `p4-migration-framework` · **Phase:** P4 · **Priority:** p1-high · **Effort:** M · **Depends on:** `p0-migration-harness`, `p1-db-compat-policy`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem

Migrations live in a user_version loop, 8 ALTERs swallow their errors, and create_tables does repairs. Per the critique this must land before any schema change, including the tombstone that was moved out of P1.

## Evidence

- src-tauri/src/database/schema.rs (~:430 loop, create_tables repairs)

## Proposed change

- Add database/migrations/ with an ordered MIGRATIONS table, a frozen V1 baseline and a schema_migrations audit table.
- Turn the swallowed ALTERs into numbered steps that propagate errors, and delete create_tables.
- Require each migration to declare a DB_COMPAT_VERSION impact, following p1-db-compat-policy.

## Acceptance criteria / verification

- [x] A CI test asserts MIGRATIONS is contiguous and ends at SCHEMA_VERSION
- [x] The migration harness passes from every vN and from JSON; fresh and migrated DBs are equivalent
- [x] `cargo test --manifest-path src-tauri/Cargo.toml --test migrations`

## Dependencies

p0-migration-harness, p1-db-compat-policy

## Implementation notes

**Summary**

- The new `src-tauri/src/database/migrations/` replaces the `user_version` match loop and `create_tables`.
  - `mod.rs` holds the ordered `MIGRATIONS` table (`Migration { version, name, db_compat, up }`) and the runner `apply_schema_migrations_on_conn`. The runner uses one savepoint, applies each step with `version > user_version`, sets `user_version` after each step, and rolls back the whole run on any error. Errors carry the failing step (`数据库迁移 vN -> vN+1 失败: …`).
  - `baseline.rs` is the frozen v1 baseline: the seven v3.8.3 tables, identical to `tests/fixtures/db/v1.sql`, plus the v0 column top-up. A new database (v0 with no tables) now runs every step from v0. `Database::init`, `Database::memory`, SQL import, backup restore and the JSON dry run all go through the same chain.
  - `legacy.rs` holds the released v2–v15 steps, moved from `schema.rs`.
  - `v016.rs` holds the new v16 step.
- `create_tables` is deleted. Its work moved into numbered steps that propagate errors:
  - **v2** now creates the tables and indexes it introduced (`provider_health`, `stream_check_logs`, `proxy_live_backup`, the request-log and stream-check indexes, and per-app `proxy_config` with its default rows). The swallowed proxy_config `ALTER`s are `add_column_if_missing` calls, including `live_takeover_active`.
  - **v3** propagates its settings writes. It no longer sets `skills_ssot_migration_pending` on a fresh database (`MigrationContext::fresh_database`), so a new install still does not auto-import app skill folders, as before.
  - **v14** first converts a beta-v2 singleton `proxy_config`, which `create_tables` used to repair on every start.
  - **v16 (new, additive)** handles the repairs that released v15 databases still need: the CCS-095 spend-limit index `idx_request_logs_provider_created_at` (missing in the v15 fixture), the usage indexes that v8/v9 releases never created, the dev `current_profile_id` marker repair, and the `proxy_config` default rows. `SCHEMA_VERSION` is now 16.
- Every step declares a `DbCompatImpact`:
  - `Additive`
  - `Breaking { db_compat_version }`
  - `PrePolicy`, which may only be used for v1–v15 steps (v2, v3, v6, v11 and v14 are non-additive steps released before CCS-091).
    The v16 step is `Additive`, so `DB_COMPAT_VERSION` stays 6.
- The runner keeps a `schema_migrations` audit table (version, name, db_compat, app_version, applied_at) and adds one row per applied step. The table is device-local: it was added to `SYNC_SKIP_TABLES` and `SYNC_PRESERVE_TABLES`, so sync neither uploads it nor overwrites it. `docs/dev/DB_COMPATIBILITY.md` was updated to describe the framework and the `db_compat` declaration.
- New fixture `tests/fixtures/db/v16.sql`, generated with `scripts/gen-db-fixture.sh` (`CCS_FIXTURE_LOCKFILE=current`). It was generated from a temporary commit of the same source tree, and its header was then pointed at `7eaea66`, which has an identical `src-tauri/src` tree.

**Files**

- `src-tauri/src/database/migrations/{mod,baseline,legacy,v016,tests}.rs` (new)
- `src-tauri/src/database/{mod,schema,backup,migration,tests}.rs`
- `src-tauri/src/lib.rs` (comment only)
- `src-tauri/tests/migrations.rs`
- `src-tauri/tests/fixtures/db/v16.sql`
- `docs/dev/DB_COMPATIBILITY.md`

**Commits**

- `7eaea66` refactor(db): versioned migration framework with frozen v1 baseline (CCS-145)
- This ticket update follows as a docs commit, together with the v16 fixture header pointing at `7eaea66`.

**Tests**

- `database::migrations::tests::migrations_are_contiguous_and_end_at_schema_version` is the CI check. It runs in `cargo test` in ci.yml.
- `migrations_declare_db_compat_impact_per_policy` checks two things: `PrePolicy` is only used for steps up to v15, and every `Breaking` step names a generation in `(LEGACY_DB_COMPAT_VERSION, DB_COMPAT_VERSION]` that never decreases.
- `fresh_database_runs_every_step_and_records_it` checks that a fresh database records audit rows 1..=16, sets no skills-import flag, has four proxy rows and the spend-limit index, and that a second run is a no-op.
- `upgrade_records_only_the_steps_it_applies`
- `v15_database_gets_spend_limit_index_from_numbered_step` is the regression test for removing `create_tables`.
- `failing_step_propagates_error_and_rolls_back` checks that a broken v16 step returns an error, leaves `user_version` at 15, adds no audit row, and rolls back the step's earlier writes. The old `create_tables` swallowed this kind of failure.
- `v16_repairs_dev_global_profile_marker` and `v14_converts_singleton_proxy_config_left_by_beta_v2` replace the two `create_tables` repair tests.
- The v13→v14 test and the database tests that used `create_tables_on_conn` now build real schemas through the chain.
- `tests/migrations.rs` now also checks, for every fixture, that `schema_migrations` records exactly the steps `N+1..=16`, and it treats `schema_migrations` rows as user data that must be preserved.

**Verification**

- `cargo test --manifest-path src-tauri/Cargo.toml --test migrations`: 9 passed, 0 failed. This covers 15 start versions (v1–v13, v15, v16) plus v0 and the 2 legacy JSON fixtures. Each one is checked for schema equality with a fresh database, row preservation, audit rows and a no-op second init.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib database::`: 69 passed, 0 failed.
- `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast`: 2495 passed, 0 failed (all targets).
- `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`: clean.
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`: clean.
- `npx prettier --check docs/dev/DB_COMPATIBILITY.md`: clean.
- Before the v16 fixture was added, the harness failed only on `db_fixtures_cover_every_schema_version` (`missing DB fixtures for schema versions [16]`). Every older fixture already matched the fresh v16 schema built by the new chain.

**Compatibility note.** v16 is additive, but by policy an older v15 build will refuse a v16 database. Downgrading goes through the CCS-091 restore flow, which uses the pre-migration backup.
