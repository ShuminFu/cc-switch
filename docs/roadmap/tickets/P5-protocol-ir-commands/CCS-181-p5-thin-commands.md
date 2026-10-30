# CCS-181: [commands] Make commands thin adapters over a Ctx/EventSink seam and reach 100% IpcError

**Status:** Done · **Kind:** refactor · **Labels:** `refactor`, `phase:P5`, `p2-medium`, `area:commands`

> **Ticket key:** `p5-thin-commands` · **Phase:** P5 · **Priority:** p2-medium · **Effort:** L · **Depends on:** `p5-command-domains-permissions`, `p2-ipc-error-type`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Evidence
- commands/proxy.rs:349-406 reset_circuit_breaker policy; commands/provider.rs:421-676 query_provider_usage_inner; *_internal/*_test_hook triplets

## Proposed change
- Add a Ctx/EventSink seam.
- Move the policy into a FailoverService and services/usage_query.rs.
- Delete the triplets.
- Add a CI check that fails on a command body over 40 lines or on direct state.db access.
- Migrate all remaining commands to IpcError.

## Acceptance criteria / verification
- [x] No *_test_hook remains
- [x] 100% of commands return IpcError (count script)
- [x] The CI body-length check passes
- [x] `cargo test --manifest-path src-tauri/Cargo.toml`

## Dependencies
p5-command-domains-permissions, p2-ipc-error-type

## Implementation notes

**Summary**
- Ctx/EventSink seam: `src-tauri/src/ctx.rs` adds `Ctx { state, events }`, the `EventSink` trait
  (emit, immediate / debounced tray refresh, optional `AppHandle`), `TauriEventSink` for commands and
  `RecordingEventSink` for tests. Services that used to need an `AppHandle` take `&Ctx`.
- Policy moved out of commands:
  - `services/failover.rs` (`FailoverService`): queue ops, auto-failover toggle (P1 switch / auto-add /
    rollback), provider health, `reset_circuit_breaker` with the switch-back-to-higher-priority policy
    (pure `recovery_priorities` helper), probe-now.
  - `services/usage_query.rs`: `query_provider_usage` (former `query_provider_usage_inner`) plus
    `refresh_provider_usage` / `refresh_subscription_quota` (cache + `usage-cache-updated` + tray via the sink).
  - Other thin-command moves: `services/proxy_settings.rs` (global / per-app proxy config, cost multiplier,
    pricing source, circuit breaker / rectifier / optimizer / log config, proxy hot-switch validation),
    `services/common_config.rs`, `services/global_proxy.rs`, `services/claude_desktop_import.rs`,
    `services/openclaw_memory.rs`, `UsageStatsService` in `services/usage_stats.rs`, stream-check
    orchestration in `StreamCheckService`, plus small additions to `McpService`, `OmoService`,
    `ProfileService`, `SkillService`, `ProviderService` (`import_default_config_with_common_snippet`,
    `ensure_official_provider`). `services/proxy.rs` was not touched.
- Triplets deleted: `switch_provider_*`, `import_default_config_*`, `{get,set}_default_cost_multiplier_*`,
  `{get,set}_pricing_model_source_*`; integration tests call the services directly. The now-unused
  `test-hooks` Cargo feature and its README mentions were removed.
- IpcError: every `#[tauri::command]` (302/302, including the ACL test stand-ins and `update_tray_menu`)
  returns `Result<_, IpcError>`. Previously infallible commands (`get_app_registry`,
  `cancel_tool_lifecycle_action`, `get_upstream_proxy_status`, `scan_local_proxies`,
  `get_claude_desktop_default_routes`) now return `Ok(..)`; the frontend payload is unchanged.
- CI check: `scripts/check-command-bodies.mjs` (+ `.d.mts`, `tests/scripts/checkCommandBodies.test.ts`)
  fails on a command body over 40 lines or a direct database query (any member access on `db` other than
  `clone()`/`as_ref()`, or `lock_conn!`); handing the database to a service (`&state.db`) is allowed.
  `ci.yml` runs it and `count-ipc-error-commands.mjs --min 100` in the frontend job.

**Commits:** e5d64f9 (code), plus the follow-up commit with this note.

**Verification**
- `node scripts/check-command-bodies.mjs` → `302 commands are thin adapters` (before: 74 violations)
- `node scripts/count-ipc-error-commands.mjs --min 100` → `IpcError commands: 302/302 (100.0%)` (before: 176/302)
- `grep -rn _test_hook src-tauri` → no matches
- `cargo fmt --check` OK; `cargo clippy --all-targets -- -D warnings` OK
- `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast` → 3052 passed, 0 failed (lib: 2820 passed)
- `pnpm typecheck` OK; `pnpm lint` OK; `pnpm format:check` OK; `pnpm test:unit` → 166 files / 1576 tests passed
- `ci.yml` parses with PyYAML (actionlint not installed in this environment)

