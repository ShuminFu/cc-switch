# CCS-035: [deeplink] Stop leaking raw deeplink URLs to logs and to the webview console

**Status:** Done · **Kind:** security · **Labels:** `security`, `phase:P0`, `p1-high`, `area:deeplink`

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
- [x] Unit test: the error payload built from `...&apiKey=sk-secret` has no `sk-secret`
- [x] Vitest: DeepLinkImportDialog never console.errors the secret
- [x] `grep -n 'url_str}\|{url}' src-tauri/src/lib.rs src-tauri/src/commands/deeplink.rs` finds nothing
- [x] `cargo test --manifest-path src-tauri/Cargo.toml deeplink` and `pnpm test:unit`

## Dependencies
None.

## Implementation notes
**Summary**
- New `src-tauri/src/deeplink/redact.rs`: `redact_url_for_log` (moved from `lib.rs`, now `pub(crate)` and re-exported from `crate::deeplink`) plus `deeplink_error_payload(url, error)`, which builds the `deeplink-error` payload with only the redacted URL.
- `lib.rs`: removed the raw `log::debug!` of the URL; every deeplink log line uses `crate::deeplink::redact_url_for_log`; `handle_deeplink_url` emits `deeplink_error_payload`; the macOS `RunEvent::Opened` handler now calls `handle_deeplink_url(.., true, "RunEvent::Opened")` instead of its own copy of the parse/emit logic. It still exits lightweight mode first. The window is now focused only when parsing succeeds, the same as the other entry points.
- Removed the unused `parse_deeplink` command, its registration, and `deeplinkApi.parseDeeplink`.
- `DeepLinkImportDialog.tsx` logs only `event.payload.error`.

**Files:** `src-tauri/src/deeplink/redact.rs` (new), `src-tauri/src/deeplink/mod.rs`, `src-tauri/src/lib.rs`, `src-tauri/src/commands/deeplink.rs`, `src/lib/api/deeplink.ts`, `src/components/DeepLinkImportDialog.tsx`, `tests/components/DeepLinkImportDialog.test.tsx` (new).

**Commits:** `25d5ff7` (code + tests); the ticket update is in a follow-up commit.

**Verification (Linux)**
- `cargo test --manifest-path src-tauri/Cargo.toml --lib deeplink`: 40 passed, including the new `deeplink::redact::tests::{error_payload_does_not_contain_secret, redact_url_keeps_keys_but_drops_values, redact_unparseable_url_drops_query}`.
- `cargo test --manifest-path src-tauri/Cargo.toml deeplink`: all passed (lib 40, tests/deeplink_import.rs 2).
- `cargo test --manifest-path src-tauri/Cargo.toml --lib`: 2010 passed, 0 failed, 2 ignored.
- `cargo fmt` clean; `cargo clippy -- -D warnings` clean.
- `grep -n 'url_str}\|{url}' src-tauri/src/lib.rs src-tauri/src/commands/deeplink.rs`: no matches.
- New vitest `tests/components/DeepLinkImportDialog.test.tsx`: fails against the old `console.error(event.payload)` ("expected ... not to contain 'sk-secret'") and passes after the fix.
- `pnpm test:unit`: 74 files / 467 tests passed; `pnpm typecheck` clean; `pnpm format:check` clean.
- Not compiled here: the `RunEvent::Opened` block is `#[cfg(target_os = "macos")]`, so it was not built on this Linux host. It only calls `handle_deeplink_url` and `crate::deeplink::redact_url_for_log`, which are not cfg-gated. The macOS CI job should confirm it builds.
