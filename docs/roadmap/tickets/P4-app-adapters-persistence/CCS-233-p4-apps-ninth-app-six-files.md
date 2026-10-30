# CCS-233: [apps] Make a 9th app touch 6 or fewer backend files

**Status:** Done · **Kind:** refactor · **Labels:** `refactor`, `phase:P4`, `p2-medium`, `area:apps`

> **Ticket key:** `p4-apps-ninth-app-six-files` · **Phase:** P4 · **Priority:** p2-medium · **Effort:** M · **Depends on:** `p4-apps-budget-ratchet`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)
> Found during the Partial-ticket triage at d962272 (CCS-150, CCS-005, CCS-149). Source tickets: [CCS-150](../P4-app-adapters-persistence/CCS-150-p4-adapter-conformance.md), [CCS-005](../P4-app-adapters-persistence/CCS-005-epic-p4.md), [CCS-149](../P4-app-adapters-persistence/CCS-149-p4-adapter-additive.md).

## Problem
The CCS-150 / P4 criterion "dummy app touches ≤ 6 backend files" measures 9: app_config.rs (4 exhaustive matches), apps/registry.rs, apps/mod.rs, the new apps/<app>/mod.rs, plus deeplink/preview.rs (2), deeplink/provider.rs (1), paths.rs (1), proxy/providers/mod.rs (2), services/config.rs (1). The 4 fixed files are always touched, so at least 3 of the other 5 must stop matching exhaustively on `AppType`.

## Proposed change
- `src-tauri/src/paths.rs:88-98` `default_app_dir`: add a home-relative default dir (plus a special-case fn for ClaudeDesktop/Hermes) to the registry descriptor or an adapter hook, and dispatch through `APPS`/`apps::adapter(app)` without a cycle (adapters use paths). `legacy_default_app_dir` (~192) can keep the known variants plus `_ => None`; confirm whether it is test-only.
- `src-tauri/src/services/config.rs:122-140`: additive apps and ClaudeDesktop return early by `SyncMode`; switch apps call `adapter.write_provider(&provider)` or a new `sync_live_from_config` hook for the Codex/Claude/Gemini specifics (or keep explicit arms with a `_ =>` catch-all).
- `src-tauri/src/deeplink/preview.rs:342` `provider_live_files` and `:362` `mcp_live_file`: add `AppAdapter::live_files()` and use `mcp().map(|m| m.live_file())`.
- `src-tauri/src/deeplink/provider.rs:172-180`: add `AppAdapter::deeplink_settings(request)`, or a `_ => build_additive_app_settings(request)` arm.
- `proxy/providers/mod.rs` is handled in CCS-236 if these four files are not enough.
- Lower `NEW_APP_BACKEND_FILE_BUDGET` to the measured value (target 6).

## Acceptance criteria / verification
- [x] `cargo test --manifest-path src-tauri/Cargo.toml --test app_adapter_conformance new_app_backend_touch_points -- --nocapture` prints ≤ 6 files; the budget equals the measured count.
- [x] The full conformance suite (13 tests), `cargo test --lib deeplink`, `cargo test --lib paths`, `cargo test --lib services::config` and `cargo test --test config_writers` pass; `adapter_paths_match_legacy_layout` (apps/mod.rs:231) passes.
- [x] `cargo clippy --all-targets -- -D warnings` and `cargo fmt --check` are clean.
- [x] When ≤ 6: tick CCS-150's criterion, set it Done, update README.md and the CCS-005 epic row.

## Dependencies
CCS-232 (`p4-apps-budget-ratchet`).

## Implementation notes

