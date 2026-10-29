import {
  hermesProviderPresets,
  type HermesProviderPreset,
} from "@/config/hermesProviderPresets";

export interface HermesPresetEntry {
  id: string;
  preset: HermesProviderPreset;
}

export const hermesPresetEntries = (
  presets: readonly HermesProviderPreset[],
): HermesPresetEntry[] =>
  presets.map((preset, index) => ({ id: `hermes-${index}`, preset }));

/**
 * Hermes 预设表。HermesFormFields 与 useHermesFormState 也静态引用该模块
 * （API 模式、默认值），按需加载拿不到单独的 chunk，所以这里直接导入。
 */
export const HERMES_PRESETS = hermesProviderPresets;
export const HERMES_PRESET_ENTRIES = hermesPresetEntries(HERMES_PRESETS);
