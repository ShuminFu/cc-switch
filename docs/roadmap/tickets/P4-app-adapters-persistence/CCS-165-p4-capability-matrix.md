# CCS-165: [settings] Add a capability matrix page

**Status:** Open · **Kind:** feature · **Labels:** `feature`, `phase:P4`, `p3-low`, `area:settings-ui`

> **Ticket key:** `p4-capability-matrix` · **Phase:** P4 · **Priority:** p3-low · **Effort:** S · **Depends on:** `p2-fe-app-registry`, `p4-adapter-conformance`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Users find missing capabilities by trial and error.

## Proposed change
A settings page rendering APPS x caps (MCP, skills, prompts, takeover, usage, session resume) from get_app_registry. It doubles as a support diagnostics view, with a copy-to-clipboard action.

## Acceptance criteria / verification
- [ ] RTL: the matrix renders for all 8 apps and matches the registry fixture
- [ ] `pnpm test:unit`

## Dependencies
p2-fe-app-registry, p4-adapter-conformance
