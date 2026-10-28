# CCS-117: [ipc] Generate src/shared/bindings.ts with a CI dirty check and typed MSW handlers

**Status:** Open · **Kind:** refactor · **Labels:** `refactor`, `phase:P2`, `p1-high`, `area:ipc`

> **Ticket key:** `p2-bindings-generate` · **Phase:** P2 · **Priority:** p1-high · **Effort:** L · **Depends on:** `p2-bindings-spike`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Frontend and backend types drift, and MSW handlers are untyped.

## Proposed change
- Derive Type on Provider, AppType, ProxyStatus, IpcError and the usage DTOs.
- Generate bindings.ts in a build step, and fail CI if the generated file differs from the committed one.
- Type tests/msw/handlers.ts against it, and add a test that every command has a handler or is allowlisted.

## Acceptance criteria / verification
- [ ] CI fails when a Rust DTO changes without regenerating
- [ ] `pnpm typecheck` and `pnpm test:unit`

## Dependencies
p2-bindings-spike
