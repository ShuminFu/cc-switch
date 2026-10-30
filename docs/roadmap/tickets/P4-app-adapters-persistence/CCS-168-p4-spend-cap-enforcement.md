# CCS-168: [proxy] Enforce per-provider spend and token caps in provider selection

**Status:** Done · **Kind:** feature · **Labels:** `feature`, `phase:P4`, `p2-medium`, `area:proxy`

> **Ticket key:** `p4-spend-cap-enforcement` · **Phase:** P4 · **Priority:** p2-medium · **Effort:** M · **Depends on:** `p4-usage-repo`, `p3-runtime-config-snapshot`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
The critique notes limits are not enforced at all; check_provider_limits is called only from the UI (commands/usage.rs:257). Proxy review idea 6.

## Proposed change
- Keep running per-provider totals in memory, seeded from the UsageRepo.
- select_providers skips a provider over its cap and fails over. With no candidate left, return a clear 429-style error rendered per client format.

## Acceptance criteria / verification
- [x] E2E: P1 at its cap routes to P2; with all at cap, the error renders in Anthropic format
- [x] `cargo test --manifest-path src-tauri/Cargo.toml --test proxy_e2e`

## Dependencies
p4-usage-repo, p3-runtime-config-snapshot

## Implementation notes

**Summary.** The provider router now enforces the existing `limitDailyUsd` / `limitMonthlyUsd` provider limits.

- `proxy/spend_caps.rs` (new): `SpendCapTracker` keeps running local-day and local-month totals per (app, provider) in memory. It only does this for providers that have a cap.
  - Totals are seeded lazily from the usage repo through the new `Database::provider_spend_at`. This uses the same windows and SQL (detail + rollups) as `check_provider_limits`, which now shares the helper.
  - After a proxy usage row is written, `UsageLogger::with_spend_caps` adds that row's cost to the totals.
  - Totals are reseeded on a day or month rollover, or once they are older than 5 minutes. This absorbs log pruning, session-log imports and the small seed/record race.
  - A provider with no cap never touches the DB, so the perf baseline is unchanged.
  - If a seed read fails, the error is logged and the request is allowed. Nothing is cached, so the next request retries the read.
- `ProviderRouter::select_providers_for_session` skips a provider whose usage is at or over its cap (`usage >= limit`, the same rule the UI uses):
  - With failover on, the request fails over to the next queued provider.
  - With failover off, the current provider is refused.
  - If no candidate is left, it returns the new `AppError::SpendCapExceeded`, which maps to `ProxyError::SpendCapExceeded` (429, log code `FO-006`). An all-circuit-open queue still reports `AllProvidersCircuitOpen`.
- `errors/render.rs::render_selection_error` renders a selection failure in the client's wire format:
  - Anthropic: `{"type":"error","error":{"type":"rate_limit_error",...}}`
  - Gemini: `{"error":{"code":429,"status":"RESOURCE_EXHAUSTED",...}}`
  - OpenAI Chat/Responses: `{"error":{"type":"insufficient_quota","code":"cc_switch_spend_cap_exceeded",...}}`

  `handle_proxied` uses it when `RequestContext::new` fails. Other selection errors keep their previous rendering.
- Out of scope: there is no token-count cap setting in provider meta (only USD limits exist), so only spend caps are enforced. A token cap would need a new meta field and UI first.

**Files:**
- `src-tauri/src/proxy/spend_caps.rs` (new)
- `src-tauri/src/proxy/provider_router.rs`, `src-tauri/src/proxy/usage/logger.rs`, `src-tauri/src/proxy/response_processor.rs`
- `src-tauri/src/proxy/handlers.rs`, `src-tauri/src/proxy/handler_context.rs`, `src-tauri/src/proxy/errors/render.rs`
- `src-tauri/src/proxy/error.rs`, `src-tauri/src/proxy/error_mapper.rs`, `src-tauri/src/proxy/log_codes.rs`, `src-tauri/src/proxy/mod.rs`
- `src-tauri/src/error.rs`, `src-tauri/src/database/repo/usage.rs`
- `src-tauri/src/proxy_harness.rs` (`seed_spend`), `src-tauri/tests/proxy_e2e.rs`

**Commits:** 79725fe (implementation and tests); this ticket update is a follow-up commit.

**Tests added:**
- E2E (`tests/proxy_e2e.rs`): `provider_at_its_spend_cap_routes_to_the_next_provider`, `spend_recorded_by_the_proxy_counts_toward_the_cap`, `all_providers_at_their_spend_cap_get_an_anthropic_rate_limit_error`. All three fail on the old code, where the capped P1 is still called.
- Unit tests: `proxy::spend_caps::tests` (5), `provider_router` tests for failover-queue skipping and for the refused current provider, and `errors::render` tests for the 429 rendering per format.

**Verification:**
- `cargo fmt --manifest-path src-tauri/Cargo.toml --check`: clean
- `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` and `--all-targets -- -D warnings`: clean
- `cargo test --manifest-path src-tauri/Cargo.toml --test proxy_e2e`: 37 passed (including the perf-baseline scenarios)
- `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast` (full suite): all binaries ok, 0 failed (lib: 2800 passed, 3 ignored)

**Review.** An independent review checked both acceptance criteria against commits 79725fe and 0134c8c, and both hold.
- The tracker's seeding uses the same windows and SQL as `check_provider_limits`, and both treat usage ≥ limit as over the cap.
- Lock order is safe: seeding takes the totals lock and then the DB lock, while the logger releases the DB lock before it records.
- The cost recorded in memory is the same `total_cost` that is written to the log row.
- The E2E tests would fail on the old code, because there the capped P1 is still called.

One gap was fixed. `handle_claude_desktop_models` also calls `select_providers`, and it mapped every router error to `DatabaseError`. So a Claude Desktop current provider at its cap made the model list fail with a misleading 500 "数据库错误". It now returns `ProxyError::SpendCapExceeded` (429).

Re-verified after the fix:
- fmt and clippy `--all-targets -D warnings`: clean
- `--test proxy_e2e`: 37 passed
- `--lib`: 2800 passed
- full `--no-fail-fast`: 0 failed
