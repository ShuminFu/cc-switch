import { call } from "./invoke";
import {
  commands,
  type AppConfigDirInfo,
  type AppType,
  type CodexHistoryBackupUsage,
  type ImportCandidate,
  type JsonValue,
  type LiveWritePlan,
  type McpSyncSkip,
  type ValidationIssue,
} from "@/lib/bindings";

/**
 * Live-config adapter diagnostics (CFG-12, CFG-22, CFG-23, CFG-44, CFG-46).
 * Everything here only reads, except `purgeCodexHistoryBackups`.
 */
export const adaptersApi = {
  /** Each tool's resolved config dir and whether it came from an override, env var or default. */
  async getConfigDirs(): Promise<AppConfigDirInfo[]> {
    return await call(commands.getAppConfigDirs());
  },

  /** MCP-capable apps whose config directory is missing (MCP sync writes nothing there). */
  async getMcpSyncSkips(): Promise<McpSyncSkip[]> {
    return await call(commands.getMcpSyncSkips());
  },

  /** Providers in an app's live config that can be imported. Contains credentials. */
  async getImportCandidates(app: AppType): Promise<ImportCandidate[]> {
    return await call(commands.getImportCandidates(app));
  },

  /**
   * What switching `app` to `providerId` would write, without writing.
   * `supported: false` means the app has no planner yet (files is empty).
   */
  async previewLiveWrite(
    app: AppType,
    providerId: string,
  ): Promise<LiveWritePlan> {
    return await call(commands.previewLiveWrite(app, providerId));
  },

  /** Advisory field issues in a provider's settings (CFG-24); never blocks a save. */
  async validateProviderSettings(
    app: AppType,
    settings: JsonValue,
  ): Promise<ValidationIssue[]> {
    return await call(commands.validateProviderSettings(app, settings));
  },

  async getCodexHistoryBackupUsage(): Promise<CodexHistoryBackupUsage> {
    return await call(commands.getCodexHistoryBackupUsage());
  },

  /** Keeps the official-history restore ledger unless `includeRestoreLedger`. */
  async purgeCodexHistoryBackups(
    includeRestoreLedger: boolean,
  ): Promise<CodexHistoryBackupUsage> {
    return await call(commands.purgeCodexHistoryBackups(includeRestoreLedger));
  },

  /** Whether Claude Desktop still routes through the local proxy (PROXY-30). */
  async getClaudeDesktopProxyDependency(): Promise<boolean> {
    return await call(commands.getClaudeDesktopProxyDependency());
  },
};
