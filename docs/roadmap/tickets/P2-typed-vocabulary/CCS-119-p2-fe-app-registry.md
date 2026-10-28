# CCS-119: [apps-ui] Add the frontend app registry and replace VALID_APPS and the capability chains

**Status:** Open · **Kind:** refactor · **Labels:** `refactor`, `phase:P2`, `p2-medium`, `area:frontend`

> **Ticket key:** `p2-fe-app-registry` · **Phase:** P2 · **Priority:** p2-medium · **Effort:** M · **Depends on:** `p2-app-registry-ipc`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
The frontend keeps its own app list and capability if-chains, and falls back to the label "OpenCode" incorrectly.

## Evidence
- src/App.tsx:124 VALID_APPS, 224-302; useProxyStatus.ts:128; ProxyToggle.tsx:35

## Proposed change
Add src/entities/app/registry.ts with useAppCaps() and appLabel(), fed by get_app_registry, with icons and colours in TS. Replace the sites above.

## Acceptance criteria / verification
- [ ] VALID_APPS is removed
- [ ] Vitest: appLabel for every app, and the fallback is not "OpenCode"
- [ ] `pnpm typecheck` and `pnpm test:unit`

## Dependencies
p2-app-registry-ipc
