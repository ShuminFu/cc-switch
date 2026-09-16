# Remaining views — behavioural spec

Companion to `config-utils.md` / `settings-area.md` / `providers-list.md`. Describes the React sources as of v3.17.0 so each area can be rebuilt in Rust/Dioxus without re-reading the TSX. All paths are relative to repo root.

## 0. Shell: App.tsx routing, header wiring, startup

### 0.1 View model

`src/App.tsx:99-113` defines the view union; `:144-168` persists it.

| Concern | Value | Location |
|---|---|---|
| `View` union | `providers \| settings \| prompts \| skills \| skillsDiscovery \| mcp \| agents \| universal \| sessions \| workspace \| openclawEnv \| openclawTools \| openclawAgents \| hermesMemory` | `App.tsx:99-113` |
| View persistence | `localStorage["cc-switch-last-view"]`, validated against `VALID_VIEWS`, fallback `providers` | `App.tsx:144-168`, write at `:184-186` |
| App persistence | `localStorage["cc-switch-last-app"]`, validated against `VALID_APPS`, fallback `claude` | `App.tsx:124-142` |
| `sharedFeatureApp` | `activeApp === "claude-desktop" ? "claude" : activeApp` — used for prompts/skills/sessions | `App.tsx:175-176` |
| Layout metrics | `DEFAULT_DRAG_BAR_HEIGHT = isWindows()||isLinux() ? 0 : 28`; `HEADER_HEIGHT = 64`; `dragBarHeight = useAppWindowControls ? 32 : DEFAULT`; `contentTopOffset = dragBarHeight + HEADER_HEIGHT` | `App.tsx:121-122, 189-192` |

Guards:
- If `visibleApps[activeApp]` is false → jump to first visible app in fixed order claude → claude-desktop → codex → gemini → grokbuild → opencode → openclaw → hermes (`App.tsx:204-220`).
- If `currentView === "sessions"` and the app has no session support → force `providers` (`App.tsx:222-236`). Session support = claude, codex, grokbuild, opencode, openclaw, gemini, hermes (`:295-302`).
- `hasSkillsSupport = sharedFeatureApp !== "openclaw"` (`:294`).
- `isOpenClawView` (controls the health banner) = `activeApp === "openclaw"` AND view ∈ {providers, workspace, sessions, openclawEnv, openclawTools, openclawAgents} (`:284-291`).

### 0.2 Keyboard shortcuts (global, `App.tsx:600-625`)

| Keys | Behaviour |
|---|---|
| `Cmd/Ctrl + ,` | `preventDefault`, `setCurrentView("settings")` — always, even inside inputs |
| `Escape` | No-op if `event.defaultPrevented`; no-op if `document.body.style.overflow === "hidden"` (a `FullScreenPanel` is open); no-op on `providers`; no-op if `isTextEditableTarget(event.target)` (INPUT/TEXTAREA/SELECT/contentEditable — `src/utils/domUtils.ts:89-99`). Otherwise `preventDefault` and go `skillsDiscovery → skills`, else → `providers`. |

`FullScreenPanel` (`src/components/common/FullScreenPanel.tsx:56-84`) installs its own bubbling-phase Escape listener that `stopPropagation()`s so App's handler never fires; it also sets `document.body.style.overflow = "hidden"` while open (`:44-51`). That interlock must be reproduced.

### 0.3 Header actions contributed per view

Header title (`App.tsx:1171-1192`):

| View | Title key |
|---|---|
| settings | `settings.title` |
| prompts | `prompts.title` `{appName: t("apps."+sharedFeatureApp)}` |
| skills / skillsDiscovery | `skills.title` |
| mcp | `mcp.unifiedPanel.title` |
| agents | `agents.title` |
| universal | `universalProvider.title` |
| sessions | `sessionManager.title` |
| workspace | `workspace.title` |
| openclawEnv/Tools/Agents | `openclaw.env.title` / `openclaw.tools.title` / `openclaw.agents.title` |
| hermesMemory | `hermes.memory.title` |

Right-hand toolbar actions (`App.tsx:1288-1395`), all `variant="ghost" size="sm"`:

| View | Button | Icon | Label key | Handler |
|---|---|---|---|---|
| prompts | Add | `Plus` | `prompts.add` | `promptPanelRef.current?.openAdd()` (`:1292`) |
| mcp | Import Existing | `Download` | `mcp.importExisting` | `mcpPanelRef.current?.openImport()` (`:1304`) |
| mcp | Add MCP | `Plus` | `mcp.addMcp` | `mcpPanelRef.current?.openAdd()` (`:1313`) |
| skills | Restore Backup | `History` | `skills.restoreFromBackup.button` | `unifiedSkillsPanelRef.current?.openRestoreFromBackup()` (`:1327`) |
| skills | Install from ZIP | `FolderArchive` | `skills.installFromZip.button` | `…openInstallFromZip()` (`:1338`) |
| skills | Import | `Download` | `skills.import` | `…openImport()` (`:1349`); green 8px dot at top-right when `hasUnmanagedSkills`, `title=skills.unmanagedAvailable` (`:1352-1365`) |
| skills | Discover | `Search` | `skills.discover` | `handleOpenSkillsDiscovery()` → sets source `repos`, view `skillsDiscovery` (`:889-892`) |
| skillsDiscovery | dynamic | see §2.3 | | `getSkillsPageHeaderActions(skillsDiscoverySource)` mapped over `skillsPageRef.current` (`:1380-1393`) |
| sessions / workspace / universal / agents / openclaw* / hermesMemory | **none** | | | |

`providers` view additionally renders `AppSwitcher`, a per-app icon cluster, and the orange add FAB (`:1396-1573`) — covered by `providers-list.md`. Note the icon cluster is what navigates *into* most of the views specced here:

| Active app | Cluster buttons (title key → view) |
|---|---|
| `hermes` | `skills.manage`→skills; `hermes.memory.title`→hermesMemory; `hermes.webui.open`→`openHermesWebUI()`; `mcp.title`→mcp (`:1423-1461`) |
| `openclaw` | `workspace.manage`→workspace; `openclaw.env.title`→openclawEnv; `openclaw.tools.title`→openclawTools; `openclaw.agents.title`→openclawAgents; `sessionManager.title`→sessions (`:1462-1509`) |
| other | `skills.manage`→skills (animated to width 0 when `!hasSkillsSupport`); `prompts.manage`→prompts; `sessionManager.title`→sessions (width 0 when `!hasSessionSupport`); `mcp.title`→mcp (`:1510-1560`) |

Imperative-ref handles to reproduce (Dioxus: model as a shared signal/enum command channel, since Dioxus has no `useImperativeHandle`):

| Ref | Handle type | Members |
|---|---|---|
| `promptPanelRef` | `PromptPanelHandle` (`prompts/PromptPanel.tsx:17-19`) | `openAdd()` |
| `mcpPanelRef` | `UnifiedMcpPanelHandle` (`mcp/UnifiedMcpPanel.tsx:29-32`) | `openAdd()`, `openImport()` |
| `unifiedSkillsPanelRef` | `UnifiedSkillsPanelHandle` (`skills/UnifiedSkillsPanel.tsx:52-58`) | `openDiscovery()`, `openImport()`, `openInstallFromZip()`, `openRestoreFromBackup()`, `checkUpdates()` — App wires only the middle three; `checkUpdates` is reachable only from the in-panel button, `openDiscovery` only via the `onOpenDiscovery` prop |
| `skillsPageRef` | `SkillsPageHandle` (`skills/SkillsPage.tsx:52-55`) | `refresh()`, `openRepoManager()` |

All four are typed `useRef<any>(null)` in App (`:255-258`) — the port should use the concrete handle types.

### 0.4 App-level Tauri event subscriptions

`useTauriEvent(name, handler)` (`src/hooks/useTauriEvent.ts:8-39`) wraps `listen` with disposal guarding.

| Event | Payload | Effect | Line |
|---|---|---|---|
| `provider-switched` (via `providersApi.onSwitched`) | `ProviderSwitchEvent` | if `appType === activeApp` → `refetch()` providers | `App.tsx:351-379` |
| `universal-provider-synced` | — | invalidate `["providers"]`, then `providersApi.updateTrayMenu()` | `:381-388` |
| `profile-applied` | — | invalidate `["profiles"]`, `["mcp","all"]`, `["skills"]`, `["proxyTakeoverStatus"]`, `["proxyStatus"]`, `["providers","claude-desktop"]` | `:392-401` |
| `webdav-sync-status-updated` | `{source?, status?, error?}` | invalidate `["settings"]`; if `source==="auto" && status==="error"` → `toast.error(settings.webdavSync.autoSyncFailedToast, {error})` | `:403-417` |
| `s3-sync-status-updated` | same | same with `settings.s3Sync.autoSyncFailedToast` | `:419-433` |
| `proxy-official-warning` | `{appType, providerName}` | `toast.warning(notifications.proxyOfficialWarning, {name}, duration 8000)` | `:435-446` |

### 0.5 Startup flows

**`src/main.tsx`** — two render branches.

1. Platform class: adds `body.is-mac` when UA/platform matches (`:19-28`).
2. Registers `listen("configLoadError")` → `handleConfigLoadError` (`:67-74`).
3. `bootstrap()` (`:76-116`):
   - `invoke<ConfigLoadErrorPayload|null>("get_init_error")`.
   - If `initError.kind === "db_version_too_new"` → render **only** `ThemeProvider > DatabaseUpgrade + Toaster` (no QueryClientProvider, no App) and `return` (`:82-93`).
   - Else if `initError.path || initError.error` → `handleConfigLoadError` → native `message()` dialog with title `errors.configLoadFailedTitle`, body `errors.configLoadFailedMessage {path, detail}` (defaults `~/.cc-switch/config.json` / `"Unknown error"`), `kind: "error"`, then `exit(1)` — no cancel option (`:42-64`).
   - Otherwise render `QueryClientProvider > ThemeProvider > UpdateProvider > App + Toaster` (`:104-115`).
   - Theme: `defaultTheme="system"`, `storageKey="cc-switch-theme"`.

**`DatabaseUpgrade`** (`src/components/DatabaseUpgrade.tsx`):

| Aspect | Detail |
|---|---|
| Props | `payload: { path?, error?, kind?, db_version?, supported_version? }` (`:19-27`) |
| Phases | `checking → upgradable \| incompatible`, plus `updating`, `error` (`:34`) |
| Phase resolution | `invoke<string|null>("check_app_update_available")`; non-null → `upgradable` + `availableVersion`; null → `incompatible`; **throw → `upgradable`** (offline must not deadlock) (`:61-83`) |
| Upgrade | `listen<{downloaded,total}>("update-download-progress")` then `invoke<boolean>("install_update_and_restart")`. `true` ⇒ app restarts, stay in `updating`. `false` ⇒ race, fall to `incompatible`. Throw ⇒ `error` + message (`:91-116`) |
| Progress | `percent = min(100, round(downloaded/total*100))` when `total > 0`, else indeterminate (`w-1/3 animate-pulse`); MB shown to 1 decimal (`:118-122, 211-241`) |
| Accent | `incompatible` → red + `AlertTriangle`; else amber + `Database` (`:124-134`) |
| Buttons | Upgrade/Retry (`dbUpgrade.upgradeNow`/`dbUpgrade.retry`) on `upgradable\|error`; `dbUpgrade.openReleases` → `invoke("open_external", {url:"https://github.com/farion1231/cc-switch/releases"})` on `incompatible\|error`; `dbUpgrade.openConfigDir` → `invoke("open_app_config_folder")`, disabled while `updating`; `dbUpgrade.quit` → `exit(0)`, disabled while `updating` (`:249-297`) |
| i18n | `dbUpgrade.{title,description,versionInfo,dbPath,checking,updateAvailable,incompatibleTitle,incompatibleDescription,preparing,downloading,upgradeNow,retry,openReleases,openConfigDir,quit}` — all present |

**`FirstRunNoticeDialog`** (`src/components/FirstRunNoticeDialog.tsx`, mounted at `App.tsx:1668`):
- `isOpen = settings != null && settings.firstRunNoticeConfirmed !== true` (`:24`). Backend decides eligibility at startup by pre-writing `true`; frontend only null-checks.
- Ack (button **or** dismiss) strips `webdavSync` from settings and calls `settingsApi.save({...rest, firstRunNoticeConfirmed:true})`, then invalidates `["settings"]` (`:26-35`). Errors are only `console.error`ed.
- Keys: `firstRunNotice.title`, `.bodyDefault`, `.bodyOfficial` (two separate paragraphs, `whitespace-pre-line`), `.confirm`. `zIndex="top"`.

**`UpdateContext` / `UpdateBadge`**:

| Item | Detail |
|---|---|
| Auto check | `setTimeout(checkUpdate, 1000)` on mount (`UpdateContext.tsx:119-126`) |
| Reentrancy | `isCheckingRef` guard returns `false` immediately (`:62-64`) |
| Dismiss storage | `localStorage["ccswitch:update:dismissedVersion"]`, migrating legacy key `"dismissedUpdateVersion"` (read → copy → remove) in both the effect (`:41-57`) and `checkUpdate` (`:74-84`) |
| `checkForUpdate` | dynamic `import("@tauri-apps/plugin-updater")`, `check({timeout: 30000})`; result `{status:"up-to-date"} | {status:"available", info:{currentVersion, availableVersion, notes?, pubDate?}}` (`src/lib/updater.ts:25-48`) |
| Errors | set `error`, `hasUpdate=false`, then **rethrow** (`:92-97`) |
| Badge | Renders **nothing** unless `hasUpdate && updateInfo`; then a ghost icon button `ArrowUpCircle`, green, title `settings.updateAvailable {version}` (`UpdateBadge.tsx:11-41`). In App it opens Settings on the `about` tab (`App.tsx:1223-1228`) |
| Note | `isDismissed` is computed and exposed but the badge ignores it — dismissal does not hide the badge |

**Env conflict warning** (`src/components/env/EnvWarningBanner.tsx`, `src/lib/api/env.ts`):

| Aspect | Detail |
|---|---|
| Startup scan | `checkAllEnvConflicts()` over apps `["claude","codex","gemini"]` in parallel, per-app failures swallowed to `[]` (`env.ts:42-59`); flattened, stored, banner shown unless `sessionStorage["env_banner_dismissed"]` (`App.tsx:493-515`) |
| On app switch | `checkEnvConflicts(activeApp)`, appended de-duped by `` `${varName}:${sourcePath}` `` (`App.tsx:563-592`) |
| Commands | `check_env_conflicts {app}` → `EnvConflict[]`; `delete_env_vars {conflicts}` → `BackupInfo`; `restore_env_backup {backupPath}` (unused by this banner) |
| Types | `EnvConflict {varName, varValue, sourceType:"system"|"file", sourcePath}`; `BackupInfo {backupPath, timestamp, conflicts}` (`src/types/env.ts:8-29`) |
| Layout | `fixed top-0 inset-x-0 z-[100]`, yellow, `AlertTriangle`; collapsed row = `env.warning.title` + `env.warning.description {count}`, expand/collapse (`env.actions.expand`/`.collapse`) and `X` dismiss |
| Expanded | select-all checkbox (`env.actions.selectAll`), scroll list (`max-h-96`) of checkbox rows showing `varName`, `env.field.value`, `env.field.source`; source label maps `sourceType==="system"` + `HKEY_CURRENT_USER`→`env.source.userRegistry`, `HKEY_LOCAL_MACHINE`→`env.source.systemRegistry`, else `env.source.systemEnv`; file type shows raw path (`:98-110`) |
| Actions | `env.actions.clearSelection` (disabled at 0) and destructive `env.actions.deleteSelected {count}` → confirm dialog (`env.confirm.title/.message{count}/.backupNotice/.confirm`, `zIndex="top"`) |
| Delete | `deleteEnvVars(selected)` → success toast `env.delete.success` + description `env.backup.location {path}`, 5 s, closeButton; then clear selection and call `onDeleted` which re-runs `checkAllEnvConflicts` and auto-hides when empty (`App.tsx:1117-1131`). Empty selection → `toast.warning(env.error.noSelection)`. Failure → `toast.error(env.delete.error, {description: String(error)})` |
| Dismiss | writes `sessionStorage["env_banner_dismissed"]="true"` (`App.tsx:1113-1116`) |

