# CCS-150: [apps] Add an adapter conformance suite that loops over APPS, plus the dummy 9th-app test

**Status:** Partial · **Kind:** chore · **Labels:** `chore`, `phase:P4`, `p1-high`, `area:apps-tests`

> **Ticket key:** `p4-adapter-conformance` · **Phase:** P4 · **Priority:** p1-high · **Effort:** M · **Depends on:** `p4-adapter-additive`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Proposed change
Add tests/app_adapter_conformance.rs, which for each app checks:
- credentials round-trip
- MCP upsert and remove
- prompt and skills paths
- takeover apply then cleanup restores the original bytes
- caps match the implemented traits
- the codex model_provider id is stable

Also add a test-only 9th app.

## Acceptance criteria / verification
- [x] Passes for all 8 apps
- [ ] The dummy app touches 6 or fewer backend files and needs no migration (documented in the PR). No migration: done. Files: it is 10 today, now tracked by a CI count; see below.
- [x] `cargo test --manifest-path src-tauri/Cargo.toml --test app_adapter_conformance`

## Dependencies
p4-adapter-additive

## Implementation notes

**Summary.** Added `src-tauri/tests/app_adapter_conformance.rs`. It builds one `Case` per `APPS` entry: the registry caps, the sync mode, `apps::adapter(app)`, and a per-slug fixture with a known key and base URL. A registered app without a fixture fails the suite. The same contract functions then run for every app:

- **Credentials round-trip:** `validate_provider`, then `credentials()` and `usage_credentials()` must return the fixture key and URL. Next, `write_provider`, then `read_live`, then read back this app's entry, and `credentials()` must still match. For additive apps, `live_has_provider` and then `remove_provider` must work. For switch-mode apps, remove must be a no-op. Claude Desktop has no generic live read, so it goes through the 3P profile writer instead (`claude_desktop_profile_carries_the_credentials`).
- **MCP upsert and remove:** upsert, re-upsert with a changed spec, remove, then remove again. Each step is checked against a byte snapshot of the whole test HOME, so the check does not depend on knowing each app's MCP file.
- **Prompt and skills paths:** every path stays under the isolated HOME. The prompt file is in the config dir. `skills_dir` ends in `skills`. Writing an enabled prompt through `PromptService` produces exactly `adapter.prompt_file()`.
- **Takeover:** for every `PROXY_TAKEOVER` app (Claude, Codex, Gemini, Grok Build), the test switches to the fixture provider and turns takeover on with `ProxyService::set_takeover_for_app` on an ephemeral port. While takeover is on, live must carry the placeholder and must not point at the upstream. After takeover is turned off, the HOME snapshot must be byte-identical to the one before.
- **Caps vs traits:** `prompt_file`/`mcp`/`takeover` must match PROMPTS/MCP/PROXY_TAKEOVER. Additive apps must refuse `import_live`. A real key must not count as a takeover placeholder.
- **Codex `model_provider` id:** `CC_SWITCH_CODEX_MODEL_PROVIDER_ID == "custom"`, and `config.toml` keeps that id after the switch, during takeover and after the restore.

**Dummy ninth app.** `DummyAdapter` is a switch-mode app with only the PROMPTS cap, using `~/.dummy-cli/config.json`. It implements `AppAdapter` in the test and passes the same adapter-level contract. The service-level takeover check needs an `AppType` variant, so it does not run for the dummy.
- `dummy_app_needs_no_schema_migration`: on a fresh DB, providers, the current provider and prompts all round-trip under `app_type = 'dummy'`. That holds because those tables are keyed by text. A new app that has takeover (the `proxy_config` CHECK constraint) or MCP/skills enablement (the per-app `enabled_*` columns, see CCS-156) still needs a migration.
- `new_app_backend_touch_points_stay_within_budget`: counts the backend files a new `AppType` must touch. That is the 3 fixed files (`app_config.rs`, `apps/registry.rs`, `apps/mod.rs`), plus the new `apps/<app>/mod.rs`, plus every production file that has an `AppType` match with no catch-all arm. `#[cfg(test)]` modules and test-only module files are skipped.
- I checked the counter against an experiment. In a throwaway worktree I added `AppType::Dummy` plus a registry entry and an adapter arm, and ran `cargo check`. The compiler reported non-exhaustive matches in exactly the files and counts the counter lists: `app_config.rs` (10), `settings.rs` (4), `deeplink/preview.rs` (2), `proxy/providers/mod.rs` (2), `deeplink/provider.rs` (1), `paths.rs` (1), `services/config.rs` (1).
- **Total: 10 files.** `NEW_APP_BACKEND_FILE_BUDGET = 10` and may only go down.

