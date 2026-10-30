# CCS-238: [sync] Prompt before applying unsigned snapshots and block tampered ones, with i18n for sync.* errors

**Status:** Done · **Kind:** feature · **Labels:** `feature`, `phase:P4`, `p2-medium`, `area:sync`

> **Ticket key:** `p4-sync-unsigned-migration-prompt` · **Phase:** P4 · **Priority:** p2-medium · **Effort:** M · **Depends on:** `p4-sync-key-field-api`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)
> Found during the Partial-ticket triage at d962272 (CCS-173, CCS-005). Source tickets: [CCS-173](../P4-app-adapters-persistence/CCS-173-p4-sync-snapshot-auth.md), [CCS-005](../P4-app-adapters-persistence/CCS-005-epic-p4.md).

## Problem
`handleDownloadClick` (~594) and its S3 twin (~816) in `WebdavSyncSection.tsx` only check `info.compatible`. With a key configured, an unsigned remote snapshot is refused by the backend with no migration path, and tampered or key-missing snapshots produce raw backend errors (AppError::localized provides only zh/en for `sync.key_missing`, `sync.manifest_key_mismatch`, `sync.manifest_signature_invalid`, `sync.manifest_signature_unsupported`, `sync.manifest_unsigned`, `sync.manifest_unsigned_rejected`).

## Proposed change
- Branch on `info.signature` before download (and in the confirm dialogs ~1631 / ~1771):
  - `invalid`: error toast/dialog about tampering or key mismatch; no download offered.
  - `keyMissing`: ask the user to enter the sync key first.
  - `unsigned` with a key configured: one-time migration confirmation (`DialogType` gains `'migrate_unsigned'`, ~187); confirm calls the download with `{ allowUnsigned: true }`.
  - `unsigned` without a key: normal dialog, optional hint.
- Map the six `sync.*` error keys to strings in all four locales.

## Acceptance criteria / verification
- [x] Tests in `tests/components/WebdavSyncSection.test.tsx` mock remote info for each signature value and assert the dialog/toast; confirming migration calls `invoke('webdav_sync_download', {allowUnsigned: true})` and the s3 equivalent.
- [x] `pnpm typecheck`, `pnpm test:unit`, the tests/i18n parity test and `pnpm format:check` pass.

## Dependencies
CCS-237 (`p4-sync-key-field-api`).

## Implementation notes

**Summary.** `WebdavSyncSection` now resolves a download gate from `info.signature` (plus the sync-key status from the shared `SYNC_SNAPSHOT_KEY_STATUS_QUERY_KEY` query and `status.signedSync`) before opening the WebDAV or S3 restore dialog:

- `invalid`: error toast (`settings.webdavSync.signature.invalid`, tampered or other key); no dialog, no download.
- `keyMissing`: warning toast asking to enter the sync key first; no download.
- `unsigned` + key configured: `DialogType` gains `'migrate_unsigned'`; the restore dialog shows the migration title/message and an "Import once" button that calls `webdav_sync_download` / `s3_sync_download` with `{ allowUnsigned: true }`.
- `unsigned` + no key: the normal dialog with an unsigned hint (`allowUnsigned: false`).
- `unsigned` after a signed sync (`signedSync`): refused up front like the backend does (downgrade error with a key, key prompt without one).
- `valid` / absent: unchanged.

Download failures carrying one of the six backend `sync.*` `i18nKey`s (`sync.key_missing`, `sync.manifest_key_mismatch`, `sync.manifest_signature_invalid`, `sync.manifest_signature_unsupported`, `sync.manifest_unsigned`, `sync.manifest_unsigned_rejected`) are shown with `settings.webdavSync.signature.errors.*` strings, added to en/zh/zh-TW/ja; other errors keep the backend message.

**Files.** `src/components/settings/WebdavSyncSection.tsx`, `src/i18n/locales/{en,zh,zh-TW,ja}.json`, `tests/components/WebdavSyncSection.test.tsx`.

**Commits.** c8f782c (feature + tests + locales); this ticket-file update follows in a docs commit.

**Verification.**
- New `remote snapshot signature (CCS-238)` block in `tests/components/WebdavSyncSection.test.tsx`, run for both WebDAV and S3 (`describe.each`), 14 tests: valid, invalid, keyMissing, unsigned without key (hint), unsigned with key (migration dialog -> `invoke("webdav_sync_download"|"s3_sync_download", { allowUnsigned: true })`), downgrade after signed sync, and localization of a `LOCALIZED` `sync.manifest_signature_invalid` download error. Against the previous component 12 of them fail; with the change all pass.
- `pnpm typecheck`: pass. `pnpm format:check`: pass.
- `pnpm test:unit`: 173 files / 1666 tests passed (includes tests/i18n locale parity, missing/used/typed keys).
- No Rust changes (backend keys already existed).
