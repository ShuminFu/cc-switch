# Feature UI (settings, usage, proxy, skills, MCP) — findings (FE-UI-xx)

Part of the [next-phase proposals](../README.md). 47 findings. Priorities: P2 = 23, P3 = 24.

## Area summary

The non-provider feature UI is broad and mostly functional, with some genuinely careful work (password-preservation logic in WebdavSyncSection, the stale-while-revalidate tool cache in AboutSection, optimistic failover toggles, the live-range date picker). The main structural risk is size and duplication: `WebdavSyncSection.tsx` (1,867 lines) is two hand-mirrored state machines, `AboutSection.tsx` (1,272 lines) mixes five concerns, `useSettings`/`useSettingsForm` duplicate the save pipeline and the directory normalisation, and the MCP/Skills/Prompts panels re-implement the same "count bar + list + per-app toggles + confirm + form" shell three times with three different state strategies. There is a meaningful amount of dead code that hides unfinished features (`RequestDetailPanel`, `DataSourceBar`, `CircuitBreakerConfigPanel`, `RepoManager`, `PromptFormModal`, `hooks/useProxyConfig`, `useProviderLimits`) — the usage drill-down, session import and budgets are half-built but unreachable. Concrete bugs found: General-tab autosave silently persists unsaved Advanced-tab directory overrides without the save side-effects; `hermesConfigDir` is skipped by sanitisation and change detection; directory inputs trim on every keystroke so paths with spaces cannot be typed; failover settings only expose three apps while takeover exposes four (and the backend eight); zh-TW is missing 30 keys. Accessibility is uneven (hover-only action bars, icon buttons without names, hand-rolled modals without focus traps, no reduced-motion handling, light-only skeleton/badge colours). Test coverage is concentrated on settings/import/export and MCP form; proxy, usage charts, pricing, OpenClaw, workspace, prompts and the whole S3 branch have no component tests. Suggested direction: (1) fix the settings save-pipeline bugs and consolidate it into one table-driven module, (2) delete or wire the dead components, (3) land the three large refactors (cloud-sync panel, managed-items panel, AboutSection/tool lifecycle), (4) adopt zod + react-hook-form for the numeric config panels, (5) add an a11y/i18n lint pass, then (6) ship the missing features (usage export/budgets, request inspector, MCP server test, skill install progress).

## Index

