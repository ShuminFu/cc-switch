# CCS-163: [proxy] Support proxy takeover, failover and usage for OpenCode, OpenClaw and Hermes

**Status:** Done · **Kind:** feature · **Labels:** `feature`, `phase:P4`, `p2-medium`, `area:proxy`

> **Ticket key:** `p4-takeover-additive-apps` · **Phase:** P4 · **Priority:** p2-medium · **Effort:** M · **Depends on:** `p4-takeover-engine`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Wire routing already exists for these apps, but takeover is hard-coded false for opencode and openclaw (services/proxy.rs:714-716), and the status has no hermes field.

## Proposed change
Implement ProxyTakeover for the additive adapters, rewriting only the selected entry's baseUrl, and include them in failover and usage.

## Acceptance criteria / verification
- [ ] Crash/restore E2E is green for opencode, openclaw and hermes
- [ ] Conformance takeover checks pass
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml takeover` and `pnpm test:unit`

## Dependencies
p4-takeover-engine

## Implementation notes

**Summary.**
- **Registry.** OpenCode, OpenClaw and Hermes now have `PROXY_TAKEOVER` and `USAGE`, with the proxy routes `/opencode`, `/openclaw` and `/hermes`. `get_app_registry`, `tests/fixtures/app-registry.json` and the frontend registry (`src/entities/app/registry.ts`) report `proxyTakeover: true` for them, so the header proxy switch and the palette action appear. `ProxyTakeoverStatus` already had a key for each app; it now reports their real state.
- **Wire routing.** `proxy/endpoints.rs` mounts `/<app>/v1/chat/completions` and `/<app>/v1/responses` for each additive app, with its own provider namespace and failover queue. Upstream calls use the new `proxy/providers/additive.rs` (`AdditiveAppAdapter`). Before this, these apps fell back to `CodexAdapter`, which could not read their entry shapes. The adapter takes `base_url` and the key from `AppAdapter::usage_credentials`, then builds the URL and Bearer auth the same way `CodexAdapter` does.
- **Adapter side.** `ProxyTakeover::entry_takeover()` returns a new `EntryTakeover` sub-trait (`apps/adapter.rs`), implemented by the three additive adapters. It covers listing entries, finding the default entry (OpenCode `model`, OpenClaw `agents.defaults.model(.primary)`, Hermes `model.provider`), reading an endpoint, checking the protocol and writing an endpoint. Writing an endpoint only changes `baseURL`/`apiKey`, `baseUrl`/`apiKey` or `base_url`/`api_key` of that one entry, through the new `set_provider_endpoint` in each `config.rs`. `has_placeholder` accepts either a whole live config or a single entry.
- **Takeover engine** (`services/proxy/takeover/additive.rs`).
  - **Selected entry.** Chosen in this order: the default-model reference, then the effective current provider, then the only entry.
  - **Requirements.** The entry must be a CC Switch provider with a `base_url`, and it must speak OpenAI-compatible Chat Completions or Responses.
  - **Rewrite.** Only that entry's base URL is rewritten, to `http://host:port/<app>/v1`. Its key becomes the install token.
  - **Routing.** The entry is recorded as the app's current provider, which the router uses when failover is off.
  - **Modes.** `Strict` (the per-app switch) returns `Provider` or `Projection` errors with the reason. `BestEffort` (`start_with_takeover`) skips the app.
  - **Legacy engine.** `takeoverEngine = "legacy"` still uses the unified path for these apps, because the legacy variants never supported them.
- **Backup and restore.**
  - **Backup.** It stays the whole pre-takeover live config, plus a raw snapshot of `opencode.json`, `openclaw.json` or `config.yaml` (`takeover_live_files`).
  - **Restore.** `write_live_config_for_app` only writes back endpoints of entries that carry the proxy token. Edits made to other entries during takeover are therefore kept.
  - **Byte-exact replay.** The raw snapshot is replayed only when the restored live config equals the backup.
  - **No backup.** Without a backup, the fallback works per entry: it takes the endpoint from the provider with the same id, or strips the token and the local URL. It does not use the app-wide SSOT path, because the current provider may have been hot-switched.
  - **Hot switch.** It only changes the proxy target; the backup is left as it is.
