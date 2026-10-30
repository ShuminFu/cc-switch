# CCS-237: [sync] Add the sync-key field to sync settings and expose signature/allowUnsigned in the API layer

**Status:** Open · **Kind:** feature · **Labels:** `feature`, `phase:P4`, `p2-medium`, `area:sync`

> **Ticket key:** `p4-sync-key-field-api` · **Phase:** P4 · **Priority:** p2-medium · **Effort:** M
> Plan: [next-phase-plan.md](../../next-phase-plan.md)
> Found during the Partial-ticket triage at d962272 (CCS-173, CCS-005). Source tickets: [CCS-173](../P4-app-adapters-persistence/CCS-173-p4-sync-snapshot-auth.md), [CCS-005](../P4-app-adapters-persistence/CCS-005-epic-p4.md).

## Problem
CCS-173 shipped backend snapshot signing, but the UI cannot use it:
- `settingsApi.getSyncSnapshotKeyStatus()` / `setSyncSnapshotKey(key|null)` (`src/lib/api/settings.ts:272-279`, type at :39) have no caller; `WebdavSyncSection.tsx` (1868 lines) has no key UI.
- `RemoteSnapshotInfo` (`src/types.ts:328`) lacks the `signature` field the backend returns (`webdav_sync.rs:200-210`, `s3_sync.rs`).
- `webdavSyncDownload()` / `s3SyncDownload()` (`settings.ts:216`, `:252`) cannot pass `allowUnsigned` (commands take `Option<bool>`: `commands/webdav_sync.rs:117`, `s3_sync.rs:109`); `WebDavSyncResult` (:64) lacks `signed`.
- `get_sync_snapshot_key_status` / `set_sync_snapshot_key` are listed in `tests/msw/unmockedCommands.ts`.

## Proposed change
- Types/API: add `signature: 'valid' | 'invalid' | 'unsigned' | 'keyMissing'` to `RemoteSnapshotInfo`, optional `{ allowUnsigned?: boolean }` to both download wrappers, `signed?: boolean` to the result; prefer `src/shared/bindings.ts` types where they exist.
- New `src/components/settings/SyncSnapshotKeyField.tsx`, rendered once above the WebDAV/S3 switch: react-query status ("configured / keyId" or "not set"), password input with a min-8-char check, Set and Clear (Clear confirms and warns that it resets signedSync).
- i18n `settings.webdavSync.syncKey.*` in en/zh/zh-TW/ja.
- MSW handlers for both commands; remove them from `unmockedCommands.ts`.

## Acceptance criteria / verification
- [ ] New cases in `tests/components/WebdavSyncSection.test.tsx`: status renders; Set calls `invoke('set_sync_snapshot_key', {key})`; Clear sends `null`.
- [ ] `pnpm typecheck`, `pnpm vitest run tests/components/WebdavSyncSection.test.tsx tests/msw/handlers.test.ts` and `pnpm format:check` pass.

## Dependencies
None.
