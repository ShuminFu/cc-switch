# CCS-229: [frontend] Feed useAppRegistry() from the backend get_app_registry command

**Status:** Open · **Kind:** refactor · **Labels:** `refactor`, `phase:P2`, `p2-medium`, `area:frontend`

> **Ticket key:** `p2-fe-app-registry-from-backend` · **Phase:** P2 · **Priority:** p2-medium · **Effort:** M
> Plan: [next-phase-plan.md](../../next-phase-plan.md)
> Found during the Partial-ticket triage at d962272 (CCS-119). Source tickets: [CCS-119](../P2-typed-vocabulary/CCS-119-p2-fe-app-registry.md).

## Problem
CCS-119 stayed Partial because CCS-113 had not landed; it is now Done: `src-tauri/src/commands/apps.rs:6` exposes `get_app_registry`, `src/lib/api/apps.ts` has `appsApi.getRegistry()`, `CapabilityMatrix.tsx:76` queries it, and `tests/fixtures/app-registry.json` plus the MSW handler (`tests/msw/handlers.ts:60`) exist. But `useAppRegistry()` in `src/entities/app/registry.ts:174-176` still returns the static `APP_REGISTRY`, and comments at lines 8 and 172 still say the IPC "has not landed". Drift between the TS table and the backend is only checked for id order (`tests/components/CapabilityMatrix.test.tsx:64`).

## Proposed change
- In `useAppRegistry()`, `useQuery({ queryKey: ['appRegistry'], queryFn: appsApi.getRegistry, staleTime: Infinity })` (same key as CapabilityMatrix).
- Merge by id into the static table: take order, label (`displayName`), aliases, `caps.mcp`, `caps.skills`, `caps.proxyTakeover`, and `additive = syncMode === 'additive'` from the backend; keep FE-only caps (`sessions`, `healthCheck`, `setDefault`, `defaultIsSwitch`) and `featureHost` from TS.
- Fall back to static `APP_REGISTRY` while loading or on error, so `appLabel`, `isAppId`, `appCaps` and first paint keep working. Callers of `useAppCaps`/`useAppRegistry` do not change.
- Update the stale comments; set CCS-119 to Done.

## Acceptance criteria / verification
- [ ] New `tests/entities/appRegistry.test.tsx` cases compare static `APP_REGISTRY` with `tests/fixtures/app-registry.json` per app (order, label, aliases, mcp, skills, proxyTakeover, additive vs syncMode).
- [ ] A hook test with a QueryClient + MSW override (e.g. `mcp:false` for codex) shows static caps before resolve and the backend value after.
- [ ] `pnpm typecheck`, `pnpm test:unit`, `pnpm lint` and `pnpm format:check` pass.

## Dependencies
None.
