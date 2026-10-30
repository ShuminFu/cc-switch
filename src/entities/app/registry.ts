/**
 * 前端应用注册表（单一来源）
 *
 * 所有应用 ID、显示名称与能力（capabilities）都在这里声明，
 * 取代散落在各组件中的 `VALID_APPS` 列表与 `app === "x" || ...` 能力判断链。
 * 图标与配色仍在 `src/config/appConfig.tsx` 中（需要 JSX）。
 *
 * 后端 `get_app_registry` IPC 落地后，`useAppRegistry()` 是唯一需要改为
 * 读取后端数据的位置；其余调用方（`useAppCaps` / `appLabel` / `isAppId`）保持不变。
 */

import type { AppId } from "@/lib/api/types";

export interface AppCaps {
  /** 支持会话管理视图 */
  sessions: boolean;
  /** 支持 Skills */
  skills: boolean;
  /** 支持 MCP 服务器管理 */
  mcp: boolean;
  /** 累加模式：多个供应商同时写入 live 配置（而非切换单一当前供应商） */
  additive: boolean;
  /** 支持本地代理接管 live 配置（Claude Desktop 走独立路由；累加模式应用只接管选中的供应商条目） */
  proxyTakeover: boolean;
  /** 提供配置健康检查（显示健康警告横幅） */
  healthCheck: boolean;
  /** 已加入配置的供应商可指定一个默认（OpenClaw 默认模型 / Hermes 启用） */
  setDefault: boolean;
  /** 「设为默认」即切换当前供应商，显示为「启用 / 使用中」 */
  defaultIsSwitch: boolean;
}

export interface AppDescriptor {
  id: AppId;
  label: string;
  caps: AppCaps;
  /** 会话 / Skills / MCP 等共享功能视图所借用的应用；缺省为自身 */
  featureHost?: AppId;
  /** 旧版持久化值中可能出现的别名（已小写），与后端注册表 `aliases` 一致 */
  aliases?: readonly string[];
}

const caps = (overrides: Partial<AppCaps> = {}): AppCaps => ({
  sessions: true,
  skills: true,
  mcp: true,
  additive: false,
  proxyTakeover: true,
  healthCheck: false,
  setDefault: false,
  defaultIsSwitch: false,
  ...overrides,
});

/** 注册表顺序即界面中的应用顺序 */
export const APP_REGISTRY: readonly AppDescriptor[] = [
  { id: "claude", label: "Claude", caps: caps() },
  {
    id: "claude-desktop",
    label: "Claude Desktop",
    caps: caps({
      sessions: false,
      skills: false,
      mcp: false,
      proxyTakeover: false,
    }),
    featureHost: "claude",
    aliases: ["claude_desktop", "claudedesktop"],
  },
  { id: "codex", label: "Codex", caps: caps() },
  { id: "gemini", label: "Gemini", caps: caps() },
  {
    id: "grokbuild",
    label: "Grok Build",
    caps: caps(),
    aliases: ["grok-build", "grok_build", "grok"],
  },
  {
    id: "opencode",
    label: "OpenCode",
    caps: caps({ additive: true }),
  },
  {
    id: "openclaw",
    label: "OpenClaw",
    caps: caps({
      skills: false,
      mcp: true,
      additive: true,
      healthCheck: true,
      setDefault: true,
    }),
  },
  {
    id: "hermes",
    label: "Hermes",
    caps: caps({
      additive: true,
      setDefault: true,
      defaultIsSwitch: true,
    }),
  },
];

const BY_ID = new Map<string, AppDescriptor>(
  APP_REGISTRY.map((app) => [app.id, app]),
);

const NO_CAPS: AppCaps = {
  sessions: false,
  skills: false,
  mcp: false,
  additive: false,
  proxyTakeover: false,
  healthCheck: false,
  setDefault: false,
  defaultIsSwitch: false,
};

/** 全部应用 ID（按注册表顺序） */
export const ALL_APP_IDS: readonly AppId[] = APP_REGISTRY.map((app) => app.id);

/** 判断任意字符串（如 localStorage 中的值）是否为已知应用 ID */
export function isAppId(value: unknown): value is AppId {
  return typeof value === "string" && BY_ID.has(value);
}

const BY_ALIAS = new Map<string, AppId>(
  APP_REGISTRY.flatMap((app) =>
    (app.aliases ?? []).map((alias) => [alias, app.id] as const),
  ),
);

/**
 * 把持久化的应用标识（localStorage、旧版设置等）经注册表别名归一为规范 AppId；
 * 未知值返回 undefined。
 */
export function resolveAppId(value: unknown): AppId | undefined {
  if (typeof value !== "string") return undefined;
  if (BY_ID.has(value)) return value as AppId;
  const normalized = value.trim().toLowerCase();
  if (BY_ID.has(normalized)) return normalized as AppId;
  return BY_ALIAS.get(normalized);
}

export function getAppDescriptor(id: string): AppDescriptor | undefined {
  return BY_ID.get(id);
}

/** 应用显示名称；未知 ID 原样返回，避免误显示为其他应用 */
export function appLabel(id: string): string {
  return BY_ID.get(id)?.label ?? id;
}

/** 应用能力；未知 ID 视为不具备任何能力 */
export function appCaps(id: string): AppCaps {
  return BY_ID.get(id)?.caps ?? NO_CAPS;
}

/** 共享功能视图（会话 / Skills / MCP 等）应使用的应用 ID */
export function sharedFeatureAppOf(id: AppId): AppId {
  return BY_ID.get(id)?.featureHost ?? id;
}

/** 具备某项能力的应用 ID（按注册表顺序） */
export function appsWithCap(cap: keyof AppCaps): AppId[] {
  return APP_REGISTRY.filter((app) => app.caps[cap]).map((app) => app.id);
}

/**
 * 应用注册表数据源。
 * 目前为静态数据；接入 `get_app_registry` 后在此处改为查询后端。
 */
export function useAppRegistry(): readonly AppDescriptor[] {
  return APP_REGISTRY;
}

/** 读取某个应用的能力 */
export function useAppCaps(id: AppId): AppCaps {
  const registry = useAppRegistry();
  return registry.find((app) => app.id === id)?.caps ?? NO_CAPS;
}
