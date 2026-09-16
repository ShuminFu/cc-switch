# CC Switch — Settings area behavioural spec for the Dioxus/Rust port

All paths relative to `/home/user/cc-switch`. Every i18n key quoted below was machine-verified to exist in `src/i18n/locales/en.json` (501 keys extracted from the settings tree + related hooks; **zero** missing — see "i18n gaps" at the end for the only two genuine misses, which are outside `settings/*`).

---

## 0. Shell & mounting

- `SettingsPage` is **not** a modal. `src/App.tsx:899-904` renders it as a full view (`open={true}`, `onOpenChange={() => setCurrentView("providers")}`, `onImportSuccess=handleImportSuccess`, `defaultTab={settingsDefaultTab}`). Props interface: `src/components/settings/SettingsPage.tsx:61-66` (`open`, `onOpenChange`, `onImportSuccess?`, `defaultTab = "general"`).
- Root layout `SettingsPage.tsx:216`: `flex flex-col h-full overflow-hidden px-6`.
- While `isLoading && !settings` (`:213`) render a centered spinner (`:217-220`). Otherwise a `Tabs` with a 6-column `TabsList` (`:227-240`):
  1. `general` — `settings.tabGeneral` ("General")
  2. `proxy` — `settings.tabProxy` ("Routing")
  3. `auth` — `settings.tabAuth` ("Auth")
  4. `advanced` — `settings.tabAdvanced` ("Advanced")
  5. `usage` — `usage.title` ("Usage Statistics")
  6. `about` — `common.about` ("About")
- Effects: on `open` → `setActiveTab(defaultTab)` + `resetStatus()` (`:114-119`); on `requiresRestart` → open restart dialog (`:121-125`); on tab change → scroll the tab container to top via `useLayoutEffect` (`:127-131`).
- **Save bar is only shown on the `advanced` tab** (`:522-543`): a single `Button` → `handleSave`, disabled while `isSaving`, label `settings.saving` + spinner while pending else `common.save` with a Save icon. All other tabs autosave.

---

## 1. Save flow (`src/hooks/useSettings.ts`, `useSettingsForm.ts`, `useSettingsMetadata.ts`, `useDirectorySettings.ts`)

### 1.1 Form state (`useSettingsForm.ts`)
- `SettingsFormState = Omit<Settings,"language"> & { language: "zh"|"zh-TW"|"en"|"ja" }` (`:8-10`).
- Loads from `useSettingsQuery()` (react-query key `["settings"]`, `queryFn: settingsApi.get()` — `src/lib/query/queries.ts:92-97`).
- On data arrival (`:103-134`) it normalizes and seeds defaults — **these are the port's defaults**:
  `showInTray ?? true`, `minimizeToTrayOnClose ?? true`, `useAppWindowControls ?? false`, `enableClaudePluginIntegration ?? false`, `silentStartup ?? false`, `skipClaudeOnboarding ?? false`, `preserveCodexOfficialAuthOnSwitch ?? false`, `unifyCodexSessionHistory ?? false`; all `*ConfigDir` run through `sanitizeDir` (trim; empty → `undefined`, `:48-52`); `language` normalized.
- `normalizeLanguage` (`:12-38`): lowercase, `_`→`-`; `zh`→`zh`; `zh-tw`/`zh-hant*`/`zh-hk*`/`zh-mo*`→`zh-TW`; `en`/`ja` pass through; any other `zh*`→`zh`; **fallback `zh`**.
- `readPersistedLanguage` (`:82-90`) reads `localStorage["language"]` if it is a supported language, else `i18n.language`.
- `updateSettings(updates)` merges into local state; if `updates.language` present it normalizes and calls `i18n.changeLanguage` immediately (`:157-161`) — **language switches the UI before the save round-trips**.
- There is **no explicit dirty flag**: the whole form state is always sent as a full `Settings` payload. Backend diffs to decide side effects (comment at `SettingsPage.tsx:186-188`).

### 1.2 `autoSaveSettings(updates)` — used by General/Proxy/Usage/Backup/cloud-sync-confirm (`useSettings.ts:182-308`)
1. `merged = {...settings, ...updates}`; bail with `null` if no settings.
2. Sanitize `claudeConfigDir`, `codexConfigDir`, `geminiConfigDir`, `grokConfigDir`, `opencodeConfigDir`, `openclawConfigDir` (`:188-197`).
3. **Strip `webdavSync` and `s3Sync` from the payload** (`:198-202`) — cloud sync creds are saved by their own commands only.
4. Capture `prevPluginEnabled` from the **live query cache** `queryClient.getQueryData<Settings>(["settings"])?.enableClaudePluginIntegration` (`:217-219`) — deliberately not the closure value (race guard).
5. `await saveMutation.mutateAsync(payload)` → `save_settings` → invalidates `["settings"]` (`src/lib/query/mutations.ts:383-394`).
6. **Side effect: launch-on-startup** (`:225-239`) — only if `payload.launchOnStartup !== undefined && !== data?.launchOnStartup`: `settingsApi.setAutoLaunch(bool)` (`set_auto_launch`). On failure → `toast.error(t("settings.autoLaunchFailed"))`.
7. **Side effect: Claude onboarding skip** (`:243-269`) — only when `updates.skipClaudeOnboarding` is present and differs: on → `apply_claude_onboarding_skip`, off → `clear_claude_onboarding_skip`. Failure toast keys `notifications.skipClaudeOnboardingFailed` / `notifications.clearClaudeOnboardingSkipFailed`.
8. **Side effect: Claude plugin integration** via `syncClaudePluginIfChanged` (`:132-178`): if changed → when enabling, read `providersApi.getCurrent("claude")` + `providersApi.getAll("claude")` to compute `isOfficial = category === "official"`, then `apply_claude_plugin_config({official})`; when disabling → `apply_claude_plugin_config({official:true})`. Then `syncCurrentProvidersLiveSafe()` (`src/utils/postChangeSync.ts` → `sync_current_providers_live`). Failure → `toast.error(t("notifications.syncClaudePluginFailed"))`. Returns `true` if the live sync ran (used to de-dupe later).
9. Persist `localStorage["language"] = updates.language` when present (`:277-286`).
10. **Tray refresh**: `providersApi.updateTrayMenu()`, failures only `console.warn` (`:289-293`).
11. Returns `{ requiresRestart: false }`. On any throw → `toast.error(t("notifications.settingsSaveFailed", {error}))` and **re-throws**.

`SettingsPage.handleAutoSave` (`:182-211`) wraps it: snapshots the previous values of the changed keys, optimistically `updateSettings(updates)`, calls `autoSaveSettings(updates)`; on failure **rolls the form back** and toasts `settings.saveFailedGeneric`. Returns `boolean` — callers that chain follow-up work (Codex history restore, usage refresh interval) short-circuit on `false`.

### 1.3 `saveSettings(overrides?, {silent})` — the Advanced Save button (`useSettings.ts:312-492`)
Same as autosave plus:
- `settingsApi.setAppConfigDirOverride(sanitizedAppDir ?? null)` → `set_app_config_dir_override` (`:363`).
- Compares previous vs new per-app dirs; if any of claude/codex/gemini/grok/opencode/openclaw changed **and** the plugin sync didn't already run, calls `syncCurrentProvidersLiveSafe()` (`:433-455`).
- `requiresRestart = sanitizedAppDir !== (initialAppConfigDir ?? undefined)`; `setRequiresRestart(appDirChanged)` (`:457-458`).
- Unless `silent`, `toast.success(t("notifications.settingsSaved"), {closeButton:true})` (`:460-467`).
- Returns `{requiresRestart}`; throws on error after `toast.error(t("notifications.settingsSaveFailed",{error}))`.

`SettingsPage.handleSave` (`:141-153`): `saveSettings(undefined,{silent:false})`; if `result.requiresRestart` → show restart dialog and **do not close**; else `closeAfterSave()` = `acknowledgeRestart()` + `clearSelection()` + `resetStatus()` + `onOpenChange(false)` (`:133-139`).

### 1.4 Restart dialog (`SettingsPage.tsx:548-577`)
- Title `settings.restartRequired`, body `settings.restartRequiredMessage`, buttons `settings.restartLater` (ghost) / `settings.restartNow`.
- Later → close dialog + `closeAfterSave()` (`:155-158`).
- Now (`:160-176`): in dev builds toast `settings.devModeRestartHint` and close; otherwise `settingsApi.restart()` (`restart_app`), on failure `toast.error(t("settings.restartFailed"))`, `finally` close.

