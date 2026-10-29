# CCS-079: [config] Make atomic_write durable, Windows-safe and free of leftover temp files

**Status:** Partial · **Kind:** bug · **Labels:** `bug`, `phase:P1`, `p1-high`, `area:config`

> **Ticket key:** `p1-atomic-write-durable` · **Phase:** P1 · **Priority:** p1-high · **Effort:** S · **Depends on:** `p0-config-writer-snapshots`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Covers verified findings backend-atomic-write-no-fsync, backend-atomic-write-windows-remove and backend-atomic-write-temp-leak. All three are fixed by rewriting the core write and rename of atomic_write.
- There is no fsync of the file or the directory, so a power loss can leave a zero-length or truncated file on XFS, APFS, or ext4 with noauto_da_alloc.
- On Windows the target is removed before the rename. That step is unnecessary, because std::fs::rename already replaces, and when the rename fails the config is lost.
- On any error the temp file, which may hold secrets, is left behind.

## Evidence
- src-tauri/src/config.rs:297-353 (317-321 write/flush, 332-342 Windows remove_file, 346 rename); no sync_all anywhere in src-tauri/src

## Proposed change
- Call f.sync_all() before the rename and fsync the parent directory after it on unix.
- Drop the Windows remove_file and use fs::rename (MoveFileExW replace), retrying briefly on a sharing violation.
- Add an RAII guard that removes the tmp file on any error.
- Alternatively use tempfile::NamedTempFile::persist.

## Acceptance criteria / verification
- [ ] Test: overwrite an existing file and then a new file; the content is correct and no `*.tmp.*` is left
- [ ] Test: when the target is a directory, rename fails, and no tmp file is left
- [ ] On Windows CI, overwrite works without remove_file
- [ ] `strace -e trace=fsync,rename` shows fsync(tmp) before the rename and fsync(dir) after it (manual)
- [ ] P0 config-writer snapshots are unchanged; `cargo test --manifest-path src-tauri/Cargo.toml config::`

## Dependencies
p0-config-writer-snapshots

## Implementation notes

**Status rationale:** Partial only because the Windows CI acceptance item cannot be exercised in this Linux environment; everything else is done and verified.

### Summary
- `atomic_write` (src-tauri/src/config.rs) now creates the temp file with `create_new`, writes, preserves the target's unix mode, and calls `sync_all()` before closing (helper `write_and_sync`, which takes the handle by value so it is closed before rename/cleanup).
- An RAII `TempFileGuard` deletes the `*.tmp.*` file on any error path (write, flush, fsync, rename); it is disarmed only after a successful rename.
- The Windows `remove_file`-before-rename was removed. `rename_replace` relies on `fs::rename` (MoveFileExW with replace) and on Windows retries up to 5 times with short backoff (20/40/60/80 ms) on ERROR_ACCESS_DENIED / ERROR_SHARING_VIOLATION / ERROR_LOCK_VIOLATION.
- On unix the parent directory is fsynced after the rename (`sync_dir`); since the rename already succeeded, a failure there is logged with `log::warn!` rather than returned (some filesystems reject directory fsync).

### Files
- src-tauri/src/config.rs (implementation + 3 tests: `atomic_write_overwrites_and_creates_without_leftover_tmp`, `atomic_write_removes_tmp_when_rename_fails`, `atomic_write_preserves_existing_permissions`)

### Commits
- ed7aac2 fix(config): make atomic_write durable and clean up temp files (CCS-079)

### Verification
- `cargo test --manifest-path src-tauri/Cargo.toml --lib config::tests::atomic_write`: 3 passed.
- Regression check: with the guard's `remove_file` disabled, `atomic_write_removes_tmp_when_rename_fails` FAILS (temp file left behind, as in the old code); restored, it passes.
- `strace -f -e trace=fsync,rename,openat` on the test binary: `openat(settings.json.tmp.N, O_CREAT|O_EXCL)` -> `fsync(tmp fd)` -> `rename(tmp, settings.json)` -> `openat(parent dir)` -> `fsync(dir fd)`, same order for the new nested file.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib`: 2218 passed, 0 failed, 3 ignored (includes `config::`).
- `cargo test --manifest-path src-tauri/Cargo.toml --test config_writers` (P0 config-writer snapshots): 16 passed, snapshots unchanged.
- `cargo fmt` clean; `cargo clippy -- -D warnings` clean.

### Remaining
- "On Windows CI, overwrite works without remove_file": the `#[cfg(windows)]` retry branch was not compiled or run here (no Windows target installed). The overwrite test is platform-neutral and will cover it on the Windows runner. On Windows the directory-target test takes about 200 ms because ERROR_ACCESS_DENIED is retried.
