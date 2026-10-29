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
}

export interface AppDescriptor {
  id: AppId;
  label: string;
  caps: AppCaps;
}

const caps = (overrides: Partial<AppCaps> = {}): AppCaps => ({
  sessions: true,
  skills: true,
  mcp: true,
  additive: false,
  ...overrides,
});

/** 注册表顺序即界面中的应用顺序 */
export const APP_REGISTRY: readonly AppDescriptor[] = [
  { id: "claude", label: "Claude", caps: caps() },
  {
    id: "claude-desktop",
    label: "Claude Desktop",
    caps: caps({ sessions: false, skills: false, mcp: false }),
  },
  { id: "codex", label: "Codex", caps: caps() },
  { id: "gemini", label: "Gemini", caps: caps() },
  { id: "grokbuild", label: "Grok Build", caps: caps() },
  { id: "opencode", label: "OpenCode", caps: caps({ additive: true }) },
  {
    id: "openclaw",
    label: "OpenClaw",
    caps: caps({ skills: false, mcp: false, additive: true }),
  },
  { id: "hermes", label: "Hermes", caps: caps({ additive: true }) },
];

const BY_ID = new Map<string, AppDescriptor>(
  APP_REGISTRY.map((app) => [app.id, app]),
);

const NO_CAPS: AppCaps = {
  sessions: false,
  skills: false,
  mcp: false,
  additive: false,
};

/** 全部应用 ID（按注册表顺序） */
export const ALL_APP_IDS: readonly AppId[] = APP_REGISTRY.map((app) => app.id);

/** 判断任意字符串（如 localStorage 中的值）是否为已知应用 ID */
export function isAppId(value: unknown): value is AppId {
  return typeof value === "string" && BY_ID.has(value);
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
