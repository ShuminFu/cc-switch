# CCS-088: [db] Validate backup file names so drive-relative Windows paths cannot escape the backups dir

**Status:** Open · **Kind:** security · **Labels:** `security`, `phase:P1`, `p3-low`, `area:database`

> **Ticket key:** `p1-backup-filename-validation` · **Phase:** P1 · **Priority:** p3-low · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding backend-restore-backup-windows-filename. "C:x.db" passes the checks, and backup_dir.join() then discards backup_dir. This affects restore, rename (old and new names) and delete.

## Evidence
- src-tauri/src/database/backup.rs:554-565, 604-608, 631-634, 667-670

## Proposed change
- Add validate_backup_filename: exactly one Component::Normal, ends in .db, no ':' or NUL.
- Allow non-ASCII names, because rename supports Chinese names.
- Use it in all three functions.

## Acceptance criteria / verification
- [ ] Tests: "C:x.db", "C:..x.db", "foo:bar.db" and "\0.db" give InvalidInput; a #[cfg(windows)] components test
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml backup`

## Dependencies
None.
