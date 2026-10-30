# CCS-234: [takeover] Route takeover per-app dispatch through ProxyTakeover instead of AppType matches

**Status:** Open · **Kind:** refactor · **Labels:** `refactor`, `phase:P4`, `p2-medium`, `area:takeover`

> **Ticket key:** `p4-takeover-dispatch-via-adapter` · **Phase:** P4 · **Priority:** p2-medium · **Effort:** L · **Depends on:** `p4-apps-budget-ratchet`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)
> Found during the Partial-ticket triage at d962272 (CCS-149, CCS-005). Source tickets: [CCS-149](../P4-app-adapters-persistence/CCS-149-p4-adapter-additive.md), [CCS-005](../P4-app-adapters-persistence/CCS-005-epic-p4.md).

## Problem
10 of the 28 match-app sites outside `apps/**` are takeover dispatch:
- `services/proxy/takeover/mod.rs`: 301 `read_live_for_takeover`, 316 `sync_live_config_to_provider`, 590 `write_live_config_for_app`, 611 `cleanup_takeover_placeholders_in_live_for_app`
- `takeover/engine.rs`: 167 current-provider requirement, 222 `write_takeover_live`, 246 `live_base_url_matches`
- `services/proxy/hot_switch.rs:38` `refresh_live_after_provider_save`
- `services/proxy/backup.rs:414` backup_json serialisation
- `services/takeover_snapshot.rs:103` `takeover_live_files`
The CCS-149 target (≤ 15) cannot be reached without them.

## Proposed change
- Extend `ProxyTakeover` (`apps/adapter.rs:234`) with e.g. `read_live`, `write_live`, `cleanup_placeholders`, `base_url_matches`, `requires_current_provider(mode)`, `live_files`, `backup_projection`, `refresh_after_save`.
- Implement them in `apps/{claude,codex,gemini,grok}` and the additive `EntryTakeover` path; call `apps::adapter(app).takeover()` at each site.
- Leave `takeover/legacy.rs` verbatim (kept for one release under CCS-154).
- Ratchet `MATCH_APP_SITE_BUDGET` to the new count.

## Acceptance criteria / verification
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --lib apps::tests::match_app_sites_outside_apps_stay_within_budget` passes with the budget lowered by ~10.
- [ ] `cargo test --lib services::proxy`, `--test takeover_crash` (10), `--test proxy_e2e` and `--test config_writers` pass; `unified_takeover_writes_the_same_live_files_as_the_legacy_engine` stays byte-identical.
- [ ] `cargo clippy --all-targets -- -D warnings` is clean.

## Dependencies
CCS-232 (`p4-apps-budget-ratchet`).
