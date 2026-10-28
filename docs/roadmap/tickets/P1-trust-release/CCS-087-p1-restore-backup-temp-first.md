# CCS-087: [db] Restore backups into a temp DB and validate before overwriting the main DB

**Status:** Open · **Kind:** bug · **Labels:** `bug`, `phase:P1`, `p2-medium`, `area:database`

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
- [ ] Test: future.db with SCHEMA_VERSION+1 gives Err, and the main DB keeps its version and provider row
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml database::tests -- restore`

## Dependencies
p0-migration-harness