**Migration result banners** (toasts, not banners, `App.tsx:517-561`):

| Check | Command | Outcome |
|---|---|---|
| Config migration | `invoke<boolean>("get_migration_result")` | `true` → `toast.success(migration.success, {closeButton:true})` |
| Skills SSOT migration | `invoke<{count, error?}|null>("get_skills_migration_result")` | `error` → `toast.error(migration.skillsFailed, {description: migration.skillsFailedDescription, closeButton})` and return. `count > 0` → `toast.success(migration.skillsSuccess {count})` then invalidate `["skills"]`. `count === 0` → silent |

Both run once on mount (deps `[t]` / `[t, queryClient]`).

**Agents panel** (`src/components/agents/AgentsPanel.tsx`) — a 22-line placeholder: centered `Bot` icon in a pulsing circle, hardcoded English "Coming Soon" and a description paragraph. **No i18n**, no data, no actions. `onOpenChange` prop is declared and destructured away unused. Reachable only via `VIEW_STORAGE_KEY` restore (no navigation button renders it in v3.17.0).

---

## 1. MCP

Files: `src/components/mcp/{UnifiedMcpPanel,McpFormModal,McpWizardModal}.tsx`, `src/components/mcp/useMcpValidation.ts`, `src/hooks/useMcp.ts`, `src/lib/api/mcp.ts`, `src/config/mcpPresets.ts`, `src/utils/tomlUtils.ts`.

### 1.1 Types (`src/types.ts:472-530`)

```ts
export interface McpServerSpec {
  type?: "stdio" | "http" | "sse";
  command?: string; args?: string[]; env?: Record<string,string>; cwd?: string;   // stdio
  url?: string; headers?: Record<string,string>;                                  // http/sse
  [key: string]: any;                                                             // extensions preserved
}
export interface McpApps {
  claude: boolean; "claude-desktop"?: boolean; codex: boolean; gemini: boolean;
  grokbuild?: boolean; opencode: boolean; openclaw: boolean; hermes: boolean;
}
export interface McpServer {
  id: string; name: string; server: McpServerSpec; apps: McpApps;
  description?: string; tags?: string[]; homepage?: string; docs?: string;
  enabled?: boolean; /** deprecated pre-3.7 */ source?: string; [key: string]: any;
}
export type McpServersMap = Record<string, McpServer>;
export interface McpStatus { userConfigPath: string; userConfigExists: boolean; serverCount: number }
export interface McpConfigResponse { configPath: string; servers: Record<string, McpServer> }
```

### 1.2 Commands (`src/lib/api/mcp.ts`)

| Method | Command | Args | Result | Line |
|---|---|---|---|---|
| `getAllServers` | `get_mcp_servers` | — | `McpServersMap` | :94-96 |
| `upsertUnifiedServer` | `upsert_mcp_server` | `{ server: McpServer }` | `void` | :101-103 |
| `deleteUnifiedServer` | `delete_mcp_server` | `{ id }` | `bool` | :108-110 |
| `toggleApp` | `toggle_mcp_app` | `{ serverId, app, enabled }` | `void` | :115-121 |
| `importFromApps` | `import_mcp_from_apps` | — | `number` (count) | :126-128 |
| `getStatus` | `get_claude_mcp_status` | — | `McpStatus` | :12-14 |
| `readConfig` | `read_claude_mcp_config` | — | `string \| null` | :16-18 |
| `validateCommand` | `validate_mcp_command` | `{ cmd }` | `bool` | :31-33 |

Deprecated and unused by these views (port only if the backend still needs them): `get_mcp_config`, `upsert_mcp_server_in_config`, `delete_mcp_server_in_config`, `set_mcp_enabled`, `upsert_claude_mcp_server`, `delete_claude_mcp_server` (`mcp.ts:20-85`).

### 1.3 Query hooks (`src/hooks/useMcp.ts`)

| Hook | Key / mutation | Invalidation |
|---|---|---|
| `useAllMcpServers` | `["mcp","all"]`, no staleTime | — |
| `useUpsertMcpServer` | `upsertUnifiedServer` | `onSuccess` → `["mcp","all"]` |
| `useToggleMcpApp` | `toggleApp` | `onSuccess` → `["mcp","all"]` |
| `useDeleteMcpServer` | `deleteUnifiedServer` | `onSuccess` → `["mcp","all"]` |
| `useImportMcpFromApps` | `importFromApps` | **`onSettled`** → `["mcp","all"]` (backend is best-effort; partial failures still persist rows — `useMcp.ts:70-74`) |

### 1.4 UnifiedMcpPanel

Layout (`UnifiedMcpPanel.tsx:143-213`): root `px-6 flex flex-col flex-1 min-h-0 overflow-hidden`, then

1. `AppCountBar` — `totalLabel = t("mcp.serverCount", {count})`, per-app counts, `appIds = MCP_APP_IDS` (`src/config/appConfig.tsx:40` ⇒ `SKILLS_APP_IDS` = `["claude","codex","gemini","grokbuild","opencode","hermes"]` — **claude-desktop and openclaw are never shown**, even though the counts object has all 8 keys, `:58-75`).
2. Scroll area `flex-1 overflow-y-auto pb-24`, three states: loading → `mcp.loading`; empty → `Server` icon + `mcp.unifiedPanel.noServers` + `mcp.emptyDescription`; else a bordered rounded list of `UnifiedMcpListItem`.

Row (`:227-317`, inside `TooltipProvider delayDuration={300}`):
- Name = `server.name || id`; if `docs` (or preset `docs`/`homepage` fallback) → 12px `ExternalLink` button titled `mcp.presets.docs` that calls `settingsApi.openExternal(url)` and swallows errors.
- Second line: `description` (truncated, `title` = full) else `tags.join(", ")`.
- Preset enrichment: `mcpPresets.find(p => p.id === id)` supplies `docs`/`homepage`/`tags` when the stored server lacks them (`:239-242`).
- `AppToggleGroup` over `MCP_APP_IDS` — 28×28 buttons, enabled = app-colored ring, disabled = `opacity-35`; tooltip shows `label` + `" ✓"` when on (`src/components/common/AppToggleGroup.tsx:57-85`).
- Edit (`Edit3`) / Delete (`Trash2`, red hover) buttons, `opacity-0 group-hover:opacity-100`.

Actions:

| Action | Behaviour |
|---|---|
| toggle app | `toggleAppMutation.mutateAsync({serverId, app, enabled})`; on throw `toast.error(common.error, {description: String(error)})` (`:77-87`) |
| edit | set `editingId`, open form |
| add (`openAdd`) | clear `editingId`, open form |
| import (`openImport`) | `importMutation.mutateAsync()`; `0` → `toast.success(mcp.unifiedPanel.noImportFound)`; `n>0` → `toast.success(mcp.unifiedPanel.importSuccess {count})`; throw → `toast.error(common.error, {description})` (`:99-114`) |
| delete | `ConfirmDialog` title `mcp.unifiedPanel.deleteServer`, message `mcp.unifiedPanel.deleteConfirm {id}`; confirm → delete mutation, close dialog, `toast.success(common.success)`; error toast keeps the dialog open (`:121-136`) |

Form mount (`:187-201`) passes `editingId`, `initialData = serversMap[editingId]`, `existingIds = Object.keys(serversMap)`, **`defaultFormat="json"`** — so the entire TOML branch of `McpFormModal` is unreachable from this view. `onSave` just closes.

### 1.5 McpFormModal

Props (`McpFormModal.tsx:28-46`): `editingId?`, `initialData?`, `onSave`, `onClose`, `existingIds = []`, `defaultFormat = "json"`, `defaultEnabledApps = ["claude","codex","gemini","grokbuild"]`.

State: `formId` (`editingId || initialData?.id || ""`), `formName`, `formDescription`, `formHomepage`, `formDocs`, `formTags` (comma-joined), `enabledApps` (7 booleans: claude, codex, gemini, grokbuild, opencode, openclaw, hermes — seeded from `initialData.apps` with `grokbuild ?? false`, else from `defaultEnabledApps`), `formConfig`, `configError`, `saving`, `isWizardOpen`, `idError`, `isDarkMode`, `showMetadata`, `selectedPreset` (`isEditing ? null : -1`).

Dark mode is observed via a `MutationObserver` on `documentElement.class` (`:125-138`) — in Dioxus use the shared theme signal.

Layout: `FullScreenPanel` titled `mcp.editServer` / `mcp.addServer`; footer = single primary button (`common.save`/`common.add`, `common.saving` while pending), disabled when `saving || (!isEditing && idError)`.

Body, upper card (`:445-692`):
1. **Preset chips** (add-mode only): `presetSelector.custom` chip (index `-1`) + one chip per `mcpPresets` entry labelled by `preset.id`, `title = t("mcp.presets.<id>.description")`. Selected chip = emerald.
2. **ID** `mcp.form.title` (required, red `*`), placeholder `mcp.form.titlePlaceholder`, **disabled when editing**; inline red `idError` on the right.
3. **Name** `mcp.form.name` / `mcp.form.namePlaceholder`.
4. **Enabled apps** `mcp.form.enabledApps` — 6 checkboxes: claude, codex, gemini, grokbuild, opencode, hermes with labels `mcp.unifiedPanel.apps.*`. **`openclaw` has state but no checkbox** (`:520-622`), so its value is only ever what `initialData`/`defaultEnabledApps` set.
5. **Collapsible** `mcp.form.additionalInfo` (ChevronUp/Down), default open when editing and any of description/tags/homepage/docs exist (`:92-101`); contains Description, Tags (`mcp.form.tags` "comma separated"), Homepage, Docs with their `*Placeholder` keys.

Body, lower card: label `mcp.form.jsonConfig` (or `mcp.form.tomlConfig`), a `mcp.form.useWizard` link shown when `isEditing || selectedPreset === -1`, then `JsonEditor` (`value`, `onChange`, placeholder `mcp.form.jsonPlaceholder`/`tomlPlaceholder`, `rows=12`, `height="100%"`, `showValidation={!useToml}`, `language = useToml ? "javascript" : "json"`), plus an `AlertCircle` + `configError` line.

`JsonEditor` props contract: `{id?, value, onChange, placeholder?, darkMode?, rows=12, showValidation=true, language="json"|"javascript", height?}` (`src/components/JsonEditor.tsx:14-35`).

#### Preset application (`:188-211`)
`ensureUniqueId(preset.id)` (appends `-1`, `-2`, … against `existingIds`, falls back to `"mcp-server"` for empty, `:179-186`) → sets id/name/description/homepage/docs/tags, serialises `preset.server` to JSON (2-space) or TOML, and immediately runs the matching validator. `applyCustom()` clears every field and `configError`.

#### Config change (`:225-268`)
- TOML: `normalizeTomlText(value)` first (smart-quote → ASCII, `src/utils/textNormalization.ts:7-22`), `validateTomlConfig`; on clean parse with an empty `formId`, auto-fill from `extractIdFromToml`.
- JSON: `parseSmartMcpJson(value)` (`src/utils/formatters.ts:26-65`) — wraps a bare `"key": {...}` fragment in braces, and if the result is a single-key object whose value is an object, returns `{id: key, config: value}`. Then `validateJsonConfig(JSON.stringify(result.config))`. On success with empty `formId` and not editing → `ensureUniqueId(result.id)`, and if `formName` is empty set it to the raw `result.id`. Parse throw → `configError = t("mcp.error.jsonInvalid") + ": " + message`.

Note the asymmetry: the JSON path validates the **extracted inner config**, while `handleSubmit` re-parses and stores `result.config` — so a `{"my-server": {...}}` paste is accepted and stored unwrapped, but a top-level `mcpServers` wrapper is rejected by `validateJsonConfig`.

#### Validation (`src/components/mcp/useMcpValidation.ts`)

| Fn | Rules |
|---|---|
| `validateJson` | empty → ok; non-object / array / parse error → `mcp.error.jsonInvalid` |
| `formatTomlError` | `"mustBeObject"`/`"parseError"` → `mcp.error.tomlInvalid`; anything else → `` `${t("mcp.error.tomlInvalid")}: ${err}` `` |
| `validateTomlConfig` | `validateToml` then `tomlToMcpServer`; stdio without `command` → `mcp.error.commandRequired`; http/sse without `url` → `mcp.wizard.urlRequired` |
| `validateJsonConfig` | base JSON check, then: has `mcpServers` key → `mcp.error.singleServerObjectRequired`; `type==="stdio"` w/o command → `mcp.error.commandRequired`; `http`/`sse` w/o url → `mcp.wizard.urlRequired` |

#### Submit (`:290-415`)
1. Trim id; empty → `toast.error(mcp.error.idRequired, {duration:3000})`.
2. Add-mode duplicate → set `idError = mcp.error.idExists`, return (no toast).
3. Build `serverSpec`: empty editor ⇒ `{type:"stdio", command:"", args:[]}`; TOML errors ⇒ `toast.error(mcp.error.tomlInvalid)` (3 s for validator errors, 4 s for parse throws); JSON parse throw ⇒ set error + `toast.error(mcp.error.jsonInvalid, {duration:4000})`.
4. Post-checks: stdio with blank command → `mcp.error.commandRequired`; http/sse with blank url → `mcp.wizard.urlRequired` (both 3 s toasts).
5. Entry = `{...initialData, id, name: (formName||id).trim() || id, server, apps: enabledApps}`; description/homepage/docs/tags are **set when non-empty and `delete`d when empty** (so clearing a field removes it); tags split on `,`, trimmed, empties dropped.
6. `upsertMutation.mutateAsync(entry)` → `toast.success(common.success, {closeButton:true})` → `await onSave()`.
7. Failure: `extractErrorMessage` → `translateMcpBackendError(detail, t)` (`src/utils/errorUtils.ts:45+`, maps Chinese backend strings onto `mcp.error.*`) → toast with the mapped/raw message or `mcp.error.saveFailed`; duration 6000 if a detail existed else 4000.

**Edge case:** spreading `initialData` preserves legacy `enabled`/`source`/unknown fields on edit.

### 1.6 McpWizardModal

Dialog (`max-w-2xl max-h-[90vh]`, `zIndex="alert"`), title `mcp.wizard.title`, hint box `mcp.wizard.hint`.

Fields: three radios `mcp.wizard.type` (`typeStdio`/`typeHttp`/`typeSse`); Title (`mcp.form.title`, required, mono); for **stdio** Command (`mcp.wizard.command`, required) + Args textarea (newline-separated, 3 rows) + Env textarea (`KEY=VALUE` per line); for **http/sse** URL (`mcp.wizard.url`, required) + Headers textarea (`KEY: VALUE` **or** `KEY=VALUE`).

Parsers (`:27-70`): env splits on the **first** `=` with `idx > 0`; headers choose `:` when it precedes `=` (or `=` is absent), else `=`; both trim and drop blank keys.

Preview (`:97-134, 400-414`): `JSON.stringify(config, null, 2)` in a `<pre>`, rendered only when any of command/args/env/url/headers is non-empty. `config.command` / `config.url` are always emitted (possibly empty strings); `args`/`env`/`headers` only when non-empty.

Hydration (`:174-222`, deps `[isOpen]` only — intentionally ignores later prop changes): title ← `initialTitle`, type ← `initialServer.type ?? (initialServer.url ? "http" : "stdio")`, then fills the matching branch and clears the other. `McpFormModal` computes `initialServer` from the live editor text (TOML/JSON parse with fallback to `initialData.server`, `:142-165`).

