# CCS-251: [lint] Replace app-literal comparisons in the five largest offenders with registry capabilities

**Status:** Done · **Kind:** refactor · **Labels:** `refactor`, `phase:F`, `p2-medium`, `area:lint`

> **Ticket key:** `f-lint-app-literals-batch1` · **Phase:** F · **Priority:** p2-medium · **Effort:** M
> Plan: [next-phase-plan.md](../../next-phase-plan.md)
> Found during the Partial-ticket triage at d962272 (CCS-007). Source tickets: [CCS-007](../F-frontend/CCS-007-epic-f.md).

## Problem

`eslint-suppressions.json` holds 142 `cc-switch/no-app-literal-comparison` suppressions (`APP_LITERAL_BUDGET = 142`, `tests/lint/eslintConfig.test.ts:13`); the Track F target is ≤ 40. The five largest files hold ~82: `AddProviderDialog.tsx` (20), `ProviderList.tsx` (20), `hooks/useProviderActions.ts` (18), `ProviderCard.tsx` (13), `UsageScriptModal.tsx` (11).

## Proposed change

- Replace `appId === "claude"`-style checks in those five files with `appCaps`, `useAppCaps`, `appsWithCap`, `sharedFeatureAppOf` from `src/entities/app/registry.ts`, adding `AppCaps` flags to `APP_REGISTRY` where no capability exists yet.
- Run `pnpm lint:prune` and lower `APP_LITERAL_BUDGET` to the new total (~60).

## Acceptance criteria / verification

- [x] `pnpm lint` exits 0 with no warnings; `pnpm vitest run tests/lint` passes with the lowered budget.
- [x] `CI=true npx vitest run tests/components/providerFormGoldens` shows the settingsConfig goldens unchanged.
- [x] `pnpm test:unit` passes (behaviour must not change).

## Dependencies

None.

## Implementation notes

- **Summary:** All 82 `cc-switch/no-app-literal-comparison` hits in the five files are gone. Boolean differences between apps now come from new `AppCaps` flags in `src/entities/app/registry.ts`: `universalProviders` (claude/codex/gemini), `defaultModel` (openclaw), `omo` (opencode), `configStatus` (claude-desktop) and `pluginSync` (claude). These sit next to the existing `additive` / `setDefault` / `defaultIsSwitch` flags. Behaviour that depends on each app's settingsConfig shape now goes through tables keyed by `AppId`, following the same pattern as `entities/provider/invalidate.ts`:
  - `src/entities/provider/settingsConfig.ts` (new): reads API key and base URL per app (shared by `UsageScriptModal` and the `AddProviderDialog` endpoint fallback), plus the official-login checks (shared by `ProviderCard` and `UsageScriptModal`; this replaces the duplicated `isOfficialProvider` logic and the `["claude","codex","gemini"].includes` lists).
  - `src/entities/provider/liveIds.ts` (new): the live-provider-id queries for additive apps. This table moved here from `useProviderShellActions`, and `ProviderList` now uses it through a single `useQuery` instead of three app-gated hooks.
  - Local tables: `PRESET_SOURCES` (AddProviderDialog: preset loaders, endpoint candidates, official-seed flags), `IMPORT_FROM_LIVE` (ProviderList), `ROUTING_BADGES` / `READ_ONLY_PROVIDER_CHECKS` (ProviderCard), `PROXY_REQUIRED_REASONS` / `SWITCH_SUCCESS_MESSAGES` (useProviderActions).
- `pnpm lint:prune`: the app-literal total dropped from 142 to 60, and `APP_LITERAL_BUDGET` was lowered to 60. The no-explicit-any / exhaustive-deps counts for the touched files also went down.
- **Tests:** `tests/entities/providerSettingsConfig.test.ts` (new: credentials for every app, the official-login matrix); `appRegistry.test.tsx` (pins which apps have each capability flag); `ProviderList.test.tsx` (Hermes live ids / model.provider, OpenClaw default model, a switch-mode app); `AddProviderDialog.test.tsx` (per-app base-URL endpoint fallback, universal-tab visibility, OpenClaw-only suggestedDefaults). The budget test fails against the old baseline (142 > 60).
- **Files:** src/entities/app/registry.ts, src/entities/provider/{settingsConfig,liveIds}.ts, src/components/providers/{AddProviderDialog,ProviderList,ProviderCard}.tsx, src/components/UsageScriptModal.tsx, src/hooks/{useProviderActions,useProviderShellActions}.ts, eslint-suppressions.json, tests listed above + tests/components/AppShell.test.tsx, tests/lint/eslintConfig.test.ts.
- **Commit:** see `git log --grep CCS-251`.
- **Verification:** `pnpm typecheck` OK; `pnpm lint` exit 0 with no warnings; `pnpm format:check` clean; `npx vitest run tests/lint tests/entities` 89/89 passed; `CI=true npx vitest run tests/components/providerFormGoldens` 467/467 passed with goldens unchanged; `pnpm test:unit` 175 files / 1706 tests passed.
