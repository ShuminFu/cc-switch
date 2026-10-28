# CCS-040: [proxy] Use one shared default for the circuit-breaker failure_threshold

**Status:** Open · **Kind:** bug · **Labels:** `bug`, `phase:P0`, `p3-low`, `area:proxy`

> **Ticket key:** `p0-failure-threshold-const` · **Phase:** P0 · **Priority:** p3-low · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding proxy-failure-threshold-mismatch. record_result falls back to 5 when the config read errors, while the breaker, schema and DAO defaults are 4. The location is provider_router.rs, not forwarder.rs.

## Evidence
- src-tauri/src/proxy/provider_router.rs:135-138 `Err(_) => 5`; circuit_breaker.rs:66; schema.rs DEFAULT 4; dao/proxy.rs:263

## Proposed change
- Add `pub const DEFAULT_CIRCUIT_FAILURE_THRESHOLD: u32 = 4` and use it at all sites.
- Prefer breaker.config.failure_threshold in record_result.

## Acceptance criteria / verification
- [ ] `grep -rn '=> 5' src-tauri/src/proxy` finds nothing
- [ ] A provider_router test with a failing config read marks the provider unhealthy after 4 failures
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml provider_router`

## Dependencies
None.
