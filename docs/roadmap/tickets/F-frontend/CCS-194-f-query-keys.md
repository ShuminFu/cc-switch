# CCS-194: [query] Add query-key factories, invalidateProviderGraph and per-query staleTime

**Status:** Done · **Kind:** refactor · **Labels:** `refactor`, `phase:F`, `p2-medium`, `area:frontend`

> **Ticket key:** `f-query-keys` · **Phase:** F · **Priority:** p2-medium · **Effort:** M · **Depends on:** `p2-fe-app-registry`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Evidence
- 23 inline ["providers", appId] keys; duplicated invalidation blocks in src/lib/query/failover.ts and mutations.ts; queryClient.ts:7-8 global staleTime 0 + refetchOnWindowFocus

## Proposed change
- Add entities/<x>/keys.ts.
- Add invalidateProviderGraph(appId), covering providers, live ids, omo, openclaw, hermes and usage-script keys.
- Set staleTime per query.

## Acceptance criteria / verification
- [x] `grep -rn '\["providers"' src` matches only keys.ts
- [x] `pnpm typecheck` and `pnpm test:unit`

## Dependencies
p2-fe-app-registry

## Implementation notes

**Summary.** Provider query keys now come from one factory and provider
changes cascade through one invalidation helper.

- `src/entities/provider/keys.ts`: `providerKeys.all`, `providerKeys.list(appId)`,
  `providerKeys.opencodeLiveIds` and `providerKeys.claudeDesktopStatus`. It is
  the only file in `src` that spells a `["providers"` key.
- `src/entities/provider/invalidate.ts`: `invalidateProviderGraph(queryClient, appId, { providerIds? })`
  invalidates `providerKeys.list(appId)`, the usage-script key of each given
  provider id, and each app's dependent caches from a per-app table (no app-literal
  comparisons): OpenCode live ids + `omo` + `omo-slim`; OpenClaw live ids /
  default model / health; Hermes (`invalidateHermesProviderCaches`); Claude
  Desktop status + `proxyStatus`. The add/update/delete/switch mutations in
  `mutations.ts` (which repeated this block four times), the deep-link
  provider import and ProviderList's import-from-live now call it. A side
  effect is that add/delete/update now also refresh the OpenClaw live ids and
  default model and the OpenCode live ids, which they previously skipped.
- `src/lib/query/failover.ts`: the queue / available / providers invalidation
  that appeared three times is now `invalidateFailoverQueueGraph()`.
- Remaining inline `["providers", ...]` sites (App.tsx, DeepLinkImportDialog,
  useDragSort, useProviderActions, profiles.ts, proxy.ts, omo.ts, queries.ts)
  now use `providerKeys`.
- staleTime: `src/lib/query/queryClient.ts` exports `STALE_TIME` tiers
  (`live` 0, `short` 10 s, `long` 5 min). `useProvidersQuery` sets
  `STALE_TIME.short`: it is DB-backed and every write path invalidates or
  refetches it, so app switches and window focus no longer refetch it each
  time. The OpenCode live-id queries are explicitly `STALE_TIME.live`. The
  global default stays `staleTime: 0` + `refetchOnWindowFocus` as the fallback
  for queries that do not set their own. Other domains (mcp/skill/proxy/usage
  keys from the plan's §entities list) still use inline keys; moving them is
  follow-up work and not part of this ticket's acceptance criteria.
- `eslint-suppressions.json`: pruned the counts for the app-literal comparisons
  that this change removed (mutations.ts 14 -> 2, ProviderList.tsx 21 -> 20).

**Files.** `src/entities/provider/keys.ts` (new), `src/entities/provider/invalidate.ts`
(new), `src/lib/query/{mutations,failover,queries,queryClient,omo,profiles,proxy}.ts`,
`src/App.tsx`, `src/components/DeepLinkImportDialog.tsx`,
`src/components/providers/ProviderList.tsx`,
`src/components/providers/forms/ProviderForm.tsx`, `src/hooks/useDragSort.ts`,
`src/hooks/useProviderActions.ts`, `eslint-suppressions.json`,
`tests/entities/providerKeys.test.ts` (new), `tests/hooks/useUpdateProviderMutation.test.tsx`.

**Commits.** `6501862` refactor(query): add provider query-key factories and invalidateProviderGraph (CCS-194).

**Verification.**
- `grep -rn '\["providers"' src` matches only `src/entities/provider/keys.ts`.
  `tests/entities/providerKeys.test.ts` enforces this. It fails on the
  pre-change tree, which had 23 inline sites. It also covers the per-app
  `invalidateProviderGraph` key sets, de-duplicated usage-script ids, and the
  fact that unrelated apps' lists are left alone.
- `pnpm typecheck`: passes.
- `pnpm lint`: passes (after `pnpm lint:prune`, which changed only the two counts above).
- `pnpm format:check`: all files pass.
- `pnpm test:unit`: 111 files / 1192 tests pass. Before the change it was
  110 / 1183; the difference is the new providerKeys suite.
- No files under `src-tauri/` were touched, so the Rust suite is unaffected.
