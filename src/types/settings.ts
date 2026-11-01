import type { SkillStorageLocation, SkillSyncMethod } from "./provider";

// 主页面显示的应用配置
export interface VisibleApps {
  claude: boolean;
  "claude-desktop": boolean;
  codex: boolean;
  gemini: boolean;
  grokbuild: boolean;
  opencode: boolean;
  openclaw: boolean;
  hermes: boolean;
}

// WebDAV 同步状态
export interface WebDavSyncStatus {
  lastSyncAt?: number | null;
  lastError?: string | null;
  lastErrorSource?: string | null;
  lastRemoteEtag?: string | null;
  lastLocalManifestHash?: string | null;
  lastRemoteManifestHash?: string | null;
}

// WebDAV 同步配置
export interface WebDavSyncSettings {
  enabled?: boolean;
  autoSync?: boolean;
  baseUrl?: string;
  username?: string;
  password?: string;
  remoteRoot?: string;
  profile?: string;
  status?: WebDavSyncStatus;
}

// S3 同步配置
export interface S3SyncSettings {
  enabled?: boolean;
  autoSync?: boolean;
  region?: string;
  bucket?: string;
  accessKeyId?: string;
  secretAccessKey?: string;
  endpoint?: string;
  remoteRoot?: string;
  profile?: string;
  status?: WebDavSyncStatus;
}

export type RemoteSnapshotLayout = "current" | "legacy";

// 远端快照信息（下载前预览）
export interface RemoteSnapshotInfo {
  deviceName: string;
  createdAt: string;
  snapshotId: string;
  version: number;
  protocolVersion: number;
  dbCompatVersion?: number | null;
  compatible: boolean;
  artifacts: string[];
  layout: RemoteSnapshotLayout;
  remotePath: string;
  /** The remote snapshot is end-to-end encrypted. */
  encrypted?: boolean;
  /** Whether the local passphrase unlocks it (null: not encrypted / no passphrase). */
  passphraseMatches?: boolean | null;
}

// 应用设置类型（用于设置对话框与 Tauri API）
// 存储在本地 ~/.cc-switch/settings.json，不随数据库同步
export interface Settings {
  // ===== 设备级 UI 设置 =====
  // 是否在系统托盘（macOS 菜单栏）显示图标
  showInTray: boolean;
  // 点击关闭按钮时是否最小化到托盘而不是关闭应用
  minimizeToTrayOnClose: boolean;
  // 是否启用应用级窗口控制按钮（最小化/最大化/关闭）
  useAppWindowControls?: boolean;
  // 启用 Claude 插件联动（写入 ~/.claude/config.json 的 primaryApiKey）
  enableClaudePluginIntegration?: boolean;
  // 跳过 Claude Code 初次安装确认（写入 ~/.claude.json 的 hasCompletedOnboarding）
  skipClaudeOnboarding?: boolean;
  // 是否开机自启
  launchOnStartup?: boolean;
  // 静默启动（程序启动时不显示主窗口）
  silentStartup?: boolean;
  // 是否启用主页面本地代理功能（默认关闭）
  enableLocalProxy?: boolean;
  // User has confirmed the local proxy first-run notice
  proxyConfirmed?: boolean;
  // User has confirmed the usage query first-run notice
  usageConfirmed?: boolean;
  usageDashboardRefreshIntervalMs?: number;
  // Whether to show the failover toggle independently on the main page
  enableFailoverToggle?: boolean;
  // Whether to show the project profile switcher on the main page header
  showProfileSwitcher?: boolean;
  // Preserve Codex ChatGPT login in auth.json when switching third-party providers
  preserveCodexOfficialAuthOnSwitch?: boolean;
  // Run official Codex under the shared "custom" provider id so future
  // sessions share one resume-history bucket with third-party providers
  unifyCodexSessionHistory?: boolean;
  // User opted in (enable dialog checkbox) to migrate existing official sessions
  unifyCodexMigrateExisting?: boolean;
  // User has confirmed the failover toggle first-run notice
  failoverConfirmed?: boolean;
  // User has confirmed the first-run welcome notice
  firstRunNoticeConfirmed?: boolean;
  // User has confirmed the auto-sync traffic warning
  autoSyncConfirmed?: boolean;
  // User has confirmed the common config first-run notice
  commonConfigConfirmed?: boolean;
  // 首选语言（可选，默认中文）
  language?: "en" | "zh" | "zh-TW" | "ja";

