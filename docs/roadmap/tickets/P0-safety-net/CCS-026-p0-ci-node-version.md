# CCS-026: [ci] Read the Node version from .node-version in CI and release

**Status:** Partial · **Kind:** chore · **Labels:** `chore`, `phase:P0`, `p3-low`, `area:ci`

> **Ticket key:** `p0-ci-node-version` · **Phase:** P0 · **Priority:** p3-low · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding ci-node-version-mismatch. CI and release use Node "20" (EOL April 2026), while .node-version pins 22.12.0.

## Evidence
- ci.yml:22-24, release.yml:36-38 `node-version: 20`; .node-version 22.12.0

## Proposed change
Switch to `node-version-file: .node-version` and add `engines.node >=22.12.0` to package.json.

## Acceptance criteria / verification
- [ ] `grep -n node-version .github/workflows/*.yml` shows only node-version-file
- [ ] The CI log prints v22.12.0

## Dependencies
None.

## Implementation notes
**Summary:** Every `actions/setup-node` step (ci.yml x3, audit.yml, release.yml) now uses `node-version-file: .node-version` instead of a hardcoded `"20"`, so `.node-version` (22.12.0) is the only Node version source. `package.json` declares `"engines": { "node": ">=22.12.0" }`. A vitest guard (`tests/ci/nodeVersion.test.ts`) asserts that no workflow contains a literal `node-version:` key, that each setup-node step has a matching `node-version-file: .node-version`, and that `engines.node` is `>=<.node-version>`.

**Files:** `.github/workflows/ci.yml`, `.github/workflows/audit.yml`, `.github/workflows/release.yml`, `package.json`, `tests/ci/nodeVersion.test.ts`.

**Commits:** `2d8443c` (code + test); this ticket-file update follows as a separate commit.

**Verification:**
- `grep -n node-version .github/workflows/*.yml`: only `node-version-file: .node-version` lines remain (ci.yml:26,174,245; audit.yml:47; release.yml:46).
- `pnpm vitest run tests/ci`: 8/8 pass. With the workflow changes reverted it fails 3/8 (audit/ci/release), so the test catches the regression.
- `pnpm test:unit`: 80 files, 538 tests pass. `pnpm typecheck` and `pnpm format:check` are clean.
- Every workflow YAML parses with `yaml.safe_load`. actionlint is not installed here, so it was not run.

**Remaining (Partial):** The criterion "the CI log prints v22.12.0" can only be checked on a real GitHub Actions run. Look at the "Setup Node.js" step output on the next CI run.

### Remaining after triage

- Blocked: The CI log prints v22.12.0. Needs a real GitHub Actions run (CI triggers only on main/PRs; the repo has no Actions history).
- Follow-up [CCS-219](../P0-safety-net/CCS-219-p0-ci-node-version-assert.md): Assert the resolved Node version after every setup-node step.
