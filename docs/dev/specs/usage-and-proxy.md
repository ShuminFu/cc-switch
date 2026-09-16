# Usage & Proxy area — behavioural spec (for the Dioxus port)

Sources are React/TS under `/home/user/cc-switch/src`. All paths absolute; line numbers are as-read.

## 0. Findings that are bugs, not behaviour to port

| # | Issue | Evidence |
|---|---|---|
| B1 | **`usage.appFilter.grokbuild` missing from every locale.** `UsageDashboard` calls `t(\`usage.appFilter.${type}\`)` with **no `defaultValue`** (`/home/user/cc-switch/src/components/usage/UsageDashboard.tsx:220`), and `UsageHero` does the same (`UsageHero.tsx:194`). The Grok Build filter button therefore renders the raw key as its `title`/`aria-label`. | `en.json` `usage.appFilter` = `{all, claude, codex, gemini, opencode}` — verified in en/zh/zh-TW/ja |
| B2 | **`usage.unpriced` missing from every locale**, and both call sites pass a **Chinese** `defaultValue` `"未定价"` (`RequestLogTable.tsx:280`, `RequestDetailPanel.tsx:289`). English/Japanese users see Chinese text. | key absent from all 4 locales |
| B3 | `proxy.server.stopped` / `proxy.server.stopFailed` missing (`useProxyStatus.ts:72,82`) — Chinese `defaultValue` fallback leaks. | — |
| B4 | `failover.tooltip.takeoverRequired` missing (`FailoverToggle.tsx:40`) — Chinese fallback leaks. | — |
| B5 | `claudeDesktop.route.stopBlockedByTakeover` missing (`ClaudeDesktopRouteToggle.tsx:47`) — Chinese fallback in a toast. | — |
| B6 | **`RequestDetailPanel.tsx` is dead code.** Nothing imports it; `RequestLogTable` has no row-click handler. The `get_request_detail` command + `useRequestDetail` hook are unreachable from the UI. | repo-wide grep for `RequestDetailPanel` returns only its own file |
| B7 | **`DataSourceBar.tsx` is dead code.** Nothing imports it; `sync_session_usage` and `get_usage_data_sources` are unreachable from the UI. | repo-wide grep |
| B8 | `useProxyConfig` exists **twice** with divergent behaviour: `/home/user/cc-switch/src/hooks/useProxyConfig.ts` and `/home/user/cc-switch/src/lib/query/proxy.ts:137`. Identical semantics, duplicated. Likewise `useProxyStatus` exists in `hooks/useProxyStatus.ts` (rich) and `lib/query/proxy.ts:12` (thin). Consumers mix the two — `ProxyPanel` uses the hooks version, `ProxyTabContent` too; `lib/query/proxy.ts`'s `useProxyStatus` is unused. Port **one**. | — |
| B9 | **`keepLastGoodUsage` does not exist** under that name. The policy lives in `/home/user/cc-switch/src/lib/query/queries.ts:104-243` as `resolveDisplayUsage` + `isTransientUsageError` + `KEEP_LAST_GOOD_MS`, and is **not** applied to any dashboard query — only to `useUsageQuery` (script path) and the subscription/quota hooks. See §1.9. | — |
| B10 | `ProxyPanel` polls health for every queue member via one `useProviderHealth` per `ProviderQueueItem` (`ProxyPanel.tsx:719`), each at 5 s. N providers ⇒ N commands / 5 s. Consider a batch command in the port. | — |
| B11 | `ProxyPanel.tsx:118` and `:194` declare `catch (error)` but never use it — the toast drops the real reason. | — |

---

# 1. Usage dashboard

## 1.1 `UsageDashboard` — `/home/user/cc-switch/src/components/usage/UsageDashboard.tsx` (435 L)

Host: `SettingsPage.tsx:513`, inside `<TabsContent value="usage">`, props wired to settings autosave:
```tsx
<UsageDashboard
  refreshIntervalMs={settings?.usageDashboardRefreshIntervalMs}
  onRefreshIntervalChange={(usageDashboardRefreshIntervalMs) =>
    handleAutoSave({ usageDashboardRefreshIntervalMs })} />
```

### Props
| Prop | Type | Notes |
|---|---|---|
| `refreshIntervalMs` | `number?` | persisted setting; normalized on mount + on every change (`:94-100`) |
| `onRefreshIntervalChange` | `(next:number)=>Promise<boolean>\|boolean\|void` | returning **`false`** reverts the local state (`:129`) |

### Local state (`:88-96`)
| State | Initial |
|---|---|
| `range: UsageRangeSelection` | `{ preset: "today" }` |
| `appType: AppTypeFilter` | `"all"` |
| `providerName: string \| undefined` | `undefined` |
| `model: string \| undefined` | `undefined` |
| `refreshIntervalMs` | `normalizeRefreshInterval(saved)` |

### Constants (`:47-75`)
- `APP_FILTER_OPTIONS = ["all", ...KNOWN_APP_TYPES]` → `all, claude, codex, gemini, grokbuild, opencode`
- `DEFAULT_REFRESH_INTERVAL_MS = 30000`; `REFRESH_INTERVAL_OPTIONS_MS = [0, 5000, 10000, 30000, 60000]`
- `normalizeRefreshInterval(v)` → `v` if in the option list, else `30000`. **Port note:** any out-of-list persisted value silently becomes 30 s.
- `APP_FILTER_ICON: Record<AppType,string>` = `{claude:"claude", codex:"openai", gemini:"gemini", grokbuild:"grok", opencode:"opencode"}`
- **Value-domain encoding** (`:72-75`): dynamic Select options are prefixed `"v:"` so a provider/model literally named `all` cannot collide with the `"all"` sentinel. `encodeOptionValue(n)=` `` `v:${n}` ``; `decodeOptionValue(v) = v==="all" ? undefined : v.slice(2)`. **Must be reproduced.**

### Cascade reset rules (`:104-116`)
- `changeAppType(next)`: sets appType; if changed → clears **both** `providerName` and `model`.
- `changeProviderName(next)`: sets it; if changed → clears `model`.
- Model Select calls `setModel` **directly** (`:278`), no cascade below it.

### `changeRefreshInterval` (`:122-139`)
1. normalize, 2. optimistic `setRefreshIntervalMs`, 3. **`queryClient.invalidateQueries({queryKey: usageKeys.all})`** immediately, 4. `await onRefreshIntervalChange`, 5. on `=== false` **or throw** → revert to previous (logs `"[UsageDashboard] Failed to persist refresh interval"`).

### `rangeLabel` (`:144-161`)
- non-custom → `getUsageRangePresetLabel(preset, t)`
- custom + `liveEndTime` → `` `${start.toLocaleString(locale)} → ${t("usage.liveEndTimeNow")}` `` (note **`→`** with spaces)
- custom + fixed → `` `${start} - ${end}` `` (note **`-`**, a different separator)

### Option pools (`:167-200`)
`useProviderStats(range, {appType}, optionsRefetch)` and `useModelStats(range, {appType, providerName}, optionsRefetch)` where `optionsRefetch = {refetchInterval: ms>0 ? ms : false}`.
**Critical:** the comment at `:164-166` warns these share query keys with the stats tables — the `refetchInterval` **must** track the panel setting, otherwise the shared key polls at the 30 s default and the "off" (`--`) setting is defeated.
Both pools re-insert the currently-selected value if it dropped out of the result set (`:189`, `:198`) so the Select can still render the selection and the user can clear it.

### Layout, in order
1. Header row — `h2` `usage.title`, `p` `usage.subtitle`.
2. App filter segmented group: icon-only buttons, `all` → `LayoutGrid` icon, others → `<ProviderIcon icon={APP_FILTER_ICON[type]} size={16}/>`; `title`+`aria-label` = `t(\`usage.appFilter.${type}\`)` (**see B1**).
3. Provider `Select` — w-100px, item `all` = `usage.allSources`; trigger `title` = `providerName ?? t("usage.filterBySource")`.
4. Model `Select` — item `all` = `usage.allModels`; trigger `title` = `model ?? t("usage.filterByModel")`.
5. Refresh-interval `Select` — `RefreshCw` icon; labels `` `${ms/1000}s` `` or `usage.refreshOff` for 0; `title`/`aria-label` = `usage.refreshInterval`.
6. `UsageDateRangePicker`.
7. `<UsageHero>` — **`appType` passed as `appType === "all" ? undefined : appType`** (`:336`).
8. `<UsageTrendChart>` — **`appType` passed raw, including `"all"`** (`:344`); normalization happens in `normalizeScopeFilters`.
9. Tabs (`defaultValue="logs"`): `logs` (`ListFilter`, `usage.requestLogs`) / `providers` (`Activity`, `usage.providerStats`) / `models` (`BarChart3`, `usage.modelStats`). Content wrapped in a `motion.div` with `delay:0.2`.
10. Accordion (`type="multiple"`, `defaultValue={[]}` = collapsed) with one item `pricing`: `Coins` icon `text-yellow-500`, title `settings.advanced.pricing.title`, desc `settings.advanced.pricing.description`, body `<PricingConfigPanel/>`.

Mount effect: **`useUsageEventBridge()`** (`:120`) — only here, by design.

Animations: root `motion.div` `opacity 0→1, y 10→0, 0.4s`.

---

## 1.2 `src/lib/usageRange.ts` (81 L)

```ts
export interface ResolvedUsageRange { startDate: number; endDate: number }   // unix seconds
```
`resolveUsageRange(selection, nowMs = Date.now())` (`:26-61`) — **`endDate` is always `floor(nowMs/1000)` except in the custom+fixed branch**:

| preset | startDate | endDate |
|---|---|---|
| `today` | `floor(startOfLocalDay(now)/1000)` | `now` |
| `1d` | `now - 86400` | `now` |
| `7d`/`14d`/`30d` | `floor(startOfLocalDay(now - (N-1)*DAY_MS)/1000)` — N = 7/14/30 | `now` |
| `custom` | `customStartDate ?? now - 86400` | `liveEndTime ? now : (customEndDate ?? now)` |

`getStartOfLocalDayDate` uses **local** `new Date(y, m, d)` — not UTC. The port must use the local timezone.
Note the asymmetry: `1d` is a rolling 24 h window; `7d`/`14d`/`30d` are calendar-day-aligned lookbacks including today.

`getUsageRangePresetLabel(preset, t)` (`:63-81`) → keys `usage.presetToday`, `usage.preset1d`, `usage.preset7d`, `usage.preset14d`, `usage.preset30d`, `usage.customRange`. All present in `en.json`.

---

## 1.3 `src/types/usage.ts` — quoted interfaces (275 L)

```ts
export interface TokenUsage {
  inputTokens: number; outputTokens: number;
  cacheReadTokens: number; cacheCreationTokens: number;
}

export interface RequestLog {
  requestId: string; providerId: string; providerName?: string;
  appType: string; model: string; requestModel?: string;
  /** 写入时实际用于计价的模型名；路由接管 + request 计价模式下可能与 model 不同 */
  pricingModel?: string;
  costMultiplier: string;
  inputTokens: number; outputTokens: number;
  cacheReadTokens: number; cacheCreationTokens: number;
  inputCostUsd: string; outputCostUsd: string;
  cacheReadCostUsd: string; cacheCreationCostUsd: string;
  totalCostUsd: string;
  isStreaming: boolean; latencyMs: number;
  firstTokenMs?: number; durationMs?: number;
  statusCode: number; errorMessage?: string;
  createdAt: number; dataSource?: string;
}

export interface SessionSyncResult { imported: number; skipped: number; filesScanned: number; errors: string[] }
export interface DataSourceSummary { dataSource: string; requestCount: number; totalCostUsd: string }
export interface PaginatedLogs { data: RequestLog[]; total: number; page: number; pageSize: number }

export interface ModelPricing {
  modelId: string; displayName: string;
  inputCostPerMillion: string; outputCostPerMillion: string;
  cacheReadCostPerMillion: string; cacheCreationCostPerMillion: string;
}

export interface UsageSummary {
  totalRequests: number; totalCost: string;
  totalInputTokens: number; totalOutputTokens: number;
  totalCacheCreationTokens: number; totalCacheReadTokens: number;
  successRate: number;
  /** input + output + cache_creation + cache_read, all cache-normalized */
  realTotalTokens: number;
  /** cache_read / (input + cache_creation + cache_read), range 0–1 */
  cacheHitRate: number;
}

export interface UsageSummaryByApp { appType: string; summary: UsageSummary }

export interface DailyStats {
  date: string; requestCount: number; totalCost: string; totalTokens: number;
  totalInputTokens: number; totalOutputTokens: number;
  totalCacheCreationTokens: number; totalCacheReadTokens: number;
}

export interface ProviderStats {
  providerId: string; providerName: string; requestCount: number;
  totalTokens: number; totalCost: string; successRate: number; avgLatencyMs: number;
}

export interface ModelStats {
  model: string; requestCount: number; totalTokens: number;
  totalCost: string; avgCostPerRequest: string;
}

export interface LogFilters {
  appType?: string; providerName?: string; model?: string;
  statusCode?: number; startDate?: number; endDate?: number;
}

export interface UsageScopeFilters { appType?: string; providerName?: string; model?: string }

export interface ProviderLimitStatus {
  providerId: string;
  dailyUsage: string; dailyLimit?: string; dailyExceeded: boolean;
  monthlyUsage: string; monthlyLimit?: string; monthlyExceeded: boolean;
}

export type UsageRangePreset = "today" | "1d" | "7d" | "14d" | "30d" | "custom";

export interface UsageRangeSelection {
  preset: UsageRangePreset;
  customStartDate?: number; customEndDate?: number;
  /** When true (custom mode only), endDate resolves to "now" instead of the
   *  fixed customEndDate snapshot, and the end-time field becomes read-only. */
  liveEndTime?: boolean;
}

export type AppType = "claude" | "codex" | "gemini" | "grokbuild" | "opencode";
export type AppTypeFilter = "all" | AppType;
export const KNOWN_APP_TYPES: ReadonlyArray<AppType> = ["claude","codex","gemini","grokbuild","opencode"];
export const CACHE_INCLUSIVE_APP_TYPES: ReadonlySet<string> = new Set(["codex","gemini","grokbuild"]);
export interface CacheNormalizableLog { appType: string; inputTokens: number; cacheReadTokens: number }
export const NON_NEGATIVE_DECIMAL_REGEX = /^\d+(?:\.\d+)?$/;
export interface StatsFilters { timeRange: UsageRangePreset; providerId?: string; appType?: string }
```

### Domain rules encoded in this file (port them literally)
- **`claude-desktop` is deliberately absent from `AppType`** (doc comment `:160-172`). The Desktop gateway's proxy traffic is recorded under its own `app_type` (the request-detail panel shows the real value) but every dashboard query folds `claude-desktop → claude` in SQL (`folded_app_type_sql`). Rationale: it is the embedded Claude Code runtime, Desktop *chat* never passes through the app, so a separate bucket would show a misleading partial number. `opencode`/`openclaw`/`hermes` have no proxy handler at all.
- **`getFreshInputTokens(log)`** (`:217-225`): if `CACHE_INCLUSIVE_APP_TYPES.has(appType) && inputTokens >= cacheReadTokens` → `inputTokens - cacheReadTokens`, else pass through. OpenAI-style protocols report cache-inclusive input **and** never report cache *creation* (always 0 → UI must label N/A, not 0).
- **`isNonNegativeDecimalString(v)`**: trim, regex test, then `Number.isFinite(Number(trimmed))`.
- **`hasUsageTokens`**: any of the 4 token counts `> 0`.
- **`isUnpricedUsage(log)`** (`:255-269`): `2xx && hasUsageTokens && Number.isFinite(totalCost) && (multiplier is NaN/absent || multiplier !== 0) && totalCost === 0`. i.e. a successful request with real tokens that priced to exactly $0 and wasn't deliberately zero-multiplied.

---

## 1.4 `src/lib/api/usage.ts` — Tauri commands (186 L)

**Naming is inconsistent**: the two script commands are camelCase, everything else is snake_case. Port must match exactly.

| TS method | Command | Args | Returns |
|---|---|---|---|
| `query` | `queryProviderUsage` | `{providerId, app: AppId}` | `UsageResult` |
| `testScript` | `testUsageScript` | `{providerId, app, scriptCode, timeout?, apiKey?, baseUrl?, accessToken?, userId?, templateType?}` | `UsageResult` |
| `getUsageSummary` | `get_usage_summary` | `{startDate?, endDate?, appType?, providerName?, model?}` | `UsageSummary` |
| `getUsageSummaryByApp` | `get_usage_summary_by_app` | `{startDate?, endDate?, providerName?, model?}` — **no `appType`** | `UsageSummaryByApp[]` |
| `getUsageTrends` | `get_usage_trends` | `{startDate?, endDate?, appType?, providerName?, model?}` | `DailyStats[]` |
| `getProviderStats` | `get_provider_stats` | same 5 | `ProviderStats[]` |
| `getModelStats` | `get_model_stats` | same 5 | `ModelStats[]` |
| `getRequestLogs` | `get_request_logs` | `{filters: LogFilters, page = 0, pageSize = 20}` | `PaginatedLogs` |
| `getRequestDetail` | `get_request_detail` | `{requestId}` | `RequestLog \| null` |
| `getModelPricing` | `get_model_pricing` | — | `ModelPricing[]` |
| `updateModelPricing` | `update_model_pricing` | `{modelId, displayName, inputCost, outputCost, cacheReadCost, cacheCreationCost}` (all strings) | `void` |
| `deleteModelPricing` | `delete_model_pricing` | `{modelId}` | `void` |
| `checkProviderLimits` | `check_provider_limits` | `{providerId, appType}` | `ProviderLimitStatus` |
| `syncSessionUsage` | `sync_session_usage` | — | `SessionSyncResult` |
| `getDataSourceBreakdown` | `get_usage_data_sources` | — | `DataSourceSummary[]` |

