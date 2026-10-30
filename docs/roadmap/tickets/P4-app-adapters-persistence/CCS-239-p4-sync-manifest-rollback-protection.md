# CCS-239: [sync] Reject signed sync manifests older than the last applied one

**Status:** Open · **Kind:** security · **Labels:** `security`, `phase:P4`, `p3-low`, `area:sync`

> **Ticket key:** `p4-sync-manifest-rollback-protection` · **Phase:** P4 · **Priority:** p3-low · **Effort:** M
> Plan: [next-phase-plan.md](../../next-phase-plan.md)
> Found during the Partial-ticket triage at d962272 (CCS-173, CCS-005). Source tickets: [CCS-173](../P4-app-adapters-persistence/CCS-173-p4-sync-snapshot-auth.md), [CCS-005](../P4-app-adapters-persistence/CCS-005-epic-p4.md).

## Problem
A re-uploaded older signed manifest still verifies, so an attacker with write access to the remote can roll a device back to an older snapshot signed with the same key. CCS-173 lists rollback protection under Remaining hardening.

## Proposed change
- Persist the last applied signed manifest's `createdAt` (or snapshotId) in the transport status next to `signedSync` (WebDAV and S3) on successful download.
- In `authorize_manifest` (`src-tauri/src/services/sync_protocol.rs:433`) or the download paths, reject an older signed manifest with a new localized key such as `sync.manifest_rollback_rejected`. Clearing the key resets the stored value.
- `merge_settings_for_save` keeps this backend-owned field, as it does `signedSync`.
- Only devices that have applied a snapshot before are protected; note this in docs.

## Acceptance criteria / verification
- [ ] New `sync_protocol` tests: an older signed manifest is rejected; an equal or newer one is accepted; clearing the key resets.
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml sync_protocol`, `cargo clippy --all-targets -- -D warnings` and `cargo fmt --check` pass.

## Dependencies
None.
