# CCS-208: [shell] Add a Cmd/Ctrl-K command palette to search and switch providers across apps

**Status:** Open · **Kind:** feature · **Labels:** `feature`, `phase:F`, `p3-low`, `area:frontend`

> **Ticket key:** `f-command-palette` · **Phase:** F · **Priority:** p3-low · **Effort:** S · **Depends on:** `f-query-keys`, `f-switching-state`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
This combines the plan backlog item with frontend idea 6. cmdk is already a dependency, and shortcuts are scattered across 8 apps.

## Proposed change
A palette listing providers for all visible apps (from the registry and query keys) plus common actions (toggle proxy, open settings).

## Acceptance criteria / verification
- [ ] RTL: Ctrl-K opens the palette; typing filters; Enter switches the provider
- [ ] `pnpm test:unit`

## Dependencies
f-query-keys, f-switching-state
