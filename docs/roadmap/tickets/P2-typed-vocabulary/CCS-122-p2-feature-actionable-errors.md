# CCS-122: [errors] Add actions to error toasts based on IpcError codes

**Status:** Open · **Kind:** feature · **Labels:** `feature`, `phase:P2`, `p3-low`, `area:frontend`

> **Ticket key:** `p2-feature-actionable-errors` · **Phase:** P2 · **Priority:** p3-low · **Effort:** M · **Depends on:** `p2-ipc-error-frontend`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Error toasts are dead ends. A missing tool, an open circuit or a takeover conflict gives the user no way to act on it.

## Proposed change
Map codes to actions: TOOL_NOT_INSTALLED offers Install, CIRCUIT_OPEN offers Reset, TAKEOVER_ACTIVE offers Stop takeover, and DB_VERSION_TOO_NEW offers Restore backup.

## Acceptance criteria / verification
- [ ] Vitest: each mapped code renders its action, and the action calls the right command
- [ ] `pnpm test:unit`

## Dependencies
p2-ipc-error-frontend
