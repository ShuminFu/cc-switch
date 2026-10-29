# CCS-106: [auth-ui] Prevent leaked and overlapping managed-auth polling timers

**Status:** Done · **Kind:** bug · **Labels:** `bug`, `phase:P1`, `p2-medium`, `area:frontend`

> **Ticket key:** `p1-fe-managed-auth-timers` · **Phase:** P1 · **Priority:** p2-medium · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding frontend-managedauth-timer-leak.
- Timers are armed in onSuccess, which runs even after unmount or cancel.
- A double-click leaks the first interval forever. After its code expires it calls stopPolling every ~8 s and kills the valid session.
- Polls can overlap.

## Evidence
- src/components/providers/forms/hooks/useManagedAuth.ts:51-123, 180-186; CopilotAuthSection.tsx:275-285,370; CodexOAuthSection.tsx:229-238,317

## Proposed change
- Keep an attempt-token ref and a mounted ref, and bail out after each await.
- Call stopPolling in onSuccess before arming timers, and give each attempt its own handles.
- Chain setTimeout instead of setInterval.
- Disable the login and Retry buttons while isPending.

## Acceptance criteria / verification
- [x] Vitest with fake timers: (a) a double start leaves vi.getTimerCount()==0 after expiry; (b) unmount before resolve means no polls; (c) at most one poll in flight
- [x] `pnpm test:unit`

## Dependencies
None.

## Implementation notes
- **Summary:** `useManagedAuth` now tracks an attempt token (`attemptRef`) and a mounted ref. `stopPolling` bumps the token and clears this attempt's handles. `startAuth` passes the current token as the mutation variable, and `onSuccess`/`onError` bail out if the token is stale or the hook has unmounted. `onSuccess` calls `stopPolling` before it arms timers and checks the token again after each await (clipboard copy, opening the browser, each poll, and the post-success refetch). Polling now chains `setTimeout`: the next poll is scheduled only after the current one settles, so polls can't overlap. A separate expiry timeout ends the attempt. The hook also returns `isStartingAuth` (`startLoginMutation.isPending`). The Copilot and Codex OAuth sections use it to disable the login and Retry buttons.
- **Files:** `src/components/providers/forms/hooks/useManagedAuth.ts`, `src/components/providers/forms/CopilotAuthSection.tsx`, `src/components/providers/forms/CodexOAuthSection.tsx`, `tests/hooks/useManagedAuth.test.tsx` (new)
- **Commits:** `96e8956` (code + tests), plus the follow-up commit recording these notes.
- **Verification:**
  - `pnpm vitest run tests/hooks/useManagedAuth.test.tsx`: 4/4 pass with fake timers. The tests cover (a) a double start: the first code's expiry does not kill the second session, and `vi.getTimerCount()===0` after expiry; (b) unmount before the start request resolves, and also cancel before it resolves: no polls and no timers; (c) at most one poll in flight. Against the previous hook, all 4 fail.
  - `pnpm typecheck`: clean. `pnpm format:check`: clean. `pnpm test:unit`: 96 files / 667 tests pass.
