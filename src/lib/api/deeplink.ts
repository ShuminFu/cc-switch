import { invoke } from "./ipc";

export type ResourceType = "provider" | "prompt" | "mcp" | "skill";

export interface DeepLinkImportRequest {
  version: string;
  resource: ResourceType;

  // Common fields
  app?: "claude" | "codex" | "gemini";
  name?: string;
  enabled?: boolean;

  // Provider fields
  homepage?: string;
  endpoint?: string;
  apiKey?: string;
  icon?: string;
  model?: string;
  notes?: string;
  haikuModel?: string;
  sonnetModel?: string;
  opusModel?: string;

  // Prompt fields
  content?: string;
  description?: string;

  // MCP fields
  apps?: string; // "claude,codex,gemini"

  // Skill fields
  repo?: string;
  directory?: string;
  branch?: string;

  // Config file fields
  config?: string;
  configFormat?: string;
  configUrl?: string;

  // Usage script fields (v3.9+)
  usageEnabled?: boolean;
  usageScript?: string;
  usageApiKey?: string;
  usageBaseUrl?: string;
  usageAccessToken?: string;
  usageUserId?: string;
  usageAutoInterval?: number;
}

/** Item the user must explicitly acknowledge before importing */
export type DeepLinkPreviewRisk = "unlistedEnv" | "mcpCommand" | "usageScript";

export interface DeepLinkPreviewMcpServer {
  id: string;
  command?: string;
  args: string[];
  url?: string;
  /** Full server spec carried by the link */
  spec: Record<string, unknown>;
  /** Already installed: only app flags are merged, this spec is not written */
  exists: boolean;
}

export interface DeepLinkPreviewFieldChange {
  field: string;
  before?: string;
  after?: string;
}

/**
 * Structured import preview built by the backend. Values are unmasked; the
 * dialog masks secrets before display.
 */
export interface DeepLinkImportPreview {
  /** Request with its inline config merged (as merge_deeplink_config) */
  request: DeepLinkImportRequest;
  unlistedEnvKeys: string[];
  deniedEnvKeys: string[];
  mcpServers: DeepLinkPreviewMcpServer[];
  usageScript?: { code: string; targetHosts: string[] };
  liveFiles: string[];
  writesLiveOnImport: boolean;
  existing?: {
    id: string;
    name: string;
    isCurrent: boolean;
    changes: DeepLinkPreviewFieldChange[];
  };
  risks: DeepLinkPreviewRisk[];
}

export interface McpImportResult {
  importedCount: number;
  importedIds: string[];
  failed: Array<{
    id: string;
    error: string;
  }>;
}

export type ImportResult =
  | { type: "provider"; id: string }
  | { type: "prompt"; id: string }
  | {
      type: "mcp";
      importedCount: number;
      importedIds: string[];
      failed: Array<{ id: string; error: string }>;
    }
  | { type: "skill"; key: string };

export const deeplinkApi = {
  /**
   * Merge configuration from Base64/URL into a deep link request
   * This is used to show the complete configuration in the confirmation dialog
   * @param request The deep link import request
   * @returns Merged deep link request with config fields populated
   */
  mergeDeeplinkConfig: async (
    request: DeepLinkImportRequest,
  ): Promise<DeepLinkImportRequest> => {
    return invoke("merge_deeplink_config", { request });
  },

  /**
   * Build the confirm-dialog preview: merged request, flagged env keys, MCP
   * commands, usage-script source and hosts, affected live files and a diff
   * against an existing provider with the same name.
   */
  previewDeeplinkImport: async (
    request: DeepLinkImportRequest,
  ): Promise<DeepLinkImportPreview> => {
    return invoke("preview_deeplink_import", { request });
  },

  /**
   * Import a resource from a deep link request (unified handler)
   * @param request The deep link import request
   * @returns Import result based on resource type
   */
  importFromDeeplink: async (
    request: DeepLinkImportRequest,
  ): Promise<ImportResult> => {
    return invoke("import_from_deeplink_unified", { request });
  },
};
