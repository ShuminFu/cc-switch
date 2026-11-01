import type { AppId } from "@/lib/api";
import { providersApi } from "@/lib/api";
import type { VisibleApps } from "@/types";

/** User manual entry point, linked from the last wizard step. */
export const USER_MANUAL_URL =
  "https://github.com/farion1231/cc-switch/blob/main/docs/user-manual/README.md";

export const WIZARD_STEPS = ["welcome", "tools", "providers", "done"] as const;
export type WizardStep = (typeof WIZARD_STEPS)[number];

export interface OnboardingApp {
  id: AppId & keyof VisibleApps;
  /** Name accepted by `get_tool_versions`, or null when the app is not a CLI we can probe. */
  tool: string | null;
}

/** Order matches the app switcher. */
export const ONBOARDING_APPS: ReadonlyArray<OnboardingApp> = [
  { id: "claude", tool: "claude" },
  { id: "claude-desktop", tool: null },
  { id: "codex", tool: "codex" },
  { id: "gemini", tool: "gemini" },
  { id: "grokbuild", tool: "grok" },
  { id: "opencode", tool: "opencode" },
  { id: "openclaw", tool: "openclaw" },
  { id: "hermes", tool: "hermes" },
];

export const DEFAULT_VISIBLE_APPS: VisibleApps = {
  claude: true,
  "claude-desktop": true,
  codex: true,
  gemini: true,
  grokbuild: true,
  opencode: true,
  openclaw: true,
  hermes: false,
};

export interface ToolProbe {
  name: string;
  version: string | null;
  installed_but_broken?: boolean;
}

/** Tool names (as probed) that are installed, including broken installs. */
export function detectedToolNames(
  probes: ReadonlyArray<ToolProbe>,
): Set<string> {
  return new Set(
    probes
      .filter((probe) => Boolean(probe.version) || probe.installed_but_broken)
      .map((probe) => probe.name),
  );
}

/**
 * Initial selection for the "tools you use" step.
 * Detected CLIs are pre-selected; apps we cannot probe keep their current
 * visibility. When nothing is detected, the current visibility is kept as is.
 */
export function initialSelection(
  current: VisibleApps | undefined,
  detected: Set<string> | null,
): VisibleApps {
  const base = { ...DEFAULT_VISIBLE_APPS, ...(current ?? {}) };
  if (!detected || detected.size === 0) return base;
  const next = { ...base };
  for (const app of ONBOARDING_APPS) {
    if (app.tool) next[app.id] = detected.has(app.tool);
  }
  if (!ONBOARDING_APPS.some((app) => next[app.id])) return base;
  return next;
}

export function selectedApps(selection: VisibleApps): OnboardingApp[] {
  return ONBOARDING_APPS.filter((app) => selection[app.id]);
}

/** Import the app's existing live config as a provider; resolves to whether anything was imported. */
export async function importLiveConfig(appId: AppId): Promise<boolean> {
  switch (appId) {
    case "opencode":
      return (await providersApi.importOpenCodeFromLive()) > 0;
    case "openclaw":
      return (await providersApi.importOpenClawFromLive()) > 0;
    case "hermes":
      return (await providersApi.importHermesFromLive()) > 0;
    case "claude-desktop":
      return (await providersApi.importClaudeDesktopFromClaude()) > 0;
    default:
      return providersApi.importDefault(appId);
  }
}