| ID | Title | Type | Priority | Effort | Confidence | Verdict |
|---|---|---|---|---|---|---|
| [FE-UI-01](#fe-ui-01) | General-tab autosave silently persists unsaved Advanced-tab directory overrides (and skips their side effects) | bug | P2 (first pass P1) | M | high | partially-confirmed |
| [FE-UI-03](#fe-ui-03) | Directory override inputs trim on every keystroke, so paths containing spaces cannot be typed | bug | P2 (first pass P1) | S | high | confirmed |
| [FE-UI-05](#fe-ui-05) | Dead components hide unfinished features (request drill-down, session import, circuit-breaker panel, repo dialog, prompt modal, proxy-config hook, provider limits) | tech-debt | P2 (first pass P1) | M | high | partially-confirmed |
| [FE-UI-06](#fe-ui-06) | zh-TW locale is missing 30 keys (tool lifecycle strings fall back to English) | bug | P2 | S | high |  |
| [FE-UI-07](#fe-ui-07) | Advanced-tab "Save" has no dirty tracking and no unsaved-changes guard | ux | P2 | M | high |  |
| [FE-UI-10](#fe-ui-10) | Config panels that fetch with ad-hoc `useEffect` show defaults on fetch failure and may write them back | bug | P2 | M | medium |  |
| [FE-UI-11](#fe-ui-11) | Hover-only action bars and nested interactive elements are not keyboard reachable | ux | P2 | S | high |  |
| [FE-UI-12](#fe-ui-12) | Hand-rolled modals/panels lack dialog semantics and focus management | ux | P2 | M | high |  |
| [FE-UI-15](#fe-ui-15) | i18n misuse — hard-coded strings, concatenated translations, inline Chinese defaults, unlocalised number/date formatting | tech-debt | P2 | M | high |  |
| [FE-UI-16](#fe-ui-16) | XL refactor — split `WebdavSyncSection` into a backend-agnostic cloud-sync panel | refactor | P2 (first pass P1) | XL | high | partially-confirmed |
| [FE-UI-17](#fe-ui-17) | L refactor — consolidate the settings save pipeline (`useSettings` / `useSettingsForm` / `useDirectorySettings`) | refactor | P2 (first pass P1) | L | high | partially-confirmed |
| [FE-UI-18](#fe-ui-18) | XL refactor — a shared `ManagedItemsPanel` for MCP, Skills and Prompts (and unify their data layer) | refactor | P2 | XL | high |  |
| [FE-UI-19](#fe-ui-19) | L refactor — decompose `AboutSection` (app update + tool lifecycle + WSL prefs + diagnostics + install commands) | refactor | P2 | L | high |  |
| [FE-UI-20](#fe-ui-20) | Numeric config panels hand-roll validation with duplicated range tables; adopt zod + react-hook-form | refactor | P2 | M | high |  |
| [FE-UI-24](#fe-ui-24) | Failover queue manager lacks reorder, health, test and reason-for-switch feedback | feature | P2 | M | high |  |
| [FE-UI-25](#fe-ui-25) | Usage dashboard has no export (CSV/JSON) of logs or aggregates | feature | P2 | M | high |  |
| [FE-UI-26](#fe-ui-26) | Usage budgets/alerts — backend limit types exist but no UI | feature | P2 | L | medium |  |
| [FE-UI-29](#fe-ui-29) | `PricingConfigPanel` omits OpenCode, fires 8 unguarded writes, and refetches on language change | bug | P2 | S | high |  |
| [FE-UI-30](#fe-ui-30) | Manual pricing entry is not normalised like the models.dev import, so manual IDs may never match usage rows | bug | P2 | S | medium |  |
| [FE-UI-32](#fe-ui-32) | MCP wizard/form gaps — no `cwd`, plain-text env secrets, silent line drops, no connection test, macOS-only shortcut, OpenClaw toggle missing in form | feature | P2 | M | high |  |
| [FE-UI-33](#fe-ui-33) | Skills UX — discovery "Uninstall" only toasts, no install progress, sequential update-all without progress, conflict/symlink explanation missing | feature | P2 | M | high |  |
| [FE-UI-34](#fe-ui-34) | OpenClaw/workspace editors reset local edits on refetch and have no unsaved-changes guard | bug | P2 | S | medium |  |
| [FE-UI-42](#fe-ui-42) | Test gaps — no component tests for proxy, usage charts/pricing, about, backups, log/rectifier, OpenClaw, workspace, prompts, Hermes; S3 branch untested | test | P2 (first pass P1) | L | high | partially-confirmed |
| [FE-UI-02](#fe-ui-02) | `hermesConfigDir` is skipped by sanitisation, form normalisation and directory-change detection | bug | P3 (first pass P1) | S | high | partially-confirmed |
| [FE-UI-04](#fe-ui-04) | Failover/takeover UI covers different app sets in different places; label maps diverge | bug | P3 (first pass P1) | M | high | partially-confirmed |
| [FE-UI-08](#fe-ui-08) | Cancelling the export file dialog shows an error toast (and the test enshrines it) | ux | P3 | S | high |  |
| [FE-UI-09](#fe-ui-09) | Autosave failure shows two error toasts | bug | P3 | S | high |  |
| [FE-UI-13](#fe-ui-13) | No reduced-motion handling; light-only skeleton and status colours in dark mode | ux | P3 | S | high |  |
| [FE-UI-14](#fe-ui-14) | Dark-mode `MutationObserver` boilerplate duplicated in six in-scope files although `useDarkMode` exists | tech-debt | P3 | S | high |  |
| [FE-UI-21](#fe-ui-21) | Segmented-control pattern copy-pasted five times in Settings | refactor | P3 | S | high |  |
| [FE-UI-22](#fe-ui-22) | Proxy status polling is duplicated across two hook modules with different cadences | tech-debt | P3 | S | high |  |
| [FE-UI-23](#fe-ui-23) | ProxyPanel UX gaps — hidden config while running, misleading stat colouring, unlocalised uptime, unawaited clipboard | ux | P3 | S | high |  |
| [FE-UI-27](#fe-ui-27) | Usage range/timezone edge cases — local-day presets vs string bucket dates, unenforced 30-day hint, Sunday-first calendar | bug | P3 | S | medium |  |
| [FE-UI-28](#fe-ui-28) | `UsageTrendChart` performance/typing — inline tooltip component, un-memoised data, `any` types, naive tick formatting | performance | P3 | S | high |  |
| [FE-UI-31](#fe-ui-31) | Usage dashboard filters/tab are lost on every Settings tab switch | ux | P3 | S | high |  |
| [FE-UI-35](#fe-ui-35) | Workspace paths are hard-coded and file existence is checked by reading full contents | bug | P3 | S | high |  |
| [FE-UI-36](#fe-ui-36) | `AgentsDefaultsPanel` can persist an empty primary model and numeric fields lack validation | bug | P3 | S | medium |  |
| [FE-UI-37](#fe-ui-37) | `BackupListSection` duplicates confirm dialogs, uses a misleading icon and offers no reveal/export | ux | P3 | S | high |  |
| [FE-UI-38](#fe-ui-38) | Settings search and a keyboard-shortcuts reference are missing | feature | P3 | M | high |  |
| [FE-UI-39](#fe-ui-39) | Proxy request inspector/replay is absent despite logging being a first-class toggle | feature | P3 | L | medium |  |
| [FE-UI-40](#fe-ui-40) | `ToggleRow` label text is not associated with (or clickable for) its switch | ux | P3 | S | high |  |
| [FE-UI-41](#fe-ui-41) | Settings tab bar enforces a 720 px minimum and overflows at narrow widths | ux | P3 | S | medium |  |
| [FE-UI-43](#fe-ui-43) | `GlobalProxySettings` round-trips URLs with an added trailing slash and has unlabelled credential inputs | bug | P3 | S | medium |  |
| [FE-UI-44](#fe-ui-44) | `ModelsDevPickerDialog` rows are `role="button"` divs without keyboard support, and the fetch bypasses the app's proxy settings | ux | P3 | S | medium |  |
| [FE-UI-45](#fe-ui-45) | Install-command snippets execute remote scripts (incl. `-ExecutionPolicy Bypass`) without surfacing the source URL or a warning | docs | P3 | S | medium |  |
| [FE-UI-46](#fe-ui-46) | Prompt management lacks versioning/history and diff against the live file | feature | P3 | M | medium |  |
| [FE-UI-47](#fe-ui-47) | `DialogContent`/`DialogOverlay` duplicate the z-index map and most feature dialogs pick ad-hoc `zIndex` values | tech-debt | P3 | S | high |  |

## Findings

### FE-UI-01: General-tab autosave silently persists unsaved Advanced-tab directory overrides (and skips their side effects)

**Type:** bug · **Priority:** P2 · **Effort:** M · **Confidence:** high

- **Evidence:** `src/hooks/useSettings.ts:182-222` — `autoSaveSettings` builds `mergedSettings = { ...settings, ...updates }` from the *whole* form state and sends it to `saveMutation.mutateAsync(payload)`; `src/components/settings/SettingsPage.tsx:182-211` — every General/Proxy toggle calls `handleAutoSave`; `src/hooks/useDirectorySettings.ts:245-265` — typing in an Advanced-tab directory field immediately writes `claudeConfigDir`/… into the same form state via `onUpdateSettings`; `src/hooks/useSettings.ts:312-363,433-458` — only the explicit `saveSettings` path runs `setAppConfigDirOverride`, `syncCurrentProvidersLiveSafe()` on directory change and the restart flag; `src/lib/query/mutations.ts:383-391` — the save mutation invalidates `["settings"]`, and `src/hooks/useSettingsForm.ts:103-134` re-initialises the form from server data on every refetch.
- **Problem:** A user who edits a config-directory override on the Advanced tab (without pressing Save), then flips any General-tab switch, has the half-typed directory persisted to `settings.json` with no live-config resync, no restart prompt and no feedback. Conversely, after the autosave refetch the form is rebuilt from the server, so the user can no longer tell what was "pending". The comment at `SettingsPage.tsx:185-188` acknowledges the full-state send but only handles rollback of the toggled key.
- **Proposal:** Make `autoSaveSettings` send only the persisted baseline plus `updates` (`{ ...queryClient.getQueryData(["settings"]), ...updates }`) instead of the dirty form; keep directory overrides in a separate "draft" slice that only `saveSettings` commits; or move directory overrides to their own explicit-save form with dirty tracking (see FE-UI-10).
- **Acceptance:** Test: edit `claudeConfigDir` on Advanced, toggle `minimizeToTrayOnClose` on General → `save_settings` payload must not contain the edited directory; pressing Save afterwards must still persist it and call `syncCurrentProvidersLiveSafe`.
- **Verification:** **partially-confirmed** — Mechanism confirmed: useSettings.ts:184 merges the whole dirty form (`{ ...settings, ...updates }`) and :222 sends it to save_settings, while useDirectorySettings.ts:251-253 writes every keystroke of a directory override straight into that form; only saveSettings runs setAppConfigDirOverride (:363) and syncCurrentProvidersLiveSafe on dir change (:439-455), and the backend save_settings (src-tauri/src/commands/settings.rs:62-72 -> settings.rs:748 update_settings) only normalizes paths with no directory-change side effects, so a half-typed override is persisted silently (BackupListSection/WebdavSyncSection on the same Advanced tab also call handleAutoSave, SettingsPage.tsx:424-426 and :449-454, so the trigger does not even require leaving the tab). Detail off: the 'no restart prompt' claim is moot because the restart flag is tied solely to the CC Switch app dir (useSettings.ts:457-458), which lives in separate useState (useDirectorySettings.ts:129-131) and is never part of the autosaved form; per-app overrides never prompt a restart even via Save. Impact is a recoverable misconfiguration (the backend immediately resolves the bad override via settings.rs get_*_override_dir / config.rs:37-43, but original files are untouched and pressing Save or correcting the field fixes it), so P2 rather than P1. Proposal caveat: option 1 (server baseline + updates) would let two rapid toggles clobber each other because the ['settings'] cache is only refreshed after each mutation settles (mutations.ts:389-391); the separate draft slice for directory overrides is the sound fix.

### FE-UI-03: Directory override inputs trim on every keystroke, so paths containing spaces cannot be typed

**Type:** bug · **Priority:** P2 · **Effort:** S · **Confidence:** high

- **Evidence:** `src/components/settings/DirectorySettings.tsx:427-432` — controlled `<Input value={displayValue} onChange={(e) => onChange(e.target.value)}>` where `displayValue = value ?? resolvedValue`; `src/hooks/useDirectorySettings.ts:245-265` — `updateDirectoryState` runs `sanitizeDir(value)` (`.trim()`, empty → `undefined`) before storing.
- **Problem:** Typing `C:\Users\John Doe\.claude` yields `JohnDoe` because the trailing space is trimmed before the next character arrives; clearing the field snaps the display back to the resolved default, so the user cannot see that the override was removed. Only the Browse button works for such paths.
- **Proposal:** Keep raw text in local component state (or in the form) and sanitise on blur/save only; show the resolved default as placeholder rather than as value when the override is empty; add an explicit "override active" affordance.
- **Acceptance:** RTL test typing `"a b"` into the Claude directory input results in `claudeConfigDir === "a b"`; clearing shows placeholder and `undefined` override.
- **Verification:** **confirmed** — Cited lines 427-432 do not exist (DirectorySettings.tsx is 242 lines), but the code is at DirectorySettings.tsx:201-204 (`displayValue = value ?? resolvedValue ?? ""`) and :215-220 (controlled Input whose onChange forwards raw text), and useDirectorySettings.ts:247-253 runs sanitizeDir (trim, empty -> undefined) before storing, with :256-262 snapping resolvedDirs back to the default on clear; the app-config input at DirectorySettings.tsx:60-65 -> useDirectorySettings.ts:267-272 has the same defect. Because React restores a controlled input to its prop value after each change, a trailing space is dropped before the next character arrives, so sequentially typing 'John Doe' yields 'JohnDoe' and clearing the field displays the resolved default instead of an empty override, as described. The finding understates the workarounds: pasting a full path, the Browse dialog (settingsApi.selectConfigDirectory, useDirectorySettings.ts:289), and inserting a space mid-string all work, and no data is affected, so this is a P2 UX defect rather than P1. No tests cover these components; the proposal (raw local text, sanitise on blur/save, placeholder for the default) is sound and not implemented.

### FE-UI-05: Dead components hide unfinished features (request drill-down, session import, circuit-breaker panel, repo dialog, prompt modal, proxy-config hook, provider limits)

**Type:** tech-debt · **Priority:** P2 · **Effort:** M · **Confidence:** high

- **Evidence:** `rg` finds no importers for `src/components/usage/RequestDetailPanel.tsx` (324 lines), `src/components/usage/DataSourceBar.tsx` (125 lines, contains the only "Import Sessions / Sync" button), `src/components/proxy/CircuitBreakerConfigPanel.tsx` (356 lines), `src/components/skills/RepoManager.tsx` (202 lines, Dialog twin of `RepoManagerPanel.tsx`), `src/components/prompts/PromptFormModal.tsx` (164 lines, twin of `PromptFormPanel.tsx`), `src/hooks/useProxyConfig.ts` (duplicate of `src/lib/query/proxy.ts:137-166`), and `useProviderLimits`/`ProviderLimitStatus` (`src/lib/query/usage.ts:349`, `src/types/usage.ts:220-228`). `src/components/usage/RequestLogTable.tsx:197-316` rows have no click handler, so the detail panel is unreachable.
- **Problem:** ~1,300 lines of unreachable UI bit-rot (e.g. `RequestDetailPanel` still uses a hand-rolled locale map at `:22-29` and light-only colours), users cannot open request details or trigger session import, and reviewers cannot tell which twin is canonical.
- **Proposal:** Decide per component: wire (`RequestDetailPanel` ← row click; `DataSourceBar` ← dashboard header; `useProviderLimits` ← budgets, FE-UI-28) or delete (`CircuitBreakerConfigPanel`, `RepoManager`, `PromptFormModal`, `hooks/useProxyConfig`). Add `knip`/`ts-prune` to CI to prevent recurrence.
- **Acceptance:** `knip` reports zero unused exports/files under `src/components` and `src/hooks`; clicking a request-log row opens the detail dialog.
- **Verification:** **partially-confirmed** — Dead code confirmed: rg finds zero importers for src/components/usage/RequestDetailPanel.tsx (324 lines), usage/DataSourceBar.tsx (125), proxy/CircuitBreakerConfigPanel.tsx (356; src/components/proxy/index.ts exports only ProxyPanel), skills/RepoManager.tsx (202; SkillsPage.tsx:27 imports RepoManagerPanel instead), prompts/PromptFormModal.tsx (164; PromptPanel.tsx:8 imports PromptFormPanel), src/hooks/useProxyConfig.ts (48) and useProviderLimits (src/lib/query/usage.ts:349); RequestLogTable.tsx:197-316 rows carry no onClick, RequestDetailPanel.tsx:22-29 hand-rolls the locale map and uses light-only bg-gray-100/bg-green-100/bg-red-50 classes (:35, :145-146, :313), and package.json has no knip/ts-prune. Details off: both useProxyConfig copies are dead (src/lib/query/proxy.ts:137 has no callers either), ProviderLimitStatus lives at src/types/usage.ts:139-148 not :220-228, and the impact is overstated because session-log import already runs automatically at startup and on a timer in src-tauri/src/lib.rs:1105-1148 (DataSourceBar would only add a manual trigger) while circuit-breaker settings are reachable through AutoFailoverConfigPanel.tsx:386-480. Unreachable code with no user-facing breakage is maintenance debt, so P2 rather than P1; the wire-or-delete proposal plus a knip check is sound and not implemented.

### FE-UI-06: zh-TW locale is missing 30 keys (tool lifecycle strings fall back to English)

**Type:** bug · **Priority:** P2 · **Effort:** S · **Confidence:** high

- **Evidence:** key-set diff of `src/i18n/locales/zh-TW.json` vs `en.json` → 30 missing (`settings.manualInstallCommands`, `settings.updateAllTools`, `settings.currentVersion`, `settings.latestVersion`, `settings.toolInstall`, `settings.toolUpdate`, `settings.toolActionDone`, …); `src/i18n/index.ts:82` `fallbackLng: "en"`; `zh.json`/`ja.json` are complete.
- **Problem:** Traditional-Chinese users see mixed English in the About tab's tool cards and batch-update toasts.
- **Proposal:** Add the keys; add a vitest that asserts all locale files share the same flattened key set (fail CI on drift).
- **Acceptance:** Locale key-parity test passes for all four files.

### FE-UI-07: Advanced-tab "Save" has no dirty tracking and no unsaved-changes guard

**Type:** ux · **Priority:** P2 · **Effort:** M · **Confidence:** high

- **Evidence:** `src/components/settings/SettingsPage.tsx:522-543` — Save button always enabled (`disabled={isSaving}` only); `src/App.tsx:899-904` — `onOpenChange={() => setCurrentView("providers")}` navigates away without checking for pending edits; `appConfigDir` edits live only in `useDirectorySettings` state (`src/hooks/useDirectorySettings.ts:129-131,267-272`) and are lost on navigation.
- **Problem:** Users cannot tell whether there is anything to save; leaving the Settings view discards the app-config-dir override silently; the Save button sits under *all* Advanced accordions (import/export, backups, cloud sync, log config) although only the directory section needs it.
- **Proposal:** Track dirty state (compare payload vs `["settings"]` cache + initial app dir), disable Save when clean, show an "Unsaved changes" chip, and prompt (ConfirmDialog) on navigation/ESC when dirty; move the Save bar inside the directory accordion.
- **Acceptance:** RTL tests: Save disabled when clean; navigating away with a dirty app dir shows a confirm; confirming discard restores the server value.

### FE-UI-10: Config panels that fetch with ad-hoc `useEffect` show defaults on fetch failure and may write them back

**Type:** bug · **Priority:** P2 · **Effort:** M · **Confidence:** medium

- **Evidence:** `src/components/settings/RectifierConfigPanel.tsx:133-157` — `useState` defaults (`enabled: true`, …) and two independent fetches whose failures only `console.error`; `:159-181` — any toggle then sends `{ ...config, ...updates }` built from those defaults; same pattern in `src/components/settings/LogConfigPanel.tsx:19-43` and `src/components/usage/ConnectivityCheckConfigPanel.tsx:27-70` (`parseNum` substitutes defaults for blank/invalid input silently).
- **Problem:** If `get_rectifier_config`/`get_optimizer_config` fails (or is slow and the user toggles), the UI presents defaults as the real state and the next toggle overwrites the backend with defaults; blank numeric inputs are "repaired" to defaults without telling the user.
- **Proposal:** Move these to React Query (`useQuery` + `useMutation`, disabled controls until `data` is loaded, error state rendered), and validate numeric fields (FE-UI-21) instead of silently defaulting.
- **Acceptance:** With `get_rectifier_config` rejected, switches are disabled and an error alert is shown; no `set_rectifier_config` call is possible.

### FE-UI-11: Hover-only action bars and nested interactive elements are not keyboard reachable

**Type:** ux · **Priority:** P2 · **Effort:** S · **Confidence:** high

- **Evidence:** `src/components/mcp/UnifiedMcpPanel.tsx:293` and `src/components/skills/UnifiedSkillsPanel.tsx:563-566` — edit/delete buttons in `opacity-0 group-hover:opacity-100` wrappers with no `group-focus-within:opacity-100` (only `ProviderCard` uses it); `src/components/workspace/DailyMemoryPanel.tsx:474-482,523-531` — a `<div onClick>` delete icon nested inside a `<button>` (invalid HTML, no keyboard access); `src/components/common/AppToggleGroup.tsx:29-37` — icon-only `<button>` with no `aria-label`/`aria-pressed` (the Tooltip is hover-only); `src/components/settings/AppVisibilitySettings.tsx:118-144`, `LanguageSettings.tsx:23-39`, `ThemeSettings.tsx:19-41`, `SkillSyncMethodSettings.tsx:28-41`, `SkillStorageLocationSettings.tsx:80-98` — segmented buttons without `role="radiogroup"`/`aria-pressed`.
- **Problem:** Screen-reader and keyboard users cannot discover or operate per-row actions and cannot tell which option is selected.
- **Proposal:** Add `group-focus-within:opacity-100` (or always-visible on `:focus-visible`), convert the nested delete div into a sibling `<button aria-label>`, give `AppToggleGroup` buttons `aria-label={label}` + `aria-pressed`, and extract a `SegmentedControl` (FE-UI-22) with proper radio semantics.
- **Acceptance:** `jest-axe`/`vitest-axe` passes on these panels; Tab reaches edit/delete buttons in MCP and Skills lists.

### FE-UI-12: Hand-rolled modals/panels lack dialog semantics and focus management

**Type:** ux · **Priority:** P2 · **Effort:** M · **Confidence:** high

- **Evidence:** `src/components/skills/UnifiedSkillsPanel.tsx:786-872` — `ImportSkillsDialog` is a `fixed inset-0` div with `<h2>`, native `<input type="checkbox">` and no `role="dialog"`, focus trap or ESC; `src/components/common/FullScreenPanel.tsx:89-96,60-84` — portal `motion.div` without `role="dialog"`/`aria-modal`/initial focus (only a manual ESC listener); `src/components/ui/dialog.tsx:120-123` — `onInteractOutside` is `preventDefault`-ed for *every* dialog app-wide, so no dialog can be dismissed by clicking the overlay (including purely informational ones).
- **Problem:** Focus stays behind the full-screen panels, the import dialog cannot be closed with ESC, and the global outside-click block is surprising for non-destructive dialogs.
- **Proposal:** Build `ImportSkillsDialog` on `Dialog`; give `FullScreenPanel` `role="dialog" aria-modal aria-labelledby` and focus the back button on open (restore focus on close); make outside-click dismissal opt-out per dialog (`dismissOnOutsideClick` prop) defaulting to allowed for info dialogs.
- **Acceptance:** Opening a FullScreenPanel moves focus inside; ESC closes ImportSkillsDialog; axe reports no "dialog without name" violations.

### FE-UI-15: i18n misuse — hard-coded strings, concatenated translations, inline Chinese defaults, unlocalised number/date formatting

**Type:** tech-debt · **Priority:** P2 · **Effort:** M · **Confidence:** high

- **Evidence:** hard-coded UI text: `src/components/settings/BackupListSection.tsx:296` ("Loading..."), `src/components/agents/AgentsPanel.tsx:14-18` ("Coming Soon" paragraph), `src/components/usage/RequestLogTable.tsx:248` (`Raw: …` tooltip); concatenation `t(...) + ": " + String(error)` in `McpFormModal.tsx:265,341`, `ConnectivityCheckConfigPanel.tsx:66`, `AutoFailoverConfigPanel.tsx:182`, `CircuitBreakerConfigPanel.tsx:131`, `FailoverQueueManager.tsx:85,100-103`; two-argument `t("key", "中文")` defaults (36 in `RequestDetailPanel.tsx`, 25 in `AutoFailoverConfigPanel.tsx`, 24 in `CircuitBreakerConfigPanel.tsx`, 14 in `PricingEditModal.tsx` …) so source-of-truth is split between JSX and `zh.json`; `toLocaleString()` with no locale in 20+ places (`ProviderStatsTable.tsx:82-85`, `ModelStatsTable.tsx:79-82`, `UsageHero.tsx:277-295`, `BackupListSection.tsx:44`, `WebdavSyncSection.tsx:153,939,953`, `UnifiedSkillsPanel.tsx:64`) although `getLocaleFromLanguage` exists in `src/components/usage/format.ts:47-57`; `fmtUsd` hard-codes `$` (`format.ts:24-32`) and `ProviderStatsTable.tsx:94` builds `${avgLatencyMs}ms`.
- **Problem:** RTL/plural/ordering issues in non-English locales, untranslatable error strings, and the app language is ignored for numbers/dates (a zh-TW user on an en-US OS sees en-US dates).
- **Proposal:** ESLint `i18next/no-literal-string` + a rule banning the two-arg `t` form in components; central `formatNumber/formatDate` helpers taking the i18n locale; pass error as interpolation (`t("x.failed", { error })`).
- **Acceptance:** Lint passes with the new rules; no `toLocaleString()` without locale in `src/components`.

### FE-UI-16: XL refactor — split `WebdavSyncSection` into a backend-agnostic cloud-sync panel

**Type:** refactor · **Priority:** P2 · **Effort:** XL · **Confidence:** high

- **Evidence:** `src/components/settings/WebdavSyncSection.tsx` is 1,867 lines with ~25 `useState` hooks (`:242-316`); every handler is mirrored for S3 — `handleTest/handleSave/handleUploadClick/handleUploadConfirm/handleDownloadClick/handleDownloadConfirm` (`:472-649`) vs `handleS3Test/…/handleS3DownloadConfirm` (`:697-861`); the two forms (`:995-1227` WebDAV, `:1230-1539` S3) and four confirmation dialogs (`:1542-1853`) are copy-pasted; the remote path `…/v2/db-v6/…` is hard-coded in the UI (`:944-945`); `queryClient.invalidateQueries()` with no key (invalidate *everything*) is called after every save/upload/download (`:517,580,640,736,796,851,909`); Synology preset detection matches any URL containing `:5005` (`:70-75`).
- **Problem:** Every behaviour fix must be applied twice (the S3 branch already lacks the password-preservation logic and the legacy-layout notices the WebDAV branch has), tests only cover WebDAV (`tests/components/WebdavSyncSection.test.tsx` has zero `s3` references), and adding a third backend (e.g. GitHub Gist, local folder) means a third copy.
- **Proposal:** Target structure: `src/components/settings/cloudSync/` with `useCloudSyncForm(adapter)` (dirty/touched/justSaved/actionState machine), `CloudSyncBackendAdapter` interface (`test`, `save`, `fetchRemoteInfo`, `upload`, `download`, `buildRemotePath`) implemented by `webdavAdapter.ts`/`s3Adapter.ts`, presentational `SyncFormFields` driven by a field schema, `RemoteSnapshotDialog` (upload/download modes), and `SyncTypeSelector` with mutual-exclusion logic. Backend returns the remote path string. Invalidate only `["settings"]` and sync-related keys.
- **Acceptance:** File ≤ 400 lines per module; parity test suite runs the same behavioural tests against both adapters; S3 gains the password-preservation and incompatible-version handling automatically.
- **Verification:** **partially-confirmed** — Confirmed: src/components/settings/WebdavSyncSection.tsx is 1,867 lines with 27 useState calls in :242-316; the WebDAV handlers sit at :472/:493/:546/:570/:593/:630 and their S3 mirrors at :697-:861 exactly as cited; queryClient.invalidateQueries() with no filter appears at :517, :580, :640, :736, :796, :851 and :909; the /v2/db-v6/ path is hard-coded at :944-945; and the Synology preset uses matchPattern ':5005' (:70-75) through url.includes in detectPreset (:85-92). tests/components/WebdavSyncSection.test.tsx (558 lines, 18 cases) has zero 's3' matches, and the S3 dialogs at :1700-1853 lack the legacyNotice/dbCompat blocks the WebDAV dialogs render at :1595-1614 and :1667-1683. Detail overstated: the S3 branch does implement secret preservation via s3SecretTouched (:726 passes it to settingsApi.s3SyncSaveSettings) and the backend refills an empty secret from the stored one (src-tauri/src/commands/s3_sync.rs:44-48, src-tauri/src/commands/settings.rs:32-42); what S3 lacks is only the UI-side pendingPasswordPreservationRef carry-over (:352-358, :500-507). No cloudSync/ adapter module exists so the proposal is not implemented, but a pure refactor with no user-facing defect is P2, not P1.

### FE-UI-17: L refactor — consolidate the settings save pipeline (`useSettings` / `useSettingsForm` / `useDirectorySettings`)

**Type:** refactor · **Priority:** P2 · **Effort:** L · **Confidence:** high

- **Evidence:** `src/hooks/useSettings.ts:182-308` (`autoSaveSettings`) and `:312-492` (`saveSettings`) duplicate ~120 lines (sanitisation, auto-launch, onboarding skip, plugin sync, language persistence, tray refresh); `sanitizeDir` is defined three times (`useSettings.ts:49-53`, `useSettingsForm.ts:48-52`, `useDirectorySettings.ts:57-61`); the normalisation block is duplicated in `useSettingsForm.ts:110-129` and `:177-196`; `initialLanguage: initialLanguageRef.current` is read from a ref during render (`useSettingsForm.ts:207`); `DirectorySettings.tsx:310-385` repeats seven near-identical `DirectoryInput` blocks and `SettingsPage.tsx:346-362` passes 14 directory props.
- **Problem:** The bugs in FE-UI-01/02/09 are direct consequences of this duplication; adding a setting with a side effect requires editing two 100-line functions.
- **Proposal:** `settingsPayload.ts` (`normalizeSettings(data)`, `buildSettingsPayload(form)` table-driven over `APP_DIRECTORY_META`), `settingsSideEffects.ts` (`runSideEffects(prev, next, { scope: "auto" | "full" })` with one entry per setting: launchOnStartup, skipClaudeOnboarding, enableClaudePluginIntegration, dir changes, appConfigDir), and a single `useSaveSettings()` that both paths call with a scope flag; `DirectorySettings` iterates `DirectoryAppId[]`.
- **Acceptance:** `useSettings.ts` < 200 lines; existing `tests/hooks/useSettings.test.tsx` (10 cases) and `useSettingsForm.test.tsx` pass unchanged; one new table-driven test covers every `*ConfigDir`.
- **Verification:** **partially-confirmed** — Confirmed: sanitizeDir is defined three times at src/hooks/useSettings.ts:49, useSettingsForm.ts:48 and useDirectorySettings.ts:57; autoSaveSettings (useSettings.ts:182-308) and saveSettings (:312-492) both run setAutoLaunch (:230/:371), the skipClaudeOnboarding toggle (:243-261/:383-399), enableClaudePluginIntegration sync (:272/:410) and updateTrayMenu (:290/:426); the normalisation block is duplicated at useSettingsForm.ts:110-129 and :177-196; and initialLanguageRef.current is read during render at :207. Citation slips: src/components/settings/DirectorySettings.tsx is 242 lines and its seven DirectoryInput blocks are at :98-164, not :310-385; SettingsPage.tsx:346-362 passes 15 props; tests/hooks/useSettings.test.tsx has 9 it() cases, not 10. APP_DIRECTORY_META and DirectoryAppId already exist in useDirectorySettings.ts:8-31 so the table-driven proposal can build on them, and no settingsPayload/settingsSideEffects/useSaveSettings module exists. This is a refactor whose linked bugs (FE-UI-01/02) were themselves verified at P2/P3, so P2.

### FE-UI-18: XL refactor — a shared `ManagedItemsPanel` for MCP, Skills and Prompts (and unify their data layer)

**Type:** refactor · **Priority:** P2 · **Effort:** XL · **Confidence:** high

- **Evidence:** `src/components/mcp/UnifiedMcpPanel.tsx:34-214` and `src/components/skills/UnifiedSkillsPanel.tsx:67-478` both implement: `enabledCounts` memo over `*_APP_IDS` (`UnifiedMcpPanel.tsx:58-75`, `UnifiedSkillsPanel.tsx:116-134`), `AppCountBar` + scrollable list + `ListItemRow` + `AppToggleGroup` + hover actions, a `confirmDialog` state object (`UnifiedMcpPanel.tsx:41-46`, `UnifiedSkillsPanel.tsx:72-79`), `useImperativeHandle` for header actions, and a `FullScreenPanel` form; `src/components/prompts/PromptPanel.tsx:21-168` does the same with a different list item and a non-React-Query store (`src/hooks/usePromptActions.ts:1-152`, local `useState` + manual `reload()` after every mutation, `toggleEnabled` depending on the `prompts` closure at `:126`); `PromptFormPanel.tsx:27-36` and the dead `PromptFormModal.tsx:33-41` both hard-code a per-app filename map.
- **Problem:** Three different implementations of the same shell with three different loading/error/empty/confirm behaviours; Prompts does not benefit from query caching, invalidation on `profile-applied` is manual, and every UX fix (keyboard access, search, sorting) must be done three times.
- **Proposal:** `ManagedItemsPanel<T>` with an adapter `{ listQuery, appIds, getId, renderPrimary, renderSecondary, onToggleApp, actions[] , form: ComponentType }`, shared `useConfirm()` hook, built-in search/filter, and migrate `usePromptActions` to React Query (`["prompts", appId]`) with optimistic toggle. Keep MCP/Skills/Prompts specific extras (update badge, docs link) as adapter render slots.
- **Acceptance:** Three panels share one list/confirm implementation; Prompts list refreshes via query invalidation on `profile-applied`; a single RTL suite exercises toggle/edit/delete through the generic panel for all three adapters.

### FE-UI-19: L refactor — decompose `AboutSection` (app update + tool lifecycle + WSL prefs + diagnostics + install commands)

**Type:** refactor · **Priority:** P2 · **Effort:** L · **Confidence:** high

- **Evidence:** `src/components/settings/AboutSection.tsx` is 1,272 lines: module-level cache (`:194-197`) re-implementing React Query's `staleTime`; 14 `useState` hooks (`:220-269`); `executeRun` (`:567-737`) and `handleRunToolAction` (`:747-793`) encode a serial install/upgrade state machine with soft/hard failure classification; the render (`:815-1272`) inlines the tool-card grid with per-card loading, WSL shell selects and conflict lists; install-command strings are built at module load for the current OS (`:109-160`); stale comments still say "6 tools" (`:348,395,537`) although `TOOL_NAMES` has seven (`:62-70`).
- **Problem:** Impossible to unit test the lifecycle logic without rendering the whole About tab; no tests exist; the module-level cache survives across tests and sessions.
- **Proposal:** `useToolVersions()` (React Query, `staleTime: 10 min`, per-tool query keys so cards update independently), `useToolLifecycle()` (install/update/preflight/diagnose state machine, pure functions `classifyRunResult` and `summarizeFailures` unit-tested), `ToolCard`, `AppUpdateCard`, `InstallCommandsCard`, `WslShellSelect`; keep `AboutSection` as a 150-line composition.
- **Acceptance:** Each new file ≤ 300 lines; unit tests for `classifyRunResult` (version unchanged / not runnable / hard failure) and for batch summary toasts; tab switch still reuses cached versions.

### FE-UI-20: Numeric config panels hand-roll validation with duplicated range tables; adopt zod + react-hook-form

**Type:** refactor · **Priority:** P2 · **Effort:** M · **Confidence:** high

- **Evidence:** `src/components/proxy/AutoFailoverConfigPanel.tsx:57-159` and `src/components/proxy/CircuitBreakerConfigPanel.tsx:43-134` contain the same `parseNum`/`ranges`/`checkRange`/`errors.join("; ")` code; numbers are stored as strings in `useState` (`AutoFailoverConfigPanel.tsx:24-36`); errors are surfaced only as a toast listing `label: min-max`, never next to the field; the streaming-idle hint says "60-600 s or 0" (`:347-352`) while the range accepts 1-59 (`:71`); `ConnectivityCheckConfigPanel.tsx:48-70` and `ProxyPanel.tsx:125-199` (address/port) do their own thing; `react-hook-form`, `@hookform/resolvers` and `zod` are already dependencies and `src/components/ui/form.tsx` exists but is unused in this scope.
- **Problem:** Inconsistent validation UX, duplicated range constants that can drift from the backend, and no field-level error messages.
- **Proposal:** Export a `proxyConfigSchema` (zod) per panel, use `useForm({ resolver: zodResolver })` with `FormField/FormMessage`, show dirty state and disable Save when pristine; share the circuit-breaker sub-schema between app-level and global panels (or delete the global one, FE-UI-05).
- **Acceptance:** Field-level messages appear under the offending input; schema unit tests cover boundaries; hint text generated from schema metadata so it cannot drift.

### FE-UI-24: Failover queue manager lacks reorder, health, test and reason-for-switch feedback

**Type:** feature · **Priority:** P2 · **Effort:** M · **Confidence:** high

- **Evidence:** `src/components/proxy/FailoverQueueManager.tsx:216-249` — queue is a static list with delete only; order "follows the main page" (`:241-249` hint); no `ProviderHealthBadge` (the one in `ProxyPanel.tsx:749-752` is only visible when the proxy is running and only in the Local Proxy accordion); `:79-104` success/error toasts are the only feedback; `src/hooks/useStreamCheck.ts` exists but is not exposed here; `ProxyStatus.failover_count/last_error` (`src/types/proxy.ts:27-28`) are never rendered.
- **Problem:** Users configure a failover order in one place and see its health in another; they cannot drag to reorder within the queue, cannot test a provider from the queue, and never learn *why* a failover happened.
- **Proposal:** Use `@dnd-kit` (already in use elsewhere) for in-queue reordering with a `reorder_failover_queue` command; show health/circuit badge and "last failure: reason, time" per item; add "Test" (stream check) and "Reset circuit" actions (`useResetCircuitBreaker` exists in `lib/query/failover.ts:271-297`); surface `failover_count`/`last_error` in the panel header.
- **Acceptance:** Drag reorder persists; each queue item shows health + last error; test button runs `useStreamCheck`.

### FE-UI-25: Usage dashboard has no export (CSV/JSON) of logs or aggregates

**Type:** feature · **Priority:** P2 · **Effort:** M · **Confidence:** high

- **Evidence:** `src/lib/api/usage.ts:20-120` exposes only query/summary/trends/stats/logs commands; `rg "csv|export" src/components/usage` → nothing; `RequestLogTable.tsx:323-399` only paginates 20 rows at a time; no i18n key `usage.export`.
- **Problem:** Users cannot take their cost data into spreadsheets or share it; the only way out is screenshots.
- **Proposal:** Add `export_usage_logs(filters, range, format)` writing CSV/JSON via the save dialog (`settingsApi.saveFileDialog` pattern from `useImportExport.ts:207`), plus "Copy as CSV" for the stats tables; include the current global filters in the export.
- **Acceptance:** Export button in the dashboard header produces a CSV whose row count equals `total` for the active filter.

### FE-UI-26: Usage budgets/alerts — backend limit types exist but no UI

**Type:** feature · **Priority:** P2 · **Effort:** L · **Confidence:** medium

- **Evidence:** `src/types/usage.ts:220-228` (`ProviderLimitStatus` with daily/monthly limit + exceeded flags) and `src/lib/query/usage.ts:349` (`useProviderLimits`) are unused by any component; `UsageHero.tsx:288-307` shows totals with no target; no settings for thresholds.
- **Problem:** The data model for budgets is half-built but users get no warning before overspending.
- **Proposal:** Budget settings (per app/provider daily/monthly USD), a progress bar + "exceeded" badge in `UsageHero`, a tray/toast alert on threshold crossing (backend event), and a "projected month-end" line in `UsageTrendChart`.
- **Acceptance:** Setting a daily budget below today's spend shows the exceeded state in the hero and a notification.

### FE-UI-29: `PricingConfigPanel` omits OpenCode, fires 8 unguarded writes, and refetches on language change

**Type:** bug · **Priority:** P2 · **Effort:** S · **Confidence:** high

- **Evidence:** `src/components/usage/PricingConfigPanel.tsx:36` `PRICING_APPS = ["claude","codex","gemini","grokbuild"]` while `src/types/usage.ts:260-266` `KNOWN_APP_TYPES` includes `opencode` (and the dashboard filter shows it); `:78-135` loads multiplier+source with `Promise.all` and the effect depends on `[t]` so switching language reloads and resets edits; `:156-166` saves 8 commands via `Promise.all` — a partial failure leaves the backend half-updated while `originalConfigs` is not updated; `:144,150` build toasts by string interpolation of two translations.
- **Problem:** OpenCode traffic cannot be given a cost multiplier/pricing source from the UI; partial save failures are invisible.
- **Proposal:** Derive apps from `KNOWN_APP_TYPES`; add a single `set_pricing_defaults(map)` command (atomic) or `Promise.allSettled` with per-app error reporting; use `useQuery` keyed by app for loading; validate with zod (FE-UI-20).
- **Acceptance:** OpenCode row visible; partial failure surfaces which app failed and keeps dirty state for it.

### FE-UI-30: Manual pricing entry is not normalised like the models.dev import, so manual IDs may never match usage rows

**Type:** bug · **Priority:** P2 · **Effort:** S · **Confidence:** medium

- **Evidence:** `src/components/usage/ModelsDevPickerDialog.tsx:76-84` `normalizeModelIdForPricing` (strip provider prefix, `:tag`, `@` → `-`, lowercase, `[1m]`) mirrors the backend `clean_model_id_for_pricing`; `src/components/usage/PricingEditModal.tsx:41-86` submits `formData.modelId` raw (`:67`) with only a non-empty check; the inputs are `required type=number` (`:141-243`) but validation errors are toasts, not field messages.
- **Problem:** A user who types `openai/gpt-4o` or `Claude-3-5-Sonnet@20241022` creates a pricing row that the cost attribution query never matches, with no warning.
- **Proposal:** Normalise on submit (show the normalised ID as a hint under the field), warn on duplicates against `useModelPricing()`, and use field-level validation.
- **Acceptance:** Entering `openai/gpt-4o` stores `gpt-4o`; duplicate ID shows an inline error.

### FE-UI-32: MCP wizard/form gaps — no `cwd`, plain-text env secrets, silent line drops, no connection test, macOS-only shortcut, OpenClaw toggle missing in form

**Type:** feature · **Priority:** P2 · **Effort:** M · **Confidence:** high

- **Evidence:** `src/types.ts:473-486` `McpServerSpec` supports `cwd` but `McpWizardModal.tsx:84-134` has no field; `:27-70` `parseEnvText`/`parseHeadersText` silently discard lines without `=`/`:`; env values are a plain `<textarea>` (`:349-360`) with no secret masking and are echoed into the JSON preview (`:400-414`); `:167-172` applies on `metaKey+Enter` only (no `ctrlKey` → shortcut dead on Windows/Linux); `McpFormModal.tsx:525-622` renders six app checkboxes (claude, codex, gemini, grokbuild, opencode, hermes) while `enabledApps` state includes `openclaw` (`:64-88`) — OpenClaw can only be toggled from the list, never in the form; `:712-725` uses `language="javascript"` for TOML (no TOML grammar); `:103-108` `useTomlFormat` memo has identical branches; `useMcp.ts:66-76` import is "from apps" only — no paste/import of a Claude Desktop / Cursor `mcpServers` file, and there is no "test server" action anywhere.
- **Problem:** Users cannot set a working directory, cannot verify a server starts, get no feedback for malformed env lines, and OpenClaw enablement is inconsistent between list and form.
- **Proposal:** Add `cwd` + per-row env/header editors with masked values and inline parse errors; add "Test connection" (`mcp_probe_server` command spawning stdio / HEAD on http) with latency/result badge in the list; "Import from file/clipboard" accepting `mcpServers` wrappers (reuse `parseSmartMcpJson`); render app checkboxes from `MCP_APP_IDS`; support `ctrlKey`; use `@codemirror/lang-toml` (or legacy mode) for TOML.
- **Acceptance:** Wizard produces `cwd`; malformed env line shows an error; Ctrl+Enter applies on Linux; OpenClaw checkbox present; test button returns ok/fail within timeout.

### FE-UI-33: Skills UX — discovery "Uninstall" only toasts, no install progress, sequential update-all without progress, conflict/symlink explanation missing

**Type:** feature · **Priority:** P2 · **Effort:** M · **Confidence:** high

- **Evidence:** `src/components/skills/SkillsPage.tsx:277-280` `handleUninstall` shows `skills.uninstallInMainPanel` while `SkillCard.tsx:133-147` still renders an active "Uninstall" button; `useSkills.ts:69-110` install is a single await with no progress events (`rg "progress|listen(" src/lib/api/skills.ts` → none); `UnifiedSkillsPanel.tsx:259-279` `handleUpdateAll` loops serially with one toast per failure and no progress indicator; conflicts surface only as a toast via `formatSkillError` (`SkillsPage.tsx:262-273`, `SKILL_DIRECTORY_CONFLICT` in `src/lib/errors/skillErrorParser.ts`); the symlink-vs-copy choice is explained only in Settings (`SkillSyncMethodSettings.tsx:42-46`), not at install/toggle time; update checks are manual (`useSkills.ts:298-305`, `enabled: false`).
- **Problem:** Dead-end button in discovery; multi-MB installs look frozen; conflicts offer no "overwrite/rename/open folder" choices; users do not understand why toggling an app for a skill may copy files.
- **Proposal:** Hide/replace the discovery Uninstall with "Manage"; emit `skill-install-progress` Tauri events (download/extract/link phases) and show a per-card progress bar; make update-all a single backend batch with a progress dialog; conflict dialog with Overwrite / Keep both / Open folder; show a one-line "linked (symlink) / copied" badge on each installed skill with a tooltip; optional background update check with a badge count (reuse `useCheckSkillUpdates`).
- **Acceptance:** Installing a large skill shows phase progress; conflict dialog appears for duplicate directory; badge shows sync method per skill.

### FE-UI-34: OpenClaw/workspace editors reset local edits on refetch and have no unsaved-changes guard

**Type:** bug · **Priority:** P2 · **Effort:** S · **Confidence:** medium

- **Evidence:** `src/components/openclaw/EnvPanel.tsx:18-24` `useEffect(() => setEditorValue(JSON.stringify(envData)), [envData])`; `ToolsPanel.tsx:40-56` and `AgentsDefaultsPanel.tsx:44-66` do the same for their forms; queries use `staleTime: 30_000` (`src/hooks/useOpenClaw.ts:59,70,81`) and the client defaults to `refetchOnWindowFocus: true` (`src/lib/query/queryClient.ts:7`); `WorkspaceFileEditor.tsx:54-65`, `DailyMemoryPanel.tsx:281-289` back/close without checking for dirty content; by contrast `HermesMemoryPanel.tsx:42-47` guards with a `loaded` flag (correct pattern).
- **Problem:** If `openclaw.json` changes on disk (external edit, or the backend normalising on save) while the user is editing, the editor content is replaced without warning; leaving an editor with unsaved Markdown loses it silently.
- **Proposal:** Hydrate once (`loaded` flag) and show a "file changed on disk — reload?" banner when `data` differs from the hydrated baseline; add dirty tracking + ConfirmDialog on close/back for all editors; share a `useEditableBuffer(data)` hook.
- **Acceptance:** Simulated refetch with different data while dirty shows the banner instead of replacing text; closing dirty editor prompts.

### FE-UI-42: Test gaps — no component tests for proxy, usage charts/pricing, about, backups, log/rectifier, OpenClaw, workspace, prompts, Hermes; S3 branch untested

**Type:** test · **Priority:** P2 · **Effort:** L · **Confidence:** high

- **Evidence:** `rg -l "ProxyPanel|FailoverQueueManager|AutoFailoverConfigPanel|AgentsDefaultsPanel|DailyMemoryPanel|WorkspaceFilesPanel|PromptPanel|HermesMemoryPanel|BackupListSection|AboutSection|LogConfigPanel|RectifierConfigPanel|TerminalSettings|UsageTrendChart|UsageHero|PricingConfigPanel|UsageDateRangePicker" tests` matches only MSW handlers and the SettingsDialog tests (which mock children); `tests/components/WebdavSyncSection.test.tsx` (558 lines) has zero `s3` references; `tests/components/UnifiedSkillsPanel.test.tsx` has one case (`:99`); `tests/hooks/useProxyStatus.test.tsx` one case; `tests/components/UsageDashboard.test.tsx` covers only the refresh interval; `useImportExport.test.tsx:214` locks in the cancel-as-error behaviour (FE-UI-08).
- **Problem:** The highest-risk interactive surfaces (failover, pricing, cloud sync S3, editors with dirty state) can regress silently; several bugs above (FE-UI-01/02/03) have no failing test today.
- **Proposal:** Add RTL tests per panel for load/error/empty/save paths; parametrise the Webdav suite over both adapters (after FE-UI-16); add `vitest-axe` smoke tests for every panel; add the locale-parity test (FE-UI-06) and a `knip` check (FE-UI-05).
- **Acceptance:** Coverage report shows ≥ 70 % statements for `src/components/{proxy,usage,openclaw,workspace,prompts,hermes}` and `src/components/settings/{BackupListSection,AboutSection,Log*,Rectifier*}`.
- **Verification:** **partially-confirmed** — Confirmed: the evidence rg -l matches only tests/msw/handlers.ts, tests/components/SettingsDialog.test.tsx, tests/integration/SettingsDialog.test.tsx and tests/components/UsageDashboard.test.tsx, and the latter three mock the named children (SettingsDialog.test.tsx:236-240, UsageDashboard.test.tsx:48-72); WebdavSyncSection.test.tsx has 0 's3' hits, UnifiedSkillsPanel.test.tsx has a single it() at :99, useProxyStatus.test.tsx one case, UsageDashboard.test.tsx three cases all about the refresh interval (:116-138), and useImportExport.test.tsx:214-224 asserts toast.error fires once when the save dialog is cancelled. vitest.config.ts:16-18 configures coverage reporters only with no thresholds, and package.json has neither vitest-axe nor knip. Detail off: usage is not wholly untested because tests/components/PricingEditModal.test.tsx, RequestLogTable.test.tsx, ModelsDevPickerDialog.test.ts and usageFormat.test.ts exist; the real gap is UsageTrendChart/UsageHero/PricingConfigPanel/UsageDateRangePicker plus all of src/components/{proxy,openclaw,workspace,prompts,hermes,agents}. A coverage gap is not a defect, so P2; the 70% target is reasonable but arbitrary.

### FE-UI-02: `hermesConfigDir` is skipped by sanitisation, form normalisation and directory-change detection

**Type:** bug · **Priority:** P3 · **Effort:** S · **Confidence:** high

- **Evidence:** `src/types.ts:405` declares `hermesConfigDir`; `src/hooks/useDirectorySettings.ts:44-55` maps `hermes → "hermesConfigDir"`; but `src/hooks/useSettings.ts:188-213` and `:320-353` sanitise only claude/codex/gemini/grok/opencode/openclaw; `src/hooks/useSettings.ts:433-447` (`claudeDirChanged || … || openclawDirChanged`) omits hermes so changing the Hermes directory never triggers `syncCurrentProvidersLiveSafe()`; `src/hooks/useSettingsForm.ts:122-127` and `:189-194` likewise omit `hermesConfigDir` from `sanitizeDir`.
- **Problem:** Hermes directory overrides can be saved as `"   "` (whitespace) and changing them does not resync the live config; the add-an-app checklist was missed in three hand-maintained lists.
- **Proposal:** Derive all per-app directory handling from `APP_DIRECTORY_META`/`DIRECTORY_KEY_TO_SETTINGS_FIELD` (iterate the table in `buildPayload`, `normalizeForm`, `detectDirChanges`) so new apps cannot be missed; add a unit test that iterates `DirectoryAppId` and asserts each field is sanitised and change-detected.
- **Acceptance:** `useSettings.test.tsx` case "changing hermesConfigDir triggers live sync"; `tsc` type-level check that the payload builder covers `keyof Pick<Settings, `${string}ConfigDir`>`.
- **Verification:** **partially-confirmed** — The omissions are real: useSettings.ts:188-211 and :320-351 sanitise six dirs and let hermesConfigDir pass through ...restSettings, :433-446 omit hermes from change detection, and useSettingsForm.ts:122-127 and :189-194 omit it too. The whitespace claim is refuted: the only UI writer, useDirectorySettings.ts:245-254, runs sanitizeDir before onUpdateSettings, and the backend AppSettings::normalize_paths trims hermes_config_dir and maps empty to None (src-tauri/src/settings.rs:610-615), invoked on every save (settings.rs:748-749) and on load (settings.rs:646), so '   ' can be neither persisted nor served to the form. The live-sync omission is real and meaningful: sync_current_to_live (src-tauri/src/services/provider/live.rs:1278-1290) projects all Hermes providers (additive mode, app_config.rs:404-409) into the resolved dir, so a Hermes-only directory change saved via Save does not re-project providers into the new directory. Impact is a stale projection until the next Hermes provider operation, with no corruption, so P3. The table-driven proposal is sound and not implemented; note src has no useSettings.test.tsx (only 4 test files exist), so the acceptance test would have to be created.

### FE-UI-04: Failover/takeover UI covers different app sets in different places; label maps diverge

**Type:** bug · **Priority:** P3 · **Effort:** M · **Confidence:** high

- **Evidence:** `src/components/settings/ProxyTabContent.tsx:449-454` failover tabs are only claude/codex/gemini; `src/components/proxy/ProxyPanel.tsx:277-301` takeover switches for claude/codex/gemini/grokbuild and `:72-75` queues for four apps; `src/types/proxy.ts:44-53` `ProxyTakeoverStatus` has eight apps incl. opencode/openclaw/hermes; `src/hooks/useProxyStatus.ts:285-294` label map falls back to `"OpenCode"` for unknown apps while `src/components/proxy/FailoverToggle.tsx:131-138` and `src/lib/query/failover.ts:468-475` fall back to `"Gemini"`, `src/components/proxy/ProxyToggle.tsx:40-49` to `"OpenCode"`, and `src/components/proxy/ProxyPanel.tsx:288-290` uses CSS `capitalize` on the raw id; `src/hooks/useProxyStatus.ts:373-378` and `src/components/proxy/ClaudeDesktopRouteToggle.tsx:209-214` compute "any takeover active" from only four apps; `src/components/proxy/ProxyPanel.tsx:83,88` toasts the raw `appType` id.
- **Problem:** A user can take over Grok Build but cannot configure its failover queue in Settings; if the backend ever reports opencode/openclaw takeover the "is anything taken over" guards are wrong; toast/labels show `grokbuild` or the wrong app name.
- **Proposal:** Introduce `PROXY_CAPABLE_APPS` (single source, ideally from the backend) and `getAppLabel(appId)` backed by `APP_ICON_MAP[app].label` (`src/config/appConfig.tsx:42-114`); drive tabs, takeover grid, queues and guards from it.
- **Acceptance:** Grep shows no inline `appType === "claude" ? "Claude" :` chains; ProxyTabContent and ProxyPanel render the same app list; unit test for `getAppLabel("grokbuild") === "Grok Build"`.
- **Verification:** **partially-confirmed** — Cited line numbers are wrong (ProxyTabContent.tsx has 285 lines, useProxyStatus.ts 248, FailoverToggle.tsx 87), but the code exists: ProxyTabContent.tsx:174-180 renders failover tabs for claude/codex/gemini only; ProxyPanel.tsx:72-75 and :277-290 cover four apps with CSS capitalize on the raw id, and :80-91 toast the raw appType (en.json:2580 '{{app}} routing enabled' renders 'grokbuild routing enabled'); label fallbacks diverge at useProxyStatus.ts:128-137 ('OpenCode'), FailoverToggle.tsx:36-43 and failover.ts:225-232 ('Gemini'), ProxyToggle.tsx:35-44 ('OpenCode'); four-app guards sit at useProxyStatus.ts:216-221 and ClaudeDesktopRouteToggle.tsx:27-32. Most of it is latent rather than user-visible: the backend ProxyTakeoverStatus has only six fields and hard-codes opencode/openclaw=false (src-tauri/src/proxy/types.rs:112-119, services/proxy.rs:711-713; other apps are rejected with '该应用不支持代理功能' at services/proxy.rs:960-966), so the four-app guards are correct in practice, and App.tsx:1250-1268 never renders ProxyToggle/FailoverToggle for opencode/openclaw/hermes, so the wrong fallback labels are unreachable. The Grok Build gap is only partly real: its failover queue is manageable from the Home provider list (ProviderList.tsx:149-188 uses appId generically with useAddToFailoverQueue) and the auto-failover switch is in the header (App.tsx:1266-1267), but AutoFailoverConfigPanel (per-app retries/timeouts/circuit breaker) is only mounted in ProxyTabContent.tsx:204 for three apps, so grokbuild's per-app proxy config cannot be edited anywhere. Net effect is cosmetic raw-id toasts plus one feature gap, so P3; the PROXY_CAPABLE_APPS/getAppLabel proposal is sound and not implemented (labels already exist in APP_ICON_MAP, appConfig.tsx:42-128).

### FE-UI-08: Cancelling the export file dialog shows an error toast (and the test enshrines it)

**Type:** ux · **Priority:** P3 · **Effort:** S · **Confidence:** high

- **Evidence:** `src/hooks/useImportExport.ts:207-215` — `if (!destination) { toast.error(t("settings.selectFileFailed", …)); return; }`; `tests/hooks/useImportExport.test.tsx:214` "should show error and return when user cancels save dialog during export".
- **Problem:** Pressing Cancel in a native dialog is not an error; the message key reused is "select file failed".
- **Proposal:** Return silently (or `toast.info` at most) on cancel; update the test to assert no error toast.
- **Acceptance:** Cancelling export produces no toast; test updated.

### FE-UI-09: Autosave failure shows two error toasts

**Type:** bug · **Priority:** P3 · **Effort:** S · **Confidence:** high

- **Evidence:** `src/hooks/useSettings.ts:296-305` — catch block toasts `notifications.settingsSaveFailed` then rethrows; `src/components/settings/SettingsPage.tsx:196-208` — catch toasts `settings.saveFailedGeneric` again.
- **Problem:** Every failed General-tab toggle produces two stacked toasts with different wording.
- **Proposal:** Let the hook own the toast and have `handleAutoSave` only roll back state (or vice-versa); return a result object instead of throwing.
- **Acceptance:** Mocked failing `save_settings` → exactly one toast.

### FE-UI-13: No reduced-motion handling; light-only skeleton and status colours in dark mode

**Type:** ux · **Priority:** P3 · **Effort:** S · **Confidence:** high

- **Evidence:** `framer-motion` used in 10 component files (e.g. `SettingsPage.tsx:249-253`, `AboutSection.tsx:815-820,1042-1047`, `UsageHero.tsx:349-354`) with zero `useReducedMotion`/`prefers-reduced-motion` references in `src/`; skeletons `animate-pulse rounded bg-gray-100` without `dark:` in `src/components/usage/ModelStatsTable.tsx:39`, `ProviderStatsTable.tsx:39`, `RequestLogTable.tsx:150`, `RequestDetailPanel.tsx:35`; status badges `bg-green-100 text-green-800` / `bg-red-50 text-red-700` without dark variants in `RequestDetailPanel.tsx:143-147,313-317`; toasts default to 2 s (`src/components/ui/sonner.tsx:17`) even for error toasts with descriptions.
- **Problem:** Motion-sensitive users get staggered card animations on every tab switch; dark-mode users see bright white skeleton blocks and unreadable light badges; error toasts disappear before they can be read.
- **Proposal:** Wrap `motion` usage in a `MotionConfig reducedMotion="user"` at the root; replace hard-coded grays with `bg-muted`; add a `Skeleton` primitive; set `toast.error` default duration ≥ 6 s via a thin `notify` wrapper.
- **Acceptance:** With `prefers-reduced-motion: reduce` no transform animations run; visual check of dark mode skeleton/badges; error toasts persist ≥ 6 s.

### FE-UI-14: Dark-mode `MutationObserver` boilerplate duplicated in six in-scope files although `useDarkMode` exists

**Type:** tech-debt · **Priority:** P3 · **Effort:** S · **Confidence:** high

- **Evidence:** Identical 12-line observer blocks in `src/components/mcp/McpFormModal.tsx:123-138`, `src/components/openclaw/EnvPanel.tsx:26-39`, `src/components/workspace/DailyMemoryPanel.tsx:72-84`, `src/components/workspace/WorkspaceFileEditor.tsx:26-36`, `src/components/prompts/PromptFormPanel.tsx:44-57`, `src/components/prompts/PromptFormModal.tsx:49-64`; the shared hook is `src/hooks/useDarkMode.ts:1-30` (used by `HermesMemoryPanel`).
- **Problem:** Six observers for one DOM attribute; divergence risk; the real fix is for `JsonEditor`/`MarkdownEditor` to read the theme themselves.
- **Proposal:** Have `JsonEditor` and `MarkdownEditor` call `useDarkMode()` internally (drop the `darkMode` prop) or at minimum replace each block with the hook.
- **Acceptance:** `rg 'classList.contains\("dark"\)' src/components` returns only the editors (or nothing).

### FE-UI-21: Segmented-control pattern copy-pasted five times in Settings

**Type:** refactor · **Priority:** P3 · **Effort:** S · **Confidence:** high

- **Evidence:** `LanguageButton` (`src/components/settings/LanguageSettings.tsx:50-67`), `ThemeButton` (`ThemeSettings.tsx:53-76`), `SyncMethodButton` (`SkillSyncMethodSettings.tsx:57-78`), `StorageButton` (`SkillStorageLocationSettings.tsx:142-165`), `AppButton` (`AppVisibilitySettings.tsx:118-144`) — identical `inline-flex gap-1 rounded-md border … p-1` wrapper and `variant={active ? "default" : "ghost"}` button.
- **Problem:** Five copies of the same component with no shared accessibility semantics (see FE-UI-11).
- **Proposal:** `src/components/ui/segmented-control.tsx` (`options`, `value`, `onChange`, `multiple?`) rendering `role="radiogroup"`/`role="radio" aria-checked` (or `aria-pressed` for the multi-select visibility case) with arrow-key navigation.
- **Acceptance:** All five settings use `SegmentedControl`; axe passes; keyboard arrows move selection.

### FE-UI-22: Proxy status polling is duplicated across two hook modules with different cadences

**Type:** tech-debt · **Priority:** P3 · **Effort:** S · **Confidence:** high

- **Evidence:** `src/hooks/useProxyStatus.ts:181-195` polls `["proxyStatus"]` every 2 s only while running and `["proxyTakeoverStatus"]` once; `src/lib/query/proxy.ts:12-51` defines another `useProxyStatus` (5 s always), `useIsProxyRunning` (2 s), `useIsLiveTakeoverActive` (2 s) and `useProxyTakeoverStatus` (2 s); `ProxyPanel.tsx:18-28,48-52` mixes both modules; `App.tsx:270`, `ProxyToggle`, `FailoverToggle`, `ClaudeDesktopRouteToggle`, `ProxyTabContent` all mount the hook, and `ProviderQueueItem` adds one `providerHealth` query per queue entry at 5 s (`src/lib/query/failover.ts:255-263`).
- **Problem:** While Settings → Proxy is open, the backend is hit by status/takeover/health polls from several observers with different intervals; `hooks/useProxyConfig.ts` duplicates `lib/query/proxy.ts:137-166` entirely.
- **Proposal:** Keep one module (`lib/query/proxy.ts`), export `proxyKeys`, give takeover status a conditional interval like status, and replace per-provider health queries with one `useProviderHealthMap(appType)` query (needs a batch command); push status via Tauri events instead of polling where possible (`useTauriEvent` already exists).
- **Acceptance:** Only one `useProxyStatus` export exists; network tab shows ≤ 1 status call per 2 s with the Proxy tab open.

### FE-UI-23: ProxyPanel UX gaps — hidden config while running, misleading stat colouring, unlocalised uptime, unawaited clipboard

**Type:** ux · **Priority:** P3 · **Effort:** S · **Confidence:** high

- **Evidence:** `src/components/proxy/ProxyPanel.tsx:315,517-603` — listen address/port form is only rendered when the proxy is stopped, while the "must stop to edit" hint (`:347-351`) is shown only when running; `:503-505` — success-rate card is "warning" whenever `success_rate <= 90`, including 0 requests; `:201-213` — `formatUptime` returns `"1h 2m 3s"` untranslated; `:332-342` — `navigator.clipboard.writeText` not awaited/caught before the success toast; `:77-103` toasts duplicate the one already emitted by `useProxyStatus.setTakeoverForApp` when that path is used elsewhere.
- **Problem:** Users looking for the port while running find nothing; a fresh proxy shows an amber "warning" card; copy can silently fail yet report success.
- **Proposal:** Always render the basic-config form (disabled while running with the hint next to it); use `Intl.NumberFormat`/`t` for uptime; treat `total_requests === 0` as neutral; await clipboard with error toast.
- **Acceptance:** Running proxy shows disabled address/port fields with hint; 0-request state renders neutral; clipboard failure shows error.

### FE-UI-27: Usage range/timezone edge cases — local-day presets vs string bucket dates, unenforced 30-day hint, Sunday-first calendar

**Type:** bug · **Priority:** P3 · **Effort:** S · **Confidence:** medium

- **Evidence:** `src/lib/usageRange.ts:11-24` computes `today`/`7d` from the browser's local day while `UsageTrendChart.tsx:62-86` builds labels from `new Date(stat.date)` (a backend string whose zone is unknown) and decides hourly buckets purely from client duration (`:58-59`); `UsageDateRangePicker.tsx:360` says "最长 30 天" but `handleApply` (`:228-241`) never enforces it; `getCalendarDays` (`:94-103`) always starts the week on Sunday regardless of locale; the live end-time ticker runs every second while the popover is open (`:158-164`).
- **Problem:** Around midnight/DST or for non-UTC users, day labels can be off by one versus the preset boundaries; very long custom ranges silently produce huge queries; week layout is wrong for zh/ja locales (Monday-first).
- **Proposal:** Have the backend return bucket start as epoch seconds plus bucket size; use `Intl.Locale.weekInfo`/a locale table for first-day-of-week; enforce or document the max range with a validation message; tick the live end time only every 30 s.
- **Acceptance:** Unit tests for `resolveUsageRange` around DST; calendar renders Monday-first for `zh-CN`; ranges > limit show an inline error.

### FE-UI-28: `UsageTrendChart` performance/typing — inline tooltip component, un-memoised data, `any` types, naive tick formatting

**Type:** performance · **Priority:** P3 · **Effort:** S · **Confidence:** high

- **Evidence:** `src/components/usage/UsageTrendChart.tsx:90-117` defines `CustomTooltip` inside the render (new component identity every refetch → Recharts remounts it), typed `any`; `:62-86` recomputes `chartData` on every render without `useMemo`; `:176` `tickFormatter` renders `0k` for values < 1000 and `1500k` for 1.5M; `:188-237` five `Area` series with hard-coded hex colours; the `cost` axis formatter `$${value}` (`:184`) can print 7-decimal floats.
- **Problem:** Unnecessary re-renders every 5-30 s refresh interval; axis labels unreadable at small/large scales.
- **Proposal:** Hoist `CustomTooltip` (typed with `TooltipProps`), `useMemo` the data, reuse `formatTokensShort`/`fmtUsd` for ticks, move colours to CSS variables shared with `UsageHero` accents.
- **Acceptance:** React Profiler shows no tooltip remount on refetch; ticks read `0`, `500`, `1.5M`.

### FE-UI-31: Usage dashboard filters/tab are lost on every Settings tab switch

**Type:** ux · **Priority:** P3 · **Effort:** S · **Confidence:** high

- **Evidence:** `src/components/usage/UsageDashboard.tsx:88-96` keeps range/app/provider/model in component state; the dashboard is a Radix `TabsContent` of `SettingsPage.tsx:512-519`, which unmounts inactive tabs (the About tab even added a module-level cache for this reason, `AboutSection.tsx:188-197`); the inner `Tabs defaultValue="logs"` (`:352`) also resets.
- **Problem:** Checking a provider setting and returning to Usage resets a carefully chosen custom range and filters.
- **Proposal:** Persist dashboard view state in a small zustand/context store or `localStorage` (`usage.dashboardState`), restore on mount; optionally lift Usage out of Settings into its own top-level view.
- **Acceptance:** Switching Settings tabs and back preserves range, filters and the active sub-tab.

### FE-UI-35: Workspace paths are hard-coded and file existence is checked by reading full contents

**Type:** bug · **Priority:** P3 · **Effort:** S · **Confidence:** high

- **Evidence:** `src/components/workspace/WorkspaceFilesPanel.tsx:92` renders literal `~/.openclaw/workspace/` and `DailyMemoryPanel.tsx:360` `~/.openclaw/workspace/memory/` although Settings supports an `openclawConfigDir` override (`src/types.ts:403`); `WorkspaceFilesPanel.tsx:60-73` `checkFileExistence` calls `workspaceApi.readFile` for all nine files just to test `!== null`.
- **Problem:** Displayed path is wrong for users with an override; nine full-file reads on every panel open/close.
- **Proposal:** Return the resolved workspace path and an `exists`/`size` map from one `list_workspace_files` command; render it instead of the literal.
- **Acceptance:** With `openclawConfigDir` overridden the panel shows the real path; one IPC call on mount.

### FE-UI-36: `AgentsDefaultsPanel` can persist an empty primary model and numeric fields lack validation

**Type:** bug · **Priority:** P3 · **Effort:** S · **Confidence:** medium

- **Evidence:** `src/components/openclaw/AgentsDefaultsPanel.tsx:143-145` writes `model = { primary: "", fallbacks }` when the primary is unset but fallbacks exist; `:152-170` `parseNum` accepts negatives/floats for `contextTokens`/`maxConcurrent` and silently drops unparsable values; no dirty indicator or Save disable.
- **Problem:** An empty `primary` is likely rejected or misread by OpenClaw; typos become silent deletions of a field.
- **Proposal:** Block save (inline error) when fallbacks exist without a primary; zod schema with integer/min bounds; dirty tracking.
- **Acceptance:** Saving with empty primary + fallbacks shows an error and does not call `set_agents_defaults`.

### FE-UI-37: `BackupListSection` duplicates confirm dialogs, uses a misleading icon and offers no reveal/export

**Type:** ux · **Priority:** P3 · **Effort:** S · **Confidence:** high

- **Evidence:** `src/components/settings/BackupListSection.tsx:410-491` two bespoke `Dialog`s instead of `ConfirmDialog`; `:284` "Backup Now" uses the `Download` icon; `:41-48` `toLocaleString()` without locale; no "reveal in file manager"/"export backup" action although `settingsApi.openExternal`/save-dialog helpers exist; list capped at `max-h-48` with no count.
- **Problem:** Inconsistent dialogs; users cannot find backup files on disk from the UI.
- **Proposal:** Use `ConfirmDialog` (destructive/info variants); `Archive`/`Save` icon; add "Show in folder" and "Export…" per backup; localised dates.
- **Acceptance:** Both confirmations go through `ConfirmDialog`; "Show in folder" opens the backup directory.

### FE-UI-38: Settings search and a keyboard-shortcuts reference are missing

**Type:** feature · **Priority:** P3 · **Effort:** M · **Confidence:** high

- **Evidence:** Settings is 6 tabs × nested accordions (`SettingsPage.tsx:227-240,323-503`, `ProxyTabContent.tsx:367-537`, `UsageDashboard.tsx:410-432`) with no search; `src/App.tsx:601-623` registers global key handling and `DailyMemoryPanel.tsx:149-169`, `McpWizardModal.tsx:167-172`, `UsageDateRangePicker` add ad-hoc shortcuts, but there is no place that lists them and no `?`/`Cmd+/` overlay; `cmdk` is already a dependency (`src/components/ui/command.tsx`).
- **Problem:** Finding "skip Claude onboarding" or "rectifier media heuristic" requires knowing which tab/accordion hides it.
- **Proposal:** A settings registry (`{ id, tab, section, titleKey, keywords }`) feeding a `Command` palette that jumps to and highlights the setting (expanding the accordion); a "Keyboard shortcuts" dialog generated from a central shortcut map used by all `keydown` handlers.
- **Acceptance:** Typing "onboarding" in the palette navigates to General → Window Behaviour and focuses the toggle; `?` opens the shortcut list.

### FE-UI-39: Proxy request inspector/replay is absent despite logging being a first-class toggle

**Type:** feature · **Priority:** P3 · **Effort:** L · **Confidence:** medium

- **Evidence:** `ProxyPanel.tsx:395-416` exposes "enable logging" but nothing in the UI reads those logs; `ProxyStatus.last_error`/`last_request_at` (`src/types/proxy.ts:26-27`) are never rendered; usage rows only carry `errorMessage` (`src/types/usage.ts:115`) — no request/response headers or bodies.
- **Problem:** When a provider fails (401/429/model mapping) users have no in-app way to see what was sent/received or to retry a request against another provider.
- **Proposal:** A "Requests" tab in the proxy panel backed by a ring buffer of recent request metadata (method, path, model, status, latency, redacted headers, truncated body) with a detail drawer and "Replay to provider X"; reuse `RequestDetailPanel` layout.
- **Acceptance:** Failed request visible within 1 s with status and error body; replay issues a new request and records a new log row.

### FE-UI-40: `ToggleRow` label text is not associated with (or clickable for) its switch

**Type:** ux · **Priority:** P3 · **Effort:** S · **Confidence:** high

- **Evidence:** `src/components/ui/toggle-row.tsx:20-40` — title/description are `<p>` elements; the `Switch` gets `aria-label={title}` but no `id`/`htmlFor`; clicking the row text does nothing.
- **Problem:** Small 44×24 hit target for every setting; description is not announced as the control's description.
- **Proposal:** Wrap the row in `<label>` (or add `id` + `aria-describedby`) so the whole row toggles; add `disabled` styling to the text.
- **Acceptance:** Clicking the title toggles the switch; axe shows the description linked via `aria-describedby`.

### FE-UI-41: Settings tab bar enforces a 720 px minimum and overflows at narrow widths

**Type:** ux · **Priority:** P3 · **Effort:** S · **Confidence:** medium

- **Evidence:** `src/components/ui/tabs.tsx:29` `TabsTrigger` has `min-w-[120px]`; `SettingsPage.tsx:227` renders six triggers in `grid-cols-6`.
- **Problem:** Below ~800 px window width the grid clips labels (Japanese labels are long) with no wrap/scroll.
- **Proposal:** Remove the min-width in the settings context or use an auto-fit/scrollable `TabsList` with icons at small widths.
- **Acceptance:** At 700 px width all six tabs are reachable without clipping.

### FE-UI-43: `GlobalProxySettings` round-trips URLs with an added trailing slash and has unlabelled credential inputs

**Type:** bug · **Priority:** P3 · **Effort:** S · **Confidence:** medium

- **Evidence:** `src/components/settings/GlobalProxySettings.tsx:21-39` `extractAuth` returns `parsed.toString()` (`http://127.0.0.1:7890` → `http://127.0.0.1:7890/`), so after load the field differs from what the user typed and `dirty` logic compares strings; `:159-167,218-239` URL/username/password inputs have placeholders only (no `<label>`/`aria-label`); `:240-253` the show-password button has `tabIndex={-1}` and no `aria-label`; `:103-106` `handleSave` is not wrapped in try/catch (`mutateAsync` rejection → unhandled promise, though the mutation itself toasts).
- **Problem:** Cosmetic URL drift, screen-reader users cannot identify the fields, and an unhandled rejection is logged on save failure.
- **Proposal:** Preserve the original string when no auth is embedded; add visually-hidden labels; make the eye button focusable with `aria-label`/`aria-pressed`; catch in `handleSave`.
- **Acceptance:** Saved `http://host:7890` reloads identically; axe passes; no unhandled rejection in the failing-save test.

### FE-UI-44: `ModelsDevPickerDialog` rows are `role="button"` divs without keyboard support, and the fetch bypasses the app's proxy settings

**Type:** ux · **Priority:** P3 · **Effort:** S · **Confidence:** medium

- **Evidence:** `src/components/usage/ModelsDevPickerDialog.tsx:349-360` — `<div role="button" aria-pressed onClick>` with no `tabIndex`/`onKeyDown`; `:161-173` — uses browser `fetch(MODELS_DEV_API_URL)` directly, so the global outbound proxy configured in Settings (`GlobalProxySettings`) and any CSP/offline handling do not apply; single-select only (`:211-215`) with a comment explaining the backend cost of batch imports.
- **Problem:** Keyboard users cannot pick a model; users behind a proxy get a bare `HTTP`/network error.
- **Proposal:** Render rows as `<button>`; fetch via a Tauri command (`fetch_models_dev_pricing`) that honours the global proxy and caches on disk; allow multi-select with a single batch command.
- **Acceptance:** Arrow/Enter selects a row; picker works with only the configured outbound proxy reachable.

### FE-UI-45: Install-command snippets execute remote scripts (incl. `-ExecutionPolicy Bypass`) without surfacing the source URL or a warning

**Type:** docs · **Priority:** P3 · **Effort:** S · **Confidence:** medium

- **Evidence:** `src/components/settings/AboutSection.tsx:109-160` builds `bash -c 'curl -fsSL <url> … && bash $tmp'` and `powershell -NoProfile -ExecutionPolicy Bypass -EncodedCommand …` (the Hermes installer URL is only visible after base64-decoding); the panel (`:1238-1260`) offers Copy with the hint `settings.oneClickInstallHint`; the in-app Install/Update buttons run the same commands via `runToolLifecycleAction` (`:593-597`) with no per-tool confirmation of what will execute (only the multi-install conflict dialog, `ToolUpgradeConfirmDialog`).
- **Problem:** Users are asked to trust opaque remote-script execution; security-conscious users cannot review the exact command before running it.
- **Proposal:** Show the resolved command (decoded) in a "What will run" expander before executing, link to each tool's official install docs, and document the behaviour in the user manual.
- **Acceptance:** Install/Update buttons show the command and URL before execution; docs page added.

### FE-UI-46: Prompt management lacks versioning/history and diff against the live file

**Type:** feature · **Priority:** P3 · **Effort:** M · **Confidence:** medium

- **Evidence:** `src/hooks/usePromptActions.ts:20-26` loads `getCurrentFileContent` but `PromptPanel.tsx` never renders it or a diff; `PromptFormPanel.tsx:67-92` overwrites content with `updatedAt` only; no history list, no "restore previous", no indication when the live `CLAUDE.md`/`AGENTS.md` was edited outside the app.
- **Problem:** Enabling a prompt silently replaces a file the user may have hand-edited; there is no way to see what changed or go back.
- **Proposal:** Keep N previous versions per prompt (backend), show a "Live file differs from enabled prompt" banner with a diff view (CodeMirror merge view) and "Import live changes" / "Restore version" actions.
- **Acceptance:** Editing `CLAUDE.md` externally shows the banner; history dialog lists versions with restore.

### FE-UI-47: `DialogContent`/`DialogOverlay` duplicate the z-index map and most feature dialogs pick ad-hoc `zIndex` values

**Type:** tech-debt · **Priority:** P3 · **Effort:** S · **Confidence:** high

- **Evidence:** `src/components/ui/dialog.tsx:60-65` and `:100-105` define the same `zIndexMap`; callers choose `"alert"`/`"top"` inconsistently (`BackupListSection.tsx:414,454`, `UnifiedSkillsPanel.tsx:451,639`, `EnvWarningBanner.tsx:245`, `ModelsDevPickerDialog.tsx:264`, `PricingConfigPanel.tsx:457` none), while `FullScreenPanel` is `z-[60]` (`FullScreenPanel.tsx:94`) and `Select`/`Popover`/`Tooltip` contents are `z-[100]`.
- **Problem:** Dialogs opened from a FullScreenPanel must remember to pass `zIndex="alert"` or render underneath it; the overlay/content maps can drift.
- **Proposal:** Single `Z_LAYERS` constant; a `useDialogLayer()` context so nested dialogs auto-increment; lint rule or wrapper (`<Modal>`) that derives the layer from the stack.
- **Acceptance:** No `zIndex=` props in feature code; nested dialog inside a FullScreenPanel renders above it by default.