### 1.5 Metadata (`useSettingsMetadata.ts`)
Mount-only: `settingsApi.isPortable()` → `is_portable_mode`; exposes `isPortable`, `requiresRestart`, `acknowledgeRestart`, `setRequiresRestart`.

### 1.6 Directories (`useDirectorySettings.ts`)
- Metadata table `:31-42`: claude `.claude`, codex `.codex`, gemini `.gemini`, grokbuild `.grok`, opencode `.config/opencode`, openclaw `.openclaw`, hermes `.hermes`. Field map `:44-55` (`grokbuild → grokConfigDir`).
- Mount load (`:157-243`) runs 16 calls in parallel: `get_app_config_dir_override`, `get_config_dir` ×7 (arg `{app}`), and 8 default-path computations that use **Tauri JS path APIs** `homeDir()` + `join()` (`:63-89`) — app default is `~/.cc-switch`.
- `resolvedDirs` = override/value or computed default. `updateDirectoryState` (`:245-265`) writes `appConfig` to local state, other keys into the settings form; same-value early-return.
- `browseDirectory/browseAppConfigDir` → `settingsApi.selectConfigDirectory(current)` → `pick_directory {defaultPath}`; empty pick is a no-op; error → `toast.error(t("settings.selectFileFailed"))` (`:281-323`).
- `resetDirectory/resetAppConfigDir` recompute the default if missing, then set the value to `undefined` (`:325-353`).

---

## 2. General tab (`SettingsPage.tsx:247-293`) — everything autosaves on change

Order of sections:

### 2.1 `LanguageSettings` (`src/components/settings/LanguageSettings.tsx`)
- Header `settings.language` / `settings.languageHint` (`:18,20`).
- Segmented row of 4 buttons, min-width 96px, active = `default` variant (`:23-39`): `zh` `settings.languageOptionChinese`, `zh-TW` `settings.languageOptionTraditionalChinese`, `en` `settings.languageOptionEnglish`, `ja` `settings.languageOptionJapanese`.
- Binds `settings.language`; `onChange` → `handleAutoSave({language})` (`SettingsPage.tsx:257`).

### 2.2 `ThemeSettings` (`ThemeSettings.tsx`) — **not an AppSettings field**
- Header `settings.theme` / `settings.themeHint`. Three buttons with icons (`:20-40`): `light` `settings.themeLight` (Sun), `dark` `settings.themeDark` (Moon), `system` `settings.themeSystem` (Monitor).
- Bound to `useTheme()` from `src/components/theme-provider.tsx`; persisted to `localStorage["cc-switch-theme"]` (`theme-provider.tsx:30,37,52`). No backend call.

### 2.3 `AppVisibilitySettings` (`AppVisibilitySettings.tsx`)
- Header `settings.appVisibility.title` / `.description` (`:72,75`).
- 8 toggle buttons (`APP_CONFIG :16-33`) binding `settings.visibleApps` (`VisibleApps`), default all-true (`:41-50`): claude `apps.claudeCode`, claude-desktop `apps.claudeDesktop`, codex `apps.codex`, gemini `apps.gemini`, grokbuild `apps.grokbuild`, opencode `apps.opencode`, openclaw `apps.openclaw`, hermes `apps.hermes`.
- **Validation: the last visible app cannot be turned off** — button disabled and handler early-returns when `isVisible && visibleCount <= 1` (`:55-58`, `:82`).
- Then `ToggleRow` `settings.appVisibility.showProfileSwitcher` / `...Description` → `showProfileSwitcher`, default `true` (`:98-104`).

### 2.4 `SkillStorageLocationSettings` (`SkillStorageLocationSettings.tsx`) — **migration, not a plain save**
- Header `settings.skillStorage.title` / `.description`. Two buttons: `cc_switch` `settings.skillStorage.ccSwitch`, `unified` `settings.skillStorage.unified`. Hint below switches between `settings.skillStorage.unifiedHint` / `.ccSwitchHint` (`:99-103`).
- Value = `settings.skillStorageLocation ?? "cc_switch"` (`SettingsPage.tsx:265`); `installedCount` from `useInstalledSkills()` (query key `["skills","installed"]`, `staleTime: Infinity`, `src/hooks/useSkills.ts:24-31`).
- Selecting the same value is a no-op; if `installedCount > 0` open the confirm dialog, else migrate immediately (`:34-41`).
- Confirm dialog (`:106-130`): title `settings.skillStorage.confirmTitle`, body `settings.skillStorage.confirmMessage {count}`, buttons `common.cancel` / `common.confirm`.
- Migration calls `skillsApi.migrateStorage(target)` → command **`migrate_skill_storage {target}`** returning `MigrationResult {migratedCount, skippedCount, errors: string[]}` (`src/lib/api/skills.ts:103-107, 210-214`). If `errors.length>0` → `toast.warning(t("settings.skillStorage.migrationPartial",{migrated,errors}))`, else `toast.success(t("settings.skillStorage.migrationSuccess",{count}))`; throw → `toast.error(String(error))` (`:48-67`).
- On success calls `onMigrated` which **only updates local form state** (`SettingsPage.tsx:267-269`) — no `save_settings` here.

### 2.5 `SkillSyncMethodSettings` (`SkillSyncMethodSettings.tsx`)
- Header `settings.skillSync.title` / `.description`. Two buttons: `symlink` `settings.skillSync.symlink`, `copy` `settings.skillSync.copy`.
- Value = `settings.skillSyncMethod ?? "auto"`; **display rule: anything that isn't `"copy"` renders as `symlink`** (`:18`). Writing always sends `"symlink"` or `"copy"`.
- When symlink is shown, extra hint `settings.skillSync.symlinkHint` (`:42-46`). Autosaves `{skillSyncMethod}`.

### 2.6 `CodexAuthSettings` (`CodexAuthSettings.tsx`)
- Section header `settings.codexAuth` (KeyRound icon) (`:95`).
- ToggleRow 1: `settings.preserveCodexOfficialAuthOnSwitch` / `...Description` → `preserveCodexOfficialAuthOnSwitch`, default false — plain autosave (`:98-106`).
- ToggleRow 2: `settings.unifyCodexSessionHistory` / `...Description` → `unifyCodexSessionHistory`, default false — **guarded both directions** (`:108-114`, handler `:27-40`):
  - **Turning ON** → `ConfirmDialog` (`:116-124`) title `confirm.unifyCodexHistory.title`, message `confirm.unifyCodexHistory.message`, checkbox `confirm.unifyCodexHistory.migrateExisting`, confirm `confirm.unifyCodexHistory.confirm`. Confirm → `onChange({unifyCodexSessionHistory:true, unifyCodexMigrateExisting: checkboxValue})`.
  - **Turning OFF** → first probe `settingsApi.hasCodexUnifyHistoryBackup()` (`has_codex_unify_history_backup`, errors coerced to `false`), then `ConfirmDialog` (`:126-139`) `confirm.unifyCodexHistoryOff.title` / `.message` / confirm `.confirm`; checkbox `confirm.unifyCodexHistoryOff.restoreBackup` shown when `hasUnifyBackup || settings.unifyCodexMigrateExisting`, **default checked** (`:54`, `:135`).
  - Disable confirm (`:56-89`): save `{unifyCodexSessionHistory:false, unifyCodexMigrateExisting:false}`; **if the save returned `false`, abort without restoring**; if checkbox unchecked, stop; else `settingsApi.restoreCodexUnifiedHistory()` → `restore_codex_unified_history` returning `CodexUnifyHistoryRestoreResult {restoredJsonlFiles, restoredStateRows, skippedReason?}`. `skippedReason === "unify_toggle_on"` → `toast.info(t("settings.unifyCodexHistoryRestoreSkippedToggleOn"))`, other skip reasons → `settings.unifyCodexHistoryRestoreNothing`; success → `toast.success(t("settings.unifyCodexHistoryRestoreCompleted",{files,rows}))`; throw → `toast.error(t("settings.unifyCodexHistoryRestoreFailed"))`.
- `ConfirmDialog` contract (`src/components/ConfirmDialog.tsx:15-52`): `onConfirm(checkboxChecked: boolean)`, `variant: "destructive"|"info"` (default destructive), checkbox state resets to `checkboxDefaultChecked` each time it opens.

