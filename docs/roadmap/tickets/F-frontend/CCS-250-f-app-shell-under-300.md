# CCS-250: [app-shell] Shrink src/App.tsx to 300 lines or fewer by extracting the command palette wiring

**Status:** Open · **Kind:** refactor · **Labels:** `refactor`, `phase:F`, `p2-medium`, `area:app-shell`

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
- [ ] `wc -l src/App.tsx` ≤ 300.
- [ ] `pnpm typecheck` and `pnpm lint` pass.
- [ ] The App, app-shell and CommandPalette vitest suites pass, including the cross-app pending-switch behaviour.

## Dependencies
None.
