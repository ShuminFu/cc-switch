# CCS-242: [coverage] Add a ratcheting --min threshold to coverage-summary.mjs and gate coverage-rust on it

**Status:** Open · **Kind:** chore · **Labels:** `chore`, `phase:P5`, `p3-low`, `area:coverage`

> **Ticket key:** `p5-coverage-min-threshold` · **Phase:** P5 · **Priority:** p3-low · **Effort:** S · **Depends on:** `p0-coverage-remeasure-baseline`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)
> Found during the Partial-ticket triage at d962272 (CCS-006). Source tickets: [CCS-006](../P5-protocol-ir-commands/CCS-006-epic-p5.md).

## Problem
The P5 criterion "proxy/ and database/ ≥ 75% line coverage" has no gate: `scripts/coverage-summary.mjs` is "report-only; no thresholds" and the `coverage-rust` job (ci.yml:352) has `continue-on-error: true` (355).

## Proposed change
- Add a repeatable `--min <dir>=<pct>` option that exits non-zero and names the failing directory when line coverage is below the floor.
- Unit tests next to the existing `parseLcov` export with a synthetic lcov fixture (pass and fail).
- ci.yml coverage-rust: remove `continue-on-error: true`, pass `--min src-tauri/src/proxy=<n> --min src-tauri/src/database=<n>` using min(75, the CCS-222 measurement).
- If either directory measures below 75%, add tests for the uncovered codec/database paths until it reaches 75%, then raise the floors.

## Acceptance criteria / verification
- [ ] `npx vitest run tests/scripts` passes with the new threshold cases.
- [ ] Running the script on a sample lcov above and below the floor exits 0 and 1 respectively.
- [ ] `~/go/bin/actionlint .github/workflows/ci.yml` exits 0.

## Dependencies
CCS-222 (`p0-coverage-remeasure-baseline`).
