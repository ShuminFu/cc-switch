# CCS-089: [db] Take a consistent pre-migration backup before any DDL, including v0 and JSON upgrades

**Status:** Done · **Kind:** bug · **Labels:** `bug`, `phase:P1`, `p1-high`, `area:database`

> **Ticket key:** `p1-db-premigration-backup` · **Phase:** P1 · **Priority:** p1-high · **Effort:** S · **Depends on:** `p0-migration-harness`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
The pre-migration backup runs only when version > 0 and after create_tables, so v0 and JSON-era upgrades get no backup. Per the critique, file-copy backups can also be stale under WAL.

## Evidence
- src-tauri/src/database/mod.rs:123-137 (:131 `version > 0`), backup_database_file

## Proposed change
- Move the backup ahead of create_tables.
- Back up for every version, including the legacy config.json, which is copied too.
- Use the SQLite online backup API, or wal_checkpoint(TRUNCATE) followed by a copy.
- Record the backup id in the startup error message so the user can restore it.

## Acceptance criteria / verification
- [x] Migration-harness test: upgrading from v0 and from the JSON fixture leaves a backup file whose content equals the pre-upgrade state
- [x] A WAL-mode test with uncheckpointed writes produces a backup that contains them
- [x] `cargo test --manifest-path src-tauri/Cargo.toml --test migrations`

## Dependencies
p0-migration-harness

## Implementation notes

**Summary**
- `Database::init` now takes the pre-migration backup right after opening the connection and **before `create_tables`** (no DDL has run yet). It covers every existing database with `user_version < SCHEMA_VERSION`, including v0. The old `version > 0` guard is gone.
- The snapshot goes through the SQLite Online Backup API (`snapshot_connection_to_backup` → `copy_database`) on the live connection, so uncheckpointed WAL frames are included. A plain file copy would miss them.
- Legacy `config.json`:
  - JSON-era upgrades (no DB, `config.json` present) call the new `Database::backup_legacy_config_json()` in `lib.rs` **before** `MultiAppConfig::load()`, because `load()` upgrades and rewrites the file in place (for example, the v3.6 → v3.7 MCP merge). The pre-upgrade bytes are saved as `backups/<backup_id>.config.json`.
  - When an older DB is upgraded and a `config.json` still sits next to it, that file is copied under the same id.
- Backup ids keep the `db_backup_YYYYMMDD_HHMMSS[_N]` format, which the backup list UI parses. `next_backup_id` also avoids collisions with `.config.json` files.
- If `create_tables` or the schema migration fails, the startup error (shown by the init-error dialog) now ends with `迁移前备份 ID：<id>（位于 <backups dir>），可用于恢复数据`. A failed JSON import logs the config.json backup id.
- A failed backup is still a warning and does not block migration, as before.

**Files**
- `src-tauri/src/database/mod.rs`: backup moved ahead of `create_tables`; error hint with the backup id
- `src-tauri/src/database/backup.rs`: `create_premigration_backup`, `backup_legacy_config_json`, `copy_legacy_config_json`, `next_backup_id`, `snapshot_connection_to_backup` (`backup_database_file` reuses it)
- `src-tauri/src/lib.rs`: backs up config.json before the JSON load/upgrade; logs the backup id when the JSON import fails
- `src-tauri/tests/migrations.rs`:
  - `every_released_schema_migrates_to_current` now asserts that the backup equals the pre-upgrade DB (user_version, schema and every row)
  - new `v0_database_gets_premigration_backup`
  - new `premigration_backup_includes_uncheckpointed_wal_writes`, with a control check that a plain file copy misses the WAL write
  - new `failed_migration_error_names_the_backup_id`
  - legacy JSON tests assert the `.config.json` backup is byte-identical to the fixture

**Commits:** `8ada1d0` (code and tests), plus this ticket update.

**Verification**
- Regression check: with `database/mod.rs` reverted to HEAD, `cargo test --test migrations` failed 3 tests (`every_released_schema_migrates_to_current`, `v0_database_gets_premigration_backup`, `failed_migration_error_names_the_backup_id`). With the fix, all pass.
- `cargo test --manifest-path src-tauri/Cargo.toml --test migrations`: 8 passed, 0 failed
- `cargo test --manifest-path src-tauri/Cargo.toml --lib`: 2241 passed, 0 failed, 3 ignored
- `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast`: all targets green, 0 failed
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`: clean
- `cargo fmt --manifest-path src-tauri/Cargo.toml --check`: clean

**Known limitation:** backup retention (`backup_retain_count`, minimum 1) still applies to `.db` backups. With retain = 1, the auto-vacuum rebuild backup taken later in the same `init` can prune the pre-migration snapshot. `.config.json` copies are never pruned.
