# CCS-173: [sync] Authenticate WebDAV/S3 snapshots with an HMAC and refuse unsigned remote imports

**Status:** Open · **Kind:** feature · **Labels:** `feature`, `phase:P4`, `p2-medium`, `area:sync`

> **Ticket key:** `p4-sync-snapshot-auth` · **Phase:** P4 · **Priority:** p2-medium · **Effort:** M · **Depends on:** `p1-sql-import-sandbox`, `p4-keychain-secrets`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Backend idea 5. The sync SHA256 comes from the same remote, so a compromised endpoint can push any snapshot.

## Proposed change
- HMAC each manifest with a sync key the user enters on each device, stored in the SecretStore.
- Refuse imports that are unsigned or carry an invalid signature, with a one-time migration prompt.

## Acceptance criteria / verification
- [ ] Tests: a tampered manifest is rejected; a correctly signed one is accepted
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml sync_protocol`

## Dependencies
p1-sql-import-sandbox, p4-keychain-secrets
