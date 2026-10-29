import type { CodexProviderPreset } from "@/config/codexProviderPresets";
import { codexPresetLoader } from "../codex/presets";

export interface GrokBuildPresetEntry {
  id: string;
  preset: CodexProviderPreset;
}

/** Grok Build 复用 Codex 预设表，与 Codex 共用同一个按需加载的模块 */
export const grokBuildPresetLoader = codexPresetLoader;

/** 只列出非官方的 Codex 预设；id 按 Codex 预设表中的位置编号 */
export const grokBuildPresetEntries = (
  presets: readonly CodexProviderPreset[],
): GrokBuildPresetEntry[] =>
  presets
    .map((preset, index) => ({ id: `grokbuild-${index}`, preset }))
    .filter(
      ({ preset }) => preset.category !== "official" && !preset.isOfficial,
    );
