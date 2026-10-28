# CCS-207: [providers] Show a switching state and disable the other Switch buttons while a switch is pending

**Status:** Open · **Kind:** feature · **Labels:** `feature`, `phase:F`, `p3-low`, `area:frontend`

> **Ticket key:** `f-switching-state` · **Phase:** F · **Priority:** p3-low · **Effort:** S · **Depends on:** `f-mutation-appid-variables`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Frontend idea 2. There is no feedback during a switch, and double switches are possible.

## Proposed change
Show a per-card pending indicator from the mutation variables, and disable Switch on the other cards while it is pending.

## Acceptance criteria / verification
- [ ] RTL with a deferred switch: the other buttons are disabled until it resolves
- [ ] `pnpm test:unit`

## Dependencies
f-mutation-appid-variables
