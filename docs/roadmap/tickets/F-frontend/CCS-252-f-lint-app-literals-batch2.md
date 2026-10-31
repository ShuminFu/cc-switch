# CCS-252: [lint] Bring the app-literal suppression count to 40 or fewer

**Status:** Done · **Kind:** refactor · **Labels:** `refactor`, `phase:F`, `p2-medium`, `area:lint`

> **Ticket key:** `f-lint-app-literals-batch2` · **Phase:** F · **Priority:** p2-medium · **Effort:** M · **Depends on:** `f-lint-app-literals-batch1`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)
> Found during the Partial-ticket triage at d962272 (CCS-007). Source tickets: [CCS-007](../F-frontend/CCS-007-epic-f.md).

## Problem
After CCS-251 about 60 suppressions remain, spread over `utils/providerConfigUtils.ts` (6), `app-shell/ProvidersHome.tsx` (5), `EditProviderDialog.tsx` (5), `sessions/utils.ts` (5) and ~30 files with 1-4 each.

## Proposed change
- Convert the 3-6-count files first, then single sites, to registry capability lookups.
- After each batch run `pnpm lint:prune` and lower `APP_LITERAL_BUDGET`; finish at ≤ 40 and tick the Track F criterion in CCS-007.

## Acceptance criteria / verification
- [x] `APP_LITERAL_BUDGET` ≤ 40 and equals the suppression count; `pnpm lint` exits 0.
- [x] `pnpm vitest run tests/lint`, `CI=true npx vitest run tests/components/providerFormGoldens` and `pnpm test:unit` pass.

## Dependencies
CCS-251 (`f-lint-app-literals-batch1`).

## Implementation notes

- **Summary:** Removed all 33 `cc-switch/no-app-literal-comparison` hits in the seven files with 4-6 each. The suppression total dropped from 60 to 27. Each change uses one of two patterns:
  - **New `AppCaps` flags** in `src/entities/app/registry.ts`: `providerTerminal` (claude) and `commonConfigSnippet` (claude/codex/gemini). These sit with the existing `omo` / `setDefault` / `defaultIsSwitch` flags, which `ProvidersHome` now also uses for its OMO, terminal and set-default actions.
  - **`AppId`-keyed tables** for per-app config shapes, following the CCS-251 pattern:
    - `EDIT_BEHAVIOR` in `EditProviderDialog` (live source none/entry/current, rename by `providerKey`, keep the DB `modelCatalog`)
    - `BASE_URL_SOURCE` in `useBaseUrlState` and `ENDPOINT_SOURCE` in `useSpeedTestEndpoints`
    - `ENV_API_KEY_FIELDS` in `utils/providerConfigUtils.ts` (a `Map`, so non-app strings never hit prototype keys)
    - `SESSION_ICON_NAMES` in `sessions/utils.ts`
    - `IMPORT_LABEL_KEYS` in `ProviderEmptyState`
- **Budget:** ran `pnpm lint:prune` and set `APP_LITERAL_BUDGET` to 27. The budget test now requires the baseline total to *equal* the budget (it was `<=`) and the budget to stay at or below the Track F target of 40. The test fails against the old baseline (60 != 27). Ticked the Track F criterion in CCS-007.
- **Tests (behaviour pins):** All of these pass against both the pre-change and the changed sources. I checked this by temporarily restoring the seven old files and re-running them: 87/87 passed.
  - `tests/components/ProvidersHome.test.tsx` (new): for each app, which optional list actions `ProvidersHome` passes (remove, OMO, terminal, set-default as switch or as default model), and the `ProviderEmptyState` snippet hint and import label.
  - `tests/hooks/useBaseUrlAndSpeedTest.test.tsx` (new): which field each app's Base URL is filled from, and which endpoint candidates are collected for speed tests.
  - `tests/utils/providerConfigUtils.apiKey.test.ts` (new): get, has and set of the API key for Gemini, Codex, Claude and others.
  - `tests/components/EditProviderDialog.test.tsx`: for each app, the live-read source and whether saving renames the provider by `providerKey`.
  - `tests/components/sessionUtils.test.ts`: the session icon name for every app, plus unknown ids.
  - `tests/entities/appRegistry.test.tsx`: pins the app set for each of the two new caps.
- **Files:** src/entities/app/registry.ts, src/components/app-shell/ProvidersHome.tsx, src/components/providers/{EditProviderDialog,ProviderEmptyState}.tsx, src/components/providers/forms/hooks/{useBaseUrlState,useSpeedTestEndpoints}.ts, src/components/sessions/utils.ts, src/utils/providerConfigUtils.ts, eslint-suppressions.json, tests/lint/eslintConfig.test.ts, the tests listed above, tests/components/AppShell.test.tsx (caps literal), docs/roadmap/tickets/F-frontend/CCS-007-epic-f.md.
- **Commit:** see `git log --grep CCS-252`.
- **Verification:**
  - `pnpm typecheck`: OK.
  - `pnpm lint`: exit 0, no warnings.
  - `pnpm format:check`: clean.
  - `npx vitest run tests/lint tests/entities`: 9 files / 91 tests passed.
  - `CI=true npx vitest run tests/components/providerFormGoldens`: 467/467 passed, goldens unchanged.
  - `pnpm test:unit`: 179 files / 1786 tests passed.