### 2.7 `WindowSettings` (`WindowSettings.tsx`)
Header `settings.windowBehavior` (`:20`). Rows in order:
1. `settings.launchOnStartup` / `...Description` → `launchOnStartup` (`:24-30`).
2. **Conditional** (only when `launchOnStartup` is true, animated): `settings.silentStartup` / `...Description` → `silentStartup` (`:32-50`).
3. `settings.enableClaudePluginIntegration` / `...Description` → `enableClaudePluginIntegration` (`:52-60`).
4. `settings.skipClaudeOnboarding` / `...Description` → `skipClaudeOnboarding` (`:62-68`).
5. `settings.minimizeToTray` / `...Description` → `minimizeToTrayOnClose` (`:70-78`).
6. **Linux only** (`isLinux()`): `settings.useAppWindowControls` / `...Description` → `useAppWindowControls` (`:80-90`).
All autosave; 1/3/4 trigger the OS-level side effects in §1.2. Note `showInTray` exists in `Settings` but **has no control in this tree**.

### 2.8 `TerminalSettings` (`TerminalSettings.tsx`) — platform-dependent
- Header `settings.terminal.title` / `.description`; a 200px-wide `Select`; footer hint `settings.terminal.fallbackHint`.
- Option lists (`:12-45`): macOS `terminal, iterm2, alacritty, kitty, ghostty, wezterm, kaku, warp` (keys `settings.terminal.options.macos.*`); Windows `cmd, powershell, wt` (`...options.windows.*`); Linux `gnome-terminal, konsole, xfce4-terminal, alacritty, kitty, ghostty` (keys `...options.linux.gnomeTerminal | konsole | xfce4Terminal | alacritty | kitty | ghostty`). Unknown platform falls back to the macOS list.
- Defaults: mac `terminal`, windows `cmd`, linux `gnome-terminal` (`:63-74`); displayed value is `value || default` (`:87`). Binds `preferredTerminal`, autosaves.
- Platform detection is **user-agent based** (`src/lib/platform.ts`), not Tauri OS API.

---

## 3. Proxy ("Routing") tab — `ProxyTabContent.tsx`

A multi-open `Accordion` with 4 items (all `defaultValue: []`, i.e. collapsed):

1. **`proxy` — Local Routing** (`:94-132`). Trigger: `settings.advanced.proxy.title` / `.description`, plus a Badge `settings.advanced.proxy.running` / `.stopped` driven by `useProxyStatus().isRunning`. Content: `<ProxyPanel>` (`src/components/proxy/ProxyPanel.tsx`, 755 lines, outside the 21 files) with props `enableLocalProxy = settings.enableLocalProxy ?? false`, `onEnableLocalProxyChange → onAutoSave({enableLocalProxy})`, `onToggleProxy`, `isProxyPending`. ProxyPanel's first control is a `ToggleRow` `settings.advanced.proxy.enableFeature` / `...Description` (`ProxyPanel.tsx:226-231`); the rest (listen address/port validation, per-app takeover, logging toggle) lives there.
2. **`failover` — Auto Failover** (`:135-215`). Trigger `settings.advanced.failover.title` / `.description`. Content: `ToggleRow` `settings.advanced.proxy.enableFailoverToggle` / `...Description` → `enableFailoverToggle` (default false); a yellow notice `proxy.failover.proxyRequired` when the proxy isn't running; then inner `Tabs` Claude/Codex/Gemini, each with `proxy.failoverQueue.title` / `.description` + `<FailoverQueueManager appType>` and `<AutoFailoverConfigPanel appType>`, both `disabled = !isRunning || !takeoverStatus?.[appType]` (`:181-182`).
3. **`rectifier`** (`:218-238`). Trigger `settings.advanced.rectifier.title` / `.description` → `<RectifierConfigPanel/>`.
4. **`globalProxy`** (`:241-261`). Trigger `settings.advanced.globalProxy.title` / `.description` → `<GlobalProxySettings/>`.

### Proxy-tab confirmations
- **Local proxy enable** (`:44-66`, `:264-272`): toggling off → `stopWithRestore()`; toggling on with `!settings.proxyConfirmed` → `ConfirmDialog` variant `info`, `confirm.proxy.title` / `.message` / `.confirm`; on confirm → `onAutoSave({proxyConfirmed:true})` then `startProxyServer()`. If already confirmed, start directly.
- **Failover toggle** (`:68-83`, `:274-282`): turning on with `!settings.failoverConfirmed` → `ConfirmDialog` info `confirm.failover.title` / `.message` / `.confirm`; confirm → `onAutoSave({failoverConfirmed:true, enableFailoverToggle:true})`. Otherwise autosave `{enableFailoverToggle}` directly.

### Backend (`src/hooks/useProxyStatus.ts`)
Queries `["proxyStatus"] → get_proxy_status`, `["proxyTakeoverStatus"] → get_proxy_takeover_status`. Mutations: `start_proxy_server` (→`ProxyServerInfo`), `stop_proxy_server`, `stop_proxy_with_restore`, `set_proxy_takeover_for_app {appType, enabled}`, `switch_proxy_provider {appType, providerId}`; plus `is_proxy_running`, `is_live_takeover_active`. Toasts: `proxy.server.started {address,port}`, `proxy.server.startFailed {detail}`, `proxy.server.stopped`, `proxy.server.stopFailed {detail}`, `proxy.stoppedWithRestore`, `proxy.stopWithRestoreFailed {detail}`.

### `RectifierConfigPanel.tsx` — **immediate save, optimistic with rollback**
- Loads `get_rectifier_config` and `get_optimizer_config` on mount (`:28-38`); renders `null` while loading (`:64`).
- Every `Switch` calls `handleChange`/`handleOptimizerChange` → optimistic `setConfig`, then `set_rectifier_config {config}` / `set_optimizer_config {config}`; on error `toast.error(String(e))` and **revert to the previous config** (`:40-62`).
- Rectifier controls: master `settings.advanced.rectifier.enabled` / `...enabledDescription` (default `true`, `:14-20`); group heading `settings.advanced.rectifier.requestGroup`; then `thinkingSignature`, `thinkingBudget`, `mediaFallback` (each `*Description`, all default true, each `disabled={!config.enabled}`), and `mediaHeuristic` at deeper indent with `disabled={!enabled || !requestMediaFallback}` (`:139`).
- Optimizer block (`:147-207`): heading `settings.advanced.optimizer.title` / `.description`; `enabled` (default **false**), then `thinkingOptimizer` and `cacheInjection` (default true), both `disabled={!optimizerConfig.enabled}`.
- Types: `RectifierConfig {enabled, requestThinkingSignature, requestThinkingBudget, requestMediaFallback, requestMediaHeuristic}` and `OptimizerConfig {enabled, thinkingOptimizer, cacheInjection}` (`src/lib/api/settings.ts:324-336`).

### `GlobalProxySettings.tsx` — **explicit Save button, local dirty flag**
- Hint paragraph `settings.globalProxy.hint`.
- Row 1: URL `Input` (placeholder literal `"http://127.0.0.1:7890 / socks5://127.0.0.1:1080"`), then icon buttons Scan (`title settings.globalProxy.scan`), Test (`settings.globalProxy.test`, disabled when `!fullUrl`), Clear (`settings.globalProxy.clear`, disabled when all three fields empty), and a `common.save` button disabled when `!dirty || isPending` (`:158-214`).
- Row 2: username `Input` (placeholder `settings.globalProxy.username`) and password `Input` with an eye/eye-off reveal toggle (placeholder `settings.globalProxy.password`) (`:217-255`).
- `Enter` in any of the three inputs saves when dirty (`:135-139`).
- Auth is encoded **into** the URL: `extractAuth` splits a stored URL into base+user+pass (`:21-39`), `mergeAuth` re-encodes using `URL` setters, with a manual `encodeURIComponent` fallback (`:42-70`). The saved value is always the merged `fullUrl`.
- Scan results render as clickable chips; clicking one fills the fields and marks dirty (`:258-272`).
- Loading state only on first load with no data (`:142-148`).
- Backend (`src/hooks/useGlobalProxy.ts`, `src/lib/api/globalProxy.ts`): query `["globalProxyUrl"] → get_global_proxy_url` (`string|null`); `useSetGlobalProxyUrl` → `set_global_proxy_url {url}` then `toast.success(t("settings.globalProxy.saved"))` and invalidates `["globalProxyUrl"]` + `["upstreamProxyStatus"]`; failure `toast.error(t("settings.globalProxy.saveFailed",{error}))`. Also `useTestProxy` (→`ProxyTestResult {success, latencyMs, error}`), `useScanProxies` (→`DetectedProxy[] {url, proxyType, port}`), `useUpstreamProxyStatus` (`UpstreamProxyStatus {enabled, proxyUrl}`).