Apply (`:136-153`): validates title → `mcp.error.idRequired`; stdio command → `mcp.error.commandRequired`; url → `mcp.wizard.urlRequired`; all 3000 ms toasts. Then `onApply(title.trim(), json)` and `handleClose()` which **resets every field**.

Back in the form (`handleWizardApply`, `:270-288`): sets `formId = title` unconditionally (bypasses the duplicate check), sets `formName` if blank, writes the config (converting to TOML when in TOML mode; conversion failure → `mcp.error.jsonInvalid`) and revalidates.

Keyboard: `Cmd+Enter` (metaKey only, **not** ctrl) on the Title/Command/URL inputs triggers Apply (`:167-172`).

Footer: `common.cancel` (outline) + `mcp.wizard.apply` (`variant="mcp"`, `Save` icon).

### 1.7 TOML/JSON import (`src/utils/tomlUtils.ts`)

| Fn | Behaviour |
|---|---|
| `validateToml(text)` | empty → `""`; normalise then `smol-toml.parse`; non-object/array → `"mustBeObject"`; throw → `e.message \|\| "parseError"` (`:10-23`) |
| `mcpServerToToml(spec)` | shallow copy, drop `undefined` keys, `stringify().trim()` — **preserves extension fields**, loses comments (`:30-41`) |
| `tomlToMcpServer(text)` | empty → throw `"TOML 内容不能为空"`. Accepts three shapes: (1) direct spec when any of `type/command/url/args/env` present; (2) `[mcp_servers.<id>]` → first entry; (3) tolerant `[mcp.servers.<id>]` → first entry. Otherwise throws the "无法识别的 TOML 格式" message (`:53-95`) |
| `normalizeServerConfig` | default `type = "stdio"`. stdio requires string `command` else throws; copies `args` (stringified elements), `env` (values stringified), `cwd`; http/sse requires string `url`, copies `headers` (values stringified). **All unknown keys are copied through** (e.g. `timeout_ms`). Unknown `type` → throw `不支持的 MCP 服务器类型: <type>` (`:101-182`) |
| `extractIdFromToml(text)` | first key under `mcp_servers` or `mcp.servers`; else basename of `command` with `.exe/.bat/.sh/.js/.py` stripped; `""` on any failure (`:189-221`) |

Note: error strings thrown here are Chinese and are surfaced verbatim via `formatTomlError` — the Rust port should keep them behind i18n or replicate.

### 1.8 Presets (`src/config/mcpPresets.ts`)

`McpPreset = Omit<McpServer, "enabled"|"description">`. `createNpxCommand(pkg, extraArgs)` emits `{command:"cmd", args:["/c","npx",...extra,pkg]}` on Windows and `{command:"npx", args:[...extra,pkg]}` elsewhere (`:9-24`).

| id | name | server | tags | docs |
|---|---|---|---|---|
| `fetch` | `mcp-server-fetch` | stdio `uvx mcp-server-fetch` | stdio, http, web | `…/servers/tree/main/src/fetch` |
| `time` | `@modelcontextprotocol/server-time` | npx `-y` | stdio, time, utility | `…/src/time` |
| `memory` | `@modelcontextprotocol/server-memory` | npx `-y` | stdio, memory, graph | `…/src/memory` |
| `sequential-thinking` | `@modelcontextprotocol/server-sequential-thinking` | npx `-y` | stdio, thinking, reasoning | `…/src/sequentialthinking` |
| `context7` | `@upstash/context7-mcp` | npx `-y` | stdio, docs, search | context7 README |

`getMcpPresetWithDescription(preset, t)` injects `description = t("mcp.presets.<id>.description")` (`:93-102`). All five description keys and `.name` keys exist in en.json.

---

## 2. Skills

Files: `src/components/skills/{UnifiedSkillsPanel,SkillsPage,SkillCard,RepoManager,RepoManagerPanel}.tsx`, `src/hooks/useSkills.ts`, `src/hooks/useSkills.helpers.ts`, `src/lib/api/skills.ts`, `src/lib/errors/skillErrorParser.ts`.

### 2.1 Types (`src/lib/api/skills.ts:16-134`)

```ts
export interface SkillApps { claude: boolean; "claude-desktop"?: boolean; codex: boolean; gemini: boolean;
  grokbuild?: boolean; opencode: boolean; openclaw: boolean; hermes: boolean }
export interface InstalledSkill { id, name, description?, directory, repoOwner?, repoName?, repoBranch?,
  readmeUrl?, apps: SkillApps, installedAt: number, contentHash?, updatedAt: number }
export interface SkillUninstallResult { backupPath?: string }
export interface SkillBackupEntry { backupId, backupPath, createdAt: number, skill: InstalledSkill }
export interface DiscoverableSkill { key, name, description, directory, readmeUrl?, repoOwner, repoName, repoBranch }
export interface UnmanagedSkill { directory, name, description?, foundIn: string[], path }
export interface ImportSkillSelection { directory: string; apps: SkillApps }
export interface SkillUpdateInfo { id, name, currentHash?, remoteHash }
export interface MigrationResult { migratedCount, skippedCount, errors: string[] }
export interface SkillsShDiscoverableSkill { key, name, directory, repoOwner, repoName, repoBranch, installs: number, readmeUrl? }
export interface SkillsShSearchResult { skills: SkillsShDiscoverableSkill[]; totalCount: number; query: string }
export interface SkillRepo { owner: string; name: string; branch: string; enabled: boolean }
```

### 2.2 Commands

| Method | Command | Args | Result | Line |
|---|---|---|---|---|
| `getInstalled` | `get_installed_skills` | — | `InstalledSkill[]` | :142 |
| `getBackups` | `get_skill_backups` | — | `SkillBackupEntry[]` | :147 |
| `deleteBackup` | `delete_skill_backup` | `{backupId}` | `bool` | :152 |
| `installUnified` | `install_skill_unified` | `{skill: DiscoverableSkill, currentApp: AppId}` | `InstalledSkill` | :160 |
| `uninstallUnified` | `uninstall_skill_unified` | `{id}` | `SkillUninstallResult` | :165 |
| `restoreBackup` | `restore_skill_backup` | `{backupId, currentApp}` | `InstalledSkill` | :170-174 |
| `toggleApp` | `toggle_skill_app` | `{id, app, enabled}` | `bool` | :178 |
| `scanUnmanaged` | `scan_unmanaged_skills` | — | `UnmanagedSkill[]` | :183 |
| `importFromApps` | `import_skills_from_apps` | `{imports: ImportSkillSelection[]}` | `InstalledSkill[]` | :188-191 |
| `discoverAvailable` | `discover_available_skills` | — | `DiscoverableSkill[]` | :195 |
| `checkUpdates` | `check_skill_updates` | — | `SkillUpdateInfo[]` | :200 |
| `updateSkill` | `update_skill` | `{id}` | `InstalledSkill` | :205 |
| `migrateStorage` | `migrate_skill_storage` | `{target:"cc_switch"|"unified"}` | `MigrationResult` | :210-213 |
| `searchSkillsSh` | `search_skills_sh` | `{query, limit, offset}` | `SkillsShSearchResult` | :217-222 |
| `getRepos` | `get_skill_repos` | — | `SkillRepo[]` | :257 |
| `addRepo` | `add_skill_repo` | `{repo: SkillRepo}` | `bool` | :262 |
| `removeRepo` | `remove_skill_repo` | `{owner, name}` | `bool` | :267 |
| `openZipFileDialog` | `open_zip_file_dialog` | — | `string \| null` | :274 |
| `installFromZip` | `install_skills_from_zip` | `{filePath, currentApp}` | `InstalledSkill[]` | :279-283 |

Legacy (unused here): `get_skills`, `get_skills_for_app`, `install_skill(_for_app)`, `uninstall_skill(_for_app)` (`:228-252`).

### 2.3 Query hooks (`src/hooks/useSkills.ts`)

| Hook | Key | Options | Cache writes |
|---|---|---|---|
| `useInstalledSkills` | `["skills","installed"]` | `staleTime: Infinity`, `keepPreviousData` | — |
| `useSkillBackups` | `["skills","backups"]` | **`enabled:false`** (manual `refetch`) | — |
| `useDiscoverableSkills` | `["skills","discoverable"]` | `staleTime: Infinity`, `keepPreviousData` | — |
| `useScanUnmanagedSkills({enabled})` | `["skills","unmanaged"]` | default `enabled:false`, `staleTime: 30 s`, `keepPreviousData` | shared cache: panel scans (enabled:true), header only subscribes for the green dot (`useSkills.ts:188-204`, `App.tsx:259-262`) |
| `useCheckSkillUpdates` | `["skills","updates"]` | `enabled:false`, `staleTime: 5 min` | — |
| `useSearchSkillsSh(q,limit,offset)` | `["skills","skillssh",q,limit,offset]` | `enabled: q.length >= 2`, `staleTime: 5 min`, `keepPreviousData` | — |
| `useSkillRepos` | `["skills","repos"]` | — | — |
| `useInstallSkill` | mutation | — | **appends** to `installed`; flips `installed:true` on the matching `discoverable` entry keyed by `` `${basename(dir).toLower()}:${owner.toLower()}:${name.toLower()}` `` (`:79-108`) |
| `useUninstallSkill` | `{id, skillKey}` | — | filters `installed` by `id`; flips `installed:false` on `discoverable` (`:116-148`) |
| `useRestoreSkillBackup` | `{backupId,currentApp}` | — | invalidates `installed` + `backups` |
| `useDeleteSkillBackup` | `backupId` | — | invalidates `backups` |
| `useToggleSkillApp` | `{id,app,enabled}` | — | invalidates `installed` |
| `useImportSkillsFromApps` | `ImportSkillSelection[]` | — | `mergeImportedSkills` (dedupe by id, imported wins, identity-returned when empty — `useSkills.helpers.ts:10-19`); invalidates `unmanaged` |
| `useInstallSkillsFromZip` | `{filePath,currentApp}` | — | **appends** without dedupe (`:280-289`) |
| `useAddSkillRepo` / `useRemoveSkillRepo` | — | — | invalidate `repos` + `discoverable` |
| `useUpdateSkill` | `id` | — | replaces the entry in `installed`; removes it from `updates` (`:314-331`) |

### 2.4 UnifiedSkillsPanel (view `skills`)

Props: `{onOpenDiscovery, currentApp}` where App passes `sharedFeatureApp === "openclaw" ? "claude" : sharedFeatureApp` (`App.tsx:919-926`).

Header row (`:351-402`): `AppCountBar` (`skills.installed {count}`, `appIds = SKILLS_APP_IDS`) plus, on the right:
- "Update All" button, wrapped in a width/opacity transition (`maxWidth 200px ↔ 0px`) driven by `skillUpdates.length > 0`; label `skills.updateAll {count}` / `skills.updatingAll`; disabled while updating.
- "Check Updates" ghost button — `skills.checkUpdates` / `skills.checkingUpdates`, disabled when fetching or `skills` is empty.

List: loading → `skills.loading`; empty → `Sparkles` + `skills.noInstalled` + `skills.noInstalledDescription`; else bordered list of rows.

Row (`InstalledSkillListItem`, `:492-597`): name; `ExternalLink` when `readmeUrl` (`settingsApi.openExternal`, errors swallowed); source label = `owner/name` or `skills.local`; amber `skills.updateAvailable` badge when an update exists; description line; `AppToggleGroup` over `SKILLS_APP_IDS`; action cluster normally `opacity-0 group-hover:opacity-100` but **forced visible when `hasUpdate`** (`:563-566`). Update button (`RefreshCw`/spinner, title `skills.update`) only when `hasUpdate`; per-row spinner condition is `updateSkillMutation.isPending && updateSkillMutation.variables === skill.id`. Uninstall (`Trash2`, red, title `skills.uninstall`).

Action semantics:

