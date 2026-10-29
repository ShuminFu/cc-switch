import {
  opencodeProviderPresets,
  type OpenCodeProviderPreset,
} from "@/config/opencodeProviderPresets";

export interface OpenCodePresetEntry {
  id: string;
  preset: OpenCodeProviderPreset;
}

export const opencodePresetEntries = (
  presets: readonly OpenCodeProviderPreset[],
): OpenCodePresetEntry[] =>
  presets.map((preset, index) => ({ id: `opencode-${index}`, preset }));

/**
 * OpenCode 预设表。OpenCodeFormFields（npm 包列表）与 useOmoModelSource
 * （模型变体）也静态引用该模块，按需加载拿不到单独的 chunk，所以这里直接导入。
 */
export const OPENCODE_PRESETS = opencodeProviderPresets;
export const OPENCODE_PRESET_ENTRIES = opencodePresetEntries(OPENCODE_PRESETS);
