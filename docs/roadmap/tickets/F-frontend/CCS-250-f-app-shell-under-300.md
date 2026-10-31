# CCS-250: [app-shell] Shrink src/App.tsx to 300 lines or fewer by extracting the command palette wiring

**Status:** Done · **Kind:** refactor · **Labels:** `refactor`, `phase:F`, `p2-medium`, `area:app-shell`

> **Ticket key:** `f-app-shell-under-300` · **Phase:** F · **Priority:** p2-medium · **Effort:** M
> Plan: [next-phase-plan.md](../../next-phase-plan.md)
> Found during the Partial-ticket triage at d962272 (CCS-007). Source tickets: [CCS-007](../F-frontend/CCS-007-epic-f.md).

## Problem
The Track F exit criterion caps `src/App.tsx` at 300 lines; it is 373 (ProviderForm.tsx is 53, already met). Most of the growth since the CCS-197 split is the CCS-208 palette wiring: `isPaletteOpen` (96), `pendingSwitch` state and effect (97-101, 179-186), `selectAppFromPalette` / `switchProviderFromPalette` (188-199), `useCommandPaletteShortcut` (129) and the `<CommandPalette>` JSX with the inline takeover toggle (341-365).

## Proposed change
- Move the palette wiring into `src/components/app-shell/` (e.g. `useCommandPaletteController` or an `AppCommandPalette.tsx` wrapper) taking `activeApp`, `setActiveApp`, `setCurrentView`, `switchProvider`, `visibleApps`, `settingsData`, `setTakeoverForApp`, `isCurrentAppTakeoverActive`, `openSettings`, `setIsAddOpen`.
- Optionally move the visibleApps/sessions-fallback effects (114-125) and the `activeProviderId` memo (137-143) into small hooks (~30 more lines).
- Optionally add a line-count guard test (like `APP_LITERAL_BUDGET`) so App.tsx cannot grow past 300 again.

## Acceptance criteria / verification
- [x] `wc -l src/App.tsx` ≤ 300.
- [x] `pnpm typecheck` and `pnpm lint` pass.
- [x] The App, app-shell and CommandPalette vitest suites pass, including the cross-app pending-switch behaviour.

## Dependencies
None.

## Implementation notes
- **Change:** the command palette wiring moved out of `App.tsx` into `src/components/app-shell/AppCommandPalette.tsx`, which owns the palette open state, `useCommandPaletteShortcut`, the cross-app `pendingSwitch` state and effect (unchanged semantics: switch only once `activeApp` is the target app, skip additive apps), and the takeover toggle shown only when the local proxy is enabled and the app supports takeover. The hidden-app fallback, the sessions-view fallback and `DEFAULT_VISIBLE_APPS` moved into `src/components/app-shell/useShellFallbacks.ts` (returns the effective `visibleApps`); `OPENCLAW_HEALTH_VIEWS` moved to `views.ts`; the `activeProviderId` memo became a plain optional-chain lookup (it is a string, so memoising bought nothing). `wc -l src/App.tsx`: 375 -> 296.
- **Guard:** `tests/lint/appShellSize.test.ts` fails if `src/App.tsx` exceeds 300 lines (it would fail on the pre-change 375-line file).
- **Lint fix needed for the acceptance item:** `pnpm lint` failed at the start of this ticket because the CCS-238 download stub in `tests/components/WebdavSyncSection.test.tsx` added a 14th `any`, one over its baseline of 13 in `eslint-suppressions.json`. Changed that `args?: any` to `args?: unknown` (separate commit) instead of raising the baseline.
- **Files:** `src/App.tsx`, `src/components/app-shell/AppCommandPalette.tsx` (new), `src/components/app-shell/useShellFallbacks.ts` (new), `src/components/app-shell/views.ts`, `tests/lint/appShellSize.test.ts` (new), `tests/components/WebdavSyncSection.test.tsx`.
- **Commits:** b212d6d (lint fix), b09df83 (refactor + guard test), plus this ticket-notes commit.
- **Verification:**
  - `wc -l src/App.tsx` -> 296.
  - `pnpm typecheck` -> pass; `pnpm lint` -> pass (0 problems); `pnpm format:check` -> pass.
  - `npx vitest run tests/lint/appShellSize.test.ts tests/integration/App.test.tsx tests/components/AppShell.test.tsx tests/components/CommandPalette.test.tsx tests/lint/` -> 9 files, 57 tests passed (includes "opens the command palette with Ctrl-K, filters and switches a provider in another app" for the cross-app pending switch, and the palette takeover gating test).
  - `pnpm test:unit` -> 174 files, 1669 tests passed.
