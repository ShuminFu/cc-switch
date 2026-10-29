# CCS-202: [shell] Unregister the onResized listener when App unmounts before it resolves

**Status:** Done · **Kind:** bug · **Labels:** `bug`, `phase:F`, `p3-low`, `area:frontend`

> **Ticket key:** `f-resize-listener-leak` · **Phase:** F · **Priority:** p3-low · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding frontend-app-resize-listener-leak. If App unmounts before onResized resolves, which always happens under StrictMode in dev, the listener is never removed.

## Evidence
- src/App.tsx:448-476; pattern src/hooks/useTauriEvent.ts:16-35

## Proposed change
After awaiting onResized, call unlisten and return if the component is no longer active.

## Acceptance criteria / verification
- [x] Vitest: unmount before resolve, then resolve; unlisten is called once
- [x] `pnpm test:unit`

## Dependencies
None (can land before f-app-shell-split).

## Implementation notes
- **Summary:** The window maximized-state sync effect moved out of `App` into a new hook, `useWindowMaximizedSync`, which `App` calls with `setIsWindowMaximized`. Before, `App` assigned the result of `await onResized()` without checking whether the component was still mounted, so a listener registered after unmount was never removed. The hook now uses the same pattern as `useTauriEvent`: if the component already unmounted when `onResized` resolves, it calls the returned unlisten right away. It also returns before registering anything if unmount happens during the first `isMaximized()` read.
- **Files:** `src/hooks/useWindowMaximizedSync.ts` (new), `src/App.tsx`, `tests/hooks/useWindowMaximizedSync.test.tsx` (new).
- **Commit:** 49e1093 (code and tests).
- **Verification:**
  - `pnpm vitest run tests/hooks/useWindowMaximizedSync.test.tsx`: 3/3 pass. With the unmount guards temporarily removed, the two race tests fail ("unlistens once when unmounted before onResized resolves" and "does not register a resize listener when unmounted before the initial sync resolves"), so they catch the regression.
  - `pnpm typecheck`: clean.
  - `pnpm test:unit`: 105 files and 1142 tests pass.
  - `pnpm format:check`: clean.
