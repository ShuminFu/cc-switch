# CCS-141: [proxy] Show live circuit-breaker state with a "probe now" action

**Status:** Done · **Kind:** feature · **Labels:** `feature`, `phase:P3`, `p3-low`, `area:proxy-ui`

> **Ticket key:** `p3-breaker-ui-probe` · **Phase:** P3 · **Priority:** p3-low · **Effort:** M · **Depends on:** `p2-proxy-dead-code-purge`, `p1-proxy-halfopen-permit`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Proxy review idea 2. The breaker state is invisible, and get_circuit_breaker_stats and get_state exist but are marked dead_code.

## Proposed change
Expose the per-provider breaker state (Closed, Open with time remaining, HalfOpen) through an event or a poll. Add "probe now", which sends a minimal request through the half-open path.

## Acceptance criteria / verification
- [x] E2E: force Open, then probe now against a healthy mock gives Closed
- [x] RTL: the state badge renders
- [x] `pnpm test:unit` and `cargo test --manifest-path src-tauri/Cargo.toml circuit_breaker`

## Dependencies
p2-proxy-dead-code-purge, p1-proxy-halfopen-permit

## Implementation notes

**Summary.** Both dependencies had already landed (half-open permit RAII guard, dead-code purge leaving `get_state` / `get_circuit_breaker_stats` reserved for this ticket).

- Backend state: `CircuitBreakerStats` gains `openRemainingSecs` (seconds until the open timeout lets a half-open probe through; `null` unless Open). `get_circuit_breaker_stats` (which used to always return `null`) now reads the running proxy's in-memory breaker via `ProxyService::get_provider_circuit_breaker_stats`. It still returns `null` when the proxy is stopped or the provider has no breaker yet. The UI polls it every 5 s and counts down locally between polls.
- Probe now: a new `probe_circuit_breaker` command calls `ProviderRouter::probe_provider`, which:
  - skips the open timeout (`CircuitBreaker::begin_manual_probe`: Open → HalfOpen) and takes the half-open permit;
  - sends one minimal request (`StreamCheckService::probe_circuit_target`). This is the same zero-token GET of the provider base URL that the reachability check uses, but here a network error, 5xx or 429 counts as still down;
  - records the result through the permit (`record_manual_probe_result`). Success closes the breaker without waiting for `success_threshold`. Failure re-opens it and restarts the timer. The DB health row is updated with the configured threshold either way.
  - A Closed breaker, or one with a probe already in flight, is not probed (`probed: false`).
  - `ProxyService` clones the router `Arc` (`ProxyServer::provider_router`) so it does not hold the server lock during the request.
- UI: `CircuitBreakerBadge` (Closed / Open · Ns / Half-open, plus a "Probe now" button when not Closed) sits next to the health badge on each failover-queue row in `ProxyPanel`. There is a `useProbeCircuitBreaker` mutation, and i18n keys were added in en/zh/zh-TW/ja.
- Scope note: unlike `reset_circuit_breaker`, a successful probe does not trigger the automatic switch back to a higher-priority provider. Real traffic routes to it again once the breaker is Closed.

**Files.** `src-tauri/src/proxy/{circuit_breaker,provider_router,server,mod}.rs`, `src-tauri/src/services/{proxy,stream_check}.rs`, `src-tauri/src/commands/{proxy,stream_check}.rs`, `src-tauri/src/lib.rs`, `src-tauri/src/proxy_harness.rs`, `src-tauri/tests/proxy_e2e.rs`, `src/components/proxy/{CircuitBreakerBadge,ProxyPanel}.tsx`, `src/lib/{api,query}/failover.ts`, `src/types/proxy.ts`, `src/shared/bindings.ts` (regenerated `COMMANDS`), `src/i18n/locales/*.json`, `tests/components/{CircuitBreakerBadge,ProxyPanel}.test.tsx`, `tests/msw/handlers.ts`.

**Commits.** `00406dc` feat(proxy): show live circuit-breaker state with a probe-now action (CCS-141), plus this ticket-file commit.

**Verification.**
- E2E: `cargo test --test proxy_e2e probe_now` passed 2/2.
  - `probe_now_closes_an_open_breaker_against_a_healthy_upstream`: forces Open with a 600 s timeout and success_threshold 3. Probe now against the healthy mock gives Closed and a healthy DB row, and real traffic is served by the provider again.
  - `probe_now_against_a_failing_upstream_keeps_the_breaker_open`.
- `cargo test --manifest-path src-tauri/Cargo.toml circuit_breaker`: 16 passed (4 new: open countdown, manual probe success/failure, Closed no-op). There is also a new `stream_check` status-classification unit test.
- RTL: `tests/components/CircuitBreakerBadge.test.tsx` passed 7/7. It covers no breaker, plus the closed, open (time remaining) and half-open badges, probe success and failure toasts, and the pending state.
- `cargo fmt --check` was clean. `cargo clippy -- -D warnings` was clean.
- Full `cargo test --no-fail-fast`: 2536 passed / 0 failed. The lib suite was 2362 passed / 0 failed / 3 ignored, including `bindings_are_up_to_date` after `UPDATE_BINDINGS=1`.
- `pnpm typecheck` was clean. `pnpm test:unit` passed 1516 tests in 156 files. `pnpm format:check` was clean.
