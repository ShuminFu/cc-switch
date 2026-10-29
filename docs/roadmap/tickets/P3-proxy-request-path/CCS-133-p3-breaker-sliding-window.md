# CCS-133: [proxy] Compute the circuit-breaker error rate over a sliding window

**Status:** Done · **Kind:** bug · **Labels:** `bug`, `phase:P3`, `p3-low`, `area:proxy`

> **Ticket key:** `p3-breaker-sliding-window` · **Phase:** P3 · **Priority:** p3-low · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding proxy-breaker-lifetime-rate, partially real. The error rate covers everything since the last HalfOpen->Closed transition. A long clean history hides an intermittent outage, and a borderline long-run rate trips on one more failure. A sustained outage is still caught by the 4-consecutive rule.

## Evidence
- src-tauri/src/proxy/circuit_breaker.rs:203-288, 259-283, 380-387, 121-123

## Proposed change
Use a ring buffer of the last N outcomes, or 60 s time buckets, with min_requests applied to the window. This also removes the u32 overflow.

## Acceptance criteria / verification
- [x] Test: 1000 successes, then 30 failure/success alternations, opens the breaker
- [x] Test: old failures that have left the window do not trip it
- [x] `cargo test --manifest-path src-tauri/Cargo.toml circuit_breaker`

## Dependencies
None.

## Implementation notes

**Summary.** `CircuitBreaker` now computes the error rate over a ring buffer
of the last `ERROR_RATE_WINDOW_SIZE` (100) outcomes (`OutcomeWindow`, a
`VecDeque<bool>` plus a running failure count behind a `Mutex`) instead of
the lifetime `total_requests` / `failed_requests` `AtomicU32` counters, which
are removed (no more u32 overflow). `min_requests` is applied to the number of
outcomes in the window and capped at the window size so a large configured
value cannot disable the error-rate check. The window is cleared on
`transition_to_closed` / `reset`, as the counters were. The 4-consecutive
failure rule is unchanged. `get_stats()` keeps its shape; `total_requests` /
`failed_requests` now report the window counts (max 100).

**Files.** `src-tauri/src/proxy/circuit_breaker.rs` (implementation + tests).

**Tests added.**
- `test_intermittent_failures_after_long_clean_history_open_breaker`: 1000
  successes, then failure/success alternations (threshold 0.3) opens the
  breaker on the 30th failure (30/100 in window). Under the old lifetime rate
  it was ~30/1059 = 2.8%, so this test failed before the change.
- `test_failures_that_left_the_window_do_not_trip_breaker`: 49 failures at a
  49% window rate, then a full window of successes; stats show 0 failures in
  the window and two new failures (2/100) do not trip.
- `test_min_requests_applies_to_window_and_is_capped_at_window_size`.

**Commits.** `1c5a217` (code), plus this ticket-file commit.

**Verification.**
- `cargo fmt --manifest-path src-tauri/Cargo.toml`: clean.
- `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`: clean.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib circuit_breaker`:
  12 passed, 0 failed.
- `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast`: 2451
  passed, 0 failed.
