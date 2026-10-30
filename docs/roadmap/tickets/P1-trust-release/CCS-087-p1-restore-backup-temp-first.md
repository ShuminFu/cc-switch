# CCS-087: [db] Restore backups into a temp DB and validate before overwriting the main DB

**Status:** Done · **Kind:** bug · **Labels:** `bug`, `phase:P1`, `p2-medium`, `area:database`

> **Ticket key:** `p1-restore-backup-temp-first` · **Phase:** P1 · **Priority:** p2-medium · **Effort:** S · **Depends on:** `p0-migration-harness`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding backend-restore-backup-overwrite-first. restore_from_backup copies the backup into the live DB first and then migrates. A backup from a newer version (user_version > SCHEMA_VERSION) leaves the main DB replaced and the app unbootable (db_version_too_new). The safety backup's id is lost on that error path.

## Evidence
- src-tauri/src/database/backup.rs:552-597 (579-590 Backup::step, 593-595 migrations, 574 safety backup); schema.rs:421-427; reference pattern backup.rs:110-139

## Proposed change
- Check the source user_version up front and return a localized "backup from newer version" error.
- Copy into a temp connection, then run create_tables_on_conn, apply_schema_migrations_on_conn and validate_basic_state there. Only then back up into main.
- Restore the safety backup automatically if anything fails after the copy.

## Acceptance criteria / verification
- [x] Test: future.db with SCHEMA_VERSION+1 gives Err, and the main DB keeps its version and provider row
- [x] `cargo test --manifest-path src-tauri/Cargo.toml database::tests -- restore`

## Dependencies
p0-migration-harness

## Implementation notes

**Summary.** `Database::restore_from_backup` (src-tauri/src/database/backup.rs) no longer copies the backup straight into the live DB:

1. The backup is opened read-only and its `user_version` is checked first; a value above `SCHEMA_VERSION` returns `AppError::Localized { key: "error.backupFromNewerVersion", .. }` before anything is touched (no safety backup is created either).
2. The backup is copied into a `NamedTempFile` connection, where `create_tables_on_conn`, `apply_schema_migrations_on_conn`, `validate_basic_state`, `ensure_model_pricing_seeded_on_conn` and `write_local_only_settings` run. Any failure (corrupt file, failed migration, backup with no providers/MCP servers) leaves the main DB untouched.
3. Only then is the safety backup taken and the validated temp DB copied into the main connection via the new `copy_database` helper. If that copy fails, the safety backup is automatically copied back into the main DB and the original error is returned (rollback failure is logged).

Behavior change to note: like SQL import, restore now rejects a backup that has neither providers nor MCP servers (`validate_basic_state`), as the ticket proposed.

**Files.**
- `src-tauri/src/database/backup.rs` — temp-first restore, version precheck, rollback, `copy_database` helper
- `src-tauri/src/database/schema.rs` — `ensure_model_pricing_seeded_on_conn` made `pub(crate)` so it can run on the temp connection
- `src-tauri/src/database/tests.rs` — `restore_from_backup_rejects_newer_version_and_keeps_main_db` (future.db at `SCHEMA_VERSION + 1` → Err, main keeps its version and provider row and local settings), `restore_from_backup_rejects_invalid_backup_and_keeps_main_db` (empty backup and non-SQLite file), `restore_from_backup_replaces_main_db_after_temp_validation` (happy path); tests isolate the home dir via `paths::scoped`

**Commits.** `1e8871b` (code + tests); this ticket update in a follow-up docs commit.

**Verification.**
- `cargo test --manifest-path src-tauri/Cargo.toml --lib database::tests -- restore` → 61 passed, 0 failed
- Regression check: with the pre-change `backup.rs`, the two rejection tests fail (the backup overwrites the main DB before migration/validation); with the change all 3 pass
- `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check` → clean
- `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` → clean
- `cargo test --manifest-path src-tauri/Cargo.toml --lib` → 2239 passed, 0 failed, 3 ignored
