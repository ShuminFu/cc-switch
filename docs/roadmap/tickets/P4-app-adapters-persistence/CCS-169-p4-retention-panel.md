# CCS-169: [settings] Add a retention and DB-size panel

**Status:** Open · **Kind:** feature · **Labels:** `feature`, `phase:P4`, `p3-low`, `area:settings-ui`

> **Ticket key:** `p4-retention-panel` · **Phase:** P4 · **Priority:** p3-low · **Effort:** S · **Depends on:** `p4-rollup-retention`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Detail retention is hard-coded to 30 days, and rollups are never pruned.

## Proposed change
A settings panel showing the DB size, row counts per table and the RetentionPolicy controls, with a "run maintenance now" button.

## Acceptance criteria / verification
- [ ] RTL test; a command test that maintenance respects the new policy
- [ ] `pnpm test:unit`

## Dependencies
p4-rollup-retention
