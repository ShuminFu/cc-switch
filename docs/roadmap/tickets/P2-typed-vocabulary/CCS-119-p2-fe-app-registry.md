# CCS-119: [apps-ui] Add the frontend app registry and replace VALID_APPS and the capability chains

**Status:** Partial · **Kind:** refactor · **Labels:** `refactor`, `phase:P2`, `p2-medium`, `area:frontend`

> **Ticket key:** `p2-fe-app-registry` · **Phase:** P2 · **Priority:** p2-medium · **Effort:** M · **Depends on:** `p2-app-registry-ipc`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
The frontend keeps its own app list and capability if-chains, and falls back to the label "OpenCode" incorrectly.

## Evidence
- src/App.tsx:124 VALID_APPS, 224-302; useProxyStatus.ts:128; ProxyToggle.tsx:35

## Proposed change
Add src/entities/app/registry.ts with useAppCaps() and appLabel(), fed by get_app_registry, with icons and colours in TS. Replace the sites above.

## Acceptance criteria / verification
- [x] VALID_APPS is removed
- [x] Vitest: appLabel for every app, and the fallback is not "OpenCode"
- [x] `pnpm typecheck` and `pnpm test:unit`

## Dependencies
p2-app-registry-ipc

## Implementation notes

**Summary.** Added `src/entities/app/registry.ts`, the frontend's single source for app ids, labels and capabilities (`sessions`, `skills`, `mcp`, `additive`). It exports `APP_REGISTRY`, `ALL_APP_IDS`, `isAppId()`, `appLabel()`, `appCaps()`, `appsWithCap()`, `useAppRegistry()` and `useAppCaps()`. Icons and colours stay in `src/config/appConfig.tsx` (they need JSX). `APP_IDS`, `SKILLS_APP_IDS` and `MCP_APP_IDS` there are now derived from the registry.

- `src/App.tsx`: removed `VALID_APPS`. `getInitialApp()` validates the stored value with `isAppId()`. The sessions fallback effect, `hasSessionSupport` and `hasSkillsSupport` (the old lines 224-302) read `useAppCaps(sharedFeatureApp)` instead of literal if-chains.
- `src/hooks/useProxyStatus.ts` and `src/components/proxy/ProxyToggle.tsx`: the takeover label now uses `appLabel()`. Before, Claude Desktop, OpenClaw and Hermes all showed up as "OpenCode". An unknown id now falls back to the raw id, not "OpenCode".

**Files:** `src/entities/app/registry.ts` (new), `src/App.tsx`, `src/hooks/useProxyStatus.ts`, `src/components/proxy/ProxyToggle.tsx`, `src/config/appConfig.tsx`, `tests/entities/appRegistry.test.tsx` (new), `tests/hooks/useProxyStatus.test.tsx`.

**Commits:** `6dd19ed` (code, tests and these notes); this SHA was filled in by a follow-up docs commit.

**Verification.**
- `pnpm typecheck`: clean.
- `npx vitest run tests/entities tests/hooks/useProxyStatus.test.tsx`: 20/20 pass. This covers `appLabel` for every app, a check that the fallback is not "OpenCode", label parity with `APP_ICON_MAP`, parity with the old capability chains, `isAppId`, and `useAppCaps`.
- Regression check: with `useProxyStatus.ts` reverted to HEAD, the new takeover-toast tests for hermes, openclaw and claude-desktop fail (3 failed). With the change they pass.
- `pnpm test:unit`: 110 files, 1174 tests passed.
- `pnpm format:check`: clean. `npx eslint` on the touched sources: clean.

**Remaining (why Partial).** The dependency `p2-app-registry-ipc` (backend `get_app_registry`) has not landed. For now the registry is static TS data. `useAppRegistry()` is the one place to switch to the backend query once that command exists; `useAppCaps`, `appLabel` and `isAppId` callers will not change. The plan's ESLint ratchet on app-id literal comparisons is not in this ticket's acceptance list and was not done.

