# Provider list area (React v3.17.0) — behavioural spec for the Dioxus port

Sources: `src/components/providers/ProviderList.tsx` (655 lines), `ProviderCard.tsx` (635),
`ProviderActions.tsx` (371), `ProviderEmptyState.tsx`, `ProviderHealthBadge.tsx`,
`FailoverPriorityBadge.tsx`, `src/hooks/useProviderActions.ts` (427), `src/hooks/useDragSort.ts` (119),
`src/lib/query/{queries,mutations,failover,omo,profiles}.ts`, `src/App.tsx`.

## 1. ProviderList

Props: `providers: Record<id, Provider>`, `currentProviderId`, `appId`, callbacks
(switch/edit/delete/removeFromConfig/disableOmo/disableOmoSlim/duplicate/configureUsage/openWebsite/
openTerminal/create/setAsDefault), flags `isLoading`, `isProxyRunning`, `isProxyTakeover`, `activeProviderId`.

Render states in order:
1. `isLoading` → three dashed skeleton blocks (`h-28`).
2. unfiltered list empty → `ProviderEmptyState` (dashed box, `Users` icon, `provider.noProviders`,
   `provider.noProvidersDescription`, plus `provider.noProvidersDescriptionSnippet` for claude/codex/gemini;
   buttons: import (`provider.importFromClaude` for claude-desktop else `provider.importCurrent`) and
   create `provider.addProvider`).
3. otherwise: Claude Desktop status banner (claude-desktop only, amber, `claudeDesktop.statusTitle`;
   messages: `statusUnsupported` (exclusive), `statusStaleRawModels`, `statusMissingRouteMappings`,
   `statusGatewayTokenMissing` when mode=proxy and no gateway token, `statusBaseUrlMismatch {expected, actual}`),
   the floating search card, then either `provider.noSearchResults` or the sortable list.

Search: hidden by default; `Cmd/Ctrl+F` opens (unless focus is in an editable element), `Escape` closes;
case-insensitive substring over name, notes, websiteUrl. Keys: `provider.searchPlaceholder`,
`provider.searchAriaLabel`, `common.clear`, `provider.searchCloseAriaLabel`, `provider.searchScopeHint`,
`provider.searchCloseHint`.

Import (empty state only): opencode → `import_opencode_providers_from_live`, openclaw →
`import_openclaw_providers_from_live`, hermes → `import_hermes_providers_from_live`, claude-desktop →
`import_claude_desktop_providers_from_claude` (counts; >0 = imported), else `import_default_config` (bool).
Success → invalidate providers (+ claudeDesktopStatus) and toast `provider.importCurrentDescription`;
nothing imported → `toast.info(provider.noProviders)`; error → `toast.error(message)`.

Current provider per app: category `omo` → `get_current_omo_provider_id`; `omo-slim` →
`get_current_omo_slim_provider_id`; hermes → `hermesModelConfig.provider == id`; else `get_current_provider`.
`isInConfig` (additive apps only): opencode → `get_opencode_live_provider_ids`, openclaw →
`get_openclaw_live_provider_ids`, hermes → `get_hermes_live_provider_ids`; others always true.
`isDefaultModel`: hermes → same as current; openclaw → `openclawDefaultModel.primary.startsWith(id + "/")`.
Failover: active iff `isProxyTakeover && autoFailoverEnabled`; priority = 1-based index in `get_failover_queue`;
toggle → `add_to_failover_queue` / `remove_from_failover_queue {appType, providerId}`.
Connectivity test: `useStreamCheck(appId).checkProvider(id, name)`, no confirm.

Sorting (`useDragSort`): `sortIndex` asc with missing last, then `createdAt` asc, then `localeCompare`
(zh-CN / zh-TW / en-US by language). Drag: pointer sensor distance 8 + keyboard; on drop send the FULL visible
list as `update_providers_sort_order { updates: [{id, sortIndex}], app }`, invalidate providers and
failoverQueue, `update_tray_menu` (best effort), toast `provider.sortUpdated` / `provider.sortUpdateFailed`.
Not optimistic.

## 2. ProviderCard

Layout: rounded-xl bordered card, `relative overflow-hidden p-4`, gradient overlay when highlighted; row =
drag handle (`GripVertical`, `provider.dragHandle`) · 32px `ProviderIcon` · name + badges + URL line ·
usage/quota area · actions cluster (hidden until hover/focus-within).

URL line: notes → websiteUrl → `env.ANTHROPIC_BASE_URL` / `env.GOOGLE_GEMINI_BASE_URL` → codex TOML
`base_url` → `provider.notConfigured`; clickable (`open_external`) unless it came from notes/fallback.

Highlight: `isActiveProvider` = omo → current; openclaw → isDefaultModel; opencode (non-omo) → false;
autoFailover → `activeProviderId == id`; else current. Green (emerald) when proxy takeover and active;
blue when active without takeover or (opencode && inConfig); dragging → `border-primary shadow-lg scale-105`.

