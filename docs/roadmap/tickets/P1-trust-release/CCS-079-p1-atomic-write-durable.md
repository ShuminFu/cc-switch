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

### Review
- I checked each acceptance criterion against ed7aac2. They hold: the file is fsynced before the rename, the directory is fsynced after it, the Windows `remove_file` is gone, the guard cleans up on error, and the tests really check the fix. The `rename_replace` and `write_and_sync` bodies, copied verbatim, also compile with `rustc --target x86_64-pc-windows-gnu`. That proves the `#[cfg(windows)]` branch type-checks, but it was not run.
- Fixed one gap: for a relative target such as `foo.json`, `path.parent()` is an empty path. `File::open("")` fails, so the directory fsync was always skipped and a warning was logged on every write. `sync_dir` now treats an empty parent as `.` and returns `io::Result`, and `atomic_write` still only logs on failure. New test: `sync_dir_treats_empty_parent_as_current_dir`.
- Left out of scope: `proxy/providers/copilot_auth.rs` and `codex_oauth_auth.rs` have their own temp-file-and-rename writers, and those still have no fsync.
- Re-verified: `cargo test --lib` 2219 passed, 0 failed; `--test config_writers` 16 passed; fmt and clippy `-D warnings` clean. Status stays Partial until the Windows CI run.

### Remaining after triage

- Blocked: On Windows CI, overwrite works without remove_file. Needs a windows-latest Actions run; CCS-228 adds Wine evidence only.
- Follow-up [CCS-228](../P1-trust-release/CCS-228-p1-config-atomic-write-wine.md): Run the Windows rename_replace / atomic_write path under Wine as interim evidence.

### Wine evidence (CCS-228)

- The `#[cfg(windows)]` code was run under Wine 9.0. Verbatim copies of `write_and_sync`, `TempFileGuard`, `rename_replace` and `owner_only_acl` (config.rs 432-634 at 742b36c), plus `atomic_write_impl` without the preview-capture and symlink preamble, were built for `x86_64-pc-windows-gnu` with windows-sys 0.61.2. The run exits 0.
- Overwrite works without `remove_file` (1 + 50 overwrites, no `*.tmp.*` left). Rename onto a non-empty directory fails with os error 5 after the full retry budget (about 207 ms), the temp file is removed and the directory is intact.
- Target held with `share_mode(0)`: Wine returns os error 5 for the rename. If the target is released after 50 ms, the retry succeeds (about 65 ms). If it is held for 1.5 s, the write gives up after about 208 ms, keeps the old content and leaves no temp file. `atomic_write_secret`, including `owner_only_acl::apply`, creates and overwrites the file.
- Negative controls: without the retries, or without `TempFileGuard` removing the temp file, the harness fails (exit 101).
- Details are in [CCS-228](../P1-trust-release/CCS-228-p1-config-atomic-write-wine.md). The "On Windows CI" box stays unchecked until a real windows-latest run.

### Wine evidence: full Windows suite (2026-10-31)

- Run: `cargo test --target x86_64-pc-windows-gnu --no-fail-fast` under Wine 9.0 (mingw cross-compile, `LANG=LC_ALL=C.UTF-8`, `WINEPATH` set to the target dir), at `bdd666e`. This was the whole crate, not an extract, so the CCS-228 note that cross-compiling fails in `aws-lc-sys` no longer applies once mingw is installed. Result: 3092 passed, 1 failed (the CCS-083 DACL test, a Wine limitation; see CCS-083).
- Under the default C locale, `database::backup::tests::backup_commands_reject_colon_filenames_without_touching_files` fails with NotFound when it renames `ok.db` to `我的备份.db`. Wine maps non-ASCII names through the unix locale. A C repro (`MoveFileExW` to a CJK name) fails with error 2 under `LANG` unset and succeeds under `C.UTF-8`. NTFS has no such limit, so this is not a product bug. Run Wine with a UTF-8 locale.
- Still needs real Windows: the "On Windows CI" box (windows-latest run).
