# CCS-037: [sync] Preserve local session_log_sync cursors on WebDAV/S3 import

**Status:** Open · **Kind:** bug · **Labels:** `bug`, `phase:P0`, `p2-medium`, `area:sync`

> **Ticket key:** `p0-hotfix-sync-skip-session-log` · **Phase:** P0 · **Priority:** p2-medium · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Sync import overwrites the local session_log_sync cursor table, so session logs get re-imported or skipped on the receiving device.

## Evidence
- src-tauri/src/database/backup.rs:15-30 SYNC_SKIP_TABLES / SYNC_PRESERVE_TABLES omit session_log_sync

## Proposed change
Add session_log_sync to the skip and preserve lists.

## Acceptance criteria / verification
- [ ] Test: seed local session_log_sync rows, import a snapshot with different rows, and assert the local rows are kept
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml database::backup`

## Dependencies
None.
