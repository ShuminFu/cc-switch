# CCS-213: [ci] Upload the proxy_e2e perf JSON as a CI artifact from the backend matrix

**Status:** Open · **Kind:** chore · **Labels:** `chore`, `phase:P0`, `p3-low`, `area:ci`

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
