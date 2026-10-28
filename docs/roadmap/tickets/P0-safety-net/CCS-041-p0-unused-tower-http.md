# CCS-041: [deps] Remove the unused tower-http 0.5 dependency

**Status:** Done · **Kind:** chore · **Labels:** `chore`, `phase:P0`, `p3-low`, `area:deps`

> **Ticket key:** `p0-unused-tower-http` · **Phase:** P0 · **Priority:** p3-low · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding ci-unused-tower-http. tower-http 0.5 (cors) is declared but never used, and it adds a second tower-http version.

## Evidence
- src-tauri/Cargo.toml:53; no tower_http use in src-tauri/src (server.rs:177/190 use `tower`)

## Proposed change
Delete the line and rebuild the lockfile.

## Acceptance criteria / verification
- [x] `cargo check --manifest-path src-tauri/Cargo.toml --all-targets` and `cargo test --manifest-path src-tauri/Cargo.toml`
- [x] `grep -n 'tower-http 0.5' src-tauri/Cargo.lock` finds nothing

## Dependencies
None.

## Implementation notes
- **Change:** Removed `tower-http = { version = "0.5", features = ["cors"] }` from `src-tauri/Cargo.toml`. Before removing it, ran `grep -rn "tower_http\|CorsLayer" src-tauri/ --include=*.rs`, which found nothing. Cargo pruned the `tower-http 0.5.2` package from `Cargo.lock`. The only version left is `tower-http 0.6.8`, pulled in by reqwest; its references in the lockfile are now unversioned (`"tower-http"`). `tower = "0.4"`, which `proxy/server.rs` uses, is unchanged.
- **Files:** `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`.
- **Commit:** `669e4ee` chore(deps): remove unused tower-http 0.5 dependency (CCS-041).
- **Tests:** No new test was added. This is a manifest-only change, and the check below covers it: the grep on the lockfile.
- **Verification:**
  - `cargo check --manifest-path src-tauri/Cargo.toml --all-targets` passes.
  - `cargo fmt --check` is clean.
  - `cargo clippy --all-targets -- -D warnings` is clean.
  - `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast`:
    - lib: 2022 passed, 2 ignored.
    - Every integration target passes except `import_export_sync` (2 passed, 24 failed). That failure is a known problem in this environment (`/nonexistent` exists because tests run as root, and a poisoned mutex then fails the tests that follow; tracked by p0-testenv-paths). It is not caused by this change.
  - `grep -n 'tower-http 0.5' src-tauri/Cargo.lock` returns no matches (exit status 1).
