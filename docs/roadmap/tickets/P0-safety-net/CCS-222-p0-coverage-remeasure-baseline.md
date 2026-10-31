# CCS-222: [coverage] Re-measure the coverage baseline on a clean tree and record proxy/ and database/ numbers

**Status:** Done · **Kind:** chore · **Labels:** `chore`, `phase:P0`, `p3-low`, `area:coverage`

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
- [x] `docs/dev/perf-baseline.json` numbers match `coverage/rust/summary-by-dir.json`; no `_caveat`.
- [x] `npx vitest run tests/scripts` passes.
- [x] proxy/ and database/ line coverage recorded in CCS-019 and CCS-006.

## Dependencies
None.

## Implementation notes

**Summary.** Re-measured Rust and frontend coverage on a clean checkout of a64ab58 (`git status` empty; the parallel lane works in a separate worktree) and refreshed `coverage` in `docs/dev/perf-baseline.json` (`recorded_at` 2026-10-31, `commit` a64ab58, `_caveat` removed, Rust and frontend `dirs` rewritten from the two `summary-by-dir.json` files). No code or CI changes.

| Area | Lines | Functions | P0 baseline (lines) |
| --- | ---: | ---: | ---: |
| `src-tauri/src/proxy/` | 92.27% (49568/53723) | 91.44% (5434/5943) | 78.60% |
| `src-tauri/src/database/` | 85.70% (7709/8995) | 59.21% (662/1118) | 78.84% |
| `src-tauri/src/services/` | 68.32% (18330/26831) | 65.61% (1820/2774) | 67.40% |
| `src-tauri/src/commands/` | 25.15% (1154/4589) | 12.58% (121/962) | 33.30% |
| Rust total | 81.21% (103668/127647) | 75.14% (11377/15142) | 70.15% |
| Frontend `src/` total | 67.25% (43771/65086) | 54.65% (1141/2088) | 52.47% |

proxy/ and database/ are both above the P5 75% line target; the ratchet itself is CCS-242.

**Disk.** Instead of `cargo clean`, stale artifacts in `src-tauri/target/debug/{deps,build,.fingerprint}` were pruned: any unit whose metadata hash did not appear in the JSON messages of no-op `cargo test --no-run`, `cargo clippy` and `cargo build` runs (or in a hard-linked binary) was deleted (11G -> 4.9G; a follow-up `cargo test --no-run` and `cargo clippy` stayed fresh). The instrumented build ran with `CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0` (coverage mapping does not depend on debuginfo) and `src-tauri/target/llvm-cov-target` (3.7G) was deleted afterwards.

**Files.** `docs/dev/perf-baseline.json`, `docs/roadmap/tickets/P0-safety-net/CCS-019-p0-ci-coverage-report.md` (re-measured baseline table), `docs/roadmap/tickets/P5-protocol-ir-commands/CCS-006-epic-p5.md` (criterion 5 audit and next step), this ticket.

**Commits.** 36a57bc (baseline refresh), plus the follow-up commit recording these notes.

**Verification**
- `cargo llvm-cov --manifest-path src-tauri/Cargo.toml --no-fail-fast --lcov --output-path coverage/rust/lcov.info` (cargo-llvm-cov 0.9.1, rustc 1.95.0) -> exit 0; 25 test binaries, 3114 passed, 0 failed, 3 ignored; report written.
- `node scripts/coverage-summary.mjs --title "Rust (cargo-llvm-cov)" --lcov coverage/rust/lcov.info --dir src-tauri/src/proxy --dir src-tauri/src/database --dir src-tauri/src/services --dir src-tauri/src/commands --json coverage/rust/summary-by-dir.json` -> table above.
- `pnpm test:coverage` -> 167 files / 1581 tests passed; `node scripts/coverage-summary.mjs --title "Frontend (vitest)" --lcov coverage/frontend/lcov.info --dir src/components --dir src/hooks --dir src/lib --json coverage/frontend/summary-by-dir.json` -> components 56.30%, hooks 60.91%, lib 49.83%, total 67.25% lines.
- Python check that every row of both `summary-by-dir.json` files equals the matching `perf-baseline.json` entry (files, lines, functions, both pcts) and that no `_caveat` remains -> ok.
- `npx vitest run tests/scripts` -> 10 files / 55 tests passed.
- `npx prettier --write docs/dev/perf-baseline.json` -> formatted.
- Not done: the optional `act` dry-run (`act` is not installed); CCS-019 AC1 still needs a real GitHub Actions PR run.

