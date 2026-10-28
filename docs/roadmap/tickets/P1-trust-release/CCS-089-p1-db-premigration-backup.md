# CCS-089: [db] Take a consistent pre-migration backup before any DDL, including v0 and JSON upgrades

**Status:** Open · **Kind:** bug · **Labels:** `bug`, `phase:P1`, `p1-high`, `area:database`

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
- [ ] Migration-harness test: upgrading from v0 and from the JSON fixture leaves a backup file whose content equals the pre-upgrade state
- [ ] A WAL-mode test with uncheckpointed writes produces a backup that contains them
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --test migrations`

## Dependencies
p0-migration-harness
