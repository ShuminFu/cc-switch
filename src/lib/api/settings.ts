import { invoke, call, callJson } from "./invoke";
import { commands } from "@/lib/bindings";
import type { CodexUnifyHistoryRestoreResult } from "@/lib/bindings";
export type { CodexUnifyHistoryRestoreResult } from "@/lib/bindings";
import type {
  Settings,
  WebDavSyncSettings,
  S3SyncSettings,
  RemoteSnapshotInfo,
} from "@/types";
import type { AppId } from "./types";

export interface ConfigTransferResult {
  success: boolean;
  message: string;
  filePath?: string;
  backupId?: string;
}

export interface WebDavTestResult {
  success: boolean;
  message?: string;
  /** The endpoint is plain HTTP (credentials and data are not encrypted). */
  insecureTransport?: boolean;
}

export interface WebDavSyncResult {
  status: string;
  insecureTransport?: boolean;
  /** Best-effort post-download warning (live config rewrite failed). */
  warning?: string;
}

/** Remote snapshot preview returned by `*_sync_fetch_remote_info`. */
export type CloudRemoteSnapshotInfo = RemoteSnapshotInfo & {
  /** The remote changed since this device last synced (another device uploaded). */
  remoteChangedSinceLastSync?: boolean;
  insecureTransport?: boolean;
};

export type CloudRemoteInfoResult = CloudRemoteSnapshotInfo | { empty: true };

/**
 * Manual upload/download options. `force` defaults to `true` on the backend
 * for compatibility; pass `false` to get a `sync.conflict.*` error when the
 * other side changed since the last sync, then retry with `true` once the
 * user explicitly chose to overwrite.
 */
export interface CloudSyncTransferOptions {
  force?: boolean;
}

