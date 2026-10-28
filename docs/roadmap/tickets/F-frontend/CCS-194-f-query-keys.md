# CCS-194: [query] Add query-key factories, invalidateProviderGraph and per-query staleTime

**Status:** Open · **Kind:** refactor · **Labels:** `refactor`, `phase:F`, `p2-medium`, `area:frontend`

> **Ticket key:** `f-query-keys` · **Phase:** F · **Priority:** p2-medium · **Effort:** M · **Depends on:** `p2-fe-app-registry`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Evidence
- 23 inline ["providers", appId] keys; duplicated invalidation blocks in src/lib/query/failover.ts and mutations.ts; queryClient.ts:7-8 global staleTime 0 + refetchOnWindowFocus

## Proposed change
- Add entities/<x>/keys.ts.
- Add invalidateProviderGraph(appId), covering providers, live ids, omo, openclaw, hermes and usage-script keys.
- Set staleTime per query.

## Acceptance criteria / verification
- [ ] `grep -rn '\["providers"' src` matches only keys.ts
- [ ] `pnpm typecheck` and `pnpm test:unit`

## Dependencies
p2-fe-app-registry
