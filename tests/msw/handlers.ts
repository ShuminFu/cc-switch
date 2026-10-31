import { http, HttpResponse, type JsonBodyType } from "msw";
import type { AppId } from "@/lib/api/types";
import type { McpServer, Provider, Settings } from "@/types";
import {
  addProvider,
  deleteProvider,
  deleteSession,
  getCurrentProviderId,
  getLiveProviderIds,
  getSessionMessages,
  getProviders,
  listProviders,
  listSessions,
  resetProviderState,
  setCurrentProviderId,
  updateProvider,
  updateSortOrder,
  getSettings,
  setSettings,
  getAppConfigDirOverride,
  setAppConfigDirOverrideState,
  getSyncSnapshotKeyStatus,
  setSyncSnapshotKeyState,
  getMcpConfig,
  setMcpServerEnabled,
  upsertMcpServer,
  deleteMcpServer,
} from "./state";
import type { CommandName, CommandResult } from "./commands";
import appRegistryFixture from "../fixtures/app-registry.json";

const TAURI_ENDPOINT = "http://tauri.local";

const withJson = async <T>(request: Request): Promise<T> => {
  try {
    const body = await request.text();
    if (!body) return {} as T;
    return JSON.parse(body) as T;
  } catch {
    return {} as T;
  }
};

type Resolved<K extends CommandName> = CommandResult<K> | HttpResponse<any>;

/**
 * 注册一个 Tauri 命令的 mock。命令名必须是 `generate_handler!` 中已注册的命令，
 * 返回值按生成的绑定类型校验（见 `./commands`）；返回 HttpResponse 可模拟失败。
 */
export const command = <K extends CommandName>(
  name: K,
  resolve: (request: Request) => Resolved<K> | Promise<Resolved<K>>,
) =>
  http.post(`${TAURI_ENDPOINT}/${name}`, async ({ request }) => {
    const result = await resolve(request);
    return result instanceof HttpResponse
      ? result
      : HttpResponse.json(result as JsonBodyType);
  });