export const settingsApi = {
  async get(): Promise<Settings> {
    return await invoke("get_settings");
  },

  async save(settings: Settings): Promise<boolean> {
    return await invoke("save_settings", { settings });
  },

  /** 是否存在统一 Codex 会话历史的迁移备份（关闭弹窗据此显示"恢复备份"勾选） */
  async hasCodexUnifyHistoryBackup(): Promise<boolean> {
    return await call(commands.hasCodexUnifyHistoryBackup());
  },

  /** 按迁移备份账本把当时迁入共享桶的官方会话还原回 openai 桶（幂等） */
  async restoreCodexUnifiedHistory(): Promise<CodexUnifyHistoryRestoreResult> {
    return await call(commands.restoreCodexUnifiedHistory());
  },

  async restart(): Promise<boolean> {
    return await call(commands.restartApp());
  },

  async installUpdateAndRestart(): Promise<boolean> {
    return await call(commands.installUpdateAndRestart());
  },

  async checkUpdates(): Promise<void> {
    await call(commands.checkForUpdates());
  },

  async isPortable(): Promise<boolean> {
    return await call(commands.isPortableMode());
  },

  async getConfigDir(appId: AppId): Promise<string> {
    return await call(commands.getConfigDir(appId));
  },

  async openConfigFolder(appId: AppId): Promise<void> {
    await call(commands.openConfigFolder(appId));
  },

  async pickDirectory(defaultPath?: string): Promise<string | null> {
    return await call(commands.pickDirectory(defaultPath ?? null));
  },

  async selectConfigDirectory(defaultPath?: string): Promise<string | null> {
    return await call(commands.pickDirectory(defaultPath ?? null));
  },

  async getClaudeCodeConfigPath(): Promise<string> {
    return await call(commands.getClaudeCodeConfigPath());
  },

  async getAppConfigPath(): Promise<string> {
    return await call(commands.getAppConfigPath());
  },

  async openAppConfigFolder(): Promise<void> {
    await call(commands.openAppConfigFolder());
  },

  async getAppConfigDirOverride(): Promise<string | null> {
    return await call(commands.getAppConfigDirOverride());
  },

  async setAppConfigDirOverride(path: string | null): Promise<boolean> {
    return await call(commands.setAppConfigDirOverride(path ?? null));
  },

  async applyClaudePluginConfig(options: {
    official: boolean;
  }): Promise<boolean> {
    const { official } = options;
    return await call(commands.applyClaudePluginConfig(official));
  },

  async applyClaudeOnboardingSkip(): Promise<boolean> {
    return await call(commands.applyClaudeOnboardingSkip());
  },

  async clearClaudeOnboardingSkip(): Promise<boolean> {
    return await call(commands.clearClaudeOnboardingSkip());
  },

  async saveFileDialog(defaultName: string): Promise<string | null> {
    return await invoke("save_file_dialog", { defaultName });
  },

  async openFileDialog(): Promise<string | null> {
    return await invoke("open_file_dialog");
  },

  async exportConfigToFile(filePath: string): Promise<ConfigTransferResult> {
    return await callJson(commands.exportConfigToFile(filePath));
  },

  async importConfigFromFile(filePath: string): Promise<ConfigTransferResult> {
    return await callJson(commands.importConfigFromFile(filePath));
  },

  // ─── WebDAV sync ──────────────────────────────────────────

  async webdavTestConnection(
    settings: WebDavSyncSettings,
    preserveEmptyPassword = true,
  ): Promise<WebDavTestResult> {
    return await invoke("webdav_test_connection", {
      settings,
      preserveEmptyPassword,
    });
  },

  async webdavSyncUpload(
    options: CloudSyncTransferOptions = {},
  ): Promise<WebDavSyncResult> {
    return await callJson(commands.webdavSyncUpload(options.force ?? null));
  },

  async webdavSyncDownload(
    options: CloudSyncTransferOptions = {},
  ): Promise<WebDavSyncResult> {
    return await callJson(commands.webdavSyncDownload(options.force ?? null));
  },

  async webdavSyncSaveSettings(
    settings: WebDavSyncSettings,
    passwordTouched = false,
  ): Promise<{ success: boolean }> {
    return await invoke("webdav_sync_save_settings", {
      settings,
      passwordTouched,
    });
  },

  async webdavSyncFetchRemoteInfo(): Promise<CloudRemoteInfoResult> {
    return await callJson(commands.webdavSyncFetchRemoteInfo());
  },

  // ===== S3 Sync API =====

  async s3TestConnection(
    settings: S3SyncSettings,
    preserveEmptyPassword = true,
  ): Promise<WebDavTestResult> {
    return await invoke("s3_test_connection", {
      settings,
      preserveEmptyPassword,
    });
  },

  async s3SyncUpload(
    options: CloudSyncTransferOptions = {},
  ): Promise<WebDavSyncResult> {
    return await callJson(commands.s3SyncUpload(options.force ?? null));
  },

  async s3SyncDownload(
    options: CloudSyncTransferOptions = {},
  ): Promise<WebDavSyncResult> {
    return await callJson(commands.s3SyncDownload(options.force ?? null));
  },

  async s3SyncSaveSettings(
    settings: S3SyncSettings,
    passwordTouched: boolean,
  ): Promise<{ success: boolean }> {
    return await invoke("s3_sync_save_settings", {
      settings,
      passwordTouched,
    });
  },

  async s3SyncFetchRemoteInfo(): Promise<CloudRemoteInfoResult> {
    return await callJson(commands.s3SyncFetchRemoteInfo());
  },

  async syncCurrentProvidersLive(): Promise<void> {
    const result = (await callJson(commands.syncCurrentProvidersLive())) as {
      success?: boolean;
      message?: string;
    };
    if (!result?.success) {
      throw new Error(result?.message || "Sync current providers failed");
    }
  },

  async openExternal(url: string): Promise<void> {
    try {
      const u = new URL(url);
      const scheme = u.protocol.replace(":", "").toLowerCase();
      if (scheme !== "http" && scheme !== "https") {
        throw new Error("Unsupported URL scheme");
      }
    } catch {
      throw new Error("Invalid URL");
    }
    await call(commands.openExternal(url));
  },

  async setAutoLaunch(enabled: boolean): Promise<boolean> {
    return await call(commands.setAutoLaunch(enabled));
  },

  async getAutoLaunchStatus(): Promise<boolean> {
    return await call(commands.getAutoLaunchStatus());
  },

  async getToolVersions(
    tools?: string[],
    wslShellByTool?: Record<
      string,
      { wslShell?: string | null; wslShellFlag?: string | null }
    >,
  ): Promise<
    Array<{
      name: string;
      version: string | null;
      latest_version: string | null;
      error: string | null;
      installed_but_broken: boolean;
      env_type: "windows" | "wsl" | "macos" | "linux" | "unknown";
      wsl_distro: string | null;
    }>
  > {
    return await invoke("get_tool_versions", { tools, wslShellByTool });
  },

  async runToolLifecycleAction(
    tools: string[],
    action: "install" | "update",
    wslShellByTool?: Record<
      string,
      { wslShell?: string | null; wslShellFlag?: string | null }
    >,
  ): Promise<void> {
    await invoke("run_tool_lifecycle_action", {
      tools,
      action,
      wslShellByTool,
    });
  },

  /** 探测各工具安装分布：枚举所有安装、标记冲突、生成锚定升级命令。
   *  诊断按钮、升级前确认、升级后补诊共用此命令，各取所需字段。 */
  async probeToolInstallations(
    tools: string[],
  ): Promise<ToolInstallationReport[]> {
    return await invoke("probe_tool_installations", { tools });
  },

  async getRectifierConfig(): Promise<RectifierConfig> {
    return await invoke("get_rectifier_config");
  },

  async setRectifierConfig(config: RectifierConfig): Promise<boolean> {
    return await invoke("set_rectifier_config", { config });
  },

  async getOptimizerConfig(): Promise<OptimizerConfig> {
    return await invoke("get_optimizer_config");
  },

  async setOptimizerConfig(config: OptimizerConfig): Promise<boolean> {
    return await invoke("set_optimizer_config", { config });
  },

  async getLogConfig(): Promise<LogConfig> {
    return await invoke("get_log_config");
  },

  async setLogConfig(config: LogConfig): Promise<boolean> {
    return await invoke("set_log_config", { config });
  },
};

