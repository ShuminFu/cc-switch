import type { TFunction } from "i18next";
import type { AppId } from "@/lib/api";
import { isAppId } from "@/entities/app/registry";
import { safeStorageGet } from "@/lib/storage";

/** 主窗口中可切换的视图；"providers" 为首页（供应商列表） */
export type View =
  | "providers"
  | "settings"
  | "prompts"
  | "skills"
  | "skillsDiscovery"
  | "mcp"
  | "agents"
  | "universal"
  | "sessions"
  | "workspace"
  | "openclawEnv"
  | "openclawTools"
  | "openclawAgents"
  | "hermesMemory";

/** 除首页外的视图（顶栏显示返回按钮与标题） */
export type SubView = Exclude<View, "providers">;

export interface ViewTitleContext {
  /** 共享功能（Prompts / Skills / Sessions）所针对的应用 */
  sharedFeatureApp: AppId;
}

type ViewTitle = (t: TFunction, ctx: ViewTitleContext) => string;

/**
 * 子视图标题表。类型为 `Record<SubView, …>`，新增视图时缺少标题会在编译期报错。
 */
export const VIEW_TITLES: Record<SubView, ViewTitle> = {
  settings: (t) => t("settings.title"),
  prompts: (t, { sharedFeatureApp }) =>
    t("prompts.title", { appName: t(`apps.${sharedFeatureApp}`) }),
  skills: (t) => t("skills.title"),
  skillsDiscovery: (t) => t("skills.title"),
  mcp: (t) => t("mcp.unifiedPanel.title"),
  agents: (t) => t("agents.title"),
  universal: (t) => t("universalProvider.title"),
  sessions: (t) => t("sessionManager.title"),
  workspace: (t) => t("workspace.title"),
  openclawEnv: (t) => t("openclaw.env.title"),
  openclawTools: (t) => t("openclaw.tools.title"),
  openclawAgents: (t) => t("openclaw.agents.title"),
  hermesMemory: (t) => t("hermes.memory.title"),
};

export const VALID_VIEWS: readonly View[] = [
  "providers",
  ...(Object.keys(VIEW_TITLES) as SubView[]),
];

export function isView(value: unknown): value is View {
  return (
    typeof value === "string" &&
    (VALID_VIEWS as readonly string[]).includes(value)
  );
}

/** 返回按钮 / Esc 的目标视图：Skills 发现页回到 Skills，其余回到首页 */
export function getBackView(view: View): View {
  return view === "skillsDiscovery" ? "skills" : "providers";
}

export const APP_STORAGE_KEY = "cc-switch-last-app";
export const VIEW_STORAGE_KEY = "cc-switch-last-view";

export const getInitialApp = (): AppId => {
  const saved = safeStorageGet(APP_STORAGE_KEY);
  return isAppId(saved) ? saved : "claude";
};

export const getInitialView = (): View => {
  const saved = safeStorageGet(VIEW_STORAGE_KEY);
  return isView(saved) ? saved : "providers";
};
