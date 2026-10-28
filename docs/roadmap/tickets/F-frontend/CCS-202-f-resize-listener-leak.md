# CCS-202: [shell] Unregister the onResized listener when App unmounts before it resolves

**Status:** Open · **Kind:** bug · **Labels:** `bug`, `phase:F`, `p3-low`, `area:frontend`

> **Ticket key:** `f-resize-listener-leak` · **Phase:** F · **Priority:** p3-low · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding frontend-app-resize-listener-leak. If App unmounts before onResized resolves, which always happens under StrictMode in dev, the listener is never removed.

## Evidence
- src/App.tsx:448-476; pattern src/hooks/useTauriEvent.ts:16-35

## Proposed change
After awaiting onResized, call unlisten and return if the component is no longer active.

## Acceptance criteria / verification
- [ ] Vitest: unmount before resolve, then resolve; unlisten is called once
- [ ] `pnpm test:unit`

## Dependencies
None (can land before f-app-shell-split).
