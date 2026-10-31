# CCS-225: [settings-ui] Show a startup notice and confirm flow when settings.json failed to load

**Status:** Done · **Kind:** bug · **Labels:** `bug`, `phase:P1`, `p2-medium`, `area:settings-ui`

> **Ticket key:** `p1-settings-load-failure-notice` · **Phase:** P1 · **Priority:** p2-medium · **Effort:** M
> Plan: [next-phase-plan.md](../../next-phase-plan.md)
> Found during the Partial-ticket triage at d962272 (CCS-084, CCS-002). Source tickets: [CCS-084](../P1-trust-release/CCS-084-p1-settings-atomic-save.md), [CCS-002](../P1-trust-release/CCS-002-epic-p1.md).

## Problem
CCS-084 added `get_settings_load_failure` / `acknowledge_settings_load_failure` (`src-tauri/src/commands/settings.rs:88-98`, registered in `commands/registry.rs:42-43`) and the wrappers `settingsApi.getLoadFailure()` / `acknowledgeLoadFailure()` (`src/lib/api/settings.ts:56-91`), but nothing in `src/` calls them and there are no i18n strings. When settings.json is corrupt the user gets defaults with no explanation, and when no backup could be made, saving is blocked (`blocks_save`, settings.rs:913) with no way to understand or confirm it.

## Proposed change
- In `src/hooks/useAppShellEvents.ts`, next to the checkMigration/checkSkillsMigration/checkStartupReport effects (~137-200), add a `checkSettingsLoadFailure` effect.
- `backupPath` non-null: persistent `toast.warning` naming the corrupt file and the backup path (closeButton, long duration).
- `backupPath` null: a ConfirmDialog (existing pattern, e.g. `UniversalProviderPanel.tsx`) explaining that saving is blocked, showing path and error; confirm calls `settingsApi.acknowledgeLoadFailure()` then invalidates the settings query.
- i18n keys (e.g. `settings.loadFailure.backedUp`, `.blocked`, `.blockedDescription`, `.confirm`) in `src/i18n/locales/{en,zh,zh-TW,ja}.json`.
- Optional: a tauri-driver case that writes invalid JSON to `$CC_SWITCH_E2E_HOME/.cc-switch/settings.json` before launch and asserts the notice.

## Acceptance criteria / verification
- [x] New vitest (tests/hooks or `tests/components/AppShell.test.tsx`) with mocked invoke: null renders nothing; backupPath shows the path in a warning; null backupPath shows the dialog and confirm calls `acknowledge_settings_load_failure`.
- [x] `pnpm typecheck`, the tests/i18n parity test and `pnpm test:unit` pass.
- [x] CCS-084 marked Done apart from its Windows ACL item.

## Dependencies
None.

## Implementation notes

**Summary**
- New `SettingsLoadFailureNotice` component (in `src/components/app-shell/AppConfirmDialogs.tsx`, next to the other app-shell confirm flows), mounted once in `App.tsx`. On mount it calls `settingsApi.getLoadFailure()`:
  - `null` → nothing rendered, no toast;
  - `backupPath` set → persistent `toast.warning` (fixed id so StrictMode does not duplicate it, `closeButton`, `duration: Infinity`) naming the file, backup path and error;
  - `backupPath` null → `ConfirmDialog` explaining that saving is blocked, with path and error. "Allow overwrite" calls `settingsApi.acknowledgeLoadFailure()`, invalidates the `["settings"]` query and toasts success (or an error toast). "Keep blocked" closes the dialog and leaves saving blocked.
- It lives in its own component instead of `useAppShellEvents` because that hook returns nothing and cannot render a dialog. This also lets the test render it without the provider/event subscriptions.
- i18n: `settings.loadFailure.{backedUp,backedUpDescription,blocked,blockedDescription,confirm,keepBlocked,acknowledged,acknowledgeFailed}` in en/zh/zh-TW/ja.
- MSW: default handlers `get_settings_load_failure → null` and `acknowledge_settings_load_failure → true`, which also removes them from `tests/msw/unmockedCommands.ts`.
- CCS-084 is marked Done; its only open item is the Windows DACL check.
- The optional tauri-driver e2e case was not added.

**Files**: `src/components/app-shell/AppConfirmDialogs.tsx`, `src/App.tsx`, `src/i18n/locales/{en,zh,zh-TW,ja}.json`, `tests/components/SettingsLoadFailureNotice.test.tsx`, `tests/msw/handlers.ts`, `tests/msw/unmockedCommands.ts`, `docs/roadmap/tickets/P1-trust-release/CCS-084-p1-settings-atomic-save.md`.

**Commits**: 7d10409 (code and tests), plus the follow-up docs commit that records these notes.

**Verification**
- `pnpm exec vitest run tests/components/SettingsLoadFailureNotice.test.tsx` → 4/4 passed (null shows nothing; backup path appears in a persistent warning; null backupPath opens the dialog and confirm calls `acknowledge_settings_load_failure` and invalidates `["settings"]`; dismissing does not acknowledge).
- `pnpm exec vitest run tests/msw/handlers.test.ts` → 4/4 passed (the handler/allowlist parity check).
- `pnpm typecheck` → clean. `pnpm format:check` → clean.
- `pnpm test:unit` → 172 files, 1628 tests passed (this includes the tests/i18n parity test).
- No Rust changes.

**Review**
- Checked every acceptance criterion against 7d10409: the backend `SettingsLoadFailure` serialises as camelCase (`backupPath`), so the frontend type matches; `i18next` uses `escapeValue: false`, so paths are not HTML-escaped; the notice is mounted unconditionally in `App.tsx`; the MSW defaults and allowlist stay in parity.
- Fixed: the backed-up warning's description is three lines (file, backup path, error), but the app `Toaster` styles descriptions without `white-space` handling, so the `\n` breaks collapsed into one run-on line. The toast now passes `descriptionClassName: "whitespace-pre-line"` (the blocked dialog already uses `whitespace-pre-line`), and the test asserts it.
- Re-verified: the targeted tests (8/8), `pnpm typecheck`, `pnpm format:check` and `pnpm test:unit` (172 files, 1628 tests) all pass.
