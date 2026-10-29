# CCS-197: [shell] Split App.tsx into AppHeader, a ViewTitle map and confirm orchestration

**Status:** Done · **Kind:** refactor · **Labels:** `refactor`, `phase:F`, `p2-medium`, `area:frontend`

> **Ticket key:** `f-app-shell-split` · **Phase:** F · **Priority:** p2-medium · **Effort:** M · **Depends on:** `p2-fe-app-registry`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Proposed change
- Extract AppHeader, a typed view map and the confirm-dialog orchestration.
- Keep the window-state effects in a hook (see f-resize-listener-leak).

## Acceptance criteria / verification
- [x] App.tsx is 300 lines or fewer
- [x] Existing App tests pass
- [x] `pnpm typecheck` and `pnpm test:unit`

## Dependencies
p2-fe-app-registry

## Implementation notes
**Summary.** `src/App.tsx` went from 1610 lines to 290. It now holds the top-level state and wires things together; the rest moved out:
- `src/components/app-shell/views.ts`: the `View` union, a typed `VIEW_TITLES: Record<SubView, …>` map (a new view with no title fails to compile), `isView`, `getBackView`, and the restore-from-storage helpers.
- `src/components/app-shell/AppHeader.tsx`: the header (back button plus `VIEW_TITLES` title on sub views; brand/settings/proxy/profile/app switcher on the home view). It is built from `AppFeatureNav.tsx` (per-app feature buttons as a typed `APP_FEATURE_NAV` map) and `HeaderViewActions.tsx` (Prompts / MCP / Skills / discovery actions). Panel handles are typed through `panelRefs.ts`, which replaces the old `useRef<any>`.
- `src/components/app-shell/AppConfirmDialogs.tsx`: confirm orchestration. `useProviderConfirm` covers delete and remove-from-live and calls `invalidateProviderGraph`. Also holds `ProviderConfirmDialog` and `HermesLaunchConfirmDialog`.
- `src/components/app-shell/AppViewContent.tsx` (the view switch), `ProvidersHome.tsx` (provider list wiring), `WindowControlsBar.tsx` and `dragRegion.ts` (typed `WebkitAppRegion` styles in place of `as any`), and `useViewShortcuts.ts` (Cmd/Ctrl+, and Esc).
- Hooks: `src/hooks/useWindowChrome.ts` holds the window-state effects (drag-bar height, decoration sync, maximize state via `useWindowMaximizedSync` from CCS-202, and the minimize/maximize/close actions). Also `useAppShellEvents.ts` (provider-switched and Tauri event subscriptions, migration toasts), `useEnvConflicts.ts` (env-conflict banner) and `useProviderShellActions.ts` (duplicate, terminal, website, import refresh, OMO disable). The live-id lookup is now a per-app query map.
- `settingsApi.getMigrationResult` / `getSkillsMigrationResult` replace the raw `invoke` calls that were in App.tsx. `src/App.tsx` is removed from the raw-invoke baseline in `tests/lint/eslintConfig.test.ts`.
- Lint baseline: App.tsx's `no-explicit-any` (12), `exhaustive-deps` (1) and `no-restricted-imports` (1) suppressions are gone. App-literal comparisons fell from 18 to 9: 2 left in App.tsx, and 7 moved into the new shell files and suppressed there (1 in AppHeader.tsx, 1 in AppViewContent.tsx, 5 in ProvidersHome.tsx). `APP_LITERAL_BUDGET` was lowered from 198 to 173 to match the new baseline total.
- Dependency `p2-fe-app-registry` was already in place (`src/entities/app/registry.ts`, static data with a `useAppRegistry` seam). The shell reads capabilities only through `useAppCaps`, so nothing here needs the backend `get_app_registry` IPC.

**Behaviour.** No intended UI change. Class names, animation keys (including grokbuild's own key), i18n keys and test ids are unchanged.

**Tests.**
- New `tests/components/AppShell.test.tsx` (12 tests) covers:
  - view-map completeness and titles
  - back-target rules and storage restore
  - feature-nav groups and capability collapse
  - AppHeader back navigation, the Hermes Web UI action and the settings-tab entry points
- New App integration test: the delete confirm path, both cancel and confirm.

**Commits.** fe938d1 (code). The ticket-notes commit follows it.

**Verification**
- `pnpm typecheck`: passes
- `pnpm lint`: passes with `--max-warnings 0`
- `pnpm format:check`: all files formatted
- `pnpm test:unit`: 114 files / 1216 tests passed. Re-ran `tests/integration/App.test.tsx`, `tests/components/AppShell.test.tsx` and `tests/lint` after the final edit: 32/32 passed.
- `wc -l src/App.tsx`: 290
- No files under `src-tauri/` were touched.
