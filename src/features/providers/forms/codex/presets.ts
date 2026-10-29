import type { CodexProviderPreset } from "@/config/codexProviderPresets";
import { createPresetLoader } from "../lazyPresets";

export interface CodexPresetEntry {
  id: string;
  preset: CodexProviderPreset;
}

/** Codex 预设表（Grok Build 也复用其中的非官方项），按需加载 */
export const codexPresetLoader = createPresetLoader(() =>
  import("@/config/codexProviderPresets").then((m) => m.codexProviderPresets),
);

export const codexPresetEntries = (
  presets: readonly CodexProviderPreset[],
): CodexPresetEntry[] =>
  presets.map((preset, index) => ({ id: `codex-${index}`, preset }));
