# CCS-206: [provider-form] Guard against closing the Add/Edit panel with unsaved changes

**Status:** Done · **Kind:** feature · **Labels:** `feature`, `phase:F`, `p3-low`, `area:frontend`

> **Ticket key:** `f-unsaved-changes-guard` · **Phase:** F · **Priority:** p3-low · **Effort:** S · **Depends on:** `f-providerform-shell`, `f-radix-dialog-component-tests`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Frontend idea 3. Pressing Esc or clicking outside discards edits silently.

## Proposed change
When formState.isDirty or an editor is dirty, intercept close and show a confirm dialog.

## Acceptance criteria / verification
- [x] RTL: edit, press Esc, the confirm appears; cancel keeps the edits
- [x] `pnpm test:unit`

## Dependencies
f-providerform-shell, f-radix-dialog-component-tests

## Implementation notes

**Summary.** `ProviderFormShell` now tracks unsaved changes as react-hook-form `isDirty` OR any user `input`/`change` event inside the `<form>` DOM. The DOM check covers editors that keep their own state outside RHF (Codex/Gemini config, API key, and so on). Events that bubble up from portaled modals, such as the common-config editor, are ignored because those are saved separately. The shell reports the state through `ProviderFormDirtyContext`. The `ProviderForm` dispatcher fills that context from the new `ProviderFormProps.onDirtyChange` prop, so none of the per-app editors had to change. The shell reports `false` when it unmounts. A new hook, `useUnsavedChangesGuard(onClose)`, returns `onDirtyChange`, `requestClose` and a `ConfirmDialog` (Discard changes / Keep editing). `AddProviderDialog` and `EditProviderDialog` send every user-initiated close through `requestClose`: Esc, the back button, Cancel, and the form's `onCancel`. Closing after a successful save still calls `onOpenChange(false)` directly. When the confirm is open, Esc closes the confirm only (it is the top Radix layer) and the edits stay. New i18n keys `providerForm.unsavedChanges.*` were added in en/zh/zh-TW/ja.

**Files.**
- `src/features/providers/forms/useUnsavedChangesGuard.tsx` (new)
- `src/features/providers/forms/ProviderFormShell.tsx`
- `src/features/providers/forms/types.ts`
- `src/components/providers/forms/ProviderForm.tsx`
- `src/components/providers/AddProviderDialog.tsx`
- `src/components/providers/EditProviderDialog.tsx`
- `src/i18n/locales/{en,zh,zh-TW,ja}.json`
- `tests/components/ProviderPanelUnsavedChanges.test.tsx` (new)

**Commits.** 8c800dc (implementation and tests), plus the follow-up docs commit that records these notes.

**Verification.**
- `pnpm vitest run tests/components/ProviderPanelUnsavedChanges.test.tsx`: 5/5 passed. These tests use the real FullScreenPanel and ProviderForm. They cover: edit, then Esc shows the confirm, then Keep editing leaves the panel open with the edits intact; edit, then back, then Discard closes the panel; Esc with no edits closes without asking, for both Add and Edit; editing the Codex config editor (not managed by RHF) and pressing Esc shows the confirm.
- Regression check: with the guard's dirty branch disabled, the 3 tests that expect a confirm failed.
- `pnpm typecheck`: clean. `pnpm format:check`: clean. `eslint` on the touched files: clean.
- `pnpm test:unit`: 145/146 files and 1433/1438 tests passed. The 5 failures were all in `tests/integration/SettingsDialog.test.tsx` and were timeouts while the concurrent Rust build loaded the machine. That file passed 5/5 when re-run alone.
