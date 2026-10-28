# CCS-139: [proxy] Add a per-request trace inspector

**Status:** Open · **Kind:** feature · **Labels:** `feature`, `phase:P3`, `p2-medium`, `area:proxy-ui`

> **Ticket key:** `p3-trace-inspector` · **Phase:** P3 · **Priority:** p2-medium · **Effort:** M · **Depends on:** `p3-route-plan`, `p3-attempt-recorder`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Users cannot see why a request went to provider B. Merges plan feature with proxy review idea 5 (request inspector).

## Proposed change
- Keep a ring buffer of the last N requests: the RoutePlan, each attempt with its status, the recovery strategies applied, TTFB, the usage source and the redacted headers.
- Add a detail panel in the request log.

## Acceptance criteria / verification
- [ ] E2E: a failover request shows 2 attempts with reasons
- [ ] The buffer is bounded (test)
- [ ] `pnpm test:unit` and `cargo test --manifest-path src-tauri/Cargo.toml trace`

## Dependencies
p3-route-plan, p3-attempt-recorder