---

## 4. Auth tab — `AuthCenterPanel.tsx` (74 lines)

Purely presentational, three cards (`:12-73`):
1. Intro card: ShieldCheck icon, `settings.authCenter.title` ("OAuth Authentication Center"), `settings.authCenter.description`, Badge `settings.authCenter.beta`.
2. "GitHub Copilot" card (literal heading), sub-text `settings.authCenter.copilotDescription`, body `<CopilotAuthSection/>`.
3. "ChatGPT (Codex OAuth)" card (literal heading), sub-text `settings.authCenter.codexOauthDescription`, body `<CodexOAuthSection/>`.

Both sections are thin wrappers over `useManagedAuth(provider)` (`src/components/providers/forms/hooks/useManagedAuth.ts:13-175`); `useCopilotAuth("github_copilot", githubDomain)` adds enterprise-domain handling and derives `username` from the default account; `useCodexOauth()` is `useManagedAuth("codex_oauth")`. Query key `["managed-auth-status", provider]`. Commands (`src/lib/api/auth.ts`): `auth_get_status`, `auth_start_login {provider, githubDomain}` (device-code flow → `ManagedAuthDeviceCodeResponse`), `auth_poll_for_account`, `auth_list_accounts`, `auth_remove_account`, `auth_set_default_account`, `auth_logout`. Legacy Copilot-specific commands still exist in `src/lib/api/copilot.ts` (`copilot_start_device_flow`, `copilot_poll_for_auth`, `copilot_get_auth_status`, `copilot_logout`, `copilot_is_authenticated`, `copilot_get_token`, `copilot_get_models`, `copilot_get_usage`, `copilot_list_accounts`, `copilot_poll_for_account`, `copilot_remove_account`, `copilot_set_default_account`, `copilot_get_token_for_account`, `copilot_get_models_for_account`, `copilot_get_usage_for_account`).

---

## 5. Advanced tab (`SettingsPage.tsx:315-506`) — Accordion, 6 items, all collapsed by default

| value | Icon/colour | Title key | Description key | Body |
|---|---|---|---|---|
| `directory` | FolderSearch/primary | `settings.advanced.configDir.title` | `.description` | `DirectorySettings` |
| `data` | Database/blue-500 | `settings.advanced.data.title` | `.description` | `ImportExportSection` |
| `backup` | HardDriveDownload/amber-500 | `settings.advanced.backup.title` | `.description` | `BackupListSection` |
| `cloudSync` | Cloud/blue-500 | `settings.advanced.cloudSync.title` | `.description` | `WebdavSyncSection` |
| `connectivityCheck` | FlaskConical/emerald-500 | `settings.advanced.connectivityCheck.title` | `.description` | `ConnectivityCheckConfigPanel` |
| `logConfig` | ScrollText/cyan-500 | `settings.advanced.logConfig.title` | `.description` | `LogConfigPanel` |

**Only the directory section is covered by the bottom Save button.** Everything else in this tab persists through its own command.

### 5.1 `DirectorySettings.tsx` — deferred save
- Block 1 (`:51-85`): heading `settings.appConfigDir` / `settings.appConfigDirDescription`; `Input` (value `appConfigDir ?? resolvedDirs.appConfig ?? ""`, placeholder `settings.browsePlaceholderApp`), icon Button browse (`title settings.browseDirectory`), icon Button reset (`title settings.resetDefault`).
- Block 2 (`:88-174`): heading `settings.configDirectoryOverride` / `settings.configDirectoryDescription`; seven identical `DirectoryInput` rows in order — Claude (`settings.claudeConfigDir`, placeholder `settings.browsePlaceholderClaude`), Codex (`settings.codexConfigDir` / `browsePlaceholderCodex`), Gemini (`settings.geminiConfigDir` / `browsePlaceholderGemini`), Grok (`settings.grokConfigDir` / `browsePlaceholderGrok`, app id **`grokbuild`**), OpenCode (`settings.opencodeConfigDir` / `browsePlaceholderOpencode`), OpenClaw (`settings.openclawConfigDir` / `browsePlaceholderOpenclaw`), Hermes (`settings.hermesConfigDir` / `browsePlaceholderHermes`).
- `DirectoryInput` (`:190-242`) shows `value ?? resolvedValue ?? ""`, i.e. the effective directory when no override is set; free-text edits go straight into form state (no validation), browse/reset as above.
- Persisted only by the Advanced **Save** button; changing the app config dir sets `requiresRestart` → restart dialog.

### 5.2 `ImportExportSection.tsx` + `useImportExport.ts` — immediate actions
- Header `settings.importExport` / `settings.importExportHint`.
- Two-column grid (`:58-114`):
  - **Import button**: label cycles `settings.selectConfigFile` → (file chosen) `settings.import` → (running) `settings.importing`; when a file is chosen the basename is shown inside the button and a small red ✕ (`aria-label common.clear`) clears the selection. Click with no file = pick a file, with a file = import.
  - **Export button**: `settings.exportConfig`.
- Status panel (`:132-211`): `importing` → `settings.importing` + `common.loading`; `success` → `settings.importSuccess`, optional `settings.backupId`: `{backupId}`, plus `settings.autoReload`; `partial-success` → `settings.importPartialSuccess` + `settings.importPartialHint`; `error` → `settings.importFailed` + message.
- Hook flow (`useImportExport.ts`): `selectImportFile` → `open_file_dialog` (error toast `settings.selectFileFailed`); `importConfig` (`:68-141`) → `import_config_from_file {filePath}` → `ConfigTransferResult {success, message, filePath?, backupId?}`; `!success` → status error + toast of `result.message || t("settings.configCorrupted")`; on success stores `backupId`, fires `onImportSuccess()` immediately, then `syncCurrentProvidersLiveSafe()` — ok → status `success` + `toast.success(t("settings.importSuccess"))`, not ok → status `partial-success` + `toast.warning(t("settings.importPartialSuccess"))`; throw → `toast.error(t("settings.importFailedError",{message}))`.
- `exportConfig` (`:143-183`): builds default name `cc-switch-export-YYYYMMDD_HHMMSS.sql` from local time; `save_file_dialog {defaultName}`; cancel → `toast.error(t("settings.selectFileFailed"))`; then `export_config_to_file {filePath}` — success → `toast.success(t("settings.configExported") + "\n" + path)`, failure → `settings.exportFailed` (+ `: message`), throw → `settings.exportFailedError {message}`.

### 5.3 `BackupListSection.tsx` + `useBackupManager.ts`
- **Policy row** (`:171-251`), both autosave via `onSettingsChange` → `handleAutoSave`:
  - `settings.backupManager.intervalLabel` → `backupIntervalHours`, default `24`; options `0` (`settings.backupManager.intervalDisabled`), `6/12/24/48` (`settings.backupManager.intervalHours {hours}`), `168` (`settings.backupManager.intervalDays {days:7}`).
  - `settings.backupManager.retainLabel` → `backupRetainCount`, default `10`; options `3,5,10,15,20,30,50` (raw numbers as labels).
- **List** (`:254-407`): heading `settings.backupManager.title`; "Backup Now" button `settings.backupManager.createBackup` / while running `settings.backupManager.creating`, disabled while creating or restoring. Empty → `settings.backupManager.empty`; loading → literal `"Loading..."` (**not translated**, `:296`).
- Each row: display name from `db_backup_YYYYMMDD_HHMMSS(_n).db` → `YYYY-MM-DD HH:MM:SS`, else filename minus `.db` (`:51-62`); subtitle `new Date(createdAt).toLocaleString()` · size (`<1KB` B, `<1MB` KB 1dp, else MB 1dp, `:35-39`). Actions: rename (pencil, `title settings.backupManager.rename`), delete (trash, `title settings.backupManager.delete`), restore (`settings.backupManager.restore` / `.restoring`).
- Inline rename editor: `Input` (placeholder `settings.backupManager.namePlaceholder`), Enter confirms, Escape cancels, confirm disabled when the trimmed value is empty (`:311-346`).
- Restore dialog (`:410-447`): `settings.backupManager.confirmTitle` / `.confirmMessage`, buttons `common.cancel` / `settings.backupManager.restore`.
- Delete dialog (`:450-491`): `settings.backupManager.deleteConfirmTitle` / `.deleteConfirmMessage`, `common.cancel` / destructive `settings.backupManager.delete` (`.deleting` while running).
- Toasts: restore → `settings.backupManager.restoreSuccess` with description `settings.backupManager.safetyBackupId: {id}`, duration 6000; failures use `extractErrorMessage(error) || t(...)` with `.restoreFailed` / `.deleteFailed` / `.renameFailed` / `.createFailed`; successes `.deleteSuccess` / `.renameSuccess` / `.createSuccess`.
- Backend (`src/lib/api/settings.ts:343-369`, `src/hooks/useBackupManager.ts`): query `["db-backups"] → list_db_backups` → `BackupEntry[] {filename, sizeBytes, createdAt}`; mutations `create_db_backup` (→ id string), `restore_db_backup {filename}` (→ safety-backup id; **onSuccess invalidates every query**), `rename_db_backup {oldFilename,newName}`, `delete_db_backup {filename}`.

