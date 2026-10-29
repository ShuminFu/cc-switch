# CCS-211: [lint] Make pnpm lint green again after the P1/P2 merges

**Status:** Done · **Kind:** chore · **Labels:** `chore`, `phase:F`, `p1-high`, `area:frontend`

> **Ticket key:** `f-lint-green` · **Phase:** F · **Priority:** p1-high · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)
> Found while verifying the lane-2 merge (348c874). The failure already existed on both merge parents.

## Problem
`pnpm lint` reports 20 errors, so the CI lint gate from CCS-017/CCS-120 is red:
- 19 × `cc-switch/no-app-literal-comparison` in `src/App.tsx` (81, 147, 348), `src/components/DeepLinkImportDialog.tsx` (340-723) and `src/components/providers/ProviderActions.tsx` (95-267). Code added in P1 and the frontend track compares app ids against string literals, and the suppression baseline `eslint-suppressions.json` holds lower counts than the code now has. ESLint also warns that some suppressions no longer match anything.
- 1 × `react-hooks/exhaustive-deps` at `DeepLinkImportDialog.tsx:175`: `requestSeqRef.current` is read in an effect cleanup (introduced by CCS-071).

## Proposed change
- Replace the literal comparisons with registry lookups (`appCaps`, `appsWithCap`, `isAppId`, or new capabilities in `src/entities/app/registry.ts`, such as `additive` or `deeplinkEnvConfig`). Do not raise the suppression counts.
- Fix the cleanup ref read by copying the ref into a local variable inside the effect, keeping the CCS-071 stale-request guard semantics.
- Prune stale suppressions (`--prune-suppressions`), so the baseline only goes down.

## Acceptance criteria / verification
- [x] `pnpm lint` exits 0 with no "suppressions left" warning.
- [x] Total suppressed `no-app-literal-comparison` count does not increase relative to `eslint-suppressions.json` at 348c874.
- [x] `pnpm typecheck`, `pnpm test:unit` and `pnpm format:check` pass, including the existing DeepLinkImportDialog stale-request tests from CCS-071.

## Dependencies
None.

## Implementation notes
**Summary.** The 19 `no-app-literal-comparison` hits were replaced with registry lookups instead of raising suppressions:
- `src/entities/app/registry.ts`: new caps `proxyTakeover` (false for Claude Desktop and additive apps), `healthCheck` (OpenClaw), `setDefault` (OpenClaw, Hermes), `defaultIsSwitch` (Hermes), plus an optional `featureHost` descriptor field with `sharedFeatureAppOf()` (Claude Desktop -> Claude).
- `src/App.tsx`: shared feature app, OpenClaw health banner and palette takeover toggle use those caps (behaviour unchanged).
- `src/components/providers/ProviderActions.tsx`: additive mode is `appCaps(appId).additive && !isOmo` (every use already handles OMO first, so it is equivalent); the default/enable button and its labels use `setDefault` / `defaultIsSwitch`.
- `src/components/DeepLinkImportDialog.tsx`: a per-app `DEEPLINK_APP_FORMATS` table (guarded by `isAppId`) picks the config parser and the tiered Haiku/Sonnet/Opus model fields. The `ParsedConfig.type` discriminants were renamed to `claudeEnv` / `codexToml` / `geminiEnv`. The effect holds `requestSeqRef` in a local `requestSeq` used by both the listener and the cleanup, which fixes `react-hooks/exhaustive-deps` with the same CCS-071 stale-request semantics.
- `eslint-suppressions.json` pruned with `pnpm lint:prune`: the entries for the three files are gone, and so are stale entries for `deeplink/McpConfirmation.tsx`, `useCommonConfigSnippet.ts` and `lib/schemas/common.ts`. The app-literal suppression total went from 158 (at 348c874) to 142. `APP_LITERAL_BUDGET` in `tests/lint/eslintConfig.test.ts` went from 173 to 142.

**Tests.** `tests/lint/eslintConfig.test.ts` has a new test that lints the three cleaned files and checks they have no baseline entry. It fails before the change (the 20 errors above) and passes after. `tests/entities/appRegistry.test.tsx` covers the new caps and `sharedFeatureAppOf`. `tests/components/ProviderSwitchingState.test.tsx` adds tests for the OpenClaw set-as-default label and enabled state, and checks that no default button appears for apps without `setDefault`. The `AppShell.test.tsx` caps fixture was extended.

**Commits.** d339160 (code + tests + baseline), plus this ticket-file follow-up.

**Verification.**
- `pnpm lint`: exit 0, with no "suppressions left" warning.
- `pnpm typecheck`: passes.
- `pnpm format:check`: all files formatted.
- `pnpm test:unit`: 155 files / 1513 tests passed, including the existing DeepLinkImportDialog stale-request tests.

