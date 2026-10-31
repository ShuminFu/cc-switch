/**
 * Query-key registry (FE-ARCH-04).
 *
 * Every TanStack Query key used by queries, mutations and invalidations should
 * come from here, so producers and consumers cannot drift apart and prefix
 * invalidation is predictable. Keys are hierarchical: invalidating a parent
 * (e.g. `queryKeys.providers.all`) also invalidates its children.
 *
 * The array shapes intentionally match the literal keys used before this
 * registry existed, so call sites outside `src/lib/query` that still spell a
 * key literally keep sharing the same cache entry while they migrate. The one
 * deliberate change is the per-provider usage-script key, which now lives
 * under its own `"usage-script"` root instead of `"usage"`: invalidating the
 * usage dashboard (pricing edits, new request logs) must not refetch every
 * provider card's third-party quota endpoint.
 */
import type { AppId } from "@/lib/api/types";
import type { HermesMemoryKind } from "@/types";
import type { UsageRangeSelection, UsageScopeFilters } from "@/types/usage";

export type RequestLogsKey = {
  preset: UsageRangeSelection["preset"];
  customStartDate?: number;
  customEndDate?: number;
  liveEndTime?: boolean;
  appType?: string;
  providerName?: string;
  model?: string;
  statusCode?: number;
  statusMin?: number;
  statusMax?: number;
  /**
   * Upper bound frozen while paging (FE-ARCH-41): pages after the first use
   * the window captured when the user left page 0, so new rows do not shift
   * later pages.
   */
  frozenEndDate?: number;
};

// Query keys
export const usageKeys = {
  all: ["usage"] as const,
  summary: (
    preset: UsageRangeSelection["preset"],
    customStartDate: number | undefined,
    customEndDate: number | undefined,
    filters?: UsageScopeFilters,
    liveEndTime?: boolean,
  ) =>
    [
      ...usageKeys.all,
      "summary",
      preset,
      customStartDate ?? 0,
      customEndDate ?? 0,
      liveEndTime ?? false,
      filters?.appType ?? null,
      filters?.providerName ?? null,
      filters?.model ?? null,
    ] as const,
  summaryByApp: (
    preset: UsageRangeSelection["preset"],
    customStartDate: number | undefined,
    customEndDate: number | undefined,
    filters?: Pick<UsageScopeFilters, "providerName" | "model">,
    liveEndTime?: boolean,
  ) =>
    [
      ...usageKeys.all,
      "summary-by-app",
      preset,
      customStartDate ?? 0,
      customEndDate ?? 0,
      liveEndTime ?? false,
      filters?.providerName ?? null,
      filters?.model ?? null,
    ] as const,
  trends: (
    preset: UsageRangeSelection["preset"],
    customStartDate: number | undefined,
    customEndDate: number | undefined,
    filters?: UsageScopeFilters,
    liveEndTime?: boolean,
  ) =>
    [
      ...usageKeys.all,
      "trends",
      preset,
      customStartDate ?? 0,
      customEndDate ?? 0,
      liveEndTime ?? false,
      filters?.appType ?? null,
      filters?.providerName ?? null,
      filters?.model ?? null,
    ] as const,
  providerStats: (
    preset: UsageRangeSelection["preset"],
    customStartDate: number | undefined,
    customEndDate: number | undefined,
    filters?: UsageScopeFilters,
    liveEndTime?: boolean,
  ) =>
    [
      ...usageKeys.all,
      "provider-stats",
      preset,
      customStartDate ?? 0,
      customEndDate ?? 0,
      liveEndTime ?? false,
      filters?.appType ?? null,
      filters?.providerName ?? null,
      filters?.model ?? null,
    ] as const,
  modelStats: (
    preset: UsageRangeSelection["preset"],
    customStartDate: number | undefined,
    customEndDate: number | undefined,
    filters?: UsageScopeFilters,
    liveEndTime?: boolean,
  ) =>
    [
      ...usageKeys.all,
      "model-stats",
      preset,
      customStartDate ?? 0,
      customEndDate ?? 0,
      liveEndTime ?? false,
      filters?.appType ?? null,
      filters?.providerName ?? null,
      filters?.model ?? null,
    ] as const,
  logs: (key: RequestLogsKey, page: number, pageSize: number) =>
    [
      ...usageKeys.all,
      "logs",
      key.preset,
      key.customStartDate ?? 0,
      key.customEndDate ?? 0,
      key.liveEndTime ?? false,
      key.appType ?? "",
      key.providerName ?? "",
      key.model ?? "",
      key.statusCode ?? -1,
      key.statusMin ?? -1,
      key.statusMax ?? -1,
      key.frozenEndDate ?? 0,
      page,
      pageSize,
    ] as const,
  detail: (requestId: string) =>
    [...usageKeys.all, "detail", requestId] as const,
  pricing: () => [...usageKeys.all, "pricing"] as const,
  limits: (providerId: string, appType: string) =>
    [...usageKeys.all, "limits", providerId, appType] as const,
  dataSources: () => [...usageKeys.all, "data-sources"] as const,
  /**
   * Per-provider usage-script query. Deliberately NOT under `usageKeys.all`
   * (FE-ARCH-04): dashboard invalidations must not refetch third-party quota
   * endpoints for every provider card.
   */
  script: (providerId: string, appType: string) =>
    ["usage-script", providerId, appType] as const,
};