### 5.4 `WebdavSyncSection.tsx` (1867 lines) — cloud sync, WebDAV **and** S3

Props (`:188-193`): `config = settings.webdavSync`, `s3Config = settings.s3Sync`, `settings`, `onAutoSave`.

**Shared shell** — header `settings.webdavSync.title` / `.description`; then a sync-type `Select` labelled `settings.syncType.label` with options `settings.syncType.webdav` / `settings.syncType.s3` (`:973-992`). Initial value = `s3Config?.enabled ? "s3" : "webdav"` (`:253-263`).

**Mutual exclusion** (`:865-924`): switching types while the other backend is enabled (`WebDAV enabled !== false && baseUrl non-empty`, or `s3.enabled === true`) opens a dialog `settings.s3Sync.mutualExclusionTitle` / `.mutualExclusionMessage`, buttons `common.cancel` / `common.confirm`. Confirm persists the *other* backend with `{enabled:false, autoSync:false}` via its save command, invalidates all queries, then switches; failure → `toast.error(t("settings.s3Sync.mutualExclusionFailed",{error}))`.

**WebDAV form** (`:995-1226`), label column fixed 160px:
1. Preset `Select` `settings.webdavSync.presets.label`; presets (`:54-82`) `jianguoyun` (`https://dav.jianguoyun.com/dav/`, match `jianguoyun.com`), `nextcloud` (`https://your-server/remote.php/dav/files/USERNAME/`, match `remote.php/dav`), `synology` (`http://your-nas-ip:5005/`, match `:5005`), `custom`. Labels/hints `settings.webdavSync.presets.{jianguoyun|nextcloud|synology|custom}` and `...Hint`. Choosing a preset overwrites `baseUrl` and marks dirty (`:403-415`); on URL blur, if the URL no longer matches the selected preset the selector flips to `custom` (`:418-424`).
2. `settings.webdavSync.baseUrl`, placeholder `.baseUrlPlaceholder`.
3. `settings.webdavSync.username`, placeholder `.usernamePlaceholder`.
4. `settings.webdavSync.password`, `type=password`, placeholder `.passwordPlaceholder`.
5. Active preset hint block (Info icon).
6. `settings.webdavSync.remoteRoot` with sub-label `.remoteRootDefault`, placeholder/default `"cc-switch-sync"`.
7. `settings.webdavSync.profile` with sub-label `.profileDefault`, placeholder/default `"default"`.
8. Auto-sync `Switch` `settings.webdavSync.autoSync` + `.autoSyncHint`. **Guarded**: enabling while `!settings.autoSyncConfirmed` opens `ConfirmDialog` info `confirm.autoSync.title` / `.message` / `.confirm` (`:426-453`, `:1856-1864`); confirm → `onAutoSave({autoSyncConfirmed:true})` then set `autoSync:true` + dirty.
9. Status line `settings.webdavSync.lastSync {time}` from `config.status.lastSyncAt * 1000` (**seconds → ms**, `:937-940`); and, when `status.lastErrorSource === "auto"`, a red block `settings.webdavSync.autoSyncLastErrorTitle` + raw error + `.autoSyncLastErrorHint` (`:1133-1143`).
10. Buttons row: Test (`settings.webdavSync.test` / `.testing`), Save (`.save` / `.saving`), plus a dirty chip `settings.webdavSync.unsaved` or a 2-second "saved" chip `settings.webdavSync.saved` (`:1146-1183`).
11. Sync row (top-bordered): Upload (`.upload` / `.uploading`, or `.fetchingRemote` during the pre-fetch) and Download (`.download` / `.downloading` / `.fetchingRemote`), both disabled unless `hasSavedConfig = config.baseUrl && config.username` (`:930-932`); when not saved, footnote `settings.webdavSync.saveBeforeSync`.
- All buttons in the WebDAV pane are disabled whenever `actionState !== "idle"` (`ActionButton :219`); `ActionState = "idle"|"testing"|"saving"|"uploading"|"downloading"|"fetching_remote"` (`:176-182`).

**WebDAV handlers**
- `buildSettings()` (`:455-468`): returns `null` if `baseUrl` is blank; trims url/username/remoteRoot/profile with the defaults above; **`password` is sent only if `passwordTouched`, else `""`** so the backend keeps the stored secret.
- Test (`:472-491`): missing url → `toast.error(t("settings.webdavSync.missingUrl"))`; `webdav_test_connection {settings, preserveEmptyPassword: !passwordTouched}` → success `settings.webdavSync.testSuccess`, failure `.testFailed {error}`.
- Save (`:493-543`): `webdav_sync_save_settings {settings, passwordTouched}`; on success clear dirty+touched, show "saved" for 2 s, `queryClient.invalidateQueries()` (all), then **auto-test** with `preserveEmptyPassword: true` → `.saveAndTestSuccess` or `toast.warning(.saveAndTestFailed {error})`. Failure → `.saveFailed {error}`. A `pendingPasswordPreservationRef` keyed on `{baseUrl,username,remoteRoot,profile}` (`:160-172`) keeps the typed password visible when the backend echoes back a redacted one (`:338-373`).
- Upload (`:546-590`): blocked while dirty (`toast.error(.unsavedChanges)`); pre-fetch `webdav_sync_fetch_remote_info` (`RemoteSnapshotInfo | {empty:true}`); fetch error → `.fetchRemoteFailed`. Then dialog; confirm → `webdav_sync_upload` → `.uploadSuccess` / `.uploadFailed {error}` + invalidate all.
- Download (`:593-650`): same dirty guard; empty remote → `toast.info(.noRemoteData)`; `!info.compatible` → `toast.error(.incompatibleVersion {protocolVersion, dbCompatVersion})` where `dbCompatVersion` renders as `db-v{n}` or `common.unknown` (`:156-158`); else dialog; confirm → `webdav_sync_download` → `.downloadSuccess` / `.downloadFailed {error}` + invalidate all.
- Displayed target path (`:944`): `/{remoteRoot||cc-switch-sync}/v2/db-v6/{profile||default}`.

**WebDAV upload dialog** (`:1543-1628`): AlertTriangle title `settings.webdavSync.confirmUpload.title`; body `.content`, bullets `.dbItem` / `.skillsItem`, `.targetPath`: `<code>`; if remote info exists a card `.existingData` with `.deviceName`, `.createdAt` (locale string), `.path`, optional `.dbCompat`; then either `.warning` (destructive) or, for `layout === "legacy"`, amber `.legacyNotice`. Footer `common.cancel` / destructive `.confirm`.
**WebDAV download dialog** (`:1631-1701`): `.confirmDownload.title`, dl-list `.deviceName` / `.createdAt` / `.path` / optional `.dbCompat` / `.artifacts` (comma joined); amber `.legacyNotice` for legacy layout; always destructive `.warning`; footer `common.cancel` / `.confirm`.

