import type { ProviderPreset } from "@/config/claudeProviderPresets";
import { createPresetLoader } from "../lazyPresets";

export interface ClaudePresetEntry {
  id: string;
  preset: ProviderPreset;
}

/** Claude 预设表（含 hidden 项），按需加载 */
export const claudePresetLoader = createPresetLoader(() =>
  import("@/config/claudeProviderPresets").then((m) => m.providerPresets),
);

/** 预设选择器条目：隐藏项不列出，id 按过滤后的位置编号 */
export const claudePresetEntries = (
  presets: readonly ProviderPreset[],
): ClaudePresetEntry[] =>
  presets
    .filter((preset) => !preset.hidden)
    .map((preset, index) => ({ id: `claude-${index}`, preset }));
