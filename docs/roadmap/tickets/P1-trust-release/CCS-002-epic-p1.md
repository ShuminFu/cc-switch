# CCS-002: [epic] P1 (v3.19): Trust release with security and correctness fixes, no structural change

**Status:** Partial · **Kind:** epic · **Labels:** `epic`, `phase:P1`, `p0-critical`, `area:security`

> **Ticket key:** `epic-p1` · **Phase:** P1 · **Priority:** p0-critical · **Effort:** XL · **Depends on:** `epic-p0`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Goal
Close the concrete security holes and data-integrity bugs before any refactor makes diffs larger. Per the critique, this also covers proxy inbound auth, the usage-script sandbox, deeplink-delivered scripts, the OAuth token stores, and a defined DB compatibility/downgrade policy. P1 has no schema-version bump: the tombstone migration moves to P4, after the migration framework.

## Exit criteria
- [x] The proxy rejects requests with no per-install token, a foreign Host, an Origin header, or a non-JSON body; a non-loopback bind requires a token. *Evidence: `proxy/inbound_auth.rs` `InboundGuard::check` (Host → 403, Origin/Sec-Fetch-Site → 403, non-JSON POST/PUT/PATCH → 415, missing/wrong token → 401; legacy `PROXY_MANAGED` accepted only on a loopback bind, as the CCS-049 spec allows); `proxy/server.rs` refuses a non-loopback bind without a token. 14 `inbound_auth` tests pass.*
- [x] No HalfOpen permit leak, no double send on post-write errors, and a failed proxy reconfiguration rolls back. *Evidence: RAII `HalfOpenPermit` (`circuit_breaker.rs`, `test_dropped_half_open_permit_frees_the_probe_slot`, `forwarder/tests.rs::cancelled_half_open_probe_releases_the_permit`, `tests/proxy_e2e.rs::client_abort_during_half_open_probe_releases_the_probe_slot`); `hyper_client.rs::raw_path_does_not_resend_after_request_was_written`; rollback survives the CCS-153 split in `services/proxy/tests.rs::update_config_to_{occupied_port_keeps_original_server_and_config,invalid_address_restores_previous_server}`. All pass.*
- [x] Usage scripts run with interrupt, memory and stack limits off the tokio workers; deeplink scripts arrive disabled. *Evidence: `usage_script.rs` runs QuickJS in `spawn_blocking` with `set_memory_limit`/`set_max_stack_size`/`set_interrupt_handler` (tests `usage_script_infinite_loop_is_interrupted_near_deadline`, `usage_script_looping_scripts_do_not_starve_runtime_workers` pass); `deeplink/provider.rs` forces `enabled = false` (`test_deeplink_usage_script_is_stored_disabled`).*
- [x] No #[tauri::command] accepts a free-form shell string; listed path-taking commands reject paths outside allowed roots. *Evidence: scan of all 302 `#[tauri::command]`s: the only string params named like commands are `validate_mcp_command(cmd)` (PATH lookup, never executed) and `test_usage_script(script_code)` (sandboxed JS, criterion 3); `launch_session_terminal` takes only `{providerId, sessionId}` and builds the resume command from a fixed template (`'abc;rm -rf ~'` quoting tests in `session_manager/mod.rs`). Path checks: `picked_paths.rs` registry + per-command reject/accept tests (CCS-075), session roots (CCS-076).*
- [ ] atomic_write fsyncs, creates new secret files 0600, follows symlinks and never deletes the target first; zero non-test std::fs::write. *Met in code and on Linux: `config.rs` `write_and_sync` (`sync_all`) + `sync_dir`, `create_new` temp at 0600 / `atomic_write_secret`, symlink resolution (4 `atomic_write_follows_*`/dangling/loop tests), `rename_replace` has no `remove_file` (Windows retries instead); `clippy.toml` bans `std::fs::write` and a scan finds 0 non-test call sites (the 3 outside `mod tests` are in `#[cfg(test)]` modules `bindings.rs`, `wire_golden.rs`, `lifecycle_run.rs` tests); `tests/write_lint.rs` 3/3. Not ticked: the Windows rename-without-delete path (CCS-079) and the owner-only DACL test (CCS-083/084) have only been type-checked, never executed on Windows.*
- [x] SQL import cannot ATTACH or VACUUM INTO; restore validates in a temp DB before touching the main DB. *Evidence: `database/backup.rs` sets `SQLITE_LIMIT_ATTACHED=0` + an authorizer on the temp import connection (`sql_import_rejects_attach_vacuum_into_and_unsafe_statements`); `database/tests.rs::restore_from_backup_{rejects_invalid_backup_and_keeps_main_db,replaces_main_db_after_temp_validation}`. All pass.*
- [x] Cost golden table fully green, including the grokbuild backfill; a deleted built-in price stays deleted. *Evidence: `proxy/usage/cost_golden.rs` has no ignored/known-bad rows; all 7 `cost_golden_*` tests pass, including `cost_golden_backfill_path_grokbuild_cache_rows`; `database/tests.rs::deleted_builtin_model_pricing_stays_deleted_after_restart` passes.*
- [x] Startup reports journal_mode=wal on local paths; pre-migration backup is consistent and also taken for v0/JSON upgrades. *Evidence: `database/journal.rs` logs `Database journal_mode={mode}` (WAL locally, DELETE in cloud-synced dirs; `init_uses_wal_with_busy_timeout_on_local_dir`, `configure_file_connection_uses_wal_locally_and_delete_in_cloud_dirs`); `tests/migrations.rs::{v0_database_gets_premigration_backup, premigration_backup_includes_uncheckpointed_wal_writes, premigration_backup_survives_minimum_retention}` and `backup_legacy_config_json` (called from `app_shell/bootstrap.rs`) for JSON upgrades. All pass.*
- [x] Documented downgrade/mixed-version sync rules, with tests. *Evidence: `docs/dev/DB_COMPATIBILITY.md` (113 lines); tests `import_of_newer_schema_dump_is_rejected_and_leaves_db_unchanged`, `apply_snapshot_rejects_newer_schema_dump_without_partial_import`, `validate_manifest_compat_rejects_next_db_compat_version_with_localized_error`, `restore_backup_over_newer_db_downgrades_and_keeps_newer_copy` pass; `tests/components/DatabaseUpgrade.test.tsx`.*
- [x] Every fix ships with a regression test or golden fixture from the P0 harnesses. *Evidence: all 61 child tickets name the tests they added (Rust unit/integration, vitest, or `tests/lint/*` guards for CSP and asset protocol); spot-checked tests still exist after the P5 moves (e.g. `powershell_single_quote` now in `platform/shell.rs`). The CCS-061 `[::1]`-listener tests skip on this IPv6-less sandbox, but its platform-independent regression test runs.*

