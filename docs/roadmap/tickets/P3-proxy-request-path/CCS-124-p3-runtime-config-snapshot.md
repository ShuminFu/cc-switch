# CCS-124: [proxy] Cache runtime config in an ArcSwap snapshot and remove DB reads from the request path

**Status:** Done · **Kind:** refactor · **Labels:** `refactor`, `phase:P3`, `p2-medium`, `area:proxy`

> **Ticket key:** `p3-runtime-config-snapshot` · **Phase:** P3 · **Priority:** p2-medium · **Effort:** M · **Depends on:** `p3-endpoint-table`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Each request does about 5 DB or config reads.

## Evidence
- proxy/handler_context.rs:95-110; provider_router.rs:43/134; the RequestForwarder 17-argument constructor

## Proposed change
- Add `ProxyState.runtime_config: ArcSwap<RuntimeConfigSnapshot>`, refreshed in apply_runtime_config and update_config.
- Replace the constructor with `RequestForwarder { shared: Arc<ForwarderDeps>, req: RequestScope }`.

## Acceptance criteria / verification
- [x] The proxy_e2e DB-query counter reads 0 per request
- [x] p50 latency is no worse than the P0 baseline
- [x] `cargo test --manifest-path src-tauri/Cargo.toml --test proxy_e2e`

## Dependencies
p3-endpoint-table

## Implementation notes

**Summary.** Request-path configuration now comes from a cached snapshot instead of the database.

- New `proxy/runtime_config.rs`: `RuntimeConfigStore { db, current: ArcSwap<RuntimeConfigSnapshot> }`. A snapshot is tagged with `Database::config_generation()` and lazily caches per-app `AppProxyConfig`, rectifier/optimizer/copilot-optimizer settings, per-app providers + failover queue + `is_current`, default cost multiplier / pricing source, and model pricing lookups. Failed loads are not cached (next access retries, preserving the old error/fallback behaviour).
- Invalidation: the existing SQLite update hook now also bumps a per-`Database` `config_generation` for every INSERT/UPDATE/DELETE except the per-request tables (`proxy_request_logs`, `provider_health`, `stream_check_logs`, `usage_daily_rollups`, `session_log_sync`). Backup-API whole-DB overwrites (`import_sql*`, `restore_from_backup`) call `mark_config_changed()` explicitly since the hook does not see them; `ProxyServer::apply_runtime_config` (used by `ProxyService::update_config`) calls `runtime_config.invalidate()`.
- `ProxyState.runtime_config` is shared with `ProviderRouter::with_runtime_config` (select_providers, record_result threshold, breaker creation), `FailoverSwitchManager::with_runtime_config` (enabled check), `RequestContext::new`, and `UsageLogger::with_runtime_config` (pricing config, provider meta, model pricing). The device-level current provider is still read live from local settings; the "local id not in DB -> clear and fall back to `is_current`" rule is kept (`AppProviders::effective_current_provider`).
- `update_provider_health_with_threshold` is a single `INSERT ... ON CONFLICT DO UPDATE` (counter increment/reset done in SQL), removing the read-before-write.
- `RequestForwarder` is now `{ shared: Arc<ForwarderDeps>, req: RequestScope }`; `ProxyState::new` builds `forwarder_deps` once (the now-redundant `ProxyState.app_handle` / `failover_manager` fields moved into it).
- `proxy_harness` counts read statements (`db_read_statement_count`); `perf_baseline_non_streaming_request` asserts 0 SELECTs per warm request and records `db_reads_per_request`. New e2e test `config_and_provider_changes_reach_the_next_request_without_restart` proves DB writes (current-provider switch, enabling failover) reach the next request with a warm snapshot.
- Dependency: `arc-swap = "1.7"` (1.7.1).

**Files.** `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`, `src-tauri/src/proxy/runtime_config.rs` (new), `src-tauri/src/proxy/{server,handler_context,forwarder,provider_router,failover_switch,handlers,response_processor,mod}.rs`, `src-tauri/src/proxy/usage/logger.rs`, `src-tauri/src/database/{mod,backup}.rs`, `src-tauri/src/database/dao/proxy.rs`, `src-tauri/src/proxy_harness.rs`, `src-tauri/tests/proxy_e2e.rs`, `docs/dev/perf-baseline.json`.

**Commits.** `4d872d4` (implementation), plus the follow-up commit that records these notes.

**Verification.**

- SQL trace of one warm request before the change: 17 statements (15 SELECTs: proxy_config x4, settings x3, providers x3, provider_health x1, cost multiplier, pricing source, model_pricing x2; 2 writes). After: 2 statements (request-log INSERT, provider-health UPSERT), 0 SELECTs.
- `cargo test --manifest-path src-tauri/Cargo.toml --test proxy_e2e` -> 16 passed, 0 failed. Perf line: `db_statements_per_request: 2.0, db_reads_per_request: 0.0, allocations_per_request: 582` (baseline 17.0 / 593).
- p50 latency, 5 interleaved runs of `perf_baseline` on this machine (debug): HEAD before the change 2.91 / 3.66 / 2.99 / 2.81 / 2.87 ms vs. after 2.26 / 2.28 / 2.24 / 2.19 / 2.18 ms. `docs/dev/perf-baseline.json` refreshed via `CC_SWITCH_UPDATE_PERF_BASELINE=1` + prettier (p50 2.28 ms).
- `cargo fmt --check` clean; `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` clean.
- Full suite `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast`: 2395 passed / 0 failed (lib 2235 passed, 3 ignored), across repeated runs. One run hit `tests/migrations.rs::every_released_schema_migrates_to_current` ("row not preserved in proxy_request_logs"). That flake also reproduces about 4 times in 30 runs on a build of the pre-change HEAD, so this change did not introduce it.
- `pnpm format:check` clean (no frontend changes).

**Review.** I checked each acceptance criterion against the code and they hold. The proxy_e2e perf test fails if any SELECT runs on a warm request, and a rerun gave `db_reads_per_request: 0.0`. Invalidation covers every config write: the update hook bumps the generation on writes to any table that is not exempt, Backup-API restores bump it explicitly, and `apply_runtime_config` calls `invalidate()`. No config table is `WITHOUT ROWID`, and the only unconditional `DELETE FROM` (the truncate path, which the hook does not see) on a cached table is in the `model_pricing` migrations, which reseed straight after.

Gap fixed in the follow-up:
- The Claude Desktop gateway routes still read the database on every request: `validate_claude_desktop_gateway_auth` called `get_or_create_gateway_token`, which runs a `settings` SELECT.
  - The token is now cached in the snapshot (`RuntimeConfigStore::claude_desktop_gateway_token`). Rotating it writes to `settings`, which invalidates the cache.
  - New unit test for this.

Added tests:
- A DAO unit test for the single-statement provider-health UPSERT. It checks the failure count increments, `is_healthy` follows the threshold (including threshold 1 on a new row), and a success resets the count without overwriting `last_failure_at`.

Re-verified:
- fmt and clippy are clean.
- `--test proxy_e2e`: 16 passed.
- Full `cargo test --no-fail-fast`: 2397 passed / 0 failed.
