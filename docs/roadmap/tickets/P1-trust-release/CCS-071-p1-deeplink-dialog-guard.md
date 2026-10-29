# CCS-071: [deeplink] Do not replace the request under an open confirm dialog

**Status:** Done · **Kind:** security · **Labels:** `security`, `phase:P1`, `p3-low`, `area:deeplink`

> **Ticket key:** `p1-deeplink-dialog-guard` · **Phase:** P1 · **Priority:** p3-low · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding frontend-deeplink-request-replaced, partially real. A new deeplink-import event swaps `request` while the dialog is open, so the user can confirm content they never reviewed. The async merge race is not realistic today.

## Evidence
- src/components/DeepLinkImportDialog.tsx:53-77, 94-100; src-tauri/src/lib.rs:154

## Proposed change
- Ignore or queue incoming requests while isOpen or isImporting (use refs), and show a toast.
- Add a sequence number for when remote configUrl fetching arrives.

## Acceptance criteria / verification
- [x] Vitest with a mocked `listen`: emit A, then B; the dialog still shows A; Import calls importFromDeeplink with A
- [x] `pnpm test:unit`

## Dependencies
None.

## Implementation notes

**Summary.** `DeepLinkImportDialog` no longer swaps `request` under an open
confirm dialog. The long-lived `deeplink-import` listener reads
`isOpenRef` / `isImportingRef` / `isMergingRef` (refs mirroring state, so the
listener never sees stale values) and, while any is set, ignores the new
request and shows a `deeplink.requestIgnoredBusy` warning toast. Each
accepted request takes a sequence number (`requestSeqRef`); a
`mergeDeeplinkConfig` result (e.g. a slow remote `configUrl` fetch) is only
applied if it still matches the latest sequence, and the effect cleanup bumps
the sequence so late merges after unmount are dropped. `handleImport` also
guards against double submission via `isImportingRef`. New i18n keys
`deeplink.requestIgnoredBusy` / `deeplink.requestIgnoredBusyDescription` in
en / zh / zh-TW / ja.

**Files.**
- `src/components/DeepLinkImportDialog.tsx`
- `src/i18n/locales/{en,zh,zh-TW,ja}.json`
- `tests/components/DeepLinkImportDialog.test.tsx`

**Commits.** `9ad078d` (fix), plus the commit that records these notes.

**Verification.**
- New tests in `tests/components/DeepLinkImportDialog.test.tsx` (mocked
  `listen` via `emitTauriEvent`): (1) emit A, then B while open: dialog still
  shows A, warning toast fires, Import sends A to
  `import_from_deeplink_unified`; (2) B arrives while A's config merge is
  pending: B is ignored, A shows and is imported; (3) after Cancel a new
  deeplink is accepted with no warning.
- Regression check: against the previous component, tests (1) and (2) fail
  (2 failed / 4 passed); with the fix, 6/6 pass.
- `pnpm typecheck`: clean.
- `pnpm test:unit`: 120 files, 1235 tests passed.
- `pnpm format:check`: all matched files use Prettier code style.