export const queryKeys = {
  settings: {
    all: ["settings"] as const,
  },

  providers: {
    all: ["providers"] as const,
    byApp: (appId: AppId | string) => ["providers", appId] as const,
    opencodeLiveIds: ["opencodeLiveProviderIds"] as const,
    claudeDesktopStatus: ["claudeDesktopStatus"] as const,
    claudeDesktopDefaultRoutes: ["claudeDesktopDefaultRoutes"] as const,
  },

  profiles: {
    all: ["profiles"] as const,
  },

  /** Claude Code subagents in ~/.claude/agents, PRODUCT-42. */
  claudeAgents: {
    all: ["claudeAgents"] as const,
  },

  /** Managed (team) policy file state, PRODUCT-24. */
  policy: {
    status: ["managedPolicyStatus"] as const,
  },

  /** Request-log retention window, PRODUCT-31. */
  logRetention: {
    status: ["requestLogRetention"] as const,
    /** Stream-check (connectivity probe) log retention, DB-25. */
    streamCheck: ["streamCheckRetention"] as const,
  },

  /** Workbench features in `src/components/workbench` (PRODUCT-14/21/22/25/43). */
  workbench: {
    customTools: ["customTools"] as const,
    history: ["configHistory"] as const,
    providerHistory: (app: string, id: string) =>
      ["configHistory", app, id] as const,
    audit: (limit: number) => ["auditEvents", limit] as const,
    sessionUsage: (sessionId: string) => ["sessionUsage", sessionId] as const,
    projectUsage: (days: number) => ["projectUsage", days] as const,
  },

  proxy: {
    status: ["proxyStatus"] as const,
    takeoverStatus: ["proxyTakeoverStatus"] as const,
    /** Legacy keys kept for invalidation of any remaining observers. */
    running: ["proxyRunning"] as const,
    liveTakeoverActive: ["liveTakeoverActive"] as const,
    config: ["proxyConfig"] as const,
    globalConfig: ["globalProxyConfig"] as const,
    appConfig: (appType: string) => ["appProxyConfig", appType] as const,
    upstreamStatus: ["upstreamProxyStatus"] as const,
    globalUrl: ["globalProxyUrl"] as const,
  },

  failover: {
    providerHealthAll: ["providerHealth"] as const,
    providerHealth: (providerId: string, appType: string) =>
      ["providerHealth", providerId, appType] as const,
    /** Batched health of one app's providers (`get_providers_health`). */
    providersHealthByApp: (appType: string) =>
      ["providerHealth", "batch", appType] as const,
    providersHealth: (appType: string, providerIds: readonly string[]) =>
      ["providerHealth", "batch", appType, providerIds.join("\n")] as const,
    circuitBreakerConfig: ["circuitBreakerConfig"] as const,
    circuitBreakerStatsAll: ["circuitBreakerStats"] as const,
    circuitBreakerStats: (providerId: string, appType: string) =>
      ["circuitBreakerStats", providerId, appType] as const,
    queue: (appType: string) => ["failoverQueue", appType] as const,
    availableProviders: (appType: string) =>
      ["availableProvidersForFailover", appType] as const,
    autoEnabled: (appType: string) => ["autoFailoverEnabled", appType] as const,
  },

  mcp: {
    root: ["mcp"] as const,
    all: ["mcp", "all"] as const,
    /** Apps whose config dir is missing, so MCP sync skipped them (CFG-46). */
    syncSkips: ["mcp", "syncSkips"] as const,
  },

  /** Live-config adapter diagnostics (`src/lib/api/adapters.ts`). */
  adapters: {
    all: ["adapters"] as const,
    configDirs: ["adapters", "configDirs"] as const,
    importCandidates: (app: AppId | string) =>
      ["adapters", "importCandidates", app] as const,
    livePreview: (app: AppId | string, providerId: string) =>
      ["adapters", "livePreview", app, providerId] as const,
    validation: (app: AppId | string, settingsJson: string) =>
      ["adapters", "validation", app, settingsJson] as const,
    codexHistoryBackupUsage: ["adapters", "codexHistoryBackupUsage"] as const,
    claudeDesktopProxyDependency: [
      "adapters",
      "claudeDesktopProxyDependency",
    ] as const,
  },

  skills: {
    all: ["skills"] as const,
    installed: ["skills", "installed"] as const,
    discoverable: ["skills", "discoverable"] as const,
    repos: ["skills", "repos"] as const,
    backups: ["skills", "backups"] as const,
    unmanaged: ["skills", "unmanaged"] as const,
    updates: ["skills", "updates"] as const,
    skillssh: (query: string, limit: number, offset: number) =>
      ["skills", "skillssh", query, limit, offset] as const,
  },

  sessions: {
    all: ["sessions"] as const,
    /** Recently deleted sessions (SESS-41); under `all` so deletes refresh it. */
    trash: ["sessions", "trash"] as const,
    /** Pin / archive / tags (SESS-18). */
    annotations: ["sessions", "annotations"] as const,
    messages: (providerId?: string, sourcePath?: string) =>
      ["sessionMessages", providerId, sourcePath] as const,
  },

  /** Per-provider usage-script / balance query (third-party endpoints). */
  usageScript: {
    all: ["usage-script"] as const,
    byProvider: usageKeys.script,
  },

  /** Usage dashboard (request-log aggregates, pricing, limits). */
  usage: usageKeys,

  subscription: {
    all: ["subscription"] as const,
    quota: (appId: AppId | string) => ["subscription", "quota", appId] as const,
  },

  codexOauth: {
    quota: (accountId?: string | null) =>
      ["codex_oauth", "quota", accountId ?? "default"] as const,
  },

  copilot: {
    quota: (accountId?: string | null) =>
      ["copilot", "quota", accountId ?? "default"] as const,
  },

  omo: {
    all: ["omo"] as const,
    currentProviderId: ["omo", "current-provider-id"] as const,
    providerCount: ["omo", "provider-count"] as const,
  },

  omoSlim: {
    all: ["omo-slim"] as const,
    currentProviderId: ["omo-slim", "current-provider-id"] as const,
    providerCount: ["omo-slim", "provider-count"] as const,
  },

  openclaw: {
    all: ["openclaw"] as const,
    liveProviderIds: ["openclaw", "liveProviderIds"] as const,
    defaultModel: ["openclaw", "defaultModel"] as const,
    env: ["openclaw", "env"] as const,
    tools: ["openclaw", "tools"] as const,
    agentsDefaults: ["openclaw", "agentsDefaults"] as const,
    health: ["openclaw", "health"] as const,
  },

  hermes: {
    all: ["hermes"] as const,
    liveProviderIds: ["hermes", "liveProviderIds"] as const,
    modelConfig: ["hermes", "modelConfig"] as const,
    memory: (kind: HermesMemoryKind) => ["hermes", "memory", kind] as const,
    memoryLimits: ["hermes", "memoryLimits"] as const,
  },

  backups: {
    db: ["db-backups"] as const,
  },

  prompts: {
    all: ["prompts"] as const,
    byApp: (appId: AppId | string) => ["prompts", appId] as const,
    currentFile: (appId: AppId | string) =>
      ["prompts", appId, "current-file"] as const,
  },

  budget: {
    enforcement: ["budget", "enforcement"] as const,
  },

  sync: {
    encryptionStatus: ["syncEncryptionStatus"] as const,
  },

  secrets: {
    vaultStatus: ["secretsVaultStatus"] as const,
  },

  projects: {
    bindings: ["projectBindings"] as const,
  },

  managedAuth: {
    status: (authProvider: string) =>
      ["managed-auth-status", authProvider] as const,
  },

  modelsDevPricing: ["models-dev-pricing"] as const,

  /** Backend config blobs edited by Settings/Usage panels (FE-UI-10). */
  backendConfig: {
    rectifier: ["backendConfig", "rectifier"] as const,
    optimizer: ["backendConfig", "optimizer"] as const,
    log: ["backendConfig", "log"] as const,
    streamCheck: ["backendConfig", "streamCheck"] as const,
  },
} as const;

/**
 * Key of the "live provider ids" query for apps with an additive live config,
 * or `null` for apps that replace the live config on switch.
 */
export const liveProviderIdsKey = (appId: AppId) => {
  switch (appId) {
    case "opencode":
      return queryKeys.providers.opencodeLiveIds;
    case "openclaw":
      return queryKeys.openclaw.liveProviderIds;
    case "hermes":
      return queryKeys.hermes.liveProviderIds;
    default:
      return null;
  }
};
