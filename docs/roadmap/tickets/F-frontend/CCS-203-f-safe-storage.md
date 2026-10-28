# CCS-203: [shell] Wrap all localStorage access in safe helpers

**Status:** Open · **Kind:** bug · **Labels:** `bug`, `phase:F`, `p3-low`, `area:frontend`

> **Ticket key:** `f-safe-storage` · **Phase:** F · **Priority:** p3-low · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding frontend-app-localstorage-unguarded, partially real. Unguarded getItem and setItem calls run in App, ThemeProvider, UpdateContext, AppSwitcher and SessionManagerPage. This is low risk in the Tauri WebView.

## Evidence
- src/App.tsx:136-142,162-168,185; theme-provider.tsx:37,52; UpdateContext.tsx:46-51; AppSwitcher.tsx:41; SessionManagerPage.tsx:109; reference i18n/index.ts:15

## Proposed change
Add safeStorageGet and safeStorageSet helpers and use them everywhere.

## Acceptance criteria / verification
- [ ] Vitest: with Storage.prototype getItem and setItem throwing, <ThemeProvider><App/></ThemeProvider> renders with the defaults
- [ ] `grep -rn 'localStorage\.' src` matches only the helper
- [ ] `pnpm test:unit` and `pnpm typecheck`

## Dependencies
None.
