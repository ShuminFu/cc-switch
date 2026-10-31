# CCS-216: [deps] Upgrade vitest 2.x to >=3.2.6 and drop the 4 ignored GHSAs from pnpm audit

**Status:** Done · **Kind:** security · **Labels:** `security`, `phase:P0`, `p2-medium`, `area:deps`

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
- [x] `pnpm install` and `pnpm test:unit` pass (App.test.tsx is known to be flaky; re-run once before treating it as a regression).
- [x] `pnpm test:coverage` and `pnpm typecheck` pass.
- [x] `pnpm audit --audit-level=high` exits 0 with no `ignoreGhsas` (check with pnpm 10.12.3, the CI version).

## Dependencies
None.

## Implementation notes
- **Change:** `vitest` and `@vitest/coverage-v8` bumped from 2.x to `^4.1.12` (resolved 4.1.12, deduped onto the existing vite 7.3.7). vitest 3.2.7 was tried first but still depends on `tinypool ^1.1.1`, so GHSA-5gmw-xhrv-c9v3 / GHSA-85c8-ppgw-ccpr stayed open; vitest 4 no longer uses tinypool. The whole `auditConfig.ignoreGhsas` block was removed from `pnpm-workspace.yaml`, and the `audit.yml` comment now says no advisories are ignored.
- **Breakage fixed:** vitest 4's `vitest/globals` types no longer pull in `@types/node`, so `tsconfig.json` lists `"node"` in `types` explicitly (tests/scripts/* use `node:*` modules and `__dirname`). `tests/hooks/useSettingsForm.test.tsx` types its `changeLanguage` spy as `MockInstance<typeof i18n.changeLanguage>` instead of `ReturnType<typeof vi.spyOn<any, any>>` (stricter spy generics); this removed 2 `no-explicit-any` hits, so `eslint-suppressions.json` was pruned. `vitest.config.ts` needed no change (explicit `coverage.include` already set; no test files appear in lcov).
- **Files:** `package.json`, `pnpm-lock.yaml`, `pnpm-workspace.yaml`, `tsconfig.json`, `tests/hooks/useSettingsForm.test.tsx`, `eslint-suppressions.json`, `.github/workflows/audit.yml`.
- **Commits:** 75099a3 (upgrade), plus this ticket-notes commit.
- **Verification (pnpm 10.12.3, node 22.22.0):**
  - `pnpm install --frozen-lockfile`: ok.
  - `pnpm audit --audit-level=high` with no `ignoreGhsas`: exit 0 (3 remaining: 2 low, 1 moderate). With vitest 3.2.7 it had failed with 2 critical tinypool advisories.
  - `pnpm test:unit`: 167 files / 1578 tests passed.
  - `pnpm test:coverage`: 167 files / 1578 tests passed; summary written (statements 56.35%, lines 57.19%).
  - `pnpm typecheck`, `pnpm lint`, `pnpm format:check`: pass. YAML of `audit.yml` and `pnpm-workspace.yaml` parses.
- **Note:** `docs/dev/perf-baseline.json` still records "vitest 2.1.9" as the tool used for that historical baseline; left unchanged on purpose.
