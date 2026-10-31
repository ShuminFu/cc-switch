# CCS-242: [coverage] Add a ratcheting --min threshold to coverage-summary.mjs and gate coverage-rust on it

**Status:** Done · **Kind:** chore · **Labels:** `chore`, `phase:P5`, `p3-low`, `area:coverage`

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
- [x] `npx vitest run tests/scripts` passes with the new threshold cases.
- [x] Running the script on a sample lcov above and below the floor exits 0 and 1 respectively.
- [x] `~/go/bin/actionlint .github/workflows/ci.yml` exits 0.

## Dependencies
CCS-222 (`p0-coverage-remeasure-baseline`).

## Implementation notes

**Summary.** `scripts/coverage-summary.mjs` gains a repeatable `--min <dir>=<pct>` option (exported helpers `parseMin`, `checkThresholds`, `formatFailure`, typed in `coverage-summary.d.mts`). A floor directory not listed via `--dir` is added to the table; the Markdown output gets a `Line floors (...): passed|FAILED` line, each failing directory is named on stderr (`coverage below floor: <dir>/ lines X% < min Y%`), and the script exits 1. A directory with no instrumented lines (e.g. a mistyped path) fails instead of passing silently. In `ci.yml` the `coverage-rust` job no longer has `continue-on-error: true` and the summary step passes `--min src-tauri/src/proxy=75 --min src-tauri/src/database=75`, i.e. min(75, CCS-222 baseline of 92.27% / 85.70%). Both directories are already above 75%, so no extra Rust tests were needed. Side effect: with `continue-on-error` gone, a failing test under `cargo llvm-cov --no-fail-fast` now also fails this job (the summary still runs via `if: always()`). The `coverage-frontend` job is unchanged (still report-only).

**Files.** `scripts/coverage-summary.mjs`, `scripts/coverage-summary.d.mts`, `tests/scripts/coverageSummary.test.ts`, `.github/workflows/ci.yml`, this ticket.

**Commits.** 9e65386 (implementation), plus the follow-up commit recording these notes.

**Verification**
- `npx vitest run tests/scripts` -> 10 files / 60 tests passed (new: `parseMin` parsing/validation, `checkThresholds` pass and fail incl. missing dir, CLI exit 0 above floor and exit 1 naming `database/` below floor).
- Synthetic lcov (`proxy/a.rs` LF 100): LH 80 with `--min src-tauri/src/proxy=75` -> exit 0; LH 70 -> exit 1 with `coverage below floor: src-tauri/src/proxy/ lines 70.00% < min 75%`.
- CCS-222 `coverage/rust/lcov.info` with both floors -> `passed`, exit 0.
- `~/go/bin/actionlint .github/workflows/ci.yml` -> exit 0; YAML `safe_load` ok.
- `pnpm typecheck` -> exit 0; `npx prettier --check` on the touched script/test files -> clean.
- Not verifiable locally: the gate on a real GitHub Actions run.
