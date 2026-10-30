# CCS-219: [ci] Assert the resolved Node version after every setup-node step

**Status:** Open · **Kind:** chore · **Labels:** `chore`, `phase:P0`, `p3-low`, `area:ci`

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
- [ ] `pnpm vitest run tests/ci` passes and fails if a check step is removed.
- [ ] Running the check locally (Node v22.22.0) exits non-zero, showing it catches a mismatch.
- [ ] `~/go/bin/actionlint .github/workflows/*.yml` exits 0.

## Dependencies
None.
