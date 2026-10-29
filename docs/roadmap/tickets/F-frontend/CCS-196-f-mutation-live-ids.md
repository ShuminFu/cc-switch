# CCS-196: [query] Invalidate the OpenCode and OpenClaw live-provider-id caches on add, update and delete

**Status:** Done · **Kind:** bug · **Labels:** `bug`, `phase:F`, `p3-low`, `area:frontend`

> **Ticket key:** `f-mutation-live-ids` · **Phase:** F · **Priority:** p3-low · **Effort:** S · **Depends on:** `f-query-keys`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding frontend-mutations-missing-liveid-invalidation, partially real. After adding an OpenCode or OpenClaw provider (addToLive defaults to true), the mounted list shows it as "not in config" until the window refocuses.

## Evidence
- src/lib/query/mutations.ts:85-130, 158-175, 205-231 vs switch :280-302; ProviderList.tsx:100-109

## Proposed change
Call invalidateProviderGraph(appId) in all four mutations and in the App.tsx remove-from-live path.

## Acceptance criteria / verification
- [x] Vitest: for add, update and delete on opencode and openclaw, the spy sees the live-id keys
- [ ] Manual: the card shows as in-config immediately
- [x] `pnpm test:unit`

## Dependencies
f-query-keys

## Implementation notes

**Summary.** The four provider mutations in `src/lib/query/mutations.ts`
(add/update/delete/switch) already call `invalidateProviderGraph(queryClient, appId)`
since CCS-194/CCS-195, and that helper invalidates `providerKeys.opencodeLiveIds`
(OpenCode) and `openclawKeys.liveProviderIds` (OpenClaw). The remaining gap was the
App.tsx remove-from-live-config path, which hand-rolled a per-app subset (no provider
list, no OpenClaw default model, no OMO / Hermes model config). It now calls
`invalidateProviderGraph(queryClient, activeApp)`. Removing the three app-literal
comparisons lowered the `cc-switch/no-app-literal-comparison` suppression count for
`src/App.tsx` from 21 to 18 (pruned in `eslint-suppressions.json`).

**Files.**
- `src/App.tsx` — remove-from-live path uses `invalidateProviderGraph`.
- `eslint-suppressions.json` — pruned stale suppressions for `src/App.tsx`.
- `tests/hooks/providerMutationLiveIds.test.tsx` (new) — add/update/delete × opencode/openclaw; the `invalidateQueries` spy sees the live-id key and the provider list key.
- `tests/integration/App.test.tsx` — new case: removing an OpenClaw provider from live config invalidates the provider list, live ids, default model and health. Fails against the previous App.tsx (verified by swapping in `HEAD~` App.tsx), passes after.

**Commits.** `c516165` (code + tests); ticket notes in the follow-up docs commit.

**Verification.**
- `npx vitest run tests/hooks/providerMutationLiveIds.test.tsx tests/integration/App.test.tsx` — 12 passed.
- `pnpm test:unit` — 113 files, 1203 tests passed.
- `pnpm typecheck` — clean. `pnpm format:check` — clean. `npx eslint` on touched files — clean (after pruning suppressions).
- Manual check ("the card shows as in-config immediately") was not run: there is no GUI session in this headless environment. The integration test covers the same cache invalidation.