**S3 form** (`:1230-1540`), same layout idiom:
1. Preset `Select` `settings.s3Sync.presets.label`; presets (`:105-148`) `aws-s3` (region ph `us-east-1`), `s3-minio` (`us-east-1`), `s3-r2` (`auto`), `s3-oss` (`cn-hangzhou`), `s3-cos` (`ap-guangzhou`), `s3-obs` (`cn-north-4`), `s3-custom` (`us-east-1`); labels `settings.s3Sync.presets.{awsS3|minio|r2|oss|cos|custom}` + matching `*Hint`. **Preset is UI-only — it is not persisted.**
2. `settings.s3Sync.region` (placeholder from preset).
3. `settings.s3Sync.bucket` (placeholder `.bucketPlaceholder`).
4. `settings.s3Sync.accessKeyId` (`.accessKeyIdPlaceholder`).
5. `settings.s3Sync.secretAccessKey`, `type=password` (`.secretAccessKeyPlaceholder`) — typing sets `s3SecretTouched`.
6. `settings.s3Sync.endpoint` + sub-label `.endpointHint`, placeholder `.endpointPlaceholder`; blank → `undefined` in the payload.
7. `settings.s3Sync.remoteRoot` + `.remoteRootDefault` (default `cc-switch-sync`).
8. `settings.s3Sync.profile` + `.profileDefault` (default `default`).
9. `settings.s3Sync.autoSync` + `.autoSyncHint` — **note: no first-run confirm on the S3 side**.
10. `settings.s3Sync.enabled` + `.enabledHint`.
11. `settings.s3Sync.lastSync {time}`, auto-error block `.autoSyncLastErrorTitle` / `.autoSyncLastErrorHint`.
12. Test / Save (`.test`/`.testing`, `.save`/`.saving`) + `.unsaved` / `.saved` chips; Upload / Download (`.upload`/`.uploading`/`.fetchingRemote`, `.download`/`.downloading`), disabled unless `s3Config.bucket && s3Config.accessKeyId` (`:933-935`), else footnote `.saveBeforeSync`.
- Commands: `s3_test_connection {settings, preserveEmptyPassword: !s3SecretTouched}`, `s3_sync_save_settings {settings, passwordTouched: s3SecretTouched}`, `s3_sync_fetch_remote_info`, `s3_sync_upload`, `s3_sync_download`. Validation: empty bucket → `toast.error(t("settings.s3Sync.missingBucket"))`. Incompatible remote → `.incompatibleVersion {version}` (uses `version`, not the db-compat pair the WebDAV path uses).
- Dialogs `:1704-1820`: `settings.s3Sync.confirmUpload.{title,content,dbItem,skillsItem,targetPath,existingData,deviceName,createdAt,warning,confirm}` and `settings.s3Sync.confirmDownload.{title,deviceName,createdAt,artifacts,warning,confirm}`. S3 target path display: `{bucket||"bucket"}/{remoteRoot||cc-switch-sync}/v2/db-v6/{profile||default}` (`:945`).

**Wire types** (`src/types.ts`):
```ts
export interface WebDavSyncStatus {            // :290-297
  lastSyncAt?: number | null; lastError?: string | null; lastErrorSource?: string | null;
  lastRemoteEtag?: string | null; lastLocalManifestHash?: string | null; lastRemoteManifestHash?: string | null;
}
export interface WebDavSyncSettings {          // :300-309
  enabled?: boolean; autoSync?: boolean; baseUrl?: string; username?: string;
  password?: string; remoteRoot?: string; profile?: string; status?: WebDavSyncStatus;
}
export interface S3SyncSettings {              // :312-323
  enabled?: boolean; autoSync?: boolean; region?: string; bucket?: string;
  accessKeyId?: string; secretAccessKey?: string; endpoint?: string;
  remoteRoot?: string; profile?: string; status?: WebDavSyncStatus;
}
export type RemoteSnapshotLayout = "current" | "legacy";   // :325
export interface RemoteSnapshotInfo {          // :328-339
  deviceName: string; createdAt: string; snapshotId: string; version: number;
  protocolVersion: number; dbCompatVersion?: number | null; compatible: boolean;
  artifacts: string[]; layout: RemoteSnapshotLayout; remotePath: string;
}
```
```ts
// src/lib/api/settings.ts:10-31
export interface ConfigTransferResult { success: boolean; message: string; filePath?: string; backupId?: string; }
export interface WebDavTestResult { success: boolean; message?: string; }
export interface CodexUnifyHistoryRestoreResult { restoredJsonlFiles: number; restoredStateRows: number; skippedReason?: string; }
export interface WebDavSyncResult { status: string; }
```

### 5.5 `ConnectivityCheckConfigPanel` (`src/components/usage/ConnectivityCheckConfigPanel.tsx`) — explicit Save
- Loads `get_stream_check_config` on mount (spinner while loading, `:72-78`); load errors render a destructive `Alert` with the raw string.
- Permanent info `Alert` `streamCheck.connectivityNote`; group heading `streamCheck.checkParams`.
- Three number inputs held **as strings** so they can be emptied (`:20-25`): `streamCheck.timeout` (min 2, max 60, default `8`), `streamCheck.maxRetries` (0–5, default `1`), `streamCheck.degradedThreshold` (1000–30000 step 1000, default `6000`).
- Save (`:48-70`): `parseInt`, `NaN` → the default; `save_stream_check_config {config}` with `StreamCheckConfig {timeoutSecs, maxRetries, degradedThresholdMs}`; success `toast.success(t("streamCheck.configSaved"))`, failure `toast.error(t("streamCheck.configSaveFailed") + ": " + String(e))`. Button label `common.saving` / `common.save`.

### 5.6 `LogConfigPanel.tsx` — immediate save, optimistic with rollback
- Mount `get_log_config`; renders `null` while loading (`:45`). State default `{enabled: true, level: "info"}` (`:19-22`).
- Row 1: `settings.advanced.logConfig.enabled` / `.enabledDescription` `Switch` → `enabled`.
- Row 2: `settings.advanced.logConfig.level` / `.levelDescription` — 120px `Select`, `disabled={!config.enabled}`, options `error|warn|info|debug|trace` labelled `settings.advanced.logConfig.levels.{level}`.
- Every change → optimistic set + `set_log_config {config}`; failure → `toast.error(String(e))` + revert (`:33-43`).
- Legend block: heading `settings.advanced.logConfig.levelHint`, then five rows `settings.advanced.logConfig.levelDesc.{error|warn|info|debug|trace}` with colour-coded mono names.
- `LogConfig {enabled: boolean, level: "error"|"warn"|"info"|"debug"|"trace"}` (`src/lib/api/settings.ts:338-341`).

---

## 6. About tab — `AboutSection.tsx` (1272 lines), prop `isPortable`

### 6.1 Version & update card (`:822-952`)
- Header `common.about` / `settings.aboutHint`.
- App icon + literal "CC Switch"; Badge `common.version` + `v{version}` (spinner while loading). Version comes from **`getVersion()` of `@tauri-apps/api/app`** (`:399`), *not* a Tauri command, cached module-wide in `appVersionCache` (`:196`) so remounts don't flash.
- `isPortable` → extra Badge `settings.portableMode`.
- Four buttons (`:865-931`):
  - `settings.officialWebsite` → `openExternal("https://ccswitch.io")`
  - `settings.github` → `openExternal("https://github.com/farion1231/cc-switch")`
  - `settings.releaseNotes` → `handleOpenReleaseNotes` (`:429-452`): target = `updateInfo?.availableVersion ?? version`, prefixed with `v` if absent; opens `.../releases/tag/v{X}` or falls back to `.../releases`; failure → `toast.error(t("settings.openReleaseNotesFailed"))`.
  - Update button, disabled while `isChecking || isDownloading`; label priority: downloading → `settings.updating`; `hasUpdate` → `settings.updateTo {version}`; checking → `settings.checking`; else `settings.checkForUpdates`.
- **`settingsApi.openExternal` validates the scheme** in JS and rejects anything that isn't http/https before invoking `open_external {url}` (`src/lib/api/settings.ts:215-226`).
- When an update exists: a highlighted panel `settings.updateAvailable {version}` plus `updateInfo.notes` clamped to 3 lines (`:934-951`).

### 6.2 Update check flow
- `useUpdate()` from `src/contexts/UpdateContext.tsx`: state `hasUpdate, updateInfo, isChecking, error, isDismissed`; actions `checkUpdate`, `dismissUpdate`, `resetDismiss`. `checkUpdate` is re-entrancy guarded by a ref, calls `checkForUpdate({timeout:30000})`, and compares against `localStorage["ccswitch:update:dismissedVersion"]` (legacy key `dismissedUpdateVersion` is migrated). An automatic check fires **1 s after app start**.
- `src/lib/updater.ts`: `getCurrentVersion()` → `getVersion()` (empty string on failure); `checkForUpdate()` **dynamically imports `@tauri-apps/plugin-updater`** and calls `check({timeout})`, returning `{status:"up-to-date"}` or `{status:"available", info: UpdateInfo {currentVersion, availableVersion, notes?, pubDate?}}`.
- `handleCheckUpdate` (`:454-501`):
  - No update pending → `checkUpdate()`; if it returns false → `toast.success(t("settings.upToDate"))`; throw → `toast.error(t("settings.checkUpdateFailed"))`.
  - Update pending **and portable** → `settingsApi.checkUpdates()` (`check_for_updates`, opens the external updater) and return — portable builds never self-install.
  - Update pending, non-portable → `resetDismiss()`, `settingsApi.installUpdateAndRestart()` (`install_update_and_restart`); a falsy result → `toast.success(t("settings.upToDate"))`; throw → `toast.error(t("settings.updateFailed"), {description: extractErrorMessage(error)})` followed by a `check_for_updates` fallback.

