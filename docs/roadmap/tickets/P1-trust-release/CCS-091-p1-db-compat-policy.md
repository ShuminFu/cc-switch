# CCS-091: [db] Define and test the downgrade and mixed-version sync policy

**Status:** Open · **Kind:** chore · **Labels:** `chore`, `phase:P1`, `p1-high`, `area:database`

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
- [ ] Test: importing a dump with compat version+1 returns a localized error and leaves the DB unchanged
- [ ] Test: the startup db_version_too_new path lists the available pre-migration backups
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml sync_protocol`

## Dependencies
p1-db-premigration-backup
