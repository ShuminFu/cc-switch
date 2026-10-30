# CCS-228: [config] Run the Windows rename_replace / atomic_write path under Wine as interim evidence

**Status:** Open · **Kind:** chore · **Labels:** `chore`, `phase:P1`, `p3-low`, `area:config`

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
- [ ] The Wine run exits 0: overwrite works without remove_file, no `*.tmp.*` remains, rename onto a directory fails after the retries without leaking the temp file.
- [ ] Results recorded in CCS-079.

## Dependencies
None.