**What remains for "6 or fewer files".** At least 4 more files need to stop matching on `AppType` exhaustively. The candidates:
- `app_config.rs` and `settings.rs`, through per-app settings state (CCS-155).
- `deeplink/{preview,provider}.rs`, `paths.rs` and `services/config.rs`, by dispatching through the registry or adapter.
- `proxy/providers/mod.rs`, which is lane-2 territory.

All of that is outside this ticket's scope.

**Bugs found by the suite (fixed here).**
- `HermesAdapter::credentials` reused OpenClaw's camelCase `apiKey`/`baseUrl` reader. Hermes `config.yaml` uses `api_key`/`base_url`, and `sanitize_hermes_provider_keys` renames camelCase on write, so `credentials()` failed for every real Hermes provider. It now reads snake_case and falls back to camelCase for legacy records. That makes it consistent with `usage_credentials`. `flat_credentials` is now private to OpenClaw.
- `session_manager::providers::opencode::get_opencode_base_dir` used `dirs::home_dir()`, which bypasses the injected `Paths` and `CC_SWITCH_TEST_HOME`. The OpenCode session root therefore escaped the test HOME. It now uses `config::get_home_dir()`. `XDG_DATA_HOME` is still honoured first.
- Regression evidence: I reverted only these two fixes (`git apply -R`) and re-ran the suite. `credentials_round_trip_for_every_app` (Hermes) and `prompt_and_skills_paths_for_every_app` (OpenCode session root) failed, 11 passed / 2 failed. With the fixes, 13 passed.

**Files.**
- New: `src-tauri/tests/app_adapter_conformance.rs`.
- Edited:
  - `src-tauri/src/live_config_testkit.rs`: re-exports `AppAdapter` and its sub-traits, `Credentials`, `adapter`, `AppCaps`, `AppDescriptor`, `SyncMode` and `APPS` for the integration test.
  - `src-tauri/src/apps/hermes/mod.rs`
  - `src-tauri/src/apps/openclaw/mod.rs`
  - `src-tauri/src/session_manager/providers/opencode.rs`
- `src/proxy/**` is untouched.

**Commits.** `6d1baa0` test(apps): add the adapter conformance suite and a dummy ninth app (CCS-150), plus this ticket update.

**Verification.**
- `cargo fmt --manifest-path src-tauri/Cargo.toml` (and `--check`): clean.
- `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`: clean. `cargo clippy --test app_adapter_conformance -- -D warnings`: clean.
- `cargo test --manifest-path src-tauri/Cargo.toml --test app_adapter_conformance`: 13 passed.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib apps::hermes`: 59 passed (filter match).
- `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast`: all green, EXIT 0.
  - lib: 2357 passed, 0 failed, 2 ignored.
  - app_adapter_conformance 13, config_writers 18 (snapshots unchanged), provider_service 33, mcp_commands 20, import_export_sync 26, proxy_e2e 19, proxy_golden 6, migrations 9 and write_lint 3: all passed. Every other integration binary also passed.

**Notes.**
- `apps/claude/plugin.rs` also calls `dirs::home_dir()` directly. The suite does not catch it, because the Claude switch path does not write the plugin file. A follow-up should move it to `get_home_dir()`.
- The lib-side `match_app_sites()` counter (CCS-149) counts the test-only `proxy/usage/cost_golden.rs`. Its `production_source` also treats `#[cfg(test)] mod foo;` as the start of a block. The counter here handles both cases. Fixing the lib counter would lower its count by 1.
