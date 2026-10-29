# CCS-107: [provider-form] Do not reset typed input when live settings finish loading in the edit dialog

**Status:** Done · **Kind:** bug · **Labels:** `bug`, `phase:P1`, `p3-low`, `area:frontend`

> **Ticket key:** `p1-fe-edit-dialog-reset` · **Phase:** P1 · **Priority:** p3-low · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding frontend-editdialog-reset-wipes-input, partially real. When editing the current provider (or an OpenClaw provider with a live entry), the form renders with DB settings. Once the live read resolves, form.reset wipes what the user typed and re-initialises the Codex and Gemini hooks.

## Evidence
- src/components/providers/EditProviderDialog.tsx:37, 45-132, 134, 162-179, 220-251; ProviderForm.tsx:379-401, 649-651; useCodexConfigState.ts:51; useGeminiConfigState.ts:77

## Proposed change
- Render ProviderForm only after hasLoadedLive, with a loading placeholder until then.
- Set hasLoadedLive on every path, including when live is falsy or the load is cancelled.

## Acceptance criteria / verification
- [x] RTL: with a deferred live promise, the form appears only after it resolves and shows live values; a non-current provider renders immediately
- [x] `pnpm test:unit`

## Dependencies
None.

## Implementation notes
- **Summary:** `EditProviderDialog` now renders `ProviderForm` only once the live read has finished (`hasLoadedLive`); until then it shows a `common.loading` spinner placeholder (`data-testid="edit-provider-loading"`) and the Save button is disabled, so the form mounts exactly once with the final initial values and nothing is reset under the user. The boolean `hasLoadedLive` state was replaced by `loadedLiveKey` (`appId:providerId`): every path (live object, falsy live, read error, non-current provider, OpenClaw) calls a single `finish()` that records the key. A cancelled read never marks completion; it is superseded by the effect re-run for the new key, so the dialog cannot get stuck on the placeholder and stale live values from another provider are never applied (`effectiveLiveSettings`). This is how the "set hasLoadedLive on every path, including cancelled" item is met without letting a cancelled read leak a stale "loaded" flag. Proxy-takeover and OpenCode (which never read live) render the form on the very first paint.
- **Note on "non-current renders immediately":** the dialog still needs `providersApi.getCurrent` to know whether the provider is current, so a non-current provider renders as soon as that call resolves, without waiting on any live read (`getLiveProviderSettings` is not called). No new prop was added to pass the current id in synchronously.
- **Files:** `src/components/providers/EditProviderDialog.tsx`, `tests/components/EditProviderDialog.test.tsx`.
- **Commits:** c236b0f (fix + tests), plus this ticket-notes commit.
- **Tests added** (the `ProviderForm` mock records each mount's `initialData`): current provider with a deferred live promise shows the placeholder and a disabled Save, then mounts the form once with the live values; same for OpenClaw; a non-current provider renders DB values and never reads live; falsy and rejected live reads fall back to DB values; proxy takeover renders synchronously without calling `getCurrent`. The two deferred-promise tests fail against the previous component (form rendered before live resolved) and pass after the fix.
- **Verification:** `npx vitest run tests/components/EditProviderDialog.test.tsx` gives 7/7 passed (2 failed with the old component). `pnpm test:unit` gives 96 files, 672 tests passed. `pnpm typecheck` is clean. `pnpm format:check` is clean. `npx eslint --max-warnings 0` on both touched files is clean.
