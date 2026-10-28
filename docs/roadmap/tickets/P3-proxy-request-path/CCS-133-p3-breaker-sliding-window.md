# CCS-133: [proxy] Compute the circuit-breaker error rate over a sliding window

**Status:** Open · **Kind:** bug · **Labels:** `bug`, `phase:P3`, `p3-low`, `area:proxy`

> **Ticket key:** `p3-breaker-sliding-window` · **Phase:** P3 · **Priority:** p3-low · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding proxy-breaker-lifetime-rate, partially real. The error rate covers everything since the last HalfOpen->Closed transition. A long clean history hides an intermittent outage, and a borderline long-run rate trips on one more failure. A sustained outage is still caught by the 4-consecutive rule.

## Evidence
- src-tauri/src/proxy/circuit_breaker.rs:203-288, 259-283, 380-387, 121-123

## Proposed change
Use a ring buffer of the last N outcomes, or 60 s time buckets, with min_requests applied to the window. This also removes the u32 overflow.

## Acceptance criteria / verification
- [ ] Test: 1000 successes, then 30 failure/success alternations, opens the breaker
- [ ] Test: old failures that have left the window do not trip it
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml circuit_breaker`

## Dependencies
None.
