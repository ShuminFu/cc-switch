# CCS-252: [lint] Bring the app-literal suppression count to 40 or fewer

**Status:** Open · **Kind:** refactor · **Labels:** `refactor`, `phase:F`, `p2-medium`, `area:lint`

> **Ticket key:** `f-lint-app-literals-batch2` · **Phase:** F · **Priority:** p2-medium · **Effort:** M · **Depends on:** `f-lint-app-literals-batch1`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)
> Found during the Partial-ticket triage at d962272 (CCS-007). Source tickets: [CCS-007](../F-frontend/CCS-007-epic-f.md).

## Problem
After CCS-251 about 60 suppressions remain, spread over `utils/providerConfigUtils.ts` (6), `app-shell/ProvidersHome.tsx` (5), `EditProviderDialog.tsx` (5), `sessions/utils.ts` (5) and ~30 files with 1-4 each.

## Proposed change
- Convert the 3-6-count files first, then single sites, to registry capability lookups.
- After each batch run `pnpm lint:prune` and lower `APP_LITERAL_BUDGET`; finish at ≤ 40 and tick the Track F criterion in CCS-007.

## Acceptance criteria / verification
- [ ] `APP_LITERAL_BUDGET` ≤ 40 and equals the suppression count; `pnpm lint` exits 0.
- [ ] `pnpm vitest run tests/lint`, `CI=true npx vitest run tests/components/providerFormGoldens` and `pnpm test:unit` pass.

## Dependencies
CCS-251 (`f-lint-app-literals-batch1`).