**Summary.** A new `AppType` now touches **5** backend files, down from 9: `app_config.rs` (4 exhaustive matches), `apps/registry.rs`, `apps/mod.rs`, the new `apps/<app>/mod.rs`, and `proxy/providers/mod.rs` (2, left for CCS-236). Four files no longer match exhaustively on `AppType`:
- `paths.rs`: `AppDescriptor` has a new `default_dir: fn(&Path) -> Option<PathBuf>` field. `Paths::default_app_dir` calls `(app.descriptor().default_dir)(&home)`. The Claude Desktop and Hermes platform special cases stay in `paths.rs` as `pub(crate)` functions, and the registry entries reference them. The registry only holds function pointers, so there is no module cycle. `legacy_default_app_dir` is test-only (inside `mod tests`) and is unchanged. `production_default_paths_match_dirs_home_dir_for_every_app` still compares every app against it.
- `deeplink/preview.rs`: `provider_live_files` and `mcp_live_file` are removed. They are replaced by a required `AppAdapter::live_files()` and a required `McpProjector::live_file()`, which every adapter implements. The preview calls `apps::adapter(app).live_files()` and `adapter(app).mcp().map(|m| m.live_file())`. Behaviour change (a bug fix): the MCP preview used to list nothing for OpenClaw, although OpenClaw has projected MCP servers into `openclaw.json` since Issue #4834. It now lists `openclaw.json`.
- `deeplink/provider.rs`: the `OpenClaw` arm is folded into `_ => build_additive_app_settings(request)`.
- `services/config.rs`: Codex, Claude and Gemini keep their explicit sync arms, and Claude Desktop stays a no-op. Then `_ if app_type.is_additive_mode() => {}` and `_ => adapter(app_type).write_provider(&provider)?`. Grok Build reaches the same `write_grok_provider_live` through its adapter, and a future switch-mode app syncs through its adapter automatically.
- `NEW_APP_BACKEND_FILE_BUDGET` 9 -> 5, matching the measured count. `MATCH_APP_SITE_BUDGET` 28 -> 25: the paths and preview matches are gone. 24 fails with "25 match-app sites".

**Tests.**
- Conformance contract (`tests/app_adapter_conformance.rs`): `assert_credentials_round_trip` now checks that `write_provider` changes at least one file and only files listed in `live_files()`. `assert_mcp_upsert_and_remove` checks that upsert writes exactly `McpProjector::live_file()`. Both run for all 8 apps and for the dummy 9th app, which gained `live_files()`.
- Regression test `deeplink::tests::test_preview_mcp_lists_the_live_file_of_every_mcp_app`: an MCP link for `claude,openclaw` must list both live files, the second ending in `openclaw.json`. Before the change, `mcp_live_file(OpenClaw)` returned `None`, so only 1 file was listed and the test fails.

**Files.** `src-tauri/src/apps/{adapter.rs,registry.rs,mod.rs}`, `src-tauri/src/apps/{claude,claude_desktop,codex,gemini,grok,hermes,opencode,openclaw}/mod.rs`, `src-tauri/src/paths.rs`, `src-tauri/src/deeplink/{preview.rs,provider.rs,tests.rs}`, `src-tauri/src/services/config.rs`, `src-tauri/tests/app_adapter_conformance.rs`. The docs commit also updates CCS-150 (now Done), the CCS-005 epic and `docs/roadmap/tickets/README.md`.

**Commits.** `2b10c68` (code), plus the follow-up docs commit that records this note.

**Verification.**
- `cargo test --test app_adapter_conformance -- --nocapture`: 13 passed. The output reads `backend files a new app touches (5): app_config.rs (4 exhaustive match), apps/<new app>/mod.rs, apps/mod.rs (1 exhaustive match), apps/registry.rs, proxy/providers/mod.rs (2 exhaustive match)`.
- `cargo test --lib -- deeplink paths services::config apps::`: 355 passed. This includes `adapter_paths_match_legacy_layout` and the new preview test.
- `cargo test --test config_writers --test deeplink_import`: 19 + 4 passed.
- `cargo test --lib match_app_sites_outside_apps`: passes at 25 and fails at 24 (measured 25).
- `cargo clippy --all-targets -- -D warnings`: clean. `cargo fmt --check`: clean.
- Full `cargo test --no-fail-fast`: 3106 passed, 0 failed across all binaries.

