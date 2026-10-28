# CCS-141: [proxy] Show live circuit-breaker state with a "probe now" action

**Status:** Open · **Kind:** feature · **Labels:** `feature`, `phase:P3`, `p3-low`, `area:proxy-ui`

> **Ticket key:** `p3-breaker-ui-probe` · **Phase:** P3 · **Priority:** p3-low · **Effort:** M · **Depends on:** `p2-proxy-dead-code-purge`, `p1-proxy-halfopen-permit`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Proxy review idea 2. The breaker state is invisible, and get_circuit_breaker_stats and get_state exist but are marked dead_code.

## Proposed change
Expose the per-provider breaker state (Closed, Open with time remaining, HalfOpen) through an event or a poll. Add "probe now", which sends a minimal request through the half-open path.

## Acceptance criteria / verification
- [ ] E2E: force Open, then probe now against a healthy mock gives Closed
- [ ] RTL: the state badge renders
- [ ] `pnpm test:unit` and `cargo test --manifest-path src-tauri/Cargo.toml circuit_breaker`

## Dependencies
p2-proxy-dead-code-purge, p1-proxy-halfopen-permit