| Action | Behaviour | Line |
|---|---|---|
| toggle app | mutate; error → `toast.error(common.error, {description})` | :136-142 |
| uninstall | ConfirmDialog `skills.uninstall` / `skills.uninstallConfirm {name}`; computes `skillKey = ${basename(directory).toLowerCase()}:${repoOwner?.toLowerCase()||""}:${repoName?.toLowerCase()||""}` (splits on `/` **and** `\`); success toast `skills.uninstallSuccess {name}` with description `skills.backup.location {path}` when `backupPath` is returned | :144-173 |
| `openImport` | `await scanUnmanaged()`; empty → `toast.success(skills.noUnmanagedFound)` and **do not open**; else open the import dialog | :175-186 |
| import submit | mutate; close dialog; `toast.success(skills.importSuccess {count})` | :188-198 |
| `openInstallFromZip` | `skillsApi.openZipFileDialog()`; `null` → silent return; then mutate. `0` → `toast.info(skills.installFromZip.noSkillsFound)`; `1` → `successSingle {name}`; `n` → `successMultiple {count}`; throw → `toast.error(skills.installFailed, {description})` | :200-230 |
| `checkUpdates` | refetch; `0` → `toast.success(skills.noUpdates)`; else `toast.info(skills.updatesFound {count})` | :232-246 |
| update one | `toast.success(skills.updateSuccess {name})` / `toast.error(skills.updateFailed, {description})` | :248-257 |
| update all | **sequential** loop over `skillUpdates`, per-failure error toast `${name}: ${error}`, then one `skills.updateAllSuccess {count}` if any succeeded | :259-279 |
| `openRestoreFromBackup` | open dialog *first*, then `refetchSkillBackups()` (dialog shows `common.loading` meanwhile) | :281-288 |
| restore | mutate `{backupId, currentApp}`; close; `skills.restoreFromBackup.success {name}` / `.failed` | :290-308 |
| delete backup | ConfirmDialog (`deleteConfirmTitle`, `deleteConfirmMessage {name}`, confirmText `skills.restoreFromBackup.delete`, `variant="destructive"`); on success refetch backups + `deleteSuccess {name}` | :310-339 |

`ConfirmDialog` here uses `zIndex="top"` (`:451`).

**RestoreSkillsDialog** (`:623-728`): `max-w-2xl max-h-[85vh]`, `zIndex="alert"`, title `.title`, description `.description`; states loading / `.empty` / cards. Card shows name, `directory` chip, description, `skills.restoreFromBackup.createdAt` + `formatSkillBackupDate(createdAt)` = `new Date(unixSeconds * 1000).toLocaleString()` (**seconds, not ms**; NaN falls back to the raw number, `:60-65`), and `.path` (break-all, `title` = full). Buttons Restore/Delete, both disabled while either op runs. Footer `common.close`.

**ImportSkillsDialog** (`:730-873`): a hand-rolled `fixed inset-0 bg-black/50 z-50` overlay (not Radix). All rows start selected; per-row app map seeded from `foundIn` (`openclaw` hardcoded `false`, `:750-754`). Row = checkbox, name, clamped description, `AppToggleGroup` over `SKILLS_APP_IDS`, truncated `path`. Footer `common.cancel` + `skills.importSelected {count}` (disabled at 0 or while importing).

### 2.5 SkillsPage (view `skillsDiscovery`)

Props `{initialApp = "claude", onSourceChange}`; handle `{refresh, openRepoManager}`.

Header actions table (`SkillsPage.tsx:57-85`):

| key | sources | labelKey | Icon | execute |
|---|---|---|---|---|
| `refresh-repos` | `["repos"]` | `skills.refresh` | `RefreshCw` | `page.refresh()` → `refetchDiscoverable()` + `refetchRepos()` |
| `manage-repos` | `["repos","skillssh"]` | `skills.repoManager` | `Settings` | `page.openRepoManager()` |

`getSkillsPageHeaderActions(source)` filters by `sources.includes(source)`; App re-renders these on every `skillsDiscoverySource` change, which the page pushes up via `onSourceChange(effectiveSource)` (`:365-367`).

Source selection: `searchSource` state, but **`effectiveSource`** = `"skillssh"` when `searchSource==="repos" && repos.length===0 && !loading`, else `searchSource` (`:359-363`). Empty discovery results keep the repos view (so Refresh remains reachable).

Toolbar (`:375-527`): a 2-button segmented control (`skills.searchSource.repos` / literal `"skills.sh"`), then either

- **repos mode**: search input (`skills.searchPlaceholder`, live filter), repo `Select` (`skills.filter.repo`, options `skills.filter.allRepos` + sorted `owner/name` set from discoverable), status `Select` (`skills.filter.placeholder`; `skills.filter.all|installed|uninstalled`), and a `skills.count {count}` line shown only while `searchQuery` is non-empty.
- **skills.sh mode**: input (`skills.skillssh.searchPlaceholder`) with Enter-to-search, and a Search button (`skills.search`) disabled when `trim().length < 2` or fetching.

Filtering (`:323-351`): repo filter → status filter → case-insensitive substring over `name` and `owner/name`.

Installed matching (`:160-201`): installed key = `` `${directory.toLowerCase()}:${owner}:${name}` ``; discoverable key uses `basename(directory)` (split on `/` **or** `\`) — an intentional asymmetry because install flattens to a leaf directory. skills.sh entries use the full `directory` (`:204-207`).

skills.sh pagination: `SKILLSSH_PAGE_SIZE = 20`; results accumulate in `accumulatedResults` (reset when `offset===0`, appended otherwise, skipped while `isPlaceholderData`, `:134-142`). New search resets offset + accumulator and only fires when `trim().length >= 2` and the query actually changed (`:145-152`). "Load More" bumps offset by 20; shown while `accumulatedResults.length < totalCount`. Footer always shows `skills.skillssh.poweredBy`.

Content states: repos → spinner / `skills.empty` + `skills.emptyDescription` + link `skills.addRepo` / `skills.noResults` / 1-2-3-column card grid. skills.sh → spinner + `skills.skillssh.loading` / prompt screen when `skillsShQuery.length < 2` / `skills.skillssh.noResults {query}` / grid + Load More.

Install (`:236-275`): resolves the skill from `accumulatedResults` (skills.sh, converted via `toDiscoverableSkill` with `description: ""`) or `discoverableSkills`; missing → `toast.error(skills.notFound)`. Success → `skills.installSuccess {name}`. Failure → `formatSkillError(message, t, "skills.installFailed")` → `toast.error(title, {description, duration: 10000})`.

Uninstall from this page is **not supported**: `toast.info(skills.uninstallInMainPanel)` (`:277-280`).

Repos (`:282-320`): add → mutate, then `await refetchDiscoverable()` and count the entries matching `owner/name/(branch||"main")` to report `skills.repo.addSuccess {owner,name,count}`; remove → `skills.repo.removeSuccess {owner,name}`; both errors → `toast.error(common.error, {description: String(error)})`.

### 2.6 SkillCard (`src/components/skills/SkillCard.tsx`)

Props `{skill: DiscoverableSkill & {installed}, onInstall(key), onUninstall(key), installs?}`. Local `loading` wraps both callbacks in try/finally.

Header: title; `directory` sub-line **only when** `directory.trim().toLowerCase() !== name.trim().toLowerCase()` (`:63-65`); `owner/name` outline badge; `installs.toLocaleString()` badge with `Download` icon when `installs` is a number; green `skills.installed` badge when installed.
Body: description clamped to 4 lines, else a flex spacer.
Footer: `skills.view` (only with `readmeUrl`) → `settingsApi.openExternal`; then either red-outlined Uninstall (`skills.uninstall`/`skills.uninstalling`) or `variant="mcp"` Install (`skills.install`/`skills.installing`) **disabled when `!skill.repoOwner`**.

### 2.7 Repo managers

Two near-identical components; `SkillsPage` mounts **`RepoManagerPanel`** (FullScreenPanel). `RepoManager` (Radix dialog variant, `max-w-2xl max-h-[80vh]`) is currently unreferenced — port one component with a dialog/panel switch.

Shared logic:
- `parseRepoUrl` strips `https?://github.com/` prefix and a trailing `.git`, then requires exactly two non-empty `/`-separated parts (`RepoManagerPanel.tsx:39-52`).
- Add → `onAdd({owner, name, branch: branch || "main", enabled: true})`; on parse failure show `skills.repo.invalidUrl` inline; on throw show `e.message` or `skills.repo.addFailed`. Inputs clear only on success.
- `getSkillCount(repo)` counts discoverable skills where owner, name and `(repoBranch||"main")` all match.
- Row: `owner/name`, `skills.repo.branch: <branch|main>`, pill `skills.repo.skillCount {count}`, ExternalLink → `https://github.com/{owner}/{name}`, Trash2 → `onRemove` (no confirmation).
- Fields: `skills.repo.url` / `.urlPlaceholder`, `skills.repo.branch` / `.branchPlaceholder`, button `skills.repo.add`; list header `skills.repo.list`, empty `skills.repo.empty`; panel title `skills.repo.title` (dialog variant also shows `skills.repo.description`).

### 2.8 Skill error parser (`src/lib/errors/skillErrorParser.ts`)

`parseSkillError(str)` → `JSON.parse`; accepted only when both `code` and `context` exist, else `null`.

| code | i18n key |
|---|---|
| `SKILL_NOT_FOUND` | `skills.error.skillNotFound` |
| `MISSING_REPO_INFO` | `skills.error.missingRepoInfo` |
| `DOWNLOAD_TIMEOUT` | `skills.error.downloadTimeout` |
| `DOWNLOAD_FAILED` | `skills.error.downloadFailed` |
| `SKILL_DIR_NOT_FOUND` | `skills.error.skillDirNotFound` |
| `SKILL_DIRECTORY_CONFLICT` | `skills.error.directoryConflict` |
| `EMPTY_ARCHIVE` | `skills.error.emptyArchive` |
| `GET_HOME_DIR_FAILED` | `skills.error.getHomeDirFailed` |
| `NO_SKILLS_IN_ZIP` | `skills.error.noSkillsInZip` |
| *(fallback)* | `skills.error.unknownError` |

| suggestion | i18n key |
|---|---|
| `checkNetwork` / `checkProxy` / `retryLater` / `checkRepoUrl` / `checkPermission` / `uninstallFirst` / `checkZipContent` | `skills.error.suggestion.<same>` |
| `http403` / `http404` / `http429` | `skills.error.http403/404/429` |
| *(fallback)* | the suggestion string itself, passed to `t()` |

`formatSkillError(str, t, defaultTitle="skills.installFailed")` → `{title: t(defaultTitle), description}`. Non-JSON input yields the raw string (or `common.error`). With a suggestion, description becomes `` `${t(errorKey, context)}\n\n${t(suggestionKey)}` `` (`:74-108`). The `context` object is passed straight to `t()` as interpolation values.

### 2.9 Migration result banner

There is no dedicated skills-migration banner component; it is the `get_skills_migration_result` toast pair described in §0.5. The storage-location migration (`migrate_skill_storage` → `MigrationResult {migratedCount, skippedCount, errors}`) is driven from the Settings page, not from these views.

---

## 3. Prompts

Files: `src/components/prompts/*`, `src/hooks/usePromptActions.ts`, `src/lib/api/prompts.ts`.

### 3.1 Type & commands (`src/lib/api/prompts.ts`)

```ts
export interface Prompt { id: string; name: string; content: string; description?: string;
  enabled: boolean; createdAt?: number; updatedAt?: number }
```

| Method | Command | Args | Result |
|---|---|---|---|
| `getPrompts` | `get_prompts` | `{app}` | `Record<string, Prompt>` |
| `upsertPrompt` | `upsert_prompt` | `{app, id, prompt}` | `void` |
| `deletePrompt` | `delete_prompt` | `{app, id}` | `void` |
| `enablePrompt` | `enable_prompt` | `{app, id}` | `void` |
| `importFromFile` | `import_prompt_from_file` | `{app}` | `string` (new id) |
| `getCurrentFileContent` | `get_current_prompt_file_content` | `{app}` | `string \| null` |

### 3.2 `usePromptActions(appId)` — plain `useState`, **not** React Query

| Fn | Behaviour |
|---|---|
| `reload` | `getPrompts(appId)` → state; then `getCurrentFileContent` into `currentFileContent`, swallowing errors to `null`. Failure of the first → `toast.error(prompts.loadFailed)` |
| `savePrompt(id, p)` | upsert → `reload()` → `toast.success(prompts.saveSuccess, {closeButton})`; on error toast `prompts.saveFailed` and **rethrow** |
| `deletePrompt(id)` | delete → reload → `prompts.deleteSuccess` / `prompts.deleteFailed` + rethrow |
| `enablePrompt(id)` | enable → reload → `prompts.enableSuccess` / `prompts.enableFailed` + rethrow |
| `toggleEnabled(id, enabled)` | **optimistic**: enabling rewrites every prompt's `enabled` to `key === id` (radio semantics); disabling flips only that entry. Then `enable_prompt` (toast `prompts.enableSuccess`) or `upsert_prompt` with `enabled:false` (toast `prompts.disableSuccess`), then `reload()`. On failure restores the pre-mutation map, toasts `prompts.enableFailed`/`prompts.disableFailed` and rethrows (`:76-127`) |
| `importFromFile()` | `import_prompt_from_file` → reload → `prompts.importSuccess`, returns id; error `prompts.importFailed` + rethrow |

`currentFileContent` and `importFromFile` are loaded/exposed but **no current view renders them** (`prompts.currentFile` / `prompts.import` keys are unused).

### 3.3 PromptPanel (view `prompts`)

Props `{open, onOpenChange, appId}` (`onOpenChange` is destructured away; App closes via the header back button). Handle: `openAdd()`.

Effects: `reload()` whenever `open` flips true (`:43-45`); `window.addEventListener("prompt-imported")` → `reload()` when `detail.app === appId` (the deep-link bridge, `:48-61`); `useTauriEvent("profile-applied", reload)` because prompts are not in React Query (`:64`).

Layout (`:102-165`): glass summary bar showing `prompts.count {count}` + `·` + either `prompts.enabledName {name}` (first prompt with `enabled`) or `prompts.noneEnabled`. Then loading `prompts.loading` / empty (`FileText` + `prompts.empty` + `prompts.emptyDescription`) / `space-y-3` list of `PromptListItem`.

`PromptListItem` (`:16-71`): fixed `h-16` card; left `PromptToggle`; name + truncated description; Edit (`Edit3`, `common.edit`) and Delete (`Trash2`, red hover, `common.delete`).
`PromptToggle`: `role="switch"`, `aria-checked`, 44×24 track, emerald when on / gray when off, knob translate `x-6`/`x-1`, `disabled` → `opacity-50 cursor-not-allowed`.

Delete → `ConfirmDialog` with `prompts.confirm.deleteTitle` / `prompts.confirm.deleteMessage {name}`; the confirm handler swallows the hook's rethrow and leaves the dialog open on failure (`:80-96`).

### 3.4 PromptFormPanel (the one actually used)

`FullScreenPanel` titled `prompts.editTitle {appName}` / `prompts.addTitle {appName}` where `appName = t("apps."+appId)`. Footer: single Save button, disabled when `!name.trim() || saving`, label `common.save`/`common.saving`.

Filename map for the content placeholder (`PromptFormPanel.tsx:27-37`): claude & claude-desktop → `CLAUDE.md`; gemini → `GEMINI.md`; codex, grokbuild, opencode, openclaw, hermes → `AGENTS.md`.

Fields: `prompts.name` (`.namePlaceholder`), `prompts.description` (`.descriptionPlaceholder`), `prompts.content` with `MarkdownEditor{value, onChange, placeholder: t("prompts.contentPlaceholder",{filename}), darkMode, minHeight:"167px"}`.

`MarkdownEditor` contract (`src/components/MarkdownEditor.tsx:8-17`): `{value, onChange?, placeholder?, darkMode?, readOnly?, className?, minHeight="300px", maxHeight?}` — CodeMirror 6 with `markdown()`, `oneDark` when dark, line wrapping, mono 14 px, `12px 0` content padding, transparent background, no focus outline.

Save (`:67-92`): guard `!name.trim()` → silent return. `id = editingId || "prompt-" + Date.now()`; `timestamp = Math.floor(Date.now()/1000)` (**seconds**); `enabled: initialData?.enabled || false`; `createdAt: initialData?.createdAt || timestamp`; `updatedAt: timestamp`; `content` and `name` trimmed, `description` trimmed-or-`undefined`. Then `await onSave(id, prompt)` and `onClose()`; errors are swallowed (hook already toasted) but `onClose` is then skipped.

`PromptFormModal` is the Radix-dialog twin (`max-w-2xl max-h-[85vh]`, `minHeight="300px"`, extra `common.cancel` button, filename map typed `Record<Exclude<AppId,"openclaw">,string>` so `openclaw` yields `undefined`) and is **currently unreferenced**.

---

## 4. Sessions

Files: `src/components/sessions/{SessionManagerPage,SessionItem,SessionMessageItem,SessionToc,utils}.ts(x)`, `src/hooks/useSessionSearch.ts`, `src/lib/api/sessions.ts`, `src/lib/query/{queries,mutations}.ts`.

### 4.1 Types & commands

```ts
// src/types.ts:454-470
export interface SessionMeta { providerId: string; sessionId: string; title?: string; summary?: string;
  projectDir?: string | null; createdAt?: number; lastActiveAt?: number; sourcePath?: string; resumeCommand?: string }
export interface SessionMessage { role: string; content: string; ts?: number }
// src/lib/api/sessions.ts:4-13
export interface DeleteSessionOptions { providerId: string; sessionId: string; sourcePath: string }
export interface DeleteSessionResult extends DeleteSessionOptions { success: boolean; error?: string }
```

| Method | Command | Args | Result |
|---|---|---|---|
| `list` | `list_sessions` | — | `SessionMeta[]` |
| `getMessages` | `get_session_messages` | `{providerId, sourcePath}` | `SessionMessage[]` |
| `delete` | `delete_session` | `{providerId, sessionId, sourcePath}` | `bool` |
| `deleteMany` | `delete_sessions` | `{items: DeleteSessionOptions[]}` | `DeleteSessionResult[]` |
| `launchTerminal` | `launch_session_terminal` | `{command, cwd, customConfig}` | `bool` |

Queries (`src/lib/query/queries.ts:307-325`): `["sessions"]` staleTime 30 s; `["sessionMessages", providerId, sourcePath]` staleTime 30 s, `enabled: Boolean(providerId && sourcePath)`.

`useDeleteSessionMutation` (`src/lib/query/mutations.ts:339-380`): on success removes the row from `["sessions"]` cache (matching all three fields), `removeQueries(["sessionMessages", providerId, sourcePath])`, invalidates `["sessions"]`, toasts `sessionManager.sessionDeleted`. On error toasts `sessionManager.deleteFailed {error}`.

### 4.2 Search (`src/hooks/useSessionSearch.ts`)

- Provider prefilter first (`providerFilter === "all"` passes everything).
- Index: `new FlexSearch.Index({tokenize:"full", resolution:9})`, one document per **array index**, content = `[sessionId, title, summary, projectDir, sourcePath].filter(Boolean).join(" ")` — **message bodies are not indexed**.
- Empty query → prefiltered list sorted by `lastActiveAt ?? createdAt ?? 0` descending.
- Non-empty → `index.search(needle, {limit: filteredByProvider.length})` mapped back through the index array; **search results are in FlexSearch relevance order, not time order**.
- Index is rebuilt (`useMemo`) whenever the prefiltered list changes.

### 4.3 Page state (`SessionManagerPage.tsx:189-228`)

| State | Init |
|---|---|
| `search` | `""` |
| `providerFilter` | `appId as ProviderFilter` (union: all, codex, grokbuild, claude, opencode, openclaw, gemini, hermes — `:83-91`) |
| `selectedKey` | `null` |
| `listViewMode` | `localStorage["cc-switch.sessionManager.listViewMode"]` ∈ {flat, grouped}, default `flat` (`:107-113`) |
| `expandedProviderGroups` / `expandedDirectoryGroups` | `localStorage["cc-switch.sessionManager.groupExpansionState"]` = `{expandedProviderIds: string[], expandedDirectoryKeys: string[]}`; malformed JSON → empty sets (`:115-160`) |
| `selectionMode`, `selectedSessionKeys`, `isBatchDeleting` | `false` / empty Set / `false` |
| `isSearchOpen`, `tocDialogOpen`, `activeMessageIndex`, `deleteTargets` | `false` / `false` / `null` / `null` |

Persistence effects: view mode on change (`:262-267`); expansion state serialised with **sorted** arrays (`:162-169`, `:269-277`).

Reconciliation effects:
- After loading, prune expansion sets to provider ids / directory keys that still exist (`filterSetToAllowedValues` returns the same reference when nothing changed, to avoid re-render loops — `:171-187`, `:279-288`).
- Prune `selectedSessionKeys` against all session keys (`:345-361`) and, while in selection mode, against *visible deletable* sessions (`:565-586`).
- Auto-select: empty list → `selectedKey = null`; otherwise if the current key is absent select the first filtered session (`:290-303`).
- Reset detail scroll to 0 on `selectedKey` change (`:339-343`).

Note `App.tsx:955-961` mounts the page with `key={sharedFeatureApp}`, so switching apps remounts it and resets all of the above except localStorage.

### 4.4 Virtualization

`useVirtualizer({count: messages.length, getScrollElement: () => scrollContainerRef.current, estimateSize: () => 120, overscan: 5, gap: 12})` (`:331-337`). Rows are absolutely positioned with `transform: translateY(start)` inside a container sized to `getTotalSize()`, each carrying `data-index` and `ref={virtualizer.measureElement}` for dynamic measurement (`:1649-1680`).

The **left session list is not virtualized** — it is a plain `ScrollArea` with a `space-y-1` (flat) or nested `Collapsible` (grouped) tree.

### 4.5 Left column (`:800-1400`)

Header has two mutually exclusive modes.

*Search open* (`:802-862`): full-width input (`sessionManager.searchPlaceholder`), `autoFocus`; `Escape` closes and clears; `onBlur` closes only when the query is empty; inline `X` clears+closes. When `selectionMode` is on, a highlighted `CheckSquare` button (`sessionManager.exitBatchModeTooltip`) remains.

*Normal* (`:864-1217`): title `sessionManager.sessionList` + count badge (`filteredSessions.length`), then icon buttons:

| Control | Detail |
|---|---|
| Batch toggle | Shown when `selectionMode || deletableFilteredSessions.length > 0`; toggles selection mode; exiting clears the selection. Tooltips `sessionManager.manageBatchTooltip` / `.exitBatchModeTooltip` |
| View mode Select | Trigger is a 28 px icon (`ListTree`/`List`), `aria-label` + `sr-only` `sessionManager.viewModeTooltip`; items `sessionManager.viewModeFlat` / `.viewModeGrouped`; tooltip shows the current label |
| Collapse all | Grouped mode only; `ChevronsDownUp`; clears both expansion sets (`sessionManager.collapseAllGroups`) |
| Search | Opens the search row and focuses on the next tick (`setTimeout(…,0)`); tooltip `sessionManager.searchSessions` |
| Provider Select | Trigger shows `ProviderIcon` (`"apps"` for `all`, else `getProviderIconName`); options All (`sessionManager.providerFilterAll`), Codex, Grok Build, Claude Code, OpenCode, OpenClaw, Gemini CLI. **`hermes` is in the type union but has no option** (`:1058-1131`) |
| Refresh | `refetch()`, tooltip `common.refresh` |

Selection sub-bar (`:1149-1216`, only in selection mode): badge `sessionManager.selectedCount {count}` (counts *deletable* selected), hint `sessionManager.batchModeHint`; buttons `sessionManager.selectAllFiltered` / `.clearFilteredSelection` (toggles over `deletableFilteredSessions`), `.clearSelection` (empties the set), and a destructive `.deleteSelected` / `.batchDeleting` button disabled while deleting or with nothing selected.

List body: spinner while `isLoading`; empty → `MessageSquare` + `sessionManager.noSessions`.

**Grouped mode** (`:1234-1389`) uses `groupSessionsByProviderAndDirectory(filteredSessions, t("sessionManager.unknownDirectory"))` (`utils.ts:157-208`) — insertion-ordered provider groups, each with insertion-ordered directory groups keyed `` `${providerId}:${trimmedDir || "__unknown_project_dir__"}` `` and labelled with the directory basename (falling back to the full path, then to the unknown label). Provider header: optional checkbox, chevron, `ProviderIcon`, label `getProviderLabel` (= `t("apps."+id)` with raw-id fallback), badge = `selected/selectable` in selection mode else total. Directory header: same pattern, `FolderOpen`, tooltip with the full `projectDir`. Group checkbox state: `false` at 0 selected, `true` when all selectable are selected, `"indeterminate"` otherwise; disabled when `selectableCount === 0` (`:594-614`); clicks `stopPropagation()` so they don't toggle the collapsible.

`SessionItem` (`SessionItem.tsx`): selected = `bg-primary/10 border-primary/30`; optional checkbox (`sessionManager.selectForBatch`, disabled when no `sourcePath`); `ProviderIcon` with a `getProviderLabel` tooltip; title = `formatSessionTitle` with `highlightText(title, searchQuery)` when searching; `ChevronRight` rotates 90° and turns primary when selected; footer `Clock` + `formatRelativeTime(lastActiveAt || createdAt)` or `common.unknown`.

### 4.6 Right column (`:1403-1702`)

No selection → `MessageSquare` + `sessionManager.selectSession`.

Header (`:1415-1613`): provider icon w/ tooltip; `formatSessionTitle`; meta row = `Clock` + `formatTimestamp(lastActiveAt ?? createdAt)`; click-to-copy `projectDir` (basename shown, tooltip shows full path + `sessionManager.clickToCopyPath`, toast `sessionManager.projectDirCopied`) and `sourcePath` (toast `.sourcePathCopied`).

Actions: **Resume button renders only on macOS** (`isMac()`, `:1521`) — label `sessionManager.resume`, disabled without `resumeCommand`, tooltip `.resumeTooltip` / `.noResumeCommand`. Destructive Delete (`.delete` / `.deleting`, tooltip `.deleteTooltip`) disabled without `sourcePath` or while deleting.

Resume-command strip (`:1584-1612`): mono truncated command + copy button (`sessionManager.copyCommand`, toast `.resumeCommandCopied`).

Messages pane (`:1616-1699`): header `MessageSquare` + `sessionManager.conversationHistory` + count badge; then spinner / `sessionManager.emptySession` / the virtualized list; then the `SessionTocSidebar`; then the floating `SessionTocDialog`.

**Resume behaviour** (`:418-440`):
1. No `resumeCommand` → no-op.
2. **Non-macOS** → copy the command, toast `sessionManager.resumeCommandCopied`, return (never launches a terminal).
3. macOS → `sessionsApi.launchTerminal({command, cwd: projectDir ?? undefined})` → `toast.success(sessionManager.terminalLaunched)`.
4. On throw → copy the command with toast `sessionManager.resumeFallbackCopied` **and** `toast.error(extractErrorMessage(error) || sessionManager.openFailed)` (two toasts).

**Copy** (`:393-406`): `navigator.clipboard.writeText`, success toast with the supplied message, failure `toast.error(extractErrorMessage(error) || t("common.error"))`.

**Delete** (`:442-545`):
- Filters `deleteTargets` to those with `sourcePath`, closes the dialog immediately.
- 1 target → `deleteSessionMutation.mutateAsync(...)` (toasts handled by the mutation) and removes the key from the selection.
- n targets → `isBatchDeleting = true`; `sessionsApi.deleteMany(...)`; successes produce keys `` `${providerId}:${sessionId}:${sourcePath ?? ""}` `` which are removed from the `["sessions"]` cache; `removeQueries(["sessionMessages", providerId, sourcePath])` per success; selection pruned; `invalidateQueries(["sessions"])`. Then `toast.success(sessionManager.batchDeleteSuccess {count})` when any succeeded and `toast.error(sessionManager.batchDeleteFailed {failed}, {description: firstError})` when any failed (both can fire). A thrown request → `toast.error(extractErrorMessage(error) || sessionManager.batchDeleteRequestFailed)`.
- `ConfirmDialog` (`:1706-1750`) switches title/message/confirm text on `length > 1`: `.batchDeleteConfirmTitle/.batchDeleteConfirmMessage{count}/.batchDeleteConfirmAction` vs `.deleteConfirmTitle/.deleteConfirmMessage{title,sessionId}/.deleteConfirmAction`; `variant="destructive"`; cancel is ignored while deleting.

### 4.7 TOC (`SessionToc.tsx`)

Both sidebar and dialog render **nothing when `items.length <= 2`** (`:31`, `:80`).

TOC items (`SessionManagerPage.tsx:366-384`): user-role messages only; for Codex sessions, rows where `shouldHideCodexMessageFromToc(content)` are dropped and the remainder previewed via `extractCodexPromptPreview`. Preview = `formatSessionMessagePreview(content, 50)` → first 50 chars + `"..."` when longer.

Codex heuristics (`utils.ts:22-67`, `:210-222`):
- Hide when the trimmed content starts with `"# AGENTS.md instructions for "`, `"<environment_context>"`, or starts with `"# Context from my IDE setup:"` **and** no prompt can be extracted.
- `extractCodexPromptFromIdeContext` scans every line for a heading matching `my request for codex` (case-insensitive, after stripping `#`s); an inline suffix must begin with `:`, `：`, `-` or `—` and is cleaned of leading separators; a bare heading takes everything after it. **The last match wins** (VS Code injects the real prompt as the final section).

Sidebar: `w-64 border-l hidden xl:block`, header `List` + `sessionManager.tocTitle`, numbered chips (1-based over the filtered TOC), 2-line clamp.
Dialog: FAB `fixed bottom-20 right-4 xl:hidden` (`size-10 rounded-full z-30`); content `max-w-md max-h-[70vh]`, `zIndex="alert"`, explicit `onInteractOutside`/`onEscapeKeyDown` closing, close button labelled `common.close`.

`scrollToMessage(index)` (`:386-391`): `virtualizer.scrollToIndex(index, {align:"center", behavior:"smooth"})`, sets `activeMessageIndex`, closes the TOC dialog, and clears the highlight after **2000 ms**.

### 4.8 SessionMessageItem

`memo`ised. Constants `COLLAPSE_THRESHOLD = 3000`, `COLLAPSED_LENGTH = 1500` (`:20-21`).

Collapse logic (`:39-48`): `isLong = content.length > 3000`; `hasSearchMatch = isLong && !expanded && searchQuery && content.toLowerCase().includes(query.toLowerCase())` — a search hit **force-expands** the message and hides the expand/collapse control; otherwise collapsed content is `slice(0,1500) + "…"`.

Styling by role (lowercased): `user` → `bg-primary/5 border-primary/20 ml-8`; `assistant` → `bg-blue-500/5 border-blue-500/20 mr-8`; other → `bg-muted/40`. `isActive` adds `ring-2 ring-primary ring-offset-2`.
Role label (`utils.ts:140-147`): assistant → literal `"AI"`; user/system/tool → `sessionManager.roleUser/.roleSystem/.roleTool`; anything else → the raw role. Tone colours: assistant blue-500, user emerald-500, system amber-500, tool purple-500, else muted.
Copy button top-right, `opacity-0 group-hover:opacity-100`, tooltip `sessionManager.copyMessage`, toast `sessionManager.messageCopied`.
Expand control: `sessionManager.expandContent` + `(Nk)` where `N = Math.round(length/1000)`, or `sessionManager.collapseContent`; `aria-expanded` set.

### 4.9 Shared helpers (`utils.ts`)

| Fn | Behaviour |
|---|---|
| `getSessionKey` | `` `${providerId}:${sessionId}:${sourcePath ?? ""}` `` |
| `getSessionDirectoryGroupKey` | `` `${providerId}:${trimmedDir || "__unknown_project_dir__"}` `` |
| `getBaseName` | trims, strips trailing separators, splits on `/` or `\`, returns the last non-empty part (or the trimmed input) |
| `formatTimestamp` | `new Date(ms).toLocaleString()`, `""` for falsy |
| `formatRelativeTime` | `<1 min` → `.justNow`; `<60 min` → `.minutesAgo{count}`; `<24 h` → `.hoursAgo{count}`; `<7 d` → `.daysAgo{count}`; else `toLocaleDateString()` |
| `getProviderLabel` | `t("apps."+id)`, falling back to the raw id when the key is missing |
| `getProviderIconName` | codex→`openai`, grokbuild→`grok`, claude→`claude`, opencode→`opencode`, openclaw→`openclaw`, else identity |
| `formatSessionTitle` | `title || basename(projectDir) || sessionId.slice(0,8)` |
| `highlightText` | escapes regex metachars, splits on a capturing case-insensitive group, wraps odd indices in `<mark class="bg-yellow-200/60 dark:bg-yellow-500/30 …">` |

---

## 5. Workspace (OpenClaw)

Files: `src/components/workspace/{WorkspaceFilesPanel,WorkspaceFileEditor,DailyMemoryPanel}.tsx`, `src/lib/api/workspace.ts`.

### 5.1 API

```ts
export interface DailyMemoryFileInfo { filename: string; date: string; sizeBytes: number; modifiedAt: number; preview: string }
export interface DailyMemorySearchResult { filename: string; date: string; sizeBytes: number; modifiedAt: number; snippet: string; matchCount: number }
```

| Method | Command | Args | Result |
|---|---|---|---|
| `readFile` | `read_workspace_file` | `{filename}` | `string \| null` |
| `writeFile` | `write_workspace_file` | `{filename, content}` | `void` |
| `listDailyMemoryFiles` | `list_daily_memory_files` | — | `DailyMemoryFileInfo[]` |
| `readDailyMemoryFile` | `read_daily_memory_file` | `{filename}` | `string \| null` |
| `writeDailyMemoryFile` | `write_daily_memory_file` | `{filename, content}` | `void` |
| `deleteDailyMemoryFile` | `delete_daily_memory_file` | `{filename}` | `void` |
| `searchDailyMemoryFiles` | `search_daily_memory_files` | `{query}` | `DailyMemorySearchResult[]` |
| `openDirectory` | `open_workspace_directory` | `{subdir: "workspace" \| "memory"}` | `void` |

No React Query anywhere in this area — plain `useState` + imperative loads.

### 5.2 WorkspaceFilesPanel

Fixed file list (`:30-52`), order matters:

| filename | icon | descKey |
|---|---|---|
| AGENTS.md | FileCode | `workspace.files.agents` |
| SOUL.md | Heart | `workspace.files.soul` |
| USER.md | User | `workspace.files.user` |
| IDENTITY.md | IdCard | `workspace.files.identity` |
| TOOLS.md | Wrench | `workspace.files.tools` |
| MEMORY.md | Brain | `workspace.files.memory` |
| HEARTBEAT.md | Activity | `workspace.files.heartbeat` |
| BOOTSTRAP.md | Rocket | `workspace.files.bootstrap` |
| BOOT.md | Power | `workspace.files.boot` |

Existence probe: `Promise.all` of `readFile` per file, `exists = content !== null`, throws → `false` (`:60-73`). Runs on mount and again after the editor closes (`:79-83`) — **note this reads every file's full contents just to test existence**.

Layout: clickable path caption `~/.openclaw/workspace/` + `FolderOpen` → `openDirectory("workspace")`, `title = workspace.openDirectory`; then a 1/2-column grid of cards, each with icon, filename, `CheckCircle2` (emerald) when present else hollow `Circle`, and the description. Last grid cell is the Daily Memory card (`Calendar`, `workspace.dailyMemory.cardTitle` / `.cardDescription`, trailing `ChevronRight`).

### 5.3 WorkspaceFileEditor

`FullScreenPanel` titled `workspace.editing {filename}`; footer Save (`common.save`/`common.saving`), disabled while saving or loading. Loads on `(isOpen, filename)` change; read failure → `toast.error(workspace.loadFailed)`. Save → `toast.success(workspace.saveSuccess)` / `toast.error(workspace.saveFailed)`. Loading placeholder reuses `prompts.loading`. Editor: `MarkdownEditor` with `placeholder = "# {filename}\n\n..."` and `minHeight="calc(100vh - 240px)"`.

### 5.4 DailyMemoryPanel

Two modes in one component, switched by `editingFile`.

**List mode** (`:344-537`): path caption `~/.openclaw/workspace/memory/` → `openDirectory("memory")`; search toggle button (`title = workspace.dailyMemory.searchScopeHint`); `workspace.dailyMemory.createToday` button.

Search: animated (framer-motion height/opacity, 150 ms) row with input (`.searchPlaceholder`), inline clear `X`, and a close button labelled `.searchCloseHint`. **300 ms debounce** on every keystroke (`:115-126`); empty query clears results without calling the backend. Errors → `toast.error(workspace.dailyMemory.searchFailed)`.

Keyboard (`:149-169`, active only while `isOpen && !editingFile`): `Cmd/Ctrl+F` opens search or refocuses the input; `Escape` closes search when open. Note `FullScreenPanel`'s own Escape handler also fires — the input is a text-editable target so panel-close is suppressed while focused, but Escape elsewhere on the page closes the whole panel.

Content: when `isActiveSearch` (`isSearchOpen && searchTerm.trim().length > 0`) show `.searching` / dashed-border empty state `.noSearchResults` / result cards (date, `formatFileSize`, primary pill `.matchCount {count}` when `> 0`, 2-line `snippet` with `whitespace-pre-line`). Otherwise show `prompts.loading` / `Calendar` + `.empty` / file cards (date, size, 2-line `preview`).

Every card has a hover-revealed `Trash2` that `stopPropagation()`s and sets `deletingFile`.

`formatFileSize` (`:36-40`): `< 1024` → `N B`; `< 1 MiB` → `X.X KB`; else `X.X MB`.
`getTodayFilename` (`:28-34`): local `YYYY-MM-DD.md`, zero-padded.

**Create today** (`:220-232`): if the filename is already in the list, open it; otherwise enter edit mode with empty content — **no file is created until Save**.

**Edit mode** (`:302-342`): `FullScreenPanel` titled `workspace.editing {filename}`, whose close action is `handleBackToList` (not panel close) — it clears content, reloads the list, and re-runs the active search. Save → `writeDailyMemoryFile` with `workspace.saveSuccess`/`workspace.saveFailed`.

**Delete** (`:250-278`): `ConfirmDialog` `.confirmDeleteTitle` / `.confirmDeleteMessage {date}` where `date = filename.replace(".md","")`. On success toast `.deleteSuccess`, clear target, leave edit mode if it was the open file, reload, re-run search. Failure → `.deleteFailed` and clear the target anyway. The dialog is rendered in **both** modes (`:331-339`, `:539-547`).

**Close panel** (`:292-300`) clears edit + all search state before calling `onClose`.

---

## 6. Universal providers

Files: `src/components/universal/{UniversalProviderPanel,UniversalProviderCard,UniversalProviderFormModal}.tsx`, `src/config/universalProviderPresets.ts`, `src/lib/api/providers.ts:206-243`.

### 6.1 Types (`src/types.ts:537-588`)

```ts
export interface UniversalProviderApps { claude: boolean; codex: boolean; gemini: boolean }
export interface ClaudeModelConfig { model?, haikuModel?, sonnetModel?, opusModel? }
export interface CodexModelConfig { model?, reasoningEffort? }
export interface GeminiModelConfig { model? }
export interface UniversalProviderModels { claude?: ClaudeModelConfig; codex?: CodexModelConfig; gemini?: GeminiModelConfig }
export interface UniversalProvider { id, name, providerType, apps, baseUrl, apiKey, models,
  websiteUrl?, notes?, icon?, iconColor?, meta?: ProviderMeta, createdAt?: number, sortIndex?: number }
export type UniversalProvidersMap = Record<string, UniversalProvider>;
```

### 6.2 Commands (`universalProvidersApi`)

| Method | Command | Args | Result |
|---|---|---|---|
| `getAll` | `get_universal_providers` | — | `UniversalProvidersMap` |
| `get` | `get_universal_provider` | `{id}` | `UniversalProvider \| null` |
| `upsert` | `upsert_universal_provider` | `{provider}` | `bool` |
| `delete` | `delete_universal_provider` | `{id}` | `bool` |
| `sync` | `sync_universal_provider` | `{id}` | `bool` |

Side-channel: the backend emits `universal-provider-synced`, which App handles by invalidating `["providers"]` and refreshing the tray (`App.tsx:381-388`).

### 6.3 Panel (view `universal`)

`App.tsx:948-953` wraps it in `<div className="px-6 pt-4">`. Local `useState`, no React Query; `loadProviders()` on mount.

Layout (`:220-318`): `Layers` icon + `universalProvider.title` + count pill; `universalProvider.description` paragraph; spinner / empty state (`Layers`, `universalProvider.empty`, `universalProvider.emptyHint`) / responsive card grid (1/2/3 columns).

| Action | Behaviour |
|---|---|
| load | `getAll()`; failure → `toast.error(universalProvider.loadError)` |
| save (`onSave`) | `upsert(provider)`; **when adding** also `sync(provider.id)`; toast `universalProvider.updated` (edit) or `.addedAndSynced` (add); reload; clear `editingProvider`. Failure → `.saveError` |
| save & sync | `upsert` then `sync`; toast `.savedAndSynced`; failure `.saveAndSyncError` |
| delete | Confirm `.deleteConfirmTitle` / `.deleteConfirmDescription {name}`, confirmText `common.delete`; then `delete(id)`, toast `.deleted` / `.deleteError`, reload, always clear the confirm state |
| sync | Confirm `.syncConfirmTitle` / `.syncConfirmDescription {name}`, confirmText `universalProvider.syncConfirm`; then `sync(id)`, toast `.synced` / `.syncError`. **Does not reload** |
| duplicate | `deepClone` + `crypto.randomUUID()` + `name + " copy"` + `createdAt = Date.now()`; `upsert` then `sync`; toast `.duplicatedAndSynced` / `.duplicateError`; reload |

**Edge case worth flagging:** `isFormOpen` is set to `true` only by `handleEdit` (`:200-203`). The standalone `universal` view therefore has **no way to create a provider** — the create path lives in `AddProviderDialog` (`src/components/providers/AddProviderDialog.tsx:345-351, 388-410`), which embeds the same panel under a "Universal" tab and supplies its own `universalProvider.add` footer button plus a separate `UniversalProviderFormModal` in add mode. Reproduce this deliberately or fix it in the port.

### 6.4 Card (`UniversalProviderCard.tsx`)

`ProviderIcon` in a 40 px rounded tile; name; `providerType` sub-line; hover-revealed actions Sync (`RefreshCw`, `universalProvider.sync`), Duplicate (`Copy`, `.duplicate`), Edit (`Edit2`, `common.edit`), Delete (`Trash2`, destructive, `common.delete`). Body: `Globe` + `baseUrl || "-"`; pills for each enabled app (literal labels `"Claude"`, `"Codex"`, `"Gemini"`) or `universalProvider.noAppsEnabled`; 2-line clamped `notes`.

### 6.5 Form modal (`UniversalProviderFormModal.tsx`)

`FullScreenPanel` titled `universalProvider.edit` / `.add`. Dark mode via `useDarkMode()` (shared `MutationObserver` hook, `src/hooks/useDarkMode.ts:49-68`).

Init effect (deps `[editingProvider, initialPreset, isOpen]`, `:66-98`): edit → copy all fields and match the preset by `providerType`; add → use `initialPreset ?? universalProviderPresets[0]`, seed name/websiteUrl/apps/models from it, clear baseUrl/apiKey/notes.

Sections:
1. **Preset chips** (add only): one button per preset with `ProviderIcon` + name; selected = primary; `selectedPreset.description` shown below. Selecting a preset in edit mode changes nothing but the highlight (`:104`).
2. **Basics**: `universalProvider.name`/`.namePlaceholder`; `.baseUrl` (placeholder `https://api.example.com`); `.apiKey` with an Eye/EyeOff reveal toggle (placeholder `sk-...`); `.websiteUrl`/`.websiteUrlPlaceholder`; `.notes`/`.notesPlaceholder`.
3. **Enabled apps** (`universalProvider.enabledApps`): three bordered rows with `Switch` — Claude Code, OpenAI Codex, Gemini CLI (literal labels).
4. **Model config** (`universalProvider.modelConfig`): per enabled app. Claude — 2-column grid of `universalProvider.model` ("主模型"), `Haiku`, `Sonnet`, `Opus` (placeholders `claude-sonnet-4-20250514` / `claude-haiku-4-20250514` / …). Codex — Model (`gpt-5.5`) and `Reasoning Effort` (`high`). Gemini — Model (`gemini-2.5-pro`).
5. **Config JSON preview** (**edit mode only**, `:638-700`): `universalProvider.configJsonPreview` + `.configJsonPreviewHint`, then read-only `JsonEditor`s (`onChange` is a no-op) at heights 180 / 280 / 140:
   - Claude: `{env:{ANTHROPIC_BASE_URL, ANTHROPIC_AUTH_TOKEN, ANTHROPIC_MODEL, ANTHROPIC_DEFAULT_HAIKU_MODEL, ANTHROPIC_DEFAULT_SONNET_MODEL, ANTHROPIC_DEFAULT_OPUS_MODEL}}` with defaults `claude-sonnet-4-20250514` / `claude-haiku-4-20250514` / `claude-sonnet-4-20250514` / `claude-sonnet-4-20250514` (`:130-146`).
   - Codex: `{auth:{OPENAI_API_KEY}, config: "<TOML>"}` where the TOML hardcodes `model_provider = "custom"`, `disable_response_storage = true`, `[model_providers.custom] name = "NewAPI"`, `wire_api = "responses"`, `requires_openai_auth = true`, and appends `/v1` to `baseUrl` unless it already ends with it (`:149-173`).
   - Gemini: `{env:{GOOGLE_GEMINI_BASE_URL, GEMINI_API_KEY, GEMINI_MODEL}}` (`:176-186`).

Footer (`:322-344`): `common.cancel`, plus either **Save & Sync** (edit mode with `onSaveAndSync`) which opens a nested `ConfirmDialog` (`.syncConfirmTitle` / `.syncConfirmDescription {name}` / confirmText `.saveAndSync`) before calling back and closing, or **`common.add`** which submits directly. Both are disabled unless `name`, `baseUrl` and `apiKey` are all non-blank.

`handleSubmit` and `buildProvider` are duplicated logic (`:189-301`): edit → spread `editingProvider` and overwrite the edited fields; add → `createUniversalProviderFromPreset(preset, crypto.randomUUID(), baseUrl, apiKey, name)` then overwrite `apps`, `models`, `websiteUrl`, `notes`.

### 6.6 Presets (`src/config/universalProviderPresets.ts`)

```ts
export interface UniversalProviderPreset { name; providerType; defaultApps; defaultModels;
  websiteUrl?; icon?; iconColor?; description?; isCustomTemplate? }
```

`NEWAPI_DEFAULT_MODELS` = claude `{model:"claude-sonnet-5", haikuModel:"claude-haiku-4-5-20251001", sonnetModel:"claude-sonnet-5", opusModel:"claude-opus-4-8"}`, codex `{model:"gpt-5.5", reasoningEffort:"high"}`, gemini `{model:"gemini-3.5-flash"}` (`:171-185`).

| name | providerType | apps | icon / colour | notes |
|---|---|---|---|---|
| `NewAPI` | `newapi` | all three true | `newapi` / `#00A67E` | `websiteUrl: https://www.newapi.pro`; hardcoded Chinese description |
| `自定义网关` | `custom_gateway` | all three true | `openai` / `#6366F1` | `isCustomTemplate: true`; hardcoded Chinese description |

Both `name` and `description` are **untranslated literals** — the port should introduce i18n keys. Helpers: `createUniversalProviderFromPreset` (deep-clones `defaultModels`, sets `createdAt = Date.now()`), `getPresetDisplayName`, `findPresetByType`.

---

## 7. OpenClaw panels

Files: `src/components/openclaw/{EnvPanel,ToolsPanel,AgentsDefaultsPanel,OpenClawHealthBanner}.tsx`, `src/components/openclaw/utils.ts`, `src/components/openclaw/hooks/useOpenClawModelOptions.ts`, `src/hooks/useOpenClaw.ts`, `src/lib/api/openclaw.ts`.

### 7.1 Types (`src/types.ts:661-715`)

```ts
export interface OpenClawDefaultModel { primary: string; fallbacks?: string[] }
export interface OpenClawModelCatalogEntry { alias?: string }
export interface OpenClawHealthWarning { code: string; message: string; path?: string }
export interface OpenClawWriteOutcome { backupPath?: string; warnings: OpenClawHealthWarning[] }
export type OpenClawToolsProfile = "minimal" | "coding" | "messaging" | "full";
export interface OpenClawAgentsDefaults { model?; models?: Record<string, OpenClawModelCatalogEntry>;
  timeoutSeconds?: number; timeout?: number; [key: string]: unknown }  // unknown fields preserved
export interface OpenClawEnvConfig { [key: string]: unknown }
export interface OpenClawToolsConfig { profile?: string; allow?: string[]; deny?: string[]; [key: string]: unknown }
```

### 7.2 Commands (`src/lib/api/openclaw.ts`)

| Method | Command | Args | Result |
|---|---|---|---|
| `getDefaultModel` | `get_openclaw_default_model` | — | `OpenClawDefaultModel \| null` |
| `setDefaultModel` | `set_openclaw_default_model` | `{model}` | `OpenClawWriteOutcome` |
| `getModelCatalog` | `get_openclaw_model_catalog` | — | `Record<string, OpenClawModelCatalogEntry> \| null` |
| `setModelCatalog` | `set_openclaw_model_catalog` | `{catalog}` | `OpenClawWriteOutcome` |
| `getAgentsDefaults` | `get_openclaw_agents_defaults` | — | `OpenClawAgentsDefaults \| null` |
| `setAgentsDefaults` | `set_openclaw_agents_defaults` | `{defaults}` | `OpenClawWriteOutcome` |
| `getEnv` | `get_openclaw_env` | — | `OpenClawEnvConfig` |
| `setEnv` | `set_openclaw_env` | `{env}` | `OpenClawWriteOutcome` |
| `getTools` | `get_openclaw_tools` | — | `OpenClawToolsConfig` |
| `setTools` | `set_openclaw_tools` | `{tools}` | `OpenClawWriteOutcome` |
| `scanHealth` | `scan_openclaw_config_health` | — | `OpenClawHealthWarning[]` |
| `getLiveProvider` | `get_openclaw_live_provider` | `{providerId}` | `Record<string, unknown> \| null` |

### 7.3 Query keys & hooks (`src/hooks/useOpenClaw.ts:14-144`)

`openclawKeys = { all: ["openclaw"], liveProviderIds, defaultModel, env, tools, agentsDefaults, health }`.

| Hook | Key | staleTime | enabled |
|---|---|---|---|
| `useOpenClawLiveProviderIds(enabled)` | `liveProviderIds` (via `providersApi.getOpenClawLiveProviderIds`) | — | param |
| `useOpenClawDefaultModel(enabled)` | `defaultModel` | — | param |
| `useOpenClawEnv` | `env` | 30 s | always |
| `useOpenClawTools` | `tools` | 30 s | always |
| `useOpenClawAgentsDefaults` | `agentsDefaults` | 30 s | always |
| `useOpenClawHealth(enabled)` | `health` | 30 s | param (`isOpenClawView`) |

Mutations invalidate: `useSaveOpenClawEnv` → env + health; `useSaveOpenClawTools` → tools + health; `useSaveOpenClawAgentsDefaults` → agentsDefaults + defaultModel + health. **All toasts live in the components**, and the returned `OpenClawWriteOutcome` (`backupPath`, `warnings`) is currently **ignored** by all three panels — the `openclaw.backupCreated {path}` key exists but is unused.

### 7.4 EnvPanel (view `openclawEnv`)

Raw JSON editing of the whole `env` section. `editorValue` is re-derived from the query whenever `envData` changes: `JSON.stringify(envData, null, 2)` when non-empty, else `"{}"` (`:18-24`) — **this clobbers unsaved edits on any refetch**.

Layout: loading → centered `common.loading`. Otherwise `openclaw.env.description`, then the smaller hint `openclaw.env.editorHint`, then `JsonEditor {rows:18, showValidation:true, language:"json"}`, then a right-aligned Save (`Save` icon, `common.save`/`common.saving`, disabled while pending).

Save (`:41-67`): `parseOpenClawEnvEditorValue` (`utils.ts:17-33`) throws sentinel codes which are mapped to descriptions:

| sentinel | i18n key |
|---|---|
| `OPENCLAW_ENV_EMPTY` | `openclaw.env.empty` |
| `OPENCLAW_ENV_INVALID_JSON` | `openclaw.env.invalidJson` |
| `OPENCLAW_ENV_OBJECT_REQUIRED` | `openclaw.env.objectRequired` |

Success → `toast.success(openclaw.env.saveSuccess)`; any failure → `toast.error(openclaw.env.saveFailed, {description})` where the description is the mapped text or the raw `extractErrorMessage`.

### 7.5 ToolsPanel (view `openclawTools`)

State: `config: OpenClawToolsConfig` plus `allowList`/`denyList` as `{id: crypto.randomUUID(), value}[]` so rows keep identity while editing (`:36-56`).

Unsupported-profile handling (`utils.ts:35-58`): `OPENCLAW_TOOL_PROFILES = ["minimal","coding","messaging","full"]`; sentinels `"__unsupported_profile__"` and `"__unset_profile__"`. `getOpenClawToolsProfileSelectValue(profile)` → unset sentinel when falsy, the profile when supported, else the unsupported sentinel. `getOpenClawUnsupportedProfile` returns the raw string when it isn't in the list.

Layout: loading → `common.loading`. Description `openclaw.tools.description`. Amber `Alert` when the profile is unsupported: `openclaw.tools.unsupportedProfileTitle` / `.unsupportedProfileDescription {value}`. Profile `Select` (220 px) with `openclaw.tools.profileUnset`, a **disabled** `"<value> (openclaw.tools.unsupportedProfileLabel)"` item when applicable, then the four profiles (`.profileMinimal/.profileCoding/.profileMessaging/.profileFull`). Selecting the unsupported sentinel is a no-op; selecting the unset sentinel sets `profile: undefined` (`:153-160`).

Allow/deny lists: `openclaw.tools.allowList` / `.denyList`; each row is a mono `Input` (`openclaw.tools.patternPlaceholder`) + destructive `Trash2`; add buttons `openclaw.tools.addAllow` / `.addDeny` append an empty row.

Save (`:78-96`): `{...otherFieldsFromConfig, profile, allow: allowList.map(v).filter(trim), deny: denyList.map(v).filter(trim)}` — unknown top-level keys are preserved, blank rows dropped. Toast `openclaw.tools.saveSuccess` / `.saveFailed` with `extractErrorMessage` as the description.

### 7.6 AgentsDefaultsPanel (view `openclawAgents`)

Model options (`useOpenClawModelOptions`, `hooks/useOpenClawModelOptions.ts`): reads `useProvidersQuery("openclaw")`, parses each provider's `settingsConfig` (string → `JSON.parse`, failures skipped), requires `config.models` to be an array, and emits `{value: "<providerKey>/<model.id>", label: "<providerName|providerKey> / <model.name|model.id>"}`, de-duplicated by value and sorted by `label.localeCompare(a, "zh-CN")`.

Hydration (`:44-66`): skipped while `agentsData === undefined`; `null` (section absent) clears every field. Populates `primaryModel`, `fallbacks`, `workspace`, `timeout` (via `getOpenClawTimeoutInputValue` = `timeoutSeconds ?? timeout`, `""` when neither is numeric — `utils.ts:60-71`), `contextTokens`, `maxConcurrent`, all `String(...)`-coerced (so `undefined` becomes the literal `"undefined"` only if the field is literally `undefined`… note `String(agentsData.workspace ?? "")` guards that).

Option shaping:
- `primaryOptions` prepends a synthetic `{value: primaryModel, label: t("openclaw.agents.notInList", {value})}` when the stored primary isn't in the catalogue (`:69-84`).
- `getFallbackOptions(i)` excludes the primary and every *other* fallback, and prepends the same synthetic entry for an unknown current value (`:87-112`).

Layout: loading → `common.loading`. Description `openclaw.agents.description`. Amber legacy alert when `typeof timeout === "number" && typeof timeoutSeconds !== "number"` → `.legacyTimeoutTitle` / `.legacyTimeoutDescription` (`:193-220`).

Card 1 `openclaw.agents.modelSection`: `Primary Model` (`.primaryModel`) — when `modelOptions.length === 0 && !modelsLoading` show the italic `.noModels` text instead of the Select; the Select uses `UNSET_SENTINEL = "__unset__"` mapped to `""` and labelled `.notSet`. Hint `.primaryModelHint`. `Fallback Models` (`.fallbackModels`) — hint `.fallbackModelsHint` shown only when empty and models exist; each row is a Select + destructive `Trash2`; `.addFallback` button hidden when there are no models. **Fallback rows are keyed by array index**, so removing a middle row remounts the tail.

Card 2 `openclaw.agents.runtimeSection`: 2-column grid of `.workspace` (placeholder `~/projects`), `.timeout` (number, `300`), `.contextTokens` (number, `200000`), `.maxConcurrent` (number, `4`).

Save (`:130-180`): starts from `{...defaults}` (preserving unknown keys). `model` is written when `primaryModel` is set (with `fallbacks` only when non-empty), or as `{primary: "", fallbacks}` when only fallbacks exist; when both are empty `model` is left **as it was in `defaults`** (never cleared). `workspace` set-or-deleted. Numerics use `parseNum` (`Number`, rejecting NaN/Infinity); each is set or `delete`d, and **`timeout` is always deleted** — this is the legacy→`timeoutSeconds` migration. Toast `openclaw.agents.saveSuccess` / `.saveFailed`.

### 7.7 OpenClawHealthBanner

Rendered by App above the content for `isOpenClawView` when warnings exist (`App.tsx:1582-1584`). Amber `Alert` with `TriangleAlert`, title `openclaw.health.title`, and a `<ul>` of `text + (path ? " (path)" : "")`, keyed `` `${code}:${path ?? message}` ``.

Code → text mapping (`:11-45`), falling back to the backend `message`:

| code | key |
|---|---|
| `invalid_tools_profile` | `openclaw.health.invalidToolsProfile` |
| `legacy_agents_timeout` | `openclaw.health.legacyTimeout` |
| `stringified_env_vars` | `openclaw.health.stringifiedEnvVars` |
| `stringified_env_shell_env` | `openclaw.health.stringifiedShellEnv` |
| `config_parse_failed` | `openclaw.health.parseFailed` |

---

## 8. Hermes memory panel

Files: `src/components/hermes/HermesMemoryPanel.tsx`, `src/hooks/useHermes.ts`, `src/lib/api/hermes.ts`.

### 8.1 Types & commands

```ts
export type HermesMemoryKind = "memory" | "user";                       // src/types.ts:730
export interface HermesMemoryLimits { memory: number; user: number; memoryEnabled: boolean; userEnabled: boolean }
export interface HermesModelConfig { default?, provider?, base_url?, context_length?, max_tokens?, [k:string]: unknown }
```

| Method | Command | Args | Result |
|---|---|---|---|
| `getModelConfig` | `get_hermes_model_config` | — | `HermesModelConfig \| null` |
| `openWebUI` | `open_hermes_web_ui` | `{path: path ?? null}` | `void` |
| `launchDashboard` | `launch_hermes_dashboard` | — | `void` |
| `getMemory` | `get_hermes_memory` | `{kind}` | `string` (empty when the file is absent) |
| `setMemory` | `set_hermes_memory` | `{kind, content}` | `void` (atomic overwrite) |
| `getMemoryLimits` | `get_hermes_memory_limits` | — | `HermesMemoryLimits` |
| `setMemoryEnabled` | `set_hermes_memory_enabled` | `{kind, enabled}` | `void` (other `memory:` fields preserved) |

Query keys (`useHermes.ts:26-32`): `hermesKeys.memory(kind) = ["hermes","memory",kind]`, `memoryLimits = ["hermes","memoryLimits"]` (staleTime 60 s), plus `liveProviderIds` and `modelConfig`. `invalidateHermesProviderCaches(qc)` invalidates `liveProviderIds` + `modelConfig` in parallel.

Mutations: `useSaveHermesMemory` invalidates `memory(kind)` and **emits the error toast itself** (`hermes.memory.saveFailed` + description); success toasts are left to the caller. `useToggleHermesMemoryEnabled` invalidates `memoryLimits`, error toast `hermes.memory.toggleFailed`.

`useOpenHermesWebUI(onOffline?)` (`:151-174`): calls `openWebUI(path)`; if `extractErrorMessage(error) === "hermes_web_offline"` (constant `HERMES_WEB_OFFLINE_ERROR`, must match `src-tauri/src/commands/hermes.rs`) it invokes `onOffline` if provided, else toasts `hermes.webui.offline`; other errors → `toast.error(hermes.webui.openFailed, {description})`.

### 8.2 Panel (view `hermesMemory`)

Root is a flex column; a `Tabs` with `activeTab: HermesMemoryKind` (default `"memory"`). Header row: `TabsList` with `hermes.memory.agentTab` ("Agent Memory (MEMORY.md)") and `hermes.memory.userTab` ("User Profile (USER.md)"), plus a right-aligned outline button `ExternalLink` + `hermes.memory.openConfig` that calls `openHermesWebUI("/config")`.

Limits with fallbacks (`:133-138`): `memory 2200`, `user 1375`, both `enabled` defaulting to `true`.

`MemoryTabPane(kind, limit, enabled)` (`:26-127`):
- Loads via `useHermesMemory(kind, true)` and hydrates the local buffer **only once** (`loaded` flag) so a post-save refetch never clobbers in-flight edits (`:42-47`).
- Top strip: `Switch` (disabled while toggling) + `hermes.memory.enableOn` / `.enableOff`; when disabled the strip turns amber and appends `hermes.memory.disabledHint`.
- Body: `prompts.loading` while `isLoading && !loaded`, else `MarkdownEditor` with `minHeight="calc(100vh - 320px)"`.
- Footer: char counter `hermes.memory.usage {current, limit}`, turning red-600/400 bold and appending `" — " + hermes.memory.overLimit` when `content.length > limit`; the note `hermes.memory.runtimeNote` (hidden below `md`); Save button disabled while pending or before hydration, success toast `hermes.memory.saveSuccess` (errors already toasted by the hook, so the catch is empty).

**Both panes are mounted simultaneously** (`TabsContent` for memory and user, `:164-173`), so both memory files are fetched on entry.

### 8.3 Hermes launch-dashboard dialog (App-level)

`App.tsx:628-630` wires `useOpenHermesWebUI(() => setLaunchDashboardOpen(true))` for the header's `hermes.webui.open` button, so an offline Web UI opens a `ConfirmDialog` instead of a toast (`:1645-1665`): title `hermes.webui.launchConfirmTitle`, message `.launchConfirmMessage` (multi-line, mentions `pip install hermes-agent[web]`), confirmText `.launchConfirmAction`, `variant="info"`. Confirm → `hermesApi.launchDashboard()` → `toast.success(hermes.webui.launching)` or `toast.error(hermes.webui.launchFailed, {description})`.

---

## 9. Profiles manage dialog

Files: `src/components/profiles/ProfileManageDialog.tsx`, `src/components/profiles/scope.ts`, `src/lib/api/profiles.ts`, `src/lib/query/profiles.ts`.

### 9.1 Types (`src/lib/api/profiles.ts:9-55`)

```ts
export type ProfileScope = "claude" | "claude-desktop" | "codex";
export interface PerApp<T> { claude: T; "claude-desktop": T; codex: T }
export interface ProfilePayload { providers: PerApp<string|null>; mcp: PerApp<string[]|null>;
  skills: PerApp<string[]|null>; prompts: PerApp<string|null> }   // all-null slot = never snapshotted
export interface Profile { id, name, payload: ProfilePayload, createdAt?, updatedAt? }
export interface CurrentProfileIds { claude: string|null; claudeDesktop: string|null; codex: string|null }  // camelCase!
export interface ProfilesResponse { profiles: Profile[]; currentIds: CurrentProfileIds }
```

| Method | Command | Args | Result |
|---|---|---|---|
| `list` | `list_profiles` | — | `ProfilesResponse` |
| `create` | `create_profile` | `{name, scope}` | `Profile` |
| `update` | `update_profile` | `{id, name?, resnapshot?, scope?}` | `Profile` |
| `delete` | `delete_profile` | `{id}` | `void` |
| `apply` | `apply_profile` | `{id, scope}` | `string[]` (warnings, best-effort) |
| `clearCurrent` | `clear_current_profile` | `{scope}` | `void` |

`ProfileScope` is kebab-case as a **command argument**, while `CurrentProfileIds` uses camelCase `claudeDesktop` as a **response field** — the port must keep both encodings.

### 9.2 Util (`src/components/profiles/scope.ts`)

- `APP_PROFILE_SCOPE: Partial<Record<AppId, ProfileScope>> = { claude: "claude", "claude-desktop": "claude-desktop", codex: "codex" }` — mirrors Rust `ProfileScope::for_app`; apps absent from the map don't render the switcher at all.
- `SCOPE_SLOT_KEYS` maps each scope to its single payload slot.
- `hasScopeSnapshot(profile, scope)` → true when **any** of `providers/mcp/skills/prompts` for that slot is non-null; used to distinguish "never snapshotted" (apply only binds the current pointer) from "snapshotted empty" (apply clears).

### 9.3 Query layer (`src/lib/query/profiles.ts`)

All mutations invalidate `["profiles"]` and then call `updateTrayMenuSafely()` (which swallows tray errors).

| Hook | Success toast | Error toast |
|---|---|---|
| `useCreateProfileMutation` | `profiles.createSuccess` | `profiles.createFailed {detail}` |
| `useUpdateProfileMutation` | `profiles.updateSuccess` | `profiles.updateFailed {detail}` |
| `useDeleteProfileMutation` | `profiles.deleteSuccess` | `profiles.deleteFailed {detail}` |
| `useClearProfileMutation` | `profiles.clearSuccess` | `profiles.applyFailed {detail}` (reuses the apply key) |
| `useApplyProfileMutation` | `profiles.applySuccess`, or `toast.warning(profiles.applyWarnings {warningCount, details})` with `duration: 10000` when the command returns warnings | `profiles.applyFailed {detail}` |

`useApplyProfileMutation` additionally invalidates `["providers","claude"]`, `["providers","claude-desktop"]`, `["providers","codex"]`, `["mcp","all"]`, `["skills"]` (`:122-132`). `detail = extractErrorMessage(error) || t("common.unknown")`, all with `closeButton: true`.

### 9.4 Dialog

`Dialog` `max-w-md`; title `profiles.manageTitle`, description `profiles.manageDescription`. Body `max-h-[50vh] overflow-y-auto`.

- Empty → centered `profiles.empty`.
- Row (hover `bg-muted/50`): in **view** state, truncated name + `Pencil` (`profiles.rename`) + destructive `Trash2` (`profiles.delete`); in **rename** state, an autofocused `Input` + `Check` (`common.confirm`, disabled when blank or pending) + `X` (`common.cancel`).
- Keyboard while renaming: `Enter` → save, `Escape` → cancel (`:110-113`). No blur handling.
- `saveRename` no-ops on a blank name; on success `cancelRename()` runs via the mutation's `onSuccess` callback.
- Delete opens a `ConfirmDialog` (`profiles.deleteConfirmTitle`, `profiles.deleteConfirmMessage {name}`, `variant="destructive"`); confirming fires the mutation **fire-and-forget** and clears the pending state immediately.
- Closing the dialog (footer `common.close` or outside/Escape) always calls `cancelRename()` first.

This dialog is purely snapshot-management: renaming and deleting act on the shared entity across all scopes, and there is **no manual re-snapshot entry point** (snapshots are refreshed automatically on switch — see the component doc comment at `:31-36`).

---

## 10. Deep link dialogs

Files: `src/components/DeepLinkImportDialog.tsx`, `src/components/deeplink/{McpConfirmation,PromptConfirmation,SkillConfirmation}.tsx`, `src/lib/api/deeplink.ts`, `src/lib/utils/base64.ts`.

### 10.1 Types & commands (`src/lib/api/deeplink.ts`)

```ts
export type ResourceType = "provider" | "prompt" | "mcp" | "skill";
export interface DeepLinkImportRequest {
  version: string; resource: ResourceType;
  app?: "claude" | "codex" | "gemini"; name?: string; enabled?: boolean;
  homepage?; endpoint?; apiKey?; icon?; model?; notes?; haikuModel?; sonnetModel?; opusModel?;   // provider
  content?; description?;                                                                        // prompt
  apps?: string;                        // "claude,codex,gemini"                                 // mcp
  repo?; directory?; branch?;                                                                    // skill
  config?; configFormat?; configUrl?;                                                            // config file
  usageEnabled?: boolean; usageScript?; usageApiKey?; usageBaseUrl?; usageAccessToken?;
  usageUserId?; usageAutoInterval?: number;                                                      // v3.9+
}
export type ImportResult =
  | { type: "provider"; id: string }
  | { type: "prompt"; id: string }
  | { type: "mcp"; importedCount: number; importedIds: string[]; failed: {id,error}[] }
  | { type: "skill"; key: string };
```

| Method | Command | Args | Result |
|---|---|---|---|
| `parseDeeplink` | `parse_deeplink` | `{url}` | `DeepLinkImportRequest` |
| `mergeDeeplinkConfig` | `merge_deeplink_config` | `{request}` | `DeepLinkImportRequest` |
| `importFromDeeplink` | `import_from_deeplink_unified` | `{request}` | `ImportResult` |

`parseDeeplink` is **not called from the frontend** — the backend parses the `ccswitch://` URL and emits the events below.

### 10.2 Events (`DeepLinkImportDialog.tsx:51-92`)

| Event | Payload | Handling |
|---|---|---|
| `deeplink-import` | `DeepLinkImportRequest` | If `config` **or** `configUrl` is present, `mergeDeeplinkConfig(payload)` first; a merge failure toasts `deeplink.configMergeError` (+ description) and falls back to the raw payload. Then `setRequest(...)` and open the dialog. |
| `deeplink-error` | `{url, error}` | `console.error` + `toast.error(deeplink.parseError, {description: payload.error})` |

Both listeners are torn down on unmount; the effect depends on `[t]` so a language change re-subscribes.

### 10.3 Dialog shell

`Dialog open={isOpen && !!request}`, `DialogContent sm:max-w-[500px] zIndex="top"`. Header left-aligned; body `max-h-[60vh] overflow-y-auto` with a custom thin scrollbar. Footer: `common.cancel` and a primary button `deeplink.import` / `deeplink.importing`, both disabled while importing.

Title / description by resource (`:292-318`):

| resource | title | description |
|---|---|---|
| `prompt` | `deeplink.importPrompt` | `deeplink.importPromptDescription` |
| `mcp` | `deeplink.importMcp` | `deeplink.importMcpDescription` |
| `skill` | `deeplink.importSkill` | `deeplink.importSkillDescription` |
| `provider` / missing | `deeplink.confirmImport` | `deeplink.confirmImportDescription` |

### 10.4 Provider body (`:344-709`)

Rendered when `resource === "provider"` or `resource` is falsy. Sections, in order:
1. Centred 80 px `ProviderIcon` when `icon` is set.
2. `deeplink.app` (capitalised), `deeplink.providerName`, `deeplink.homepage` (blue), `deeplink.endpoint` — comma-split into lines, first prefixed `🔹 ` and annotated `(deeplink.primaryEndpoint)` when there is more than one, rest prefixed `└ `.
3. `deeplink.apiKey` — masked as `apiKey.slice(0,4) + "*".repeat(20)` when longer than 4, else `"****"` (`:210-213`).
4. Models: for `app === "claude"` show `deeplink.haikuModel`, `.sonnetModel`, `.opusModel`, `.multiModel` (the last for `model`); otherwise a single `deeplink.model` row.
5. `deeplink.notes` when present.
6. **Config file block** when `config || configUrl`: `deeplink.configSource` pill `deeplink.configEmbedded` (base64) or `.configRemote` (url) + uppercase `configFormat`; then parsed details in `deeplink.configDetails`:
   - `claude` → `parsed.env` key/value grid.
   - `codex` → `parsed.auth` grid under a literal `"Auth:"` label, plus a `<pre>` of `parsed.config` truncated to 300 chars under a literal `"TOML Config:"`.
   - `gemini` → the flat object as a key/value grid.
   Values are masked by `maskValue`: keys containing `TOKEN`, `KEY`, `SECRET` or `PASSWORD` (case-insensitive) and longer than 8 chars become `value.slice(0,8) + "*".repeat(12)` (`:281-290`). Then `deeplink.configUrl` when remote.
   Decoding uses a **local** `b64ToUtf8` (`:233-242`) that falls back to plain `atob` — unlike the shared `decodeBase64Utf8` used by the sub-components, it does not repair spaces or padding.
7. **Usage block** when `usageScript`: `deeplink.usageScript` with a green/grey pill `deeplink.usageScriptEnabled` / `.usageScriptDisabled` (enabled unless `usageEnabled === false`); `deeplink.usageApiKey` (masked `slice(0,4)+12*`) only when it differs from `apiKey`; `deeplink.usageBaseUrl` only when it differs from `endpoint`; `deeplink.usageAutoInterval` → `deeplink.usageAutoIntervalValue {minutes}` when `> 0`.
8. Yellow warning box `deeplink.warning`.

### 10.5 Resource sub-components

All three use `decodeBase64Utf8` (`src/lib/utils/base64.ts:13-43`), which replaces spaces with `+`, retries with `=` padding, and finally falls back to `decodeURIComponent(escape(atob(...)))` or the raw input.

**PromptConfirmation**: heading `deeplink.prompt.title`; fields `.app` (capitalised), `.name`, optional `.description`, `.contentPreview` (`<pre>`, first 500 chars + `"..."`); yellow ⚠️ `.enabledWarning` when `request.enabled`.

**McpConfirmation**: heading `deeplink.mcp.title`; `.targetApps` pills from `request.apps.split(",")` (trimmed, capitalised); `.serverCount {count}` from `JSON.parse(decoded).mcpServers || {}`; a `max-h-64` scroll list of `{id, "Command: <command> " | "URL: <url> "}`; ⚠️ `.enabledWarning` when `request.enabled`. A parse failure logs and yields `null`, so the count reads 0 and the list is empty.

**SkillConfirmation**: heading `deeplink.skill.title`; mono boxes for `.repo` and `.directory`; `.branch` defaulting to `"main"`; a blue info box with ℹ️ `.hint` + `.hintDetail`. (The key `deeplink.skill.skillsPath` exists but is unused.)

### 10.6 Import result handling (`:94-203`)

`importFromDeeplink(request)`, then:

| Result | Refresh | Toast |
|---|---|---|
| `type: "provider"` | invalidate `["providers", request.app]` | `deeplink.importSuccess` + `.importSuccessDescription {name}` |
| `type: "prompt"` | `window.dispatchEvent(new CustomEvent("prompt-imported", {detail:{app}}))` — prompts are not in React Query | `deeplink.promptImportSuccess` + `.promptImportSuccessDescription {name}` |
| `type: "mcp"` | `refreshMcp`: `invalidateQueries({queryKey:["mcp","all"], refetchType:"all"})` **then** `refetchQueries({queryKey:["mcp","all"], type:"all"})` | `failed.length > 0` → `toast.warning(deeplink.mcpPartialSuccess, {description: .mcpPartialSuccessDescription {success, failed}})`; else `toast.success(deeplink.mcpImportSuccess, {description: .mcpImportSuccessDescription {count}})` |
| `type: "skill"` | invalidate + refetch `["skills"]` with `refetchType/type: "all"` | `deeplink.skillImportSuccess` + `.skillImportSuccessDescription {repo}` |
| no `type` but `{importedCount:number, importedIds:[], failed:[]}` (`isMcpImportResult`, `:34-49`) | same as mcp | same as mcp |
| anything else (legacy string id) | invalidate `["providers", request.app]` | provider success toast |

The dialog closes **only after** all refreshes resolve (`:194`). Any throw → `toast.error(deeplink.importError, {description})` and the dialog stays open. `isImporting` is cleared in `finally`.

---

## Appendix A — shared primitives the port must provide

| Primitive | Contract | Source |
|---|---|---|
| `FullScreenPanel` | `{isOpen, title, onClose, children, footer?, contentClassName?}`; portals to `document.body`, `z-[60]`, fades 200 ms, locks `body.overflow`, 28 px drag bar on macOS only, 64 px header with a back `ArrowLeft` button, content `px-6 py-6 space-y-6` (twMerge-overridable), sticky bordered footer right-aligned `gap-3`; bubbling-phase Escape that ignores `defaultPrevented` and text-editable targets and `stopPropagation()`s | `common/FullScreenPanel.tsx` |
| `ConfirmDialog` | `{isOpen, title, message, confirmText?, cancelText?, variant="destructive"|"info", zIndex="base"|"nested"|"alert"|"top" (default "alert"), checkboxLabel?, checkboxDefaultChecked?, onConfirm(checked), onCancel}`; `max-w-sm`, `whitespace-pre-line` message, `AlertTriangle`/`Info` icon, outside-click/Escape → `onCancel` | `ConfirmDialog.tsx` |
| `AppCountBar` | `{totalLabel, counts: Partial<Record<AppId,number>>, appIds=APP_IDS}`; glass bar with an outline total badge and per-app coloured badges `LABEL: N` | `common/AppCountBar.tsx` |
| `AppToggleGroup` | `{apps, onToggle(app, enabled), appIds=APP_IDS}`; 28 px icon buttons, active = per-app ring/tint, inactive = `opacity-35`; tooltip `LABEL` + `" ✓"` | `common/AppToggleGroup.tsx` |
| `ListItemRow` | `{isLast?, children}`; `group flex items-center gap-3 px-4 py-2.5 hover:bg-muted/50`, bottom border except on the last row | `common/ListItemRow.tsx` |
| `APP_IDS` / `SKILLS_APP_IDS` / `MCP_APP_IDS` | 8 / 6 / 6 entries; per-app `{label, icon, activeClass, badgeClass}` in `APP_ICON_MAP` (orange Claude, amber Claude Desktop, green Codex, blue Gemini, cyan Grok Build, indigo OpenCode, rose OpenClaw, Hermes) | `config/appConfig.tsx:18-120` |
| `MarkdownEditor` / `JsonEditor` | see §3.4 and §1.5 | `MarkdownEditor.tsx`, `JsonEditor.tsx` |
| `useDarkMode()` | `MutationObserver` on `documentElement.class` for `"dark"` | `hooks/useDarkMode.ts` |
| `useTauriEvent(name, handler)` | ref-stable handler, disposal-guarded `listen` | `hooks/useTauriEvent.ts` |
| `useLastValidValue(v)` | retains the last non-nullish value during close animations | `hooks/useLastValidValue.ts` |
| `isTextEditableTarget(t)` | INPUT / TEXTAREA / SELECT / `isContentEditable` | `utils/domUtils.ts` |

## Appendix B — findings / risks for the port

1. **Missing i18n key**: `provider.duplicateLiveIdsLoadFailed` (`App.tsx:757`) is absent from `en.json` and relies on its Chinese `defaultValue`. Every other key in the 43 audited files resolves.
2. **Universal view has no "add" entry point** (§6.3) — creating a universal provider is only possible from `AddProviderDialog`'s Universal tab.
3. **`AgentsPanel` is an un-i18n'd English placeholder** with no navigation route in v3.17.0 (§0.5).
4. **`McpFormModal`'s TOML mode is unreachable** from `UnifiedMcpPanel` (`defaultFormat="json"` is hardcoded), as is `McpWizardModal`'s TOML conversion path.
5. **`openclaw` has no checkbox in `McpFormModal`** despite being in the `enabledApps` state (§1.5), and neither `claude-desktop` nor `openclaw` ever appear in the MCP/Skills count bars or toggle groups.
6. **`EnvPanel` re-derives its editor text from the query on every `envData` change** — a background refetch discards unsaved JSON (§7.4). `HermesMemoryPanel` explicitly avoids the same bug with its `loaded` flag (§8.2) — pick one behaviour deliberately.
7. **`OpenClawWriteOutcome.backupPath` / `.warnings` are silently dropped** by all three OpenClaw panels; `openclaw.backupCreated` is an orphan key.
8. **Session resume is macOS-only** (§4.6); every other platform silently degrades to "copy command".
9. **Hermes has no option in the session provider filter**, although it is a valid `ProviderFilter` value and a session-capable app (§4.5).
10. **`WorkspaceFilesPanel` reads the full contents of 9 files just to test existence** (§5.2) — the Rust port should add an `exists` command.
11. **`skillBackups.createdAt` is Unix *seconds*** while `SessionMeta` timestamps and `InstalledSkill.installedAt` are milliseconds (§2.4).
12. **Two different base64 decoders** coexist: the robust `decodeBase64Utf8` in the sub-components and the naive local `b64ToUtf8` in `DeepLinkImportDialog` (§10.4).
13. **`UpdateBadge` ignores `isDismissed`** (§0.5), so the dismiss API in `UpdateContext` currently has no visible effect.
14. Dead/duplicate components to consolidate rather than port twice: `prompts/PromptFormModal.tsx` vs `PromptFormPanel.tsx`, `skills/RepoManager.tsx` vs `RepoManagerPanel.tsx`, and `handleSubmit`/`buildProvider` in `UniversalProviderFormModal`.
15. **Chinese literals still in code paths** the port must translate: `tomlUtils` thrown messages (§1.7), `universalProviderPresets` names/descriptions (§6.6), `AgentsPanel` English copy, and many `defaultValue` strings across `App.tsx`/`SessionManagerPage.tsx`.