## Child tickets
p1-proxy-inbound-auth, p1-proxy-body-limit, p1-proxy-halfopen-permit, p1-proxy-raw-send-no-resend, p1-proxy-config-rollback, p1-proxy-lifecycle-lock, p1-proxy-first-byte-timeout, p1-proxy-raw-body-timeout, p1-proxy-auth-guard-refresh, p1-proxy-mask-url, p1-proxy-connect-status, p1-proxy-connect-userinfo, p1-proxy-connect-ipv6, p1-bridge-tool-result, p1-bridge-image-document, p1-bridge-chat-stream-terminal, p1-bridge-stop-reason-tool-use, p1-bridge-gemini-delta, p1-usage-script-sandbox, p1-usage-script-placeholder-escape, p1-deeplink-script-disabled, p1-deeplink-env-allowlist, p1-deeplink-dialog-guard, p1-deeplink-mask-value, p1-deeplink-confirm-diff, p1-ipc-session-terminal, p1-ipc-path-validation, p1-session-load-messages-roots, p1-csp-tighten, p1-asset-protocol-remove, p1-atomic-write-durable, p1-atomic-write-perms, p1-atomic-write-symlink, p1-route-writes-atomic-lint, p1-oauth-token-store-hardening, p1-settings-atomic-save, p1-settings-save-race, p1-sql-import-sandbox, p1-restore-backup-temp-first, p1-backup-filename-validation, p1-db-premigration-backup, p1-db-wal-busy-timeout, p1-db-compat-policy, p1-usage-input-semantics, p1-usage-session-importer-semantics, p1-pricing-no-reseed, p1-spend-limits-correct, p1-session-sync-partial-line, p1-session-sync-final-entry, p1-switch-provider-off-main, p1-switch-lock-all-apps, p1-subscription-keychain-blocking, p1-skill-zip-extraction, p1-gemini-env-writer, p1-claude-json-optimistic, p1-powershell-quote, p1-fe-jsoneditor-onchange, p1-fe-managed-auth-timers, p1-fe-edit-dialog-reset, p1-fe-mcp-stdio-validation, p1-fe-skills-cache-dedupe

