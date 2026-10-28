# CCS-106: [auth-ui] Prevent leaked and overlapping managed-auth polling timers

**Status:** Open · **Kind:** bug · **Labels:** `bug`, `phase:P1`, `p2-medium`, `area:frontend`

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
- [ ] Vitest with fake timers: (a) a double start leaves vi.getTimerCount()==0 after expiry; (b) unmount before resolve means no polls; (c) at most one poll in flight
- [ ] `pnpm test:unit`

## Dependencies
None.
