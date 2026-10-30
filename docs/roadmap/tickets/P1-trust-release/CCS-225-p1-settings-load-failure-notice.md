# CCS-225: [settings-ui] Show a startup notice and confirm flow when settings.json failed to load

**Status:** Open · **Kind:** bug · **Labels:** `bug`, `phase:P1`, `p2-medium`, `area:settings-ui`

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
- [ ] New vitest (tests/hooks or `tests/components/AppShell.test.tsx`) with mocked invoke: null renders nothing; backupPath shows the path in a warning; null backupPath shows the dialog and confirm calls `acknowledge_settings_load_failure`.
- [ ] `pnpm typecheck`, the tests/i18n parity test and `pnpm test:unit` pass.
- [ ] CCS-084 marked Done apart from its Windows ACL item.

## Dependencies
None.
