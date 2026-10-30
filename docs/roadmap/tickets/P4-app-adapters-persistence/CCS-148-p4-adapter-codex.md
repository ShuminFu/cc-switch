# CCS-148: [apps] Migrate Codex to AppAdapter, keeping the history-migration ids and markers stable

**Status:** Done · **Kind:** refactor · **Labels:** `refactor`, `phase:P4`, `p2-medium`, `area:apps`

> **Ticket key:** `p4-adapter-codex` · **Phase:** P4 · **Priority:** p2-medium · **Effort:** L · **Depends on:** `p4-adapter-trait-gemini-grok`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
The critique notes codex_history_migration.rs (2630 lines) writes threads.model_provider into Codex's own state DB using CC_SWITCH_CODEX_MODEL_PROVIDER_ID, and stores markers in settings.json. Moving Codex code must not change these.

## Evidence
- codex_history_migration.rs:644, 1157; codex_config.rs:244-250 (paired rollback)

## Proposed change
Add apps/codex, which keeps the provider id constant and the marker keys. Keep the auth.json and config.toml pair rollback.

## Acceptance criteria / verification
- [x] The Codex state DB snapshot is unchanged
- [x] A conformance check asserts the provider id constant
- [x] `cargo test --manifest-path src-tauri/Cargo.toml codex`

## Dependencies
p4-adapter-trait-gemini-grok

## Implementation notes

**Summary.** Added `apps/codex` (`CodexAdapter`). `apps::adapter()` now returns adapters for Claude, Claude Desktop, Codex, Gemini and Grok Build.

- Moved files: `codex_config.rs` became `apps/codex/config.rs`, `codex_history_migration.rs` became `apps/codex/history_migration.rs`, and `codex_state_db.rs` became `apps/codex/state_db.rs`. `lib.rs` keeps `crate::codex_config`, `crate::codex_history_migration` and `crate::codex_state_db` as `use` aliases, so other callers (including `src/proxy/**`, the session manager and the testkit) did not change. The only edits inside the moved files are the two `include_str!` resource paths and `pub(super)` on five history-migration constants, so the conformance tests can read them.
- What moved into `CodexAdapter`:
  - live read (including the `modelCatalog` reverse-parse) and import;
  - the `auth.json` + `config.toml` write. It still goes through `write_codex_provider_live_with_catalog` → `write_codex_live_atomic`, so the paired rollback is unchanged;
  - validation, credentials and the switch-away backfill;
  - the TOML common-config snippet. The toml_edit merge/subset/strip helpers, `extract_common_config` and `update_toml_common_config_snippet` moved to `apps/codex/common_config.rs`, and `services::provider::update_toml_common_config_snippet` stays as a re-export;
  - MCP projection, takeover placeholder detection (auth token, bearer token, official proxy route) and session roots.
- Trait changes (`apps/adapter.rs`):
  - `AppAdapter::import_category` (default `"custom"`). Codex returns `"official"` for login-only live configs.
  - `ProxyTakeover::official_provider_supported` (default false). `official_provider_supports_proxy_takeover` now delegates to it.
- Removed Codex arms from:
  - `services/provider/live.rs` (common config contains/apply/remove, backfill, write, read, import and import category). The never-constructed `LiveSnapshot::Codex` enum was deleted.
  - `services/provider/mod.rs` (sync-from-live scope, extract, validate, credentials).
  - `prompt_files.rs` (the now-unused `get_base_dir_with_fallback` was dropped), `commands/config.rs`, `services/mcp.rs` (upsert/remove), `services/skill.rs` and `session_manager::provider_roots`.
- `reapply_current_codex_official_live` is a Codex-only workflow, not per-app dispatch, so it moved verbatim from `services/provider/mod.rs` to `services/provider/codex_official.rs`. It is re-exported under the same path.
- `services/proxy.rs`: `is_codex_live_taken_over` now delegates to the adapter, and the private `codex_live_has_proxy_placeholder` was folded into it. `src/proxy/**` is untouched.

**History-migration ids and markers (unchanged).** The conformance tests in `apps/codex/mod.rs` pin these values:
- `history_migration_provider_ids_are_stable`: `CC_SWITCH_CODEX_MODEL_PROVIDER_ID == "custom"`, legacy `"ccswitch"`, official `"openai"`, and the official proxy route `"cc-switch-official"`.
- `history_migration_backup_names_are_stable`: the three migration backup dir names and `state_5.sqlite`.
- `history_migration_marker_keys_are_stable`: the exact `localMigrations` JSON keys and field names for `codexThirdPartyHistoryProviderBucketV1`, `codexProviderTemplateV1` and `codexOfficialHistoryUnifyV1`.

The `config_writers` snapshot for the Codex state DB migration (`codex_state_db_provider_bucket_migration`) passes without any snapshot changes.

**Match-app count.** Production `AppType::Codex` references, with test modules and comments excluded:

| File | Before | After |
|---|---|---|
| `services/provider/live.rs` | 8 | 0 |
| `services/provider/mod.rs` | 8 | 0 |
| `prompt_files.rs` | 1 | 0 |
| `commands/config.rs` | 1 | 0 |
| `services/skill.rs` | 2 | 0 |
| `services/mcp.rs` | 3 | 1 |
| `services/proxy.rs` | 26 | 27 |

The one left in `services/mcp.rs` is the `McpApps` flag-diff. `services/proxy.rs` gained one because of the adapter delegation. Its takeover and restore flow is left for the takeover refactor.

**Tests.**
- `apps::tests::adapter_backed_apps_have_no_arms_in_provider_service` now covers Codex automatically. It fails on the pre-change sources (8 + 8 + 1 + 1 Codex arms) and passes now.
- `migrated_apps_have_adapters` and `adapter_paths_match_legacy_layout` were extended to Codex. The prompt path equals the old `auth.json parent + AGENTS.md`, and the session roots equal `session_manager::providers::codex::session_roots()`.
- New in `apps/codex/mod.rs`: the three conformance tests above, plus `import_category_detects_official_login`, `credentials_read_auth_and_config_base_url`, `validate_rejects_non_string_config`, `common_snippet_round_trips_config_toml`, `takeover_detects_placeholders_and_official_route` and `backfill_keeps_stored_model_catalog_and_strips_mcp_servers`.
- New in `tests/config_writers.rs`: `codex_failed_config_write_rolls_back_auth_json` (unix). A dangling `config.toml` symlink makes the second write fail, and the test asserts `auth.json` is restored. I confirmed it fails when the rollback in `write_codex_live_atomic` is disabled, and passes with the rollback in place.

**Commits.** `17dc423` refactor(apps): migrate Codex to AppAdapter (CCS-148), plus this ticket update.

**Verification.**
- `cargo fmt --manifest-path src-tauri/Cargo.toml`: clean.
- `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`: clean.
- `cargo test --manifest-path src-tauri/Cargo.toml codex`: all green. That is 507 lib tests plus the matching integration tests (config_writers 5, provider_service 16, import_export_sync 14, and others).
- `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast`: all green.
  - lib: 2343 passed, 0 failed, 2 ignored.
  - config_writers: 18 passed. The snapshots are unchanged.
  - provider_service 33, provider_commands 10, mcp_commands 20, import_export_sync 26, skill_sync 8, proxy_e2e 19, proxy_golden 6, migrations 9 and write_lint 3: all passed. Every other integration binary also passed.
