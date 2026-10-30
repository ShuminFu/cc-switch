# CCS-146: [apps] Add the AppAdapter trait and migrate Gemini and Grok

**Status:** Done · **Kind:** refactor · **Labels:** `refactor`, `phase:P4`, `p1-high`, `area:apps`

> **Ticket key:** `p4-adapter-trait-gemini-grok` · **Phase:** P4 · **Priority:** p1-high · **Effort:** L · **Depends on:** `p2-app-registry`, `p1-route-writes-atomic-lint`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
About 74 match-app sites spread per-app logic across live.rs, prompt_files.rs, skill.rs, the commands and services/provider/mod.rs (3986 lines, which the critique notes the plan missed).

## Evidence
- live.rs:1016-1170 write_live_snapshot, :1309, :1427, :485-665; services/provider/mod.rs

## Proposed change
- Add apps/adapter.rs: trait AppAdapter (config_dir, prompt_file, skills_dir, read_live, write_provider, remove_provider, import_live, credentials, common_snippet, mcp(), takeover(), session_source()).
- Add apps/gemini/ and apps/grok/, with code moved from *_config.rs.
- Add settings::override_dir(app).

## Acceptance criteria / verification
- [x] Gemini and Grok have no arms in live.rs or services/provider/mod.rs
- [x] Writer snapshots are byte-identical
- [x] `cargo test --manifest-path src-tauri/Cargo.toml --test config_writers`

## Dependencies
p2-app-registry, p1-route-writes-atomic-lint

## Implementation notes

**Summary.** Added `src-tauri/src/apps/adapter.rs` with the `AppAdapter` trait (`config_dir`, `prompt_file`, `skills_dir`, `read_live`, `import_live`, `write_provider`, `remove_provider` (no-op default for switch-mode apps), `validate_provider`, `backfill_live_settings`, `credentials`, `common_snippet()`, `mcp()`, `takeover()`, `session_source()`). The optional capabilities are sub-traits: `CommonSnippet`, `McpProjector`, `ProxyTakeover` and `SessionSource`. `apps::adapter(app) -> Option<&'static dyn AppAdapter>` returns `GeminiAdapter` / `GrokAdapter` and `None` for apps not migrated yet. The match is exhaustive, so a new `AppType` has to choose a branch.

- `gemini_config.rs` became `apps/gemini/config.rs`, `services/provider/gemini_auth.rs` became `apps/gemini/auth.rs`, and `grok_config.rs` became `apps/grok/config.rs`. `lib.rs` keeps `crate::gemini_config` / `crate::grok_config` as `use` aliases, so other callers (including `src/proxy/**`, which lane 2 owns) are unchanged.
- `write_gemini_live`, the Gemini read/import blocks, Gemini common-config extract/contains/apply/remove, Gemini/Grok credentials and validation, and Grok backfill MCP stripping all moved into the adapters. The JSON subset/merge/remove helpers moved to `apps/json_snippet.rs` and are shared with live.rs.
- `live.rs` and `services/provider/mod.rs` dispatch through `apps::adapter()` before their remaining per-app matches. The never-constructed `LiveSnapshot::Gemini` variant was removed. `ProviderService::write_gemini_live` (still used by `services/proxy.rs` and `services/config.rs`) delegates to `apps::gemini::write_live`.
- Other callers now use the adapter too: `prompt_files.rs` (prompt_file), `services/skill.rs` (skills_dir), `services/mcp.rs` (single-server upsert/remove via `mcp()`), `session_manager::provider_roots` (session_source, canonical slug only) and `services/proxy.rs` Gemini/Grok placeholder checks (`takeover().has_placeholder`). The `services/proxy.rs` changes are limited to the bodies of two private helpers.
- `settings::override_dir(app)` was added. `get_gemini_override_dir` / `get_grok_override_dir` were removed, and their callers (configs and the Windows-only `commands/misc.rs::wsl_distro_for_tool`) now use it. The other five getters stay until their apps migrate (CCS-147..149).

**Files.** New: `src-tauri/src/apps/{adapter.rs,json_snippet.rs,gemini/mod.rs,grok/mod.rs}`. Moved: `apps/gemini/{config.rs,auth.rs}` and `apps/grok/config.rs`. Edited: `apps/mod.rs`, `lib.rs`, `settings.rs`, `services/provider/{live.rs,mod.rs}`, `prompt_files.rs`, `services/{skill.rs,mcp.rs,proxy.rs}`, `session_manager/mod.rs`, `commands/misc.rs`.

**Commits.** `89e0bdd` refactor(apps): add AppAdapter trait and migrate Gemini and Grok (CCS-146), plus this ticket update.

**Tests added** (`apps::tests`):
- `adapter_backed_apps_have_no_arms_in_provider_service` is a regression guard. It scans the production code (test modules excluded) of live.rs and services/provider/mod.rs for `AppType::<migrated app>`. On the pre-change sources it finds 13 hits in live.rs and 8 in mod.rs; it now finds 0.
- `only_gemini_and_grok_have_adapters`
- `adapter_paths_match_legacy_layout`
- `gemini_credentials_default_base_url`
- `grok_credentials_come_from_config_toml`
- `gemini_common_snippet_round_trips_env`
- `takeover_placeholder_detection`

**Verification.**
- `cargo fmt --manifest-path src-tauri/Cargo.toml`: clean.
- `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`: clean.
- `cargo test --manifest-path src-tauri/Cargo.toml --test config_writers`: 17 passed. The writer snapshots are unchanged, so the output is byte-identical.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib apps::`: 50 passed.
- `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast`: all green. The lib suite had 2330 passed, 0 failed, 2 ignored. All integration test binaries passed, including write_lint, provider_service, mcp_commands and proxy_commands.
- The Windows-only `wsl_distro_for_tool` edit was not compiled locally (Linux runner). It is a mechanical swap to `settings::override_dir(&AppType::…)`.