### 6.3 Local environment / tool cards (`:954-1219`)
- Tools (`:62-70`): `claude, codex, gemini, grok, opencode, openclaw, hermes`; display names `Claude Code, Codex, Gemini CLI, Grok Build, OpenCode, OpenClaw, Hermes` (`:162-170`); app-id map at `:178-186` (`grok → grokbuild`).
- Section header `settings.localEnvCheck` plus three buttons:
  - Diagnose: `settings.toolDiagnose` / `settings.toolDiagnosing` → `probe_tool_installations {tools}` for all seven; conflicting tools are stored per card; zero conflicts → `toast.info(t("settings.toolDiagnoseNoConflict"))`; throw → `toast.error(t("settings.toolDiagnoseFailed"), {description})` (`:539-564`).
  - Refresh: `common.refresh` / `common.refreshing` → `loadAllToolVersions({force:true})`.
  - Batch update: `settings.updateAllTools {count}`, disabled when nothing is updatable.
  All three are disabled while `isLoadingTools || isAnyBusy` (`isAnyBusy = batchAction || toolActions non-empty || preflightTools non-empty`, `:810-813`).
- Caching: module-level `toolVersionsCache {data, at}` with a **10-minute TTL** (`:194-195`); stale-while-revalidate on remount; single-tool refreshes merge by name and deliberately do **not** reset the TTL stamp (`:299-312`).
- Version probing: `get_tool_versions {tools, wslShellByTool}` → `Array<{name, version, latest_version, error, installed_but_broken, env_type: "windows"|"wsl"|"macos"|"linux"|"unknown", wsl_distro}>` (`src/lib/api/settings.ts:236-254`). All seven are probed **concurrently**, each card leaving its loading state independently (`:351-355`).
- Per card: icon + name, an env badge for wsl/windows/macos/linux (`ENV_BADGE_CONFIG :83-107`, keys `settings.envBadge.{wsl|windows|macos|linux}`, suffixed ` · {wsl_distro}`); status glyph = spinner | `settings.updateAvailableShort` chip | green check | yellow alert; rows `settings.currentVersion` (value, or `settings.installedNotRunnable`, or `common.notInstalled`, or `common.loading`) and `settings.latestVersion` (value or `common.unknown`); the raw `tool.error` line when not installed.
- "Outdated" is decided by `isUpdateAvailable(current, latest)` in `src/lib/version.ts` — a real semver compare including pre-release ordering, returning `false` when either side is unparseable (guards against pre-release builds looking "outdated").
- **WSL-only extra controls** (`env_type === "wsl"`, `:1118-1157`): two 82px `Select`s — shell (`auto` + `sh, bash, zsh, fish, dash`) and shell flag (`auto` + `-lic, -lc, -c`), labelled `common.auto` for the auto entry. Changing either updates `wslShellByTool[tool]` and re-probes **only that tool** (`:367-388`).
- Conflict panel (`:1160-1176`): heading `settings.toolConflictTitle`, hint `settings.toolConflictHint`, one `ToolInstallRow` per installation.
- Card footer: loading → `common.loading`; `installed_but_broken` → text `settings.toolCheckEnv` and **no button**; else a button `settings.toolInstall` (outline) or `settings.toolUpdate` (default), disabled while busy; if nothing to do → `settings.toolReady`.

### 6.4 Install/upgrade execution
- `handleRunToolAction` (`:747-793`) is the single entry lock: early-returns when any target tool is already in `preflightTools` or `toolActions`; registers preflight; for `install` goes straight to `executeRun`; for `update` first calls `probe_tool_installations`; a probe error falls through to `executeRun`; reports with `needs_confirmation` open `ToolUpgradeConfirmDialog`.
- `executeRun` (`:567-737`) runs the tools **serially**, one `run_tool_lifecycle_action {tools:[t], action, wslShellByTool}` per tool, re-probing each tool right after. Result classification:
  - Version present and changed → success; after an `update`, always re-diagnose silently.
  - `action === "update"` and version unchanged while still outdated → **soft failure** `settings.toolActionVersionUnchanged {version, latest}` + silent re-diagnose.
  - Exit 0 but still no version → soft failure with `tool.error` or `settings.toolNotRunnable` + silent re-diagnose.
  - Thrown error → hard failure.
- Final toast: all-good → `toast.success(t("settings.toolActionDone",{count, action}))` where `action` is `settings.toolInstall` / `settings.toolUpdate`; zero successes and no hard failures → `toast.warning(settings.toolActionVersionUnchangedTitle)` or `settings.toolActionInstalledNotRunnable`; zero successes with hard failures → `toast.error(settings.toolActionFailed)`; partial → `toast.warning(t("settings.toolActionPartial",{succeeded, failed, action}))`. Batch descriptions show only the last line of each error; single-tool shows the full detail (`:684-694`).
- `ToolUpgradeConfirmDialog.tsx`: title `settings.toolUpgradeConfirmTitle`, description `settings.toolUpgradeConfirmHint`; per plan — tool name, optional `settings.toolUpgradeUnanchoredHint` when `!plan.anchored`, the install list, `settings.toolUpgradeWillRun` + `<code>{plan.command}</code>`; footer `common.cancel` / `settings.toolUpgradeConfirmBtn`.
- `ToolInstallRow.tsx`: source badge, path (truncated, `title=path`), then `inst.version` or `settings.toolConflictNotRunnable`, plus a `settings.toolConflictDefault` pill when `is_path_default`.
- Types (`src/lib/api/settings.ts:305-322`): `ToolInstallation {path, version, runnable, error, source, is_path_default}`; `ToolInstallationReport {tool, installs, is_conflict, needs_confirmation, command, anchored}`.

### 6.5 Manual install commands (`:1221-1261`)
Collapsible button `settings.manualInstallCommands` (chevron rotates); when open, hint `settings.oneClickInstallHint`, a `common.copy` button, and a `<pre>` of the platform command block. Copy uses **`navigator.clipboard.writeText`** → `settings.installCommandsCopied` / `settings.installCommandsCopyFailed` (`:503-511`). The block itself is chosen at module load by `isWindows()` (`:158-160`): POSIX uses `bash -c 'tmp=$(mktemp) && curl -fsSL <url> -o $tmp && bash $tmp; …'` wrappers with `|| npm i -g …` fallbacks (`:109-141`); Windows uses plain `npm i -g` plus a base64 `-EncodedCommand` PowerShell one-liner for Hermes (`:112-156`; note the UTF-16LE encoding helper at `:115-122`).

### 6.6 Sponsor links
**There are none.** A repo-wide case-insensitive grep for `sponsor|afdian|donate|爱发电` across `src/` returns nothing. The About tab's only outbound links are the three listed in §6.1.

---

## 7. Usage tab

`SettingsPage.tsx:512-519` mounts `<UsageDashboard refreshIntervalMs={settings?.usageDashboardRefreshIntervalMs} onRefreshIntervalChange={(v) => handleAutoSave({usageDashboardRefreshIntervalMs: v})} />` — the refresh interval is the **only** `AppSettings` field this tab owns. `UsageDashboard.changeRefreshInterval` (`:120-138`) optimistically sets, invalidates `usageKeys.all`, and reverts if the save returns `false` or throws. Allowed values `[0, 5000, 10000, 30000, 60000]`, default `30000`; anything else is normalized to the default (`:49-59`).

