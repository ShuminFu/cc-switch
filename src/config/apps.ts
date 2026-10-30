import type { AppId } from "@/lib/api/types";

/**
 * Single registry of the apps CC Switch manages (FE-ARCH-10).
 *
 * Every app list, label map and capability check in the UI should derive from
 * this file instead of repeating `appId === "..."` comparisons. The module is
 * plain data (no React, no icons) so it can be imported from the query layer,
 * hooks and tests without pulling UI code into their dependency graph. The
 * visual parts (icons, colour classes) live in `appConfig.tsx`, keyed by the
 * same `AppId`.
 *
 * Adding an app: add it to `AppId`, add an entry here (TypeScript enforces
 * one entry per id), add its visuals to `APP_ICON_MAP`, its presets and the
 * Rust support.
 */

/** Feature shortcuts shown in the providers-page toolbar, in display order. */
export type AppToolbarFeature =
  | "skills"
  | "prompts"
  | "sessions"
  | "mcp"
  | "workspace"
  | "openclawEnv"
  | "openclawTools"
  | "openclawAgents"
  | "hermesMemory"
  | "hermesWebUI";

export interface AppCapabilities {
  /** The local proxy can take this app over (per-app proxy toggle). */
  proxy: boolean;
  /** Failover queue / auto-failover toggle. */
  failover: boolean;
  /** Claude Desktop's own gateway routing toggle replaces the proxy toggle. */
  desktopRoute: boolean;
  /** Listed as a target in the unified MCP panel. */
  mcp: boolean;
  /** Listed as a target in the unified Skills panel. */
  skills: boolean;
  /** Prompt (system prompt file) management. */
  prompts: boolean;
  /** Session manager. */
  sessions: boolean;
  /** Universal (cross-app) providers can sync into this app. */
  universal: boolean;
  /**
   * Additive live config: several providers can be written to the live config
   * at once (OpenCode/OpenClaw/Hermes), so providers can be removed from the
   * live config without being deleted, and duplicated providers need a fresh
   * provider key.
   */
  additiveLiveConfig: boolean;
  /** "Open terminal with this provider" card action. */
  terminal: boolean;
  /** Oh-My-OpenCode (OMO / OMO Slim) integration. */
  omo: boolean;
  /** Health warnings banner for the live config. */
  healthBanner: boolean;
  /** Proxy/failover toggles and profile switcher area in the header. */
  headerProxyControls: boolean;
}

export interface AppDefinition {
  id: AppId;
  /** i18n key for the app name (`apps.<id>`). */
  labelKey: string;
  /** Untranslated product name, used in toasts and as i18n default. */
  label: string;
  /** Name shown in the app switcher (Claude is "Claude Code" there). */
  switcherLabel: string;
  /** `ProviderIcon` icon name. */
  iconName: string;
  /**
   * App whose shared features (skills, prompts, MCP, sessions) are shown for
   * this app. Claude Desktop shares Claude's.
   */
  featureApp: AppId;
  /**
   * App used as the default target in the Skills panels when this app itself
   * is not a skills target (OpenClaw falls back to Claude).
   */
  skillsTargetApp: AppId;
  /**
   * What "set as default" means on a provider card: `"model"` sets the
   * default model (OpenClaw), `"switch"` switches the provider (Hermes).
   */
  setAsDefault: "model" | "switch" | null;
  /** Feature shortcuts in the providers-page toolbar. */
  toolbar: readonly AppToolbarFeature[];
  capabilities: AppCapabilities;
}

const NO_CAPABILITIES: AppCapabilities = {
  proxy: false,
  failover: false,
  desktopRoute: false,
  mcp: false,
  skills: false,
  prompts: false,
  sessions: false,
  universal: false,
  additiveLiveConfig: false,
  terminal: false,
  omo: false,
  healthBanner: false,
  headerProxyControls: false,
};

const DEFAULT_TOOLBAR: readonly AppToolbarFeature[] = [
  "skills",
  "prompts",
  "sessions",
  "mcp",
];

const defineApp = (
  id: AppId,
  def: Omit<
    AppDefinition,
    | "id"
    | "labelKey"
    | "capabilities"
    | "featureApp"
    | "skillsTargetApp"
    | "setAsDefault"
    | "toolbar"
    | "switcherLabel"
  > &
    Partial<
      Pick<
        AppDefinition,
        | "featureApp"
        | "skillsTargetApp"
        | "setAsDefault"
        | "toolbar"
        | "switcherLabel"
      >
    > & { capabilities: Partial<AppCapabilities> },
): AppDefinition => ({
  id,
  labelKey: `apps.${id}`,
  switcherLabel: def.switcherLabel ?? def.label,
  featureApp: def.featureApp ?? id,
  skillsTargetApp: def.skillsTargetApp ?? def.featureApp ?? id,
  setAsDefault: def.setAsDefault ?? null,
  toolbar: def.toolbar ?? DEFAULT_TOOLBAR,
  label: def.label,
  iconName: def.iconName,
  capabilities: { ...NO_CAPABILITIES, ...def.capabilities },
});

