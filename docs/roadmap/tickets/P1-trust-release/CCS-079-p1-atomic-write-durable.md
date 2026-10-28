# CCS-079: [config] Make atomic_write durable, Windows-safe and free of leftover temp files

**Status:** Open · **Kind:** bug · **Labels:** `bug`, `phase:P1`, `p1-high`, `area:config`

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
