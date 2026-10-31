import type { AppId } from "@/lib/api/types";
import { ALL_APP_IDS, APP_REGISTRY, appsWith } from "./apps";

/**
 * Which glyph represents an app. Config stores data only; `<AppIcon>`
 * (`src/components/AppIcon.tsx`) renders it (FE-ARCH-29).
 */
export type AppIconSpec =
  | { kind: "brand"; brand: "claude" | "codex" | "gemini" | "openclaw" }
  | { kind: "provider"; icon: string; name: string };

export interface AppConfig {
  label: string;
  icon: AppIconSpec;
  activeClass: string;
  badgeClass: string;
}

/** All app ids in display order (derived from `APP_REGISTRY`). */
export const APP_IDS: AppId[] = [...ALL_APP_IDS];

/** App IDs shown in Skills panels (excludes OpenClaw — it doesn't support Skills) */
export const SKILLS_APP_IDS: AppId[] = appsWith("skills");

/** App IDs shown in MCP panels (excludes OpenClaw) */
export const MCP_APP_IDS: AppId[] = appsWith("mcp");

export const APP_ICON_MAP: Record<AppId, AppConfig> = {
  claude: {
    label: APP_REGISTRY["claude"].label,
    icon: { kind: "brand", brand: "claude" },
    activeClass:
      "bg-orange-500/10 ring-1 ring-orange-500/20 hover:bg-orange-500/20 text-orange-600 dark:text-orange-400",
    badgeClass:
      "bg-orange-500/10 text-orange-700 dark:text-orange-300 hover:bg-orange-500/20 border-0 gap-1.5",
  },
  "claude-desktop": {
    label: APP_REGISTRY["claude-desktop"].label,
    icon: { kind: "brand", brand: "claude" },
    activeClass:
      "bg-amber-500/10 ring-1 ring-amber-500/20 hover:bg-amber-500/20 text-amber-700 dark:text-amber-300",
    badgeClass:
      "bg-amber-500/10 text-amber-700 dark:text-amber-300 hover:bg-amber-500/20 border-0 gap-1.5",
  },
  codex: {
    label: APP_REGISTRY["codex"].label,
    icon: { kind: "brand", brand: "codex" },
    activeClass:
      "bg-green-500/10 ring-1 ring-green-500/20 hover:bg-green-500/20 text-green-600 dark:text-green-400",
    badgeClass:
      "bg-green-500/10 text-green-700 dark:text-green-300 hover:bg-green-500/20 border-0 gap-1.5",
  },
  gemini: {
    label: APP_REGISTRY["gemini"].label,
    icon: { kind: "brand", brand: "gemini" },
    activeClass:
      "bg-blue-500/10 ring-1 ring-blue-500/20 hover:bg-blue-500/20 text-blue-600 dark:text-blue-400",
    badgeClass:
      "bg-blue-500/10 text-blue-700 dark:text-blue-300 hover:bg-blue-500/20 border-0 gap-1.5",
  },
  grokbuild: {
    label: APP_REGISTRY["grokbuild"].label,
    icon: { kind: "provider", icon: "grok", name: "Grok Build" },
    activeClass:
      "bg-cyan-500/10 ring-1 ring-cyan-500/20 hover:bg-cyan-500/20 text-cyan-700 dark:text-cyan-300",
    badgeClass:
      "bg-cyan-500/10 text-cyan-700 dark:text-cyan-300 hover:bg-cyan-500/20 border-0 gap-1.5",
  },
  opencode: {
    label: APP_REGISTRY["opencode"].label,
    icon: { kind: "provider", icon: "opencode", name: "OpenCode" },
    activeClass:
      "bg-indigo-500/10 ring-1 ring-indigo-500/20 hover:bg-indigo-500/20 text-indigo-600 dark:text-indigo-400",
    badgeClass:
      "bg-indigo-500/10 text-indigo-700 dark:text-indigo-300 hover:bg-indigo-500/20 border-0 gap-1.5",
  },
  openclaw: {
    label: APP_REGISTRY["openclaw"].label,
    icon: { kind: "brand", brand: "openclaw" },
    activeClass:
      "bg-rose-500/10 ring-1 ring-rose-500/20 hover:bg-rose-500/20 text-rose-600 dark:text-rose-400",
    badgeClass:
      "bg-rose-500/10 text-rose-700 dark:text-rose-300 hover:bg-rose-500/20 border-0 gap-1.5",
  },
  hermes: {
    label: APP_REGISTRY["hermes"].label,
    icon: { kind: "provider", icon: "hermes", name: "Hermes" },
    activeClass:
      "bg-violet-500/10 ring-1 ring-violet-500/20 hover:bg-violet-500/20 text-violet-600 dark:text-violet-400",
    badgeClass:
      "bg-violet-500/10 text-violet-700 dark:text-violet-300 hover:bg-violet-500/20 border-0 gap-1.5",
  },
};