## Epic closure audit

Audited 2026-10-30 against branch `claude/funny-carson-9c7j3e` (HEAD `c0c770a`, after the P5 command-layer merge and the CCS-152/153/154/163/168/212 proxy changes). Targeted runs: `cargo test --lib` filtered to the criterion tests (83 passed, 0 failed), `--test migrations` premigration/v0/legacy-JSON (5 passed), `--test write_lint` (3 passed), `vitest tests/lint/csp.test.ts tests/lint/assetProtocol.test.ts` (7 passed). Full suites were not re-run here.

### Child tickets

| Ticket | Key | Status |
|---|---|---|
| CCS-049 | `p1-proxy-inbound-auth` | Done |
| CCS-050 | `p1-proxy-body-limit` | Done |
| CCS-051 | `p1-proxy-halfopen-permit` | Done |
| CCS-052 | `p1-proxy-raw-send-no-resend` | Done |
| CCS-053 | `p1-proxy-config-rollback` | Done |
| CCS-054 | `p1-proxy-lifecycle-lock` | Done |
| CCS-055 | `p1-proxy-first-byte-timeout` | Done |
| CCS-056 | `p1-proxy-raw-body-timeout` | Done |
| CCS-057 | `p1-proxy-auth-guard-refresh` | Done |
| CCS-058 | `p1-proxy-mask-url` | Done |
| CCS-059 | `p1-proxy-connect-status` | Done |
| CCS-060 | `p1-proxy-connect-userinfo` | Done |
| CCS-061 | `p1-proxy-connect-ipv6` | Partial |
| CCS-062 | `p1-bridge-tool-result` | Done |
| CCS-063 | `p1-bridge-image-document` | Done |
| CCS-064 | `p1-bridge-chat-stream-terminal` | Done |
| CCS-065 | `p1-bridge-stop-reason-tool-use` | Done |
| CCS-066 | `p1-bridge-gemini-delta` | Done |
| CCS-067 | `p1-usage-script-sandbox` | Done |
| CCS-068 | `p1-usage-script-placeholder-escape` | Done |
| CCS-069 | `p1-deeplink-script-disabled` | Done |
| CCS-070 | `p1-deeplink-env-allowlist` | Done |
| CCS-071 | `p1-deeplink-dialog-guard` | Done |
| CCS-072 | `p1-deeplink-mask-value` | Done |
| CCS-073 | `p1-deeplink-confirm-diff` | Done |
| CCS-074 | `p1-ipc-session-terminal` | Done |
| CCS-075 | `p1-ipc-path-validation` | Done |
| CCS-076 | `p1-session-load-messages-roots` | Done |
| CCS-077 | `p1-csp-tighten` | Partial |
| CCS-078 | `p1-asset-protocol-remove` | Partial |
| CCS-079 | `p1-atomic-write-durable` | Partial |
| CCS-080 | `p1-atomic-write-perms` | Done |
| CCS-081 | `p1-atomic-write-symlink` | Done |
| CCS-082 | `p1-route-writes-atomic-lint` | Done |
| CCS-083 | `p1-oauth-token-store-hardening` | Partial |
| CCS-084 | `p1-settings-atomic-save` | Partial |
| CCS-085 | `p1-settings-save-race` | Done |
| CCS-086 | `p1-sql-import-sandbox` | Done |
| CCS-087 | `p1-restore-backup-temp-first` | Done |
| CCS-088 | `p1-backup-filename-validation` | Done |
| CCS-089 | `p1-db-premigration-backup` | Done |
| CCS-090 | `p1-db-wal-busy-timeout` | Done |
| CCS-091 | `p1-db-compat-policy` | Done |
| CCS-092 | `p1-usage-input-semantics` | Done |
| CCS-093 | `p1-usage-session-importer-semantics` | Done |
| CCS-094 | `p1-pricing-no-reseed` | Done |
| CCS-095 | `p1-spend-limits-correct` | Done |
| CCS-096 | `p1-session-sync-partial-line` | Done |
| CCS-097 | `p1-session-sync-final-entry` | Done |
| CCS-098 | `p1-switch-provider-off-main` | Done |
| CCS-099 | `p1-switch-lock-all-apps` | Done |
| CCS-100 | `p1-subscription-keychain-blocking` | Partial |
| CCS-101 | `p1-skill-zip-extraction` | Done |
| CCS-102 | `p1-gemini-env-writer` | Done |
| CCS-103 | `p1-claude-json-optimistic` | Done |
| CCS-104 | `p1-powershell-quote` | Done |
| CCS-105 | `p1-fe-jsoneditor-onchange` | Done |
| CCS-106 | `p1-fe-managed-auth-timers` | Done |
| CCS-107 | `p1-fe-edit-dialog-reset` | Done |
| CCS-108 | `p1-fe-mcp-stdio-validation` | Done |
| CCS-109 | `p1-fe-skills-cache-dedupe` | Done |

