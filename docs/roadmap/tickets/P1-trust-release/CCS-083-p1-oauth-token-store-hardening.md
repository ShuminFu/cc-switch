# CCS-083: [auth] Harden the Copilot and Codex OAuth token stores

**Status:** Open · **Kind:** security · **Labels:** `security`, `phase:P1`, `p2-medium`, `area:auth`

> **Ticket key:** `p1-oauth-token-store-hardening` · **Phase:** P1 · **Priority:** p2-medium · **Effort:** M · **Depends on:** `p1-atomic-write-perms`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
The critique found that copilot_auth.rs (2083 lines) and codex_oauth_auth.rs (1127 lines) keep plaintext token JSON, each with its own atomic writer. They have no Windows equivalent of 0600, no fsync, and are not covered by the lint or the plan's catalog.

## Evidence
- src-tauri/src/proxy/providers/copilot_auth.rs:1247-1294 (:1273 0600); codex_oauth_auth.rs:811

## Proposed change
- Replace both writers with a shared `atomic_write_secret` (0600, fsync, symlink-safe).
- On Windows, set an owner-only ACL on the file.
- Add the token files to the config-writer snapshots.
- The keychain migration is tracked in p4-keychain-secrets.

## Acceptance criteria / verification
- [ ] Unix test: the token file is 0600 after first write and after rewrite
- [ ] Windows CI test: the ACL has only the current user
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml copilot_auth codex_oauth`

## Dependencies
p1-atomic-write-perms
