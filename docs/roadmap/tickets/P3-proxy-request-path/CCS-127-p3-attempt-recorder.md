# CCS-127: [proxy] Add AttemptRecorder to own status, breaker, health and try_switch for each attempt

**Status:** Open · **Kind:** refactor · **Labels:** `refactor`, `phase:P3`, `p2-medium`, `area:proxy`

> **Ticket key:** `p3-attempt-recorder` · **Phase:** P3 · **Priority:** p2-medium · **Effort:** M · **Depends on:** `p3-route-plan`, `p1-proxy-halfopen-permit`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
The success-rate and status update block is copied about 9 times, and try_switch 4 times, in forward_with_retry_inner.

## Evidence
- forwarder.rs:510-528, 616, 762, 926

## Proposed change
- Add pipeline/failover.rs AttemptRecorder, which owns the HalfOpenPermit guard (from P1), records the outcome once, and emits one switch per real failover.

## Acceptance criteria / verification
- [ ] `grep -c success_rate src-tauri/src/proxy/forwarder.rs` is 1 or fewer
- [ ] The P1 half-open and proxy_e2e tests are green

## Dependencies
p3-route-plan, p1-proxy-halfopen-permit
