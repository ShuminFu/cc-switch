# CCS-219: [ci] Assert the resolved Node version after every setup-node step

**Status:** Done · **Kind:** chore · **Labels:** `chore`, `phase:P0`, `p3-low`, `area:ci`

> **Ticket key:** `p0-ci-node-version-assert` · **Phase:** P0 · **Priority:** p3-low · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)
> Found during the Partial-ticket triage at d962272 (CCS-026). Source tickets: [CCS-026](../P0-safety-net/CCS-026-p0-ci-node-version.md).

## Problem
Every workflow reads `node-version-file: .node-version` (ci.yml:32,217,308; audit.yml:47; release.yml:49,81; proxy-fixture.yml:34) and `tests/ci/nodeVersion.test.ts` guards the YAML, but the CCS-026 criterion "the CI log prints v22.12.0" can only be seen on a real run, and a setup-node resolution drift would pass silently.

## Proposed change
- After each setup-node step (ci.yml 30/215/306, audit.yml 45, release.yml 47/79, proxy-fixture.yml 32) add `run: node --version && test "$(node --version)" = "v$(cat .node-version)"`.
- Extend `tests/ci/nodeVersion.test.ts` to require the check step right after every setup-node step.
- This is hardening; the CCS-026 box still needs one real Actions log.

## Acceptance criteria / verification
- [x] `pnpm vitest run tests/ci` passes and fails if a check step is removed.
- [x] Running the check locally (Node v22.22.0) exits non-zero, showing it catches a mismatch.
- [x] `~/go/bin/actionlint .github/workflows/*.yml` exits 0.

## Dependencies
None.

## Implementation notes
- **Summary:** every `actions/setup-node` step (ci.yml frontend / tauri-build-smoke / coverage-frontend, audit.yml pnpm-audit, release.yml check-versions / release, proxy-fixture.yml) is now immediately followed by a `Check resolved Node version` step that runs `node --version` and then `test "$(node --version)" = "v$(tr -d '[:space:]' < .node-version)"`. The step sets `shell: bash` because the release matrix includes Windows runners (default shell pwsh); `tr -d '[:space:]'` instead of plain `cat` tolerates a CRLF `.node-version` from the `* text=auto` checkout on Windows. `tests/ci/nodeVersion.test.ts` parses each workflow with js-yaml and requires, for every job, that the step after each setup-node step is such a bash check step.
- **Files:** `.github/workflows/{ci,audit,release,proxy-fixture}.yml`, `tests/ci/nodeVersion.test.ts`.
- **Commits:** ed70c1b (workflows + test); this ticket-file update follows in a docs commit.
- **Verification:**
  - `pnpm vitest run tests/ci` → 15 files / 140 tests passed. With the release.yml `check-versions` check step removed: `release.yml asserts the resolved Node version right after setup-node` fails (1 failed / 17 passed); restored afterwards.
  - Local check with Node v22.22.0: `bash -c 'node --version; test "$(node --version)" = "v$(tr -d "[:space:]" < .node-version)"'` prints `v22.22.0` and exits 1 (mismatch with 22.12.0 caught).
  - `~/go/bin/actionlint .github/workflows/*.yml` → exit 0; every workflow parses with `yaml.safe_load`.
  - `pnpm typecheck` clean, `pnpm format:check` clean, `pnpm test:unit` → 171 files / 1624 tests passed.
  - Still outstanding (outside this ticket, per CCS-026): one real Actions run log showing `v22.12.0`.
