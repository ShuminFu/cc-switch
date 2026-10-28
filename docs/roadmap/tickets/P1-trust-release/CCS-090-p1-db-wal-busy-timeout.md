# CCS-090: [db] Enable WAL, synchronous=NORMAL and busy_timeout on local DB paths

**Status:** Open · **Kind:** refactor · **Labels:** `refactor`, `phase:P1`, `p2-medium`, `area:database`

> **Ticket key:** `p1-db-wal-busy-timeout` · **Phase:** P1 · **Priority:** p2-medium · **Effort:** S · **Depends on:** `p1-db-premigration-backup`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
The DB runs in DELETE journal mode with no busy_timeout, so locks contend between the proxy logger, session sync and UI queries.

## Evidence
- src-tauri/src/database/mod.rs Database::init; set_app_config_dir_override (settings.rs:296) can point at iCloud or Dropbox

## Proposed change
- Verify snapshot_to_memory, WebDAV/S3 export and update_hook under WAL.
- Set `journal_mode=WAL`, `synchronous=NORMAL` and `busy_timeout=5000`.
- Specify non-local detection: the override dir is inside a known cloud-sync root (iCloud Drive, Dropbox, OneDrive, Google Drive), or a statfs network filesystem type. In that case keep DELETE mode and log it.

## Acceptance criteria / verification
- [ ] A startup log/test asserts `PRAGMA journal_mode` == wal on a local temp dir and delete on a simulated cloud path
- [ ] Existing sync/export tests pass under WAL
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml database::`

## Dependencies
p1-db-premigration-backup
