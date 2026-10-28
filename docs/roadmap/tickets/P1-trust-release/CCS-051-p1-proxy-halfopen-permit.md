# CCS-051: [proxy] Release the HalfOpen probe slot when a request is cancelled

**Status:** Open · **Kind:** bug · **Labels:** `bug`, `phase:P1`, `p1-high`, `area:proxy`

> **Ticket key:** `p1-proxy-halfopen-permit` · **Phase:** P1 · **Priority:** p1-high · **Effort:** M · **Depends on:** `p0-proxy-harness-e2e`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding proxy-halfopen-leak, partially real. In failover mode with 2 or more providers, dropping the request future between allow_provider_request and record_* leaks half_open_requests=1. A client disconnect or Esc can do this. The provider then stays HalfOpen: is_available returns true, but allow_request always says no, until a manual reset or a proxy restart.

## Evidence
- src-tauri/src/proxy/circuit_breaker.rs:313-333 (allow_half_open_probe), 133-150 (is_available), 376
- forwarder.rs:432-490 (permit taken, awaits before record); handlers.rs:194-205; server.rs:156-197

## Proposed change
- Return a `HalfOpenPermit` RAII guard (Arc<CircuitBreaker> plus a released flag) from allow_provider_request. Its Drop calls release_half_open_permit unless record_success or record_failure consumed it.
- Store the acquisition Instant and treat a stale permit as free.
- Make transition_to_closed reset the counter.

## Acceptance criteria / verification
- [ ] circuit_breaker test: acquire the probe, drop the guard, and the next probe is allowed
- [ ] forwarder/router test: 2 providers, A forced Open with timeout 0, a forward against a hanging mock is cancelled with tokio::time::timeout, then allow_request(A) is allowed. It fails before the fix.
- [ ] proxy_e2e scenario for client-abort during probe
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml circuit_breaker`

## Dependencies
p0-proxy-harness-e2e
