# CCS-173: [sync] Authenticate WebDAV/S3 snapshots with an HMAC and refuse unsigned remote imports

**Status:** Partial · **Kind:** feature · **Labels:** `feature`, `phase:P4`, `p2-medium`, `area:sync`

> **Ticket key:** `p4-sync-snapshot-auth` · **Phase:** P4 · **Priority:** p2-medium · **Effort:** M · **Depends on:** `p1-sql-import-sandbox`, `p4-keychain-secrets`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Backend idea 5. The sync SHA256 comes from the same remote, so a compromised endpoint can push any snapshot.

## Proposed change
- HMAC each manifest with a sync key the user enters on each device, stored in the SecretStore.
- Refuse imports that are unsigned or carry an invalid signature, with a one-time migration prompt.

## Acceptance criteria / verification
- [x] Tests: a tampered manifest is rejected; a correctly signed one is accepted
- [x] `cargo test --manifest-path src-tauri/Cargo.toml sync_protocol`

## Dependencies
p1-sql-import-sandbox, p4-keychain-secrets

## Implementation notes

**Status Partial:** the backend is complete and the acceptance tests pass. The UI is not
done (it belongs to the frontend lane): a sync-key input that calls
`set_sync_snapshot_key`, and the one-time migration prompt. Until that ships, no device
has a key, so sync works exactly as before (unsigned).

**Summary**
- `services/sync_protocol.rs`: `SyncKey` is PBKDF2-HMAC-SHA256 of the passphrase (600k
  rounds, fixed salt, at least 8 chars). Its `Debug` output hides the key, and `key_id` is a
  public fingerprint. `SyncManifest.signature` is an optional `{alg: "HMAC-SHA256", keyId, mac}`
  that older apps ignore. The MAC covers a domain tag plus the re-serialized manifest
  without its signature, so unknown JSON fields cannot change what is verified.
  `authorize_manifest` decides whether to import. `upload_signing_key` and
  `manifest_signature_status` cover upload and preview. `build_local_snapshot` signs
  when a key is set.
- How imports are decided:
  - Signed manifest: verified with the local key. These are always refused:
    - no local key (`sync.key_missing`)
    - a different key (`sync.manifest_key_mismatch`)
    - an unsupported alg
    - a bad or malformed MAC (`sync.manifest_signature_invalid`)
  - Unsigned manifest with a key set: refused with `sync.manifest_unsigned` unless the
    download is called with `allowUnsigned: true`. That call is the one-time migration.
  - Downgrade guard: after any signed sync or confirmed migration, the transport's
    `status.signedSync` is set. From then on unsigned manifests are always refused
    (`sync.manifest_unsigned_rejected`), and so are unsigned uploads without a key.
  - No key and no signed history: unsigned import works as before.
  - Clearing the key resets `signedSync`.
- WebDAV/S3 `download` checks the signature before validating compatibility or fetching any
  artifact. It takes `allow_unsigned`, and the commands take an optional `allowUnsigned`.
  Uploads are signed. Remote info gains `signature` (`valid`/`invalid`/`unsigned`/`keyMissing`),
  and the download result gains `signed`.
- Key storage: `settings::{get,set}_sync_snapshot_key` keep the derived key (hex) in the
  SecretStore under `settings.sync.snapshot_key`, never in settings.json. New commands
  `get_sync_snapshot_key_status` and `set_sync_snapshot_key` return `{configured, keyId}`.
  `SecretPolicy::store()` is no longer test-only.
- New dependency: `pbkdf2 0.12` (hmac feature). It was already in Cargo.lock through zip.
- Documented in `docs/dev/SECRET_STORAGE.md`.

**Files:** src-tauri/Cargo.toml, src-tauri/Cargo.lock, src-tauri/src/services/sync_protocol.rs,
src-tauri/src/services/webdav_sync.rs, src-tauri/src/services/s3_sync.rs, src-tauri/src/settings.rs,
src-tauri/src/secret_store.rs, src-tauri/src/commands/{settings,webdav_sync,s3_sync}.rs,
src-tauri/src/lib.rs, src/shared/bindings.ts (regenerated), tests/msw/unmockedCommands.ts,
docs/dev/SECRET_STORAGE.md.

**Commits:** 71a94b1 (feature), plus this ticket-notes commit.

