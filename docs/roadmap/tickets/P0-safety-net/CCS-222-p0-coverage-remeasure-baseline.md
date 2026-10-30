# CCS-222: [coverage] Re-measure the coverage baseline on a clean tree and record proxy/ and database/ numbers

**Status:** Open · **Kind:** chore · **Labels:** `chore`, `phase:P0`, `p3-low`, `area:coverage`

> **Ticket key:** `p0-coverage-remeasure-baseline` · **Phase:** P0 · **Priority:** p3-low · **Effort:** M
> Plan: [next-phase-plan.md](../../next-phase-plan.md)
> Found during the Partial-ticket triage at d962272 (CCS-019, CCS-006). Source tickets: [CCS-019](../P0-safety-net/CCS-019-p0-ci-coverage-report.md), [CCS-006](../P5-protocol-ir-commands/CCS-006-epic-p5.md).

## Problem
`docs/dev/perf-baseline.json` `coverage.rust` carries a `_caveat`: it was measured at 6330783 with a parallel lane's uncommitted proxy test changes in the tree. The P5 epic (CCS-006) needs current proxy/ and database/ line coverage (P0 baseline: proxy 78.60%, database 78.84%) before a threshold can be set, and nobody has measured since. cargo-llvm-cov is installed at `/root/.cargo/bin/cargo-llvm-cov`; disk is tight (5.4G free, `src-tauri/target` 9.1G).

## Proposed change
- Free space first (`cargo clean --manifest-path src-tauri/Cargo.toml`, or point `CARGO_LLVM_COV_TARGET_DIR` elsewhere).
- On a clean checkout: `mkdir -p dist coverage/rust && cargo llvm-cov --manifest-path src-tauri/Cargo.toml --no-fail-fast --lcov --output-path coverage/rust/lcov.info`, then `node scripts/coverage-summary.mjs --title "Rust (cargo-llvm-cov)" --lcov coverage/rust/lcov.info` with the same `--dir` args as ci.yml 403-407 (proxy, database, services, commands) and `--json coverage/rust/summary-by-dir.json`.
- Update `coverage.rust.dirs`, `recorded_at`, `commit`; remove `_caveat`. Re-run `pnpm test:coverage` and refresh the frontend numbers.
- Record proxy/ and database/ numbers in CCS-019's baseline table and the CCS-006 epic audit.
- Optional: if `act` and a Docker daemon are available, dry-run `act pull_request -j coverage-frontend` / `-j coverage-rust` with `--artifact-server-path` in the scratchpad. This does not tick CCS-019 AC1.

## Acceptance criteria / verification
- [ ] `docs/dev/perf-baseline.json` numbers match `coverage/rust/summary-by-dir.json`; no `_caveat`.
- [ ] `npx vitest run tests/scripts` passes.
- [ ] proxy/ and database/ line coverage recorded in CCS-019 and CCS-006.

## Dependencies
None.
