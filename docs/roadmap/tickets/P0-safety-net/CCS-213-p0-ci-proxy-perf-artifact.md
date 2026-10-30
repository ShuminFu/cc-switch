# CCS-213: [ci] Upload the proxy_e2e perf JSON as a CI artifact from the backend matrix

**Status:** Partial · **Kind:** chore · **Labels:** `chore`, `phase:P0`, `p3-low`, `area:ci`

> **Ticket key:** `p0-ci-proxy-perf-artifact` · **Phase:** P0 · **Priority:** p3-low · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)
> Found during the Partial-ticket triage at d962272 (CCS-014, CCS-001). Source tickets: [CCS-014](../P0-safety-net/CCS-014-p0-proxy-harness-e2e.md), [CCS-001](../P0-safety-net/CCS-001-epic-p0.md).

## Problem
`proxy_e2e` writes its perf baseline to `env!("CARGO_TARGET_TMPDIR")/proxy-e2e-perf.json` (`src-tauri/tests/proxy_e2e.rs:2009`), which resolves to `src-tauri/target/tmp/proxy-e2e-perf.json`. Nothing in CI keeps it: `grep -c proxy-e2e-perf .github/workflows/ci.yml` returns 0. The `backend` matrix job (ci.yml ~93-164) ends with "Run tests" (`cargo test --manifest-path src-tauri/Cargo.toml`) and has no upload step. CCS-014 handed this to the CI lane and it was never picked up, so the per-OS perf numbers from the matrix are thrown away.

## Proposed change
- In `.github/workflows/ci.yml`, after the `backend` job's "Run tests" step, add:
  - `name: Upload proxy e2e perf artifact`, `if: always()`
  - `uses: actions/upload-artifact@cf430e030ddbb5b0abf93d22962f4752f3646cd9 # v7.0.2` (the SHA already pinned at ci.yml:288/345/412)
  - `with:` `name: proxy-e2e-perf-${{ matrix.os }}`, `path: src-tauri/target/tmp/proxy-e2e-perf.json`, `if-no-files-found: warn`
- Update the CCS-014 notes: the artifact step is in place; only the real-run confirmation stays blocked on GitHub Actions.

## Acceptance criteria / verification
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --test proxy_e2e perf_baseline` leaves `src-tauri/target/tmp/proxy-e2e-perf.json` on disk.
- [ ] `~/go/bin/actionlint .github/workflows/ci.yml` exits 0.
- [ ] `pnpm vitest run tests/ci` passes (SHA-pin guards included).

## Dependencies
None.

## Implementation notes

**Summary.** The `backend` job in `.github/workflows/ci.yml` now has an `Upload proxy e2e perf artifact` step right after "Run tests". It runs with `if: always()`, uses `actions/upload-artifact@cf430e030ddbb5b0abf93d22962f4752f3646cd9 # v7.0.2` (the SHA already pinned elsewhere in ci.yml), and uploads `src-tauri/target/tmp/proxy-e2e-perf.json` as `proxy-e2e-perf-${{ matrix.os }}` with `if-no-files-found: warn`. A new guard, `tests/ci/proxyPerfArtifact.test.ts`, checks two things: that `proxy_e2e.rs` still writes `proxy-e2e-perf.json` under `CARGO_TARGET_TMPDIR`, and that the backend step exists after "Run tests" with the right condition, pinned action, name, path and missing-file policy. The CCS-014 notes now record that the artifact step is in place.

**Files.** `.github/workflows/ci.yml`, `tests/ci/proxyPerfArtifact.test.ts`, `docs/roadmap/tickets/P0-safety-net/CCS-014-p0-proxy-harness-e2e.md`, this ticket.

**Commits.** `3de35b2` (workflow step, guard test, CCS-014 notes), plus the follow-up commit that records these notes.

**Verification.**
- `cargo test --manifest-path src-tauri/Cargo.toml --test proxy_e2e perf_baseline`: 1 passed. After deleting the file first, `src-tauri/target/tmp/proxy-e2e-perf.json` (377 bytes) is back on disk.
- `~/go/bin/actionlint .github/workflows/ci.yml`: exit 0. The YAML parses with `yaml.safe_load`.
- `pnpm vitest run tests/ci`: 11 files, 94 tests passed (SHA-pin guards included). The new guard fails against the pre-change `ci.yml` (1 failed / 1 passed) and passes after the change.
- `pnpm typecheck`: clean. `prettier --check` on the new test file and `ci.yml`: clean.

**Remaining (Partial).** Confirming that the artifact really uploads on ubuntu-22.04, windows-latest and macos-latest needs a GitHub Actions run of the backend matrix. That cannot be done in this environment, and the repo has no Actions history.
