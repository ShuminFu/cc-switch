# CCS-088: [db] Validate backup file names so drive-relative Windows paths cannot escape the backups dir

**Status:** Done · **Kind:** security · **Labels:** `security`, `phase:P1`, `p3-low`, `area:database`

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
- [x] Tests: "C:x.db", "C:..x.db", "foo:bar.db" and "\0.db" give InvalidInput; a #[cfg(windows)] components test
- [x] `cargo test --manifest-path src-tauri/Cargo.toml backup`

## Dependencies
None.

## Implementation notes
- Added `validate_backup_filename` in `src-tauri/src/database/backup.rs`: rejects `..`, `/`, `\`, `:`, NUL and names not ending in `.db`, and requires the path to parse as exactly one `Component::Normal` equal to the input. Non-ASCII names (e.g. Chinese) remain allowed.
- Used in `restore_from_backup`, `delete_backup` and `rename_backup` (old filename, and the final `<new>.db` name, which keeps the existing "Invalid characters in new name" message).
- Tests (in `database::backup::tests`):
  - `validate_backup_filename_rejects_drive_relative_and_nul_names`: `C:x.db`, `C:..x.db`, `foo:bar.db`, `\0.db` etc. give `InvalidInput`; normal and Chinese names pass.
  - `backup_commands_reject_colon_filenames_without_touching_files`: restore/delete/rename (both sides) reject those names; on Unix a real `foo:bar.db` file is not deleted (regression test: fails with the `:` check removed, passes with it); renaming to `我的备份` still works.
  - `drive_relative_names_are_not_single_normal_components_on_windows` (`#[cfg(windows)]`): asserts `C:x.db` parses as a `Prefix` and `join` escapes the base dir. Compiled/run only on a Windows runner; not executed locally (Linux).
- Commit: 18ed52f
- Verification:
  - `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`: clean
  - `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` (also with `--tests`): clean
  - `cargo test --manifest-path src-tauri/Cargo.toml --lib backup`: 38 passed, 0 failed
  - `cargo test --manifest-path src-tauri/Cargo.toml --lib`: 2241 passed, 0 failed, 3 ignored
