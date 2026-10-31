import { lazy, type ComponentType } from "react";
import type { TFunction } from "i18next";
import type { AppId } from "@/lib/api/types";
import ProvidersPage from "@/routes/ProvidersPage";
import type { HelpTopic } from "@/lib/helpLinks";
import type { ViewId } from "./navigation";

export interface RouteTitleContext {
  featureApp: AppId;
}

export interface RouteDefinition {
  /** Header title for sub-pages (the root page fills the header itself). */
  title?: (t: TFunction, ctx: RouteTitleContext) => string;
  Component: ComponentType;
  /**
   * Show the active app's live-config health banner on this page (for apps
   * with the `healthBanner` capability).
   */
  appHealthBanner?: boolean;
  /** Manual page opened by the "?" next to the header title (DOCS-16). */
  help?: HelpTopic;
}

/**
 * Route registry (FE-ARCH-02). Adding a page means adding a `ViewId`, a
 * page component under `src/routes/` and an entry here.
 *
 * Every page except the providers list is code-split and loaded on first
 * visit (PERF-15 / FE-ARCH-08), so the settings/usage dashboard (recharts),
 * session manager (flexsearch) and editors do not load at startup.
 */
export const ROUTES: Record<ViewId, RouteDefinition> = {
  providers: {
    Component: ProvidersPage,
    appHealthBanner: true,
  },
  settings: {
    help: "settings",
    title: (t) => t("settings.title"),
    Component: lazy(() => import("@/routes/SettingsRoute")),
  },
  prompts: {
    help: "prompts",
    title: (t, { featureApp }) =>
      t("prompts.title", { appName: t(`apps.${featureApp}`) }),
    Component: lazy(() => import("@/routes/PromptsRoute")),
  },
  skills: {
    help: "skills",
    title: (t) => t("skills.title"),
    Component: lazy(() => import("@/routes/SkillsRoute")),
  },
  skillsDiscovery: {
    help: "skills",
    title: (t) => t("skills.title"),
    Component: lazy(() => import("@/routes/SkillsDiscoveryRoute")),
  },
  mcp: {
    help: "mcp",
    title: (t) => t("mcp.unifiedPanel.title"),
    Component: lazy(() => import("@/routes/McpRoute")),
  },
  agents: {
    title: (t) => t("agents.title"),
    Component: lazy(() => import("@/routes/AgentsRoute")),
  },
  universal: {
    title: (t) => t("universalProvider.title", { defaultValue: "统一供应商" }),
    Component: lazy(() => import("@/routes/UniversalRoute")),
  },
  sessions: {
    help: "sessions",
    title: (t) => t("sessionManager.title"),
    Component: lazy(() => import("@/routes/SessionsRoute")),
    appHealthBanner: true,
  },
  workspace: {
    title: (t) => t("workspace.title"),
    Component: lazy(() => import("@/components/workspace/WorkspaceFilesPanel")),
    appHealthBanner: true,
  },
  openclawEnv: {
    title: (t) => t("openclaw.env.title"),
    Component: lazy(() => import("@/components/openclaw/EnvPanel")),
    appHealthBanner: true,
  },
  openclawTools: {
    title: (t) => t("openclaw.tools.title"),
    Component: lazy(() => import("@/components/openclaw/ToolsPanel")),
    appHealthBanner: true,
  },
  openclawAgents: {
    title: (t) => t("openclaw.agents.title"),
    Component: lazy(() => import("@/components/openclaw/AgentsDefaultsPanel")),
    appHealthBanner: true,
  },
  hermesMemory: {
    title: (t) => t("hermes.memory.title"),
    Component: lazy(() => import("@/components/hermes/HermesMemoryPanel")),
  },
};