Component tree (`src/components/usage/`):
- `UsageDashboard.tsx` (435) — filter bar (app-type buttons + `usage.allSources` / `usage.allModels` selects + refresh-interval select + `UsageDateRangePicker`), inner `Tabs` logs/providers/models, then an Accordion item `settings.advanced.pricing.title` / `.description` → `PricingConfigPanel`. Mounts `useUsageEventBridge()` (listens for the backend `usage-log-recorded` event and invalidates all usage queries) and `useProviderStats`/`useModelStats` for the filter dropdowns.
- `UsageHero.tsx` (408) — headline totals; `useUsageSummaryByApp`.
- `UsageTrendChart.tsx` (243) — time-series; `useUsageTrends`.
- `RequestLogTable.tsx` (404) — paginated log table; `useRequestLogs`; opens `RequestDetailPanel`.
- `RequestDetailPanel.tsx` (324) — single request drill-down; `useRequestDetail`.
- `ProviderStatsTable.tsx` (103) — per-provider aggregates; `useProviderStats`.
- `ModelStatsTable.tsx` (97) — per-model aggregates; `useModelStats`.
- `PricingConfigPanel.tsx` (482) — pricing list/editor; `useModelPricing`, `useDeleteModelPricing`.
- `PricingEditModal.tsx` (259) — edit one model's pricing; `useUpdateModelPricing`.
- `ModelsDevPickerDialog.tsx` (452) — import pricing from models.dev; own `useQuery` + `useUpdateModelPricing`.
- `UsageDateRangePicker.tsx` (507) — preset/custom range picker (no data hooks).
- `DataSourceBar.tsx` (125) — data-source breakdown strip; own `useQuery` over `get_usage_data_sources`.
- `ConnectivityCheckConfigPanel.tsx` (169) — reused by the Advanced tab, see §5.5.
- `format.ts` — number/locale helpers.

Commands (`src/lib/api/usage.ts`): `queryProviderUsage {providerId, app}`, `testUsageScript`, `get_usage_summary`, `get_usage_summary_by_app`, `get_usage_trends`, `get_provider_stats`, `get_model_stats`, `get_request_logs`, `get_request_detail {requestId}`, `get_model_pricing`, `update_model_pricing`, `delete_model_pricing {modelId}`, `check_provider_limits {providerId, appType}`, `sync_session_usage`, `get_usage_data_sources`. Hooks live in `src/lib/query/usage.ts` with the `usageKeys` factory at `:37` and `useUpdateModelPricing`/`useDeleteModelPricing` invalidating `usageKeys.all` (`:378`, `:389`).

---

## 8. Platform-specific behaviour & Tauri JS APIs beyond `invoke`

**Platform gates** (all via user-agent sniffing in `src/lib/platform.ts` — the Rust port should swap these for `cfg!(target_os=…)` or the Tauri OS plugin):
- `WindowSettings.tsx:80-90` — the "use app window controls" toggle renders **only on Linux**.
- `TerminalSettings.tsx:48-74` — terminal option list and default vary by mac/windows/linux; unknown platform → macOS list.
- `AboutSection.tsx:158-160` — the manual install command block differs on Windows (npm-only + base64 PowerShell for Hermes) vs POSIX (curl|bash installers with npm fallbacks).
- `AboutSection.tsx:1118` — the WSL shell/flag selects appear only when the backend reports `env_type === "wsl"` (Windows only in practice).
- `platform.ts:41-51` — drag regions are disabled entirely on Linux (Wayland/Tauri #13440); relevant to the window chrome around this page.

**Tauri JS APIs other than `invoke`:**
- `@tauri-apps/api/app → getVersion()` — `src/lib/updater.ts:1,18` and `AboutSection.tsx:28,399`.
- `@tauri-apps/api/path → homeDir(), join()` — `src/hooks/useDirectorySettings.ts:4,65-66,80-81` (default config-dir computation).
- `@tauri-apps/plugin-updater → check()` — dynamically imported in `src/lib/updater.ts:31`.
- **Web APIs** that need Rust equivalents: `navigator.clipboard.writeText` (`AboutSection.tsx:505`), `localStorage` for `"language"` (`useSettings.ts:279,416`), `"cc-switch-theme"` (`theme-provider.tsx:30`), and `"ccswitch:update:dismissedVersion"` / legacy `"dismissedUpdateVersion"` (`UpdateContext.tsx:19-20`), `new URL()` parsing in `GlobalProxySettings` and `settingsApi.openExternal`, `btoa` for the PowerShell encoded command (`AboutSection.tsx:121`), and `toLocaleString()` date/size formatting in the backup and sync sections.
- No direct `@tauri-apps/api/window` usage inside `settings/*`; window controls are handled by `App.tsx` (`notifyWindowControlError`, around `:885`).

---

## 9. Complete command inventory touched by the Settings area

`get_settings`, `save_settings {settings}`, `has_codex_unify_history_backup`, `restore_codex_unified_history`, `restart_app`, `install_update_and_restart`, `check_for_updates`, `is_portable_mode`, `get_config_dir {app}`, `open_config_folder {app}`, `pick_directory {defaultPath}`, `get_claude_code_config_path`, `get_app_config_path`, `open_app_config_folder`, `get_app_config_dir_override`, `set_app_config_dir_override {path}`, `apply_claude_plugin_config {official}`, `apply_claude_onboarding_skip`, `clear_claude_onboarding_skip`, `save_file_dialog {defaultName}`, `open_file_dialog`, `export_config_to_file {filePath}`, `import_config_from_file {filePath}`, `webdav_test_connection {settings, preserveEmptyPassword}`, `webdav_sync_upload`, `webdav_sync_download`, `webdav_sync_save_settings {settings, passwordTouched}`, `webdav_sync_fetch_remote_info`, `s3_test_connection`, `s3_sync_upload`, `s3_sync_download`, `s3_sync_save_settings`, `s3_sync_fetch_remote_info`, `sync_current_providers_live`, `open_external {url}`, `set_auto_launch {enabled}`, `get_auto_launch_status`, `get_tool_versions {tools, wslShellByTool}`, `run_tool_lifecycle_action {tools, action, wslShellByTool}`, `probe_tool_installations {tools}`, `get_rectifier_config`, `set_rectifier_config {config}`, `get_optimizer_config`, `set_optimizer_config {config}`, `get_log_config`, `set_log_config {config}`, `create_db_backup`, `list_db_backups`, `restore_db_backup {filename}`, `rename_db_backup {oldFilename,newName}`, `delete_db_backup {filename}`, `migrate_skill_storage {target}`, `get_stream_check_config`, `save_stream_check_config {config}`, `get_global_proxy_url`, `set_global_proxy_url {url}`, plus the proxy (`get_proxy_status`, `get_proxy_takeover_status`, `start_proxy_server`, `stop_proxy_server`, `stop_proxy_with_restore`, `set_proxy_takeover_for_app`, `switch_proxy_provider`, `is_proxy_running`, `is_live_takeover_active`), auth (`auth_*`, `copilot_*`) and usage command sets listed above. Tray refresh goes through `providersApi.updateTrayMenu()`.

---

## 10. Things worth flagging before you port

1. **`showInTray` has no UI.** It is in `Settings` (`types.ts:346`) and is defaulted to `true` in the form, but nothing in `settings/*` toggles it.
2. **`Settings.language` is `"en"|"zh"|"zh-TW"|"ja"`** while `SettingsFormState["language"]` is the same union but non-optional — the payload always carries a language.
3. **Autosave sends the whole form.** A stale field left in local state after a failed save would be replayed by any later unrelated save; that is why `handleAutoSave` rolls back on failure (`SettingsPage.tsx:186-207`). Preserve that behaviour, or move to true per-field patches.
4. **`webdavSync` / `s3Sync` are stripped from every `save_settings` payload** (`useSettings.ts:198-202`, `:338-342`); they round-trip only through their own commands. Do not let the Rust form struct serialize them back.
5. **`lastSyncAt` is in seconds**, multiplied by 1000 for display (`WebdavSyncSection.tsx:939`, `:953`).
6. **i18n gaps (real):** `proxy.server.stopped` and `proxy.server.stopFailed` are referenced in `src/hooks/useProxyStatus.ts:72,84` but absent from `src/i18n/locales/en.json` (`proxy.server` only has `started` and `startFailed`) — English users get the Chinese `defaultValue`. Everything referenced from `src/components/settings/*` resolves.
7. **Untranslated literal:** the backup list's loading state is a hard-coded `"Loading..."` (`BackupListSection.tsx:296`) while every sibling uses `common.loading`.
8. **S3 has no auto-sync first-run confirmation** even though WebDAV does (`confirm.autoSync.*`); also the S3 preset selector is display-only and is never persisted, so it resets to `aws-s3` on every mount (`WebdavSyncSection.tsx:276`).
9. **The S3 incompatible-snapshot toast uses `{version}`** while WebDAV's uses `{protocolVersion, dbCompatVersion}` — the two messages are not interchangeable.