---

## 1.5 `src/lib/query/usage.ts` — keys, hooks, mutations (392 L)

`DEFAULT_REFETCH_INTERVAL_MS = 30000` (`:10`).

### Query keys (`:37-150`)
Every stats key is a **flat tuple** with fixed arity and `??` defaults — the port must reproduce the padding or caches will collide:

| key fn | shape |
|---|---|
| `all` | `["usage"]` |
| `summary` | `[...all,"summary",preset, cs??0, ce??0, live??false, appType??null, providerName??null, model??null]` |
| `summaryByApp` | `[...all,"summary-by-app",preset, cs??0, ce??0, live??false, providerName??null, model??null]` |
| `trends` | `[...all,"trends",…]` (same 9-tuple as summary) |
| `providerStats` | `[...all,"provider-stats",…]` |
| `modelStats` | `[...all,"model-stats",…]` |
| `logs` | `[...all,"logs",preset, cs??0, ce??0, live??false, appType??"", providerName??"", model??"", statusCode??-1, page, pageSize]` — note **`""`/`-1`** sentinels here vs `null` above |
| `detail` | `[...all,"detail",requestId]` |
| `pricing` | `[...all,"pricing"]` |
| `limits` | `[...all,"limits",providerId,appType]` |
| `script` | `[...all, providerId, appType]` — **no discriminator segment**, sits directly under `["usage", …]`; this is what `useUsageQuery` and `useUsageCacheBridge` write |

**Consequence:** `invalidateQueries({queryKey: usageKeys.all})` (used by the event bridge and both pricing mutations) also invalidates every **script-usage** query, because `script()` lives under the same prefix.

### `normalizeScopeFilters` (`:153-159`)
Maps `appType === "all"` → `undefined` (backend semantics: no filter). `providerName`/`model` pass through unchanged. Applied in `useUsageSummary`, `useUsageTrends`, `useProviderStats`, `useModelStats` — **not** in `useUsageSummaryByApp` (which has no appType) and **not** in `useRequestLogs` (the caller strips `"all"` itself).

### Hooks
| Hook | Key | refetchInterval | Other |
|---|---|---|---|
| `useUsageSummary(range, filters?, options?)` | `summary` | `options ?? 30000` | `refetchIntervalInBackground: options ?? false` |
| `useUsageSummaryByApp(range, filters?, options?)` | `summaryByApp` | same | filters is `Pick<…,"providerName"\|"model">` |
| `useUsageTrends` | `trends` | same | |
| `useProviderStats` | `providerStats` | same | |
| `useModelStats` | `modelStats` | same | |
| `useRequestLogs({filters, range, page=0, pageSize=20, options})` | `logs` | same | queryFn merges `{...filters, ...resolveUsageRange(range)}` |
| `useRequestDetail(requestId)` | `detail` | — | `enabled: !!requestId` |
| `useModelPricing()` | `pricing` | — | no polling |
| `useProviderLimits(providerId, appType)` | `limits` | — | `enabled: !!providerId && !!appType` |

**All queryFns call `resolveUsageRange(range)` inside the fn, not in the key.** With `liveEndTime` or any `endDate: now` preset the key is stable while the resolved window slides — so a refetch on the same key yields a different window. Intentional. The port must resolve the range at fetch time, not at key time.

### Mutations
| Mutation | Fn | onSuccess |
|---|---|---|
| `useUpdateModelPricing()` (`:357`) | `usageApi.updateModelPricing(6 args)` | `invalidateQueries({queryKey: usageKeys.all})` |
| `useDeleteModelPricing()` (`:383`) | `usageApi.deleteModelPricing(modelId)` | `invalidateQueries({queryKey: usageKeys.all})` |

Neither raises a toast — the callers do (`PricingEditModal`, `ModelsDevPickerDialog`; `PricingConfigPanel` delete is silent on success).

### Global query defaults — `/home/user/cc-switch/src/lib/query/queryClient.ts`
```ts
queries:   { retry: 1, refetchOnWindowFocus: true, staleTime: 0 }
mutations: { retry: false }
```
`staleTime: 0` + `refetchOnWindowFocus: true` means **every dashboard query refetches on window focus regardless of the refresh-interval setting**, including when it is set to "off".

---

## 1.6 Keep-last-good ("`keepLastGoodUsage`") — `/home/user/cc-switch/src/lib/query/queries.ts:104-243`

**This policy is NOT applied to any dashboard query.** It applies to `useUsageQuery` (provider usage script) and, via `useQuotaKeepLastGood`, to `useSubscriptionQuota` / `useCodexOauthQuota`. Included here because the task named it.

```ts
export interface UsageLikeResult { success: boolean; error?: string | null }
export interface LastGoodSnapshot<T> { data: T; at: number }
export type LastGoodUsage = LastGoodSnapshot<UsageResult>;
export const KEEP_LAST_GOOD_MS = 10 * 60 * 1000;
export interface ResolveDisplayUsageOptions { rejected?: boolean; keepMs?: number }
export function resolveDisplayUsage<T extends UsageLikeResult>(
  raw: T | undefined, dataUpdatedAt: number,
  prevLastGood: LastGoodSnapshot<T> | null, now: number,
  options: ResolveDisplayUsageOptions = {},
): { data: T | undefined; lastQueriedAt: number | null; lastGood: LastGoodSnapshot<T> | null }
```

### `isTransientUsageError(result)` (`:142-167`) — **whitelist, fail-safe**
`success === true` → `false`. Empty error → `false`. Otherwise lowercase the message and:
- substring match on `"network error"`, `"request failed"`, `"请求失败"`, `"failed to read response"`, `"读取响应失败"` → transient;
- first `/http\s+(\d{3})/` match → transient iff `500 ≤ s ≤ 599 || s === 429`; **all other 4xx (401/403/404) are deterministic**;
- anything unrecognized → **not transient** (surface immediately).

Backend error-string contracts it depends on: native balance/coding_plan/subscription emit `"API error (HTTP <code>…)"`; JS `usage_script` emits `"HTTP <code> …"`.

### `resolveDisplayUsage` decision table
| Input | data | lastQueriedAt | lastGood |
|---|---|---|---|
| `rejected && raw.success` (stale value react-query retained) **and** `now - at < keepMs` | `raw` | `at` | reseeded `{raw, dataUpdatedAt \|\| now}` |
| `rejected && raw.success` but **out of window** | `undefined` | `at` | **kept** (not cleared — reject is transient) |
| `raw.success` (normal) | `raw` | `dataUpdatedAt` | refreshed |
| failure, **transient**, lastGood within `keepMs` | `lastGood.data` | `lastGood.at` | unchanged |
| failure, **deterministic** (auth/4xx/parse) | `raw` | `dataUpdatedAt` | **`null` — snapshot discarded** |

The `lastGood = null` on deterministic failure is load-bearing: otherwise one later network blip resurrects an already-invalid quota.
Window is anchored to the **last real success**, not to the failure — so a total outage cannot mask longer than a single 5xx.

### Callers
- `useUsageQuery` (`:245-305`): key `usageKeys.script(providerId, appId)`; `staleTime = autoQueryInterval>0 ? interval*60000 : 300000`; `refetchInterval = autoQueryInterval>0 ? max(interval,1)*60000 : false`; `refetchIntervalInBackground: true`; `refetchOnWindowFocus: false`; `retry: 1`, `retryDelay: 1500`; `gcTime: 600000`. Holds `lastGoodRef` **per hook instance** (per card). On `isError` with no displayable value, synthesizes `{success:false, error: extractErrorMessage(query.error) || undefined}`. Returns `{...query, data, lastQueriedAt}`.
- `useQuotaKeepLastGood(query, scopeKey)` (`subscription.ts:47-77`): same policy; `scopeKey` (appId or bound accountId) identity change **drops the snapshot** so one account's quota can't mask another's failure. Placeholder on reject:
```ts
const QUERY_REJECTED_PLACEHOLDER: SubscriptionQuota = {
  tool: "", credentialStatus: "valid", credentialMessage: null,
  success: false, tiers: [], extraUsage: null, error: null, queriedAt: null };
```

---

## 1.7 Event bridges

### `useUsageEventBridge` — `/home/user/cc-switch/src/hooks/useUsageEventBridge.ts` (41 L)
Raw `listen("usage-log-recorded", …)` in a `useEffect` (not `useTauriEvent`), with a `disposed` guard so an unlisten arriving after unmount still detaches. Handler: **`queryClient.invalidateQueries({queryKey: usageKeys.all})`** — payload ignored entirely.
Backend emits when a row lands in `proxy_request_logs`, **debounced/coalesced at 200 ms**; sources: proxy logs, Claude/Codex/Gemini session sync, startup archival. Mounted **only** on `UsageDashboard`.

### `useUsageCacheBridge` — `/home/user/cc-switch/src/hooks/useUsageCacheBridge.ts` (43 L)
`useTauriEvent<UsageCacheUpdatedPayload>("usage-cache-updated", …)`. Discriminated payload:
```ts
type UsageCacheUpdatedPayload =
  | { kind: "script";       appType: AppId; providerId: string; data: UsageResult }
  | { kind: "subscription"; appType: AppId;                     data: SubscriptionQuota };
```
- `"script"` → `setQueryData(usageKeys.script(providerId, appType), data)`
- `"subscription"` → `setQueryData(subscriptionKeys.quota(appType), data)`
- unknown `kind` → ignored.

It **writes** cache rather than invalidating: purpose is that tray-triggered refreshes (which never go through the frontend) land in React Query, so the Rust `UsageCache` and RQ don't diverge.

`useTauriEvent` — `/home/user/cc-switch/src/hooks/useTauriEvent.ts` (39 L) is the generic listen/unlisten wrapper.

---

## 1.8 `src/components/usage/format.ts` (97 L)

| Fn | Behaviour |
|---|---|
| `parseFiniteNumber(v)` | number → itself if finite else `null`; string → `parseFloat`, finite else `null`; otherwise `null` |
| `fmtInt(v, locale?, fallback="--")` | `Intl.NumberFormat(locale).format(Math.trunc(n))`, else fallback |
| `fmtUsd(v, digits, fallback="--")` | `` `$${n.toFixed(digits)}` ``, else fallback |
| `getLocaleFromLanguage(lang)` | `"" → "en-US"`; normalize (lowercase, `_`→`-`); `"zh"→"zh-CN"`; `zh-tw`/`zh-hant*`/`zh-hk*`/`zh-mo*` → `"zh-TW"`; other `zh*` → `"zh-CN"`; `ja*` → `"ja-JP"`; else `"en-US"` |
| `getResolvedLang(i18n)` | `resolvedLanguage \|\| language \|\| "en"` |
| `formatTokensShort(v, lang, compactDecimals: 1\|2 = 1)` | `≤0` or non-finite → `"0"`. zh-Hant: `≥1e8` → `` `${(v/1e8).toFixed(2)} 億` ``, `≥1e4` → `` `${(v/1e4).toFixed(d)} 萬` ``, else `toLocaleString("zh-TW")`. zh/ja: same with `亿`/`万`, `toLocaleString()`. Else: `≥1e9` → `B` (2dp), `≥1e6` → `M` (**2dp, ignores `d`**), `≥1e3` → `K` (`d` dp), else `toLocaleString()` |

Note `M`/`B`/`億`/`亿` are hard-coded to 2 decimals; only the `K`/`万`/`萬` tier honours `compactDecimals`.

---

## 1.9 `UsageHero` — `/home/user/cc-switch/src/components/usage/UsageHero.tsx` (408 L)

**Data:** `useUsageSummaryByApp(range, {providerName, model}, {refetchInterval: ms>0 ? ms : false})` → `get_usage_summary_by_app`. Note it never passes `appType`; app selection is a **client-side pick**.

### `aggregateSummaries(items)` (`:82-113`)
Per-app rows already carry fresh-input semantics (normalized in SQL), so plain addition is correct for token counts. But:
- `successCount += Math.round((s.totalRequests * s.successRate) / 100)` — rebuild the count, then `successRate = totalRequests>0 ? successCount/totalRequests*100 : 0`. **Never average the rates.**
- `totalCost = totalCostNum.toFixed(6)` (string, 6 dp).
- `realTotalTokens = input + output + cacheCreation + cacheRead`.
- `cacheHitRate = cacheableInput>0 ? cacheRead/cacheableInput : 0` where `cacheableInput = input + cacheCreation + cacheRead` (**output excluded**).

`pickSummary(apps, appType)` (`:115-124`): `[]` → `undefined`; `appType` set → `apps.find(a=>a.appType===appType)?.summary`; else aggregate **all** rows. Comment at `:183-186`: no client-side filtering by `KNOWN_APP_TYPES` — that list governs only which buttons appear, not which rows join the "all" total, so Hero matches the tables below.

### `deriveCacheWriteState(appTypes)` (`:134-142`)
`[]` → `"ok"`; all in `CACHE_INCLUSIVE_APP_TYPES` → `"na"`; none → `"ok"`; mixed → `"partial"`. Input is `[appType]` when one app is selected, else every contributing `a.appType`.
Display (`:209-225`): `"na"` → value `"N/A"`, muted, tooltip `usage.cacheWriteNotReported`; `"partial"` → real value + tooltip `usage.cacheWritePartial`; `"ok"` → no tooltip.

### `TITLE_THEMES` (`:48-72`)
| app | accent | iconBg |
|---|---|---|
| all | `text-primary` | `bg-primary/10` |
| claude | `text-amber-600 dark:text-amber-400` | `bg-amber-500/10` |
| codex | `text-neutral-700 dark:text-neutral-300` | `bg-neutral-500/10` |
| gemini | `text-sky-600 dark:text-sky-400` | `bg-sky-500/10` |
| grokbuild | `text-rose-600 dark:text-rose-400` | `bg-rose-500/10` |
| opencode | `text-purple-600 dark:text-purple-400` | `bg-purple-500/10` |

`AppGlyph` (`:149-163`): if `appType in APP_ICON_MAP` clone that element with `size:20`; else `<Zap className="h-5 w-5 {accent}"/>`.

### Layout
- **Loading:** `Card` with `min-h-[200px]`, centered `Loader2 h-6 w-6`.
- **Top-left:** themed icon tile → label line (`appLabel` + `•` separator when an app is selected, then `usage.realTotal`) → `realTotal.toLocaleString()` at `text-2xl md:text-3xl`, `title` = same string, plus a badge `≈ {formatTokensShort(realTotal, lang, 2)}`.
- **Top-right pill:** `usage.totalRequests` (`Activity` `text-blue-500`, `requests.toLocaleString()`), divider, `usage.totalCost` (`text-green-500`, **`fmtUsd(totalCost, 4)`**, `"--"` when null).
- **Bottom grid** `grid-cols-2 lg:grid-cols-5`, four `MiniStat`s + hit-rate card:

| MiniStat | icon | label key | value | accent |
|---|---|---|---|---|
| 1 | `ArrowDownToLine` | `usage.freshInput` | `formatTokensShort(input, lang)` | `text-blue-500` |
| 2 | `ArrowUpFromLine` | `usage.output` | output | `text-purple-500` |
| 3 | `Database` | `usage.cacheWrite` | `cacheWriteDisplay.value` | `text-amber-500` |
| 4 | `Sparkles` | `usage.cacheRead` | cacheRead | `text-emerald-500` |

`MiniStat` renders an `Info h-3 w-3` glyph (right-aligned) only when a `tooltip` is present; `muted` adds `text-muted-foreground/70`; the tooltip is the container `title` attribute.

- **Hit-rate card** (`col-span-2 lg:col-span-1`): label `usage.cacheHitRate`, value `hitPercentLabel%` in `text-emerald-500`. `hitPercent = clamp(hitRate*100, 0, 100)`; **`hitPercentLabel = hitPercent.toFixed(hitPercent >= 99.95 ? 0 : 1)`** (so 99.97 → `100`, not `100.0`). Bar: `h-1.5` track, animated fill `width 0 → ${hitPercent}%`, `0.8s easeOut`.

---

## 1.10 `UsageTrendChart` — `/home/user/cc-switch/src/components/usage/UsageTrendChart.tsx` (243 L)

**Chart type:** recharts `<AreaChart>` in `<ResponsiveContainer width="100%" height="100%">` inside a `h-[350px]` box. Margin `{top:10, right:10, left:0, bottom:0}`.

**Data:** `useUsageTrends(range, {appType, providerName, model}, {refetchInterval: ms>0 ? ms : false})`. **`appType` arrives possibly as `"all"`** and is normalized by `normalizeScopeFilters`.

**Bucketing:** `durationSeconds = max(endDate-startDate, 0)`; **`isHourly = durationSeconds <= 86400`**. Label formatting per point:
- hourly → `toLocaleString(dateLocale, {month:"2-digit", day:"2-digit", hour:"2-digit", minute:"2-digit"})`
- daily → `toLocaleDateString(dateLocale, {month:"2-digit", day:"2-digit"})`

