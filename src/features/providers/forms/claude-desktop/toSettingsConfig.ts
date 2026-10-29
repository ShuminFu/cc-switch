import type {
  ClaudeApiFormat,
  ClaudeDesktopModelRoute,
  ProviderMeta,
} from "@/types";
import type { ProviderFormInitialData } from "../types";
import type {
  ClaudeDesktopApiKeyField,
  ClaudeDesktopMode,
} from "./fromProvider";
import { isClaudeSafeRoute, type RouteRow } from "./routes";

function clonePlainRecord(value: unknown): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    return {};
  }
  return { ...(value as Record<string, unknown>) };
}

export interface ClaudeDesktopSave {
  settingsConfig: string;
  meta: ProviderMeta;
}

/**
 * 官方供应商使用 Claude Desktop 内置 1P 模式，保持空 env 占位；
 * 不写 claudeDesktopMode / claudeDesktopModelRoutes / apiFormat，
 * 与启动 seed 的 OFFICIAL_SEEDS 占位语义一致。
 */
export const toClaudeDesktopOfficialSave = (
  initialData?: ProviderFormInitialData,
): ClaudeDesktopSave => {
  const settingsConfig = clonePlainRecord(initialData?.settingsConfig);
  settingsConfig.env = {};
  const meta: ProviderMeta = { ...(initialData?.meta ?? {}) };
  delete meta.claudeDesktopMode;
  delete meta.claudeDesktopModelRoutes;
  delete meta.apiFormat;
  delete meta.endpointAutoSelect;
  delete meta.isFullUrl;
  return { settingsConfig: JSON.stringify(settingsConfig, null, 2), meta };
};

export interface ClaudeDesktopSaveInput {
  initialData?: ProviderFormInitialData;
  mode: ClaudeDesktopMode;
  apiFormat: ClaudeApiFormat;
  baseUrl: string;
  apiKey: string;
  apiKeyField: ClaudeDesktopApiKeyField;
  /** Copilot / Codex OAuth：不写 API Key，由托管账号认证 */
  usesManagedOAuth: boolean;
  routes: RouteRow[];
  providerType?: string;
  githubAccountId: string | null;
  codexAccountId: string | null;
  codexFastMode: boolean;
}

export type ClaudeDesktopSaveResult =
  | ({ ok: true } & ClaudeDesktopSave)
  | {
      ok: false;
      error:
        | "claudeDesktop.routesRequired"
        | "claudeDesktop.directModelInvalid";
    };

/** 非官方供应商：env 写入端点与 API Key，meta 写入模式与模型路由 */
export const toClaudeDesktopSave = ({
  initialData,
  mode,
  apiFormat,
  baseUrl,
  apiKey,
  apiKeyField,
  usesManagedOAuth,
  routes,
  providerType,
  githubAccountId,
  codexAccountId,
  codexFastMode,
}: ClaudeDesktopSaveInput): ClaudeDesktopSaveResult => {
  const routeEntries = routes
    .map((route) => ({
      ...route,
      route: route.route.trim(),
      model: route.model.trim(),
      labelOverride: route.labelOverride.trim(),
    }))
    .filter((route) => route.route || route.model);

  if (mode === "proxy") {
    // 固定四档（Sonnet / Opus / Fable / Haiku），route_id 由 UI 生成、恒合法，
    // 因此只要求至少填一个实际请求模型；留空档继承第一个已填档（Sonnet 优先），
    // 对齐 Claude Code 的兜底，保证落库四档齐全、子 agent 不会找不到模型。
    const primary = routeEntries.find((route) => route.model);
    if (!primary) {
      return { ok: false, error: "claudeDesktop.routesRequired" };
    }
    for (const route of routeEntries) {
      if (!route.model) {
        route.model = primary.model;
        if (!route.labelOverride) {
          route.labelOverride = primary.labelOverride || primary.model;
        }
        // 回填的是同一个上游模型，1M 能力声明应与 primary 一致，
        // 避免同模型在不同档声明不同 1M（除非该档用户已显式勾选）。
        if (!route.supports1m) {
          route.supports1m = primary.supports1m;
        }
      }
    }
  } else {
    const invalid = routeEntries.find(
      (route) => !route.route || !isClaudeSafeRoute(route.route),
    );
    if (invalid) {
      return { ok: false, error: "claudeDesktop.directModelInvalid" };
    }
  }

  const settingsConfig = clonePlainRecord(initialData?.settingsConfig);
  const env = clonePlainRecord(settingsConfig.env);
  delete env.ANTHROPIC_AUTH_TOKEN;
  delete env.ANTHROPIC_API_KEY;
  const normalizedBaseUrl = baseUrl.trim().replace(/\/+$/, "");
  settingsConfig.env = usesManagedOAuth
    ? { ...env, ANTHROPIC_BASE_URL: normalizedBaseUrl }
    : {
        ...env,
        ANTHROPIC_BASE_URL: normalizedBaseUrl,
        [apiKeyField]: apiKey.trim(),
      };

  const routeMap = routeEntries.reduce<Record<string, ClaudeDesktopModelRoute>>(
    (acc, route) => {
      acc[route.route] = {
        model: mode === "direct" ? route.route : route.model || route.route,
        labelOverride:
          route.labelOverride || (mode === "proxy" ? route.model : undefined),
        supports1m: route.supports1m || undefined,
      };
      return acc;
    },
    {},
  );

  const meta: ProviderMeta = {
    ...(initialData?.meta ?? {}),
    claudeDesktopMode: mode,
    apiFormat: mode === "proxy" ? apiFormat : "anthropic",
  };

  meta.claudeDesktopModelRoutes = routeMap;
  meta.providerType = providerType;
  meta.authBinding =
    providerType === "github_copilot"
      ? {
          source: "managed_account",
          authProvider: "github_copilot",
          accountId: githubAccountId ?? undefined,
        }
      : providerType === "codex_oauth"
        ? {
            source: "managed_account",
            authProvider: "codex_oauth",
            accountId: codexAccountId ?? undefined,
          }
        : undefined;
  meta.codexFastMode =
    providerType === "codex_oauth" ? codexFastMode : undefined;

  delete meta.endpointAutoSelect;
  delete meta.isFullUrl;

  return {
    ok: true,
    settingsConfig: JSON.stringify(settingsConfig, null, 2),
    meta,
  };
};
