# CCS-128: [proxy] Count failovers only when a request actually failed over, and report recovery separately

**Status:** Done · **Kind:** bug · **Labels:** `bug`, `phase:P3`, `p3-low`, `area:proxy`

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
- [x] Test: queue [P1,P2], P1 returns 5xx once and P2 returns 200, so count==1; the next request where P1 returns 200 keeps count 1 and recovery 1
- [x] Test: N parallel requests increment the count once
- [x] `cargo test --manifest-path src-tauri/Cargo.toml proxy::forwarder`

## Dependencies
p3-attempt-recorder

## Implementation notes

**Summary.** `AttemptRecorder` (`pipeline/failover.rs`) now carries an `earlier_provider_failed` flag, set by `Attempt::fail_provider` and when `begin` is rejected by the circuit breaker. On `Attempt::succeed`, the switch is counted only when the provider differs from the one current at request start **and** from the previous entry in `current_providers` (swapped under its write lock, so concurrent requests that start before the async `try_switch` lands count the switch once). A counted switch goes to `failover_count` if an earlier provider failed in this loop, otherwise to the new `ProxyStatus.recovery_count` (`#[serde(default)]`, optional in the TS bindings) with an info log, so a return to a higher-priority provider is no longer reported as a failover. `try_switch` is still spawned whenever the provider differs from the start provider (it dedupes itself), so UI/tray sync is unchanged. Counting on `try_switch` returning `Ok(true)` was not used: the result is only known asynchronously after the response is returned, and it is always `Ok(false)` without an `AppHandle` (tests/headless); the `current_providers` swap gives the same once-per-transition guarantee synchronously. No minimum dwell time was added (optional in the ticket).

**Files.** `src-tauri/src/proxy/pipeline/failover.rs`, `src-tauri/src/proxy/types.rs`, `src-tauri/src/proxy/forwarder.rs` (tests), `src/shared/bindings.ts` (regenerated).

**Commits.** `7b919bf` fix(proxy): count failovers only after an earlier provider failed (CCS-128); this ticket-file update follows as a docs commit.

**Verification.**
- New forwarder tests against local upstreams: `transient_failure_counts_one_failover_and_return_counts_recovery` (queue [P1,P2], P1 5xx once then P2 200: failover 1; next request P1 200 with current P2: failover 1, recovery 1; a third request stays at 1/1) and `parallel_failovers_before_the_switch_lands_count_once` (8 parallel requests, P1 always 5xx: failover 1). With the old counting logic restored they fail (`left: 2, right: 1` and `left: 8, right: 1`).
- New recorder unit tests: breaker-rejected provider counts as earlier failure, first-attempt success on another provider counts recovery, concurrent switches to the same provider count once; the old `success_on_other_provider_counts_one_failover` now fails a provider first.
- `cargo test --lib proxy::`: 1234 passed / 0 failed. `cargo test --no-fail-fast` (full): lib 2255 passed / 0 failed / 3 ignored, all integration suites green (incl. `proxy_e2e` 19 passed).
- `cargo fmt --check` clean; `cargo clippy -- -D warnings` clean; `UPDATE_BINDINGS=1 cargo test --lib bindings` 2 passed.
- `pnpm typecheck` clean; `pnpm test:unit` 154 files / 1501 tests passed; `pnpm format:check` clean.
