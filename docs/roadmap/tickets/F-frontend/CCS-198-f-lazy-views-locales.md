# CCS-198: [perf] Lazy-load heavy views, per-app presets and locales

**Status:** Open · **Kind:** refactor · **Labels:** `refactor`, `phase:F`, `p3-low`, `area:frontend-perf`

> **Ticket key:** `f-lazy-views-locales` · **Phase:** F · **Priority:** p3-low · **Effort:** M · **Depends on:** `f-app-shell-split`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Proposed change
- React.lazy for SessionManagerPage, settings sync, UsageScriptModal, the skills/MCP/proxy panels and the CodeMirror editors.
- Load per-app presets and locale JSON on demand.

## Acceptance criteria / verification
- [ ] The initial JS bundle is at least 30% smaller than v3.17 (`pnpm build:renderer` size report in the PR)
- [ ] `pnpm test:unit`

## Dependencies
f-app-shell-split
