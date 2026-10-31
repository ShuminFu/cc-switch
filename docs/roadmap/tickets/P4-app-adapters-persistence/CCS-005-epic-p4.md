# CCS-005: [epic] P4 (v3.22): Per-app adapters, takeover engine, persistence and migration framework

**Status:** Done · **Kind:** epic · **Labels:** `epic`, `phase:P4`, `p1-high`, `area:apps`

> **Ticket key:** `epic-p4` · **Phase:** P4 · **Priority:** p1-high · **Effort:** XL · **Depends on:** `epic-p3`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Goal
Collapse the per-app fan-out (including services/provider/mod.rs, which the plan missed) and give usage, pricing and schema a single owner each, one app or table per PR. The versioned migration framework lands first, so no schema change ships the old way. Then ship takeover for additive apps, OpenClaw MCP and the related features.

## Exit criteria
- [x] Migration framework: MIGRATIONS is contiguous and ends at SCHEMA_VERSION (CI check); create_tables is deleted; each schema bump has a documented DB_COMPAT_VERSION decision. Evidence: `MIGRATIONS` = v1..v21, `SCHEMA_VERSION = 21` (`database/mod.rs`). `migrations_are_contiguous_and_end_at_schema_version` and `migrations_declare_db_compat_impact_per_policy` pass, and they run in the CI `cargo test`. No `fn create_tables` remains in `src-tauri/src`. Every step declares `db_compat` (v16–v21 `Additive`, so `DB_COMPAT_VERSION` stays 6), and `docs/dev/DB_COMPATIBILITY.md` holds the policy.
- [x] live.rs, prompt_files.rs, skill.rs and commands/config.rs contain no match app_type; backend match-app sites outside apps/** number 15 or fewer. Evidence: `apps::tests::match_app_sites_outside_apps_stay_within_budget` passes with `MATCH_APP_SITE_BUDGET = 4` (it was 40 before CCS-232). The 4 sites are `deeplink/provider.rs`, `services/config.rs` and 2 in the time-gated `services/proxy/takeover/legacy.rs`. The same test asserts that the four named files have no `AppType` match. Delivered by CCS-232, CCS-234, CCS-235 and CCS-236.
- [x] Conformance suite green for all 8 apps (credentials, MCP, prompt/skills paths, takeover apply/cleanup restores bytes, caps match traits, codex model_provider id stable). Evidence: `cargo test --test app_adapter_conformance` gives 13 passed, including `credentials_round_trip_for_every_app`, `mcp_upsert_and_remove_for_every_app`, `prompt_and_skills_paths_for_every_app`, `takeover_apply_then_cleanup_restores_original_bytes` (all 7 `PROXY_TAKEOVER` apps), `caps_match_implemented_traits_for_every_app` and `codex_model_provider_id_is_stable`.
- [x] No services/proxy/ file exceeds 1500 lines of production code; crash/restore E2E green for claude, codex, gemini, grokbuild, opencode, openclaw and hermes. Evidence: the largest production file under `services/proxy/` is now `backup.rs` at 530 lines before its `#[cfg(test)]` block. `takeover/mod.rs` dropped to 420 after CCS-234, and `tests.rs` is test-only. `cargo test --test takeover_crash` gives 10 passed and covers all 7 apps on Linux. The 3-OS run belongs to CCS-152's own acceptance; this criterion does not require it.
- [x] Fresh and migrated DBs are schema-equivalent; a test-only 9th app touches 6 or fewer backend files and needs no migration. Evidence: `cargo test --test migrations` gives 14 passed (`every_released_schema_migrates_to_current`, `ninth_app_needs_no_migration`), and `dummy_app_needs_no_schema_migration` passes. `new_app_backend_touch_points_stay_within_budget` reports **4** files (`app_config.rs`, `apps/<new app>/mod.rs`, `apps/mod.rs`, `apps/registry.rs`) with `NEW_APP_BACKEND_FILE_BUDGET = 4`. Delivered by CCS-233 and CCS-236.
- [x] One cost function; the cost golden table (at least 50 rows) passes; backfill output equals ingest output. Evidence: `pricing::engine::cost` is the only `fn cost` in `src-tauri/src`. The golden table has 72 rows with a `>= 50` guard, and the 7 `cost_golden_*` tests pass, including `cost_golden_backfill_output_equals_ingest_output`.
- [x] No SQL outside database/ (CI grep); dashboard summary p95 is 50 ms or less on a 1M-row DB; superseded_by backfill is chunked and resumable. Evidence: `no_sql_outside_database` passes. CCS-160's 1M-row benchmark measured a "today" summary p95 of 24 ms (`superseded_benchmark_one_million_rows`, ignored by default and not re-run here). `superseded_backfill_resumes_from_cursor_after_kill` and `superseded_backfill_matches_exists_and_queries_switch_when_done` pass.
- [x] Killing the process mid-import causes no double counting; a downgraded app still reads current providers through dual-written keys. Evidence: `session_import_killed_between_files_does_not_double_count`, `new_settings_json_is_readable_by_the_old_struct` and `frontend_save_uses_flat_keys_for_legacy_apps` pass.

## Child tickets
p4-migration-framework, p4-adapter-trait-gemini-grok, p4-adapter-claude, p4-adapter-codex, p4-adapter-additive, p4-adapter-conformance, p4-mcp-projector, p4-takeover-crash-e2e, p4-services-proxy-split, p4-takeover-engine, p4-per-app-settings-state, p4-join-tables, p4-pricing-engine, p4-pricing-provenance, p4-usage-repo, p4-superseded-by-backfill, p4-rollup-retention, p4-json-key-order, p4-takeover-additive-apps, p4-openclaw-mcp, p4-capability-matrix, p4-drift-detection, p4-budget-alerts, p4-spend-cap-enforcement, p4-retention-panel, p4-live-config-journal, p4-usage-rescan, p4-keychain-secrets, p4-sync-snapshot-auth

## Epic closure audit

Re-audited 2026-10-31 on branch `claude/funny-carson-9c7j3e` at `673be73`, after lane 2 (CCS-213..253) was merged, against the current code and the notes on the child and follow-up tickets. Every measurement was re-run here: the targeted binaries `app_adapter_conformance` (13 passed), `takeover_crash` (10 passed) and `migrations` (14 passed), plus one filtered `--lib` run of the migration, match-app budget, cost_golden, superseded, SQL-guard, import-kill and settings-downgrade tests (16 passed). The production line counts under `services/proxy/` were measured with `awk` up to `#[cfg(test)]`. The full `cargo test` was not run. **Result: all 8 exit criteria are met. Status: Done.** Two child tickets stay Partial, but only for checks that need other machines (see below). Neither check is an exit criterion of this epic.

### Child and follow-up tickets

| Ticket | Key | Status |
|---|---|---|
| CCS-145 | p4-migration-framework | Done |
| CCS-146 | p4-adapter-trait-gemini-grok | Done |
| CCS-147 | p4-adapter-claude | Done |
| CCS-148 | p4-adapter-codex | Done |
| CCS-149 | p4-adapter-additive | Done |
| CCS-150 | p4-adapter-conformance | Done |
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
| CCS-172 | p4-keychain-secrets | Partial (manual check on macOS, Windows and a KWallet desktop) |
| CCS-173 | p4-sync-snapshot-auth | Partial in its file; all acceptance boxes are ticked and its follow-ups CCS-237/238/239 are Done, so only product/protocol decisions remain |
| CCS-212 | p4-hot-switch-rollback-snapshot | Done |
| CCS-232 | p4-apps-budget-ratchet | Done |
| CCS-233 | p4-apps-ninth-app-six-files | Done |
| CCS-234 | p4-takeover-dispatch-via-adapter | Done |
| CCS-235 | p4-apps-config-profile-keyed-access | Done |
| CCS-236 | p4-apps-match-sites-under-15 | Done |
| CCS-237 | p4-sync-key-field-api | Done (lane 2) |
| CCS-238 | p4-sync-unsigned-migration-prompt | Done (lane 2) |
| CCS-239 | p4-sync-manifest-rollback-protection | Done |
| CCS-240 | p4-secrets-linux-secret-service-check | Done |

36 of 39 tickets are Done. Three are Partial: CCS-152, CCS-172 and, in its file only, CCS-173.

### What is done
- **Versioned migrations.** v1–v21 run as numbered steps, and each step declares its `db_compat` impact. CI checks contiguity and the compat policy. `create_tables` is gone. Fresh and migrated databases are schema-equivalent for every released schema, and a 9th app needs no migration.
- **App adapters.** All 8 apps have an `AppAdapter`. Only 4 `AppType` match sites remain outside `apps/**` (target 15), and 2 of them are in the time-gated legacy takeover path. A 9th app touches 4 backend files (target 6). Both budgets are ratcheted in tests, so they cannot regress. Takeover dispatch goes through `ProxyTakeover` (CCS-234), `app_config`/profile access is keyed (CCS-235), and stream-check, terminal and proxy-upstream dispatch go through adapter hooks (CCS-236). The conformance suite is green for all 8 apps.
- **Takeover.** `services/proxy` is split up, and its largest production file is 530 lines. Crash/restore E2E with raw-byte snapshots passes for all 7 takeover apps on Linux. Additive-app takeover, spend caps and the hot-switch rollback snapshot have shipped.
- **Pricing and usage.** There is one cost function with a 72-row golden table, and backfill output equals ingest output. The CI guard keeps SQL inside `database/`. The `superseded_by` backfill is chunked and resumable (p95 24 ms on 1M rows), and rollups are stored as integer micro-USD with retention.
- **Persistence and security.** Kill-mid-import is idempotent. Per-app settings keys are dual-written, so a downgraded app still reads current providers. Secrets live in the OS keychain, and the Linux Secret Service and no-D-Bus paths are exercised by `scripts/check-linux-secret-service.sh` (CCS-240). Sync snapshots are HMAC-signed. The settings UI has a sync-key field (`SyncSnapshotKeyField.tsx`), prompts before an unsigned snapshot is applied, blocks tampered snapshots and rejects rolled-back manifests (CCS-237/238/239).

### What remains (outside this sandbox; no exit criterion depends on it)
1. **CCS-152 "Green on 3 OSes":** needs a green GitHub Actions run of the `ci.yml` backend matrix on `windows-latest` and `macos-latest`. That run includes `takeover_crash`, which is 10/10 on Linux here. Cross-compiling to Windows or Apple fails on `aws-lc-sys`, so this cannot be done locally.
2. **CCS-172 manual keychain check:** a human must run the `docs/dev/SECRET_STORAGE.md` checklist on a real Mac (Keychain Access entries, prompts, downgrade), on Windows (Credential Manager, owner-only ACL on the fallback file, PowerShell downgrade) and on a KWallet Linux desktop with interactive unlock prompts.
3. **CCS-173 optional hardening (product and protocol decisions, not acceptance):** make the sync key mandatory once the key UI has shipped in a release (a time gate). Binding the signature to the remote location or profile needs a manifest version bump. Its Status line can move to Done; this audit did not edit it because it is a child ticket.
4. **Index drift:** `docs/roadmap/tickets/README.md` still lists CCS-149 as Partial and CCS-232..240 as Open. All of them are Done in their own files.