Badges in order: `OMO` (violet) / `Slim` (indigo); claude-desktop non-official proxy mode →
`claudeDesktop.modeProxy`; claude non-official apiFormat≠anthropic → `claudeCode.needsRouting`; codex needing
routing (apiFormat openai_chat/anthropic or TOML wire_api chat/anthropic) → `codex.needsRouting`; claude
official → `claudeCode.noRoutingSupport`; codex official routable (`codex-official`) → `codex.officialRouting`
(takeover) / `codex.nativeLogin`; codex official otherwise → `codex.noRoutingSupport`; health badge when
proxy running and in failover queue (`health.operational` green / `health.degraded` yellow /
`health.circuitOpen` red, tooltip `health.consecutiveFailures {count}`); `FailoverPriorityBadge` `P{n}`
(`failover.priority.tooltip`); partner star when `category == third_party && meta.isPartner`
(`provider.officialPartner`); hermes read-only (`settingsConfig._cc_source == providers_dict`) →
`provider.managedByHermes` / `provider.managedByHermesHint`.

Usage area (first match): Copilot (`meta.providerType == github_copilot`) → `CopilotQuotaFooter`; codex
OAuth → `CodexOauthQuotaFooter`; official → `SubscriptionQuotaFooter` when official subscription usage is
enabled else nothing; multiple plans → `usage.multiplePlans {count}` + expand/collapse
(`usage.expand`/`usage.collapse`); else inline `UsageFooter`. `query_usage` polled with
`meta.usage_script.autoQueryInterval` only when the provider is current (or in config for additive apps).

## 3. ProviderActions

`isAdditiveMode = (opencode && !omo) || openclaw || hermes`; `isFailoverMode = !additive && !omo &&
autoFailoverEnabled`. Optional "set default" (openclaw/hermes when inConfig): `Zap`; hermes labels
`provider.inUse`/`provider.enable`, openclaw `provider.isDefault`/`provider.setAsDefault`; disabled when
already default.

Main button (first match):

| condition | style | icon | label | disabled |
|---|---|---|---|---|
| omo && current | secondary | Check | `provider.inUse` | no |
| omo && !current | default | Play | `provider.enable` | no |
| additive && inConfig | orange | Minus | `provider.removeFromConfig` | iff isDefaultModel |
| additive && !inConfig | emerald | Plus | `provider.addToConfig` | no |
| failover && inQueue | blue-100 | Check | `failover.inQueue` | no |
| failover && !inQueue | blue-500 | Plus | `failover.addQueue` | no |
| current | secondary | Check | `provider.inUse` | yes |
| official blocked by proxy | default | Play | `provider.enable` | yes, title `provider.blockedByProxyHint` |
| default | default (emerald under takeover) | Play | `provider.enable` | no |

Click: omo → disableOmo/switch; additive → removeFromConfig/switch(add); failover → toggle queue; else switch.
Icon buttons: Edit (disabled when hermes read-only), Duplicate (`provider.duplicate`), Connectivity
(`provider.connectivityCheck`, only non-official), Usage (`provider.configureUsage`; not for official
without subscription, copilot, codex oauth), Terminal (`provider.openTerminal`, claude only), Delete
(`canDelete = !readOnly && (omo || additive || !current)`).

## 4. Actions (useProviderActions)

switch(provider):
1. classify copilot / codex chat / codex anthropic formats.
2. if proxy not running and non-official: warning reason `notifications.proxyReasonCopilot |
   proxyReasonOpenAIChat | proxyReasonOpenAIResponses | proxyReasonAnthropicMessages | proxyReasonClaudeDesktop
   | proxyReasonFullUrl` → `toast.warning(notifications.proxyRequiredForSwitch {reason})` (switch continues).
3. hard block: takeover && official && not `codex-official` → `toast.error(notifications.officialBlockedByProxy)`, return.
4. `switch_provider {id, app}` → `{warnings}`; error toast title `notifications.switchFailedTitle`,
   description `notifications.switchFailed {error}` with copy action.
5. claude: if `enableClaudePluginIntegration` → `apply_claude_plugin_config {official}`
   (`notifications.syncClaudePluginFailed` on error).
6. warnings → `toast.warning(notifications.backfillWarning)`.
7. success toast: `notifications.switchSuccess`; codex `codexRestartRequired`; grokbuild
   `grokBuildRestartRequired`; claude-desktop `claudeDesktopProxyRestartRequired`/`claudeDesktopRestartRequired`;
   opencode/openclaw `addToConfigSuccess`.
Invalidate: providers; claude-desktop → proxyStatus, claudeDesktopStatus; opencode → live ids, omo ids;
openclaw → live ids, defaultModel, health; hermes caches; `update_tray_menu`.

delete(id): confirm dialog (`confirm.deleteProvider`, `confirm.deleteProviderMessage {name}`) →
`delete_provider {id, app}` → invalidate providers (+ omo/openclaw/hermes keys), `update_tray_menu`,
toast `notifications.deleteSuccess` / `notifications.deleteFailed`. Remove-from-config (additive):
`confirm.removeProvider` / `confirm.removeProviderMessage` → `remove_provider_from_live_config`,
toast `notifications.removeFromConfigSuccess`.

