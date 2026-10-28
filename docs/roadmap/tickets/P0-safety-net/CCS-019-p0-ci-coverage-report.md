# CCS-019: [ci] Publish report-only coverage from cargo-llvm-cov and vitest

**Status:** Partial · **Kind:** chore · **Labels:** `chore`, `phase:P0`, `p3-low`, `area:ci`

> **Ticket key:** `p0-ci-coverage-report` · **Phase:** P0 · **Priority:** p3-low · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
The plan's P5 target of 75% coverage for proxy/ and database/ needs a baseline from now on.

## Proposed change
- Add a CI job running `cargo llvm-cov --manifest-path src-tauri/Cargo.toml --lcov` and `vitest --coverage`, uploading both as artifacts.
- Report only for now; ratchet in P5.

## Acceptance criteria / verification
- [ ] Coverage artifacts appear on PR runs, with per-directory summaries for proxy/ and database/
- [x] Baseline numbers recorded in docs/dev/perf-baseline.json or the README for dev

## Dependencies
None.

## Implementation notes

**Status rationale:** Partial only because the first acceptance item ("artifacts appear on PR runs") can only be observed on a real GitHub Actions PR run; everything is implemented and verified locally.

**Change**
- `.github/workflows/ci.yml`: two new report-only jobs (`continue-on-error: true`, no thresholds):
  - `coverage-frontend` (ubuntu-latest): `pnpm test:coverage` (vitest + v8), per-directory summary for `src/components`, `src/hooks`, `src/lib` appended to `$GITHUB_STEP_SUMMARY`, uploads `coverage/frontend/` as artifact `coverage-frontend`.
  - `coverage-rust` (ubuntu-22.04): installs the pinned toolchain + `llvm-tools-preview`, `cargo-llvm-cov` via `taiki-e/install-action@v2`, runs `cargo llvm-cov --manifest-path src-tauri/Cargo.toml --no-fail-fast --lcov --output-path coverage/rust/lcov.info`, per-directory summary for `src-tauri/src/{proxy,database,services,commands}`, uploads `coverage/rust/` as artifact `coverage-rust`.
- `scripts/coverage-summary.mjs` (+ `.d.mts` types): dependency-free LCOV parser that prints a Markdown table (lines/functions per directory + total) and optionally writes `summary-by-dir.json`.
- `vitest.config.ts`: v8 provider, `text-summary`/`lcov`/`json-summary` reporters, output in `coverage/frontend`, `include: src/**`.
- `package.json` / `pnpm-lock.yaml`: `@vitest/coverage-v8@^2.1.9` and `test:coverage` script.
- `.gitignore` and `eslint.config.js`: ignore `coverage/`.
- `docs/dev/perf-baseline.json`: `coverage` baseline (below), with refresh instructions.
- `tests/scripts/coverageSummary.test.ts`: unit tests for the summary script (absolute vs relative SF paths, sibling-prefix exclusion, n/a for empty dirs, Markdown output).

**Baseline** (commit 6330783 working tree, Linux x86_64)

| Area | Lines | Functions |
| --- | ---: | ---: |
| `src-tauri/src/proxy/` | 78.60% | 79.36% |
| `src-tauri/src/database/` | 78.84% | 42.58% |
| Rust total | 70.15% | 62.77% |
| Frontend `src/` total | 52.47% | 38.33% |

**Commits:** 933c8ef (implementation), plus the follow-up commit recording these notes.

**Verification**
- `cargo llvm-cov --manifest-path src-tauri/Cargo.toml --no-fail-fast --lcov --output-path coverage/rust/lcov.info` -> exit 0, 2179 tests passed, 0 failed; lcov written.
- `pnpm test:coverage` -> 79 files / 530 tests passed; `coverage/frontend/lcov.info` + `coverage-summary.json` written.
- `node scripts/coverage-summary.mjs ...` on both reports -> tables above.
- `npx vitest run tests/scripts` -> 3/3 passed.
- `pnpm typecheck`, `pnpm lint`, `pnpm format:check` -> pass. `pnpm test:unit` -> pass (two `tests/integration` cases timed out once while the instrumented cargo build saturated the CPU; re-run green 9/9).
- `python3 -c "import yaml..." .github/workflows/ci.yml` -> ok; `actionlint .github/workflows/ci.yml` -> no findings.

**Review** (fix(ci): review follow-up for CCS-019): (1) `coverage.exclude: ["src/**/*.d.ts"]` replaced vitest's default excludes, so colocated `src/**/*.test.ts` files were counted as covered source; it now spreads `coverageConfigDefaults.exclude`. (2) vitest skips writing the report when any test fails (`reportOnFailure` defaults to false), so one flaky test would leave the report-only job with no artifact; set `reportOnFailure: true`. (3) Under v8 instrumentation `tests/integration/App.test.tsx` reproducibly exceeded the 5s default test timeout (5.1s), so `test:coverage` now passes `--testTimeout=20000`. Frontend baseline refreshed (52.68% -> 52.47% lines, `src/lib` 34.23% -> 33.51%); the Rust baseline is unchanged but flagged in perf-baseline.json as measured with the parallel lane's uncommitted proxy test changes in the tree. Re-verified: `pnpm test:coverage` 79 files / 530 tests green with report written, `pnpm typecheck`, `pnpm lint`, `pnpm format:check`, `pnpm test:unit`, `npx vitest run tests/scripts` pass. Status stays Partial (PR-run artifact check still needs GitHub Actions).

**Remaining:** confirm on the first PR run that both artifacts upload and the job summaries render (needs GitHub Actions).
