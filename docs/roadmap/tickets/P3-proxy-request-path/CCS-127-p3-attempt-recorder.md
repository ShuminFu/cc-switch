# CCS-127: [proxy] Add AttemptRecorder to own status, breaker, health and try_switch for each attempt

**Status:** Done · **Kind:** refactor · **Labels:** `refactor`, `phase:P3`, `p2-medium`, `area:proxy`

> **Ticket key:** `p3-attempt-recorder` · **Phase:** P3 · **Priority:** p2-medium · **Effort:** M · **Depends on:** `p3-route-plan`, `p1-proxy-halfopen-permit`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
The success-rate and status update block is copied about 9 times, and try_switch 4 times, in forward_with_retry_inner.

## Evidence
- forwarder.rs:510-528, 616, 762, 926

## Proposed change
- Add pipeline/failover.rs AttemptRecorder, which owns the HalfOpenPermit guard (from P1), records the outcome once, and emits one switch per real failover.

## Acceptance criteria / verification
- [x] `grep -c success_rate src-tauri/src/proxy/forwarder.rs` is 1 or fewer
- [x] The P1 half-open and proxy_e2e tests are green

## Dependencies
p3-route-plan, p1-proxy-halfopen-permit

## Implementation notes

**Summary.** Added `src-tauri/src/proxy/pipeline/failover.rs` with `AttemptRecorder` (one per client request) and `Attempt` (one per provider attempt). `AttemptRecorder::begin` acquires the breaker permit (skipped in the single-provider bypass), refreshes `current_provider`/`current_provider_id`, and returns an `Attempt` that owns the `HalfOpenPermit` guard. The attempt records its outcome exactly once through consuming methods:
- `succeed()`: breaker/health success (sync for half-open probes, spawned otherwise), `current_providers`, success stats, and one `failover_count` increment plus one `try_switch` when the provider differs from the one current at request start;
- `fail_provider(error, status_message)`: breaker failure + DB health + `last_error` (no `failed_requests`, since the loop moves on to the next provider);
- `fail_request(message)`: neutral permit release + request failure.
`AttemptRecorder::fail_request` handles the terminal "all providers failed or blocked" paths. Dropping an `Attempt` without recording (a cancelled request) still releases the probe slot through the guard. `forward_with_retry_inner` and `handle_rectifier_retry_failure` now only call the recorder; `record_success_result` was removed. Behaviour is unchanged (failover counting semantics are left to CCS-128).

**Files.** `src-tauri/src/proxy/pipeline/mod.rs` (new), `src-tauri/src/proxy/pipeline/failover.rs` (new, with 6 unit tests), `src-tauri/src/proxy/mod.rs`, `src-tauri/src/proxy/forwarder.rs` (-390 lines).

**Commits.** `1f995a3` refactor(proxy): record each failover attempt through AttemptRecorder (CCS-127); this ticket-file update follows as a separate docs commit.

**Verification.**
- `grep -c success_rate src-tauri/src/proxy/forwarder.rs` gives 0 (was 16); `grep -c try_switch` gives 0 (was 4).
- `cargo fmt --check`: clean. `cargo clippy -- -D warnings` and `cargo clippy --all-targets -- -D warnings`: clean.
- `cargo test --lib proxy::pipeline::failover`: 6 passed (stats and failover on success, breaker trip on provider failure, single-provider bypass, neutral half-open release on client error, release on drop, success rate on request failure).
- `cargo test --lib half_open`: 11 passed (P1 half-open tests, including `cancelled_half_open_probe_releases_the_permit`).
- `cargo test --no-fail-fast` (full suite): lib 2250 passed / 0 failed / 3 ignored; integration tests all green, including `proxy_e2e` 19 passed and `proxy_golden` 6 passed.
