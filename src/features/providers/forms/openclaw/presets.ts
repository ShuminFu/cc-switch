import {
  openclawProviderPresets,
  type OpenClawProviderPreset,
} from "@/config/openclawProviderPresets";

export interface OpenClawPresetEntry {
  id: string;
  preset: OpenClawProviderPreset;
}

export const openclawPresetEntries = (
  presets: readonly OpenClawProviderPreset[],
): OpenClawPresetEntry[] =>
  presets.map((preset, index) => ({ id: `openclaw-${index}`, preset }));

/**
 * OpenClaw 预设表。OpenClawFormFields 也静态引用该模块（API 协议列表），
 * 按需加载拿不到单独的 chunk，所以这里直接导入。
 */
export const OPENCLAW_PRESETS = openclawProviderPresets;
export const OPENCLAW_PRESET_ENTRIES = openclawPresetEntries(OPENCLAW_PRESETS);
