# CCS-204: [a11y] Give icon-only buttons accessible names

**Status:** Done · **Kind:** bug · **Labels:** `bug`, `phase:F`, `p3-low`, `area:a11y`

> **Ticket key:** `f-a11y-icon-buttons` · **Phase:** F · **Priority:** p3-low · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding frontend-a11y-icon-buttons, partially real. About 11 unlabeled size="icon" buttons were confirmed. UniversalProviderCard is fine. The total of 61 is unverified.

## Evidence
- OpenCodeFormFields.tsx ~780,893,1030,1115; OmoFormFields.tsx:1028; BackupListSection.tsx:328,337; SessionManagerPage.tsx:825,999,1136,1591

## Proposed change
- Add aria-label={t(...)} to each unlabeled button. For tooltip-wrapped buttons, use the TooltipContent text.
- Add a dev warning in Button when size=icon has no label.

## Acceptance criteria / verification
- [x] RTL getByRole('button',{name}) finds each button in BackupListSection rename mode and the SessionManagerPage toolbar
- [x] jest-axe reports no button-name violations
- [x] `pnpm test:unit`

## Dependencies
None.

## Implementation notes

**Summary.** Every icon-only `<Button>`/`<button>` in `src/` that had no `aria-label`, `aria-labelledby`, `title` or text now has a translated `aria-label` (38 buttons across 26 components; tooltip-wrapped buttons reuse their `TooltipContent` key, e.g. `sessionManager.searchSessions`, `common.refresh`, `sessionManager.copyCommand`, `sessionManager.copyMessage`). Buttons that already had a `title` (e.g. `UniversalProviderCard`, `ProviderActions`) were left alone, since `title` is part of the accessible name. New `common.*` keys (`dismiss`, `selectModel`, `showPassword`, `hidePassword`, `openDocs`, `previousPage`, `nextPage`, `previousMonth`, `nextMonth`) were added to en/zh/zh-TW/ja. `Button` now calls `console.warn` in dev builds (`import.meta.env.DEV`) once per element when a `size="icon"` button mounts without an accessible name (it composes its ref so it can check the rendered node, and this works with `asChild`). axe also flagged the two `BackupListSection` policy `SelectTrigger`s (role=combobox, no name), so their `<Label>`s are now tied to them with `htmlFor`/`id`.

**Test dependency.** Added `jest-axe@10.0.0` as a devDependency. A small local declaration lives in `tests/a11y/jest-axe.d.ts`; `@types/jest-axe` was not added because it pulls in `@types/jest` globals. In the lockfile, recharts' optional `react-is` peer now resolves to 18.3.1 (it was 17.0.2), which matches React 18.

**Files.** `src/components/ui/button.tsx`, `src/App.tsx`, `src/components/{common/FullScreenPanel,env/EnvWarningBanner,openclaw/AgentsDefaultsPanel,openclaw/ToolsPanel,proxy/FailoverQueueManager,sessions/SessionManagerPage,sessions/SessionMessageItem,sessions/SessionToc,settings/BackupListSection,settings/GlobalProxySettings,skills/UnifiedSkillsPanel,universal/UniversalProviderFormModal,usage/RequestLogTable,usage/UsageDateRangePicker,workspace/DailyMemoryPanel}.tsx`, `src/components/providers/forms/{BasicFormFields,ClaudeDesktopProviderForm,ClaudeFormFields,EndpointSpeedTest,HermesFormFields,OmoFormFields,OpenClawFormFields,OpenCodeFormFields,shared/ModelDropdown,shared/ModelInputWithFetch}.tsx`, `src/i18n/locales/{en,zh,zh-TW,ja}.json`, `package.json`, `pnpm-lock.yaml`, `tests/a11y/{buttonName.ts,jest-axe.d.ts}`, `tests/components/IconButtonA11y.test.tsx`, `tests/components/SessionManagerPage.test.tsx`.

**Commits.** `2b13165` fix(a11y): give icon-only buttons accessible names (CCS-204), plus this ticket update.

**Verification.**
- New tests: `IconButtonA11y.test.tsx` covers BackupListSection rename mode (`getByRole('button',{name})` for confirm/cancel, plus axe `button-name` finding no violations), the dev warning firing, and no warning for buttons named by aria-label, title or sr-only text. A new `SessionManagerPage` test covers the toolbar search, refresh and close-search buttons and copy command (`getByRole` plus axe `button-name` finding no violations). With the three component files reverted to HEAD, 3 of the new tests fail; with the change, all pass.
- `pnpm test:unit`: 107 files, 1150 tests passed. No "has no accessible name" dev warning appeared anywhere in the suite.
- `pnpm typecheck`, `pnpm format:check` and `pnpm lint` (eslint, `--max-warnings 0`) are clean.
