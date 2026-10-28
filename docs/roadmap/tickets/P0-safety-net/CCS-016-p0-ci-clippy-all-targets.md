# CCS-016: [ci] Run clippy with --all-targets

**Status:** Open · **Kind:** chore · **Labels:** `chore`, `phase:P0`, `p2-medium`, `area:ci`

> **Ticket key:** `p0-ci-clippy-all-targets` · **Phase:** P0 · **Priority:** p2-medium · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding ci-clippy-all-targets: clippy lints only the lib/bin targets, so about 133 test modules and src-tauri/tests/* are never linted.

## Evidence
- .github/workflows/ci.yml:102 `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`

## Proposed change
Fix the existing test-code warnings, then change the step to `--all-targets`.

## Acceptance criteria / verification
- [ ] `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings` passes locally (with a dist/ dir)
- [ ] A deliberate lint added in tests/ fails CI (then remove it)

## Dependencies
None.