Point shape: `{rawDate, label, hour, inputTokens, outputTokens, cacheCreationTokens, cacheReadTokens, cost: parseFiniteNumber(stat.totalCost) ?? null}`.

**Axes:**
| Axis | id | side | ticks |
|---|---|---|---|
| X | — | bottom | `dataKey="label"`, `axisLine={false}`, `tickLine={false}`, `fill hsl(var(--muted-foreground))`, `fontSize 12`, `dy={10}` |
| Y | `tokens` | left | `tickFormatter: v => ` `` `${(v/1000).toFixed(0)}k` `` |
| Y | `cost` | **right** (`orientation="right"`) | `tickFormatter: v => ` `` `$${v}` `` (raw, unformatted) |

`<CartesianGrid strokeDasharray="3 3" vertical={false} stroke="hsl(var(--border))" opacity={0.4}/>`, plus `<Legend/>` (default recharts legend).

**Series (5), all `type="monotone"`, `strokeWidth={2}`:**
| dataKey | yAxisId | name (i18n) | stroke | fill |
|---|---|---|---|---|
| `inputTokens` | tokens | `usage.inputTokens` | `#3b82f6` | `url(#colorInput)` |
| `outputTokens` | tokens | `usage.outputTokens` | `#22c55e` | `url(#colorOutput)` |
| `cacheCreationTokens` | tokens | `usage.cacheCreationTokens` | `#f97316` | `url(#colorCacheCreation)` |
| `cacheReadTokens` | tokens | `usage.cacheReadTokens` | `#a855f7` | `url(#colorCacheRead)` |
| `cost` | **cost** | `usage.cost` | `#f43f5e` | **`fill="none"`, `strokeDasharray="4 4"`** |

The four token areas use `fillOpacity={1}` over a `linearGradient` `x1=0 y1=0 x2=0 y2=1` with stops `5% stopOpacity 0.2` → `95% stopOpacity 0`, same hue as the stroke. The cost series is a dashed line only.

**Tooltip:** custom component (`:90-117`). Container `rounded-lg border bg-background/95 p-3 shadow-lg backdrop-blur-md`; bold `label` line; one row per payload entry coloured by `entry.color` with a `h-2 w-2` dot, `entry.name:`, and value = **`fmtUsd(entry.value, 6)` when `dataKey === "cost"`, else `fmtInt(entry.value, dateLocale)`**. Returns `null` when inactive.

**Header:** `h3` `usage.trends` + right-aligned `rangeLabel`.
**Loading:** `h-[350px]` box, `Loader2 h-8 w-8 animate-spin text-muted-foreground/30`.

---

## 1.11 `ProviderStatsTable` — `.../ProviderStatsTable.tsx` (103 L)

Data: `useProviderStats(range, {appType, providerName, model}, {refetchInterval})`. Loading: `h-[400px] animate-pulse rounded bg-gray-100` (**hard-coded light grey, not theme-aware**).

| Col | Header key | Align | Cell |
|---|---|---|---|
| 1 | `usage.provider` | left | `stat.providerName`, `font-medium` |
| 2 | `usage.requests` | right | `requestCount.toLocaleString()` |
| 3 | `usage.tokens` | right | `totalTokens.toLocaleString()` |
| 4 | `usage.cost` | right | `fmtUsd(totalCost, 4)` |
| 5 | `usage.successRate` | right | `` `${successRate.toFixed(1)}%` `` |
| 6 | `usage.avgLatency` | right | `` `${avgLatencyMs}ms` `` |

Empty: `colSpan={6}`, centered `usage.noData`. Row key `stat.providerId`.
**Edge case:** the empty branch tests `stats?.length === 0`, so while `stats` is `undefined` neither the empty row nor any data row renders — an empty table body.

## 1.12 `ModelStatsTable` — `.../ModelStatsTable.tsx` (97 L)

Data: `useModelStats(…)`. Same loading skeleton and same `stats?.length === 0` edge case.

| Col | Header key | Align | Cell |
|---|---|---|---|
| 1 | `usage.model` | left | `stat.model`, `font-mono text-sm` |
| 2 | `usage.requests` | right | `.toLocaleString()` |
| 3 | `usage.tokens` | right | `.toLocaleString()` |
| 4 | `usage.totalCost` | right | `fmtUsd(totalCost, 4)` |
| 5 | `usage.avgCost` | right | **`fmtUsd(avgCostPerRequest, 6)`** |

Empty `colSpan={5}`. Row key `stat.model`.

---

## 1.13 `RequestLogTable` — `.../RequestLogTable.tsx` (404 L)

`pageSize = 20` (const). Local state: `statusCode?: number`, `page`, `pageInput: string`.

**Filters** (`:64-72`): app/provider/model come from the dashboard top bar; only `statusCode` is local. `appType` is mapped `dashboardAppType && !== "all" ? it : undefined` here (not via `normalizeScopeFilters`).

**Page reset effect** (`:88-97`): `setPage(0)` on change of `dashboardAppType, providerName, model, range.customEndDate, range.customStartDate, range.preset`. **`range.liveEndTime` is NOT in the dep list** — toggling live-end alone does not reset the page.

**Controls bar:** status `Select` (w-100px) with items `all` (`common.all`), `200 OK`, `400`, `401`, `429`, `500`; changing it also `setPage(0)`. Then `UsageDateRangePicker` rendered **only if `onRangeChange` is provided**.

**Loading:** `h-[400px] animate-pulse rounded bg-gray-100`.

**Columns (9, all `text-center whitespace-nowrap`):**
| # | Header key | Cell |
|---|---|---|
| 1 | `usage.time` | `new Date(createdAt*1000).toLocaleString(locale, {month:"2-digit",day:"2-digit",hour:"2-digit",minute:"2-digit"})` |
| 2 | `usage.provider` | `providerName \|\| t("usage.unknownProvider")` |
| 3 | `usage.billingModel` | if `requestModel && requestModel !== model`: `{requestModel}` + muted `" → " + model`; else `model`. `title` = `` `${requestModel} → ${model}` `` or `model`. `font-mono text-xs max-w-[200px] truncate` |
| 4 | `usage.inputTokens` | `fmtInt(getFreshInputTokens(log), locale)`; `title` = `` `Raw: ${inputTokens.toLocaleString()}` `` **only when cache-inclusive** (`inputTokens !== freshInput`). Sub-line when `cacheRead>0 \|\| cacheCreation>0`: `R{n}` and/or `W{n}` joined by **`·`**, `text-[10px]` |
| 5 | `usage.outputTokens` | `fmtInt(outputTokens, locale)` |
| 6 | `usage.totalCost` | `unpriced ? t("usage.unpriced","未定价") : fmtUsd(totalCostUsd, 4)`, muted when unpriced. Sub-line `×{m.toFixed(2)}` when `costMultiplier` parses finite **and ≠ 1** |
| 7 | `usage.timingInfo` | `` `${(latencyMs/1000).toFixed(1)}s` `` + muted `` `/${(firstTokenMs/1000).toFixed(1)}s` `` when `firstTokenMs != null` |
| 8 | `usage.status` | `statusCode`, `text-green-600` if 2xx else `text-red-600` |
| 9 | `usage.source` (defaultValue `"Source"`) | `dataSource \|\| "proxy"` |

Empty: `colSpan={9}` centered `usage.noData`. Row key `log.requestId`. **No row click** — see B6.

**Pagination** (`:323-399`): `totalPages = Math.ceil(total/pageSize)`.
- Prev button disabled at `page === 0`; Next disabled at `page >= totalPages - 1`.
- Page buttons: if `totalPages <= 9` render all; else a `Set` of `{0,1,2} ∪ {tp-3,tp-2,tp-1} ∪ [page-1, page+1]` clamped to range, sorted ascending, with a `…` span (`key = ellipsis-${i}`) inserted wherever consecutive entries differ by more than 1. Label is `p+1`; current page uses `variant="default"`, others `"outline"`.
- Jump box: text `Input` (w-16) + button `usage.goToPage`, placeholder `usage.pageInputPlaceholder`. `handleGoToPage`: **`/^\d+$/` on the trimmed value; silently no-ops if not all digits or if `parsed < 1 || parsed > totalPages`**; else `setPage(parsed-1)` and clear the box. Enter key triggers it.
- Left side: `t("usage.totalRecords", { total })`.

---

## 1.14 `RequestDetailPanel` — `.../RequestDetailPanel.tsx` (324 L) — **unmounted, see B6**

`useRequestDetail(requestId)` → `get_request_detail`. Renders as a `Dialog max-w-2xl max-h-[80vh] overflow-y-auto`, `onOpenChange={onClose}`.
**Locale derivation is its own ad-hoc chain** (`:22-29`) — exact equality on `i18n.language` against `"zh"`/`"zh-TW"`/`"ja"` else `"en-US"` — it does **not** use `getLocaleFromLanguage`. Port should unify.

States: loading → `h-[400px] animate-pulse`; `!request` → title `usage.requestDetail` + centered `usage.requestNotFound`.

