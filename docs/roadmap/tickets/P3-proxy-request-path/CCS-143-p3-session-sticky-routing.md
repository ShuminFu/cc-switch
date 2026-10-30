# CCS-143: [proxy] Optionally keep each session_id on one provider so prompt caches survive

**Status:** Done · **Kind:** feature · **Labels:** `feature`, `phase:P3`, `p3-low`, `area:proxy`

> **Ticket key:** `p3-session-sticky-routing` · **Phase:** P3 · **Priority:** p3-low · **Effort:** S · **Depends on:** `p3-failover-flapping`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Proxy review idea 4. Routing always starts at the head of the queue, so a session that failed over loses its prompt cache when it returns.

## Proposed change
Add an opt-in setting: a bounded LRU mapping session_id to provider. Prefer the sticky provider while its breaker is available.

## Acceptance criteria / verification
- [x] E2E: with the setting on, after a failover the same session_id stays on P2 until P2 fails
- [x] The LRU is bounded
- [x] `cargo test --manifest-path src-tauri/Cargo.toml --test proxy_e2e`

## Dependencies
p3-failover-flapping

## Implementation notes

**Summary.** This adds an opt-in device setting, `stickySessionRouting` (`AppSettings::sticky_session_routing`, default off), shown as a toggle in Settings → Proxy → Auto Failover. When the setting is on, failover is enabled for the app, and the session_id came from the client (generated UUIDs are ignored), these things happen:
- `RequestContext` passes the session to `ProviderRouter::select_providers_for_session`.
- The router keeps a bounded LRU of (app, session_id) → provider_id, with `STICKY_SESSION_CAPACITY` = 1024. It is an `IndexMap` in recency order, and the least recently used entry is evicted.
- If the sticky provider is in the available list (breaker allows it and it is not in a Retry-After cooldown), the router moves it to the head of the list. The other providers keep queue order.
- After a successful attempt, the forwarder records the provider that served the request.

If the sticky provider fails, the request fails over as usual, and the provider that succeeds becomes the new sticky one. When the setting is off, or failover is off, routing is unchanged. The settings file did not need a schema migration.

**Files.**
- `src-tauri/src/proxy/provider_router.rs`: `StickySessions` LRU, `STICKY_SESSION_CAPACITY`, `select_providers_for_session`, `remember_session_provider`, `sticky_session_provider`, unit tests
- `src-tauri/src/proxy/handler_context.rs`: `RequestContext::sticky_session` is computed and passed to selection and to `RequestScope`
- `src-tauri/src/proxy/forwarder.rs` (+ `forwarder/tests.rs`, `forwarder/wire_golden.rs`): `RequestScope::sticky_session`, which records the provider after a successful attempt
- `src-tauri/src/settings.rs`: `sticky_session_routing` field and `sticky_session_routing_enabled()`
- `src-tauri/tests/proxy_e2e.rs`: `sticky_session_stays_on_the_failover_provider_until_it_fails`, `sticky_session_routing_is_off_by_default`
- `src/types.ts`, `src/components/settings/ProxyTabContent.tsx`, `src/i18n/locales/{en,zh,zh-TW,ja}.json`: settings toggle

**Commits.** a974312 (code); this ticket edit is in a follow-up docs commit.

**Verification.**
- `cargo test --manifest-path src-tauri/Cargo.toml --test proxy_e2e`: 27 passed / 0 failed. The new E2E test sets breaker threshold 5, so only the sticky table moves traffic:
  1. P1 returns 500, and session s1 fails over to P2.
  2. P1 is healthy again, but the next 2 s1 requests still go to P2.
  3. Session s2, and a request with no client session id, go to P1.
  4. P2 returns 500, s1 fails over to P1 and stays on P1 afterwards.
- The off-by-default E2E test shows that without the setting, s1 goes back to P1 right after the failover. So the sticky test fails without the feature.
- Unit tests: `test_sticky_sessions_evicts_least_recently_used` and `test_sticky_session_table_is_bounded` cover the bound (1025 inserts leave 1024 entries, with the oldest evicted). `test_sticky_session_prefers_its_provider_while_available` covers per-session and per-app scope, and checks that the sticky provider is not preferred while it is cooling down or its breaker is open.
- `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast`: lib 2372 passed / 0 failed / 3 ignored, and all integration targets pass.
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`: clean. `cargo fmt`: clean.
- `pnpm typecheck`: clean. `pnpm format:check`: clean. `pnpm test:unit`: 156 files / 1516 tests passed.
