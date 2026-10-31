# CCS-005: [epic] P4 (v3.22): Per-app adapters, takeover engine, persistence and migration framework

**Status:** Partial · **Kind:** epic · **Labels:** `epic`, `phase:P4`, `p1-high`, `area:apps`

> **Ticket key:** `epic-p4` · **Phase:** P4 · **Priority:** p1-high · **Effort:** XL · **Depends on:** `epic-p3`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Goal
Collapse the per-app fan-out (including services/provider/mod.rs, which the plan missed) and give usage, pricing and schema a single owner each, one app or table per PR. The versioned migration framework lands first, so no schema change ships the old way. Then ship takeover for additive apps, OpenClaw MCP and the related features.

## Exit criteria
- [x] Migration framework: MIGRATIONS is contiguous and ends at SCHEMA_VERSION (CI check); create_tables is deleted; each schema bump has a documented DB_COMPAT_VERSION decision. Evidence: `MIGRATIONS` = v1..v21, `SCHEMA_VERSION = 21`; `migrations_are_contiguous_and_end_at_schema_version` and `migrations_declare_db_compat_impact_per_policy` pass (they run in CI `cargo test`). No `fn create_tables` remains; the name appears only in comments. Every step declares `db_compat` (v16–v21 `Additive`, `DB_COMPAT_VERSION` stays 6), and the policy is in `docs/dev/DB_COMPATIBILITY.md`.
- [x] live.rs, prompt_files.rs, skill.rs and commands/config.rs contain no match app_type; backend match-app sites outside apps/** number 15 or fewer. Met since CCS-236: the counter reports **4** sites (budget 4; 2 of them in time-gated `takeover/legacy.rs`). Audit text: Partly met: the four files have no AppType match (asserted by `match_app_sites_outside_apps_stay_within_budget`). Not met: the same counter finds **30** sites today (40 after CCS-149, 56 before it), against a target of 15. See the closure audit.
- [x] Conformance suite green for all 8 apps (credentials, MCP, prompt/skills paths, takeover apply/cleanup restores bytes, caps match traits, codex model_provider id stable). Evidence: `cargo test --test app_adapter_conformance` gives 13 passed. That includes `credentials_round_trip_for_every_app`, `mcp_upsert_and_remove_for_every_app`, `prompt_and_skills_paths_for_every_app`, `takeover_apply_then_cleanup_restores_original_bytes` (all 7 `PROXY_TAKEOVER` apps), `caps_match_implemented_traits_for_every_app` and `codex_model_provider_id_is_stable`.
- [x] No services/proxy/ file exceeds 1500 lines of production code; crash/restore E2E green for claude, codex, gemini, grokbuild, opencode, openclaw and hermes. Evidence: the largest production file is `services/proxy/takeover/mod.rs` at 684 lines (`tests.rs`, 4718 lines, is `#[cfg(test)] mod tests;`). `cargo test --test takeover_crash` gives 10 passed, covering all 7 apps in `cases()`, on Linux. The 3-OS run is tracked by CCS-152 through the CI matrix.
- [x] Fresh and migrated DBs are schema-equivalent; a test-only 9th app touches 6 or fewer backend files and needs no migration. Met since CCS-233: `new_app_backend_touch_points_stay_within_budget` reports **5** files (budget 5); **4** files (budget 4) since CCS-236. Audit text: Partly met: schema equivalence holds (`every_released_schema_migrates_to_current` in `tests/migrations.rs`, 14 passed), and no migration is needed (`dummy_app_needs_no_schema_migration`, `dao::ninth_app_needs_no_migration`). Not met: `new_app_backend_touch_points_stay_within_budget` reports **9** files (budget constant 10), against a target of 6.
- [x] One cost function; the cost golden table (at least 50 rows) passes; backfill output equals ingest output. Evidence: `pricing::engine::cost` is the only `fn cost`. The golden table has 72 rows with a `>= 50` guard. The 7 `cost_golden_*` tests pass, including `cost_golden_backfill_output_equals_ingest_output`.
- [x] No SQL outside database/ (CI grep); dashboard summary p95 is 50 ms or less on a 1M-row DB; superseded_by backfill is chunked and resumable. Evidence: `no_sql_outside_database` passes; its allowlist covers only foreign-DB readers and the test harness. CCS-160's 1M-row benchmark measured a "today" summary p95 of 24 ms after the backfill (`superseded_benchmark_one_million_rows`, ignored by default and not re-run here). `superseded_backfill_resumes_from_cursor_after_kill` and the parity tests pass.
- [x] Killing the process mid-import causes no double counting; a downgraded app still reads current providers through dual-written keys. Evidence: `session_import_killed_between_files_does_not_double_count`, `new_settings_json_is_readable_by_the_old_struct` and `frontend_save_uses_flat_keys_for_legacy_apps` pass.

## Child tickets
p4-migration-framework, p4-adapter-trait-gemini-grok, p4-adapter-claude, p4-adapter-codex, p4-adapter-additive, p4-adapter-conformance, p4-mcp-projector, p4-takeover-crash-e2e, p4-services-proxy-split, p4-takeover-engine, p4-per-app-settings-state, p4-join-tables, p4-pricing-engine, p4-pricing-provenance, p4-usage-repo, p4-superseded-by-backfill, p4-rollup-retention, p4-json-key-order, p4-takeover-additive-apps, p4-openclaw-mcp, p4-capability-matrix, p4-drift-detection, p4-budget-alerts, p4-spend-cap-enforcement, p4-retention-panel, p4-live-config-journal, p4-usage-rescan, p4-keychain-secrets, p4-sync-snapshot-auth

## Epic closure audit

Audited 2026-10-30 on branch `claude/funny-carson-9c7j3e` against the current code and the child tickets' notes. Each measurement was re-run here: the targeted test binaries `app_adapter_conformance` (13 passed), `takeover_crash` (10 passed) and `migrations` (14 passed), plus filtered `--lib` runs of the migration, match-app, cost_golden, superseded, SQL-guard, import-kill and settings-downgrade tests (all passed). The full `cargo test` was not run. **Result: 6 of 8 exit criteria are met (7 of 8 after CCS-233). Status: Partial.**

### Child tickets

| Ticket | Key | Status |
|---|---|---|
| CCS-145 | p4-migration-framework | Done |
| CCS-146 | p4-adapter-trait-gemini-grok | Done |
| CCS-147 | p4-adapter-claude | Done |
| CCS-148 | p4-adapter-codex | Done |
| CCS-149 | p4-adapter-additive | Done (match-app sites 4 since CCS-236) |
| CCS-150 | p4-adapter-conformance | Done (9th app touches 4 files since CCS-236) |
| CCS-151 | p4-mcp-projector | Done |
| CCS-152 | p4-takeover-crash-e2e | Partial (green on 3 OSes is not verified) |
| CCS-153 | p4-services-proxy-split | Done |
| CCS-154 | p4-takeover-engine | Done |
| CCS-155 | p4-per-app-settings-state | Done |
| CCS-156 | p4-join-tables | Done |
| CCS-157 | p4-pricing-engine | Done |
| CCS-158 | p4-pricing-provenance | Done |
| CCS-159 | p4-usage-repo | Done |
| CCS-160 | p4-superseded-by-backfill | Done |
| CCS-161 | p4-rollup-retention | Done |
| CCS-162 | p4-json-key-order | Done |
| CCS-163 | p4-takeover-additive-apps | Done |
| CCS-164 | p4-openclaw-mcp | Done |
| CCS-165 | p4-capability-matrix | Done |
| CCS-166 | p4-drift-detection | Done |
| CCS-167 | p4-budget-alerts | Done |
| CCS-168 | p4-spend-cap-enforcement | Done |
| CCS-169 | p4-retention-panel | Done |
| CCS-170 | p4-live-config-journal | Done |
| CCS-171 | p4-usage-rescan | Done |
| CCS-172 | p4-keychain-secrets | Partial (manual check on macOS, Windows and Linux) |
| CCS-173 | p4-sync-snapshot-auth | Partial (sync-key UI and migration prompt) |
| CCS-212 | p4-hot-switch-rollback-snapshot | Done |

24 of 30 tickets are Done and 6 are Partial.

### What is done
- **Versioned migrations.** v1–v21 run as numbered steps, each with a declared `db_compat` impact, and CI checks contiguity and the compat policy. `create_tables` is gone. Fresh and migrated databases are schema-equivalent for every released schema.
- **App adapters.** All 8 apps have an `AppAdapter`, and `apps::adapter()` is exhaustive. The provider service, `live.rs`, `prompt_files.rs`, `skill.rs` and `commands/config.rs` no longer branch on `AppType`. The conformance suite is green for all 8 apps, and takeover byte-restore is covered for all 7 takeover apps.
- **Takeover.** `services/proxy` is split into `{lifecycle, takeover/{engine,claude,codex,additive,legacy}, backup, hot_switch, config}`, and the largest production file is 684 lines. Crash/restore E2E with raw-byte snapshots passes for all 7 apps on Linux. The additive-app takeover, spend caps and hot-switch rollback snapshot (CCS-212) have shipped.
- **Pricing and usage.** One cost function (`pricing::engine::cost`) with a 72-row golden table, and backfill output equals ingest output. Pricing provenance is recorded, `UsageRepo` / `PricingRepo` are in place, and the CI guard keeps SQL inside `database/`. The `superseded_by` backfill is chunked and resumable (p95 24 ms on 1M rows), and rollups are integer micro-USD with retention.
- **Persistence safety.** Kill-mid-import is idempotent, and per-app settings keys are dual-written so a downgraded app still reads current providers.

### What remains
1. **Resolved by CCS-233/234/235/236 (now 4 sites).** **Match-app sites outside `apps/**`: 30 today, target ≤ 15.** Measured with the same heuristic as `apps::tests::match_app_sites`. Remaining files:
   - `app_config.rs` (4)
   - `services/proxy/takeover/mod.rs` (4) and `takeover/engine.rs` (3)
   - `services/profile.rs` (3)
   - `services/proxy/takeover/legacy.rs` (2)
   - `deeplink/preview.rs` (2)
   - `proxy/providers/mod.rs` (2)
   - one each in `paths.rs`, `deeplink/provider.rs`, `services/config.rs`, `services/stream_check.rs`, `services/takeover_snapshot.rs`, `services/proxy/backup.rs`, `services/proxy/hot_switch.rs` and `commands/terminal.rs`
   - two test-only files the lib counter still counts: `proxy/usage/cost_golden.rs` and `proxy/forwarder/wire_golden.rs`

   Next steps:
   - Route the takeover engine's per-app branches through the adapter or `EntryTakeover` traits.
   - Dispatch the deeplink, profile and paths sites through the registry.
   - Make the counter skip test-only modules.
   - Ratchet `MATCH_APP_SITE_BUDGET` from 40 to the measured 30.

   Note: `commands/terminal.rs` is reworked by lane 2's CCS-179 (`platform/terminal`), which is not merged yet.
2. **Resolved by CCS-233 (now 5 files).** **A 9th app touches 9 backend files, target ≤ 6.** The remaining exhaustive matches are in `app_config.rs`, `deeplink/preview.rs`, `deeplink/provider.rs`, `paths.rs`, `proxy/providers/mod.rs` and `services/config.rs`, on top of the 3 fixed files plus the new adapter. At least 3 of these must drop their exhaustive `AppType` matches (catch-all arm or registry dispatch). `NEW_APP_BACKEND_FILE_BUDGET` can also be ratcheted from 10 to 9.
3. **Not exit criteria, but they keep child tickets Partial:**
   - CCS-152 needs a green 3-OS CI run of `takeover_crash` (ubuntu, windows and macos in `ci.yml`).
   - CCS-172 needs the manual keychain check on real macOS, Windows and Linux desktops (`docs/dev/SECRET_STORAGE.md`).
   - CCS-173 needs the frontend sync-key field and migration prompt, plus manifest rollback protection.

### Remaining after triage

- Blocked: CCS-152: takeover_crash green on 3 OSes. Needs a windows-latest/macos-latest run of the ci.yml backend matrix, or real machines (cross-compiling to windows-gnu/apple fails on aws-lc-sys).
- Blocked: CCS-172: manual keychain checks on macOS and Windows. Need real machines with interactive use.
- Follow-up [CCS-232](../P4-app-adapters-persistence/CCS-232-p4-apps-budget-ratchet.md): Make the match-app counter skip test-only modules and ratchet both app budgets to today's counts.
- Follow-up [CCS-233](../P4-app-adapters-persistence/CCS-233-p4-apps-ninth-app-six-files.md): Make a 9th app touch 6 or fewer backend files.
- Follow-up [CCS-234](../P4-app-adapters-persistence/CCS-234-p4-takeover-dispatch-via-adapter.md): Route takeover per-app dispatch through ProxyTakeover instead of AppType matches.
- Follow-up [CCS-235](../P4-app-adapters-persistence/CCS-235-p4-apps-config-profile-keyed-access.md): Replace per-app field matches in app_config.rs and services/profile.rs with keyed access.
- Follow-up [CCS-236](../P4-app-adapters-persistence/CCS-236-p4-apps-match-sites-under-15.md): Move the remaining stream-check, terminal and proxy-provider dispatch into adapters and set the match-app budget to 15 or fewer.
- Follow-up [CCS-237](../P4-app-adapters-persistence/CCS-237-p4-sync-key-field-api.md): Add the sync-key field to sync settings and expose signature/allowUnsigned in the API layer.
- Follow-up [CCS-238](../P4-app-adapters-persistence/CCS-238-p4-sync-unsigned-migration-prompt.md): Prompt before applying unsigned snapshots and block tampered ones, with i18n for sync.* errors.
- Follow-up [CCS-239](../P4-app-adapters-persistence/CCS-239-p4-sync-manifest-rollback-protection.md): Reject signed sync manifests older than the last applied one.
- Follow-up [CCS-240](../P4-app-adapters-persistence/CCS-240-p4-secrets-linux-secret-service-check.md): Exercise KeyringSecretStore against a headless gnome-keyring and the no-D-Bus fallback.
