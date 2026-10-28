# CCS-013: [tests] Introduce a hermetic TestEnv with injected Paths and remove the global test_mutex

**Status:** Open · **Kind:** refactor · **Labels:** `refactor`, `phase:P0`, `p1-high`, `area:tests`

> **Ticket key:** `p0-testenv-paths` · **Phase:** P0 · **Priority:** p1-high · **Effort:** L
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Integration tests mutate global HOME and serialize on test_mutex, so they cannot run in parallel and they leak state between tests.

## Evidence
- src-tauri/tests/support.rs:7-67 (HOME override + test_mutex)
- config.rs and settings.rs resolve paths via dirs::home_dir directly

## Proposed change
- Add a `Paths` struct injected into config.rs and settings.rs.
- Add `TestEnv { home: TempDir, db, settings }`.
- The production default `Paths` must resolve exactly as dirs::home_dir does today. Add a test that asserts the resolved paths for every app match.

## Acceptance criteria / verification
- [ ] test_mutex is deleted; `cargo test --manifest-path src-tauri/Cargo.toml` passes with default parallelism, run 5 times in a row
- [ ] A path-equivalence test covers all 8 apps
- [ ] `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`

## Dependencies
None.
