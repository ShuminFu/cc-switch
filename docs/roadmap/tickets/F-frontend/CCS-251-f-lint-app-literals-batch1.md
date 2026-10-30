# CCS-251: [lint] Replace app-literal comparisons in the five largest offenders with registry capabilities

**Status:** Open · **Kind:** refactor · **Labels:** `refactor`, `phase:F`, `p2-medium`, `area:lint`

> **Ticket key:** `f-lint-app-literals-batch1` · **Phase:** F · **Priority:** p2-medium · **Effort:** M
> Plan: [next-phase-plan.md](../../next-phase-plan.md)
> Found during the Partial-ticket triage at d962272 (CCS-007). Source tickets: [CCS-007](../F-frontend/CCS-007-epic-f.md).

## Problem
`eslint-suppressions.json` holds 142 `cc-switch/no-app-literal-comparison` suppressions (`APP_LITERAL_BUDGET = 142`, `tests/lint/eslintConfig.test.ts:13`); the Track F target is ≤ 40. The five largest files hold ~82: `AddProviderDialog.tsx` (20), `ProviderList.tsx` (20), `hooks/useProviderActions.ts` (18), `ProviderCard.tsx` (13), `UsageScriptModal.tsx` (11).

## Proposed change
- Replace `appId === "claude"`-style checks in those five files with `appCaps`, `useAppCaps`, `appsWithCap`, `sharedFeatureAppOf` from `src/entities/app/registry.ts`, adding `AppCaps` flags to `APP_REGISTRY` where no capability exists yet.
- Run `pnpm lint:prune` and lower `APP_LITERAL_BUDGET` to the new total (~60).

## Acceptance criteria / verification
- [ ] `pnpm lint` exits 0 with no warnings; `pnpm vitest run tests/lint` passes with the lowered budget.
- [ ] `CI=true npx vitest run tests/components/providerFormGoldens` shows the settingsConfig goldens unchanged.
- [ ] `pnpm test:unit` passes (behaviour must not change).

## Dependencies
None.
