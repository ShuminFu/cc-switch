# CCS-040: [proxy] Use one shared default for the circuit-breaker failure_threshold

**Status:** Done · **Kind:** bug · **Labels:** `bug`, `phase:P0`, `p3-low`, `area:proxy`

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

## Implementation notes

**Summary.** Added `pub const DEFAULT_CIRCUIT_FAILURE_THRESHOLD: u32 = 4` in `proxy/circuit_breaker.rs` and used it for `CircuitBreakerConfig::default()` and the DAO fallback in `get_proxy_config_for_app`. `ProviderRouter::record_result` no longer falls back to a hard-coded `5`: it fetches the breaker first and, when the `proxy_config` read fails, uses the breaker's own configured threshold (new `CircuitBreaker::failure_threshold()` getter) and logs a warning. The DB health flag and the breaker now flip on the same failure. The schema.rs SQL `DEFAULT 4` literals stay as SQL text. A test now checks that they match the constant. The per-app seed values (for example `claude` = 8) are intentional and were left unchanged.

**Files**
- `src-tauri/src/proxy/circuit_breaker.rs`: constant, default, `failure_threshold()` getter
- `src-tauri/src/proxy/provider_router.rs`: `record_result` fallback, plus tests
- `src-tauri/src/database/dao/proxy.rs`: DAO fallback uses the constant

**Tests added** (`proxy::provider_router::tests`)
- `test_record_result_config_read_failure_uses_default_threshold`: drops `proxy_config` so the config read fails, then checks that the provider stays healthy after 3 failures and becomes unhealthy after 4, with the breaker `Open`. It was confirmed to fail when the fallback is temporarily set back to `5`.
- `test_failure_threshold_defaults_are_consistent`: checks that the breaker default and the schema column DEFAULT (row inserted with only `app_type`) both equal the constant.

**Commits:** 3963da4 (code + tests); this ticket update is in a follow-up commit.

**Verification**
- `grep -rn 'Err(_) => 5' src-tauri/src/proxy`: no matches. The literal `grep -rn '=> 5'` from the checklist still matches unrelated code: HTTP status mappings such as `=> 503` in `error_mapper.rs`, and `_ => 5` (polling interval) in `providers/codex_oauth_auth.rs`. None of these are failure-threshold fallbacks.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib provider_router`: 8 passed
- `cargo test --manifest-path src-tauri/Cargo.toml --lib`: 2022 passed, 0 failed, 2 ignored
- `cargo fmt --check`: clean. `cargo clippy -- -D warnings` and `cargo clippy --all-targets -- -D warnings`: clean.