**Verification**
- `cargo test --manifest-path src-tauri/Cargo.toml sync_protocol`: 37 passed. This includes
  `tampered_manifest_is_rejected` (hash, size, added artifact, device, createdAt,
  snapshotId, dbCompat, flipped or malformed MAC) and
  `signed_manifest_is_accepted_after_a_json_round_trip`, plus tests for key mismatch, a
  forged key id, migration, the downgrade guard, the legacy path, key derivation and
  `build_local_snapshot` signing.
- The settings tests `sync_snapshot_key_is_stored_outside_settings_and_clearing_resets_signed_sync`
  and `corrupted_sync_snapshot_key_is_an_error_not_a_missing_key` pass.
- `cargo fmt --check`: clean.
- `cargo clippy --all-targets -- -D warnings`: clean.
- `cargo test --no-fail-fast` (full suite, all targets): 2697 passed, 0 failed (lib: 2499 passed, 3 ignored).
- `UPDATE_BINDINGS=1 cargo test --lib bindings`: regenerated, up to date.
- `pnpm typecheck`: clean.
- `vitest tests/msw/handlers.test.ts`: 4 passed.
- prettier check on the touched TS: clean.

**Review**
- Re-checked both acceptance criteria against the code: `tampered_manifest_is_rejected` and
  `signed_manifest_is_accepted_after_a_json_round_trip` assert the right error keys, and
  WebDAV/S3 `download` authorize the manifest before any artifact is fetched.
- Fixed: a full `save_settings` (`merge_settings_for_save`) took `webdavSync`/`s3Sync.status`
  from the frontend payload, so a stale or field-stripped copy (e.g. `UsageScriptModal`
  and `FirstRunNoticeDialog` drop only `webdavSync` and send `s3Sync` back) could reset
  `signedSync` and silently lift the downgrade guard. The merge now always keeps the
  backend value. Test: `save_settings_should_keep_backend_signed_sync_flag`.
- Status stays Partial: the sync-key UI and the migration prompt are still missing.
- More notes for later: the signature does not bind the remote location or profile, so a
  signed snapshot from another profile with the same key would verify, which is the same
  class of problem as rollback. Also, a future manifest field added to `SyncManifest` will
  make older apps fail the MAC check, so treat any new field as a protocol version bump.

**Remaining / follow-ups**
- Frontend:
  - add a sync-key field in WebdavSyncSection (one key shared by WebDAV and S3), using
    `get_sync_snapshot_key_status` and `set_sync_snapshot_key`;
  - before downloading, use remote info `signature` to show the migration prompt
    (`unsigned`) or a tamper/key warning (`invalid`/`keyMissing`), then call
    `webdav_sync_download`/`s3_sync_download` with `allowUnsigned: true` after the user
    confirms;
  - add i18n strings for the new `sync.*` error keys.
- Possible hardening:
  - make the key mandatory once the UI ships;
  - add rollback protection: a re-uploaded older signed manifest still verifies, so reject
    `createdAt` older than the last applied one.

### Remaining after triage

- Blocked: Make the sync key mandatory (refuse unsigned uploads/imports with no key). Needs a product decision after the key UI ships in a release.
- Blocked: Bind the signature to the remote location/profile. Needs a protocol decision and a manifest version bump.
- Follow-up [CCS-237](../P4-app-adapters-persistence/CCS-237-p4-sync-key-field-api.md): Add the sync-key field to sync settings and expose signature/allowUnsigned in the API layer.
- Follow-up [CCS-238](../P4-app-adapters-persistence/CCS-238-p4-sync-unsigned-migration-prompt.md): Prompt before applying unsigned snapshots and block tampered ones, with i18n for sync.* errors.
- Follow-up [CCS-239](../P4-app-adapters-persistence/CCS-239-p4-sync-manifest-rollback-protection.md): Reject signed sync manifests older than the last applied one.

## Owner decision: GitHub Actions checks skipped

By the repository owner's decision on 2026-10-31, checks that need a GitHub Actions run are skipped (not verified). None of this ticket's remaining items depend only on GitHub Actions, so that decision does not change its status.

### Still blocked by

- Make the sync key mandatory (refuse unsigned uploads/imports when no key is set). **Needs:** Product decision, to be made after the sync-key UI has shipped in a release
- Bind the HMAC signature to the remote location/profile. **Needs:** Protocol decision plus a manifest version bump (maintainer decision)
