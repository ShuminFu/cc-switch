# CCS-216: [deps] Upgrade vitest 2.x to >=3.2.6 and drop the 4 ignored GHSAs from pnpm audit

**Status:** Open · **Kind:** security · **Labels:** `security`, `phase:P0`, `p2-medium`, `area:deps`

> **Ticket key:** `p0-deps-vitest-upgrade` · **Phase:** P0 · **Priority:** p2-medium · **Effort:** M
> Plan: [next-phase-plan.md](../../next-phase-plan.md)
> Found during the Partial-ticket triage at d962272 (CCS-018). Source tickets: [CCS-018](../P0-safety-net/CCS-018-p0-ci-cargo-deny.md).

## Problem
`package.json:54` has `vitest ^2.0.5` and `package.json:38` has `@vitest/coverage-v8 ^2.1.9`. To keep `pnpm audit --audit-level=high` green, `pnpm-workspace.yaml` `auditConfig.ignoreGhsas` lists GHSA-5xrq-8626-4rwp, GHSA-5gmw-xhrv-c9v3, GHSA-85c8-ppgw-ccpr and GHSA-587h-7wmj-h5pm, all fixed by vitest >=3.2.6 (tinypool >=2.1.2, vite >=6.4.4).

## Proposed change
- Bump `vitest` and `@vitest/coverage-v8` together to the same major (>=3.2.6, or 4); the vite 7 already in use is compatible.
- Fix any config or API breakage in `vitest.config.*` and the test setup.
- Remove the 4 ignore entries, and the `auditConfig` block if it ends up empty.

## Acceptance criteria / verification
- [ ] `pnpm install` and `pnpm test:unit` pass (App.test.tsx is known to be flaky; re-run once before treating it as a regression).
- [ ] `pnpm test:coverage` and `pnpm typecheck` pass.
- [ ] `pnpm audit --audit-level=high` exits 0 with no `ignoreGhsas` (check with pnpm 10.12.3, the CI version).

## Dependencies
None.