  // 主页面显示的应用（默认全部显示）
  visibleApps?: VisibleApps;

  // ===== 设备级目录覆盖 =====
  // 覆盖 Claude Code 配置目录（可选）
  claudeConfigDir?: string;
  // 覆盖 Codex 配置目录（可选）
  codexConfigDir?: string;
  // 覆盖 Gemini 配置目录（可选）
  geminiConfigDir?: string;
  // 覆盖 Grok Build 配置目录（可选）
  grokConfigDir?: string;
  // 覆盖 OpenCode 配置目录（可选）
  opencodeConfigDir?: string;
  // 覆盖 OpenClaw 配置目录（可选）
  openclawConfigDir?: string;
  // 覆盖 Hermes 配置目录（可选）
  hermesConfigDir?: string;

  // ===== 当前供应商 ID（设备级）=====
  // 当前 Claude 供应商 ID（优先于数据库 is_current）
  currentProviderClaude?: string;
  // 当前 Claude Desktop 供应商 ID（优先于数据库 is_current）
  currentProviderClaudeDesktop?: string;
  // 当前 Codex 供应商 ID（优先于数据库 is_current）
  currentProviderCodex?: string;
  // 当前 Gemini 供应商 ID（优先于数据库 is_current）
  currentProviderGemini?: string;

  // ===== Skill 同步设置 =====
  // Skill 同步方式：auto（默认，优先 symlink）、symlink、copy
  skillSyncMethod?: SkillSyncMethod;
  // Skill 存储位置：cc_switch（默认）或 unified（~/.agents/skills/）
  skillStorageLocation?: SkillStorageLocation;

  // ===== WebDAV v2 同步设置 =====
  webdavSync?: WebDavSyncSettings;

  // ===== S3 同步设置 =====
  s3Sync?: S3SyncSettings;

  // ===== 备份策略设置 =====
  // Auto-backup interval in hours (0=disabled, default 24)
  backupIntervalHours?: number;
  // Maximum backup files to retain (default 10)
  backupRetainCount?: number;

  // ===== 终端设置 =====
  // 首选终端应用（可选，默认使用系统默认终端）
  // macOS: "terminal" | "iterm2" | "warp" | "alacritty" | "kitty" | "ghostty" | "wezterm" | "kaku"
  // Windows: "cmd" | "powershell" | "wt"
  // Linux: "gnome-terminal" | "konsole" | "xfce4-terminal" | "alacritty" | "kitty" | "ghostty"
  preferredTerminal?: string;

  // ===== 本机自动迁移状态 =====
  localMigrations?: {
    codexThirdPartyHistoryProviderBucketV1?: {
      completedAt: string;
      targetProviderId: string;
      sourceProviderIds?: string[];
      migratedJsonlFiles?: number;
      migratedStateRows?: number;
    };
  };

  // ===== 系统通知 / 后台健康探测（缺省时使用后端默认值）=====
  notifications?: NotificationToggles;
  healthProbe?: HealthProbeConfig;
}

// 系统通知开关：默认仅开启故障转移与熔断通知
export interface NotificationToggles {
  failover: boolean;
  circuitOpen: boolean;
  budgetExceeded: boolean;
  quotaExhausted: boolean;
  syncError: boolean;
}

// 后台健康探测：默认关闭
export interface HealthProbeConfig {
  enabled: boolean;
  intervalSecs: number;
}