export const handlers = [
  command("get_app_registry", () => appRegistryFixture),
  command("get_migration_result", () => false),
  command("get_skills_migration_result", () => null),
  command("get_startup_report", () => ({ steps: [] })),
  command("get_settings_load_failure", () => null),
  command("acknowledge_settings_load_failure", () => true),
  command("get_providers", async (request) => {
    const { app } = await withJson<{ app: AppId }>(request);
    // 后端 `inFailoverQueue` 为 `#[serde(default)] bool`，序列化时总会带上
    return Object.fromEntries(
      Object.entries(getProviders(app)).map(([id, provider]) => [
        id,
        { ...provider, inFailoverQueue: provider.inFailoverQueue ?? false },
      ]),
    );
  }),

  command("get_current_provider", async (request) => {
    const { app } = await withJson<{ app: AppId }>(request);
    return getCurrentProviderId(app);
  }),

  command("update_providers_sort_order", async (request) => {
    const { updates = [], app } = await withJson<{
      updates: { id: string; sortIndex: number }[];
      app: AppId;
    }>(request);
    updateSortOrder(app, updates);
    return true;
  }),

  command("update_tray_menu", () => true),

  command("get_opencode_live_provider_ids", () =>
    getLiveProviderIds("opencode"),
  ),

  command("get_openclaw_live_provider_ids", () =>
    getLiveProviderIds("openclaw"),
  ),

  command("get_openclaw_default_model", () => ({
    primary: null,
    fallback: [],
  })),

  command("scan_openclaw_config_health", () => []),

  command("switch_provider", async (request) => {
    const { id, app } = await withJson<{ id: string; app: AppId }>(request);
    const providers = listProviders(app);
    if (!providers[id]) {
      return HttpResponse.json(false, { status: 404 });
    }
    setCurrentProviderId(app, id);
    return true;
  }),

  command("revert_last_switch", () => ({
    providerId: null,
    restoredFiles: [],
  })),

  command("add_provider", async (request) => {
    const { provider, app } = await withJson<{
      provider: Provider & { id?: string };
      app: AppId;
    }>(request);

    const newId = provider.id ?? `mock-${Date.now()}`;
    addProvider(app, { ...provider, id: newId });
    return true;
  }),

  command("update_provider", async (request) => {
    const { provider, app } = await withJson<{
      provider: Provider;
      app: AppId;
    }>(request);
    updateProvider(app, provider);
    return true;
  }),

  // 默认不写 live（等同非当前供应商），保存流程不弹 diff；需要时在用例内覆盖
  command("preview_live_write", () => ({
    writesLive: false,
    skipReason: "notCurrent" as const,
    files: [],
  })),

  command("delete_provider", async (request) => {
    const { id, app } = await withJson<{ id: string; app: AppId }>(request);
    deleteProvider(app, id);
    return true;
  }),

  command("import_default_config", async () => {
    resetProviderState();
    return true;
  }),

  command("open_external", () => true),

  command("list_sessions", () => listSessions()),

  command("get_session_messages", async (request) => {
    const { providerId, sourcePath } = await withJson<{
      providerId: string;
      sourcePath: string;
    }>(request);
    return getSessionMessages(providerId, sourcePath);
  }),

  command("delete_session", async (request) => {
    const { providerId, sessionId, sourcePath } = await withJson<{
      providerId: string;
      sessionId: string;
      sourcePath: string;
    }>(request);
    return deleteSession(providerId, sessionId, sourcePath);
  }),

  command("delete_sessions", async (request) => {
    const { items = [] } = await withJson<{
      items?: {
        providerId: string;
        sessionId: string;
        sourcePath: string;
      }[];
    }>(request);

    return items.map((item) => ({
      providerId: item.providerId,
      sessionId: item.sessionId,
      sourcePath: item.sourcePath,
      success: deleteSession(item.providerId, item.sessionId, item.sourcePath),
    }));
  }),

  // MCP APIs
  command("get_mcp_config", async (request) => {
    const { app } = await withJson<{ app: AppId }>(request);
    return getMcpConfig(app);
  }),

  command("set_mcp_enabled", async (request) => {
    const { app, id, enabled } = await withJson<{
      app: AppId;
      id: string;
      enabled: boolean;
    }>(request);
    setMcpServerEnabled(app, id, enabled);
    return true;
  }),

  command("upsert_mcp_server_in_config", async (request) => {
    const { app, id, spec } = await withJson<{
      app: AppId;
      id: string;
      spec: McpServer;
    }>(request);
    upsertMcpServer(app, id, spec);
    return true;
  }),

  command("delete_mcp_server_in_config", async (request) => {
    const { app, id } = await withJson<{ app: AppId; id: string }>(request);
    deleteMcpServer(app, id);
    return true;
  }),

  command("restart_app", () => true),

  command("get_settings", () => getSettings()),

  command("check_env_conflicts", () => []),

  command("save_settings", async (request) => {
    const { settings } = await withJson<{ settings: Settings }>(request);
    setSettings(settings);
    return true;
  }),

  command("set_app_config_dir_override", async (request) => {
    const { path } = await withJson<{ path: string | null }>(request);
    setAppConfigDirOverrideState(path ?? null);
    return true;
  }),

  command("get_app_config_dir_override", () => getAppConfigDirOverride()),

  command("get_sync_snapshot_key_status", () => getSyncSnapshotKeyStatus()),

  command("set_sync_snapshot_key", async (request) => {
    const { key } = await withJson<{ key: string | null }>(request);
    if (key && Array.from(key.trim()).length < 8) {
      return HttpResponse.json(
        {
          code: "LOCALIZED",
          message: "The sync key must be at least 8 characters.",
          details: null,
          i18nKey: "sync.key_too_short",
        },
        { status: 400 },
      );
    }
    return setSyncSnapshotKeyState(key ?? null);
  }),

  command("apply_claude_plugin_config", async (request) => {
    const { official } = await withJson<{ official: boolean }>(request);
    setSettings({ enableClaudePluginIntegration: !official });
    return true;
  }),

  command("apply_claude_onboarding_skip", () => true),

  command("clear_claude_onboarding_skip", () => true),

  command("get_config_dir", async (request) => {
    const { app } = await withJson<{ app: AppId }>(request);
    return app === "claude" ? "/default/claude" : "/default/codex";
  }),

  command("is_portable_mode", () => false),

  command("pick_directory", async (request) => {
    const { defaultPath, default_path } = await withJson<{
      defaultPath?: string;
      default_path?: string;
    }>(request);
    const initial = defaultPath ?? default_path;
    return initial ? `${initial}/picked` : "/mock/selected-dir";
  }),

  command("open_file_dialog", () => "/mock/import-settings.json"),

  command("import_config_from_file", async (request) => {
    const { filePath } = await withJson<{ filePath: string }>(request);
    if (!filePath) {
      return { success: false, message: "Missing file" };
    }
    setSettings({ language: "en" });
    return { success: true, backupId: "backup-123" };
  }),

  command("export_config_to_file", async (request) => {
    const { filePath } = await withJson<{ filePath: string }>(request);
    if (!filePath) {
      return { success: false, message: "Invalid destination" };
    }
    return { success: true, filePath };
  }),

  command("save_file_dialog", () => "/mock/export-settings.json"),

  // Sync current providers live (no-op success)
  command("sync_current_providers_live", () => ({ success: true })),

  // Proxy status (for SettingsPage / ProxyPanel hooks)
  command("get_proxy_status", () => ({
    running: false,
    address: "127.0.0.1",
    port: 0,
    active_connections: 0,
    total_requests: 0,
    success_requests: 0,
    failed_requests: 0,
    success_rate: 0,
    uptime_seconds: 0,
    current_provider: null,
    current_provider_id: null,
    last_request_at: null,
    last_error: null,
    failover_count: 0,
    active_targets: [],
  })),

  command("get_proxy_takeover_status", () => ({
    claude: false,
    codex: false,
    gemini: false,
    grokbuild: false,
  })),

  command("is_live_takeover_active", () => false),

  // Failover / circuit breaker defaults
  command("get_failover_queue", () => []),
  command("get_available_providers_for_failover", () => []),
  command("add_to_failover_queue", () => true),
  command("remove_from_failover_queue", () => true),

  command("get_circuit_breaker_config", () => ({
    failureThreshold: 3,
    successThreshold: 2,
    timeoutSeconds: 60,
    errorRateThreshold: 50,
    minRequests: 5,
  })),
  command("update_circuit_breaker_config", () => true),
  command("get_provider_health", () => ({
    provider_id: "mock-provider",
    app_type: "claude",
    is_healthy: true,
    consecutive_failures: 0,
    last_success_at: null,
    last_failure_at: null,
    last_error: null,
    updated_at: new Date().toISOString(),
  })),
  command("reset_circuit_breaker", () => true),
  command("get_circuit_breaker_stats", () => null),
  command("probe_circuit_breaker", () => null),
];
