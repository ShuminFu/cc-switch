# CCS-228: [config] Run the Windows rename_replace / atomic_write path under Wine as interim evidence

**Status:** Done · **Kind:** chore · **Labels:** `chore`, `phase:P1`, `p3-low`, `area:config`

> **Ticket key:** `p1-config-atomic-write-wine` · **Phase:** P1 · **Priority:** p3-low · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)
> Found during the Partial-ticket triage at d962272 (CCS-079, CCS-002). Source tickets: [CCS-079](../P1-trust-release/CCS-079-p1-atomic-write-durable.md), [CCS-002](../P1-trust-release/CCS-002-epic-p1.md).

## Problem
Every CCS-079 item except "on Windows CI, overwrite works without remove_file" is met (`src-tauri/src/config.rs`: `atomic_write_impl` 324, `TempFileGuard` 362, `rename_replace` 488 with retries on 5/32/33 under `#[cfg(windows)]`). That `#[cfg(windows)]` branch has never executed. The full crate cannot cross-compile to windows-gnu (aws-lc-sys), but wine, x86_64-w64-mingw32-gcc and the `x86_64-pc-windows-gnu` rust target are installed.

## Proposed change
- In the scratchpad, build a standalone bin crate with verbatim copies of `write_and_sync`, `TempFileGuard`, `rename_replace` and a cut-down `atomic_write_impl` (no ACL/symlink handling, or include `owner_only_acl` if it builds with windows-sys).
- Port `atomic_write_overwrites_and_creates_without_leftover_tmp` and `atomic_write_removes_tmp_when_rename_fails` into `main()` asserts.
- Optional: hold the target open with `share_mode(0)` in a thread that releases it after ~50 ms and assert the retry succeeds.
- `cargo build --target x86_64-pc-windows-gnu` and `wine target/x86_64-pc-windows-gnu/debug/<bin>.exe`.
- Record results as Wine evidence in CCS-079 notes. No repo code change unless Wine exposes a bug; the Windows CI box stays unchecked.

## Acceptance criteria / verification
- [x] The Wine run exits 0: overwrite works without remove_file, no `*.tmp.*` remains, rename onto a directory fails after the retries without leaking the temp file.
- [x] Results recorded in CCS-079.

## Dependencies
None.

## Implementation notes

**Summary:** Built a standalone bin crate in the session scratchpad (not committed) that `include!`s verbatim line ranges of `src-tauri/src/config.rs` at 742b36c, cross-compiled it for `x86_64-pc-windows-gnu` and ran it under Wine 9.0. The `#[cfg(windows)]` branches of `rename_replace` and `owner_only_acl` executed for the first time and all assertions passed (exit 0). Wine exposed no bug, so there is no repo code change, as the ticket intended. The "Windows CI" item in CCS-079 stays unchecked.

### Harness
- `impl_extract.rs`: `atomic_write_impl` (config.rs 324-387) with only the `capture_write` / `record_prior_contents` / `resolve_write_target` preamble removed (write-preview capture and symlink following are out of scope).
- `helpers_extract.rs`: config.rs 432-634 unchanged (`write_and_sync`, `TempFileGuard`, `rename_replace`, `mod owner_only_acl`). `cmp` against `sed -n 432,634p src-tauri/src/config.rs` shows they are identical.
- `main.rs`: a minimal `AppError` (`Io` / `IoContext` / `Config`), `atomic_write` / `atomic_write_secret` wrappers, and the checks below. Dependencies: `windows-sys =0.61.2` (the same version as Cargo.lock), built with `cargo +1.95 build --offline --target x86_64-pc-windows-gnu` with no warnings.

### Wine results (`wine target/x86_64-pc-windows-gnu/debug/wine-atomic.exe`, exit 0)
1. Port of `atomic_write_overwrites_and_creates_without_leftover_tmp`, plus 50 more overwrites of the same file: content correct, no `*.tmp.*` left. Overwriting works without `remove_file`, because `MoveFileExW` with `MOVEFILE_REPLACE_EXISTING` replaces the target.
2. Port of `atomic_write_removes_tmp_when_rename_fails` (the target is a non-empty directory): fails with `IoContext`, os error 5, after about 207 ms, which is the full 20+40+60+80 ms retry budget. The temp file is removed and the directory and its contents are intact.
3. Target held with `share_mode(0)` in a thread for 50 ms. A bare `fs::rename` returns os error 5 under Wine (Wine reports ERROR_ACCESS_DENIED rather than ERROR_SHARING_VIOLATION; both are in the retry set). `atomic_write` retries and succeeds after about 65 ms. New content is in place and no `*.tmp.*` is left.
4. Target held for 1.5 s, which is longer than the retry budget: `atomic_write` gives up after about 208 ms with os error 5. The old content is kept and the temp file is removed.
5. `atomic_write_secret` create and overwrite: `owner_only_acl::apply` (protected DACL for the current user only) succeeds before the rename, the owner can reopen the file, and no `*.tmp.*` is left.

### Negative controls (the harness detects regressions)
- With `ATTEMPTS` patched to 1 (no retry), check 3 fails: `retry must succeed after release: IoContext { .. os error 5 }`, exit 101.
- With `TempFileGuard::drop` no longer calling `remove_file`, check 2 fails: `assertion failed: leftover_tmp_files(&dir).is_empty()`, exit 101.
- With the verbatim file restored, the run passes again (`ALL PASS`, exit 0).

### Files
- docs/roadmap/tickets/P1-trust-release/CCS-228-p1-config-atomic-write-wine.md (this file)
- docs/roadmap/tickets/P1-trust-release/CCS-079-p1-atomic-write-durable.md (Wine evidence section)

### Commits
- 9708994 docs(roadmap): record Wine run of the Windows atomic_write path (CCS-228)

### Remaining
- None for this ticket. CCS-079's "On Windows CI, overwrite works without remove_file" box still needs a real windows-latest runner. Wine is interim evidence, not a substitute.
