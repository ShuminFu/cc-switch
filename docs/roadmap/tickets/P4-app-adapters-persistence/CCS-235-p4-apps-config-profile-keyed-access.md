# CCS-235: [apps] Replace per-app field matches in app_config.rs and services/profile.rs with keyed access

**Status:** Done · **Kind:** refactor · **Labels:** `refactor`, `phase:P4`, `p3-low`, `area:apps`

> **Ticket key:** `p4-apps-config-profile-keyed-access` · **Phase:** P4 · **Priority:** p3-low · **Effort:** M · **Depends on:** `p4-apps-budget-ratchet`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)
> Found during the Partial-ticket triage at d962272 (CCS-149, CCS-005). Source tickets: [CCS-149](../P4-app-adapters-persistence/CCS-149-p4-adapter-additive.md), [CCS-005](../P4-app-adapters-persistence/CCS-005-epic-p4.md).

## Problem
7 match-app sites are per-app field accessors: `app_config.rs` 656 `mcp_for`, 670 `mcp_for_mut`, 794 `auto_import_prompt_if_exists`, 837 `migrate_mcp_to_unified`; `services/profile.rs` 78 `ProfileScope::for_app`, 100 `PerApp::get`, 109 `PerApp::get_mut`. The app_config.rs matches are also exhaustive, so they count against the 9th-app file budget.

## Proposed change
- `mcp_for`/`mcp_for_mut`: one field-accessor macro or a keyed map with serde flatten, keeping the JSON shape.
- `auto_import_prompt_if_exists`: reuse a `prompts_for_mut(app)` accessor.
- `migrate_mcp_to_unified`: iterate an explicit `[(AppType, &servers)]` list of the 4 v3.6 apps.
- profile.rs: drive the 3 sites from a const `[(AppType, ProfileScope)]` table or a registry flag, keeping serde layout and alias deserialisation.
- Ratchet `MATCH_APP_SITE_BUDGET`.

## Acceptance criteria / verification
- [x] `cargo test --manifest-path src-tauri/Cargo.toml --lib app_config`, `--lib services::profile`, `--lib settings`, `--test mcp_commands` and `--test import_export_sync` pass, JSON round-trips included.
- [x] The match-app budget test passes with the budget lowered by ~7.
- [x] `cargo clippy --all-targets -- -D warnings` is clean.

## Dependencies
CCS-232 (`p4-apps-budget-ratchet`).

## Implementation notes

**Summary.** The 7 per-app field-accessor `match app` sites are gone:
- `app_config.rs`: new `impl_app_slots!` macro keeps the single AppType → legacy field mapping and generates `for_app` / `for_app_mut` (still an exhaustive, compiler-checked match) on both `McpRoot` and `PromptRoot`. `MultiAppConfig::mcp_for(_mut)` delegate to it; `auto_import_prompt_if_exists` uses `config.prompts.for_app_mut(&app)`; `migrate_mcp_to_unified` iterates an explicit `[(AppType, &servers)]` list of the 4 v3.6 apps (Claude/Codex/Gemini/OpenCode). Struct fields, serde renames and aliases are untouched, so the JSON shape is unchanged.
- `services/profile.rs`: `ProfileScope::for_app` reads a const `SCOPE_BY_APP: [(AppType, ProfileScope); 3]` table; `PerApp::get(_mut)` go through `for_app` plus a private `slot(_mut)` keyed by `ProfileScope`. Serde layout and registry-alias deserialisation are unchanged.
- `apps/mod.rs`: `MATCH_APP_SITE_BUDGET` lowered 15 → 8 (the old budget test fails at 8 before this change).

**Tests added.** `app_config::tests::legacy_app_slots_are_keyed_by_app_id` (for every registry app, writing through the keyed accessors lands in the JSON slot keyed by `app.as_str()` and round-trips without touching other slots); `services::profile::tests::test_per_app_keyed_access_hits_serde_slot` (same for `PerApp`).

**Files.** `src-tauri/src/app_config.rs`, `src-tauri/src/services/profile.rs`, `src-tauri/src/apps/mod.rs`.

**Commits.** ca5b8e3 (code + tests); this ticket update in a follow-up docs commit.

**Verification.**
- `cargo fmt --manifest-path src-tauri/Cargo.toml --check`: clean.
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`: clean.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib -- app_config services::profile match_app`: 27 passed.
- `cargo test --manifest-path src-tauri/Cargo.toml --test mcp_commands --test import_export_sync --test profile_roundtrip`: 27 / 22 / 6 passed.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib` (includes `settings`): 2862 passed, 0 failed, 3 ignored.
