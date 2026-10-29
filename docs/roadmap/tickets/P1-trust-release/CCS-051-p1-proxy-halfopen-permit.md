# CCS-051: [proxy] Release the HalfOpen probe slot when a request is cancelled

**Status:** Done · **Kind:** bug · **Labels:** `bug`, `phase:P1`, `p1-high`, `area:proxy`

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
- [x] circuit_breaker test: acquire the probe, drop the guard, and the next probe is allowed
- [x] forwarder/router test: 2 providers, A forced Open with timeout 0, a forward against a hanging mock is cancelled with tokio::time::timeout, then allow_request(A) is allowed. It fails before the fix.
- [x] proxy_e2e scenario for client-abort during probe
- [x] `cargo test --manifest-path src-tauri/Cargo.toml circuit_breaker`

## Dependencies
p0-proxy-harness-e2e

## Implementation notes
**Summary.** `CircuitBreaker::allow_request` (and `ProviderRouter::allow_provider_request`) now return `AllowResult { allowed, permit: Option<HalfOpenPermit> }`. `HalfOpenPermit` is an RAII guard. It holds the breaker's shared probe slot (`Arc<Mutex<Option<HalfOpenSlot>>>`, the same `Arc` the breaker owns) plus its permit id and a `released` flag. `record_success` / `record_failure` / `record_result` / `release_permit_neutral` now take `Option<&HalfOpenPermit>` and release it. If none of them runs first, for example because the request future was dropped on a client disconnect or Esc, `Drop` releases the slot. Release is idempotent and only clears the slot when the ids match, so a guard from an earlier probe or an earlier HalfOpen round can never free a newer probe.
- The forwarder keeps the guard (`half_open_permit`) for the whole attempt and passes `half_open_permit.as_ref()` everywhere the old `used_half_open_permit: bool` went.
- The slot stores the time it was acquired. A permit held longer than `HALF_OPEN_PERMIT_STALE_AFTER` (30 min, well above the 1200 s upstream timeout limit) counts as free. Reclaiming one logs `CB-007`.
- `transition_to_closed` (and so `reset`) clears the slot. `transition_to_half_open` already did.
- The ticket proposed holding `Arc<CircuitBreaker>` in the guard. Holding the shared slot `Arc` gives the same lifetime guarantee without changing `allow_request` to take `self: &Arc<Self>`.

**Files**
- `src-tauri/src/proxy/circuit_breaker.rs`: guard, slot, stale check, close reset, unit tests.
- `src-tauri/src/proxy/provider_router.rs`: API takes the permit.
- `src-tauri/src/proxy/forwarder.rs`: holds the guard for the attempt, plus the regression test.
- `src-tauri/src/proxy/log_codes.rs`: `CB-007`.
- `src-tauri/src/proxy_harness.rs`: `MockReply::delayed` / `MockReply::Delayed` for hanging upstreams.
- `src-tauri/tests/proxy_e2e.rs`: client-abort scenario.

**Commits:** 9b3c349 (code and tests). The ticket notes are in a follow-up docs commit.

**Tests added**
- circuit_breaker:
  - `test_dropped_half_open_permit_frees_the_probe_slot`: acquire, drop, next probe allowed.
  - `test_recorded_permit_drop_does_not_release_newer_probe`
  - `test_permit_from_previous_half_open_round_does_not_free_new_slot`
  - `test_stale_half_open_permit_is_treated_as_free`
  - `test_transition_to_closed_clears_half_open_slot`
- forwarder: `cancelled_half_open_probe_releases_the_permit`. Two providers; A is Open with timeout 0. `forward_with_retry` against a hanging TCP upstream is cancelled with `tokio::time::timeout(300ms)`. The test asserts the probe reached the upstream and that `allow_provider_request(A)` is then allowed.
- proxy_e2e: `client_abort_during_half_open_probe_releases_the_probe_slot`. The client aborts the half-open probe against a delayed mock. A must then be probed again and close.

**Verification**
- Regression check: with `HalfOpenPermit::drop` temporarily made a no-op, the new circuit_breaker drop test, the forwarder test and the e2e test all FAILED (e2e: `a stayed half-open with a leaked probe slot`). With the fix they pass.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib circuit_breaker`: 9 passed, 0 failed.
- `cargo test ... --test proxy_e2e client_abort`: 1 passed.
- `cargo fmt --check`: clean.
- `cargo clippy -- -D warnings` and `cargo clippy --tests -- -D warnings`: clean.
- `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast` (full suite): 2261 passed, 0 failed.
