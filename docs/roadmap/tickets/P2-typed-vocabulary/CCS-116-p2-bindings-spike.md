# CCS-116: [ipc] Time-boxed tauri-specta spike on the usage domain, with a written decision

**Status:** Open · **Kind:** chore · **Labels:** `chore`, `phase:P2`, `p1-high`, `area:ipc`

> **Ticket key:** `p2-bindings-spike` · **Phase:** P2 · **Priority:** p1-high · **Effort:** S · **Depends on:** `p2-ipc-error-type`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
About 2.9k lines of hand-written TS mirror backend types and drift. Whether tauri-specta fits Tauri 2 and the existing serde shapes is unknown.

## Proposed change
- Spend one week generating bindings for the usage domain with tauri-specta.
- Write docs/dev/adr/bindings.md recording the choice. The fallback is ts-rs plus a generated command-name map.
- settingsConfig stays Value at the boundary.

## Acceptance criteria / verification
- [ ] The ADR is merged with the decision and the observed blockers
- [ ] The usage domain compiles against the generated types: `pnpm typecheck`

## Dependencies
p2-ipc-error-type
