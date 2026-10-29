# Provider management UI — findings (FE-PROV-xx)

Part of the [next-phase proposals](../README.md). 55 findings. Priorities: P1 = 2, P2 = 28, P3 = 25.

## Area summary

The provider UI is feature-rich and clearly battle-hardened (sequence guards in the Codex common-config hook, SSOT protection for the Codex model catalog, Hermes URL validation, stable row keys for list editors), but it has grown by accretion into one 2.6k-line god component (`ProviderFormFull`) that mounts every app's hooks on every render and branches on `appId` in submit, preset handling, meta assembly and JSX. The most serious risks are state bugs rooted in that architecture: hooks receive non-reactive `form.getValues("settingsConfig")` snapshots so JSON-editor edits can be silently overwritten by the next field keystroke, the edit dialog backfills live config after the form is already editable, and the Add dialog's Tabs unmount the form (losing all input) when the user peeks at the "universal" tab. A second cluster is validation/i18n: zod and helper error strings are hard-coded Chinese or English literals, native `required` attributes short-circuit the app's own soft-validation, base-URL validation exists only for Hermes, and invalid Codex/Gemini editor state is silently replaced with stale defaults on save. A third cluster is UX/a11y: the full-screen panel has no dialog semantics or focus management, card actions are hover-only, TOML/.env editors use JavaScript highlighting, and cards for OpenCode/OpenClaw/Hermes/GrokBuild show "not configured" because endpoint extraction only knows Claude/Codex/Gemini shapes. Test coverage is thin where it matters: the dialogs are tested with the form mocked out, ProviderCard is covered only by a source-string test, and no test exercises a real `ProviderForm` submit for any app. Suggested direction: (1) fix the four data-loss bugs first (FE-PROV-01/02/04/05/06), (2) land the per-app "form adapter + config codec" architecture (FE-PROV-17/19/22) so new apps stop adding branches to the shell, (3) then build the high-value features on that base (health panel, command palette, groups/bulk ops, key hygiene).

## Index

