# CCS-203: [shell] Wrap all localStorage access in safe helpers

**Status:** Done · **Kind:** bug · **Labels:** `bug`, `phase:F`, `p3-low`, `area:frontend`

> **Ticket key:** `f-safe-storage` · **Phase:** F · **Priority:** p3-low · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding frontend-app-localstorage-unguarded, partially real. Unguarded getItem and setItem calls run in App, ThemeProvider, UpdateContext, AppSwitcher and SessionManagerPage. This is low risk in the Tauri WebView.

## Evidence
- src/App.tsx:136-142,162-168,185; theme-provider.tsx:37,52; UpdateContext.tsx:46-51; AppSwitcher.tsx:41; SessionManagerPage.tsx:109; reference i18n/index.ts:15

## Proposed change
Add safeStorageGet and safeStorageSet helpers and use them everywhere.

## Acceptance criteria / verification
- [x] Vitest: with Storage.prototype getItem and setItem throwing, <ThemeProvider><App/></ThemeProvider> renders with the defaults
- [x] `grep -rn 'localStorage\.' src` matches only the helper
- [x] `pnpm test:unit` and `pnpm typecheck`

## Dependencies
None.

## Implementation notes
- **Summary:** New `src/lib/storage.ts` provides `safeStorageGet`, `safeStorageSet` and `safeStorageRemove`. Each one catches errors from reading `window.localStorage` itself and from `getItem`/`setItem`/`removeItem`, logs a `console.warn`, and returns `null`/`false`, so callers fall back to their defaults. Every direct `localStorage` call in `src/` now goes through these helpers: App (last app/view), ThemeProvider, UpdateContext (dismissed version and the legacy-key migration), AppSwitcher, SessionManagerPage (list mode and group expansion), useSettings/useSettingsForm (language), i18n, and the three legacy common-config snippet migrations (Claude/Codex/Gemini). `sessionStorage` use in App (env banner) was not touched; it is out of scope for this ticket.
- **Files:** `src/lib/storage.ts` (new), `src/App.tsx`, `src/components/theme-provider.tsx`, `src/contexts/UpdateContext.tsx`, `src/components/AppSwitcher.tsx`, `src/components/sessions/SessionManagerPage.tsx`, `src/hooks/useSettings.ts`, `src/hooks/useSettingsForm.ts`, `src/i18n/index.ts`, `src/components/providers/forms/hooks/{useCodexCommonConfig,useGeminiCommonConfig,useCommonConfigSnippet}.ts`, `tests/integration/App.test.tsx`, `tests/lib/storage.test.ts` (new).
- **Commit:** dac63b3 (code and tests).
- **Verification:**
  - `pnpm vitest run tests/integration/App.test.tsx`: 5/5 pass. The new test "renders with defaults when localStorage getItem and setItem throw" spies on `Storage.prototype.getItem`/`setItem` to throw, renders `<ThemeProvider><App/></ThemeProvider>`, and checks that the claude provider list and the light theme come up. With the pre-change `src/` files restored it fails with `SecurityError: storage disabled`.
  - `pnpm vitest run tests/lib/storage.test.ts`: 3/3 pass (normal round trip, throwing methods, throwing `localStorage` getter).
  - `grep -rn 'localStorage\.' src`: no matches. The helper reaches storage through `window.localStorage` without a trailing `.`, so nothing in `src/` calls a storage method directly.
  - `pnpm typecheck`: clean.
  - `pnpm test:unit`: 106 files and 1146 tests pass.
  - `pnpm format:check`: clean.
