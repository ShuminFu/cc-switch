# CCS-041: [deps] Remove the unused tower-http 0.5 dependency

**Status:** Open · **Kind:** chore · **Labels:** `chore`, `phase:P0`, `p3-low`, `area:deps`

> **Ticket key:** `p0-unused-tower-http` · **Phase:** P0 · **Priority:** p3-low · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding ci-unused-tower-http. tower-http 0.5 (cors) is declared but never used, and it adds a second tower-http version.

## Evidence
- src-tauri/Cargo.toml:53; no tower_http use in src-tauri/src (server.rs:177/190 use `tower`)

## Proposed change
Delete the line and rebuild the lockfile.

## Acceptance criteria / verification
- [ ] `cargo check --manifest-path src-tauri/Cargo.toml --all-targets` and `cargo test --manifest-path src-tauri/Cargo.toml`
- [ ] `grep -n 'tower-http 0.5' src-tauri/Cargo.lock` finds nothing

## Dependencies
None.