/** Shared-feature capabilities of the Claude Code family. */
const CLAUDE_FEATURES: Partial<AppCapabilities> = {
  mcp: true,
  skills: true,
  prompts: true,
  sessions: true,
};

/**
 * The registry, in app-switcher order. TypeScript requires exactly one entry
 * per `AppId`.
 */
export const APP_REGISTRY: Readonly<Record<AppId, AppDefinition>> = {
  claude: defineApp("claude", {
    label: "Claude",
    switcherLabel: "Claude Code",
    iconName: "claude",
    capabilities: {
      ...CLAUDE_FEATURES,
      proxy: true,
      failover: true,
      universal: true,
      terminal: true,
      headerProxyControls: true,
    },
  }),
  "claude-desktop": defineApp("claude-desktop", {
    label: "Claude Desktop",
    iconName: "claude",
    featureApp: "claude",
    capabilities: {
      // Claude Desktop shares Claude's MCP/skills/prompts/sessions pages via
      // `featureApp`, but is not itself a target in those panels.
      desktopRoute: true,
      headerProxyControls: true,
    },
  }),
  codex: defineApp("codex", {
    label: "Codex",
    iconName: "openai",
    capabilities: {
      ...CLAUDE_FEATURES,
      proxy: true,
      failover: true,
      universal: true,
      headerProxyControls: true,
    },
  }),
  gemini: defineApp("gemini", {
    label: "Gemini",
    iconName: "gemini",
    capabilities: {
      ...CLAUDE_FEATURES,
      proxy: true,
      failover: true,
      universal: true,
      headerProxyControls: true,
    },
  }),
  grokbuild: defineApp("grokbuild", {
    label: "Grok Build",
    iconName: "grok",
    capabilities: {
      ...CLAUDE_FEATURES,
      proxy: true,
      failover: true,
      headerProxyControls: true,
    },
  }),
  opencode: defineApp("opencode", {
    label: "OpenCode",
    iconName: "opencode",
    capabilities: {
      ...CLAUDE_FEATURES,
      additiveLiveConfig: true,
      omo: true,
    },
  }),
  openclaw: defineApp("openclaw", {
    label: "OpenClaw",
    iconName: "openclaw",
    skillsTargetApp: "claude",
    setAsDefault: "model",
    toolbar: [
      "workspace",
      "openclawEnv",
      "openclawTools",
      "openclawAgents",
      "sessions",
    ],
    capabilities: {
      sessions: true,
      additiveLiveConfig: true,
      healthBanner: true,
    },
  }),
  hermes: defineApp("hermes", {
    label: "Hermes",
    iconName: "hermes",
    setAsDefault: "switch",
    toolbar: ["skills", "hermesMemory", "hermesWebUI", "mcp"],
    capabilities: {
      ...CLAUDE_FEATURES,
      additiveLiveConfig: true,
    },
  }),
};

/** All app ids in display order. */
export const ALL_APP_IDS: readonly AppId[] = Object.keys(
  APP_REGISTRY,
) as AppId[];

export const isAppId = (value: unknown): value is AppId =>
  typeof value === "string" &&
  Object.prototype.hasOwnProperty.call(APP_REGISTRY, value);

export const getAppDefinition = (appId: AppId): AppDefinition =>
  APP_REGISTRY[appId];

export const appHas = (appId: AppId, capability: keyof AppCapabilities) =>
  APP_REGISTRY[appId]?.capabilities[capability] === true;

/** App ids that have the given capability, in display order. */
export const appsWith = (capability: keyof AppCapabilities): AppId[] =>
  ALL_APP_IDS.filter((id) => APP_REGISTRY[id].capabilities[capability]);

/** Default `visibleApps` setting: every app visible. */
export const defaultVisibleApps = (): Record<AppId, boolean> =>
  Object.fromEntries(ALL_APP_IDS.map((id) => [id, true])) as Record<
    AppId,
    boolean
  >;

/**
 * Whether an app is visible. A missing `visibleApps` setting means every app
 * is visible; a missing key inside it means hidden (matches the backend).
 */
export const isAppVisible = (
  visibleApps: Partial<Record<AppId, boolean>> | undefined,
  appId: AppId,
): boolean => (visibleApps ? visibleApps[appId] === true : true);

/** First app the user has not hidden, or Claude when everything is hidden. */
export const getFirstVisibleApp = (
  visibleApps: Partial<Record<AppId, boolean>> | undefined,
): AppId => ALL_APP_IDS.find((id) => isAppVisible(visibleApps, id)) ?? "claude";
