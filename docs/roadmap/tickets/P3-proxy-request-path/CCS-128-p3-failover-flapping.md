# CCS-128: [proxy] Count failovers only when a request actually failed over, and report recovery separately

**Status:** Open · **Kind:** bug · **Labels:** `bug`, `phase:P3`, `p3-low`, `area:proxy`

> **Ticket key:** `p3-failover-flapping` · **Phase:** P3 · **Priority:** p3-low · **Effort:** S · **Depends on:** `p3-attempt-recorder`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding proxy-failover-flapping, partially real.
- One transient P1 failure causes two hot switches (P1 to P2, then back to P1 on the next request) and adds 2 to failover_count; the recovery is reported as a failover.
- Concurrent requests that start before the async switch lands each increment the counter.

## Evidence
- src-tauri/src/proxy/forwarder.rs:510-528 (also 616, 762, 926); handler_context.rs:110,226; provider_router.rs:51-78; failover_switch.rs:41-133

## Proposed change
- In AttemptRecorder, set a flag when an earlier provider in this loop failed, and increment only then.
- Emit a `recovery` event or counter for a return to a higher-priority provider, optionally with a minimum dwell time.
- Count only when try_switch returns Ok(true).

## Acceptance criteria / verification
- [ ] Test: queue [P1,P2], P1 returns 5xx once and P2 returns 200, so count==1; the next request where P1 returns 200 keeps count 1 and recovery 1
- [ ] Test: N parallel requests increment the count once
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml proxy::forwarder`

## Dependencies
p3-attempt-recorder
