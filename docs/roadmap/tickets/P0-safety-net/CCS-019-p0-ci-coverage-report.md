# CCS-019: [ci] Publish report-only coverage from cargo-llvm-cov and vitest

**Status:** Open · **Kind:** chore · **Labels:** `chore`, `phase:P0`, `p3-low`, `area:ci`

> **Ticket key:** `p0-ci-coverage-report` · **Phase:** P0 · **Priority:** p3-low · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
The plan's P5 target of 75% coverage for proxy/ and database/ needs a baseline from now on.

## Proposed change
- Add a CI job running `cargo llvm-cov --manifest-path src-tauri/Cargo.toml --lcov` and `vitest --coverage`, uploading both as artifacts.
- Report only for now; ratchet in P5.

## Acceptance criteria / verification
- [ ] Coverage artifacts appear on PR runs, with per-directory summaries for proxy/ and database/
- [ ] Baseline numbers recorded in docs/dev/perf-baseline.json or the README for dev

## Dependencies
None.
