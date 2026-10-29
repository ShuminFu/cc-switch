import type { ProviderFormData } from "@/lib/schemas/provider";
import type { ClaudeApiFormat } from "@/types";
import { resolveManagedAccountId } from "@/lib/authBinding";
import type { ProviderFormInitialData } from "../types";
import { initialRouteRows, normalizeProxyRows, type RouteRow } from "./routes";

export type ClaudeDesktopApiKeyField =
  | "ANTHROPIC_AUTH_TOKEN"
  | "ANTHROPIC_API_KEY";
export type ClaudeDesktopMode = "direct" | "proxy";

/** settingsConfig.env 中的字符串值；缺失或不是字符串时为 "" */
export function envString(
  settingsConfig: Record<string, unknown> | undefined,
  key: string,
) {
  const env = settingsConfig?.env;
  if (!env || typeof env !== "object") return "";
  const value = (env as Record<string, unknown>)[key];
  return typeof value === "string" ? value : "";
}

export interface ClaudeDesktopFormInitialState {
  defaultValues: ProviderFormData;
  mode: ClaudeDesktopMode;
  apiFormat: ClaudeApiFormat;
  baseUrl: string;
  apiKey: string;
  apiKeyField: ClaudeDesktopApiKeyField;
  githubAccountId: string | null;
  codexAccountId: string | null;
  codexFastMode: boolean;
  routes: RouteRow[];
  /** 已保存过路由时不再用后端默认路由回填 */
  hasSavedRoutes: boolean;
}

/** 由已保存的供应商（或新建时的空值）得到 Claude Desktop 编辑器的初始状态 */
export const claudeDesktopFormFromProvider = (
  initialData?: ProviderFormInitialData,
): ClaudeDesktopFormInitialState => {
  const settingsConfig = initialData?.settingsConfig;
  const meta = initialData?.meta;
  const mode = meta?.claudeDesktopMode ?? "direct";
  const rows = initialRouteRows(meta?.claudeDesktopModelRoutes);
  return {
    defaultValues: {
      name: initialData?.name ?? "",
      websiteUrl: initialData?.websiteUrl ?? "",
      notes: initialData?.notes ?? "",
      settingsConfig: JSON.stringify(settingsConfig ?? { env: {} }, null, 2),
      icon: initialData?.icon ?? "",
      iconColor: initialData?.iconColor ?? "",
    },
    mode,
    apiFormat: meta?.apiFormat ?? "anthropic",
    baseUrl: envString(settingsConfig, "ANTHROPIC_BASE_URL"),
    apiKey:
      envString(settingsConfig, "ANTHROPIC_AUTH_TOKEN") ||
      envString(settingsConfig, "ANTHROPIC_API_KEY"),
    apiKeyField: envString(settingsConfig, "ANTHROPIC_API_KEY")
      ? "ANTHROPIC_API_KEY"
      : "ANTHROPIC_AUTH_TOKEN",
    githubAccountId: resolveManagedAccountId(meta, "github_copilot"),
    codexAccountId: resolveManagedAccountId(meta, "codex_oauth"),
    codexFastMode: meta?.codexFastMode ?? false,
    // proxy 模式归一化成固定四档；但初始无任何 route 时保持空数组，交给 seed
    // effect 用默认路由回填（默认 1M 声明、ANTHROPIC_MODEL 预填），避免过早
    // normalize 成空四档把 routes.length 撑到 4、永久挡住 seed。
    routes:
      mode === "proxy" && rows.length > 0 ? normalizeProxyRows(rows) : rows,
    hasSavedRoutes:
      Object.keys(meta?.claudeDesktopModelRoutes ?? {}).length > 0,
  };
};