Sections (each a `rounded-lg border p-4` with an `h3`, `dl` `grid-cols-2 gap-3 text-sm`):
1. **`usage.basicInfo`** — `usage.requestId` (mono), `usage.time` (full `toLocaleString`), `usage.provider` (name + mono grey `providerId`), `usage.appType` (**raw `request.appType` — this is where the un-folded `claude-desktop` value surfaces**), `usage.model` (mono; conditionally `usage.requestModel` when `requestModel !== model`; conditionally `usage.pricingModel` when `pricingModel !== model`), `usage.status` (pill, `bg-green-100 text-green-800` for 2xx else red).
2. **`usage.tokenUsage`** — `usage.inputTokens` = `freshInput.toLocaleString()` plus `({usage.rawInputLabel}: {inputTokens})` when cache-inclusive; `usage.outputTokens`; `usage.cacheReadTokens`; `usage.cacheCreationTokens`; col-span-2 `usage.totalTokens` = **`freshInput + outputTokens`** (cache excluded — differs from Hero's `realTotalTokens`).
3. **`usage.costBreakdown`** — four costs each `$${parseFloat(x).toFixed(6)}` with a `({usage.baseCost})` suffix; conditional col-span-2 `usage.costMultiplier` = `×{costMultiplier}` when it parses ≠ 1; total row `usage.totalCost` (+ `({usage.withMultiplier})` when multiplier ≠ 1) = `usage.unpriced` or `$…toFixed(6)`, `text-primary` or muted. The border class shifts between the multiplier row and the total row depending on whether the multiplier row rendered.
4. **`usage.performance`** — only `usage.latency` = `{latencyMs}ms`.
5. **Conditional `usage.errorMessage`** — red-bordered `bg-red-50` block.

---

## 1.15 `UsageDateRangePicker` — `.../UsageDateRangePicker.tsx` (507 L)

`PRESETS = ["today","1d","7d","14d","30d"]` (**`custom` excluded** — reached by Confirm).

### Props
`selection: UsageRangeSelection`, `onApply(selection)`, `triggerLabel: string`.

### Helpers (`:34-103`)
`startOfDay`, `isSameDay`, `toTs(d)=floor(ms/1000)`, `fromTs`, `fmtDate → "YYYY-MM-DD"`, `fmtTime → "HH:MM"`, `parseDateInput(ts, "Y-M-D")` (keeps the existing h/m; returns `ts` unchanged on non-finite parts), `parseTimeInput(ts,"H:M")` (keeps y/m/d), `setDateKeepTime(ts, day)`, `getCalendarDays(month)` → **exactly 42 days** starting from `first - first.getDay()` (Sunday-first, fixed 6×7 grid).

### State
`open`, `activeField: "start"|"end"`, `draftStart`, `draftEnd`, `draftLiveEnd`, `displayMonth`, `error: string|null`.

### Effects
- On **open** (`:138-155`): re-resolve from `selection`; reset draft start/end; `draftLiveEnd = selection.preset==="custom" ? (liveEndTime ?? false) : false`; `displayMonth` = month of resolved start; `activeField="start"`; clear error.
- **Live tick** (`:158-164`): while `open && draftLiveEnd`, `setDraftEnd(floor(Date.now()/1000))` immediately then **every 1000 ms**; cleared on close or untoggle.

### Trigger
`Button` h-9 w-100px, `variant = selection.preset === "custom" ? "default" : "outline"`, `CalendarDays` icon, truncated `triggerLabel`, `ChevronDown` at 50% opacity, `title={triggerLabel}`.

### Popover (`align="end"`, `w-[620px] max-w-[calc(100vw-2rem)] p-3`, class `usage-range-popover`)
1. **Preset row** (bordered bottom): five `size="sm"` buttons; `variant="default"` for the active preset. Click → `onApply({preset})` **and close immediately** (discards drafts).
2. **Fields column** (`usage-range-fields`): hint `usage.customRangeHint` (defaultValue mentions "最长 30 天" but **no 30-day cap is enforced anywhere**), then start card, end card, live checkbox, error, buttons.
   - `renderField(field)` (`:248-317`): card styling has three states — end-field-while-live (`opacity-50 cursor-not-allowed`), active (`border-primary ring-1 ring-primary/30 bg-primary/5`), idle. Contains a `type="date"` input (`fmtDate`) and a `type="time" step={60}` input (`fmtTime`). Both set `activeField` on focus and on card click; when live, the end card is `readOnly` + `pointer-events-none` and all handlers early-return. Changing the date input also moves `displayMonth` to that month.
   - **Live checkbox** (`:365-380`): label `usage.liveEndTime`. Checking it sets `draftEnd = now` and forces `activeField="start"`.
   - Buttons: ghost `common.cancel` (just closes, discards), primary `common.confirm` → `handleApply`.
3. **Calendar** (`usage-range-calendar`): month nav `ChevronLeft`/`ChevronRight` (h-7 w-7 ghost); the centre month label is a **button that jumps `displayMonth` to the current month** (`goToToday`, `title = usage.presetToday`), formatted `{year:"numeric", month:"long"}`. Weekday headers via `Intl.DateTimeFormat(locale,{weekday:"narrow"})` over `new Date(2024,0,7+i)` (2024-01-07 is a Sunday). Day grid `grid-cols-7 gap-px`, buttons `h-7` with `aria-label` = localized date, `aria-current="date"` for today, `aria-pressed` for endpoints. Classes: outside-month `text-muted-foreground/30`; in-range non-endpoint `bg-primary/10 text-primary`; endpoint `bg-primary text-primary-foreground font-medium`; today non-endpoint `ring-1 ring-primary/40`. `inRange` compares **`startOfDay`-normalized** values inclusively.

### `handleDatePick(day)` (`:186-226`)
- Clear error.
- If `draftLiveEnd`: the calendar controls **only** the start date — `setDraftStart(setDateKeepTime(draftStart, day))` and return.
- `activeField === "start"`: set start; **if the new start > draftEnd, push end to the same value**; then auto-advance `activeField` to `"end"`.
- `activeField === "end"`: if the picked value < draftStart, treat it as a **new start** and keep `activeField="end"`; else set end.
- If the picked day is outside `displayMonth`, navigate `displayMonth` to it.

### `handleApply` (`:228-241`)
If `draftStart > draftEnd` → set `error = t("usage.invalidTimeRangeOrder", "开始时间不能晚于结束时间")` and **do not close**. Else `onApply({preset:"custom", customStartDate: draftStart, customEndDate: draftEnd, liveEndTime: draftLiveEnd})` and close.

---

## 1.16 `PricingConfigPanel` — `.../PricingConfigPanel.tsx` (482 L)

`PRICING_APPS = ["claude","codex","gemini","grokbuild"]` (**no `opencode`**). `PricingModelSource = "request" | "response"`.

### Part A — global billing defaults
State `appConfigs: Record<PricingApp,{multiplier:string; source:PricingModelSource}>`, default `{multiplier:"1", source:"response"}` for all four; plus `originalConfigs` (for dirty detection), `isConfigLoading`, `isSaving`.

**Load** (`:78-135`), on mount, **not** React Query — a raw `Promise.all` over the four apps, each doing `Promise.all([proxyApi.getDefaultCostMultiplier(app), proxyApi.getPricingModelSource(app)])`. Source is coerced: `=== "request" ? "request" : "response"`. Guarded by an `isMounted` flag. On failure → `toast.error(t("settings.globalProxy.pricingLoadFailed",{error}))`. **The effect depends on `[t]`, so a language switch re-runs the whole load and discards unsaved edits.**

`isDirty` = `originalConfigs !== null && any app differs in multiplier or source`. Save button `disabled={isConfigLoading || isSaving || !isDirty}`.

**Save** (`:138-182`): for each app, trim the multiplier; empty → `toast.error(\`${t(\`apps.${app}\`)}: ${t("settings.globalProxy.defaultCostMultiplierRequired")}\`)` and abort; failing `isNonNegativeDecimalString` → same pattern with `…Invalid`. Then `Promise.all` over `flatMap`: `setDefaultCostMultiplier(app, trimmed)` + `setPricingModelSource(app, source)`. Success → `toast.success(t("settings.globalProxy.pricingSaved"))` and `setOriginalConfigs({...appConfigs})` (**note: stores the untrimmed values, so a trailing space re-marks it dirty**). Failure → `pricingSaveFailed`.

**Table:** 3 columns — `pricingAppLabel`, `defaultCostMultiplierLabel`, `pricingModelSourceLabel`. Rows keyed by app, label `t(\`apps.${app}\`)`. Multiplier is `Input type="number" step="0.01" min="0" inputMode="decimal"` (h-7 w-24, placeholder `1`); source is a `Select` (h-7 w-28) with items `response` (`pricingModelSourceResponse`) then `request` (`pricingModelSourceRequest`). All inputs `disabled={isSaving}`.

### Part B — per-model pricing
`useModelPricing()` + `useDeleteModelPricing()`.
- Panel-level loading → centered `Loader2 h-5 w-5`; error → destructive `Alert` `"{usage.loadPricingError}: {String(error)}"`.
- Header: `{t("usage.modelPricingDesc")} {t("usage.perMillion")}` + `common.add` button (calls `e.stopPropagation()` first — it sits inside an Accordion trigger context).
- `handleAddNew` (`:190-200`) sets `isAddingNew = true` and seeds `editingModel = {modelId:"", displayName:"", inputCostPerMillion:"0", outputCostPerMillion:"0", cacheReadCostPerMillion:"0", cacheCreationCostPerMillion:"0"}`.
- Empty → `Alert` `usage.noPricingData`.
- Table columns: `usage.model` (mono), `usage.displayName`, then right-aligned `usage.inputCost`, `usage.outputCost`, `usage.cacheReadCost`, `usage.cacheWriteCost` (each rendered as **`${value}` raw string, no `toFixed`**), `common.actions` = ghost `Pencil` (`title=common.edit`, sets `isAddingNew=false` + `editingModel=model`) and ghost `Trash2` (`title=common.delete`, `text-destructive`, sets `deleteConfirm=modelId`).
- `PricingEditModal` mounted when `editingModel` truthy; `onClose` clears both `editingModel` and `isAddingNew`.
- **Delete confirm `Dialog`**: title `usage.deleteConfirmTitle`, desc `usage.deleteConfirmDesc`, outline `common.cancel`, destructive button showing `common.deleting` while `deleteMutation.isPending` else `common.delete`. `handleDelete` clears `deleteConfirm` in the mutation's own `onSuccess`. **No success toast** on delete; the mutation's `onSuccess` invalidates `usageKeys.all`.

---

## 1.17 `PricingEditModal` — `.../PricingEditModal.tsx` (259 L)

Rendered in a `FullScreenPanel` (not a Dialog). `PRICE_INPUT_STEP = "0.0001"`.

Title: `usage.addPricing` when new, else `` `${t("usage.editPricing")} - ${model.modelId}` ``.
Footer: a single submit button bound via `form="pricing-form"`; icon `Plus` (new) / `Save` (edit); label `common.saving` while pending, else `common.add` / `common.save`; `disabled={updatePricing.isPending}`.

**Form state** is seeded once from the `model` prop (`:32-39`) and maps `*PerMillion` → `inputCost`/`outputCost`/`cacheReadCost`/`cacheCreationCost`.

**Fields, in order:**
1. *(new only)* a hint strip: `usage.modelsDevHint` + outline button `usage.importFromModelsDev` with a `Globe` icon → opens the picker.
2. *(new only)* `usage.modelId`, `required`, placeholder `usage.modelIdPlaceholder`.
3. `usage.displayName`, `required`, placeholder `usage.displayNamePlaceholder`.
4. `usage.inputCostPerMillion` — `type="number" step="0.0001" min="0" required`.
5. `usage.outputCostPerMillion` — same.
6. `usage.cacheReadCostPerMillion` — same.
7. `usage.cacheCreationCostPerMillion` — same.

**`handleSubmit`** (`:41-86`): `preventDefault`; if new and `modelId.trim()` empty → `toast.error(usage.modelIdRequired)`; then each of the four costs must pass `isNonNegativeDecimalString` else `toast.error(usage.invalidPrice)`. Calls `updatePricing.mutateAsync({modelId: isNew ? formData.modelId : model.modelId, …})` — **the edit path always uses the original `model.modelId`, so the ID is immutable once created**. Success → `toast.success(usage.pricingAdded | usage.pricingUpdated, {closeButton:true})` + `onClose()`. Catch → `toast.error(String(error))`.

`ModelsDevPickerDialog` is mounted only when `isNew && isPickerOpen`; its `onImported` closes both the picker **and** the edit panel.

---

## 1.18 `ModelsDevPickerDialog` — `.../ModelsDevPickerDialog.tsx` (452 L)

**Network:** plain `fetch("https://models.dev/api.json")` (not a Tauri command), wrapped in `useQuery({queryKey:["models-dev-pricing"], enabled: open, staleTime: 3_600_000, retry: 1})`; non-`ok` → `throw new Error(\`HTTP ${res.status}\`)`.

`DEFAULT_VISIBLE_ROWS = 50`, `MAX_VISIBLE_ROWS = 200` (dataset ≈ 5000 rows).

### Exported pure functions (unit-testable; port these first)
```ts
export function normalizeModelIdForPricing(modelId: string): string
```
Must mirror the backend `clean_model_id_for_pricing` (`usage_stats.rs`): take the segment after the **last** `/`, cut at the first `:`, trim, replace **all** `@` with `-`, lowercase, then strip a trailing `[1m]` and trim again. Cost-attribution queries use this normalized form, so an un-normalized ID would never match.

```ts
export function formatPrice(value: number): string
```
Non-finite or `<= 0` → `"0"`; `>= 1e12` → `"0"` (guards `toFixed` degrading to exponential — such a "price" is dirty data); else `value.toFixed(6)` with trailing zeros stripped then a trailing `.` stripped, `|| "0"`. **Never use `String(n)`** — decimals would serialize as scientific notation and the backend parser would reject them.

```ts
export function flattenModels(data: ModelsDevResponse): ModelsDevEntry[]
```
Skip non-object providers; `providerName = provider.name || providerId`; for each model read `cost.{input,output,cache_read,cache_write}` accepting only `typeof === "number"`; **skip the entry when both `input` and `output` are absent**; skip when `normalizedId` is empty; missing cache values default to `0`. Sort by `releaseDate.localeCompare` **descending**, tie-break `modelName` ascending.

Entry shape: `{key: \`${providerId}/${modelId}\`, providerId, providerName, modelId, normalizedId, modelName, releaseDate, input, output, cacheRead, cacheWrite}`.

### Behaviour
- On `open` → reset `search`, `providerFilter="all"`, `selected=null`.
- `providers`: de-duplicated `Map<providerId, providerName>`, sorted by name.
- `isFiltering = search.trim() !== "" || providerFilter !== "all"`.
- `filtered`: provider match AND (no query OR query substring-matches any of `modelId` / `normalizedId` / `modelName` / `providerName`, all lowercased).
- `visible = filtered.slice(0, isFiltering ? 200 : 50)`.
- **Single-selection only** (`toggleEntry`, `:213-215`): clicking a different row replaces the selection, clicking the selected row deselects. The comment explains why: batch import would trigger one full zero-cost backfill scan per row inside `update_model_pricing`.
- `handleImport`: `updatePricing.mutateAsync({modelId: selected.normalizedId, displayName: selected.modelName, inputCost: formatPrice(input), outputCost: formatPrice(output), cacheReadCost: formatPrice(cacheRead), cacheCreationCost: formatPrice(cacheWrite)})` → `toast.success(t("usage.modelsDevImported",{name}))` + `onImported()`. Catch → `toast.error(String(error))`.

### Layout
`DialogContent zIndex="top" max-w-3xl h-[80vh]`, `onOpenChange` ignores close requests while `updatePricing.isPending`. **`onEscapeKeyDown` calls `preventDefault()` when `isTextEditableTarget(e.target)`** — ESC inside the search box must not close the dialog and lose the selection.
Header: `usage.modelsDevPickerTitle` / `usage.modelsDevPickerDesc`.
Body states: loading → centred `Loader2`; error → destructive `Alert` `"{usage.modelsDevLoadError}: {msg}"` with an inline outline `usage.modelsDevRetry` button calling `refetch()`; else the toolbar (provider `Select` w-44 with `usage.modelsDevAllProviders`, then a `Search`-prefixed `Input` with `usage.modelsDevSearchPlaceholder`) and the scroll list.
Empty list → centred `usage.modelsDevNoResults`.
Row: `role="button"`, `aria-pressed`, selected → `bg-accent/50` else `hover:bg-muted/40`; a `Check` icon toggled between `visible`/`invisible` (space always reserved); name + provider + optional `releaseDate`; second line = `normalizedId` in mono with `title={modelId}` (the raw ID); then four fixed `w-16` price columns labelled `usage.inputCost`, `usage.outputCost`, `usage.cacheReadCost`, `usage.cacheWriteCost` showing `${formatPrice(v)}`.
Truncation footer when `filtered.length > visible.length`: `usage.modelsDevTruncated` (when filtering) or `usage.modelsDevDefaultHint`, both interpolating `{shown, total}`.
Footer: outline `common.cancel` (disabled while pending) and primary import button `disabled={!selected || isPending}` showing `usage.modelsDevImporting` + spinner, else `usage.modelsDevImportButton`.

---

## 1.19 `ConnectivityCheckConfigPanel` — `.../ConnectivityCheckConfigPanel.tsx` (169 L)

Host: `SettingsPage.tsx:478`, Advanced tab, accordion item `connectivityCheck` (`FlaskConical` `text-emerald-500`, `settings.advanced.connectivityCheck.title`/`.description`).

**Types** — `/home/user/cc-switch/src/lib/api/connectivity-check.ts`:
```ts
export type HealthStatus = "operational" | "degraded" | "failed";

export interface StreamCheckConfig {
  /** 单次探测超时（秒） */        timeoutSecs: number;
  /** 超时类失败的最大重试次数 */   maxRetries: number;
  /** 降级阈值（毫秒）：可达但 TTFB 超过该值判定为"较慢" */ degradedThresholdMs: number;
}

export interface StreamCheckResult {
  status: HealthStatus; success: boolean; message: string;
  responseTimeMs?: number; httpStatus?: number;
  testedAt: number; retryCount: number;
}
```
File-header contract: the check only probes whether `base_url` is reachable — **it sends no real model request and never touches the failover circuit breaker.**

**Commands:**
| Fn | Command | Args | Returns |
|---|---|---|---|
| `streamCheckProvider` | `stream_check_provider` | `{appType, providerId}` | `StreamCheckResult` |
| `streamCheckAllProviders` | `stream_check_all_providers` | `{appType, proxyTargetsOnly = false}` | `Array<[string, StreamCheckResult]>` (tuple pairs) |
| `getStreamCheckConfig` | `get_stream_check_config` | — | `StreamCheckConfig` |
| `saveStreamCheckConfig` | `save_stream_check_config` | `{config}` | `void` |

**Panel:** no React Query — `useState` + `useEffect(loadConfig, [])`. All three values held as **strings** so the number inputs can be fully cleared; defaults `"8"`, `"1"`, `"6000"`. Load error → local `error` state rendered as a destructive `Alert` above.

Layout: destructive error `Alert` (conditional) → informational `Alert` with `Info` icon and `streamCheck.connectivityNote` (long defaultValue: reachable ≠ correctly configured) → `h4` `streamCheck.checkParams` → a `grid-cols-1 md:grid-cols-3`:

| Field | id | key | input attrs |
|---|---|---|---|
| timeout | `timeoutSecs` | `streamCheck.timeout` | `number min=2 max=60` |
| retries | `maxRetries` | `streamCheck.maxRetries` | `number min=0 max=5` |
| degraded threshold | `degradedThresholdMs` | `streamCheck.degradedThreshold` | `number min=1000 max=30000 step=1000` |

→ right-aligned Save button (`Loader2` + `common.saving` while saving, else `Save` + `common.save`).

**`handleSave`** (`:48-70`): `parseNum(v, default) = isNaN(parseInt(v)) ? default : parsed` — defaults `8/1/6000`. **`min`/`max` are HTML hints only; nothing clamps**, so a typed `999` is persisted verbatim. `0` is explicitly a valid value. Toasts: `streamCheck.configSaved` (with `closeButton`) / `` `${t("streamCheck.configSaveFailed")}: ${String(e)}` ``.

---

## 1.20 `DataSourceBar` — `.../DataSourceBar.tsx` (125 L) — **unmounted, see B7**

Query key `[...usageKeys.all, "data-sources"]` → `get_usage_data_sources`; `refetchInterval: ms>0 ? ms : false`, `refetchIntervalInBackground: false`.
`DATA_SOURCE_ICONS`: `proxy`/`codex_db` → `Database`; `session_log`/`codex_session`/`gemini_session`/`opencode_session` → `FileText`; unknown → `Database`.
Renders `null` when there are no sources. Label `usage.dataSources`; per-source chip `t(\`usage.dataSource.${id}\`, {defaultValue: id})` + count.
`handleSync` → `sync_session_usage`: `imported > 0` → `toast.success(t("usage.sessionSync.imported",{count}))` + `invalidateQueries(usageKeys.all)`; `imported === 0` → `toast.info(usage.sessionSync.upToDate)`; throw → `toast.error(usage.sessionSync.failed)`. Button label is `usage.sessionSync.resync` when any non-`proxy` source exists, else `usage.sessionSync.import`; `title` = `usage.sessionSync.trigger`.

---

# 2. UsageFooter and quota footers

## 2.0 Dispatch — `/home/user/cc-switch/src/components/providers/ProviderCard.tsx`

Precedence for the inline slot (`:503-541`), first match wins:
1. `isCopilot` → `<CopilotQuotaFooter meta inline isCurrent/>` — `isCopilot = meta.providerType === "github_copilot" || meta.usage_script.templateType === "github_copilot"` (`:222`)
2. `isCodexOauth` (`meta.providerType === "codex_oauth"`) → `<CodexOauthQuotaFooter meta inline isCurrent/>`
3. `isOfficial` → `<SubscriptionQuotaFooter appId inline isCurrent autoQueryInterval={meta.usage_script?.autoQueryInterval ?? 0}/>` **only when `officialSubscriptionEnabled`** (`isOfficial && appId ∈ {claude,codex,gemini} && usageEnabled && templateType === "official_subscription"`), else `null`
4. `hasMultiplePlans` → a `usage.multiplePlans` count chip + an expand toggle
5. otherwise → `<UsageFooter … inline/>`

Expanded state (`:620-631`) renders `<UsageFooter … inline={false}/>` below the card.

`shouldAutoQuery` in the card (`:255`) uses `isInConfig` for `opencode | openclaw | hermes`, `isCurrent` otherwise. `hasMultiplePlans = usage?.success && data.length > 1 && !isTokenPlan`.

## 2.1 `UsageFooter` — `/home/user/cc-switch/src/components/UsageFooter.tsx` (452 L)

### Props
`provider`, `providerId`, `appId`, `usageEnabled`, `isCurrent`, `isInConfig = false`, `inline = false`.

### Data
`isTokenPlan = provider.meta?.usage_script?.templateType === "token_plan"` (`:56`).
`shouldAutoQuery = appId === "opencode" ? isInConfig : isCurrent` — **note this differs from ProviderCard's three-app list; UsageFooter checks only `opencode`** (`:60`).
`autoQueryInterval = shouldAutoQuery ? meta.usage_script?.autoQueryInterval || 0 : 0`.
`useUsageQuery(providerId, appId, {enabled: usageEnabled, autoQueryInterval})` → `{data: usage, isFetching: loading, isError, lastQueriedAt, refetch}`.

**Relative-time ticker** (`:77-88`): `now` state refreshed every **30 000 ms**, only while `lastQueriedAt` is set.

### Render gates, in order
1. `if (!usageEnabled || (!usage && !isError)) return null` (`:93`). The comment explains the `isError` term: on a first-query failure `data` is empty, and without this the footer would vanish with no retry affordance.
2. `if (!usage || !usage.success)` → **error state**. Inline: a bordered chip, `AlertCircle size=12` red, `usage.queryFailed`, refresh button. Expanded: `mt-3 rounded-xl` card showing **`usage.error || t("usage.queryFailed")`** (inline never shows the backend message) + refresh.
3. `usageDataList = usage.data || []`; `if (length === 0) return null`.
4. **Token-plan inline branch** (`isTokenPlan && inline`, `:144-188`): two rows. Row 1 = `Clock size=10` + relative time (or `usage.never`) + refresh. Row 2 = optional `💰 {planLabel}` (taken from `tiers[0].planLabel`) then one `<TierBadge>` per entry — **reuses the official-subscription badge component**.
5. **Generic inline branch** (`:191-275`): uses **only `usageDataList[0]`**. `isExpired = firstUsage.isValid === false`. Row 1 identical to above. Row 2: `usage.used` + `used.toFixed(2)`; `usage.remaining` + `remaining.toFixed(2)` coloured red when expired, **orange when `remaining < (total || remaining) * 0.1`** (note: when `total` is undefined this compares `r < r*0.1`, always false), green otherwise; then `unit`; then `extra` truncated to `max-w-[150px]` with a `title`.
6. **Expanded branch** (`:277-310`): card with header `usage.planUsage` + relative time + refresh, then one `<UsagePlanItem>` per element (keyed by index).

### `toQuotaTier(data)` (`:21-43`)
If `data.extra` starts with `"{"`, `JSON.parse` it and build `{name: planName || "", utilization: used || 0, resetsAt: parsed.resetsAt || null, usedValueUsd: parsed.usedValueUsd ?? null, maxValueUsd: parsed.maxValueUsd ?? null, planLabel: parsed.planLabel ?? null}`; on parse failure fall through to `{name, utilization, resetsAt: extra || null}`. So the token-plan path smuggles structured data through the free-text `extra` field.

### `UsagePlanItem` (`:316-428`)
Three fixed-width columns: **25 %** plan name (`💰 {planName}` truncated with `title`, red when expired; `—` at 50 % opacity when absent), **30 %** `extra` (truncated, red when expired) plus an `invalidMessage || t("usage.invalid")` pill, **45 %** right-aligned metrics — `usage.total` (with **`total === -1` rendered as `∞`**), `|`, `usage.used`, `|`, `usage.remaining` (same red/orange/green rule), then `unit`. Each metric block renders only when the field is `!== undefined`.

### `formatRelativeTime(ts, now, t)` (`:431-450`)
`<60 s` → `usage.justNow`; `<3600` → `usage.minutesAgo {count}`; `<86400` → `usage.hoursAgo {count}`; else `usage.daysAgo {count}`. **Duplicated verbatim in `SubscriptionQuotaFooter.tsx:82-94` and `CopilotQuotaFooter.tsx:21-33` — collapse to one helper in the port.**

## 2.2 `SubscriptionQuotaFooter` — `/home/user/cc-switch/src/components/SubscriptionQuotaFooter.tsx` (431 L)

### Types — `/home/user/cc-switch/src/types/subscription.ts`
```ts
export type CredentialStatus = "valid" | "expired" | "not_found" | "parse_error";
export interface QuotaTier {
  name: string; utilization: number; /* 0-100 */ resetsAt: string | null;
  usedValueUsd?: number | null; maxValueUsd?: number | null; planLabel?: string | null;
}
export interface ExtraUsage {
  isEnabled: boolean; monthlyLimit: number | null; usedCredits: number | null;
  utilization: number | null; currency: string | null;
}
export interface SubscriptionQuota {
  tool: string; credentialStatus: CredentialStatus; credentialMessage: string | null;
  success: boolean; tiers: QuotaTier[]; extraUsage: ExtraUsage | null;
  error: string | null; queriedAt: number | null;
}
```

### `TIER_I18N_KEYS` (exported, `:25-42`)
| tier name | key |
|---|---|
| `five_hour` | `subscription.fiveHour` |
| `seven_day` | `subscription.sevenDay` |
| `seven_day_opus` | `subscription.sevenDayOpus` |
| `seven_day_sonnet` | `subscription.sevenDaySonnet` |
| `30_day` | `subscription.thirtyDay` (Codex free plan's secondary window is 30 d; paid is 7 d) |
| `gemini_pro` / `gemini_flash` / `gemini_flash_lite` | `subscription.geminiPro` / `…Flash` / `…FlashLite` |
| `weekly_limit` | `subscription.sevenDay` (Token Plan alias) |
| `monthly` | `subscription.monthly` (Volcengine Agent/Coding plan) |
| `premium` | `subscription.copilotPremium` |

**Tiers whose `name` is not in this map are dropped entirely** (`:213-216`), and if that leaves zero tiers the component returns `null`.

### Helpers
- `utilizationColor(u)`: `≥90` red, `≥70` orange, else green (`text-green-600 dark:text-green-400`).
- `countdownStr(resetsAt)`: `null` when absent or already elapsed; `>24 h` → `` `${d}d${h%24}h` ``; `>0 h` → `` `${h}h${m}m` ``; else `` `${m}m` ``.
- `formatResetTime` wraps it in `t("subscription.resetsIn",{time})`.
- `HIDDEN_INLINE_TIERS = new Set(["seven_day_sonnet"])` — filtered out of the inline row only.

### `SubscriptionQuotaView` — the shared renderer (5 states × inline/expanded)
Injected props: `quota`, `loading`, `refetch`, `appIdForExpiredHint`, `inline`. Own 30 s `now` ticker gated on `quota?.queriedAt`.
1. `!quota || credentialStatus === "not_found"` → `null`
2. `credentialStatus === "parse_error"` → `null` (silent by design)
3. `credentialStatus === "expired" && !success` → amber block (`border-amber-200 dark:border-amber-800 bg-amber-50 dark:bg-amber-900/20`), `subscription.expired`; **expanded also shows `t("subscription.expiredHint",{tool: appIdForExpiredHint})`**; refresh button `title=subscription.refresh`
4. `!success` → red failure; inline shows `subscription.queryFailed`, expanded shows `quota.error || t("subscription.queryFailed")`
5. success → inline: two rows (time + refresh; then `TierBadge` per visible tier). Expanded: header `subscription.title` + time + refresh, one `TierBar` per tier, then a conditional **extra-usage** line (`quota.extraUsage?.isEnabled`) reading `subscription.extraUsage` + `{$}{usedCredits.toFixed(2)}` and `` ` / {$}{monthlyLimit.toFixed(2)}` `` when the limit is non-null; the `$` prefix appears only when `currency === "USD"`.

`TierBadge` (exported, `:308-340`): `label: utilization%` via `t("subscription.utilization",{value: Math.round(u)})` coloured by `utilizationColor`; optional `($used/$max)` when **both** `usedValueUsd` and `maxValueUsd` are non-null; optional `Clock` + `countdownStr`.
`TierBar` (`:343-395`): 25 % label, flex-1 `h-2` track with a fill at `min(u,100)%` coloured `bg-red-500 / bg-orange-500 / bg-green-500` at the same 90/70 breakpoints, then 30 % column with `Math.round(u)%` and the truncated reset text.

### The thin wrapper (`:401-429`)
```ts
useSubscriptionQuota(appId, isCurrent, isCurrent && autoQueryInterval > 0, autoQueryInterval)
```
default `autoQueryInterval = 5`. **Returns `null` when `!isCurrent`** — after the hook runs, so hook order is stable.

### `useSubscriptionQuota` — `/home/user/cc-switch/src/lib/query/subscription.ts:79-105`
`subscriptionKeys.quota(appId) = ["subscription","quota",appId]`; `queryFn: subscriptionApi.getQuota(appId)`; **`enabled: enabled && ["claude","codex","gemini"].includes(appId)`**; `refetchInterval = autoQuery && mins>0 ? max(mins,1)*60000 : false`; `refetchIntervalInBackground` / `refetchOnWindowFocus` = `Boolean(refetchInterval)`; `staleTime = mins>0 ? max(mins,1)*60000 : 300000`; `retry: 1`. Wrapped in `useQuotaKeepLastGood(query, appId)`.

## 2.3 `CodexOauthQuotaFooter` — `/home/user/cc-switch/src/components/CodexOauthQuotaFooter.tsx` (41 L)
Pure adapter: `useCodexOauthQuota(meta, {enabled: true, autoQuery: isCurrent})` → `<SubscriptionQuotaView quota loading refetch appIdForExpiredHint="codex_oauth" inline/>`. Reuses all five states. Unlike the CLI wrapper it does **not** return `null` when not current — it just stops polling.

`useCodexOauthQuota` (`subscription.ts:122-140`): `accountId = resolveManagedAccountId(meta, PROVIDER_TYPES.CODEX_OAUTH)`; key `["codex_oauth","quota", accountId ?? "default"]` (so cards bound to the same account share one request, and `null` falls back to the backend default account); `queryFn: subscriptionApi.getCodexOauthQuota(accountId)`; `refetchInterval/InBackground/OnWindowFocus` all gated on `autoQuery`, interval `300000`; `staleTime: 300000`; `retry: 1`; wrapped with `useQuotaKeepLastGood(query, accountId ?? "default")`.

## 2.4 `CopilotQuotaFooter` — `/home/user/cc-switch/src/components/CopilotQuotaFooter.tsx` (189 L)
`accountId = resolveManagedAccountId(meta, PROVIDER_TYPES.GITHUB_COPILOT)`; `useCopilotQuota(accountId, {enabled:true, autoQuery:isCurrent})`. Own 30 s ticker.

**Does NOT reuse `SubscriptionQuotaView`** — it has its own 3-state render and its own `TierBar` copy, but imports `TierBadge` and `utilizationColor`.
1. `!quota` → `null`
2. `!quota.success` → inline: red chip showing **`quota.error || t("subscription.queryFailed")`**; **expanded: `return null`** (no error UI at all)
3. `tiers.length === 0` → `null`
4. inline success: row 1 = optional `quota.plan` text + `Clock` + relative time (`usage.never` defaultValue **`"Never"`**, unlike the other footers' `"从未更新"`) + refresh; row 2 = `TierBadge` per tier
5. expanded success: header `quota.plan || t("subscription.title")` + time + refresh; per tier a 25 % label **hard-coded to `t("subscription.copilotPremium",{defaultValue:"Premium"})`**, a bar with the same 90/70 colour rule, and `Math.round(u)%`.

### `useCopilotQuota` — `/home/user/cc-switch/src/lib/query/copilot.ts` (63 L)
```ts
export interface CopilotQuota {
  success: boolean; plan: string | null; resetDate: string | null;
  tiers: QuotaTier[]; error: string | null; queriedAt: number | null;
}
```
Key `["copilot","quota", accountId ?? "default"]`. `queryFn` calls `copilotGetUsageForAccount(accountId)` or `copilotGetUsage()`, then derives from `usage.quota_snapshots.premium_interactions`:
`utilization = entitlement > 0 ? ((entitlement - remaining) / entitlement) * 100 : 0`, and returns a single tier `{name:"premium", utilization, resetsAt: usage.quota_reset_date}` with `queriedAt: Date.now()`.
`refetchInterval/InBackground/OnWindowFocus` gated on `autoQuery`, interval `300000`; `staleTime: 300000`; `retry: 1`. **No keep-last-good wrapper** — and the `queryFn` never returns `success:false` (it throws instead), so state 2 is only reachable via a synthesized/cached value.

---

# 3. `UsageScriptModal` — `/home/user/cc-switch/src/components/UsageScriptModal.tsx` (1621 L)

## 3.1 Template types — `/home/user/cc-switch/src/config/constants.ts`
```ts
export const PROVIDER_TYPES = { GITHUB_COPILOT: "github_copilot", CODEX_OAUTH: "codex_oauth" } as const;
export const TEMPLATE_TYPES = {
  CUSTOM: "custom", GENERAL: "general", NEW_API: "newapi",
  GITHUB_COPILOT: "github_copilot", TOKEN_PLAN: "token_plan",
  BALANCE: "balance", OFFICIAL_SUBSCRIPTION: "official_subscription",
} as const;
export type TemplateType = (typeof TEMPLATE_TYPES)[keyof typeof TEMPLATE_TYPES];
```
```ts
const NATIVE_USAGE_TEMPLATES = new Set([GITHUB_COPILOT, TOKEN_PLAN, BALANCE, OFFICIAL_SUBSCRIPTION]);  // :186
```
"Native" ⇒ no JS script, no code editor, no help block, Format button disabled, save-time script validation skipped.

`TEMPLATE_NAME_KEYS` (`:129-139`): `usageScript.templateCustom | templateGeneral | templateNewAPI | templateCopilot | templateTokenPlan | templateBalance | templateOfficialSubscription`.

## 3.2 `UsageScript` type — `/home/user/cc-switch/src/types.ts:55-87`
```ts
export interface UsageScript {
  enabled: boolean; language: "javascript"; code: string;
  timeout?: number;                // 秒，默认 10
  templateType?: TemplateType;
  apiKey?: string; baseUrl?: string;          // general / newapi
  accessToken?: string; userId?: string;      // newapi
  accessKeyId?: string; secretAccessKey?: string;   // 火山方舟，与推理 Key 分离
  teamOrganizationId?: string; teamProjectId?: string; // 智谱团队（请求头 bigmodel-organization / bigmodel-project）
  codingPlanProvider?: string;
  autoQueryInterval?: number;      // 分钟，0 = 禁用
  autoIntervalMinutes?: number;    // 别名字段
  request?: { url?: string; method?: string; headers?: Record<string,string>; body?: any };
}
const DEFAULT_USAGE_SCRIPT: UsageScript = { enabled:false, language:"javascript", code:"", timeout:10, autoQueryInterval:5 };
export function createUsageScript(overrides?: Partial<UsageScript>): UsageScript
```

## 3.3 Preset template bodies (`generatePresetTemplates(t)`, `:56-127`)
`CUSTOM` and `GENERAL` and `NEW_API` carry JS source; the four native types map to `""`.
- `GENERAL` hits `{{baseUrl}}/user/balance` with `Authorization: Bearer {{apiKey}}` and `User-Agent: cc-switch/1.0`, extractor returns `{isValid: response.is_active || true, remaining: response.balance, unit:"USD"}`.
- `NEW_API` hits `{{baseUrl}}/api/user/self` with `Authorization: Bearer {{accessToken}}` and **`New-Api-User: {{userId}}`**; extractor divides quota by **500000** to get USD and falls back to `{isValid:false, invalidMessage: response.message || t("usageScript.queryFailedMessage")}`.
Both interpolate i18n strings (`usageScript.defaultPlan`, `usageScript.queryFailedMessage`) **into the generated code**, so switching language regenerates the templates.

## 3.4 Detection

### `detectBalanceProvider(baseUrl)` (`:141-156`) — `BALANCE_PROVIDERS`
| id | label | pattern |
|---|---|---|
| `deepseek` | DeepSeek | `/api\.deepseek\.com/i` |
| `stepfun` | StepFun | `/api\.stepfun\.(ai\|com)/i` |
| `siliconflow` | SiliconFlow | `/api\.siliconflow\.(cn\|com)/i` |
| `openrouter` | OpenRouter | `/openrouter\.ai/i` |
| `novita` | Novita AI | `/api\.novita\.ai/i` |

### Coding-plan detection — `/home/user/cc-switch/src/config/codingPlanProviders.ts`
Mirrors the backend `services/coding_plan.rs::detect_provider`, which uses `url.contains(...)`; the frontend uses equivalent regexes. **First match wins** and order matters:
| id | label | pattern |
|---|---|---|
| `kimi` | Kimi For Coding | `/api\.kimi\.com\/coding/i` |
| `zhipu` | Zhipu GLM (智谱) | `/bigmodel\.cn\|api\.z\.ai/i` |
| `zhipu_team` | Zhipu GLM Team (智谱团队) | `/bigmodel\.cn/i` — **placeholder only** |
| `minimax` | MiniMax | `/api\.minimaxi?\.com\|api\.minimax\.io/i` |
| `zenmux` | ZenMux | `/zenmux\./i` |
| `volcengine` | 火山方舟 (Volcengine) | `/volces\.com\/api\/coding/i` |

**`zhipu_team` can never be auto-detected**: personal `zhipu` precedes it and matches the same host, so `detectCodingPlanProvider` always returns `zhipu` first. Team plan must be chosen manually, and is therefore never auto-injected by `injectCodingPlanUsageScript`. Deliberate — documented at `codingPlanProviders.ts:31-37`.

`injectCodingPlanUsageScript(appId, provider)`: **Claude only**, only when `meta.usage_script` is entirely absent (never overwrites), reads `settingsConfig.env.ANTHROPIC_BASE_URL`, and on a hit writes `createUsageScript({enabled:true, templateType:"token_plan", codingPlanProvider})` with empty `code`.

### `isOfficialSubscriptionProvider(provider, appId)` (`:158-184`)
`false` unless `appId ∈ {claude, codex, gemini}`. `true` if `provider.category === "official"`. Otherwise per app:
- **claude** — `env.ANTHROPIC_BASE_URL` missing or blank
- **codex** — no `experimental bearer token` in the TOML **and** `auth.OPENAI_API_KEY` missing/blank
- **gemini** — both `env.GEMINI_API_KEY` and `env.GOOGLE_GEMINI_BASE_URL` missing/blank

## 3.5 `getProviderCredentials()` (`:212-305`)
Per-app extraction, then **`trimTrailingSlash`** on the baseUrl (`replace(/\/+$/, "")`) so `{{baseUrl}}/path` never double-slashes — mirrors the backend `Provider::resolve_usage_credentials`.

| appId | apiKey | baseUrl |
|---|---|---|
| `claude`, `claude-desktop` | `env.ANTHROPIC_AUTH_TOKEN \|\| env.ANTHROPIC_API_KEY \|\| env.OPENROUTER_API_KEY \|\| env.GOOGLE_API_KEY` | `env.ANTHROPIC_BASE_URL` |
| `codex` | `auth.OPENAI_API_KEY` if non-blank, else `extractCodexExperimentalBearerToken(config)` | `extractCodexBaseUrl(config)` (TOML) |
| `gemini` | `env.GEMINI_API_KEY \|\| env.GOOGLE_API_KEY` | `env.GOOGLE_GEMINI_BASE_URL` |
| `grokbuild` | `parseGrokBuildConfig(config, provider.name).apiKey` | `.baseUrl` |
| `hermes` | `config.api_key` (flat snake_case) | `config.base_url` |
| `openclaw` | `config.apiKey` (flat camelCase) | `config.baseUrl` |
| `opencode` | `config.options.apiKey` | `config.options.baseURL` (**capital URL**) |
| else | `undefined` | `undefined` |
Whole body wrapped in try/catch → logs and returns `{undefined, undefined}`.

**`effectiveScriptCredentials`** (`:361-366`) mirrors the backend `resolve_script_credentials` precedence: explicit non-blank script value wins, else the provider credential. `baseUrl` is additionally trailing-slash-stripped.

## 3.6 Initial state

### `script` initializer (`:310-352`)
1. Saved script exists → `createUsageScript(saved)`; **if `isOfficialSubscription` and the saved `templateType !== official_subscription`, discard it entirely and return a fresh default**; if it's `token_plan` with no `codingPlanProvider`, backfill `detectCodingPlanProvider(baseUrl) || "kimi"`.
2. No saved script: coding-plan detected → `createUsageScript({codingPlanProvider: detected})`; balance provider detected → bare default; official-subscription → bare default; otherwise → `createUsageScript({code: PRESET_TEMPLATES.GENERAL})`.

### `selectedTemplate` initializer (`:406-451`), first match wins
1. `meta.providerType === "github_copilot"` → `GITHUB_COPILOT`
2. saved `templateType` (accepted only if `!isOfficialSubscription` **or** it already is `OFFICIAL_SUBSCRIPTION`)
3. `isOfficialSubscription` → `OFFICIAL_SUBSCRIPTION` (but the enable switch stays off)
4. legacy inference: `accessToken || userId` → `NEW_API`
5. legacy inference: `apiKey || baseUrl` → `GENERAL`
6. coding-plan URL → `TOKEN_PLAN`
7. balance URL → `BALANCE`
8. default `GENERAL`

## 3.7 Validation

| Fn | Rules |
|---|---|
| `validateTimeout(v)` (`:369-386`) | `NaN` or blank → **10**; non-integer → `toast.warning(usageScript.timeoutMustBeInteger)` but still accepted as `Math.floor`; `< 0` → `toast.error(usageScript.timeoutCannotBeNegative)` and return **10**; else `Math.floor(n)` |
| `validateAndClampInterval(v)` (`:389-411`) | `NaN`/blank → **0**; non-integer → `toast.warning(usageScript.intervalMustBeInteger)`; `< 0` → `toast.error(usageScript.intervalCannotBeNegative)` → **0**; else clamp to `[0, 1440]`; if clamping changed a positive value → `toast.info(t("usageScript.intervalAdjusted",{value: clamped}))` |

Both run **on blur only**. While typing, an empty field is stored as `"" as unknown as number` (`:1445`, `:1473`) — an intentional lie to the type system so the input can be cleared. A Rust port should model these as `Option<String>` edit buffers with a parse-on-blur step.

**`handleSave`** (`:465-495`): for non-native templates, `enabled && !code.trim()` → `toast.error(usageScript.scriptEmpty)`; `enabled && !code.includes("return")` → `toast.error(usageScript.mustHaveReturn, {duration: 5000})`. Then `onSave({...script, templateType: selectedTemplate})` and `onClose()`. Note the `return` check is a **naive substring test**, not a parse.

## 3.8 Script test flow — `handleTest` (`:497-...`)
Sets `testing = true`; `finally` clears it. Branches by `selectedTemplate`, each returning early:

| Template | Call | Success | Cache write | Failure |
|---|---|---|---|---|
| `OFFICIAL_SUBSCRIPTION` | dynamic `import("@/lib/api/subscription")` → `getQuota(appId)` | needs `success && tiers.length>0`; summary = `tiers.map(t => \`${t.name}: ${Math.round(t.utilization)}%\`).join(", ")` | `setQueryData(["subscription","quota",appId], quota)` | `toast.error(\`${testFailed}: ${quota.error \|\| t("endpointTest.noResult")}\`, {duration:5000})` |
| `BALANCE` | `getBalance(providerCredentials.baseUrl ?? "", apiKey ?? "")` | `success && data.length>0`; summary via `formatUsageDataSummary(d, {invalid, remaining, used})` | `setQueryData(["usage", provider.id, appId], result)` | same shape |
| `TOKEN_PLAN` | `getCodingPlanQuota(baseUrl, apiKey, accessKeyId?, secretAccessKey?, provider?, teamOrgId?, teamProjectId?)` | tier-percent summary | converts tiers to `{planName: tier.name, remaining: 100-u, total: 100, used: u, unit:"%"}` and writes `["usage", provider.id, appId]` | same shape |
| `GITHUB_COPILOT` | `copilotGetUsageForAccount(accountId)` or `copilotGetUsage()` | summary `` `[${plan}] ${remaining} ${rem}/${entitlement} (${resetDate}: …)` `` | writes one `UsageData` with `unit: t("usageScript.premiumRequests")` | (throws → outer catch) |
| custom/general/newapi | `usageApi.testScript(provider.id, appId, code, timeout, apiKey, baseUrl, accessToken, userId, selectedTemplate)` | `success && data.length>0` | `setQueryData(["usage", provider.id, appId], result)` | same shape |

**TOKEN_PLAN argument selection** (`:583-600`): `isZenMux` → baseUrl/apiKey come from the **script fields**; otherwise from the **provider config**. `accessKeyId`/`secretAccessKey` passed only when `isVolcengine`; `codingPlanProvider`/`teamOrganizationId`/`teamProjectId` passed only when `isZhipuTeam`.

Outer `catch` (`:687-696`): uses **`extractErrorMessage(error)`**, because a Tauri `Err(String)` rejects with a bare string and `.message` would be `undefined`.

Success toasts are `` `${t("usageScript.testSuccess")}${summary}` `` with `{duration: 3000, closeButton: true}`.

**Note the cache keys written here are the literal `["usage", provider.id, appId]` / `["subscription","quota",appId]`, matching `usageKeys.script()` and `subscriptionKeys.quota()` only by coincidence of shape — the port should use the key builders.**

## 3.9 `handleFormat` (`:698-718`)
`prettier.format(code, {parser:"babel", plugins:[parserBabel, pluginEstree], semi:true, singleQuote:false, tabWidth:2, printWidth:80})` then `.trim()`. Success `toast.success(usageScript.formatSuccess,{duration:1000, closeButton:true})`; failure `` `${t("usageScript.formatFailed")}: ${error?.message || t("jsonEditor.invalidJson")}` `` at 3000 ms.

## 3.10 `handleUsePreset(presetName)` (`:720-806`) — field clearing per template
| Preset | Fields set |
|---|---|
| `CUSTOM` | `code=preset`; clears apiKey, baseUrl, accessToken, userId (so both test and real query fall back to provider credentials, matching the "supported variables" display) |
| `GENERAL` | `code=preset`; clears accessToken, userId |
| `NEW_API` | `code=preset`; clears **apiKey** only |
| `GITHUB_COPILOT` | `code=""`; clears all four |
| `TOKEN_PLAN` | `code=""`; resolves `provider = script.codingPlanProvider \|\| autoDetected \|\| "kimi"`; **keeps** apiKey/baseUrl iff zenmux, accessKeyId/secretAccessKey iff volcengine, teamOrganizationId/teamProjectId iff zhipu_team; clears accessToken/userId always; sets `codingPlanProvider` |
| `BALANCE` | `code=""`; clears all four |
| `OFFICIAL_SUBSCRIPTION` | `code=""`; clears all four |
Then `setSelectedTemplate(presetName)`.

## 3.11 Enable toggle and consent
`handleEnableToggle(checked)` (`:455-461`): turning **on** while `!settingsData?.usageConfirmed` opens `ConfirmDialog` (`variant="info"`, `confirm.usage.title/.message/.confirm`); otherwise applies directly.
`handleUsageConfirm` (`:463-476`): closes the dialog, strips `webdavSync` from settings (`const {webdavSync: _, ...rest}`) before `settingsApi.save({...rest, usageConfirmed:true})`, invalidates `["settings"]`, then sets `enabled: true` **regardless of whether the save threw**.

## 3.12 Layout (FullScreenPanel)
Title `` `${t("usageScript.title")} - ${provider.name}` ``.
**Footer** (`:814-855`): left group — secondary `usageScript.testScript` (`Play`, label `usageScript.testing` while testing, `disabled={!script.enabled || testing}`), outline `usageScript.format` (`Wand2`, `disabled={!script.enabled || NATIVE_USAGE_TEMPLATES.has(selectedTemplate)}`). Right group — outline `common.cancel`, primary `usageScript.saveConfig` (`Save`).

**Body:**
1. Always-visible enable strip: `usageScript.enableUsageQuery` + `Switch`.
2. Everything below renders **only when `script.enabled`**.
3. **Template picker card** — label `usageScript.presetTemplate`, then buttons over `Object.keys(PRESET_TEMPLATES)` filtered by (`:876-898`): Copilot provider → only `GITHUB_COPILOT`; `isOfficialSubscription` → only `OFFICIAL_SUBSCRIPTION`; otherwise **exclude both** `GITHUB_COPILOT` and `OFFICIAL_SUBSCRIPTION`. Selected → `variant="default"`.
4. **Per-template info blocks:**
   - `CUSTOM` → `usageScript.supportedVariables` with two rows showing `{{baseUrl}}` and `{{apiKey}}` (green mono) `=` the effective value or an italic `common.notSet`. The apiKey value is masked `••••••••` behind an `Eye`/`EyeOff` toggle (`aria-label` = `apiKeyInput.show`/`.hide`).
   - `GITHUB_COPILOT` → `usageScript.copilotAutoAuth`
   - `BALANCE` → `usageScript.balanceHint` + one chip per `BALANCE_PROVIDERS` entry whose pattern matches the current baseUrl
   - `OFFICIAL_SUBSCRIPTION` → `usageScript.officialSubscriptionHint`
   - `TOKEN_PLAN` → `usageScript.tokenPlanHint` + a button per `CODING_PLAN_PROVIDERS` entry, selected one `variant="default"`; click sets `codingPlanProvider` **without clearing any credential fields**
5. **Credentials block** — shown when `shouldShowCredentialsConfig` = `GENERAL || NEW_API || (TOKEN_PLAN && codingPlanProvider === "zenmux")` (`:806-810`). Header `usageScript.credentialsConfig` + `usageScript.credentialsHint`, grid `md:grid-cols-2`:

| Template | Fields |
|---|---|
| `GENERAL` | `API Key` (+ `({usageScript.optional})`, password w/ eye toggle, placeholder `usageScript.apiKeyPlaceholder`), `usageScript.baseUrl` (+ optional, placeholder `usageScript.baseUrlPlaceholder`) |
| `NEW_API` | `usageScript.baseUrl` (placeholder `https://api.newapi.com`), `usageScript.accessToken` (password + eye, placeholder `usageScript.accessTokenPlaceholder`), `usageScript.userId` (placeholder `usageScript.userIdPlaceholder`) — **none marked optional** |
| `TOKEN_PLAN` + zenmux | `usageScript.baseUrl` (placeholder `https://api.zenmux.com/v1/...`), `API Key` (password + eye, placeholder `sk-...`) |

6. **Volcengine block** (`TOKEN_PLAN && codingPlanProvider === "volcengine"`, `:1284-1370`) — sits **outside** `shouldShowCredentialsConfig`. Header `usageScript.credentialsConfig`, hint `usageScript.volcengineAkSkHint`, then `usageScript.volcengineKeyConsoleLink` followed by a button opening `https://console.volcengine.com/iam/keymanage` via `settingsApi.openExternal` (with an `ExternalLink` icon). Fields: `usageScript.accessKeyId` (text, placeholder `AKLT...`) and `usageScript.secretAccessKey` (password + eye, placeholder `••••••••`). Rationale comment: the control-plane usage API needs account AK/SK signing, a different credential pair from the inference API key.
7. **Zhipu-team block** (`codingPlanProvider === "zhipu_team"`, `:1372-1443`) — hint `usageScript.zhipuTeamHint`, link `usageScript.zhipuTeamConsoleLink` → `https://bigmodel.cn/coding-plan/team/usage-stats`. Fields `usageScript.organizationId` and `usageScript.projectId` with their `*Placeholder` keys. `api_key` reuses the provider's inference credential.
8. **Always-visible common config** (`:1446-1505`): `usageScript.timeoutSeconds` (`number min=0`, value `script.timeout ?? 10`) and `usageScript.autoIntervalMinutes` (`number min=0 max=1440`, value **`script.autoQueryInterval ?? script.autoIntervalMinutes ?? 5`** — the alias fallback). Both write on change, validate on blur.
9. **Extractor editor** (non-native only): label `usageScript.extractorCode`, hint `usageScript.extractorHint`, `<JsonEditor id="usage-code" height={480} language="javascript" showMinimap={false} darkMode={isDarkMode}/>`.
10. **Help block** (non-native only): `usageScript.scriptHelp`; `usageScript.configFormat` + a literal `<pre>` example; `usageScript.extractorFormat` + list of `usageScript.field{IsValid,InvalidMessage,Remaining,Unit,PlanName,Total,Used,Extra}`; `usageScript.tips` + `tip1` (interpolating `{apiKey}`→`"{{apiKey}}"` and `{baseUrl}`→`"{{baseUrl}}"`), `tip2`, `tip3`.
11. `ConfirmDialog` for the usage consent.

All `usageScript.*` and `confirm.usage.*` keys verified present in `en.json` (111/111).

---

# 4. Proxy area

## 4.1 `src/types/proxy.ts` — quoted interfaces (141 L)

**Note the casing split: transport/status types are `snake_case` (raw serde passthrough); config and queue types are `camelCase`.** The port must not unify them.

```ts
export interface ProxyConfig {
  listen_address: string; listen_port: number;
  max_retries: number; request_timeout: number; enable_logging: boolean;
  live_takeover_active?: boolean;
  streaming_first_byte_timeout: number; streaming_idle_timeout: number; non_streaming_timeout: number;
}

export interface ProxyStatus {
  running: boolean; address: string; port: number;
  active_connections: number; total_requests: number;
  success_requests: number; failed_requests: number; success_rate: number;
  uptime_seconds: number;
  current_provider: string | null; current_provider_id: string | null;
  last_request_at: string | null; last_error: string | null;
  failover_count: number; active_targets?: ActiveTarget[];
}

export interface ActiveTarget { app_type: string; provider_name: string; provider_id: string }
export interface ProxyServerInfo { address: string; port: number; started_at: string }

export interface ProxyTakeoverStatus {
  claude: boolean; "claude-desktop"?: boolean; codex: boolean; gemini: boolean;
  grokbuild: boolean; opencode: boolean; openclaw: boolean; hermes: boolean;
}

export interface ProviderHealth {
  provider_id: string; app_type: string; is_healthy: boolean;
  consecutive_failures: number;
  last_success_at: string | null; last_failure_at: string | null;
  last_error: string | null; updated_at: string;
}

export interface CircuitBreakerConfig {
  failureThreshold: number; successThreshold: number;
  timeoutSeconds: number; errorRateThreshold: number; minRequests: number;
}
export type CircuitState = "closed" | "open" | "half_open";
export interface CircuitBreakerStats {
  state: CircuitState; consecutiveFailures: number; consecutiveSuccesses: number;
  totalRequests: number; failedRequests: number;
}
export enum ProviderHealthStatus { Healthy="healthy", Degraded="degraded", Failed="failed", Unknown="unknown" }
export interface ProviderHealthWithStatus extends ProviderHealth { status: ProviderHealthStatus; circuitState?: CircuitState }

export interface ProxyUsageRecord {
  provider_id: string; app_type: string; endpoint: string;
  request_tokens: number | null; response_tokens: number | null;
  status_code: number; latency_ms: number; error: string | null; timestamp: string;
}

export interface FailoverQueueItem {
  providerId: string; providerName: string; providerNotes?: string; sortIndex?: number;
}

export interface GlobalProxyConfig {
  proxyEnabled: boolean; listenAddress: string; listenPort: number; enableLogging: boolean;
}

export interface AppProxyConfig {
  appType: string; enabled: boolean; autoFailoverEnabled: boolean; maxRetries: number;
  streamingFirstByteTimeout: number; streamingIdleTimeout: number; nonStreamingTimeout: number;
  circuitFailureThreshold: number; circuitSuccessThreshold: number; circuitTimeoutSeconds: number;
  circuitErrorRateThreshold: number; circuitMinRequests: number;
}
```
`errorRateThreshold` / `circuitErrorRateThreshold` are stored as a **0–1 fraction** on the wire and edited as a **0–100 integer** in both config panels.

## 4.2 `src/lib/api/proxy.ts` — commands (120 L)
| Method | Command | Args | Returns |
|---|---|---|---|
| `startProxyServer` | `start_proxy_server` | — | `ProxyServerInfo` |
| `stopProxyWithRestore` | `stop_proxy_with_restore` | — | `void` |
| `getProxyStatus` | `get_proxy_status` | — | `ProxyStatus` |
| `isProxyRunning` | `is_proxy_running` | — | `boolean` |
| `isLiveTakeoverActive` | `is_live_takeover_active` | — | `boolean` |
| `switchProxyProvider` | `switch_proxy_provider` | `{appType, providerId}` | `void` |
| `getProxyTakeoverStatus` | `get_proxy_takeover_status` | — | `ProxyTakeoverStatus` |
| `setProxyTakeoverForApp` | `set_proxy_takeover_for_app` | `{appType, enabled}` | `void` |
| `getProxyConfig` | `get_proxy_config` | — | `ProxyConfig` *(legacy v2)* |
| `updateProxyConfig` | `update_proxy_config` | `{config}` | `void` *(legacy v2)* |
| `getGlobalProxyConfig` | `get_global_proxy_config` | — | `GlobalProxyConfig` |
| `updateGlobalProxyConfig` | `update_global_proxy_config` | `{config}` | `void` |
| `getProxyConfigForApp` | `get_proxy_config_for_app` | `{appType}` | `AppProxyConfig` |
| `updateProxyConfigForApp` | `update_proxy_config_for_app` | `{config}` | `void` |
| `getDefaultCostMultiplier` | `get_default_cost_multiplier` | `{appType}` | `string` |
| `setDefaultCostMultiplier` | `set_default_cost_multiplier` | `{appType, value}` | `void` |
| `getPricingModelSource` | `get_pricing_model_source` | `{appType}` | `string` |
| `setPricingModelSource` | `set_pricing_model_source` | `{appType, value}` | `void` |

**`stop_proxy_server` is invoked directly in `useProxyStatus.ts:69` but is absent from `proxyApi`.**

## 4.3 `src/lib/api/failover.ts` — commands (99 L)
Also declares a local `Provider` shape (`:9-21`): `{id, name, settingsConfig: unknown, websiteUrl?, category?, createdAt?, sortIndex?, notes?, meta?, icon?, iconColor?}`.

| Method | Command | Args | Returns |
|---|---|---|---|
| `getProviderHealth` | `get_provider_health` | `{providerId, appType}` | `ProviderHealth` |
| `resetCircuitBreaker` | `reset_circuit_breaker` | `{providerId, appType}` | `void` |
| `getCircuitBreakerConfig` | `get_circuit_breaker_config` | — | `CircuitBreakerConfig` |
| `updateCircuitBreakerConfig` | `update_circuit_breaker_config` | `{config}` | `void` |
| `getCircuitBreakerStats` | `get_circuit_breaker_stats` | `{providerId, appType}` | `CircuitBreakerStats \| null` |
| `getFailoverQueue` | `get_failover_queue` | `{appType}` | `FailoverQueueItem[]` |
| `getAvailableProvidersForFailover` | `get_available_providers_for_failover` | `{appType}` | `Provider[]` |
| `addToFailoverQueue` | `add_to_failover_queue` | `{appType, providerId}` | `void` |
| `removeFromFailoverQueue` | `remove_from_failover_queue` | `{appType, providerId}` | `void` |
| `getAutoFailoverEnabled` | `get_auto_failover_enabled` | `{appType}` | `boolean` |
| `setAutoFailoverEnabled` | `set_auto_failover_enabled` | `{appType, enabled}` | `void` |

## 4.4 `src/lib/query/proxy.ts` — hooks (243 L)

### Queries
| Hook | Key | Interval |
|---|---|---|
| `useProxyStatus` (**unused**) | `["proxyStatus"]` | 5000 |
| `useIsProxyRunning` | `["proxyRunning"]` | 2000 |
| `useIsLiveTakeoverActive` | `["liveTakeoverActive"]` | 2000 |
| `useProxyTakeoverStatus` | `["proxyTakeoverStatus"]` | 2000 |
| `useProxyConfig` (legacy) | `["proxyConfig"]` | — |
| `useGlobalProxyConfig` | `["globalProxyConfig"]` | — |
| `useAppProxyConfig(appType)` | `["appProxyConfig", appType]` | — (`enabled: !!appType`) |

### Mutations and their invalidations
| Hook | Fn | Invalidates | Toasts |
|---|---|---|---|
| `useStartProxyServer` | `start_proxy_server` | `proxyStatus`, `proxyRunning`, `liveTakeoverActive`, `proxyTakeoverStatus` | none |
| `useStopProxyServer` | `stop_proxy_with_restore` | same four | none |
| `useSetProxyTakeoverForApp` | `set_proxy_takeover_for_app` | `proxyTakeoverStatus`, `liveTakeoverActive` | none |
| `useSwitchProxyProvider` | `switch_proxy_provider` | `proxyStatus`, `["providers", appType]` | error → `proxy.switchFailed {error}` |
| `useProxyConfig().updateConfig` | `update_proxy_config` | `proxyConfig`, `proxyStatus` | `proxy.settings.toast.saved` / `…saveFailed {error}` |
| `useUpdateGlobalProxyConfig` | `update_global_proxy_config` | `globalProxyConfig`, `proxyConfig`, `proxyStatus` | same pair |
| `useUpdateAppProxyConfig` | `update_proxy_config_for_app` | `["appProxyConfig",appType]`, `["autoFailoverEnabled",appType]`, `proxyConfig`, `circuitBreakerConfig`, `proxyStatus` | same pair |

Success toasts carry `{closeButton: true}`.

## 4.5 `src/lib/query/failover.ts` — hooks (290 L)

| Hook | Key | Notes |
|---|---|---|
| `useProviderHealth(pid, app)` | `["providerHealth", pid, app]` | `refetchInterval: 5000`, **`retry: false`**, `enabled: !!pid && !!app` |
| `useCircuitBreakerConfig()` | `["circuitBreakerConfig"]` | — |
| `useCircuitBreakerStats(pid, app)` | `["circuitBreakerStats", pid, app]` | `refetchInterval: 5000` |
| `useFailoverQueue(app)` | `["failoverQueue", app]` | `enabled: !!appType` |
| `useAvailableProvidersForFailover(app)` | `["availableProvidersForFailover", app]` | `enabled: !!appType` |
| `useAutoFailoverEnabled(app)` | `["autoFailoverEnabled", app]` | **`placeholderData: false`** (matches backend default) |

### Mutations
- **`useResetCircuitBreaker`** — invalidates `["providerHealth",pid,app]`, `["providers",app]`, `["proxyStatus"]`. The provider list is refreshed because a reset can trigger an automatic switch back to a higher-priority provider; `proxyStatus` for `active_targets`.
- **`useUpdateCircuitBreakerConfig`** — invalidates `["circuitBreakerConfig"]`. No toast (caller does it).
- **`useAddToFailoverQueue`** — invalidates `failoverQueue`, `availableProvidersForFailover`, `["providers",app]`.
- **`useRemoveFromFailoverQueue`** — the same three **plus** `["providerHealth",pid,app]` and `["circuitBreakerStats",pid,app]` (a provider outside the queue no longer needs health monitoring).
- **`useSetAutoFailoverEnabled`** — the only **optimistic** mutation in the area:
  - `onMutate`: `cancelQueries(["autoFailoverEnabled",app])`, snapshot `previousValue`, `setQueryData(key, enabled)`, return `{previousValue, appType}`
  - `onSuccess`: app label map `claude→"Claude"`, `codex→"Codex"`, `grokbuild→"Grok Build"`, **everything else → `"Gemini"`**; toast `failover.enabled` / `failover.disabled` with `{app}` and `{closeButton:true}`
  - `onError`: roll back `previousValue` if defined; `detail = extractErrorMessage(error) || t("common.unknown")`; toast `failover.toggleFailed {detail}`
  - `onSettled`: invalidate `autoFailoverEnabled`, `failoverQueue`, `availableProvidersForFailover`, `["providers",app]`, `["proxyStatus"]` — because enabling can immediately switch to queue P1 **and**, when the queue is empty, auto-add the current provider to it.

## 4.6 `src/hooks/useProxyStatus.ts` (248 L) — the rich hook actually used by the UI

**Queries**
- `["proxyStatus"]` → `get_proxy_status`; **`refetchInterval: (query) => query.state.data?.running ? 2000 : false`** — polls only while running; `placeholderData: (prev) => prev` to avoid flicker.
- `["proxyTakeoverStatus"]` → `get_proxy_takeover_status`; **no polling**, `placeholderData: prev`.

**Mutations**
| Name | Command | Success | Error |
|---|---|---|---|
| `startProxyServer` | `start_proxy_server` | toast `proxy.server.started {address,port}`; invalidate `proxyStatus` | `proxy.server.startFailed {detail}` |
| `stopProxyServer` | **`stop_proxy_server`** (stop only — does not rewrite/restore other apps' takeover) | `proxy.server.stopped`; invalidate `proxyStatus` | `proxy.server.stopFailed {detail}` |
| `stopWithRestore` | `stop_proxy_with_restore` | `proxy.stoppedWithRestore`; invalidate `proxyStatus` + `proxyTakeoverStatus`; **`removeQueries(["providerHealth"])` and `removeQueries(["circuitBreakerStats"])`** — hard-delete, because the backend cleared the DB rows and reset the breakers. Failover queue and toggle state intentionally survive. | `proxy.stopWithRestoreFailed {detail}` |
| `setTakeoverForApp` | `set_proxy_takeover_for_app` | label map `claude/codex/gemini/grokbuild` else **`"OpenCode"`**; toast `proxy.takeover.enabled` / `.disabled` `{app}`; invalidate `proxyStatus` + `proxyTakeoverStatus` | `proxy.takeover.failed {detail}` |
| `switchProxyProvider` | `switch_proxy_provider` | invalidate `proxyStatus`, no toast | `proxy.switchFailed {error: detail}` |

All errors go through `extractErrorMessage(error) || t("common.unknown")`.

**Returned surface:** `{status, isLoading, isRunning: status?.running || false, takeoverStatus, isTakeoverActive, startProxyServer, stopProxyServer, stopWithRestore, setTakeoverForApp, switchProxyProvider, checkRunning, checkTakeoverActive, isStarting, isStoppingServer, isStopping, isPending}`.
- **`isTakeoverActive` ORs only `claude || codex || gemini || grokbuild`** — `opencode`/`openclaw`/`hermes`/`claude-desktop` are excluded.
- `checkRunning` / `checkTakeoverActive` are imperative one-shots that swallow errors and return `false`.
- `isPending` = start || stopServer || stopWithRestore || setTakeover (**excludes `switchProxyProvider`**).

## 4.7 `src/hooks/useProxyConfig.ts` (48 L)
`["proxyConfig"]` → `get_proxy_config`; update → `update_proxy_config {config}`; success toast `proxy.settings.toast.saved` + invalidate `proxyConfig`, `proxyStatus`; error `proxy.settings.toast.saveFailed {error: error.message}`. Returns `{config, isLoading, updateConfig, isUpdating}`. **Byte-for-byte equivalent to `lib/query/proxy.ts:137` — see B8.**

## 4.8 `ProxyTabContent` — `/home/user/cc-switch/src/components/settings/ProxyTabContent.tsx` (285 L)
Props `{settings: SettingsFormState, onAutoSave}`. Uses the **hooks** `useProxyStatus`. Local: `showProxyConfirm`, `showFailoverConfirm`.

`handleToggleProxy(checked)`: off → `stopWithRestore()`; on + `!settings.proxyConfirmed` → open confirm; on + confirmed → `startProxyServer()`. Errors only `console.error`.
`handleProxyConfirm`: close, `await onAutoSave({proxyConfirmed:true})`, then `startProxyServer()`.
`handleFailoverToggleChange`: on + `!failoverConfirmed` → confirm dialog; else `onAutoSave({enableFailoverToggle: checked})`.
`handleFailoverConfirm`: `onAutoSave({failoverConfirmed:true, enableFailoverToggle:true})`.

**Accordion `type="multiple"`, `defaultValue={[]}`, four items:**
1. `proxy` — `Server` green, `settings.advanced.proxy.title/.description`, plus a right-aligned `Badge` (`variant = isRunning ? "default" : "secondary"`) with a pulsing `Activity` and `settings.advanced.proxy.running/.stopped`. Body `<ProxyPanel enableLocalProxy onEnableLocalProxyChange onToggleProxy isProxyPending/>`.
2. `failover` — `Activity` orange, `settings.advanced.failover.title/.description`. Body: a `ToggleRow` (`ShieldAlert` orange, `settings.advanced.proxy.enableFailoverToggle` + `…Description`); a yellow warning `proxy.failover.proxyRequired` when `!isRunning`; then **`Tabs defaultValue="claude"` with exactly three tabs — Claude / Codex / Gemini (no Grok Build)**. Per tab: `failoverDisabled = !isRunning || !(takeoverStatus?.[appType] ?? false)`, an `h4` `proxy.failoverQueue.title` + `.description`, `<FailoverQueueManager appType disabled/>`, a divider, and `<AutoFailoverConfigPanel appType disabled/>`.
3. `rectifier` — `Zap` purple, `<RectifierConfigPanel/>`.
4. `globalProxy` — `Globe` cyan, `<GlobalProxySettings/>` (outbound proxy; out of scope here).

Two `ConfirmDialog`s at the end: `confirm.proxy.*` and `confirm.failover.*`, both `variant="info"`.

## 4.9 `ProxyPanel` — `/home/user/cc-switch/src/components/proxy/ProxyPanel.tsx` (755 L)

Data: `useProxyStatus()` (hooks version) → `{status, isRunning}`; `useProxyTakeoverStatus()`; `useGlobalProxyConfig()` + `useUpdateGlobalProxyConfig()`; four `useFailoverQueue("claude"|"codex"|"gemini"|"grokbuild")`.
Local: `listenAddress` (default `"127.0.0.1"`), `listenPort` (**string**, default `"15721"`), synced from `globalConfig` via effect (`:63-68`).

### `handleSaveBasicConfig` (`:125-199`) — validation
**Address** accepted if: `"localhost"` (normalized to `127.0.0.1`), `"0.0.0.0"`, a valid IPv4 (regex `^(\d{1,3}\.){3}\d{1,3}$` **and** every octet `0..255`), or a valid IPv6 literal — tested by `new URL(\`http://[${addr}]/\`)` not throwing, requiring a `:`. `"::"` is accepted because the backend (`services/proxy.rs`) rewrites it to `::1`. Failure → `toast.error(proxy.settings.invalidAddress)`.
**Port**: must match `/^\d+$/` on the trimmed string **and** be within `1024..65535`. Either failure → `toast.error(proxy.settings.invalidPort)`.
Then `updateGlobalConfig.mutateAsync({...globalConfig, listenAddress: normalizedAddress, listenPort: port})`; toast `proxy.settings.configSaved` / `proxy.settings.configSaveFailed`.
*(Note: the hook itself also fires `proxy.settings.toast.saved`, so a successful save shows **two** toasts.)*

### Helpers
`formatUptime(s)` (`:201-213`): `` `${h}h ${m}m ${sec}s` `` / `` `${m}m ${sec}s` `` / `` `${sec}s` ``.
`formatAddressForUrl(addr, port)` (`:216-220`): wraps in `[...]` when the address contains `:`, prefixes `http://`.

### Layout (numbered as in the source comments)
1. `ToggleRow` — `Zap` green, `settings.advanced.proxy.enableFeature` + `…Description`, bound to `enableLocalProxy`. **Always visible.**
2. Proxy-service row — `Power` green, title `proxyConfig.proxyEnabled`, subtitle `settings.advanced.proxy.running/.stopped`, `Switch checked={isRunning} onCheckedChange={onToggleProxy} disabled={isProxyPending}`. **Always visible.**
3. App-takeover card — inside `AnimatePresence`, rendered only when `isRunning`; `opacity/height 0↔auto`, `0.25s easeInOut`. Header `proxyConfig.appTakeover`, a `sm:grid-cols-2 lg:grid-cols-4` of four switches over `["claude","codex","gemini","grokbuild"]` (label capitalized, `grokbuild → "Grok Build"`), footer hint `proxy.takeover.hint`. `handleTakeoverChange` toasts `proxy.takeover.enabled/.disabled {app: appType}` (**raw app id, not a display label**) or `proxy.takeover.failed {detail}`.

**When `isRunning && status`:**
4. Service info — `proxy.panel.serviceAddress`, a `<code>` with `formatAddressForUrl`, a `common.copy` button (`navigator.clipboard.writeText` → toast `proxy.panel.addressCopied`), and the note `proxy.settings.restartRequired`. Then a `provider.inUse` section: if `active_targets` is non-empty, a 2-col grid of `{app_type} → {provider_name}` (truncated, with `title`); else if `current_provider`, the line `proxy.panel.currentProvider` + name; else a yellow `proxy.panel.waitingFirstRequest`.
5. Logging toggle — `proxy.settings.fields.enableLogging.label/.description`, `checked={globalConfig?.enableLogging ?? true}`, disabled while the mutation is pending. `handleLoggingChange` sends the **whole** `globalConfig` with `enableLogging` replaced; toasts `proxy.logging.enabled/.disabled/.failed`.
6. Queue groups — rendered only if **any** of the four queues is non-empty; header `ListOrdered` + `proxy.failoverQueue.title`; one `ProviderQueueGroup` per non-empty queue (labels `Claude`, `Codex`, `Gemini`, `Grok Build`).
7. Stats grid `md:grid-cols-4` of `StatCard`s: `active_connections` (`Activity`), `total_requests` (`TrendingUp`), `` `${success_rate.toFixed(1)}%` `` (`Clock`, **`variant = success_rate > 90 ? "success" : "warning"`**), `formatUptime(uptime_seconds)` (`Clock`).

**When stopped:**
8. Basic settings card — `proxy.settings.basic.title/.description`, a 2-col grid with `listenAddress` (text) and `listenPort` (`type="number"`), each with its `.label` / `.placeholder` / `.description` keys, then a right-aligned Save button (`Loader2` + `common.saving` while pending, else `Save` + `common.save`). **Address/port are editable only while stopped.**
9. Stopped hint — a circular `Server` glyph, `proxy.panel.stoppedTitle`, `proxy.panel.stoppedDescription`.

### Sub-components
`StatCard` (`:635-652`): variants `default` (no extra class), `success` (`border-green-500/40 bg-green-500/5`), `warning` (`border-yellow-500/40 bg-yellow-500/5`).
`ProviderQueueGroup` (`:665-700`): finds `activeTarget = status.active_targets?.find(t => t.app_type === appType)`; renders a label + hairline, then one `ProviderQueueItem` per target with `priority = index + 1` and `isCurrent = activeTarget?.provider_id === target.id`.
`ProviderQueueItem` (`:712-755`): **calls `useProviderHealth(provider.id, appType)` — one 5 s poll per row (B10)**. Current row gets `border-primary/40 bg-primary/10 text-primary font-medium`, a filled priority circle and a `provider.inUse` chip. Trailing `<ProviderHealthBadge consecutiveFailures={health?.consecutive_failures ?? 0} isHealthy={health?.is_healthy}/>`.

## 4.10 `AutoFailoverConfigPanel` — `.../AutoFailoverConfigPanel.tsx` (519 L)

Props `{appType: string, disabled = false}`. Data `useAppProxyConfig(appType)` + `useUpdateAppProxyConfig()`.
All nine numeric fields held as **strings** (so they can be emptied); `autoFailoverEnabled` as a boolean. Seeded from `config` via effect, with **`circuitErrorRateThreshold: String(Math.round(config.circuitErrorRateThreshold * 100))`**.

### Validation (`handleSave`, `:57-185`)
`parseNum(v)`: `/^-?\d+$/` on the trimmed string, else `NaN` (so `"3.5"` and `"3px"` are rejected outright).
Ranges — **inclusive**, `NaN` also fails:
| Field | min | max | i18n label used in the error |
|---|---|---|---|
| `maxRetries` | 0 | 10 | `proxy.autoFailover.maxRetries` |
| `streamingFirstByteTimeout` | 1 | 120 | `…streamingFirstByte` |
| `streamingIdleTimeout` | 0 | 600 | `…streamingIdle` |
| `nonStreamingTimeout` | 60 | 1200 | `…nonStreaming` |
| `circuitFailureThreshold` | 1 | 20 | `…failureThreshold` |
| `circuitSuccessThreshold` | 1 | 10 | `…successThreshold` |
| `circuitTimeoutSeconds` | 0 | 300 | `…timeout` |
| `circuitErrorRateThreshold` | 0 | 100 | `…errorRate` |
| `circuitMinRequests` | 5 | 100 | `…minRequests` |

Collects **all** violations into `errors: string[]` as `` `${label}: ${min}-${max}` ``; any → `toast.error(t("proxy.autoFailover.validationFailed",{fields: errors.join("; ")}))` and abort.
On success sends the full `AppProxyConfig` including **`enabled: config.enabled`** (preserved untouched) and **`circuitErrorRateThreshold: raw.circuitErrorRateThreshold / 100`**. Toast `proxy.autoFailover.configSaved` (`closeButton`) / `` `${configSaveFailed}: ${String(e)}` ``.
*(The mutation also toasts `proxy.settings.toast.saved` — double toast again.)*
`handleReset` re-seeds the form from `config` (including the ×100 conversion).

### Layout
Loading → centered `Loader2 h-6 w-6`. `isDisabled = disabled || updateConfig.isPending` applies to every input.
1. Conditional destructive `Alert` with `String(error)`.
2. Blue info `Alert` (`border-blue-500/40 bg-blue-500/10`, `Info` icon) with `proxy.autoFailover.info`.
3. **Retry & timeout card** — `proxy.autoFailover.retrySettings`; `md:grid-cols-2`: `maxRetries` (`number min=0 max=10`, hint `…maxRetriesHint`) and `circuitFailureThreshold` (`min=1 max=20`, hint `…failureThresholdHint`).
4. **Timeout card** — `…timeoutSettings`; `md:grid-cols-3`: `streamingFirstByteTimeout` (`1..120`), `streamingIdleTimeout` (`0..600`, hint says *"范围 60-600 秒，填 0 禁用"* — **the hint's stated minimum disagrees with the enforced `min=0`**), `nonStreamingTimeout` (`60..1200`).
5. **Circuit-breaker card** — `…circuitBreakerSettings`; `grid-cols-2 md:grid-cols-4`: `circuitSuccessThreshold` (`1..10`), `circuitTimeoutSeconds` (`0..300`), `circuitErrorRateThreshold` (`0..100 step=5`), `circuitMinRequests` (`5..100`), each with its `*Hint`.
6. Footer — outline `common.reset`, primary Save (`Loader2`+`common.saving` / `Save`+`common.save`).

All input `id`s are suffixed `-${appType}` so the three tab instances don't collide.
**`autoFailoverEnabled` is in `formData` and is submitted, but no control in this panel edits it** — the switch lives in `FailoverQueueManager`. It is therefore whatever the last fetched config said.

## 4.11 `CircuitBreakerConfigPanel` — `.../CircuitBreakerConfigPanel.tsx` (356 L)

**Currently unmounted** — nothing imports it (`ProxyTabContent` uses the per-app panel instead). It is the *global* breaker config; the per-app one supersedes it.

`useCircuitBreakerConfig()` + `useUpdateCircuitBreakerConfig()`. Same string-state + `parseNum` + inclusive-range pattern:
| Field | min | max |
|---|---|---|
| `failureThreshold` | 1 | 20 |
| `successThreshold` | 1 | 10 |
| `timeoutSeconds` | 0 | 300 |
| `errorRateThreshold` | 0 | 100 |
| `minRequests` | 5 | 100 |
Errors aggregate into `circuitBreaker.validationFailed {fields}`. Save sends `errorRateThreshold / 100`; toasts `circuitBreaker.configSaved` (`closeButton`) / `` `${circuitBreaker.saveFailed}: ${String(error)}` ``.
Loading renders a plain text `circuitBreaker.loading` (not a spinner).
Layout: `h3` `circuitBreaker.title` + `circuitBreaker.description`, a hairline, a `md:grid-cols-2` in the order **failureThreshold, timeoutSeconds, successThreshold, errorRateThreshold, minRequests** (each with `*Hint`), then `circuitBreaker.saveConfig` + `common.reset`, then an instructions block `circuitBreaker.instructionsTitle` with five bullets `circuitBreaker.instructions.{failureThreshold,timeout,successThreshold,errorRate,minRequests}` each prefixed by the bold field label.

## 4.12 `FailoverQueueManager` — `.../FailoverQueueManager.tsx` (311 L)

Props `{appType: AppId, disabled = false}`. Local `selectedProviderId: string` (`""`).
Hooks: `useAutoFailoverEnabled(appType)` (default `false`), `useSetAutoFailoverEnabled()`, `useFailoverQueue(appType)`, `useAvailableProvidersForFailover(appType)`, `useAddToFailoverQueue()`, `useRemoveFromFailoverQueue()`.

Early returns: queue loading → centered `Loader2`; `queueError` → destructive `Alert` with `AlertTriangle` and `String(queueError)`.

### Layout
1. **Auto-failover switch card** — `proxy.failover.autoSwitch`, plus a green `common.enabled` pill when on; description `proxy.failover.autoSwitchDescription`; `Switch disabled={disabled || setFailoverEnabled.isPending}`. Handler is `mutate` (not `mutateAsync`) — toasts come from the mutation's own `onSuccess`/`onError` (§4.5), and the optimistic update makes the switch respond instantly.
2. Blue info `Alert` — `proxy.failoverQueue.info` (queue order mirrors the home-page provider order).
3. **Add row** — a `Select` (flex-1, placeholder `proxy.failoverQueue.selectProvider`, `disabled={disabled || isProvidersLoading}`) listing `availableProviders` with `{name}` plus a small `({notes})` when present; when the list is empty it renders a non-item div `proxy.failoverQueue.noAvailableProviders`. Beside it an icon Button (`Plus`, or `Loader2` while pending) `disabled={disabled || !selectedProviderId || addToQueue.isPending}`.
   `handleAddProvider`: `mutateAsync`, clear the selection, toast `proxy.failoverQueue.addSuccess` (`closeButton`) / `` `${addFailed}: ${String(error)}` ``.
4. **Queue list** — empty → a dashed box with `proxy.failoverQueue.empty`; else one `QueueItem` per entry keyed by `providerId`.
   `QueueItem`: a circled `index + 1`, `providerName` (+ `({providerNotes})`) truncated, and a ghost `Trash2` (or spinner) with `aria-label = t("common.delete")`, `disabled={disabled || isRemoving}`. **`isRemoving` is the shared `removeFromQueue.isPending`, so removing one row disables every row's button.**
   `handleRemoveProvider`: toast `proxy.failoverQueue.removeSuccess` / `` `${removeFailed}: ${String(error)}` ``.
5. Footer hint `proxy.failoverQueue.orderHint` — shown only when the queue is non-empty; says order is edited by dragging on the home page, not here.

**Ordering is read-only in this component**: `sortIndex` comes from the provider list; there is no drag handle.

## 4.13 Header toggles

### `ProxyToggle` — `.../ProxyToggle.tsx` (90 L)
Props `{className?, activeApp: AppId}`. From `useProxyStatus()`: `isRunning, takeoverStatus, setTakeoverForApp, isPending, status`.
`handleToggle(checked)` → `setTakeoverForApp({appType: activeApp, enabled: checked})`, errors only `console.error` (the hook toasts).
`takeoverEnabled = takeoverStatus?.[activeApp] || false`. Label map: claude/codex/gemini/grokbuild else **`"OpenCode"`**.
Tooltip (`title`): takeover on + running → `proxy.takeover.tooltip.active {appLabel, address, port}`; takeover on + **not** running → `proxy.takeover.tooltip.broken {appLabel}`; off → `proxy.takeover.tooltip.inactive {appLabel}`.
Icon: `Loader2` spinning while `isPending`, else `Radio` — `text-emerald-500 animate-pulse` when taken over, muted otherwise. Switch `disabled={isPending}`.

### `FailoverToggle` — `.../FailoverToggle.tsx` (87 L)
Props `{className?, activeApp: AppId}`. `useAutoFailoverEnabled(activeApp)` + `useSetAutoFailoverEnabled()` + `useProxyStatus().takeoverStatus`.
`takeoverEnabled = takeoverStatus?.[activeApp] ?? false`.
**`handleToggle`: `if (checked && !takeoverEnabled) return;`** — silently refuses to enable failover for an app that isn't taken over (the switch is also `disabled` in that state, so this is belt-and-braces).
Label map: claude/codex/grokbuild else **`"Gemini"`** (differs from ProxyToggle's fallback).
Tooltip: `!takeoverEnabled` → `failover.tooltip.takeoverRequired {app}` (**B4**); enabled → `failover.tooltip.enabled {app}`; else `failover.tooltip.disabled {app}`.
Icon: `Loader2` while `isPending || isLoading`, else `Shuffle` (emerald + pulse when on). Switch `disabled={isPending || isLoading || !takeoverEnabled}`.

### `ClaudeDesktopRouteToggle` — `.../ClaudeDesktopRouteToggle.tsx` (99 L)
From `useProxyStatus()`: `isRunning, status, takeoverStatus, startProxyServer, stopProxyServer, isStarting, isStoppingServer`.
`isBusy = isStarting || isStoppingServer`. `otherTakeoverActive = claude || codex || gemini || grokbuild`. `routeAddress = status?.address ?? "127.0.0.1"`, `routePort = status?.port ?? 15721`.
`handleToggle(checked)`: on → `startProxyServer()`. Off → **if `otherTakeoverActive`, `toast.warning(claudeDesktop.route.stopBlockedByTakeover, {duration:5000})` and return without stopping** (B5); else `stopProxyServer()` (the plain stop, *not* stop-with-restore).
Tooltip: `claudeDesktop.route.tooltip.active {address,port}` / `…inactive {address,port}`. Same `Radio`/`Loader2` visual language.

## 4.14 `src/lib/requestOverrides.ts` (≈150 L)

Front-end validators for per-provider local-proxy request overrides, deliberately mirrored against the Rust `http` crate.

```ts
export interface RequestOverrideJsonResult { value?: Record<string, unknown>; error?: string }
export interface HeaderOverrideValidationResult { headers?: Record<string, string>; error?: string }
```
(`LocalProxyRequestOverrides` itself is in `src/types.ts`: `{headers?: Record<string,string>; body?: Record<string,unknown>}`.)

| Fn | Rule |
|---|---|
| `isValidHttpHeaderName(n)` | `/^[!#$%&'*+\-.^_\`\|~0-9A-Za-z]+$/` — RFC 9110 token, aligned with `http::HeaderName` |
| `isValidHttpHeaderValue(v)` | rejects `[\x00-\x08\x0a-\x1f\x7f]` — i.e. **all control chars except tab (`\x09`)**, matching `http::HeaderValue`'s runtime guard |
| `isPlainObject(v)` | object, non-null, not an array |
| `parseRequestOverrideJson(raw)` | blank → `{}`; `JSON.parse`; non-plain-object → `{error:"JSON must be an object"}`; throw → `{error: message}` |
| `parseBodyOverrideJson(raw)` | as above, then rejects a `stream` key: `'Body override must not include protocol field "stream"'` |
| `parseHeaderOverrideJson(raw)` | see below |

`parseHeaderOverrideJson` per entry, in order — **first failure aborts the whole parse**:
1. trimmed name empty → `"Header name must not be empty"`
2. invalid token → `` `Header "${name}" name is not a valid HTTP token` ``
3. non-string value → `` `Header "${name}" value must be a string` ``
4. control chars → `` `Header "${name}" value contains control characters` ``
5. lowercase-collision with an already-accepted header → `` `Header "${name}" duplicates another header after case normalization` ``
6. in `PROTECTED_LOCAL_PROXY_HEADER_NAMES` → `` `Header "${name}" is managed by the local proxy and cannot be overridden` ``
Accepted headers are stored **lowercased**.

`PROTECTED_LOCAL_PROXY_HEADER_NAMES` (54 entries) — hop-by-hop and framing (`host`, `content-length`, `transfer-encoding`, `connection`, `proxy-authorization`, `proxy-authenticate`, `te`, `trailer`, `upgrade`, `accept-encoding`, `content-type`); auth (`authorization`, `x-api-key`, `x-goog-api-key`, `chatgpt-account-id`); session/client (`session_id`, `x-client-request-id`, `x-codex-window-id`); forwarding (`x-forwarded-host/-port/-proto`, `forwarded`); CDN/edge (`cf-connecting-ip`, `cf-ipcountry`, `cf-ray`, `cf-visitor`, `true-client-ip`, `fastly-client-ip`, `x-azure-clientip`, `x-azure-fdid`, `x-azure-ref`, `akamai-origin-hop`, `x-akamai-config-log-detail`); tracing (`x-request-id`, `x-correlation-id`, `x-trace-id`, `x-amzn-trace-id`, `x-b3-traceid/-spanid/-parentspanid/-sampled`, `traceparent`, `tracestate`).

---

# 5. Port checklist — cross-cutting invariants

1. **Range resolution happens at fetch time, not key time** (§1.5). Keys must keep the flat `??`-padded tuple shape, with `null` sentinels for stats keys and `""`/`-1` for the logs key.
2. **`usageKeys.script()` shares the `["usage", …]` prefix with the dashboard**, so an `invalidate(all)` also drops script-usage caches (§1.5).
3. **Two different totals coexist by design**: Hero's `realTotalTokens` (input+output+cacheWrite+cacheRead) vs. the detail panel's `freshInput + output`. Don't unify.
4. **`cacheHitRate` denominator excludes output tokens.**
5. **Aggregate success rate must be rebuilt from counts, never averaged** (§1.9).
6. **`claude-desktop` folds into `claude` in every dashboard query but is shown verbatim in the request detail** (§1.3).
7. **Cache-inclusive app types (`codex`, `gemini`, `grokbuild`)**: subtract cache reads from input, and label cache *writes* N/A rather than 0.
8. **Error-rate thresholds are 0–1 on the wire, 0–100 in the UI** — ×100 on load, ÷100 on save, in both config panels.
9. **Number inputs are string-backed everywhere** so they can be emptied; validation is on blur (usage script) or on save (proxy panels), never on change.
10. **Keep-last-good** (§1.6) is a whitelist: only 5xx/429 and a fixed set of network phrases may mask a stale success; every other failure surfaces immediately **and clears the snapshot**.
11. **The `"v:"` option-value prefix** in the dashboard filters must survive the port, or a provider named `all` breaks the filter.
12. **Several mutations toast twice** (hook + caller): `ProxyPanel.handleSaveBasicConfig`, `handleLoggingChange`, `AutoFailoverConfigPanel.handleSave`. Decide once during the port.
13. `queryClient` defaults `staleTime: 0` + `refetchOnWindowFocus: true` mean focus always refetches, even with the dashboard refresh set to "off".
14. Fix B1–B5 (missing i18n keys, two of which leak Chinese into every locale) while porting; decide whether to drop or wire up B6/B7 (`RequestDetailPanel`, `DataSourceBar`) and B8 (duplicated proxy hooks) rather than porting dead code.
