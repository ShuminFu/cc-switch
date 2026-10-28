# CCS-035: [deeplink] Stop leaking raw deeplink URLs to logs and to the webview console

**Status:** Open · **Kind:** security · **Labels:** `security`, `phase:P0`, `p1-high`, `area:deeplink`

> **Ticket key:** `p0-hotfix-deeplink-redaction` · **Phase:** P0 · **Priority:** p1-high · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Covers verified findings backend-deeplink-log-secrets and frontend-deeplink-log-secret. Raw ccswitch:// URLs carrying apiKey, usageApiKey and usageAccessToken reach the log file at debug level and the devtools console. The RunEvent::Opened path skips the shared handler.

## Evidence
- src-tauri/src/commands/deeplink.rs:11 logs the raw URL at info (the command is unused by the frontend)
- src-tauri/src/lib.rs:143 logs the raw URL at debug; lib.rs:173-181 and :1643-1650 emit `{url: raw}` in deeplink-error
- lib.rs:1611-1650 RunEvent::Opened does not route through handle_deeplink_url (lib.rs:130-150)
- src/components/DeepLinkImportDialog.tsx:82 console.error(event.payload)

## Proposed change
- Make redact_url_for_log pub(crate) in crate::deeplink and use it everywhere.
- Remove the raw debug log.
- Emit only redacted URLs, or none.
- Route RunEvent::Opened through handle_deeplink_url.
- Have the frontend log payload.error only.
- Delete the unused parse_deeplink command and its TS wrapper.

## Acceptance criteria / verification
- [ ] Unit test: the error payload built from `...&apiKey=sk-secret` has no `sk-secret`
- [ ] Vitest: DeepLinkImportDialog never console.errors the secret
- [ ] `grep -n 'url_str}\|{url}' src-tauri/src/lib.rs src-tauri/src/commands/deeplink.rs` finds nothing
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml deeplink` and `pnpm test:unit`

## Dependencies
None.
