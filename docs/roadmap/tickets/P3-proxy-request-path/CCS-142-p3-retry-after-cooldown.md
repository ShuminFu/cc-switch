# CCS-142: [proxy] Honour Retry-After on 429 and 529 with a per-provider cooldown

**Status:** Done · **Kind:** feature · **Labels:** `feature`, `phase:P3`, `p3-low`, `area:proxy`

> **Ticket key:** `p3-retry-after-cooldown` · **Phase:** P3 · **Priority:** p3-low · **Effort:** S · **Depends on:** `p3-attempt-recorder`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Proxy review idea 3. Rate-limit responses count as breaker failures and are retried immediately.

## Proposed change
- Parse Retry-After (seconds or an HTTP date) and put the provider in a cooldown that select_providers respects.
- Do not count it as a breaker failure.

## Acceptance criteria / verification
- [x] E2E: P1 returns 429 with Retry-After 2, so requests go to P2 for about 2 s and then back to P1; the breaker stays Closed
- [x] `cargo test --manifest-path src-tauri/Cargo.toml --test proxy_e2e`

## Dependencies
p3-attempt-recorder

## Implementation notes

**Summary.** A 429 or 529 upstream response that carries `Retry-After` (delay-seconds or an IMF-fixdate HTTP-date; a past date means zero; capped at 600 s) now puts the provider into a per-(app, provider) cooldown held by `ProviderRouter`. The cooldown is separate from the circuit breaker. While a provider is cooling down, `select_providers` (failover on) skips it. If every provider that is otherwise available is cooling down, they are still returned in queue order as a last resort, so the request does not fail with "no providers". The failed attempt fails over to the next provider. It is not recorded as a breaker failure or in DB health, and any HalfOpen permit is released neutrally. It still counts as an earlier-provider failure, so the next provider taking over counts as a failover. A new log code, `FWD-004`, is used for this. A 429/529 with no parseable `Retry-After` keeps the old behaviour: it is retryable and counts as a breaker failure. When failover is off, single-provider routing ignores the cooldown, as it already ignores the breaker.

**Files.**
- `src-tauri/src/proxy/pipeline/rate_limit.rs` (new): `is_rate_limit_status`, `rate_limit_retry_after`, `MAX_COOLDOWN`, unit tests
- `src-tauri/src/proxy/provider_router.rs`: `start_cooldown` / `cooldown_remaining`, cooldown-aware `select_providers`, unit test
- `src-tauri/src/proxy/pipeline/failover.rs`: `Attempt::rate_limited`, `AttemptFailureInput::retry_after`, rate-limit branch in `settle_failure`, unit tests
- `src-tauri/src/proxy/forwarder.rs` (+ `forwarder/tests.rs`): `forward` reports the `Retry-After` of the response it received, and `send_with_recovery` passes it on
- `src-tauri/src/proxy/log_codes.rs`: `FWD-004`
- `src-tauri/src/proxy_harness.rs`: `MockReply::with_header`
- `src-tauri/tests/proxy_e2e.rs`: `retry_after_cools_the_provider_down_without_tripping_the_breaker`

**Commits.** 731b766 (code); this ticket edit is in a follow-up docs commit.

**Verification.**
- `cargo test --manifest-path src-tauri/Cargo.toml --test proxy_e2e`: 25 passed / 0 failed. The new E2E test: P1 returns 429 with `Retry-After: 2` and the request goes to P2. The next 3 requests within 2 s also go to P2 (P1 hits stay at 1). P1's breaker stays Closed with 0 consecutive failures (failure_threshold = 1, so one counted failure would have opened it) and its health is untouched. After about 2 s the next request goes back to P1.
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`: clean. `cargo fmt --check`: clean.
- `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast`: lib 2369 passed / 0 failed / 3 ignored, and all integration targets pass except one intermittent failure in `tests/migrations.rs::every_released_schema_migrates_to_current`. That failure is "v9 fixture: row not preserved in proxy_request_logs" and fails about 1 run in 3–10 when the test is run alone. It does not touch any code changed here (no proxy code involved) and is a pre-existing flake to look into separately.
- No frontend changes, so pnpm checks were not needed.
