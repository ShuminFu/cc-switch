# CCS-155: [settings] Use keyed per-app maps with old-key dual-write, and migrate profiles and localStorage

**Status:** Done · **Kind:** refactor · **Labels:** `refactor`, `phase:P4`, `p2-medium`, `area:settings`

> **Ticket key:** `p4-per-app-settings-state` · **Phase:** P4 · **Priority:** p2-medium · **Effort:** M · **Depends on:** `p2-app-registry`, `p1-settings-save-race`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Per-app settings are flat keys, so every new app needs new keys. The critique adds three points:
- reading the old keys is not enough; without also writing them, a downgrade loses current providers, visible apps and overrides
- the profiles table and localStorage are keyed by app id

## Evidence
- settings.rs current_provider_*, visible apps, config dirs; schema.rs:314 profiles; tests/profile_roundtrip.rs; App.tsx STORAGE_KEY, useCodexCommonConfig, useGeminiCommonConfig

## Proposed change
- Serialize CommonConfigSnippets, VisibleApps, current_providers and config_dirs as maps, with serde reading the old flat keys and also writing them for one release.
- Map profiles and localStorage keys through the registry aliases.

## Acceptance criteria / verification
- [x] Test: a new-version settings.json loaded by the old struct (a copy kept in tests) still finds the current providers
- [x] profile_roundtrip passes
- [x] `cargo test --manifest-path src-tauri/Cargo.toml settings` and `pnpm test:unit`

## Dependencies
p2-app-registry, p1-settings-save-race

## Implementation notes

**Summary.**
- `settings.rs`: the 7 `*_config_dir` and 8 `current_provider_*` fields are replaced by one flattened `PerAppSettings { config_dirs, current_providers }`, both `BTreeMap<slug, String>`. It serializes as `configDirs` / `currentProviders` maps and also writes the old flat keys (`claudeConfigDir`, `currentProviderClaude`, ...) for one release, so a downgraded build still finds them. The legacy key tables (`LEGACY_CONFIG_DIR_KEYS`, `LEGACY_CURRENT_PROVIDER_KEYS`) are frozen, and new apps only go into the maps. On read the map wins and flat keys fill in any app missing from it (a file saved by an old build has only flat keys, because the old struct drops unknown fields). Map keys are normalized through registry aliases, and keys for unknown or future apps are kept. New accessors are `config_dir` / `set_config_dir` / `with_config_dir` / `current_provider_id` / `set_current_provider_id`. `override_dir`, `get_current_provider` and `set_current_provider` use them.
- Frontend save path: `save_settings` now takes a `serde_json::Value` and calls `AppSettings::from_frontend_value`. This drops map entries for apps that have a legacy flat key, so the flat key the form edited (or removed) decides. Otherwise the stale map the frontend round-trips would swallow edits and clears. Apps without a legacy key keep their map entry.
- `VisibleApps` is now a slug-keyed map. It writes one bool per registered app, with defaults taken from registry `default_visible`, which is the same JSON shape as the old struct. It also keeps unknown keys and normalizes aliases.
- `CommonConfigSnippets` (app_config.rs) is a slug-keyed map with the same JSON shape, alias-normalized. `migration.rs` still migrates only the claude, codex and gemini snippets, as before.
- Profiles: `PerApp<T>` deserialization maps keys through registry aliases (for example `claude_desktop` → `claude-desktop`). The canonical key wins over an alias, and unknown apps are ignored. The `current_profile_id_<scope>` keys in the settings table already used registry slugs, so they are unchanged.
- localStorage: `registry.ts` gains `aliases` (mirroring the backend) and `resolveAppId()`. `getInitialApp()` resolves `cc-switch-last-app` through it, and `AppSwitcher` reuses `APP_STORAGE_KEY` instead of a duplicate literal. The legacy common-config localStorage keys are one-time migration reads and were left as they are.

**Files.** src-tauri/src/settings.rs, src-tauri/src/commands/settings.rs, src-tauri/src/app_config.rs, src-tauri/src/database/migration.rs, src-tauri/src/services/profile.rs, src-tauri/src/apps/hermes/config.rs, src-tauri/tests/{config_writers,hermes_roundtrip,mcp_commands,test_env_isolation}.rs, src/entities/app/registry.ts, src/components/app-shell/views.ts, src/components/AppSwitcher.tsx, tests/entities/appRegistry.test.tsx.

**Commits.** a44a9bd (code + tests); this ticket update in a follow-up docs commit.

**Tests added.**
- `settings::tests::new_settings_json_is_readable_by_the_old_struct`: a frozen copy of the old struct (`legacy_v1`) reads a new-version settings.json and finds every current provider, config dir and visible-app flag.
- `settings_serialize_keyed_maps_and_roundtrip`, `old_flat_settings_json_still_loads`, `keyed_map_wins_over_flat_keys_and_keeps_unknown_apps`, `frontend_save_uses_flat_keys_for_legacy_apps`, `normalize_trims_and_drops_empty_config_dirs`.
- `services::profile::tests::test_payload_maps_app_keys_through_registry_aliases`, `app_config::tests::common_config_snippets_keep_json_shape_and_map_aliases`.
- vitest: `resolves persisted app ids through registry aliases`, `restores the last app from localStorage via aliases`.

**Verification.**
- `cargo fmt --manifest-path src-tauri/Cargo.toml`: clean
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`: passed
- `cargo test --manifest-path src-tauri/Cargo.toml --lib settings`: 50 passed, 0 failed
- `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast` (full suite): 2562 passed, 0 failed; `profile_roundtrip`: 6 passed
- `pnpm typecheck`: passed; `pnpm test:unit`: 156 files, 1523 tests passed; prettier and eslint on the touched TS files: clean
