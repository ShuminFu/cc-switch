# CCS-237: [sync] Add the sync-key field to sync settings and expose signature/allowUnsigned in the API layer

**Status:** Done · **Kind:** feature · **Labels:** `feature`, `phase:P4`, `p2-medium`, `area:sync`

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
- [x] New cases in `tests/components/WebdavSyncSection.test.tsx`: status renders; Set calls `invoke('set_sync_snapshot_key', {key})`; Clear sends `null`.
- [x] `pnpm typecheck`, `pnpm vitest run tests/components/WebdavSyncSection.test.tsx tests/msw/handlers.test.ts` and `pnpm format:check` pass.

## Dependencies
None.

## Implementation notes

**Summary.** The snapshot signing key from CCS-173 is now usable from the UI, and the API layer carries the signature fields:
- `src/types.ts`: new `RemoteSnapshotSignature` (`valid | invalid | unsigned | keyMissing`, matching `manifest_signature_status`), required `signature` on `RemoteSnapshotInfo`, `signedSync?` on `WebDavSyncStatus`. `src/shared/bindings.ts` has no generated types for these (the sync commands return `serde_json::Value`), so they stay hand-written.
- `src/lib/api/settings.ts`: `webdavSyncDownload(options?)` / `s3SyncDownload(options?)` take `{ allowUnsigned?: boolean }` and always send `allowUnsigned` (default `false`, same as the backend default); `WebDavSyncResult.signed?`.
- New `src/components/settings/SyncSnapshotKeyField.tsx`, rendered once in `WebdavSyncSection` above the WebDAV/S3 selector: react-query status (`configured` + key ID, or "not set"), password input with a min-8-char check (trimmed character count, same rule as `SyncKey::from_passphrase`), Set, and Clear behind a `ConfirmDialog` that warns it also resets signedSync. Clearing invalidates the `["settings"]` query so the reset status is picked up.
- i18n `settings.webdavSync.syncKey.*` in en/zh/zh-TW/ja.
- MSW: handlers for `get_sync_snapshot_key_status` / `set_sync_snapshot_key` (state in `tests/msw/state.ts`, reset per test; short keys are rejected and clearing resets `signedSync` like the backend); both removed from `tests/msw/unmockedCommands.ts`.
- Out of scope (not in this ticket): wiring the `allowUnsigned` one-time-import prompt and showing `signature` in the download confirmation dialog; the API/types now support it.

**Files.** `src/components/settings/SyncSnapshotKeyField.tsx` (new), `src/components/settings/WebdavSyncSection.tsx`, `src/lib/api/settings.ts`, `src/types.ts`, `src/i18n/locales/{en,zh,zh-TW,ja}.json`, `tests/components/WebdavSyncSection.test.tsx`, `tests/lib/syncApi.test.ts` (new), `tests/msw/{handlers.ts,handlers.test.ts,state.ts,unmockedCommands.ts}`.

**Commits.** 3cafa33 (implementation + tests); this ticket update in a follow-up commit.

**Verification.**
- `pnpm vitest run tests/components/WebdavSyncSection.test.tsx tests/msw/handlers.test.ts tests/lib/syncApi.test.ts` → 3 files, 33 tests passed (new: not-set status renders and `invoke("get_sync_snapshot_key_status")`; configured status shows key ID; <8 chars blocks Set without invoking; Set calls `invoke("set_sync_snapshot_key", { key })`; Clear confirms then sends `{ key: null }`; MSW set/get/clear round-trip and short-key rejection; both download wrappers send `allowUnsigned` false by default and forward `true`).
- `pnpm typecheck` → clean. `pnpm format:check` → all files formatted. `npx eslint` on the touched files → clean.
- `pnpm test:unit` → 173 files, 1652 tests passed.
- No Rust changes.