54 Done, 7 Partial, 0 Open (61 tickets).

### What is done
- 9 of 10 exit criteria are met and ticked above, each with a passing test on the current tree. That covers proxy inbound auth, proxy correctness, the usage-script sandbox, deeplink scripts arriving disabled, IPC shell/path hardening, SQL import and restore, cost golden plus pricing, WAL and pre-migration backups, the DB compatibility policy, and a regression test per fix.
- The P1 fixes survived the later refactors: rollback tests now live in `services/proxy/tests.rs` after CCS-153; `commands/misc.rs` is gone after CCS-179, and its PowerShell quoting and terminal launch now sit in `platform/shell.rs` and `platform/terminal/`; inbound-token handling is shared with the takeover engine (CCS-154/163).
- Every Partial child ticket has its code and Linux tests merged. Each is waiting only on a check this headless Linux sandbox cannot run, except CCS-084, which also still needs its UI notice.

### What remains (why the epic is Partial)
- **Criterion 5, Windows execution (CCS-079, CCS-083, CCS-084).** `rename_replace` without a pre-delete, with retry on sharing violations, and the owner-only DACL in `atomic_write_secret` were only type-checked for `x86_64-pc-windows-gnu`. They need one green `windows-latest` run of `.github/workflows/ci.yml` (`cargo test`, including `config::tests::atomic_write_secret_sets_owner_only_acl`). Tick criterion 5 and close CCS-079/083 when that run passes.
- **CCS-084 UI (not an exit criterion, but the ticket is open).** The backend `get_settings_load_failure` / `acknowledge_settings_load_failure` commands and the TS wrapper (`src/lib/api/settings.ts`) exist. No component calls them yet. It still needs a startup notice that shows the backup path, or a confirm action that unblocks saves when no backup could be made, plus i18n strings.
- **CCS-061 (IPv6 CONNECT).** The two `[::1]`-listener tests skip here because the kernel has no IPv6 (`EAFNOSUPPORT`). They need one run on a host or CI runner with IPv6 loopback.
- **CCS-077 / CCS-078 (CSP, asset protocol).** These need a manual `pnpm tauri dev` check with devtools open: the models.dev picker loads, the Copilot avatar and URL icons render with no CSP violations, `fetch('https://example.com')` is blocked, and icons still render without `asset://`. Static guards (`tests/lint/csp.test.ts`, `tests/lint/assetProtocol.test.ts`) already pass.
- **CCS-100 (keychain off main thread).** Needs `cargo clippy` on the macOS CI runner, plus a manual check on macOS that other commands keep responding while a keychain prompt is open and that the quota falls back to the credentials file after 20 s.

Once the Windows CI run passes, tick criterion 5 and set this epic to Done. The other items are child-ticket closures and do not block any exit criterion.

### Remaining after triage

- Blocked: Exit criterion 5 / CCS-079 / CCS-083 / CCS-084 Windows parts (rename_replace without pre-delete, owner-only DACL test). Need a Windows machine or a green windows-latest CI job.
- Blocked: CCS-061 [::1]-listener tests. Need a host or CI runner with IPv6 loopback (this container has none).
- Blocked: CCS-100 macOS clippy and keychain-prompt responsiveness check. Need a macOS machine or the macos-latest CI job.
- Blocked: CCS-077 real Copilot avatar / models.dev data load. Need outbound access and a signed-in Copilot account (CCS-226 checks the CSP side).
- Follow-up [CCS-225](../P1-trust-release/CCS-225-p1-settings-load-failure-notice.md): Show a startup notice and confirm flow when settings.json failed to load.
- Follow-up [CCS-226](../P1-trust-release/CCS-226-p1-e2e-csp-icons-runtime.md): Check the tightened CSP and asset-protocol removal at runtime with a headless WebKitGTK spec.
- Follow-up [CCS-228](../P1-trust-release/CCS-228-p1-config-atomic-write-wine.md): Run the Windows rename_replace / atomic_write path under Wine as interim evidence.
