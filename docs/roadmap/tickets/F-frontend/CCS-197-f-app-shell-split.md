# CCS-197: [shell] Split App.tsx into AppHeader, a ViewTitle map and confirm orchestration

**Status:** Open · **Kind:** refactor · **Labels:** `refactor`, `phase:F`, `p2-medium`, `area:frontend`

> **Ticket key:** `f-app-shell-split` · **Phase:** F · **Priority:** p2-medium · **Effort:** M · **Depends on:** `p2-fe-app-registry`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Proposed change
- Extract AppHeader, a typed view map and the confirm-dialog orchestration.
- Keep the window-state effects in a hook (see f-resize-listener-leak).

## Acceptance criteria / verification
- [ ] App.tsx is 300 lines or fewer
- [ ] Existing App tests pass
- [ ] `pnpm typecheck` and `pnpm test:unit`

## Dependencies
p2-fe-app-registry