duplicate: copy gets `sortIndex + 1`, shift the rest via `update_providers_sort_order` first
(`provider.sortUpdateFailed` aborts), then add.
add: `add_provider {provider, app, addToLive}` (+ `ensure_claude_desktop_official_provider` /
`ensure_codex_official_provider` seeds); toast `notifications.providerAdded` / `notifications.addFailed {error}`;
runs `injectCodingPlanUsageScript` (claude coding-plan base URLs) and OpenClaw model catalog registration
(`notifications.openclawModelsRegistered`).
update: `update_provider {provider, app, originalId}`; `notifications.updateSuccess` / `updateFailed`.
setAsDefaultModel (openclaw): `set_openclaw_default_model {primary: "<id>/<firstModel>", fallbacks}`;
`notifications.openclawDefaultModelSet` / `openclawNoModels`.

Events: `provider-switched {appType, providerId}` → refetch when appType == active app;
`universal-provider-synced` → invalidate providers + tray; `profile-applied` → invalidate profiles, mcp,
skills, proxy, providers(claude-desktop); `proxy-official-warning` → `notifications.proxyOfficialWarning` (8 s).

## 5. Header pieces (providers view)

Gate: proxy/failover cluster only on providers view and app ∉ {opencode, openclaw, hermes}.
- ProxyToggle (not claude-desktop, `settings.enableLocalProxy`): `Radio` icon + Switch bound to
  `takeoverStatus[app]` (`get_proxy_takeover_status`); toggle → `set_proxy_takeover_for_app {appType, enabled}`;
  toasts `proxy.takeover.enabled/disabled`; tooltips `proxy.takeover.tooltip.active {appLabel, address, port}`
  / `.broken` / `.inactive`. `get_proxy_status` polled every 2 s while running.
- FailoverToggle (not claude-desktop, `settings.enableFailoverToggle`): `Shuffle` + Switch bound to
  `get_auto_failover_enabled`; disabled unless takeover active; `set_auto_failover_enabled` (optimistic);
  toasts `failover.enabled/disabled {app}`, `failover.toggleFailed`; tooltips `failover.tooltip.enabled/disabled`.
- ClaudeDesktopRouteToggle (claude-desktop): Switch bound to proxy `isRunning`; on → `start_proxy_server`;
  off blocked while any of claude/codex/gemini/grokbuild is taken over
  (`claudeDesktop.route.stopBlockedByTakeover`, key missing in en.json) else `stop_proxy_server`;
  tooltips `claudeDesktop.route.tooltip.active/inactive {address, port}` (default 127.0.0.1:15721).
- ProfileSwitcher (`settings.showProfileSwitcher`, only claude/claude-desktop/codex): combobox of
  `list_profiles`; select → `apply_profile {id, scope}`; `create_profile {name, scope}`;
  `clear_current_profile {scope}`; manage dialog. Keys `profiles.*`.
- UpdateBadge: green `ArrowUpCircle` when an update is available (`settings.updateAvailable {version}`),
  opens Settings → About.

## 6. Add / edit entry points

`AddProviderDialog` (`FullScreenPanel`, title `provider.addNewProvider`): tabs (`apps.{appId} +
provider.tabProvider`, `provider.tabUniversal`) for claude/codex/gemini; the form is `ProviderForm`
(`form="provider-form"`, external submit `common.add`, footer hint `provider.addFooterHint`).
`ProviderForm` dispatches: claude-desktop → `ClaudeDesktopProviderForm`; grokbuild → `GrokBuildProviderForm`;
else `ProviderFormFull` (preset selector inside). Props: `appId, providerId?, submitLabel, onSubmit(values),
onCancel, onUniversalPresetSelect?, onManageUniversalProviders?, onSubmittingChange?, initialData?,
showButtons?, isProxyTakeover?`. Output `ProviderFormValues = { name, websiteUrl?, notes?, settingsConfig
(JSON string), icon?, iconColor?, presetId?, presetCategory?, isPartner?, meta?, providerKey?,
suggestedDefaults? }`. Submit builds the Provider, seeds `meta.custom_endpoints` from preset
`endpointCandidates` + the config's base URL, then `add_provider`. `EditProviderDialog` reuses the form
with `initialData` and calls `update_provider {provider, app, originalId}`.

## 7. i18n

Namespaces: `provider.*` (69), `notifications.*` (44), `confirm.*`, `failover.*`, `health.*`,
`streamCheck.*`, `claudeDesktop.*`, `proxy.*`, `codex.*`, `claudeCode.*`, `usage.*`, `profiles.*`,
`universalProvider.*`, `omo.*`. Missing in en.json (fallback to inline Chinese defaults in React):
`provider.duplicateLiveIdsLoadFailed`, `failover.tooltip.takeoverRequired`,
`notifications.proxyReasonClaudeDesktop`, `claudeDesktop.route.stopBlockedByTakeover`.
