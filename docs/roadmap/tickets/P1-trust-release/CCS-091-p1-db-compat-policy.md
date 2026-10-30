# CCS-091: [db] Define and test the downgrade and mixed-version sync policy

**Status:** Done · **Kind:** chore · **Labels:** `chore`, `phase:P1`, `p1-high`, `area:database`

> **Ticket key:** `p1-db-compat-policy` · **Phase:** P1 · **Priority:** p1-high · **Effort:** M · **Depends on:** `p1-db-premigration-backup`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
From the critique:
- An older app refuses a newer DB, so once a schema bump ships, a rollback or channel switch bricks startup.
- SQL dumps carry user_version, and nothing defines what an older or newer device does with them.
- DB_COMPAT_VERSION and the LEGACY constants have no rule for when to bump.

## Evidence
- src-tauri/src/database/mod.rs:170 stored_user_version_exceeds_supported; backup.rs:398 PRAGMA user_version in dumps; services/sync_protocol.rs:27-28 (DB_COMPAT_VERSION=6, LEGACY=5); s3_sync.rs:221-227

## Proposed change
- Write docs/dev/DB_COMPATIBILITY.md:
  - schema changes must be additive and forward-readable within a compat version
  - when to bump DB_COMPAT_VERSION
  - the downgrade restore flow (startup error dialog offers "restore pre-migration backup")
- On sync import of a newer-compat dump, show a clear error and do not import partially.
- Add a PR template checkbox for schema changes.

## Acceptance criteria / verification
- [x] Test: importing a dump with compat version+1 returns a localized error and leaves the DB unchanged
- [x] Test: the startup db_version_too_new path lists the available pre-migration backups
- [x] `cargo test --manifest-path src-tauri/Cargo.toml sync_protocol`

## Dependencies
p1-db-premigration-backup

## Implementation notes

**Summary**
- `docs/dev/DB_COMPATIBILITY.md` sets the rules. Schema changes are forward-migration only and additive within a `DB_COMPAT_VERSION`. It says when to bump `DB_COMPAT_VERSION` (non-additive migrations, `db.sql` format changes) and that `LEGACY_DB_COMPAT_VERSION` is frozen. It covers how mixed-version sync behaves, including the accepted limit that an older device's upload drops newer columns, and the downgrade restore flow. The constants in `sync_protocol.rs` now point to the doc.
- Import of a newer-schema dump: `Database::ensure_sql_dump_version_supported` reads the dump's `PRAGMA user_version=N;`. For manual import it runs before the pre-import backup. For sync it runs in `apply_snapshot` before skills are replaced. A dump with `N > SCHEMA_VERSION` returns the localized error `backup.sql.newer_schema`, and nothing is changed. A forged header is caught by a second check of `user_version` on the imported temp DB. A manifest with `DB_COMPAT_VERSION + 1` was already rejected with the localized `sync.manifest_db_version_incompatible`; there is now an explicit test for both layouts.
- Startup `db_version_too_new`: `init_status::db_version_too_new_payload` builds the payload. It now includes `restorable_backups`, from the new `Database::list_restorable_backups()`: the backups with `user_version <= SCHEMA_VERSION`, newest first, skipping unreadable ones. The new Tauri command `restore_backup_over_newer_db` works only in recovery mode. It validates and migrates the backup in a temp DB, keeps the newer DB as `backups/db_newer_v{N}_<ts>.sqlite` (not a `.db` file, so retention never prunes it), then writes the backup over `cc-switch.db`. `DatabaseUpgrade.tsx` lists the backups with a confirmed "Restore" button that restores and then restarts. Strings were added in en, zh, zh-TW and ja.
- `.github/pull_request_template.md` has a new checkbox for schema changes.

**Files**
`docs/dev/DB_COMPATIBILITY.md`, `.github/pull_request_template.md`, `src-tauri/src/database/backup.rs`, `src-tauri/src/database/tests.rs`, `src-tauri/src/init_status.rs`, `src-tauri/src/lib.rs`, `src-tauri/src/commands/import_export.rs`, `src-tauri/src/services/sync_protocol.rs`, `src/components/DatabaseUpgrade.tsx`, `src/i18n/locales/{en,zh,zh-TW,ja}.json`, `tests/components/DatabaseUpgrade.test.tsx`

**Commits**
- `254388f` fix(db): define downgrade and mixed-version sync policy (CCS-091)
- this ticket update follows as its own docs commit

**Tests added**
- `database::tests::import_of_newer_schema_dump_is_rejected_and_leaves_db_unchanged` covers manual and sync import, plus a forged header.
- `services::sync_protocol::tests::apply_snapshot_rejects_newer_schema_dump_without_partial_import` checks that the DB and skills are untouched.
- `services::sync_protocol::tests::validate_manifest_compat_rejects_next_db_compat_version_with_localized_error`
- `database::tests::db_version_too_new_payload_lists_restorable_premigration_backups` checks that a too-new backup and a corrupt backup are excluded.
- `database::tests::restore_backup_over_newer_db_downgrades_and_keeps_newer_copy` checks that `Database::init` then succeeds.
- `tests/components/DatabaseUpgrade.test.tsx` has 3 tests for the list, the confirmed restore plus restart, and the cancel path.
- The two import tests were checked with the version check disabled. Both failed, with a non-localized `AppError::Database` error and a skills-zip error, and both pass with the fix.

**Verification**
- `cargo test --manifest-path src-tauri/Cargo.toml --lib -- sync_protocol newer_schema db_version_too_new restore_backup_over_newer init_error`: 27 passed, 0 failed.
- `cargo fmt -- --check`: clean. `cargo clippy -- -D warnings`: clean.
- `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast`: 2419 passed, 0 failed (all targets).
- `pnpm typecheck`: clean. `pnpm test:unit`: 147 files, 1443 tests passed. `pnpm format:check`: clean.