| ID | Title | Type | Priority | Effort | Confidence | Verdict |
|---|---|---|---|---|---|---|
| [FE-PROV-01](#fe-prov-01) | JSON-editor edits are silently overwritten by the next field edit (stale `getValues` snapshots) | bug | P1 | M | high | confirmed |
| [FE-PROV-06](#fe-prov-06) | OMO custom agent/category rows vanish before a model is chosen | bug | P1 | S | high | confirmed |
| [FE-PROV-02](#fe-prov-02) | Edit dialog backfills live config after the form is already editable, resetting user input | bug | P2 (first pass P1) | M | high | partially-confirmed |
| [FE-PROV-03](#fe-prov-03) | Validation messages are hard-coded Chinese/English literals outside i18n | bug | P2 (first pass P1) | S | high | confirmed |
| [FE-PROV-04](#fe-prov-04) | Switching to the "Universal" tab in Add Provider unmounts the form and loses all input | bug | P2 (first pass P1) | S | high | confirmed |
| [FE-PROV-05](#fe-prov-05) | Hermes provider-key rename is accepted by the form but ignored on save | bug | P2 (first pass P1) | S | high | confirmed |
| [FE-PROV-07](#fe-prov-07) | Native `required` attribute short-circuits the app's soft-validation flow | bug | P2 | S | medium |  |
| [FE-PROV-08](#fe-prov-08) | Invalid Codex/Gemini editor state is silently replaced by stale defaults on save | bug | P2 | S | high |  |
| [FE-PROV-09](#fe-prov-09) | Cards for OpenCode/OpenClaw/Hermes/GrokBuild show "not configured" instead of their endpoint | bug | P2 | S | high |  |
| [FE-PROV-10](#fe-prov-10) | Speed-test endpoint edits are persisted immediately in edit mode, bypassing Cancel | bug | P2 | M | high |  |
| [FE-PROV-11](#fe-prov-11) | OpenClaw/Hermes model editors mis-track expanded rows after deletion and promise reordering that does not exist | bug | P2 | S | high |  |
| [FE-PROV-12](#fe-prov-12) | Duplicate provider uses an English-only " copy" suffix and a colliding sortIndex | bug | P2 | S | high |  |
| [FE-PROV-13](#fe-prov-13) | Drag reorder is not optimistic and every card polls health every 5 s | performance | P2 | S | high |  |
| [FE-PROV-14](#fe-prov-14) | Full-screen panels lack dialog semantics and focus management; icon buttons lack names | ux | P2 | M | high |  |
| [FE-PROV-15](#fe-prov-15) | i18n string concatenation and emoji-in-copy | bug | P2 | S | high |  |
| [FE-PROV-16](#fe-prov-16) | TOML and .env editors use JavaScript syntax highlighting | ux | P2 | S | high |  |
| [FE-PROV-17](#fe-prov-17) | Split `ProviderFormFull` into a shell plus per-app form adapters | refactor | P2 | XL | high |  |
| [FE-PROV-18](#fe-prov-18) | Three near-identical common-config stacks (hooks, modals, dark-mode observers) | refactor | P2 | L | high |  |
| [FE-PROV-19](#fe-prov-19) | Preset identity is array-index based and parsed in six places | refactor | P2 | M | high |  |
| [FE-PROV-20](#fe-prov-20) | OpenCode model editor can persist draft keys and silently drops models on rename collision | bug | P2 | S | high |  |
| [FE-PROV-21](#fe-prov-21) | Base-URL and API-key validation is inconsistent across apps | bug | P2 | M | high |  |
| [FE-PROV-22](#fe-prov-22) | Per-app config knowledge is scattered; `getApiKeyFromConfig` has a wrong Codex shape | refactor | P2 | L | high |  |
| [FE-PROV-23](#fe-prov-23) | Secrets are shown in plain text in editors/previews and may leak into error toasts | security | P2 | M | medium |  |
| [FE-PROV-24](#fe-prov-24) | No unsaved-changes guard or "live config differs" view in the edit dialog | ux | P2 | M | high |  |
| [FE-PROV-25](#fe-prov-25) | `SectionHeader` is declared inside `OmoFormFields` render (remount every render) | bug | P2 | S | high |  |
| [FE-PROV-32](#fe-prov-32) | Test gaps: dialogs tested with the form mocked out; no real `ProviderForm` submit test; brittle source-string test | test | P2 | L | high |  |
| [FE-PROV-41](#fe-prov-41) | Escape inside a nested FullScreenPanel (speed test, common-config, universal form) also closes the parent Add/Edit dialog, discarding all edits | bug | P2 | S | high |  |
| [FE-PROV-42](#fe-prov-42) | Duplicate is enabled for Hermes-managed (read-only) providers and produces an uneditable, undeletable copy | bug | P2 | S | high |  |
| [FE-PROV-43](#fe-prov-43) | Partner star and promotion key are only persisted when the user adds draft custom endpoints | bug | P2 | S | high |  |
| [FE-PROV-44](#fe-prov-44) | Additive-mode duplicate-key guard is client-only and vacuous while the key queries are loading or errored; backend add_provider silently overwrites the existing provider | bug | P2 | S | medium |  |
| [FE-PROV-26](#fe-prov-26) | Unguarded `JSON.parse` of form output in both dialogs | bug | P3 | S | high |  |
| [FE-PROV-27](#fe-prov-27) | First-run "common config" notice fires inside every provider form for every app | ux | P3 | S | high |  |
| [FE-PROV-28](#fe-prov-28) | OpenCode option values are JSON-parsed on every keystroke; keys starting with `option-` render blank | ux | P3 | S | high |  |
| [FE-PROV-29](#fe-prov-29) | Hover-only action bars hide every action from keyboard/touch users | ux | P3 | S | high |  |
| [FE-PROV-30](#fe-prov-30) | Inconsistent terminology across per-app forms and actions | ux | P3 | S | high |  |
| [FE-PROV-31](#fe-prov-31) | GrokBuild re-serialises TOML on every keystroke and re-validates on every render | performance | P3 | S | high |  |
| [FE-PROV-33](#fe-prov-33) | Provider list lacks filters, grouping, tags and bulk operations | feature | P3 | L | high |  |
| [FE-PROV-34](#fe-prov-34) | Provider health dashboard and connectivity history (dead `HealthStatusIndicator`) | feature | P3 | L | high |  |
| [FE-PROV-35](#fe-prov-35) | Quick-switch command palette | feature | P3 | M | high |  |
| [FE-PROV-36](#fe-prov-36) | API-key hygiene: paste normalisation, expiry/rotation reminders, last-used | feature | P3 | M | medium |  |
| [FE-PROV-37](#fe-prov-37) | Universal-provider model is confusing and duplicated; add provider templating instead | ux | P3 | L | medium |  |
| [FE-PROV-38](#fe-prov-38) | Docs gap: no documentation of form architecture, `ProviderMeta` field semantics or per-app `settingsConfig` shapes | docs | P3 | S | high |  |
| [FE-PROV-39](#fe-prov-39) | Dead code and duplicated computations in the form shell | tech-debt | P3 | S | high |  |
| [FE-PROV-40](#fe-prov-40) | Per-project provider binding (profiles) does not cover most apps or expose which provider a profile will apply | feature | P3 | L | medium |  |
| [FE-PROV-45](#fe-prov-45) | Add-mode speed-test drafts are discarded when the panel is reopened; the hook meant to carry them (useCustomEndpoints) is dead code | bug | P3 | S | high |  |
| [FE-PROV-46](#fe-prov-46) | useManagedAuth keeps running after unmount: a device-code login that resolves after the dialog closes opens the browser, writes the clipboard and starts an uncleared polling interval | bug | P3 | S | medium |  |
| [FE-PROV-47](#fe-prov-47) | Universal provider save in the panel misreports a sync failure as a save failure and leaves the list stale, diverging from the AddProviderDialog implementation of the same flow | bug | P3 | S | high |  |
| [FE-PROV-48](#fe-prov-48) | Gemini .env editor comments, blank lines and key order are destroyed by every API-key/base-URL/model keystroke | bug | P3 | S | high |  |
| [FE-PROV-49](#fe-prov-49) | Card action buttons never reflect in-flight mutations, so double-clicks issue concurrent switch/failover/add-to-config calls | bug | P3 | S | medium |  |
| [FE-PROV-50](#fe-prov-50) | OpenClaw model-catalog registration failure after add is swallowed (console.warn only) while the UI reports success | bug | P3 | S | high |  |
| [FE-PROV-51](#fe-prov-51) | Additive-mode cards show 'Add' and hide the default-model button until the live-id query resolves, then flip | ux | P3 | S | high |  |
| [FE-PROV-52](#fe-prov-52) | Provider-key field is triplicated (~210 lines, key regex copied 10x) and the 'existing keys' source is owned by three different hooks | refactor | P3 | M | high |  |
| [FE-PROV-53](#fe-prov-53) | No behaviour tests for the ProviderActions/ProviderCard state matrix or ProviderList's additive, failover and Hermes branches | test | P3 | M | high |  |
| [FE-PROV-54](#fe-prov-54) | Show the configured model(s) and API format on provider cards (and include them in search) | feature | P3 | M | medium |  |
| [FE-PROV-55](#fe-prov-55) | Per-provider 'Copy share link' (ccswitch:// deep link) action is missing although the importer exists | feature | P3 | M | medium |  |

## Findings

### FE-PROV-01: JSON-editor edits are silently overwritten by the next field edit (stale `getValues` snapshots)

**Type:** bug · **Priority:** P1 · **Effort:** M · **Confidence:** high

- **Evidence:** `src/components/providers/forms/ProviderForm.tsx:483-498` — `useApiKeyState({ initialConfig: form.getValues("settingsConfig") ... })` and `useBaseUrlState({ settingsConfig: form.getValues("settingsConfig") ... })`; `src/components/providers/forms/ProviderForm.tsx:508-510` — `useModelState({ settingsConfig: form.getValues("settingsConfig") })`; `src/components/providers/forms/ProviderForm.tsx:2413-2416` — Claude JSON editor writes with `form.setValue("settingsConfig", value)` and nothing in the file calls `form.watch("settingsConfig")`/`useWatch` (only `watch("name")` at 2356 and `watch("websiteUrl")` at 1522/1535/...); `src/components/providers/forms/CommonConfigEditor.tsx:56-62` — comment admits "parent uses form.getValues which doesn't trigger re-renders" and mirrors the value locally; `src/components/providers/forms/hooks/useApiKeyState.ts:55-84` — `handleApiKeyChange` rewrites the config from the render-time `initialConfig`; `src/components/providers/forms/hooks/useBaseUrlState.ts:87-108` — same with `settingsConfig`; `src/components/providers/forms/hooks/useModelState.ts:147-150,200-203` — `latestConfigRef.current = settingsConfig` is only refreshed on render.
- **Problem:** `form.setValue` on an unwatched field does not re-render `ProviderFormFull` (react-hook-form 7.65, `package.json:87`), so after the user edits the Claude JSON editor (e.g. adds env vars or pastes a full config) the API-key/base-URL/model hooks still hold the pre-edit string. The next keystroke in the API Key, endpoint or model inputs calls `setApiKeyInConfig(staleConfig, key)` and pushes that back into the form, discarding the JSON edits; the editor then visibly reverts because `CommonConfigEditor` re-syncs from the prop. Soft validation (`ProviderForm.tsx:1157-1200`) also reads the stale `apiKey`/`baseUrl` state and can warn "API Key missing" when the JSON already contains it.
- **Proposal:** Make the config draft the single reactive source: subscribe with `useWatch({ control, name: "settingsConfig" })` (or move `settingsConfig` out of RHF into a `useReducer` draft) and feed that value to the hooks; have the hooks mutate through an `onChange(updater: (current) => next)` that always reads the latest draft instead of a captured string. Add a regression test: edit JSON editor, then type in API Key, assert JSON edits survive.
- **Acceptance:** RTL test "JSON edit followed by API-key keystroke keeps JSON changes" passes for Claude (and Gemini env/Codex TOML analogues); no `form.getValues("settingsConfig")` is passed as a hook input in `ProviderForm.tsx`.
- **Verification:** **confirmed** — Behaviour verified, though the cited line numbers are offset: the hooks receive render-time snapshots at ProviderForm.tsx:448-455 (`useApiKeyState({ initialConfig: form.getValues("settingsConfig") ... })`), 457-464 (`useBaseUrlState({ settingsConfig: form.getValues("settingsConfig") ... })`) and 478-481 (`useModelState`), the Claude editor writes with a bare `onChange={(value) => form.setValue("settingsConfig", value)}` at 2465-2467, `handleSettingsConfigChange` (410-415) is also a bare `setValue`, only `isSubmitting` is subscribed from `formState` (408) and the only `watch` calls are `name`/`websiteUrl` (1582-1649, 2342). Installed react-hook-form 7.65.0 (`node_modules/react-hook-form/dist/index.esm.mjs:1676-1707`) emits only `{name, values}` for an unwatched `setValue`, which fails every branch of `shouldRenderFormState` (1052-1059), so `ProviderFormFull` does not re-render after a JSON edit; `useApiKeyState.ts:55-84` then rewrites from the stale `initialConfig`, `useBaseUrlState.ts:87-109` from the stale `settingsConfig`, and `useModelState.ts:147-150,200-203` from a ref refreshed only on render, while `CommonConfigEditor.tsx:56-62` re-syncs `localValue` from the prop on the next render, producing the visible revert. `ApiKeyInput.tsx:47` and `EndpointField.tsx:99` fire per keystroke, soft validation at ProviderForm.tsx:1197-1210 reads the hook state `apiKey`/`baseUrl`, and `onSubmit={form.handleSubmit(handleSubmit)}` (1861) captures the render-time closure, so the spurious 'API Key required' warning is also real. No test covers this (tests/components/CommonConfigEditor.test.tsx tests toggles in isolation; the dialog tests mock ProviderForm), and nothing in the codebase implements the proposal; one detail: Codex config lives in React state in useCodexConfigState and `useBaseUrlState` is given `codexConfig: ""` (461-463), so the 'Codex TOML analogue' in the acceptance is probably not needed. P1 is proportionate because the discard is deterministic and sits in the main Claude add/edit flow (API key field is shown whenever a preset is selected or in edit mode for non-official categories).

### FE-PROV-06: OMO custom agent/category rows vanish before a model is chosen

**Type:** bug · **Priority:** P1 · **Effort:** S · **Confidence:** high

- **Evidence:** `src/components/providers/forms/OmoFormFields.tsx:351-357` — `customAgents`/`customCategories` are re-derived from `agents`/`categories` on every prop change; `src/components/providers/forms/OmoFormFields.tsx:1151-1165` — "Custom" adds an unkeyed row only to local state; `src/components/providers/forms/OmoFormFields.tsx:254-302` — `mergeCustomModelsIntoStore` drops entries without a model (293-299); `src/components/providers/forms/OmoFormFields.tsx:92-121` — `DeferredKeyInput` commits only on blur.
- **Problem:** Add a custom agent, then change any built-in agent's model (or import a local file): `onAgentsChange` fires, the effect rebuilds `customAgents` from the store, and the half-filled custom row disappears. Rows with a key but no model are likewise not persisted and vanish on the next store change.
- **Proposal:** Keep custom rows as explicit local draft state with a `draftId`, merge the store-derived rows with drafts instead of replacing (`useEffect` should only add/remove rows whose keys appeared/disappeared in the store), and persist a keyed row even when the model is empty (or show an inline "select a model" hint).
- **Acceptance:** Test: click Custom, change a builtin model, custom row still rendered; after choosing a model the entry is in `agents`.
- **Verification:** **confirmed** — Confirmed, and the impact is broader than described: src/components/providers/forms/OmoFormFields.tsx:351-357 re-derives `customAgents`/`customCategories` from the store via `collectCustomModels` (236-251, which only yields keys present in the store) on every `agents`/`categories` identity change, `addCustomModel` at 1151-1165 adds `{ key: "", model: "", sourceKey: "" }` to local state only, and a builtin model change at 516-535 calls `setter({ ...store, [key]: nextEntry })` (i.e. `onAgentsChange`, wired to `omoDraft.setOmoAgents` at ProviderForm.tsx:2279), so the unkeyed row is dropped exactly as claimed. Worse, `updateCustom` at 993-998 always calls `syncCustoms(next)`, and `mergeCustomModelsIntoStore` (258-302) skips `!targetKey` rows (272) and deletes keyed rows with no model and no other fields (293-300), so the resulting new store object triggers the same effect and the row vanishes on its own first key blur (`DeferredKeyInput` 92-121) or first model select, meaning a new custom row cannot be completed through the UI in either order. The local-file import case (852-861) also replaces rows but is by design (toast says `replaced`). No test covers OmoFormFields (no *.test.tsx under src/components/providers). The draft-merging part of the proposal is sound; the alternative of persisting a keyed row with an empty model would write `{}` agent entries into the OMO profile that `mergeCustomModelsIntoStore` deliberately drops, so keeping drafts local until key and model are both set is preferable. P1 stands (feature effectively unusable for new rows, no persisted-data loss).

### FE-PROV-02: Edit dialog backfills live config after the form is already editable, resetting user input

**Type:** bug · **Priority:** P2 · **Effort:** M · **Confidence:** high

- **Evidence:** `src/components/providers/EditProviderDialog.tsx:45-132` — live settings are fetched asynchronously (`getCurrent` then `getLiveProviderSettings` / `openclawApi.getLiveProvider`) after open; `src/components/providers/EditProviderDialog.tsx:134-179` — `initialSettingsConfig` and `initialData` are recomputed when `liveSettings` arrives; `src/components/providers/forms/ProviderForm.tsx:368-389` — `defaultValues` memo depends on `initialData`; `src/components/providers/forms/ProviderForm.tsx:637-639` — `useEffect(() => form.reset(defaultValues), [defaultValues, form])`; `src/components/providers/forms/ProviderForm.tsx:335-365` — second effect resets ~10 state slices on `initialData` change; `src/components/providers/forms/hooks/useCodexConfigState.ts:51-129` — Codex auth/config/catalog re-initialised on `[initialData]`.
- **Problem:** The form mounts immediately with DB data (`provider.settingsConfig`), the user can start typing, and when the live read resolves every RHF field and most per-app state is reset to the live values. On slow IPC (or when the app is current and two sequential calls run) this discards edits; it also causes a visible content swap. There is no loading gate (`hasLoadedLive` is only used to prevent re-reads).
- **Proposal:** Gate rendering of `ProviderForm` on `hasLoadedLive` (show the existing skeleton/spinner), or key the form on a `loadedRevision` so it mounts once with final data; alternatively resolve live settings in `App.tsx` before opening the dialog. Keep `isProxyTakeover`/OpenCode short-circuits.
- **Acceptance:** Test with a delayed `getLiveProviderSettings` mock: typing before resolution is preserved (or the form is not interactive until loaded); no `form.reset` fires after first user input.
- **Verification:** **partially-confirmed** — Core observation holds: EditProviderDialog.tsx:241 renders `ProviderForm` immediately with `initialData` built from `provider.settingsConfig` (134-179), the live read resolves later in the effect at 100-117, `hasLoadedLive` is only consulted inside the effect (line 55) and never gates rendering, App.tsx:994-996 opens the dialog with a bare `setEditingProvider(provider)`, and when `liveSettings` arrives the new `initialData` triggers `form.reset(defaultValues)` (ProviderForm.tsx:649-651), the ten-slice state reset (340-376) and the Codex re-init on `[initialData]` (useCodexConfigState.ts:51-129), all unconditionally. The detail that is off: the backfill only fires for the app's current provider (`provider.id === currentId`, line 102) or any OpenClaw provider (80-98); for every other provider `liveSettings` stays null and no reset occurs, and the edit-discard requires typing within the window of two sequential Tauri invokes, which is tens of milliseconds on a desktop app. The realistic, reproducible symptom is the visible content swap (and a reset even when live deep-equals DB, since `initialSettingsConfig` is a new object), so P2 rather than P1. The proposal (gate on `hasLoadedLive` or key the form on a load revision) is sound and not implemented; tests/components/EditProviderDialog.test.tsx mocks ProviderForm entirely so it cannot catch this.

### FE-PROV-03: Validation messages are hard-coded Chinese/English literals outside i18n

**Type:** bug · **Priority:** P2 · **Effort:** S · **Confidence:** high

- **Evidence:** `src/lib/schemas/provider.ts:38-54` — zod messages `"请输入有效的网址"`, `"请填写配置内容"` and `parseJsonError` (lines 6-36) producing `"JSON 格式错误：…"`; `src/i18n/locales/en.json:1076` has `fillConfigContent` but the schema never uses it; `src/utils/providerConfigUtils.ts:73-89,103` — `validateJsonConfig` default field name `"配置"` and `"配置 JSON 解析失败…"`; `src/components/providers/forms/hooks/useCodexConfigState.ts:172-183` — `"Auth JSON must be an object"`, `"Invalid JSON format"`; `src/components/providers/forms/hooks/useGeminiConfigState.ts:122-133` — same English literals; `src/components/providers/forms/hooks/useCodexTomlValidation.ts:29-31` — `"TOML 格式错误"`; `src/utils/grokBuildConfig.ts:133-161` — English `"Missing api_key or env_key"` etc.; `src/components/providers/forms/hooks/useCommonConfigSnippet.ts:261,340` — `"通用配置片段"`, `"提取的配置"`.
- **Problem:** Users see Chinese zod errors on an English UI (and English helper errors on a Chinese UI). Error strings are produced in utility modules with no access to `t`, so they cannot be localised at the call site either.
- **Proposal:** Return error *codes* (e.g. `{ code: "invalidJson", position }`) from utilities and map to i18n keys at render; build the zod schema via `createProviderSchema(t)` (memoised in the form) or use `z.config({ customError })` with keys. Add `validation.*` keys to all four locales.
- **Acceptance:** `rg "[\x{4e00}-\x{9fff}]" src/lib/schemas src/utils/providerConfigUtils.ts src/utils/grokBuildConfig.ts src/components/providers/forms/hooks` returns no user-facing literals; switching locale changes every validation message.
- **Verification:** **confirmed** — Every cited literal is present: src/lib/schemas/provider.ts:8,17,25,35 (`JSON 格式错误：...`), :40 (`.url("请输入有效的网址")`), :44 (`.min(1, "请填写配置内容")`); providerConfigUtils.ts:72-89 (`fieldName = "配置"`, `${fieldName}JSON格式错误，请检查语法`) and :101-103; useCodexConfigState.ts:176,180 and useGeminiConfigState.ts:128,130 (English literals); useCodexTomlValidation.ts:29-31; grokBuildConfig.ts:133-161; useCommonConfigSnippet.ts:261,340 (which passes `"通用配置片段"`/`"提取的配置"` even though `t` is in scope there). No mitigation exists: `fillConfigContent` appears only in the four locale files and nowhere in code, there is no `setErrorMap`/`z.config`/schema factory under src/lib/schemas or src/utils, `FormMessage` (src/components/ui/form.tsx:136-137) renders `error.message` raw, and `CommonConfigEditor.tsx:192-194,344-346` prints `commonConfigError` verbatim. One nuance: GrokBuildProviderForm.tsx:328-333 wraps the Grok error in `t("grokBuild.invalidToml", { error })`, so only the inner message is untranslated there. The proposal is sound and not implemented (zod 4.1.12 is installed, so `z.config({ customError })` is available; `grokBuildConfig.test.ts:86-100` asserts the literal strings and would need updating), but this is an i18n/UX defect with no data or security impact, so P2.

### FE-PROV-04: Switching to the "Universal" tab in Add Provider unmounts the form and loses all input

**Type:** bug · **Priority:** P2 · **Effort:** S · **Confidence:** high

- **Evidence:** `src/components/providers/AddProviderDialog.tsx:364-391` — `ProviderForm` is rendered inside `<TabsContent value="app-specific">`; `src/components/ui/tabs.tsx:37-50` — thin wrapper over Radix `TabsPrimitive.Content` without `forceMount`, which unmounts inactive content; `src/components/providers/AddProviderDialog.tsx:313-353` — footer swaps between the two tabs' actions.
- **Problem:** A user who fills the app-specific form, clicks "Universal" to look, and comes back finds an empty form (every `useState` in `ProviderFormFull` and RHF state is re-created). The two tabs also have unrelated semantics (a form vs. a management panel), which makes the accidental switch likely.
- **Proposal:** Keep the form mounted (`forceMount` + `hidden` on inactive content), or move the Universal panel out of the Add dialog into its own entry point (Settings or a dedicated panel) and keep only a "Create from universal preset" shortcut inside the form. Warn on tab switch if the form is dirty.
- **Acceptance:** Test: type a name, switch tab and back, name persists. No `ProviderForm` remount on tab change (spy on an effect).
- **Verification:** **confirmed** — Verified: AddProviderDialog.tsx:363-391 renders `ProviderForm` inside `<TabsContent value="app-specific">` and `UniversalProviderPanel` inside `<TabsContent value="universal">`, with the footer swapping at 313-353; src/components/ui/tabs.tsx:37-50 forwards props to `TabsPrimitive.Content` without `forceMount`, and `forceMount` appears nowhere in src. Installed @radix-ui/react-tabs 1.1.13 (`dist/index.mjs:157-175`) renders `Presence present={forceMount || isSelected}` with `children: present && children`, so the inactive tab's subtree unmounts and all `useState`/RHF state in `ProviderFormFull` is recreated on return. The tab is shown for claude, codex, gemini and other apps except opencode/openclaw/hermes/grokbuild/claude-desktop (48-53); tests/components/AddProviderDialog.test.tsx mocks ProviderForm and never switches tabs. The proposal is accurate (with `forceMount` Radix sets `hidden: !present` which is false, so an explicit `hidden` on the inactive content is indeed required). Priority lowered to P2: it loses only unsaved form input on a deliberate tab click and is recoverable by retyping, with no persisted-data or security impact.

### FE-PROV-05: Hermes provider-key rename is accepted by the form but ignored on save

**Type:** bug · **Priority:** P2 · **Effort:** S · **Confidence:** high

- **Evidence:** `src/components/providers/forms/ProviderForm.tsx:1357-1360` — `payload.providerKey = hermesForm.hermesProviderKey`; `src/components/providers/forms/ProviderForm.tsx:2026-2093` — Hermes key input is editable whenever `!isProviderKeyLocked`; `src/components/providers/EditProviderDialog.tsx:191-195` — `nextProviderId` only honours `values.providerKey` for `"opencode" || "openclaw"`, so Hermes keeps `provider.id`; backend supports renames via `original_id` (`src-tauri/src/services/provider/mod.rs:2141-2222`).
- **Problem:** Editing a Hermes provider's key shows a successful save but the id is unchanged; the duplicate-key check in `ProviderForm.tsx:1082-1104` and the "locked" hint suggest renaming is supported. OpenCode/OpenClaw behave differently from Hermes for the same field.
- **Proposal:** Include `"hermes"` in the `nextProviderId` condition (or better, let the form adapter decide `id` so the dialog has no app list). Add a test for the rename path per additive app.
- **Acceptance:** Editing a DB-only Hermes provider's key results in `updateProvider({ provider: { id: newKey }, originalId: oldKey })`.
- **Verification:** **confirmed** — Confirmed: src/components/providers/EditProviderDialog.tsx:191-195 computes `nextProviderId = (appId === "opencode" || appId === "openclaw") && values.providerKey?.trim() ? values.providerKey.trim() : provider.id`, so for Hermes the dialog always submits `id === originalId` (lines 211-214) and the backend takes the non-rename branch, while src/components/providers/forms/ProviderForm.tsx:1383-1384 sets `payload.providerKey = hermesForm.hermesProviderKey`, the input at 2019-2033 is only `disabled` when `isProviderKeyLocked` (971-981: id present in `hermesLiveProviderIds`), and the duplicate check at 1133-1137 is gated on `!isProviderKeyLocked`. The backend really supports the rename for Hermes: src-tauri/src/app_config.rs:404-409 includes Hermes in `is_additive_mode()` and src-tauri/src/services/provider/mod.rs:2145-2222 renames whenever the original id is not in live config; the add path is also inconsistent with edit, since src/components/providers/AddProviderDialog.tsx:153-158 and src/lib/query/mutations.ts:58-70 honour `providerKey` for Hermes. DB-only (editable-key) Hermes providers are realistic because duplicate sets `addToLive = false` (src/App.tsx:770) and `removeFromLiveConfig` exists (src/App.tsx:663), even though a plain add defaults to live (src-tauri/src/commands/provider.rs: `addToLive.unwrap_or(true)`); no test in scope covers the rename (only src/utils/*.test.ts exist under the reviewed paths). The cited line numbers drift by ~25 lines (actual 1383-1384 and 2012-2083 rather than 1357-1360 and 2026-2093) but the content holds, and the proposal (add `hermes` to the condition, mirroring AddProviderDialog) is sound and not implemented. Priority lowered to P2: the save is a silent no-op on a niche flow with no data loss and a delete/re-add or duplicate workaround.

### FE-PROV-07: Native `required` attribute short-circuits the app's soft-validation flow

**Type:** bug · **Priority:** P2 · **Effort:** S · **Confidence:** medium

- **Evidence:** `src/components/providers/forms/ClaudeFormFields.tsx:650-663` — template inputs have `required`; `src/components/providers/forms/ProviderForm.tsx:1861-1865` — `<form id="provider-form">` without `noValidate` (grep for `noValidate` in `src/` returns nothing); `src/components/providers/forms/ProviderForm.tsx:1021-1029,2467-2515` — the soft-issue confirm dialog for missing template values; `src/components/providers/AddProviderDialog.tsx:326-329` — the submit button lives in the footer outside the form (`form="provider-form"`).
- **Problem:** The browser's native validation bubble fires first (in the WebView's locale, unstyled) and prevents `handleSubmit` from running, so the designed "save anyway" confirmation can never appear for template presets (Bedrock etc.); the bubble can target an input scrolled out of view.
- **Proposal:** Add `noValidate` to the three provider forms (`ProviderForm`, `ClaudeDesktopProviderForm`, `GrokBuildProviderForm`), drop `required` from template inputs (keep `aria-required`), and rely on the existing soft validation.
- **Acceptance:** Submitting with an empty template value opens the soft-validation ConfirmDialog.

### FE-PROV-08: Invalid Codex/Gemini editor state is silently replaced by stale defaults on save

**Type:** bug · **Priority:** P2 · **Effort:** S · **Confidence:** high

- **Evidence:** `src/components/providers/forms/ProviderForm.tsx:1272-1327` — on `JSON.parse(codexAuth)` failure, `settingsConfig = values.settingsConfig.trim()`; `src/components/providers/forms/ProviderForm.tsx:1328-1339` — same fallback for Gemini `config`; `src/components/providers/forms/ProviderForm.tsx:368-389` — for Codex the RHF `settingsConfig` is `CODEX_DEFAULT_CONFIG` (`{auth:{},config:""}`) in add mode or the *initial* JSON in edit mode; `src/components/providers/forms/ProviderForm.tsx:2447-2456` — submit only disabled on `isSubmitting`, `codexAuthError`/`codexConfigError`/`geminiConfigError` never block.
- **Problem:** A typo in auth.json or Gemini config.json makes the form save either an empty config (add) or the pre-edit config (edit) with a success toast; all TOML/catalog edits made in that session are lost without any message.
- **Proposal:** Block submit (and show the error near the editor) when any editor error is set; remove the silent fallbacks and surface a toast instead.
- **Acceptance:** With invalid auth.json, clicking Save shows an error and does not call `onSubmit`.

### FE-PROV-09: Cards for OpenCode/OpenClaw/Hermes/GrokBuild show "not configured" instead of their endpoint

**Type:** bug · **Priority:** P2 · **Effort:** S · **Confidence:** high

- **Evidence:** `src/components/providers/ProviderCard.tsx:106-136` — `extractApiUrl` only reads `env.ANTHROPIC_BASE_URL`, `env.GOOGLE_GEMINI_BASE_URL` and the Codex TOML `config`; OpenCode stores `options.baseURL` (`src/components/providers/forms/helpers/opencodeFormUtils.ts:35-47`), OpenClaw `baseUrl` (59-68), Hermes `base_url` (`src/components/providers/forms/hooks/useHermesFormState.ts:21-25`), GrokBuild `[model.<profile>].base_url` (`src/utils/grokBuildConfig.ts:52-58`); `src/components/providers/AddProviderDialog.tsx:237-281` contains the correct per-app extraction but is not shared; `src/components/providers/ProviderCard.tsx:107-109` — notes replace the endpoint line entirely.
- **Problem:** Custom providers for four apps display the fallback text unless `websiteUrl` happens to be set; adding a note hides the endpoint for every app. The same per-app knowledge is duplicated in the dialog.
- **Proposal:** Add `getProviderEndpoint(appId, provider)` to `providerConfigUtils` (used by card, AddProviderDialog, speed-test seeding) and render notes on a separate line.
- **Acceptance:** An OpenCode provider with `options.baseURL` shows that URL on its card; AddProviderDialog uses the shared helper.

### FE-PROV-10: Speed-test endpoint edits are persisted immediately in edit mode, bypassing Cancel

**Type:** bug · **Priority:** P2 · **Effort:** M · **Confidence:** high

- **Evidence:** `src/components/providers/forms/EndpointSpeedTest.tsx:393-443` — `handleSave` in edit mode (`isEditMode = Boolean(providerId)`, line 117) calls `vscodeApi.addCustomEndpoint`/`removeCustomEndpoint` one by one; `src/components/providers/forms/EndpointSpeedTest.tsx:201-216` — add mode instead defers via `onCustomEndpointsChange`; `src/components/providers/forms/ProviderForm.tsx:2066-2068,2187-2189` — `onCustomEndpointsChange` is `undefined` in edit mode.
- **Problem:** The inner panel's "Save" writes to the database even if the user then cancels the outer Edit Provider dialog; a failure mid-loop leaves a partially applied set; add and edit modes behave differently for the same control.
- **Proposal:** Treat endpoints as part of the form draft in both modes (`meta.custom_endpoints` built in `performSubmit`), or add a batch `setCustomEndpoints` command and label the inner button "Apply".
- **Acceptance:** Cancelling the edit dialog after changing endpoints leaves `getCustomEndpoints` unchanged.

### FE-PROV-11: OpenClaw/Hermes model editors mis-track expanded rows after deletion and promise reordering that does not exist

**Type:** bug · **Priority:** P2 · **Effort:** S · **Confidence:** high

- **Evidence:** `src/components/providers/forms/OpenClawFormFields.tsx:91-93,162-173` — `expandedModels` keyed by numeric index; deleting row *i* deletes `updated[i]` but later indices shift; `src/components/providers/forms/HermesFormFields.tsx:160-162,252-262` — identical pattern; `src/components/providers/forms/OpenClawFormFields.tsx:663-667` — hint says drag or reorder changes the default model, `341-348` first row is "primary", yet no reorder control exists.
- **Problem:** After removing a model, the advanced panel opens on the wrong row; users cannot change which model is primary without deleting and re-adding entries.
- **Proposal:** Key expansion state by the stable `modelKeys[index]`; add move-up/move-down (or dnd-kit, already a dependency) for model rows; fix the hint.
- **Acceptance:** Delete first model with second expanded; second stays expanded. A "move up" action changes `models[0]`.

### FE-PROV-12: Duplicate provider uses an English-only " copy" suffix and a colliding sortIndex

**Type:** bug · **Priority:** P2 · **Effort:** S · **Confidence:** high

- **Evidence:** `src/App.tsx:718` — `name: \`${provider.name} copy\``; `src/components/universal/UniversalProviderPanel.tsx:175` — same; `src/App.tsx:711-712` — `sortIndex: provider.sortIndex + 1` without renumbering; `src/hooks/useDragSort.ts:27-41` — equal `sortIndex` falls back to `createdAt`/`name`.
- **Problem:** Non-English users get an English suffix; the copy shares its index with the next provider so it lands in an unexpected position and the next drag writes a full renumbering anyway.
- **Proposal:** Add `provider.copySuffix` to locales; after insert call `updateSortOrder` with the renumbered list (or let the backend insert-after).
- **Acceptance:** Duplicating provider at position 2 places the copy at position 3 deterministically; suffix is localised.

### FE-PROV-13: Drag reorder is not optimistic and every card polls health every 5 s

**Type:** performance · **Priority:** P2 · **Effort:** S · **Confidence:** high

- **Evidence:** `src/hooks/useDragSort.ts:53-112` — awaits `updateSortOrder`, two `invalidateQueries`, `updateTrayMenu`, then a success toast on every drop; no `setQueryData`; `src/components/providers/ProviderCard.tsx:176` — `useProviderHealth(provider.id, appId)` per card; `src/lib/query/failover.ts:12-20` — `refetchInterval: 5000`, `retry: false`; the badge renders only when `isProxyRunning && isInFailoverQueue && health` (`ProviderCard.tsx:442-447`).
- **Problem:** With N providers the list snaps back to the old order until the refetch completes (visible jump) and N IPC calls fire every 5 s even when the proxy is off; the toast is noise.
- **Proposal:** Optimistically reorder via `queryClient.setQueryData(["providers", appId], ...)` and roll back on error; enable the health query only when `isProxyRunning && isInFailoverQueue`; drop the success toast.
- **Acceptance:** No health IPC when proxy is stopped; reorder renders instantly in a test with a delayed `updateSortOrder` mock.

### FE-PROV-14: Full-screen panels lack dialog semantics and focus management; icon buttons lack names

**Type:** ux · **Priority:** P2 · **Effort:** M · **Confidence:** high

- **Evidence:** `src/components/common/FullScreenPanel.tsx:86-166` — portal `motion.div` without `role="dialog"`, `aria-modal`, `aria-labelledby`, focus trap or focus restore (grep for `role="dialog"|aria-modal|FocusScope|FocusTrap` in `src/components/common` and `src/components/providers` returns nothing); `src/components/common/FullScreenPanel.tsx:128-137` — back button has only an icon; `src/components/providers/ProviderActions.tsx:308-338,355-367` — Test/Usage/Delete buttons are "disabled" via opacity but remain focusable/clickable (no `disabled`/`aria-disabled`), names via `title` only; `src/components/providers/ProviderCard.tsx:547-565` — expand button lacks `aria-expanded`/`type="button"`; `src/components/providers/forms/EndpointSpeedTest.tsx:564-577,624-633` — rows are clickable `div`s and the remove button has no label; `aria-label` count across `src/components/providers` is 19 occurrences in 9 files.
- **Problem:** Keyboard users can tab out of the modal into the page behind it, focus is not returned on close, and screen readers get no modal context or button names.
- **Proposal:** Implement `FullScreenPanel` on Radix `Dialog` (or wrap in `FocusScope` + `RemoveScroll`), add `aria-labelledby` to the title, `aria-label` to the back button; add `aria-label`/`aria-disabled` to icon buttons; make endpoint rows `button`s.
- **Acceptance:** axe (or `vitest-axe`) run on Add/Edit dialogs reports no critical violations; Tab cycles inside the panel; Esc returns focus to the opener.

### FE-PROV-15: i18n string concatenation and emoji-in-copy

**Type:** bug · **Priority:** P2 · **Effort:** S · **Confidence:** high

- **Evidence:** `src/components/providers/AddProviderDialog.tsx:370` — `{t(\`apps.${appId}\`)} {t("provider.tabProvider")}`; `src/components/providers/forms/EndpointSpeedTest.tsx:492` — `{entries.length} {t("endpointTest.endpoints")}` (no plural); `src/components/providers/forms/ProviderForm.tsx:2473-2481` — bullet list built with `"• "` + `\n\n` + hint; `src/App.tsx:750-753` — `t(...) + ": " + errorMessage`; `src/components/providers/HealthStatusIndicator.tsx:47` — `` ` (${responseTimeMs}ms)` ``; `src/components/providers/FailoverPriorityBadge.tsx:31` — `P{priority}`; `src/components/providers/forms/ProviderPresetSelector.tsx:205-236`, `shared/EndpointField.tsx:46-49`, `shared/ApiKeySection.tsx:79` — "💡" baked into copy.
- **Problem:** Word order, pluralisation and punctuation cannot be localised; emoji renders inconsistently across platforms and screen readers announce it.
- **Proposal:** Replace with interpolated keys (`provider.tabProviderFor` with `{{app}}`, `endpointTest.endpointCount` with `count`), render lists as `<ul>`, move icons out of strings.
- **Acceptance:** No JSX text node combining two `t()` calls or a number with a `t()` call in the provider UI.

### FE-PROV-16: TOML and .env editors use JavaScript syntax highlighting

**Type:** ux · **Priority:** P2 · **Effort:** S · **Confidence:** high

- **Evidence:** `src/components/providers/forms/CodexConfigSections.tsx:362-370` — config.toml via `JsonEditor language="javascript"`; `src/components/providers/forms/CodexCommonConfigModal.tsx:133-143` — same; `src/components/providers/forms/GrokBuildProviderForm.tsx:531-539` — same; `src/components/providers/forms/GeminiConfigSections.tsx:95-105` — `.env` as JavaScript; `src/components/JsonEditor.tsx:22,138-151` — `language: "json" | "javascript"` only; `package.json:43-49` — no TOML/properties mode installed; TOML errors come from a 500 ms debounced hook (`useCodexTomlValidation.ts:40-53`) rendered as a paragraph instead of inline lint markers.
- **Problem:** Wrong tokenisation (strings/sections mis-coloured, `#` comments not recognised), no inline error positions for TOML, misleading component name.
- **Proposal:** Add `@codemirror/legacy-modes` (`toml`, `properties`) via `StreamLanguage`, rename `JsonEditor` to `CodeEditor` with `language: "json" | "toml" | "dotenv" | "javascript"`, and wire TOML parse errors into CodeMirror's `linter`.
- **Acceptance:** Codex config editor highlights `[model_providers.x]` headers and `#` comments; a TOML error is underlined at its line.

### FE-PROV-17: Split `ProviderFormFull` into a shell plus per-app form adapters

**Type:** refactor · **Priority:** P2 · **Effort:** XL · **Confidence:** high

- **Evidence:** `src/components/providers/forms/ProviderForm.tsx` is 2578 lines; it unconditionally mounts every app's hooks (`useCodexConfigState` 548-566, `useGeminiConfigState` 735-755, `useOpencodeFormState` 866-872, `useOpenclawFormState` 886-892, `useHermesFormState` 894-898, `useOmoModelSource` 851-856, three common-config hooks 696-800, six `useApiKeyLink` calls 1517-1585 each with `form.watch("websiteUrl")`), ~40 `useState` slices (295-558), app branches in `handleSubmit` (1000-1105, provider-key validation triplicated), `performSubmit` (1134-1480, nested ternaries for every `ProviderMeta` field), `handlePresetChange` (1613-1830), and JSX (1893-2093: the provider-key input block is copy-pasted three times, ~65 lines each, with the regex `/^[a-z0-9]+(-[a-z0-9]+)*$/` inlined nine times). `ClaudeDesktopProviderForm.tsx` (1175 lines) and `GrokBuildProviderForm.tsx` (576) already escaped via the dispatcher at 240-251. `useBaseUrlState` is mounted with `codexConfig: ""` and a no-op callback (493-498) and carries dead Gemini/Codex branches (`useBaseUrlState.ts:54-84,111-159`); `useCustomEndpoints` is exported but has no consumer (`hooks/index.ts:7`).
- **Problem:** Every new app adds branches to the shell; state for six apps is allocated per form; the submit pipeline is impossible to unit-test per app; bugs like FE-PROV-05 arise because knowledge is scattered across dialog, form and hooks.
- **Proposal:** Introduce `ProviderFormShell` (preset selector, basic fields, soft-validation/confirm pipeline, pricing, submit buttons) and a `ProviderFormAdapter` interface per app: `useDraft(initialData, preset) -> Draft`, `Fields` component, `validate(draft) -> { hard: Issue[]; soft: Issue[] }`, `serialize(draft) -> { settingsConfig: object; meta: Partial<ProviderMeta>; providerKey?: string }`, `applyPreset(preset)`. Register adapters in `adapters/{claude,codex,gemini,opencode,omo,openclaw,hermes,claudeDesktop,grokbuild}.ts`; the shell picks one by `appId` and renders only that adapter's hooks (adapter rendered as a child component so hooks are app-scoped). Extract `ProviderKeyField` (shared by opencode/openclaw/hermes) with `PROVIDER_KEY_PATTERN` in one place. Migrate app by app, starting with hermes/openclaw/opencode (they already have `useXFormState`), then gemini, codex, claude.
- **Acceptance:** `ProviderForm.tsx` < 400 lines with no `appId ===` branches; each adapter has a unit test for `validate` and `serialize`; `useBaseUrlState`/`useCustomEndpoints` removed; per-app integration test (FE-PROV-32) green.

### FE-PROV-18: Three near-identical common-config stacks (hooks, modals, dark-mode observers)

**Type:** refactor · **Priority:** P2 · **Effort:** L · **Confidence:** high

- **Evidence:** `src/components/providers/forms/hooks/useCommonConfigSnippet.ts` (371 lines, JSON), `useCodexCommonConfig.ts` (549, TOML), `useGeminiCommonConfig.ts` (543, env) share the same lifecycle (legacy localStorage migration 62-116 / 120-170 / 158-215, edit-mode init, new-mode init, toggle, snippet change, extract); only the Codex variant has the op-sequence staleness guard (`useCodexCommonConfig.ts:69-88`); modals `CommonConfigEditor.tsx:266-350`, `CodexCommonConfigModal.tsx`, `GeminiCommonConfigModal.tsx` are copies; the `MutationObserver` dark-mode detector is duplicated seven times (`CommonConfigEditor.tsx:39-54`, `CodexConfigSections.tsx:46-61,135-150`, `GeminiConfigSections.tsx:30-45,137-152`, `CodexCommonConfigModal.tsx:32-48`, `GeminiCommonConfigModal.tsx:26-42`) although `useDarkMode` exists (`ProviderForm.tsx:16`).
- **Problem:** Fixes (like the staleness guard) land in one copy only; each form mounts three hooks that each issue a `getCommonConfigSnippet` IPC on mount even for apps that do not use them (`ProviderForm.tsx:696-800`, only the Claude one has `enabled`).
- **Proposal:** One `useCommonConfig({ app, codec })` where `codec = { parse, hasSnippet, apply, remove, isEmpty }` (JSON codec, TOML codec delegating to backend, env codec) and one `CommonConfigModal` taking a `language`; replace observers with `useDarkMode`; mount only for the active app via the adapter (FE-PROV-17).
- **Acceptance:** One hook file and one modal; `getCommonConfigSnippet` called once per form open for the active app only.

### FE-PROV-19: Preset identity is array-index based and parsed in six places

**Type:** refactor · **Priority:** P2 · **Effort:** M · **Confidence:** high

- **Evidence:** `src/components/providers/forms/ProviderForm.tsx:641-678` — ids `\`codex-${index}\``, `\`claude-${index}\`` etc.; `src/components/providers/forms/hooks/useProviderCategory.ts:47-90` — regex `^(claude|codex|gemini|opencode|openclaw|hermes)-(\d+)$` and `parseInt`; `src/components/providers/AddProviderDialog.tsx:134-150,174-235` — `parseInt(values.presetId.replace("codex-", ""))` per app, with no `grokbuild` branch; `src/components/providers/forms/GrokBuildProviderForm.tsx:55-60` — grok entries filter official presets so indices differ from Codex's; `src/components/providers/forms/ClaudeDesktopProviderForm.tsx:354-361`; `payload.presetId` is persisted into provider meta through `AddProviderDialog` (`providerData.meta`).
- **Problem:** Inserting/removing/reordering a preset in a config file silently changes which preset an id refers to; cross-app id collisions are prevented only by prefix conventions; every consumer re-implements parsing.
- **Proposal:** Give each preset a stable `id` slug; add `src/config/presetRegistry.ts` with `getPresets(appId)` and `findPreset(appId, id)`; store the slug; delete the six mapping/parsing copies.
- **Acceptance:** No `parseInt(...replace(` on preset ids; a test asserts preset ids are unique and stable.

### FE-PROV-20: OpenCode model editor can persist draft keys and silently drops models on rename collision

**Type:** bug · **Priority:** P2 · **Effort:** S · **Confidence:** high

- **Evidence:** `src/components/providers/forms/OpenCodeFormFields.tsx:264-270` — new model key is `model-${Date.now()}` with no draft prefix (headers/extra options use `draft-header:`/`draft-option:` and are filtered in `useOpencodeFormState.ts:145-167,179-204`, but `handleOpencodeModelsChange` writes `config.models = models` verbatim at 169-177); `src/components/providers/forms/OpenCodeFormFields.tsx:286-306` — renaming to an existing key overwrites one of the two entries; `src/components/providers/forms/ProviderForm.tsx:1037-1039` — only checks that at least one model exists.
- **Problem:** Saving with an unrenamed row writes `"model-1730000000000"` into opencode.json; renaming two models to the same id loses one with no feedback.
- **Proposal:** Use a draft prefix for models and strip them on save; reject duplicate ids inline (`aria-invalid` + message) and in `validate`.
- **Acceptance:** Saving with an untouched new row does not emit a `model-<timestamp>` key; renaming to an existing id is refused.

### FE-PROV-21: Base-URL and API-key validation is inconsistent across apps

**Type:** bug · **Priority:** P2 · **Effort:** M · **Confidence:** high

- **Evidence:** `src/components/providers/forms/HermesFormFields.tsx:73-102,178-182,306-327` — only Hermes validates URL scheme/host client-side; `src/components/providers/forms/shared/EndpointField.tsx:95-102`, `OpenCodeFormFields.tsx:591-607`, `OpenClawFormFields.tsx:238-254` — plain inputs; `src/components/providers/forms/ProviderForm.tsx:1157-1200` — soft validation checks emptiness only; `src/components/providers/AddProviderDialog.tsx:167-172` — custom endpoints accepted when `url.startsWith("http")`; `src/lib/schemas/provider.ts:40` — `websiteUrl` gets `.url()` but endpoints never do; `src/components/providers/forms/ApiKeyInput.tsx:43-53` — no key sanitation (quotes, `Bearer `, newlines, placeholder literals like the `"your-api-key-here"` shown in placeholders at `CommonConfigEditor.tsx:253-258`).
- **Problem:** Typos (`htp://`, trailing spaces, pasted keys with quotes/newlines) are only discovered at switch/request time; behaviour differs per app for the same field.
- **Proposal:** Shared `validateEndpointUrl(url, { allowTemplate })` and `normalizeApiKey(raw)` (trim, strip quotes/`Bearer `, flag control chars/placeholder values) used by `EndpointField`/`ApiKeyInput` with inline, localised errors and `aria-invalid`; reuse Hermes' implementation.
- **Acceptance:** Every app's endpoint field shows the same inline error for `htp://x`; pasting `"sk-abc"` with quotes stores `sk-abc`.

### FE-PROV-22: Per-app config knowledge is scattered; `getApiKeyFromConfig` has a wrong Codex shape

**Type:** refactor · **Priority:** P2 · **Effort:** L · **Confidence:** high

- **Evidence:** `src/utils/providerConfigUtils.ts:155-200,280-343` — `getApiKeyFromConfig`/`setApiKeyInConfig` read/write `env.CODEX_API_KEY` for `appType === "codex"`, but Codex configs are `{ auth: { OPENAI_API_KEY }, config }` (`useCodexConfigState.ts:21-30,54-65`), so those branches are dead/misleading; endpoint extraction is implemented three times (`ProviderCard.tsx:106-136`, `AddProviderDialog.tsx:237-281`, `useSpeedTestEndpoints.ts:44-156`); "is official" heuristics live in `ProviderCard.tsx:72-104` and `useProviderActions.ts:158-180`; duplication logic in `App.tsx:710-760` re-fetches live ids per app.
- **Problem:** Every consumer re-derives app-specific facts from raw JSON/TOML, so each new app or config-shape change touches many files and the helpers drift (see Codex key path).
- **Proposal:** Introduce a `ProviderCodec` per app (`src/lib/providerCodecs/<app>.ts`): `getEndpoint`, `getApiKey`, `setApiKey`, `getModels`, `isOfficial`, `needsProxy`, `defaultConfig`, `keyPattern`. Cards, dialogs, actions and the form adapters (FE-PROV-17) consume the codec; delete the dead Codex branches.
- **Acceptance:** `rg "CODEX_API_KEY" src/utils` empty; `ProviderCard`, `AddProviderDialog`, `useSpeedTestEndpoints` import `getEndpoint` from the codec; unit tests per codec.

### FE-PROV-23: Secrets are shown in plain text in editors/previews and may leak into error toasts

**Type:** security · **Priority:** P2 · **Effort:** M · **Confidence:** medium

- **Evidence:** `src/components/providers/forms/ApiKeyInput.tsx:43-63` — masked input with reveal; but the same key is visible in the Claude JSON editor (`CommonConfigEditor.tsx:250-263`), Codex `auth.json` editor (`CodexConfigSections.tsx:79-87`), Gemini env editor (`GeminiConfigSections.tsx:95-105`), OMO preview (`ProviderForm.tsx:2383-2394`), universal previews (`UniversalProviderFormModal.tsx:659-697`) and the OpenClaw/Hermes raw JSON editors (`ProviderForm.tsx:2425-2446`); `src/lib/query/mutations.ts:131-139,185-193,316-335` — raw backend error text is toasted (and offered for copy) which can include config fragments.
- **Problem:** Shoulder-surfing/screen-share exposure of API keys whenever an edit panel is open, regardless of the masked field; toast/copy of raw errors can carry secrets.
- **Proposal:** Render secret-bearing editors collapsed by default with a "Show raw config" toggle, mask known secret paths in read-only previews (`sk-…abcd`), and pass errors through a redaction helper before toasting.
- **Acceptance:** Opening Edit Provider shows no plaintext key until the user expands the raw editor; redaction unit test.

### FE-PROV-24: No unsaved-changes guard or "live config differs" view in the edit dialog

**Type:** ux · **Priority:** P2 · **Effort:** M · **Confidence:** high

- **Evidence:** `src/components/common/FullScreenPanel.tsx:60-84,128-137` — Esc/back close immediately; `src/components/providers/EditProviderDialog.tsx:228,241-251` — `onClose` → `onOpenChange(false)` with no dirty check (RHF `formState.isDirty` is never read; most state is outside RHF anyway); `src/components/providers/EditProviderDialog.tsx:100-123` — live settings are fetched but never shown side by side with DB settings; `useProviderActions.ts:268-277` — backfill warning toast after switch indicates live/DB divergence is a real scenario.
- **Problem:** A long form can be lost by one Esc; users cannot see what the live file currently contains vs. the stored provider before saving over it.
- **Proposal:** Track dirtiness in the shell (adapter `isDirty(draft)`), confirm on close; add a "Live differs from saved" banner with a side-by-side diff (reuse the existing JSON editor in read-only mode) and a "Load live into form" action.
- **Acceptance:** Esc on a dirty edit form shows a confirm; diff banner appears when live and DB configs differ.

### FE-PROV-25: `SectionHeader` is declared inside `OmoFormFields` render (remount every render)

**Type:** bug · **Priority:** P2 · **Effort:** S · **Confidence:** high

- **Evidence:** `src/components/providers/forms/OmoFormFields.tsx:1057-1092` — `const SectionHeader = ({...}) => (<button ...>)` inside the component body, used as `<SectionHeader />` at 1112; `src/components/providers/forms/OmoFormFields.tsx:839-888` — `handleImportFromLocal` deps omit `readSlimLocalFile`, `isSlim`, `builtinAgentKeys`.
- **Problem:** React sees a new component type each render, unmounting/remounting the four section headers on every keystroke (focus loss on the header button, extra DOM work); the stale callback can import the wrong (standard vs slim) file after a category change.
- **Proposal:** Hoist `SectionHeader` to module scope; complete the dependency array (or switch to `useEvent`-style ref).
- **Acceptance:** ESLint `react/no-unstable-nested-components` and `react-hooks/exhaustive-deps` clean for this file.

### FE-PROV-32: Test gaps: dialogs tested with the form mocked out; no real `ProviderForm` submit test; brittle source-string test

**Type:** test · **Priority:** P2 · **Effort:** L · **Confidence:** high

- **Evidence:** `tests/components/AddProviderDialog.test.tsx:29-43` and `tests/components/EditProviderDialog.test.tsx:41-90` mock `ProviderForm` entirely (so FE-PROV-01/02/04/05 are untestable there); `tests/components/ProviderForm.codexCatalog.test.ts` only tests `normalizeCodexCatalogModelsForSave`; `tests/components/ProviderCardLayout.test.ts:15-26` asserts Tailwind class strings in the source file; grep shows no tests for `ProviderCard` behaviour, `ProviderActions`, `EndpointSpeedTest`, `OpenClawFormFields`, `HermesFormFields`, `OmoFormFields` interaction (only `mergeCustomModelsIntoStore`), `UniversalProvider*`, `Profile*`; `tests/hooks/useProviderActions.test.tsx:195-198,460,581` assert Chinese default strings; `tests/components/ClaudeFormFields.test.tsx:187-191` queries the button by the Chinese literal "一键设置".
- **Problem:** The highest-risk code (submit pipeline, state sync, dialogs) has no executable specification; tests coupled to zh literals break on copy changes; the layout test breaks on any refactor without catching regressions.
- **Proposal:** Add `tests/integration/providerForm.<app>.test.tsx` rendering the real `ProviderForm` with mocked `@/lib/api`, covering preset select → field edits → JSON edit → submit payload per app; add regressions for FE-PROV-01/02/04/05/06/08/20; replace the source-string test with a render snapshot; assert on i18n keys (mock `t` to return keys) instead of literals.
- **Acceptance:** Coverage report shows `ProviderForm.tsx` and `*FormFields.tsx` > 70% lines; CI test names reference each fixed bug.

### FE-PROV-41: Escape inside a nested FullScreenPanel (speed test, common-config, universal form) also closes the parent Add/Edit dialog, discarding all edits

**Type:** bug · **Priority:** P2 · **Effort:** S · **Confidence:** high

- **Evidence:** src/components/common/FullScreenPanel.tsx:60-84 — every open panel registers its own bubble-phase `window` keydown listener; the handler bails only on `event.defaultPrevented` or `isTextEditableTarget(event.target)` (src/utils/domUtils.ts:1-11 — INPUT/TEXTAREA/SELECT/contentEditable only), then calls `event.stopPropagation()` (which does not stop sibling listeners on the same `window`) and `onClose()`. Listeners are added in open order, so the outer dialog's listener (src/components/providers/EditProviderDialog.tsx:225-228; src/components/providers/AddProviderDialog.tsx:356-360) runs before the inner one. Panels nested inside the provider form: src/components/providers/forms/EndpointSpeedTest.tsx:482-487, src/components/providers/forms/CommonConfigEditor.tsx:266, src/components/providers/forms/CodexCommonConfigModal.tsx:68, src/components/providers/forms/GeminiCommonConfigModal.tsx:62, and src/components/universal/UniversalProviderFormModal.tsx:347 (opened from AddProviderDialog.tsx:404-411). Only Radix dialogs are protected because they preventDefault on Escape.
- **Problem:** With focus on a button or list row (e.g., right after clicking an endpoint row or the speed-test button), pressing Esc fires the Edit/Add dialog's handler first: the whole form unmounts and every unsaved edit is lost, and only afterwards does the inner panel close. Users expect Esc to close the innermost panel only. (Distinct from FE-PROV-24, which is about the missing dirty-check; even with a dirty check the wrong panel would be targeted.)
- **Proposal:** Give FullScreenPanel a module-level open-panel stack (push on open, pop on close) and let a panel act on Escape only when it is the top of the stack; alternatively register in capture phase and call `event.preventDefault()` + `event.stopImmediatePropagation()` so outer panels see `defaultPrevented`. Add a vitest with two nested panels.
- **Acceptance:** With the edit dialog open and the endpoint speed-test panel open, focusing a button and pressing Esc closes only the speed-test panel and the edit form keeps its state; a second Esc closes the dialog. A unit test covers two nested FullScreenPanels.
- **Verification:** added by completeness-critic pass (not independently re-verified)

### FE-PROV-42: Duplicate is enabled for Hermes-managed (read-only) providers and produces an uneditable, undeletable copy

**Type:** bug · **Priority:** P2 · **Effort:** S · **Confidence:** high

- **Evidence:** src/components/providers/ProviderActions.tsx:298-306 — the Duplicate button has no `isReadOnly` gate, while Edit (287-293) and Delete (`canDelete = !isReadOnly && ...` at 223-224, 355-367) are gated; src/components/providers/ProviderCard.tsx:226-227 `isHermesReadOnly = isHermesReadOnlyProvider(provider.settingsConfig)`, which checks the `_cc_source === "providers_dict"` marker inside settingsConfig (src/config/hermesProviderPresets.ts:13-29); src/App.tsx:710-726 deep-clones `settingsConfig` (marker included) into the copy and 763-770 adds it with `addToLive = false`.
- **Problem:** The copy inherits the read-only marker, so it renders the 'Hermes Managed' badge with Edit and Delete disabled: a zombie DB row the UI can never fix or remove. Its only live action is 'Add to config', which pushes a bogus dict-overlay entry into the Hermes config.
- **Proposal:** Pass `isReadOnly` to the Duplicate button (disabled with the managed hint), or strip `_cc_source` from the cloned settingsConfig in `handleDuplicateProvider` so the copy becomes a normal editable custom provider; add a test either way.
- **Acceptance:** For a provider whose settingsConfig contains `_cc_source: "providers_dict"`, Duplicate is disabled with the managed tooltip (or the duplicate has no marker and is editable/deletable). An RTL test for ProviderActions asserts the disabled state.
- **Verification:** added by completeness-critic pass (not independently re-verified)

### FE-PROV-43: Partner star and promotion key are only persisted when the user adds draft custom endpoints

**Type:** bug · **Priority:** P2 · **Effort:** S · **Confidence:** high

- **Evidence:** src/components/providers/forms/ProviderForm.tsx:1411-1451 — `isPartner: true` and `partnerPromotionKey` are merged into `mergedMeta` only inside `if (!isEditMode && draftCustomEndpoints.length > 0)`; `payload.isPartner` (1396-1398) is never read by src/components/providers/AddProviderDialog.tsx:110-311 (it only consumes presetCategory/meta/presetId/providerKey/suggestedDefaults). `draftCustomEndpoints` changes only while EndpointSpeedTest is mounted (src/components/providers/forms/ClaudeFormFields.tsx:703-716, conditional on `isEndpointModalOpen`) and only for `isCustom` entries (src/components/providers/forms/EndpointSpeedTest.tsx:201-216). No Rust setter exists (`is_partner` appears only as struct fields at src-tauri/src/provider.rs:421-429). Consumers: src/components/providers/ProviderCard.tsx:455-465 (partner star) and src/components/providers/forms/GrokBuildProviderForm.tsx:95.
- **Problem:** In the normal flow (pick a partner preset, click Add) `meta.isPartner` and `meta.partnerPromotionKey` are silently dropped, so the partner badge never appears and the promotion key is lost; they only survive if the user happens to open the speed-test panel and add a custom URL. The partner marketing feature is effectively dead.
- **Proposal:** Move the partner/promotion merge out of the endpoint block and apply it to `nextMeta` from `activePreset` unconditionally in add mode; make AddProviderDialog honour `values.isPartner`. Add a test that adds a partner preset without opening the endpoint modal.
- **Acceptance:** Adding any `isPartner` preset without touching endpoints yields `meta.isPartner === true` and `meta.partnerPromotionKey` set; the card shows the partner star; test in tests/components/AddProviderDialog.test.tsx with a real ProviderForm submit.
- **Verification:** added by completeness-critic pass (not independently re-verified)

### FE-PROV-44: Additive-mode duplicate-key guard is client-only and vacuous while the key queries are loading or errored; backend add_provider silently overwrites the existing provider

**Type:** bug · **Priority:** P2 · **Effort:** S · **Confidence:** medium

- **Evidence:** src/components/providers/forms/ProviderForm.tsx:950-969 — `isProviderKeyLockStateLoading` returns `false` whenever `!isEditMode`; 850-858 `opencodeLiveProviderIds` defaults to `[]` while loading or on error, and 905-948 builds `additiveExistingProviderKeys` from that plus `existingOpencodeKeys` (src/components/providers/forms/hooks/useOmoModelSource.ts:48-54), whose query swallows backend errors and returns `{}` (src/lib/query/queries.ts:68-76); 1077-1083 the duplicate toast is the only guard. src/lib/query/mutations.ts:58-73 uses `providerKey` verbatim as the row id; src-tauri/src/services/provider/mod.rs:2088-2105 `ProviderService::add` performs no existence check and src-tauri/src/database/dao/providers.rs:189-238 `save_provider` UPDATEs when `(id, app_type)` exists, then mod.rs:2117-2121 rewrites the live entry.
- **Problem:** If the providers or live-id query is pending or failed (unparsable opencode.json / openclaw.json makes the live-id command error; a DB read error yields `{}`), the add form accepts a key that already exists and `add_provider` overwrites that provider's settings, meta and live entry in place while toasting '供应商已添加' — silent data loss with no undo.
- **Proposal:** In add mode, extend `isProviderKeyLockStateLoading` to cover `isLoading || isError` of both the providers query and the live-id query (block submit with the existing 'status loading' toast and disable the key input), and add a backend guard in `ProviderService::add` that returns a localized error when `(id, app_type)` already exists.
- **Acceptance:** `add_provider` with an existing id returns an error instead of updating; with the live-id query mocked to reject, the add form refuses to submit; a Rust unit test and a vitest cover both.
- **Verification:** added by completeness-critic pass (not independently re-verified)

### FE-PROV-26: Unguarded `JSON.parse` of form output in both dialogs

**Type:** bug · **Priority:** P3 · **Effort:** S · **Confidence:** high

- **Evidence:** `src/components/providers/AddProviderDialog.tsx:112-115` and `src/components/providers/EditProviderDialog.tsx:187-190` — `JSON.parse(values.settingsConfig)` with no try/catch; `ProviderForm.tsx:1272-1368` builds `settingsConfig` strings for codex/gemini/omo outside the zod-validated field.
- **Problem:** A malformed string from any adapter path throws inside RHF's `handleSubmit`, producing an unhandled rejection, no toast, and a dialog stuck open with `isFormSubmitting` possibly true.
- **Proposal:** Have `ProviderForm` emit `settingsConfig` as an object (parsed once, validated) and remove the dialog-side parse; wrap submit in try/catch with a toast.
- **Acceptance:** `onSubmit` receives an object; a thrown error in submit shows a toast and re-enables the button.

### FE-PROV-27: First-run "common config" notice fires inside every provider form for every app

**Type:** ux · **Priority:** P3 · **Effort:** S · **Confidence:** high

- **Evidence:** `src/components/providers/forms/ProviderForm.tsx:275-291,2458-2466` — `ConfirmDialog` driven by `settingsData.commonConfigConfirmed !== true`, both Confirm and Cancel call the same `handleCommonConfigConfirm`; it renders even for OpenCode/OpenClaw/Hermes forms that have no common-config feature (editor branch at 2345-2452).
- **Problem:** The user opens "Add provider" for Hermes and is interrupted by a notice about a feature that form does not have; the dialog has no meaningful cancel.
- **Proposal:** Show the notice only when the active adapter supports common config (claude/codex/gemini), ideally from the common-config toggle itself, or move it to onboarding/Settings.
- **Acceptance:** Opening Add Provider for Hermes with `commonConfigConfirmed` unset shows no notice.

### FE-PROV-28: OpenCode option values are JSON-parsed on every keystroke; keys starting with `option-` render blank

**Type:** ux · **Priority:** P3 · **Effort:** S · **Confidence:** high

- **Evidence:** `src/components/providers/forms/OpenCodeFormFields.tsx:431-450,493-509` — `JSON.parse(value)` per keystroke falling back to string; display re-stringifies non-strings at 1095-1099; `src/components/providers/forms/OpenCodeFormFields.tsx:123-157` — `ModelOptionKeyInput` treats any key starting with `option-` as a placeholder (`displayValue = ""`), and `handleAddModelOption`/`handleAddModelExtraField` create `option-${Date.now()}` keys (388-398,453-460) that are not stripped on save (`useOpencodeFormState.ts:169-177`); the existing test only covers the extra-options draft prefix (`tests/components/OpenCodeFormFields.test.tsx:136-143`).
- **Problem:** Typing `"abc"` becomes `abc`, `1e3` becomes `1000`, `tru` becomes a string then `true` a boolean mid-typing; a legitimate option literally named `option-x` is invisible; unfilled rows persist bogus keys into `models[*].options`.
- **Proposal:** Commit on blur with an explicit "JSON" toggle (or detect only well-formed objects/arrays/numbers/booleans on blur); use the `draft-option:` prefix pattern (contains `:`) for model options/extra fields and filter on save.
- **Acceptance:** Typing `"abc"` keeps the quotes until blur; `option-x` is displayed; saving with an untouched row writes no `option-<timestamp>` key.

### FE-PROV-29: Hover-only action bars hide every action from keyboard/touch users

**Type:** ux · **Priority:** P3 · **Effort:** S · **Confidence:** high

- **Evidence:** `src/components/providers/ProviderCard.tsx:569` — `opacity-0 pointer-events-none group-hover:opacity-100 group-focus-within:opacity-100 ...`; `src/components/universal/UniversalProviderCard.tsx:48` — `opacity-0 group-hover:opacity-100` with no `focus-within`; `src/components/providers/forms/EndpointSpeedTest.tsx:630` — remove button `opacity-0 group-hover:opacity-100`.
- **Problem:** On touch devices and for keyboard users (universal card) the actions are undiscoverable; the main "Enable"/"Add" button is also hidden until hover, which hurts first-time discoverability of the core action.
- **Proposal:** Always show the primary action; put secondary actions in a kebab `DropdownMenu`; keep `focus-within` on all variants.
- **Acceptance:** Primary button visible without hover; actions reachable via Tab on universal cards.

### FE-PROV-30: Inconsistent terminology across per-app forms and actions

**Type:** ux · **Priority:** P3 · **Effort:** S · **Confidence:** high

- **Evidence:** endpoint labels: `OpenCodeFormFields.tsx:593` "Base URL", `OpenClawFormFields.tsx:241` "API 端点", `HermesFormFields.tsx:304` "API 端点", `UniversalProviderFormModal.tsx:413` "API 地址", `ClaudeFormFields.tsx:674` `providerForm.apiEndpoint`, `CodexFormFields.tsx:461` `codexConfig.apiUrlLabel`; model columns: `CodexFormFields.tsx:869-886` "菜单显示名 / 实际请求模型 / 上下文窗口" vs `OpenCodeFormFields.tsx:839-848` "模型 ID / 显示名称" vs `ClaudeFormFields.tsx:892-913` "模型角色 / 显示名称 / 实际请求模型"; main button semantics in `ProviderActions.tsx:122-219,231-260` ("启用/已在用", "添加/移除", "加入/已加入", "设为默认"); provider-key hints differ per app (`ProviderForm.tsx:1956-1966,2023-2031,2076-2088`).
- **Problem:** The same concept has 4-5 names depending on which app tab is active, increasing the learning cost of a tool whose purpose is to make apps interchangeable.
- **Proposal:** Define a terminology table in i18n (`field.endpoint`, `field.apiKey`, `field.modelId`, `field.displayName`, `action.activate` ...) and reuse keys across adapters; document the glossary.
- **Acceptance:** One i18n key per concept used by all forms; glossary page in docs.

### FE-PROV-31: GrokBuild re-serialises TOML on every keystroke and re-validates on every render

**Type:** performance · **Priority:** P3 · **Effort:** S · **Confidence:** high

- **Evidence:** `src/components/providers/forms/GrokBuildProviderForm.tsx:210-224` — every structured field change calls `updateGrokBuildConfig`, which parses and `stringifyToml`s the whole document (`src/utils/grokBuildConfig.ts:79-131`), discarding comments/ordering; `GrokBuildProviderForm.tsx:393` — `validateGrokBuildConfig(rawConfig)` runs on every render; raw editor edits re-parse and push into form state (280-291).
- **Problem:** Comments and formatting in a hand-edited config.toml are lost on the first structured edit; parse work on each render.
- **Proposal:** Memoise validation (`useMemo` on `rawConfig`), and edit TOML with line-preserving helpers like the Codex `setCodexBaseUrl` family (`providerConfigUtils.ts:1257-1373`) or via the backend `toml_edit` command already used for common config.
- **Acceptance:** A comment in config.toml survives changing the base URL field; validation runs once per rawConfig change.

### FE-PROV-33: Provider list lacks filters, grouping, tags and bulk operations

**Type:** feature · **Priority:** P3 · **Effort:** L · **Confidence:** high

- **Evidence:** `src/components/providers/ProviderList.tsx:280-289` — search matches only `name`, `notes`, `websiteUrl`; no category/health/"needs routing" filter; `src/components/providers/ProviderList.tsx:180-189` — failover queue toggled one card at a time; no multi-select, no bulk delete/export; `Provider` has `category` but cards do not group by it (`ProviderList.tsx:371-443` flat list).
- **Problem:** Power users with 20-40 providers across categories have only a flat, drag-sorted list; routine operations (add five providers to the failover queue, delete stale ones, export a subset) are click-heavy.
- **Proposal:** Filter chips (category, in-queue, needs-routing, has-usage-script), optional group-by-category headers, user tags stored in `meta.tags`, and a multi-select mode with bulk add-to-failover / delete / export (reusing the import/export module).
- **Acceptance:** Filter and multi-select render in the list; bulk add-to-queue calls `addToQueue` for each selected id; tags persist in meta.

### FE-PROV-34: Provider health dashboard and connectivity history (dead `HealthStatusIndicator`)

**Type:** feature · **Priority:** P3 · **Effort:** L · **Confidence:** high

- **Evidence:** `src/components/providers/HealthStatusIndicator.tsx` has no importers (grep across `src`); health is only a badge when `isProxyRunning && isInFailoverQueue` (`ProviderCard.tsx:442-447`); connectivity checks are fire-and-forget via `useStreamCheck` (`ProviderList.tsx:94,202-207`) with results surfaced only as toasts; `ProviderHealthBadge.tsx` exposes consecutive failures via `title` only.
- **Problem:** There is no place to see latency/failure history, circuit state or last check per provider, which is the information needed to tune failover order or pick a provider.
- **Proposal:** A "Health" panel (per app) listing providers with last check latency, consecutive failures, circuit state and a "Check all" button, with a small history sparkline persisted by the backend; reuse `HealthStatusIndicator`.
- **Acceptance:** Panel reachable from the provider page; `HealthStatusIndicator` used; check-all triggers `checkProvider` for all non-official providers.

### FE-PROV-35: Quick-switch command palette

**Type:** feature · **Priority:** P3 · **Effort:** M · **Confidence:** high

- **Evidence:** cmdk `Command` is already used (`src/components/profiles/ProfileSwitcher.tsx:16-23,133-205`, `OmoFormFields.tsx:131-226`); global shortcuts exist for search (`ProviderList.tsx:247-268`, `ProviderPresetSelector.tsx:181-193`) but nothing to switch providers without the mouse; `useProviderActions.switchProvider` is the single switch entry point (`useProviderActions.ts:156-318`).
- **Problem:** Switching providers (the app's core action) requires navigating to the app tab, finding the card and hovering to reveal "Enable".
- **Proposal:** `ProviderCommandPalette` (Cmd/Ctrl+K) listing providers of all apps with actions switch / edit / test / add-to-failover, reusing `useProvidersQuery` and `useProviderActions`.
- **Acceptance:** Cmd+K opens the palette; selecting an entry calls `switchProvider` for the right app.

### FE-PROV-36: API-key hygiene: paste normalisation, expiry/rotation reminders, last-used

**Type:** feature · **Priority:** P3 · **Effort:** M · **Confidence:** medium

- **Evidence:** `src/components/providers/forms/ApiKeyInput.tsx:43-53` — plain `<input type=password>` with no paste handling; `ProviderMeta` carries `custom_endpoints[].lastUsed` (`providerMetaUtils.ts:1-59`) but nothing about key age/expiry; nothing in the card surfaces when a key was added.
- **Problem:** Keys pasted with surrounding quotes/`Bearer `/newlines fail silently at request time; rotated or expiring keys are not tracked, so users discover expiry via 401s.
- **Proposal:** `onPaste` normalisation in `ApiKeyInput` (trim, strip quotes/`Bearer `, warn on control chars); optional `meta.apiKey = { addedAt, expiresAt?, label? }` with a card hint when expiry is near; show "key added N days ago".
- **Acceptance:** Pasting `"sk-x"\n` stores `sk-x`; a provider with `expiresAt` within 7 days shows a reminder badge.

### FE-PROV-37: Universal-provider model is confusing and duplicated; add provider templating instead

**Type:** ux · **Priority:** P3 · **Effort:** L · **Confidence:** medium

- **Evidence:** `src/components/providers/AddProviderDialog.tsx:47-53,313-353` — universal tab exists only for claude/codex/gemini and swaps the footer semantics; `src/components/universal/UniversalProviderFormModal.tsx:189-301` — `handleSubmit` and `buildProvider` are duplicated; 130-186 model defaults hard-code stale ids (`claude-sonnet-4-20250514`, `gemini-2.5-pro`); `UniversalProviderPanel.tsx:33-52` loads with ad-hoc `useState` instead of TanStack Query; duplication of a regular provider is per app only (`App.tsx:710-760`) with no "copy to another app" or "save as template".
- **Problem:** Two parallel provider models (per-app vs universal) with different forms, different sync semantics and partial app coverage; users cannot reuse a configured provider in another app without retyping.
- **Proposal:** Reframe "universal" as templates: "Save as template" from any provider, "Create provider from template for app X" via the per-app adapters (FE-PROV-17), optional "linked" templates that re-sync; migrate `UniversalProviderPanel` to TanStack Query; dedupe the builder.
- **Acceptance:** A Claude provider can be copied to Codex via UI; `UniversalProviderFormModal` has a single builder; universal data fetched via `useQuery`.

### FE-PROV-38: Docs gap: no documentation of form architecture, `ProviderMeta` field semantics or per-app `settingsConfig` shapes

**Type:** docs · **Priority:** P3 · **Effort:** S · **Confidence:** high

- **Evidence:** `src/components/providers/forms/ProviderForm.tsx:1452-1540` assembles ~20 `ProviderMeta` fields with conditional persistence rules (e.g. `apiKeyField` only when codex+anthropic and not the default; `codexFastMode` only for codex OAuth; `promptCacheRouting` only when chat format and not "auto") that exist nowhere else; per-app config shapes are implicit in `opencodeFormUtils.ts:6-68`, `useHermesFormState.ts:21-31`, `grokBuildConfig.ts:7-18`; most explanatory comments are Chinese-only inline notes.
- **Problem:** Contributors (and the AI agents the repo clearly uses) cannot know which meta fields are valid for which app without reading the 2.6k-line form; cross-language maintainers are excluded by comment language.
- **Proposal:** Add `docs/providers/` with `form-architecture.md` (shell/adapters), `provider-meta.md` (field, apps, persistence rule, consumer), `settings-config-shapes.md` (per app JSON/TOML examples), and JSDoc on `ProviderMeta`; keep English + Chinese.
- **Acceptance:** Docs exist and are linked from CONTRIBUTING/README; `ProviderMeta` fields have JSDoc.

### FE-PROV-39: Dead code and duplicated computations in the form shell

**Type:** tech-debt · **Priority:** P3 · **Effort:** S · **Confidence:** high

- **Evidence:** `src/components/providers/forms/ProviderForm.tsx:302-307` — `useState(() => { if (initialData) return []; return []; })`; 1419-1431 — `needsClearEndpoints` can never be true inside `if (draftCustomEndpoints.length > 0)`; 1109-1116 vs 1143-1150 vs 2109-2124 — `isCopilotProvider`/`isCodexOauthProvider` computed three times; 262-264 — `ProviderFormFull` throws for `claude-desktop` after the dispatcher already routes it (240-251); `useCodexConfigState.ts:311-312` — `getCodexAuthApiKey`/`validateCodexAuth` returned but unused (grep); `hooks/useCustomEndpoints.ts` exported but unused; `HealthStatusIndicator.tsx` unused; `useApiKeyState.ts:76-83` lists `selectedPresetId` as a dep it never reads.
- **Problem:** Noise that hides real logic and misleads readers (e.g. the clear-endpoints branch suggests clearing is supported in edit mode when it is not).
- **Proposal:** Remove dead branches/exports during the FE-PROV-17 migration; enable `@typescript-eslint/no-unused-vars` for exports via `knip` or `ts-prune` in CI.
- **Acceptance:** `knip` reports no unused exports under `src/components/providers`.

### FE-PROV-40: Per-project provider binding (profiles) does not cover most apps or expose which provider a profile will apply

**Type:** feature · **Priority:** P3 · **Effort:** L · **Confidence:** medium

- **Evidence:** `src/components/profiles/scope.ts:9-13` — profiles exist only for claude / claude-desktop / codex; `src/components/profiles/ProfileSwitcher.tsx:76-81` — the switcher is hidden for other apps; `ProfileSwitcher.tsx:155-159` only shows "no snapshot" per scope, never which provider/MCP set a profile contains; snapshots are implicit ("auto-saved on switch", `ProfileManageDialog.tsx:31-36`).
- **Problem:** Users cannot bind a provider to a project for Gemini/OpenCode/OpenClaw/Hermes, and cannot see what applying a profile will change before applying it.
- **Proposal:** Extend `ProfileScope` to the remaining apps (backend mirror), show a preview (provider name, MCP count) per scope in the switcher, and allow explicit "Snapshot now".
- **Acceptance:** ProfileSwitcher visible on all app tabs; hovering/expanding a profile shows the provider it will activate.

### FE-PROV-45: Add-mode speed-test drafts are discarded when the panel is reopened; the hook meant to carry them (useCustomEndpoints) is dead code

**Type:** bug · **Priority:** P3 · **Effort:** S · **Confidence:** high

- **Evidence:** src/components/providers/forms/EndpointSpeedTest.tsx:100-102 initialises `entries` once from `buildInitialEntries(initialEndpoints, value)` (57-85: only preset/config candidates plus the currently selected URL); the panel is mounted only while open (src/components/providers/forms/ClaudeFormFields.tsx:703, CodexFormFields.tsx:1011, GeminiFormFields.tsx:196), so each reopen rebuilds entries; 201-216 then emits `onCustomEndpointsChange` with whatever `isCustom` entries exist, overwriting `draftCustomEndpoints` via src/components/providers/forms/ProviderForm.tsx:2137-2139. src/components/providers/forms/hooks/useSpeedTestEndpoints.ts:33-35,44-105 never feeds the drafts back as candidates, and src/components/providers/forms/hooks/useCustomEndpoints.ts (which merges drafts) has no importers (only exported at hooks/index.ts:7).
- **Problem:** User adds https://b.example (not the selected URL), saves the panel, reopens it to re-test: the draft vanishes from the list, `draftCustomEndpoints` is reset to [], and the endpoint is never written to `meta.custom_endpoints` on submit.
- **Proposal:** Pass `draftCustomEndpoints` into `useSpeedTestEndpoints` (or directly to EndpointSpeedTest) as `isCustom: true` candidates in add mode; delete `useCustomEndpoints` or wire it in; add a test that opens, adds, closes and reopens the panel.
- **Acceptance:** Add flow: open panel → add custom URL → Save → reopen: the URL is still listed as custom; submitting the form persists it in `meta.custom_endpoints`.
- **Verification:** added by completeness-critic pass (not independently re-verified)

### FE-PROV-46: useManagedAuth keeps running after unmount: a device-code login that resolves after the dialog closes opens the browser, writes the clipboard and starts an uncleared polling interval

**Type:** bug · **Priority:** P3 · **Effort:** S · **Confidence:** medium

- **Evidence:** src/components/providers/forms/hooks/useManagedAuth.ts:51-55 — the unmount cleanup only calls `stopPolling()`; 57-123 the side effects live in the hook-level `useMutation({ onSuccess })`, which TanStack Query still invokes after the component unmounts (only `mutate(vars, callbacks)` callbacks are skipped): it copies the user code (64-68), calls `settingsApi.openExternal` (70-74), starts `setInterval(pollOnce, >= 8s)` (117) and a `setTimeout` for `expires_in` seconds (118-122) with nothing left to clear them; `pollOnce` also calls `refetchStatus()`/`setPollingState` on the dead component (95-101). CopilotAuthSection.tsx:278/292 and CodexOAuthSection.tsx:232/246 trigger `addAccount` from inside the provider form dialogs.
- **Problem:** Closing the Add/Edit dialog while `authStartLogin` is in flight does not cancel the login: the OS browser pops up after the user left, and the backend `authPollForAccount` is polled in the background for up to the device-code lifetime (~15 min), with React warnings for state updates on an unmounted component.
- **Proposal:** Keep an `isMountedRef` (set false in the cleanup) and an `AbortController`; in `onSuccess` return early when unmounted; store interval/timeout ids in refs that the cleanup clears even when created later; or move the side effects to per-call `mutate(undefined, { onSuccess })` callbacks, which are skipped after unmount.
- **Acceptance:** Test with fake timers: unmount while `authStartLogin` is pending, then resolve it — no `openExternal`, no `copyText`, no interval scheduled, no state update warnings.
- **Verification:** added by completeness-critic pass (not independently re-verified)

### FE-PROV-47: Universal provider save in the panel misreports a sync failure as a save failure and leaves the list stale, diverging from the AddProviderDialog implementation of the same flow

**Type:** bug · **Priority:** P3 · **Effort:** S · **Confidence:** high

- **Evidence:** src/components/universal/UniversalProviderPanel.tsx:55-86 — a single try/catch wraps `upsert` then `sync`; a sync error toasts `universalProvider.saveError` ('保存统一供应商失败') and skips `loadProviders()`/`setEditingProvider(null)`; src/components/providers/AddProviderDialog.tsx:62-103 implements the identical flow with two-phase error handling (`addedButSyncFailed` warning); src/components/universal/UniversalProviderFormModal.tsx:229-230 calls `onClose()` synchronously right after `onSave(provider)`, so the form input is gone before the error is known.
- **Problem:** When sync fails after a successful upsert, the user is told the provider was not saved although it was persisted, the list does not show it, and re-adding creates a duplicate; the modal has already closed so nothing can be retried. (Complements FE-PROV-37, which covers duplication inside the modal, not the panel/dialog divergence.)
- **Proposal:** Extract a shared `saveUniversalProvider(provider, { sync })` helper used by both the panel and AddProviderDialog with distinct toasts for upsert vs sync failure; always `loadProviders()` after a successful upsert; await `onSave` before closing the modal and keep it open on upsert failure.
- **Acceptance:** With `universalProvidersApi.sync` mocked to reject, the panel shows the 'added but sync failed' warning, the new provider appears exactly once in the list, and no `saveError` toast fires; with `upsert` rejecting, the modal stays open with the entered values.
- **Verification:** added by completeness-critic pass (not independently re-verified)

### FE-PROV-48: Gemini .env editor comments, blank lines and key order are destroyed by every API-key/base-URL/model keystroke

**Type:** bug · **Priority:** P3 · **Effort:** S · **Confidence:** high

- **Evidence:** src/components/providers/forms/hooks/useGeminiConfigState.ts:26-74 — `envObjToString` emits only `KEY=VALUE` lines with priority keys reordered first, and `envStringToObj` skips `#` comments and blank lines (63); 154-193 the API-key, base-URL and model handlers rebuild the whole env text through that round trip on every change; src/components/providers/forms/hooks/useGeminiCommonConfig.ts:259-261,313-315 do the same when toggling the common snippet.
- **Problem:** A user who annotates the env editor (`# prod key`, grouped sections) loses the annotations and ordering the moment they type one character in the API Key box, with no warning; the editor presents itself as free-form `.env` text but is lossy.
- **Proposal:** Make the env helpers line-preserving: patch the matching `KEY=` line in place, append new keys at the end, and leave comments/blank/unknown lines untouched (the same approach the Codex TOML helpers already use, src/utils/providerConfigUtils.ts:1257-1373); unit-test the round trip.
- **Acceptance:** Given env text `# note\nGEMINI_API_KEY=a\n\nFOO=1`, typing key `b` produces `# note\nGEMINI_API_KEY=b\n\nFOO=1`; toggling the common snippet keeps comments.
- **Verification:** added by completeness-critic pass (not independently re-verified)

### FE-PROV-49: Card action buttons never reflect in-flight mutations, so double-clicks issue concurrent switch/failover/add-to-config calls

**Type:** bug · **Priority:** P3 · **Effort:** S · **Confidence:** medium

- **Evidence:** src/hooks/useProviderActions.ts:421-426 computes `isLoading` from the four mutations, but src/App.tsx:304-315 does not destructure it and App.tsx:987 passes the providers-query `isLoading` (App.tsx:279) to ProviderList instead; src/components/providers/ProviderActions.tsx:271-280 the main button has no pending/disabled input; src/components/providers/ProviderList.tsx:180-189 calls `addToQueue.mutate`/`removeFromQueue.mutate` with no `isPending` guard; src-tauri/src/commands/provider.rs:103-110 `switch_provider` is a synchronous command, so two invocations run concurrently on Tauri's blocking pool.
- **Problem:** Two quick clicks on different cards start two concurrent `switch_provider` calls whose DB `set_current` and live-file writes can interleave (DB says B, live config says A); both success toasts fire and `syncClaudePlugin` (useProviderActions.ts:48-72,264-266) can apply the wrong `official` flag last. Failover toggles can likewise be queued twice.
- **Proposal:** Thread `isLoading` (or per-mutation `isPending`) from `useProviderActions` and the failover mutations down to ProviderList/ProviderActions and disable the main and failover buttons while pending; optionally serialise provider mutations per app in Rust with a mutex.
- **Acceptance:** While a switch is pending every card's main button is disabled; an RTL test asserts a second click does not call `onSwitch`; a hook test asserts `isLoading` is consumed by ProviderList.
- **Verification:** added by completeness-critic pass (not independently re-verified)

### FE-PROV-50: OpenClaw model-catalog registration failure after add is swallowed (console.warn only) while the UI reports success

**Type:** bug · **Priority:** P3 · **Effort:** S · **Confidence:** high

- **Evidence:** src/hooks/useProviderActions.ts:89-132 — after `addProviderMutation` resolves (which already toasts '供应商已添加' in src/lib/query/mutations.ts:122-129), the catalog merge and `setDefaultModel` run inside a try whose catch only `console.warn`s (125-131); the merge is a read-modify-write of the whole catalog (96-98) with no retry; tests/hooks/useProviderActions.test.tsx has no case for this branch.
- **Problem:** The provider appears added, but its models are missing from the OpenClaw `/model` allowlist and no default model is set; the user gets no indication and has to discover it inside OpenClaw. A concurrent catalog write can also be clobbered by the read-modify-write.
- **Proposal:** Toast a warning with the error (`notifications.openclawModelsRegisterFailed`) and offer a retry action; add a backend `merge_openclaw_model_catalog` command (atomic merge) instead of get+set from the frontend; add a hook test for the failure path.
- **Acceptance:** With `openclawApi.setModelCatalog` mocked to reject, a warning toast with a retry appears and the provider still exists; tests/hooks/useProviderActions.test.tsx covers it.
- **Verification:** added by completeness-critic pass (not independently re-verified)

### FE-PROV-51: Additive-mode cards show 'Add' and hide the default-model button until the live-id query resolves, then flip

**Type:** ux · **Priority:** P3 · **Effort:** S · **Confidence:** high

- **Evidence:** src/components/providers/ProviderList.tsx:119-133 — `isProviderInConfig` returns `false` while `opencodeLiveIds`/`openclawLiveIds`/`hermesLiveIds` are still `undefined`, and no loading flag is forwarded (404); src/components/providers/ProviderActions.tsx:143-165,231-260 render the green '添加' main button and omit the OpenClaw/Hermes default-model button whenever `!isInConfig`; src/components/providers/ProviderCard.tsx:301-306 drives the blue highlight from the same value; a click in that window runs `onSwitch` → `switchProvider`, which re-applies and toasts '已添加到配置' (src/hooks/useProviderActions.ts:298-301).
- **Problem:** Every switch to the OpenCode/OpenClaw/Hermes tab flashes the whole list from 'not in config' to 'in config' once the query lands, and a click during the window re-adds an already-added provider with a misleading success toast.
- **Proposal:** Expose `isLoading` from `useOpenClawLiveProviderIds`/`useHermesLiveProviderIds`/the opencode query, pass a tri-state `isInConfig: boolean | undefined` to the card, and render the main button disabled with a spinner (and no highlight change) until the value is known.
- **Acceptance:** With the live-id query pending, additive cards show a disabled main button and no default-model button toggling; an RTL test with a deferred query shows no 'Add' label before resolution.
- **Verification:** added by completeness-critic pass (not independently re-verified)

### FE-PROV-52: Provider-key field is triplicated (~210 lines, key regex copied 10x) and the 'existing keys' source is owned by three different hooks

**Type:** refactor · **Priority:** P3 · **Effort:** M · **Confidence:** high

- **Evidence:** src/components/providers/forms/ProviderForm.tsx:1876-2086 — three near-identical `beforeNameSlot` blocks for opencode/openclaw/hermes; the pattern `/^[a-z0-9]+(-[a-z0-9]+)*$/` appears at 1056, 1903, 1919, 1932, 1969, 1985, 1998, 2039, 2055, 2068; the submit-time validation blocks 1058-1139 are repeated three times. Existing-key lists are computed in three places with different owners — src/components/providers/forms/hooks/useOmoModelSource.ts:48-54 (opencode), useOpenclawFormState.ts:61-68, useHermesFormState.ts:81-87 — each issuing its own `useProvidersQuery(app)` on every form mount regardless of `appId`. Dead helpers remain alongside: src/components/providers/forms/hooks/useCustomEndpoints.ts has no importers, and `useBaseUrlState`'s codex/gemini branches (useBaseUrlState.ts:54-84,111-159) are unreachable from the only call site (ProviderForm.tsx:457-464 passes `codexConfig: ""` and a no-op).
- **Problem:** Fixes to key handling (FE-PROV-05, the duplicate-key guard above) must be applied three times and drift; every provider form fetches the openclaw and hermes provider lists even for Claude; dead hooks mislead readers. Narrower and independently shippable compared with the full-shell split in FE-PROV-17.
- **Proposal:** Extract `<ProviderKeyField>` plus `isValidProviderKey()` and a single `useAdditiveProviderKey({ appId, providerId })` hook that owns the existing-keys and live-id queries (enabled by `appId`), the locked/loading state and the i18n messages; delete `useCustomEndpoints` and the unused `useBaseUrlState` branches (or wire the former into the speed-test fix).
- **Acceptance:** One implementation of key validation with unit tests; `grep -c 'a-z0-9]+(-' src/components/providers/forms/ProviderForm.tsx` returns 0; rendering the Claude form issues no `useProvidersQuery("openclaw")`/`("hermes")` requests.
- **Verification:** added by completeness-critic pass (not independently re-verified)

### FE-PROV-53: No behaviour tests for the ProviderActions/ProviderCard state matrix or ProviderList's additive, failover and Hermes branches

**Type:** test · **Priority:** P3 · **Effort:** M · **Confidence:** high

- **Evidence:** tests/components/ProviderCardLayout.test.ts is the only card test and asserts source strings; tests/components/ProviderList.test.tsx:144-309 covers skeleton/empty/order/search with ProviderCard mocked out; nothing exercises src/components/providers/ProviderActions.tsx:88-224 (additive vs failover vs OMO vs read-only vs official-blocked states, `canDelete`), src/components/providers/ProviderCard.tsx:72-136,290-306 (`isOfficialProvider`, `extractApiUrl`, active/blue/green states) or src/components/providers/ProviderList.tsx:119-146,180-189,383-437 (isInConfig, OpenClaw default model, Hermes current, failover toggles); tests/hooks/useProviderActions.test.tsx:137-671 has no OpenClaw catalog-registration case. (FE-PROV-32 covers the dialog/form side only.)
- **Problem:** Regressions such as Duplicate enabled on read-only providers, the loading-state flicker or a change to the official heuristic are invisible to CI, and the per-app button-state matrix is the most branch-heavy UI code in the area.
- **Proposal:** Add RTL tests that render ProviderActions across the prop matrix (labels/disabled snapshots), ProviderCard with representative settingsConfig per app, and ProviderList for additive apps with pending/resolved live-id queries and failover enabled; add a useProviderActions test for catalog-registration failure.
- **Acceptance:** New tests fail if Duplicate is enabled for `isReadOnly`, if `isOfficialProvider` changes for a Bedrock-style config, or if the failover toggle fires while pending; coverage for ProviderActions.tsx and ProviderCard.tsx is reported.
- **Verification:** added by completeness-critic pass (not independently re-verified)

### FE-PROV-54: Show the configured model(s) and API format on provider cards (and include them in search)

**Type:** feature · **Priority:** P3 · **Effort:** M · **Confidence:** medium

- **Evidence:** src/components/providers/ProviderCard.tsx:365-497 renders only name, status badges and the endpoint; the model is already parsed elsewhere (src/components/providers/forms/hooks/useModelState.ts:41-116 for Claude env keys, src/utils/providerConfigUtils.ts:1395-1414 `extractCodexModelName`, OpenClaw `settingsConfig.models[0].id` in src/hooks/useProviderActions.ts:383-386, `ProviderMeta.claudeDesktopModelRoutes`); src/components/providers/ProviderList.tsx:280-289 search matches name/notes/websiteUrl only.
- **Problem:** Several providers often share one relay endpoint and differ only by default model or mapping; the list cannot tell them apart without opening each edit dialog, and search cannot find a provider by model name.
- **Proposal:** Add `getProviderModelSummary(provider, appId)` reusing the parsers above and render a compact monospace chip (primary model, with haiku/opus in the title) next to the URL; include the summary in the search fields.
- **Acceptance:** A Claude provider with `ANTHROPIC_MODEL=x` shows chip `x`; a Codex provider shows the TOML `model`; searching `x` matches that card; snapshot/RTL tests cover one provider per app.
- **Verification:** added by completeness-critic pass (not independently re-verified)

### FE-PROV-55: Per-provider 'Copy share link' (ccswitch:// deep link) action is missing although the importer exists

**Type:** feature · **Priority:** P3 · **Effort:** M · **Confidence:** medium

- **Evidence:** src/lib/api/deeplink.ts:5-47,70-78 — `DeepLinkImportRequest` already carries provider fields (endpoint, apiKey, model, haikuModel/sonnetModel/opusModel, notes, usage script) and `parseDeeplink`; src/components/DeepLinkImportDialog.tsx handles imports; no builder exists (grep `ccswitch://` in src returns only the parser comment) and src/components/providers/ProviderActions.tsx:283-368 offers edit/duplicate/test/usage/terminal/delete but no share or export. FE-PROV-33 asks only for bulk export.
- **Problem:** Sharing a relay configuration with a teammate (the inverse of an existing feature) requires retyping endpoint, models and notes; the data model and parser are already in place.
- **Proposal:** Add `buildProviderDeeplink(provider, appId, { includeApiKey })` mirroring the parser's fields, and a 'Share' item in ProviderActions that copies the link via `copyText`, with an explicit confirm when including the API key (default off).
- **Acceptance:** For a Claude provider, the copied link round-trips through `parse_deeplink` into an equivalent provider (minus the key when excluded); a unit test covers the builder/parser round trip for Claude, Codex and Gemini.
- **Verification:** added by completeness-critic pass (not independently re-verified)
