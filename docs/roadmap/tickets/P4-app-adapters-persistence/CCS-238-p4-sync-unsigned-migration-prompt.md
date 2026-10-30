# CCS-238: [sync] Prompt before applying unsigned snapshots and block tampered ones, with i18n for sync.* errors

**Status:** Open · **Kind:** feature · **Labels:** `feature`, `phase:P4`, `p2-medium`, `area:sync`

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
- [ ] Tests in `tests/components/WebdavSyncSection.test.tsx` mock remote info for each signature value and assert the dialog/toast; confirming migration calls `invoke('webdav_sync_download', {allowUnsigned: true})` and the s3 equivalent.
- [ ] `pnpm typecheck`, `pnpm test:unit`, the tests/i18n parity test and `pnpm format:check` pass.

## Dependencies
CCS-237 (`p4-sync-key-field-api`).
