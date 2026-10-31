# CCS-239: [sync] Reject signed sync manifests older than the last applied one

**Status:** Done · **Kind:** security · **Labels:** `security`, `phase:P4`, `p3-low`, `area:sync`

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
- [x] New `sync_protocol` tests: an older signed manifest is rejected; an equal or newer one is accepted; clearing the key resets.
- [x] `cargo test --manifest-path src-tauri/Cargo.toml sync_protocol`, `cargo clippy --all-targets -- -D warnings` and `cargo fmt --check` pass.

## Dependencies
None.

## Implementation notes
- **Summary:** `WebDavSyncStatus` (used by WebDAV and S3) gains the backend-owned
  `lastSignedManifestCreatedAt`. After a signed download is applied,
  `record_applied_manifest` stores the manifest's `createdAt` (only signed imports with
  a readable RFC 3339 timestamp; unsigned migration imports leave it unset).
  `SignaturePolicy::for_status` turns it into a rollback floor, and `authorize_manifest`
  refuses a verified signed manifest whose `createdAt` is older (or unreadable once a
  floor exists) with `sync.manifest_rollback_rejected`; equal or newer timestamps are
  accepted, timezone offsets are compared as instants, and an unreadable stored floor
  is ignored with a warning. `set_sync_snapshot_key(None)` resets the floor next to
  `signedSync`, and `merge_settings_for_save` keeps the stored value. The frontend maps
  the new error key to `settings.webdavSync.signature.errors.manifestRollbackRejected`
  (en/zh/zh-TW/ja). `docs/dev/SECRET_STORAGE.md` documents it, including that only
  devices that already applied a signed snapshot are protected (uploads do not set the
  floor) and that the check relies on the uploading devices' clocks.
- **Files:** `src-tauri/src/services/sync_protocol.rs`, `src-tauri/src/services/webdav_sync.rs`,
  `src-tauri/src/services/s3_sync.rs`, `src-tauri/src/settings.rs`,
  `src-tauri/src/commands/settings.rs`, `src/components/settings/WebdavSyncSection.tsx`,
  `src/types.ts`, `src/i18n/locales/{en,zh,zh-TW,ja}.json`,
  `tests/components/WebdavSyncSection.test.tsx`, `tests/msw/state.ts`,
  `docs/dev/SECRET_STORAGE.md`.
- **Commits:** d41d589 (code, tests, docs), plus this ticket-file commit.
- **Tests:** new `sync_protocol` tests `older_signed_manifest_is_rejected_as_rollback`
  (fails without the check: the older signed manifest was accepted),
  `equal_or_newer_signed_manifest_is_accepted`,
  `signed_manifest_with_unreadable_created_at_is_rejected_once_a_floor_exists`,
  `unsigned_import_does_not_set_the_rollback_floor`,
  `clearing_the_rollback_floor_accepts_older_signed_manifests_again`; extended
  `sync_snapshot_key_is_stored_outside_settings_and_clearing_resets_signed_sync`
  (clearing the key resets the floor on disk) and
  `save_settings_should_keep_backend_signed_sync_flag` (save keeps the floor); new vitest
  case localizing `sync.manifest_rollback_rejected`.
- **Verification:**
  - `cargo test --manifest-path src-tauri/Cargo.toml --lib sync_protocol`: 42 passed, 0 failed.
  - `cargo fmt --manifest-path src-tauri/Cargo.toml --check`: clean.
  - `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`: clean.
  - `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast`: lib 2860 passed / 0 failed
    (5 ignored); all integration test binaries passed (0 failed).
  - `pnpm typecheck`: clean; `pnpm format:check`: clean; `pnpm test:unit`: 173 files, 1668 tests passed.
