# CCS-235: [apps] Replace per-app field matches in app_config.rs and services/profile.rs with keyed access

**Status:** Open · **Kind:** refactor · **Labels:** `refactor`, `phase:P4`, `p3-low`, `area:apps`

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
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --lib app_config`, `--lib services::profile`, `--lib settings`, `--test mcp_commands` and `--test import_export_sync` pass, JSON round-trips included.
- [ ] The match-app budget test passes with the budget lowered by ~7.
- [ ] `cargo clippy --all-targets -- -D warnings` is clean.

## Dependencies
CCS-232 (`p4-apps-budget-ratchet`).
