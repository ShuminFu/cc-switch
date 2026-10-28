# CCS-172: [security] Store secrets in the OS keychain or credential manager

**Status:** Open · **Kind:** feature · **Labels:** `feature`, `phase:P4`, `p2-medium`, `area:security`

> **Ticket key:** `p4-keychain-secrets` · **Phase:** P4 · **Priority:** p2-medium · **Effort:** L · **Depends on:** `p1-oauth-token-store-hardening`, `p4-per-app-settings-state`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Backend idea 6 plus the critique on the OAuth token stores. WebDAV/S3 passwords, provider API keys and OAuth tokens sit in plaintext in settings.json, SQLite and token JSON files.

## Proposed change
- Use the keyring crate behind a SecretStore trait, with a file fallback (0600) on headless Linux.
- Migrate the WebDAV/S3 secrets and the Copilot/Codex tokens first.
- Keep provider API keys in the live config files, since the CLIs need them.

## Acceptance criteria / verification
- [ ] Tests with a mock SecretStore: migration moves secrets and blanks the settings fields
- [ ] A downgrade path is documented
- [ ] Manual check on macOS, Windows and Linux
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml secret_store`

## Dependencies
p1-oauth-token-store-hardening, p4-per-app-settings-state
