# CCS-234: [takeover] Route takeover per-app dispatch through ProxyTakeover instead of AppType matches

**Status:** Done · **Kind:** refactor · **Labels:** `refactor`, `phase:P4`, `p2-medium`, `area:takeover`

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
- [x] `cargo test --manifest-path src-tauri/Cargo.toml --lib apps::tests::match_app_sites_outside_apps_stay_within_budget` passes with the budget lowered by ~10.
- [x] `cargo test --lib services::proxy`, `--test takeover_crash` (10), `--test proxy_e2e` and `--test config_writers` pass; `unified_takeover_writes_the_same_live_files_as_the_legacy_engine` stays byte-identical.
- [x] `cargo clippy --all-targets -- -D warnings` is clean.

## Dependencies
CCS-232 (`p4-apps-budget-ratchet`).

## Implementation notes

**Summary.** `ProxyTakeover` (`apps/adapter.rs`) now carries the per-app takeover behaviour that used to be
hand-dispatched with `match AppType`:

| Trait method | Replaces |
| --- | --- |
| `live_files` | `takeover_snapshot::takeover_live_files` match (Gemini: `.env` only) |
| `read_live` | `read_live_for_takeover` match |
| `write_live` / `write_takeover_live` (Codex: provider-aware) | `write_live_config_for_app` match, engine `write_takeover_live` match |
| `strip_placeholders` / `write_stripped_live` (Grok writes raw text, skipping validation) | `cleanup_takeover_placeholders_in_live_for_app` match |
| `base_url_matches` (default: entry-level, used by OpenCode/OpenClaw/Hermes) | engine `live_base_url_matches` match + `entries_base_url_matches` |
| `provider_need` (`TakeoverProviderNeed::{None, Required, RequiredWhenStrict}`) | engine `takeover_provider` match |
| `sync_live_credentials` | `sync_live_config_to_provider` match (Grok keeps failing hard on DB errors; the others log a warning, as before) |
| `backup_projection` (Gemini: `{"env": …}`) | `backup.rs` `backup_json` match |
| `refresh_after_save`, `preserves_live_toml_mcp` | `hot_switch.rs` `refresh_live_after_provider_save` match |

The live readers, writers, cleanup and credential write-back for Claude/Codex/Gemini/Grok moved verbatim into
`apps/{claude,codex,gemini,grok}/takeover.rs`. Writers receive config whose token sentinel the service has
already bound (`bind_proxy_token`), so `apps/` does not depend on `ProxyService`. The `ProxyService` helpers that
`takeover/legacy.rs` and the tests use (`read_claude_live`, `write_codex_takeover_live_for_provider`, …) are now
thin wrappers, so `legacy.rs` is unchanged. Additive apps still go through `EntryTakeover`, chosen by
capability (`entry_takeover().is_some()`) and not by app. Claude and Codex hot-switch refreshes share
`rebuild_takeover_live_from_provider`, which reuses the engine's `project_takeover_live` / `write_takeover_live`.
`is_local_proxy_url` / `proxy_urls_match` moved to `apps/adapter.rs`.

`MATCH_APP_SITE_BUDGET` 25 → 15 (meets the CCS-149 P4 target). The budget test also asserts that the five
takeover files no longer match on `AppType`. The 15 remaining sites are `app_config.rs` (4),
`services/profile.rs` (3), `proxy/providers/mod.rs` (2), `takeover/legacy.rs` (2, kept under CCS-154),
`commands/terminal.rs`, `deeplink/provider.rs`, `services/config.rs` and `services/stream_check.rs`.

**Files.** `src-tauri/src/apps/adapter.rs`, `apps/mod.rs` (budget + new
`takeover_dispatch_matches_previous_per_app_arms` test), `apps/{claude,codex,gemini,grok}/{mod,takeover}.rs`,
`apps/{opencode,openclaw,hermes}/mod.rs`, `services/proxy/{backup,hot_switch,tests}.rs`,
`services/proxy/takeover/{mod,engine,additive,claude,codex,gemini,grok}.rs`, `services/takeover_snapshot.rs`.

**Commits.** `2e2c225` refactor(takeover): dispatch per-app takeover through ProxyTakeover (CCS-234), plus this
ticket-notes commit.

**Verification.**
- `cargo test --lib apps::tests::match_app_sites_outside_apps_stay_within_budget`: passes with budget 15. It
  fails before the change, when the counter reports 25 sites.
- `cargo test --lib`: 2860 passed, 0 failed, 3 ignored. This includes `services::proxy` and
  `unified_takeover_writes_the_same_live_files_as_the_legacy_engine`.
- `cargo test --test takeover_crash`: 10 passed. `--test proxy_e2e`: 37 passed. `--test config_writers`: 19 passed.
- `cargo test --no-fail-fast` (full suite): 3107 passed, 0 failed.
- `cargo clippy --all-targets -- -D warnings`: clean. `cargo fmt --check`: clean.
