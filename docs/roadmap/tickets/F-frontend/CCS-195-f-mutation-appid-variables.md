# CCS-195: [query] Bind provider mutation invalidation to the variables' appId, not activeApp

**Status:** Done · **Kind:** bug · **Labels:** `bug`, `phase:F`, `p3-low`, `area:frontend`

> **Ticket key:** `f-mutation-appid-variables` · **Phase:** F · **Priority:** p3-low · **Effort:** S · **Depends on:** `f-query-keys`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding frontend-mutations-activeapp-binding. TanStack v5 passes updated options to a pending mutation, so after a tab change onSuccess invalidates the new app and skips the original app's specific caches.

## Evidence
- src/lib/query/mutations.ts:15,143,197,263 (272-305); useProviderActions.ts:42-45; App.tsx:311

## Proposed change
Use mutate({ providerId, appId }) and read variables.appId in both mutationFn and onSuccess.

## Acceptance criteria / verification
- [x] renderHook test: mutate on opencode, rerender with claude, resolve; invalidateQueries gets ["providers","opencode"] and ["opencodeLiveProviderIds"]; the same for add, update and delete
- [x] `pnpm test:unit`

## Dependencies
f-query-keys

## Implementation notes

**Summary.** `useAddProviderMutation`, `useUpdateProviderMutation`,
`useDeleteProviderMutation` and `useSwitchProviderMutation` no longer take an
`appId` argument. The target app now travels in the mutation variables
(`{ appId, provider }`, `{ appId, provider, originalId }`,
`{ appId, providerId }`) and both `mutationFn` and `onSuccess` read it from
there, so a tab change while a mutation is pending can no longer redirect
`invalidateProviderGraph` to the newly active app. `useProviderActions` passes
`appId: activeApp` at call time. The variable types are exported from
`src/lib/query/mutations.ts` (`AddProviderVariables`,
`UpdateProviderVariables`, `ProviderIdVariables`, `AddProviderInput`).

**Files.**
- `src/lib/query/mutations.ts`
- `src/hooks/useProviderActions.ts`
- `tests/hooks/providerMutationsAppBinding.test.tsx` (new regression test)
- `tests/hooks/useAddProviderMutation.test.tsx`, `tests/hooks/useUpdateProviderMutation.test.tsx`, `tests/hooks/useProviderActions.test.tsx` (updated to the new variables shape)

**Commits.** `907de33` (code + tests); the ticket-notes commit follows.

**Verification.**
- New `providerMutationsAppBinding.test.tsx`: renderHook `useProviderActions("opencode")`,
  start add / update / delete / switch with a pending API promise, rerender with
  `"claude"`, resolve. It asserts `invalidateQueries` got `["providers","opencode"]` and
  `["opencodeLiveProviderIds"]` and never `["providers","claude"]`. Against the
  pre-change `mutations.ts` / `useProviderActions.ts` all 4 cases fail. With the change, all 4 pass.
- `pnpm typecheck`: clean.
- `pnpm test:unit`: 112 files / 1196 tests passed.
- `pnpm format:check`: clean. `eslint` on the touched files: clean.
