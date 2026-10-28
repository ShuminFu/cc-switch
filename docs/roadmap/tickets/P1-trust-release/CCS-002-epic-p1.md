# CCS-002: [epic] P1 (v3.19): Trust release with security and correctness fixes, no structural change

**Status:** Open · **Kind:** epic · **Labels:** `epic`, `phase:P1`, `p0-critical`, `area:security`

> **Ticket key:** `epic-p1` · **Phase:** P1 · **Priority:** p0-critical · **Effort:** XL · **Depends on:** `epic-p0`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Goal
Close the concrete security holes and data-integrity bugs before any refactor makes diffs larger. Per the critique, this also covers proxy inbound auth, the usage-script sandbox, deeplink-delivered scripts, the OAuth token stores, and a defined DB compatibility/downgrade policy. P1 has no schema-version bump: the tombstone migration moves to P4, after the migration framework.

## Exit criteria
- [ ] The proxy rejects requests with no per-install token, a foreign Host, an Origin header, or a non-JSON body; a non-loopback bind requires a token
- [ ] No HalfOpen permit leak, no double send on post-write errors, and a failed proxy reconfiguration rolls back
- [ ] Usage scripts run with interrupt, memory and stack limits off the tokio workers; deeplink scripts arrive disabled
- [ ] No #[tauri::command] accepts a free-form shell string; listed path-taking commands reject paths outside allowed roots
- [ ] atomic_write fsyncs, creates new secret files 0600, follows symlinks and never deletes the target first; zero non-test std::fs::write
- [ ] SQL import cannot ATTACH or VACUUM INTO; restore validates in a temp DB before touching the main DB
- [ ] Cost golden table fully green, including the grokbuild backfill; a deleted built-in price stays deleted
- [ ] Startup reports journal_mode=wal on local paths; pre-migration backup is consistent and also taken for v0/JSON upgrades
- [ ] Documented downgrade/mixed-version sync rules, with tests
- [ ] Every fix ships with a regression test or golden fixture from the P0 harnesses

## Child tickets
p1-proxy-inbound-auth, p1-proxy-body-limit, p1-proxy-halfopen-permit, p1-proxy-raw-send-no-resend, p1-proxy-config-rollback, p1-proxy-lifecycle-lock, p1-proxy-first-byte-timeout, p1-proxy-raw-body-timeout, p1-proxy-auth-guard-refresh, p1-proxy-mask-url, p1-proxy-connect-status, p1-proxy-connect-userinfo, p1-proxy-connect-ipv6, p1-bridge-tool-result, p1-bridge-image-document, p1-bridge-chat-stream-terminal, p1-bridge-stop-reason-tool-use, p1-bridge-gemini-delta, p1-usage-script-sandbox, p1-usage-script-placeholder-escape, p1-deeplink-script-disabled, p1-deeplink-env-allowlist, p1-deeplink-dialog-guard, p1-deeplink-mask-value, p1-deeplink-confirm-diff, p1-ipc-session-terminal, p1-ipc-path-validation, p1-session-load-messages-roots, p1-csp-tighten, p1-asset-protocol-remove, p1-atomic-write-durable, p1-atomic-write-perms, p1-atomic-write-symlink, p1-route-writes-atomic-lint, p1-oauth-token-store-hardening, p1-settings-atomic-save, p1-settings-save-race, p1-sql-import-sandbox, p1-restore-backup-temp-first, p1-backup-filename-validation, p1-db-premigration-backup, p1-db-wal-busy-timeout, p1-db-compat-policy, p1-usage-input-semantics, p1-usage-session-importer-semantics, p1-pricing-no-reseed, p1-spend-limits-correct, p1-session-sync-partial-line, p1-session-sync-final-entry, p1-switch-provider-off-main, p1-switch-lock-all-apps, p1-subscription-keychain-blocking, p1-skill-zip-extraction, p1-gemini-env-writer, p1-claude-json-optimistic, p1-powershell-quote, p1-fe-jsoneditor-onchange, p1-fe-managed-auth-timers, p1-fe-edit-dialog-reset, p1-fe-mcp-stdio-validation, p1-fe-skills-cache-dedupe