- **Saves during takeover.** `write_live_with_common_config` goes through `write_entry_respecting_takeover`. When the provider being saved is the taken-over entry, its entry is written, the live config is recorded as the new backup, and the entry is pointed back at the proxy. This covers edits, key rotation and sync after import.
- **DB fix.** `update_proxy_config_for_app` now ensures the row exists. The seed rows only cover claude, codex, gemini and grokbuild, so before this fix the `UPDATE` matched no row and the additive apps' `enabled` flag was silently lost.
- **Not supported.** Entries that speak other protocols cannot be taken over, and the per-app switch reports this. These are OpenCode `@ai-sdk/anthropic` / `google`, OpenClaw `anthropic-messages` and similar, and Hermes `anthropic_messages` / `bedrock_converse`. Hermes `providers:` dict entries are also excluded: Hermes manages them and CC Switch does not write them. Hermes' top-level `model.base_url` is left untouched, as the ticket asks to rewrite only the selected entry.

**Files.**
- Proxy services: `src-tauri/src/services/proxy/takeover/additive.rs` (new), `takeover/{mod,engine}.rs`, `services/proxy/{mod,backup,tests}.rs`, `services/takeover_snapshot.rs`, `services/provider/live.rs` (one hook).
- Proxy: `src-tauri/src/proxy/providers/additive.rs` (new), `proxy/providers/mod.rs`, `proxy/endpoints.rs`, `proxy/types.rs` (tests).
- Adapters: `src-tauri/src/apps/{adapter.rs,registry.rs}`, `apps/{opencode,openclaw,hermes}/{mod,config}.rs`.
- DB: `src-tauri/src/database/dao/proxy.rs`.
- Test support: `src-tauri/src/{proxy_harness,live_config_testkit}.rs`.
- Tests: `src-tauri/tests/{takeover_crash,proxy_e2e}.rs`.
- Frontend: `src/entities/app/registry.ts`, `tests/fixtures/app-registry.json`, `tests/{entities/appRegistry,components/CapabilityMatrix,integration/App}.test.tsx`.
- Not touched: `commands/**`, `lib.rs`, `tray.rs`.

**Commits.** `f80fdfd` feat(proxy): take over the selected entry of OpenCode, OpenClaw and Hermes (CCS-163). This ticket record follows as a docs commit.

**Tests added or changed.**
- `takeover_crash.rs`:
  - opencode, openclaw and hermes cases, hand-written with comments, JSON5 and YAML, each with a second `hand-only` entry. They run through every existing crash/restore test: byte-for-byte for hand-written and CC Switch-written files, both entry points, and the legacy variants.
  - `additive_takeover_rewrites_only_the_selected_entry`: selected entry → `/<app>/v1` plus install token, other entry unchanged, current provider set.
  - `additive_crash_recovery_keeps_edits_to_other_entries`.
  - `additive_provider_save_during_takeover_keeps_the_proxy_endpoint`: regression test. It fails when the save hook is disabled.
- `app_adapter_conformance`: `takeover_apply_then_cleanup_restores_original_bytes` and `caps_match_implemented_traits_for_every_app` now run for all 7 takeover apps. No test change was needed.
- `proxy_e2e.rs`:
  - `additive_app_routes_fail_over_and_log_usage_per_app`: each of the 3 apps, 503 → failover, the upstream gets the provider key, and the usage row has `app_type` = the app with its tokens.
  - `additive_app_without_failover_uses_the_taken_over_provider`.
  - The harness gained `app_provider` / `openai`.
- Lib:
  - `proxy_config_rows_are_created_for_apps_that_gained_takeover`: regression test, fails without the DAO fix.
  - `additive_takeover_reports_why_an_entry_cannot_be_taken_over`.
  - Per-adapter `entry_takeover_*` tests and `proxy::providers::additive` tests.
  - Updated registry, status and base-URL expectations.

**Verification (Linux).**
- `cargo fmt`: clean. `cargo clippy --all-targets -- -D warnings`: clean.
- `cargo test --manifest-path src-tauri/Cargo.toml takeover`: lib 49 passed; `takeover_crash` filter 5 passed; conformance 1 passed; the other filters passed.
- `cargo test --test takeover_crash`: 8 passed.
- `cargo test --test app_adapter_conformance`: 13 passed.
- `cargo test --test proxy_e2e additive`: 2 passed.
- `cargo test --no-fail-fast`: 3032 passed, 0 failed.
- `pnpm typecheck`: clean. `pnpm test:unit`: 163 files, 1567 tests passed. `pnpm format:check`: clean.

**Remaining / limitations.**
- Only verified on Linux. The CI matrix runs the same suites on Windows and macOS.
- Taking over Anthropic-protocol entries would need `/<app>/v1/messages` routes and Anthropic upstream auth for these apps. That is out of scope here.
