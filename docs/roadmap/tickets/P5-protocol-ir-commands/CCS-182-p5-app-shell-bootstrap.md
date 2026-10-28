# CCS-182: [shell] Add a StartupStep pipeline with a startup diagnostics report and a single run-event router

**Status:** Open · **Kind:** refactor · **Labels:** `refactor`, `phase:P5`, `p2-medium`, `area:shell`

> **Ticket key:** `p5-app-shell-bootstrap` · **Phase:** P5 · **Priority:** p2-medium · **Effort:** M · **Depends on:** `p5-thin-commands`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
lib.rs run() is a monolith. A failed migration or live import shows up as a silent "missing providers" (backlog: startup diagnostics).

## Proposed change
- app_shell/bootstrap.rs with StartupStep, plus a get_startup_report command and UI.
- run_events.rs, a single router that includes the deep-link handling.
- Move the lightweight-mode AtomicBool into AppState.

## Acceptance criteria / verification
- [ ] lib.rs run() is 300 lines or fewer
- [ ] Test: a failing step appears in the report with its error code
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml bootstrap`

## Dependencies
p5-thin-commands
