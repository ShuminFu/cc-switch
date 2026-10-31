# CCS-149: [apps] Migrate OpenCode, OpenClaw and Hermes to AppAdapter and remove the remaining fan-out

**Status:** Done · **Kind:** refactor · **Labels:** `refactor`, `phase:P4`, `p2-medium`, `area:apps`

> **Ticket key:** `p4-adapter-additive` · **Phase:** P4 · **Priority:** p2-medium · **Effort:** L · **Depends on:** `p4-adapter-claude`, `p4-adapter-codex`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Proposed change
- Add apps/opencode (including OMO and OMO-Slim), apps/openclaw and apps/hermes.
- Replace the 7 get_<app>_override_dir functions with settings::override_dir.
- Remove the remaining match-app sites in services/provider/mod.rs.

## Acceptance criteria / verification
- [x] live.rs, prompt_files.rs, skill.rs and commands/config.rs have no match app_type
- [x] Backend match-app sites outside apps/** number 15 or fewer (CI count). Met by CCS-236: the counter reports **4** sites (`MATCH_APP_SITE_BUDGET = 4`; was 40 after this ticket, 56 before it).
- [x] Writer snapshots are identical

## Dependencies
p4-adapter-claude, p4-adapter-codex

## Implementation notes

**Summary.** Added `apps/opencode` (`OpenCodeAdapter`), `apps/openclaw` (`OpenClawAdapter`) and `apps/hermes` (`HermesAdapter`). All 8 apps now have an adapter, so `apps::adapter()` returns `&'static dyn AppAdapter` instead of an `Option`. Its match is exhaustive, and every `if let Some(adapter)` / "not migrated yet" fallback is gone.

- Moved files: `opencode_config.rs` became `apps/opencode/config.rs`, `openclaw_config.rs` became `apps/openclaw/config.rs`, and `hermes_config.rs` became `apps/hermes/config.rs`. `lib.rs` keeps `crate::opencode_config` and `crate::openclaw_config` as `use` aliases, and keeps `crate::hermes_config` as a `pub use` alias for the integration tests. The only edits inside the moved files swap the override getter for `settings::override_dir`.
- What moved into the adapters:
  - live read (each app's localized "config missing" error);
  - the additive write. The OpenCode full-config fragment extraction and the typed/raw fallbacks are kept verbatim, as is the OpenClaw typed/raw fallback;
  - `remove_provider`, which skips when the config dir is missing;
  - validation, credentials and paths (config dir, prompt file, skills dir, session roots);
  - MCP projection for OpenCode and Hermes. OpenClaw has none (Issue #4834);
  - the OpenCode/OpenClaw common-config extract, through the new `ExtractOnlySnippet`. `contains` is always false and `apply`/`remove` return the input unchanged, which is what the old fallbacks did. Hermes still has no snippet.
- New `AppAdapter` methods:
  - `live_has_provider`: replaces the `provider_exists_in_live_config` match.
  - `apply_switch_defaults`: the Hermes `model:` update after a switch. The warning key is `"{app}_model_defaults_failed:{id}"`, which is still `hermes_model_defaults_failed:…`.
  - `omo_variant(category)`: OpenCode returns the OMO / OMO Slim variant. `OmoVariant::for_category` and `OmoVariant::counterpart` were added to `services/omo.rs`.
  - `usage_credentials(settings)`: replaces the per-app match in `Provider::resolve_usage_credentials` (the plan's provider.rs:147-200). The lenient JS-parity semantics are unchanged.
- `services/provider/mod.rs` no longer names any `AppType::` variant in production code. The `matches!(app_type, AppType::OpenCode)` OMO checks in add, rename, update, delete, switch and switch_normal, plus `remove_from_live_config`, the live rollback after a switch and the extract/validate/credentials matches, all go through the adapter. In `live.rs`, `write_live_snapshot` and the three `remove_*_provider_from_live` helpers were deleted, and `read_live_settings` / `provider_exists_in_live_config` became one-line adapter calls. The `import_*_providers_from_live` functions stay where they are because they are not per-app dispatch.
- `services/mcp.rs` (upsert/remove), `services/skill.rs::get_app_skills_dir`, `prompt_files.rs`, `commands/config.rs::app_config_dir` and `session_manager::provider_roots` are now single adapter calls.
- `settings::get_codex_override_dir`, `get_opencode_override_dir`, `get_openclaw_override_dir` and `get_hermes_override_dir` were removed. All 7 per-app getters are now replaced by `settings::override_dir(app)`. Callers updated: `apps/codex/config.rs`, the three moved config modules, and the Windows-only `commands/misc.rs::wsl_distro_for_tool`.
- `services/proxy.rs`: one call site was adapted to the non-Option `adapter()`, and one stale comment was fixed. The takeover code was not restructured. `src/proxy/**` is untouched.

**Match-app count (CI).** `apps::tests::match_app_sites_outside_apps_stay_within_budget` walks `src/` at test time, skipping `apps/**`, `#[cfg(test)]` modules and comments. It counts each `match` whose first arm starts with `AppType::`. It asserts two things:
1. `services/provider/{live,mod}.rs`, `prompt_files.rs`, `services/skill.rs` and `commands/config.rs` have 0 such sites. Before this change they had 3, 6, 1, 2 and 1, so the test failed on the pre-change tree (checked by running the same counter over `git archive HEAD`).
2. The total is at most `MATCH_APP_SITE_BUDGET = 40`. It was 56 before this change.

**What remains for "15 or fewer".** The remaining 40 sites are outside this ticket's scope:
- `services/proxy.rs` (13): the takeover backup, restore, cleanup and detect paths. This is CCS-153 / CCS-154, the takeover engine split.
- `app_config.rs` (10) and `settings.rs` (4): `McpApps` / `SkillApps` flag accessors, prompt maps, override and current-provider fields. This is CCS-155, per-app settings state.
- `services/profile.rs` (3), `deeplink/preview.rs` (2), `deeplink/provider.rs` (1), `proxy/providers/mod.rs` (2, lane-2 territory), `proxy/usage/cost_golden.rs` (1), `services/config.rs` (1, the legacy `MultiAppConfig` sync), `services/stream_check.rs` (1), `paths.rs` (1) and `commands/misc.rs` (1).

The budget constant should be lowered as each of those tickets lands.

**Writer snapshots.** `tests/config_writers.rs` passes with no snapshot changes (18 passed), so the output is byte-identical. That includes the OpenCode/OpenClaw/Hermes writer snapshots and `hermes_roundtrip`.

**Files.** New: `src-tauri/src/apps/{opencode,openclaw,hermes}/mod.rs`. Moved: `apps/{opencode,openclaw,hermes}/config.rs`. Edited:
- `apps/{adapter.rs,mod.rs}` and `apps/{claude,claude_desktop,codex,gemini,grok}/mod.rs` (`usage_credentials`)
- `apps/codex/config.rs`, `lib.rs`, `settings.rs` and `provider.rs`
- `services/provider/{live.rs,mod.rs}`, `services/{mcp,omo,proxy,skill}.rs`
- `prompt_files.rs`, `commands/{config,misc}.rs` and `session_manager/mod.rs`

**Commits.** `a85ca8e` refactor(apps): migrate OpenCode, OpenClaw and Hermes to AppAdapter (CCS-149), plus this ticket update.

**Tests.**
- `apps::tests`:
  - `adapter_backed_apps_have_no_arms_in_provider_service` now covers all 8 apps.
  - `migrated_apps_have_adapters` was replaced by `adapter_capabilities_match_registry`. For each app it checks the prompt file against `PROMPTS`, the MCP projector against `MCP`, and takeover against `PROXY_TAKEOVER`. For additive apps it also checks that `import_live` errors.
  - `adapter_paths_match_legacy_layout` was extended to OpenCode, OpenClaw and Hermes. The skills defaults equal the old hard-coded paths, and the session roots equal the old `provider_roots` values.
  - New: `match_app_sites_outside_apps_stay_within_budget` and `usage_credentials_follow_each_app_shape`.
- `apps::opencode::tests` (credentials, OMO variant/counterpart, extract-only snippet), `apps::openclaw::tests` and `apps::hermes::tests`.
- The existing additive/OMO tests in `services/provider/mod.rs` (switch, rename, update, delete, import, `live_config_managed` rollback) pass unchanged.

**Verification.**
- `cargo fmt --manifest-path src-tauri/Cargo.toml`: clean (`--check` OK).
- `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`: clean.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib apps::`: 267 passed.
- `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast`: all green.
  - lib: 2353 passed, 0 failed, 2 ignored.
  - config_writers 18, hermes_roundtrip 2, provider_service 33, provider_commands 10, mcp_commands 20, import_export_sync 26, skill_sync 8, proxy_e2e 19, proxy_golden 6, migrations 9 and write_lint 3: all passed. Every other integration binary also passed.
- Not compiled locally (Linux runner): the Windows-only `wsl_distro_for_tool` edit. It is a mechanical swap to `settings::override_dir(&AppType::…)`.

**Follow-ups for other lanes.**
- `src/config/hermesProviderSource.ts` has a doc comment that still says `hermes_config.rs::get_providers`; it should now be `apps/hermes/config.rs`. That file is in the frontend lane, so I did not edit it.

### Remaining after triage

- Blocked: Removing the 2 sites in takeover/legacy.rs. Time-gated by CCS-154 (legacy engine deleted one release after it ships); not needed to reach ≤ 15.
- Follow-up [CCS-221](../P0-safety-net/CCS-221-p0-docs-ticket-evidence-refresh.md): Tick already-verified acceptance boxes and refresh stale evidence in Partial tickets.
- Follow-up [CCS-232](../P4-app-adapters-persistence/CCS-232-p4-apps-budget-ratchet.md): Make the match-app counter skip test-only modules and ratchet both app budgets to today's counts.
- Follow-up [CCS-233](../P4-app-adapters-persistence/CCS-233-p4-apps-ninth-app-six-files.md): Make a 9th app touch 6 or fewer backend files.
- Follow-up [CCS-234](../P4-app-adapters-persistence/CCS-234-p4-takeover-dispatch-via-adapter.md): Route takeover per-app dispatch through ProxyTakeover instead of AppType matches.
- Follow-up [CCS-235](../P4-app-adapters-persistence/CCS-235-p4-apps-config-profile-keyed-access.md): Replace per-app field matches in app_config.rs and services/profile.rs with keyed access.
- Follow-up [CCS-236](../P4-app-adapters-persistence/CCS-236-p4-apps-match-sites-under-15.md): Move the remaining stream-check, terminal and proxy-provider dispatch into adapters and set the match-app budget to 15 or fewer.
