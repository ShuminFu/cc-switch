import { invoke } from "@tauri-apps/api/core";

/**
 * IPC wrappers for the wave-5 workbench features. Each type mirrors a struct
 * in `src-tauri/src/features/*.rs` (camelCase over the wire).
 */

// ─── Adjacent CLI tools (features/custom_tools.rs, PRODUCT-43) ───

export type ToolFileFormat = "env" | "yaml" | "json" | "toml";

export interface ToolField {
  id: string;
  label: string;
  file: string;
  format: ToolFileFormat;
  key: string;
  secret?: boolean;
}

export interface ToolDescriptor {
  id: string;
  name: string;
  homepage?: string | null;
  detectDir?: string | null;
  fields: ToolField[];
  builtin: boolean;
}

export interface ToolProfileView {
  id: string;
  name: string;
  values: Record<string, string>;
  secretsSet: string[];
}

export interface ToolStatus {
  descriptor: ToolDescriptor;
  installed: boolean;
  files: string[];
  live: Record<string, string>;
  liveSecretsSet: string[];
  liveError?: string;
  profiles: ToolProfileView[];
  activeProfile: string | null;
}

export interface CustomToolsOverview {
  tools: ToolStatus[];
  descriptorErrors: { file: string; error: string }[];
  descriptorsDir: string;
}

export interface ToolProfileInput {
  id?: string;
  name: string;
  /** Secret fields left out keep their stored value. */
  values: Record<string, string>;
}

export const customToolsApi = {
  list: () => invoke<CustomToolsOverview>("list_custom_tools"),
  saveProfile: (toolId: string, profile: ToolProfileInput) =>
    invoke<string>("save_custom_tool_profile", { toolId, profile }),
  deleteProfile: (toolId: string, profileId: string) =>
    invoke<void>("delete_custom_tool_profile", { toolId, profileId }),
  activate: (toolId: string, profileId: string) =>
    invoke<number>("activate_custom_tool_profile", { toolId, profileId }),
  importLive: (toolId: string, name: string) =>
    invoke<string>("import_custom_tool_live", { toolId, name }),
};

// ─── Config history (features/config_history.rs, PRODUCT-21) ───

export interface HistorySummary {
  entityType: string;
  app: string;
  entityId: string;
  name: string;
  versions: number;
  lastTs: number;
  deleted: boolean;
}

export interface HistoryChange {
  path: string;
  before?: unknown;
  after?: unknown;
}

export interface HistoryVersion {
  versionId: string;
  ts: number;
  action: string;
  source: string;
  snapshot: unknown | null;
  changes: HistoryChange[];
}

export interface RestoreOutcome {
  recreated: boolean;
  missingSecrets: string[];
}

export const historyApi = {
  list: () => invoke<HistorySummary[]>("list_config_history"),
  provider: (app: string, id: string) =>
    invoke<HistoryVersion[]>("get_provider_history", { app, id }),
  restore: (app: string, id: string, versionId: string) =>
    invoke<RestoreOutcome>("restore_provider_version", { app, id, versionId }),
};

// ─── Audit log (features/audit_log.rs, PRODUCT-25) ───

export interface AuditEvent {
  ts: number;
  actor: string;
  action: string;
  entityType: string;
  entityId: string;
  app?: string;
  beforeHash?: string;
  afterHash?: string;
  detail?: string;
}

export const auditApi = {
  list: (limit: number) => invoke<AuditEvent[]>("list_audit_events", { limit }),
  export: () => invoke<string | null>("export_audit_log"),
};

// ─── Share bundle (features/share_bundle.rs, PRODUCT-22) ───

export interface ExportScope {
  providerApps: string[];
  mcp: boolean;
  promptApps: string[];
  stripSecrets: boolean;
}

export interface ExportSummary {
  path: string | null;
  providers: number;
  mcpServers: number;
  prompts: number;
  secretsStripped: number;
}

export type BundleItemStatus = "new" | "identical" | "conflict";

export interface BundlePreviewItem {
  key: string;
  kind: "provider" | "mcp" | "prompt";
  app?: string;
  id: string;
  name: string;
  status: BundleItemStatus;
  missingSecrets: string[];
}

export interface BundlePreview {
  createdAt: string;
  secretsStripped: boolean;
  items: BundlePreviewItem[];
}

export interface ApplyBundleRequest {
  keys: string[];
  overwrite: boolean;
  secrets: Record<string, Record<string, string>>;
}

export interface ApplyBundleResult {
  added: number;
  updated: number;
  skipped: number;
  errors: string[];
}

export const shareBundleApi = {
  export: (scope: ExportScope) =>
    invoke<ExportSummary>("export_share_bundle", { scope }),
  preview: () => invoke<BundlePreview | null>("preview_share_bundle"),
  apply: (request: ApplyBundleRequest) =>
    invoke<ApplyBundleResult>("apply_share_bundle", { request }),
};

// ─── Usage attribution (features/session_cost.rs, PRODUCT-14) ───

export interface UsageTotals {
  requests: number;
  inputTokens: number;
  outputTokens: number;
  cacheReadTokens: number;
  cacheCreationTokens: number;
  totalCostUsd: number;
}

export interface ProjectUsage extends UsageTotals {
  projectDir: string;
  sessions: number;
}

export interface ProjectUsageReport {
  since: number;
  projects: ProjectUsage[];
  unattributed: UsageTotals;
  total: UsageTotals;
}

export const sessionCostApi = {
  session: (sessionId: string) =>
    invoke<UsageTotals | null>("get_session_usage", { sessionId }),
  byProject: (days: number) =>
    invoke<ProjectUsageReport>("get_usage_by_project", { days }),
};

// ─── Prompt sharing (features/prompt_share.rs, PRODUCT-59) ───

export interface PromptCopyResult {
  app: string;
  status: "created" | "updated" | "unchanged" | "error";
  activated: boolean;
  error?: string;
}

export const promptShareApi = {
  copy: (
    app: string,
    promptId: string,
    targetApps: string[],
    activate: boolean,
  ) =>
    invoke<PromptCopyResult[]>("copy_prompt_to_apps", {
      app,
      promptId,
      targetApps,
      activate,
    }),
};

// ─── Database recovery (features/db_recovery.rs, PRODUCT-72) ───

export interface RecoveryBackup {
  filename: string;
  sizeBytes: number;
  createdAt: string;
  schemaVersion?: number;
  compatible: boolean;
}

export const dbRecoveryApi = {
  list: () => invoke<RecoveryBackup[]>("list_recovery_backups"),
  restore: (filename: string) =>
    invoke<string>("restore_backup_for_recovery", { filename }),
};

export const formatUsd = (value: number) =>
  value >= 100
    ? `$${value.toFixed(0)}`
    : value >= 1
      ? `$${value.toFixed(2)}`
      : `$${value.toFixed(4)}`;

export const formatTokens = (value: number) =>
  value >= 1_000_000
    ? `${(value / 1_000_000).toFixed(1)}M`
    : value >= 1_000
      ? `${(value / 1_000).toFixed(1)}k`
      : String(value);

/** Compact rendering of a history value for the diff list. */
export const formatHistoryValue = (value: unknown): string => {
  if (value === undefined) return "∅";
  if (typeof value === "string") return value;
  return JSON.stringify(value);
};
