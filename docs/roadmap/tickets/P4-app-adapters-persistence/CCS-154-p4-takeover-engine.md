# CCS-154: [proxy] Unify takeover behind takeover(app, Strict|BestEffort) with TakeoverError and a registry-driven status

**Status:** Done · **Kind:** refactor · **Labels:** `refactor`, `phase:P4`, `p2-medium`, `area:proxy`

> **Ticket key:** `p4-takeover-engine` · **Phase:** P4 · **Priority:** p2-medium · **Effort:** L · **Depends on:** `p4-services-proxy-split`, `p4-adapter-additive`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
There are 3 takeover variants x 4 apps, and the base URL is built in 5 places. The critique notes ProxyTakeoverStatus has no hermes field at all; only opencode and openclaw are hard-coded false.

## Evidence
- services/proxy.rs:1528-1712, 714-716; proxy/types.rs:112-119

## Proposed change
- Use a ProxyTakeover trait on the adapters.
- Turn backup, restore and cleanup into loops.
- Take the base URL from descriptor.proxy_route.
- Add a TakeoverError enum.
- Generate ProxyTakeoverStatus from the registry as a map, keeping the existing serde field names for compatibility.

## Acceptance criteria / verification
- [ ] Crash/restore E2E is green
- [ ] The status JSON is backward compatible (snapshot)
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml takeover` and `pnpm typecheck`

## Dependencies
p4-services-proxy-split, p4-adapter-additive

## Implementation notes

**Summary.**
- **Engine.** `services/proxy/takeover/engine.rs` adds `ProxyService::takeover(app, TakeoverMode::{Strict, BestEffort})`, plus `takeover_all()` for `start_with_takeover`. These replace `takeover_live_configs`, `takeover_live_config_strict` and `takeover_live_config_best_effort`. Per app, only two steps differ: projecting the takeover fields (`project_takeover_live`) and writing the live config (`write_takeover_live`).
  - `Strict`: the live config must exist and be readable, and Claude and Codex need a current provider.
  - `BestEffort`: an app whose live config is missing or unreadable is skipped. Claude without a current provider falls back to the provider-less projection (`PreserveExistingOrAuthToken`).
  - In both modes, projection and write errors are returned. The old best-effort path swallowed write errors for Claude, Gemini and Grok.
- **`TakeoverError`.** Variants: `Unsupported`, `ProxyAddress`, `LiveUnavailable`, `Provider`, `Projection`, `Write` and `Legacy`, built with `thiserror`. `Display` gives the same error strings as before, and `From<TakeoverError> for String` keeps the `String`-returning callers unchanged.
- **Base URL from the registry.** `ProxyService::takeover_base_url(app)` combines `proxy_origin()` (formerly `build_proxy_urls`) with `descriptor.proxy_route` through the new `ProxyTakeover::base_url(origin, route)`.
  - The default is `{origin}{route}/v1`, which Grok Build uses (`/grokbuild/v1`).
  - Claude, Codex and Gemini override it to keep the URLs already written into users' live files:
    - Claude: bare origin.
    - Codex: `/v1`, because the CLI probes `/v1/models` and there is no `/codex/v1/models`.
    - Gemini: bare origin.
  - The five hand-built copies are gone: three takeover variants, `live_takeover_matches_current_proxy` and the Codex/Grok/Claude hot-switch syncs.
- **Loops.** These now loop over the `PROXY_TAKEOVER` apps instead of fanning out per app: `backup_live_configs`, `backup_live_config_strict`, `restore_live_config_for_app_inner`, `detect_takeover_in_live_configs`, `detect_takeover_in_live_config_for_app`, `live_has_proxy_placeholder_for_app` (now the adapter's `has_placeholder`), `live_takeover_matches_current_proxy` (generic, with a per-app `live_base_url_matches`), the live refresh in `update_config` after a restart, and `is_takeover_active`. The `is_*_live_taken_over` wrappers were removed.
- **Status.** `ProxyTakeoverStatus` (`proxy/types.rs`) is now generated from the registry. It is a `slug -> bool` map serialized in `APPS` order, with the same keys as the old struct (`claude`, `codex`, `gemini`, `grokbuild`, `opencode`, `openclaw`) plus `claude-desktop` and `hermes`. The TS `ProxyTakeoverStatus` already declared both. Apps without `PROXY_TAKEOVER` are always `false`. Accessors: `get(&AppType)`, `any()`, `enabled_apps()`. Deserialization accepts legacy payloads. `commands/proxy.rs::stop_proxy_server` changed by one line, to `takeover.any()`.
- **Fallback flag.** `takeoverEngine = "legacy"` (`TakeoverEngine::uses_unified_takeover()`) dispatches `takeover` and `takeover_all` to the previous three variants. They are kept verbatim in `takeover/legacy.rs` for one release, except that they get their URLs from `takeover_base_url`.

**Files.**
- Proxy services: `src-tauri/src/services/proxy/takeover/{engine.rs,legacy.rs}` (new), `takeover/{mod,claude,codex,gemini,grok}.rs`, `services/proxy/{mod,backup,config,hot_switch,lifecycle,tests}.rs`.
- Shared: `src-tauri/src/services/takeover_snapshot.rs`, `src-tauri/src/proxy/types.rs`.
- Adapters: `src-tauri/src/apps/{adapter.rs,claude/mod.rs,codex/mod.rs,gemini/mod.rs}`.
- Commands: `src-tauri/src/commands/proxy.rs` (one line).

**Commits.** `49e08ba` refactor(proxy): unify takeover behind takeover(app, Strict|BestEffort) (CCS-154). This ticket record is a follow-up docs commit.

**Tests added.**
- `proxy::types::tests::takeover_status_json_snapshot_is_backward_compatible`: an exact JSON snapshot, plus a check that every legacy key is present.
- `takeover_status_never_reports_apps_without_takeover_capability`.
- `takeover_status_deserializes_legacy_payloads`.
- `services::proxy::tests::takeover_status_is_generated_from_the_registry`: regression test. Before this change, the status had no `hermes` or `claude-desktop` key.
- `takeover_base_url_comes_from_the_registry_and_keeps_existing_live_urls`: checks the exact legacy URLs, and that the 4 non-takeover apps are `Unsupported`.
- `unified_takeover_writes_the_same_live_files_as_the_legacy_engine`: 4 apps × both modes, with byte-identical live files across the two engines.
- `strict_takeover_requires_live_config_and_best_effort_skips_it`.
- `strict_takeover_reports_a_missing_current_provider`.
- The line-budget test now includes `engine.rs` and `legacy.rs`.

**Verification (Linux).**
- `cargo fmt`: clean. `cargo clippy --all-targets -- -D warnings`: clean.
- `cargo test --manifest-path src-tauri/Cargo.toml takeover`: lib 44 passed; `takeover_crash` 3 matched; conformance, profile, provider_commands and provider_service filters all passed.
- `cargo test --test takeover_crash`: 5 passed. This is the crash/restore E2E for unified and legacy, for all 4 apps and both entry points.
- `cargo test --no-fail-fast`: 3020 passed, 0 failed (lib 2783 passed, 3 ignored; `proxy_e2e` 32 passed).
- `pnpm typecheck`: clean. `pnpm test:unit`: 163 files, 1567 tests passed. No frontend file changed.

**Notes.**
- Behaviour change: in `start_with_takeover` and the port-change refresh, Claude without a current provider no longer aborts takeover. It is projected with the existing-credentials policy, as the old best-effort path already did. In the refresh, write errors are now reported instead of swallowed.
- Not done here: moving the per-app projection and write steps fully into the adapters. They depend on `ProxyService` state (the provider DB, `bind_proxy_token`). They are now the only per-app `match` arms left in takeover.
