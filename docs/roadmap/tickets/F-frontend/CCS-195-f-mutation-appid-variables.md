# CCS-195: [query] Bind provider mutation invalidation to the variables' appId, not activeApp

**Status:** Open · **Kind:** bug · **Labels:** `bug`, `phase:F`, `p3-low`, `area:frontend`

> **Ticket key:** `f-mutation-appid-variables` · **Phase:** F · **Priority:** p3-low · **Effort:** S · **Depends on:** `f-query-keys`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding frontend-mutations-activeapp-binding. TanStack v5 passes updated options to a pending mutation, so after a tab change onSuccess invalidates the new app and skips the original app's specific caches.

## Evidence
- src/lib/query/mutations.ts:15,143,197,263 (272-305); useProviderActions.ts:42-45; App.tsx:311

## Proposed change
Use mutate({ providerId, appId }) and read variables.appId in both mutationFn and onSuccess.

## Acceptance criteria / verification
- [ ] renderHook test: mutate on opencode, rerender with claude, resolve; invalidateQueries gets ["providers","opencode"] and ["opencodeLiveProviderIds"]; the same for add, update and delete
- [ ] `pnpm test:unit`

## Dependencies
f-query-keys
