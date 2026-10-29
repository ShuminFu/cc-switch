# CCS-198: [perf] Lazy-load heavy views, per-app presets and locales

**Status:** Done · **Kind:** refactor · **Labels:** `refactor`, `phase:F`, `p3-low`, `area:frontend-perf`

> **Ticket key:** `f-lazy-views-locales` · **Phase:** F · **Priority:** p3-low · **Effort:** M · **Depends on:** `f-app-shell-split`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Proposed change
- React.lazy for SessionManagerPage, settings sync, UsageScriptModal, the skills/MCP/proxy panels and the CodeMirror editors.
- Load per-app presets and locale JSON on demand.

## Acceptance criteria / verification
- [x] The initial JS bundle is at least 30% smaller than v3.17 (`pnpm build:renderer` size report in the PR)
- [x] `pnpm test:unit`

## Dependencies
f-app-shell-split

## Implementation notes

**Summary.** The renderer entry chunk no longer contains the non-home views, the provider form stack or the locale JSON:

- `src/components/app-shell/AppViewContent.tsx`: every non-home view (SettingsPage incl. WebDAV sync/usage charts, PromptPanel, UnifiedMcpPanel, SkillsPage, UnifiedSkillsPanel, AgentsPanel, UniversalProviderPanel, SessionManagerPage, WorkspaceFilesPanel, OpenClaw Env/Tools/AgentsDefaults panels, HermesMemoryPanel) loads with `React.lazy` under a `Suspense` spinner. Panel refs still work through lazy components.
- `src/App.tsx`: `AddProviderDialog` / `EditProviderDialog` (provider forms, all per-app presets, CodeMirror editors) and `UsageScriptModal` (prettier, CodeMirror) are lazy. The dialogs sit inside the new `LazyMount`, which mounts them on first open and keeps them mounted after that so the Radix close animation still runs.
- `src/components/common/LazyMount.tsx` (new): `lazyNamed()` for named exports, `LazyMount`, `LazyViewFallback`.
- `src/i18n/index.ts`: the four locale JSON files are no longer static imports. `LOCALE_LOADERS` and a small i18next `lazyLocaleBackend` fetch only the active language and its fallback chain (e.g. `ja` + `en`) as separate chunks. `i18nReady` resolves once they are loaded, and `src/main.tsx` awaits it before the first render, so translation keys never flash on screen. `react.useSuspense` is off, so a language switch re-renders through `languageChanged` instead of suspending.
- Light modules so the always-loaded shell does not pull in the heavy parents: `src/components/skills/skillsPageHeaderActions.ts` (header actions + `SkillsPageSource`/`SkillsPageHandle`, re-exported from `SkillsPage`) and `src/config/hermesProviderSource.ts` (`isHermesReadOnlyProvider` + markers, re-exported from `hermesProviderPresets`; `ProviderCard` imports the light one).
- Per-app presets: all `config/*ProviderPresets.ts` modules (~270 kB) now ship only in the lazy `ProviderForm` chunk. That chunk downloads the first time the add/edit dialog opens. Splitting presets further, into one chunk per app inside the form, would need `ProviderForm` and its hooks to load presets asynchronously. That refactor was left out of this ticket.

**Tests.**
- `tests/lint/lazyEntryImports.test.ts` (new): guard that fails if the shell files statically import any lazy-only module. It would have failed on HEAD before this change, which had static imports of the dialogs and views.
- `tests/i18n/lazyLocales.test.ts` (new): checks that there are no static locale JSON imports. It also checks that a `ja` instance loads only `ja` + `en` and switches to `en` on demand.
- `tests/components/LazyMount.test.tsx` (new): the chunk loader is not called before the first open, and the component stays mounted after it closes.
- `tests/integration/App.test.tsx`: the usage/add/edit dialogs are awaited with `findByTestId`, because they now resolve asynchronously.

**Files.** src/App.tsx, src/main.tsx, src/i18n/index.ts, src/components/common/LazyMount.tsx, src/components/app-shell/{AppViewContent,AppHeader,HeaderViewActions,panelRefs}.ts(x), src/components/skills/{SkillsPage.tsx,skillsPageHeaderActions.ts}, src/components/providers/ProviderCard.tsx, src/config/{hermesProviderPresets,hermesProviderSource}.ts, tests/integration/App.test.tsx, tests/components/LazyMount.test.tsx, tests/i18n/lazyLocales.test.ts, tests/lint/lazyEntryImports.test.ts.

**Commits.** 95c3ef0 (code + tests); this ticket update in a follow-up commit.

**Verification.**
- `pnpm build:renderer` size report (v3.17.0 = parent commit 95806df vs this change). Initial JS is the single entry chunk referenced by `dist/index.html`. There are no other modulepreloads.

  | | before (v3.17.0) | after | change |
  |---|---|---|---|
  | entry `index-*.js` | 4,229.39 kB (gzip 1,252.54 kB) | 944.92 kB (gzip 312.28 kB) | **−77.7 %** (gzip −75.1 %) |

  On-demand chunks after: UsageScriptModal 629.00 kB, ProviderForm 579.63 kB, SettingsPage 572.22 kB, shared CodeMirror 501.82 kB, SessionManagerPage 114.37 kB, MarkdownEditor 99.46 kB. Locales: en 137.87 / ja 177.24 / zh 130.99 / zh-TW 132.47 kB.
- `pnpm test:unit`: 117 files, 1225 tests passed.
- `pnpm typecheck`: clean. `pnpm lint`: clean (0 warnings). `pnpm format:check`: clean.
- No Rust files touched.

**Review.** Re-checked independently: `pnpm build:renderer` entry chunk is 944.92 kB and `dist/index.html` references only that script, with the locales (`en`/`ja`/`zh`/`zh-TW`) and the heavy views/dialogs in separate chunks. `pnpm test:unit` (117 files, 1225 tests), `pnpm typecheck` and `pnpm format:check` pass. One follow-up fix: the `configLoadError` listener in `src/main.tsx` is registered before `bootstrap()` waits for `i18nReady`. An early event could therefore show the error dialog before the locale chunk arrived, falling back to the hard-coded Chinese `defaultValue`. `handleConfigLoadError` now awaits `i18nReady` first. `i18nReady` never rejects, so this cannot block the exit path. Per-app preset splitting is still limited to "all presets load with the lazy ProviderForm chunk", as noted above. The acceptance criteria (≥30% smaller entry, unit tests) are met.
