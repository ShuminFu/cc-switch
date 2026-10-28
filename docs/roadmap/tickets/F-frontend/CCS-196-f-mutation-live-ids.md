# CCS-196: [query] Invalidate the OpenCode and OpenClaw live-provider-id caches on add, update and delete

**Status:** Open · **Kind:** bug · **Labels:** `bug`, `phase:F`, `p3-low`, `area:frontend`

> **Ticket key:** `f-mutation-live-ids` · **Phase:** F · **Priority:** p3-low · **Effort:** S · **Depends on:** `f-query-keys`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding frontend-mutations-missing-liveid-invalidation, partially real. After adding an OpenCode or OpenClaw provider (addToLive defaults to true), the mounted list shows it as "not in config" until the window refocuses.

## Evidence
- src/lib/query/mutations.ts:85-130, 158-175, 205-231 vs switch :280-302; ProviderList.tsx:100-109

## Proposed change
Call invalidateProviderGraph(appId) in all four mutations and in the App.tsx remove-from-live path.

## Acceptance criteria / verification
- [ ] Vitest: for add, update and delete on opencode and openclaw, the spy sees the live-id keys
- [ ] Manual: the card shows as in-config immediately
- [ ] `pnpm test:unit`

## Dependencies
f-query-keys