/** 单处工具安装的诊断信息（多处安装冲突检测）。字段对应后端 ToolInstallation。 */
export interface ToolInstallation {
  path: string;
  version: string | null;
  runnable: boolean;
  error: string | null;
  source: string;
  is_path_default: boolean;
}

/** 一次"探测工具安装分布"的结果。字段对应后端 ToolInstallationReport。 */
export interface ToolInstallationReport {
  tool: string;
  installs: ToolInstallation[];
  is_conflict: boolean;
  needs_confirmation: boolean;
  command: string;
  anchored: boolean;
}

export interface RectifierConfig {
  enabled: boolean;
  requestThinkingSignature: boolean;
  requestThinkingBudget: boolean;
  requestMediaFallback: boolean;
  requestMediaHeuristic: boolean;
}

export interface OptimizerConfig {
  enabled: boolean;
  thinkingOptimizer: boolean;
  cacheInjection: boolean;
}

export interface LogConfig {
  enabled: boolean;
  level: "error" | "warn" | "info" | "debug" | "trace";
}

export interface BackupEntry {
  filename: string;
  sizeBytes: number;
  createdAt: string;
}

export const backupsApi = {
  async createDbBackup(): Promise<string> {
    return await call(commands.createDbBackup());
  },

  async listDbBackups(): Promise<BackupEntry[]> {
    return await invoke("list_db_backups");
  },

  async restoreDbBackup(filename: string): Promise<string> {
    return await call(commands.restoreDbBackup(filename));
  },

  async renameDbBackup(oldFilename: string, newName: string): Promise<string> {
    return await call(commands.renameDbBackup(oldFilename, newName));
  },

  async deleteDbBackup(filename: string): Promise<void> {
    await call(commands.deleteDbBackup(filename));
  },
};
