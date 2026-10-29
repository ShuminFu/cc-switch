import type { GeminiProviderPreset } from "@/config/geminiProviderPresets";
import { createPresetLoader } from "../lazyPresets";

export interface GeminiPresetEntry {
  id: string;
  preset: GeminiProviderPreset;
}

/** Gemini 预设表，按需加载 */
export const geminiPresetLoader = createPresetLoader(() =>
  import("@/config/geminiProviderPresets").then((m) => m.geminiProviderPresets),
);

export const geminiPresetEntries = (
  presets: readonly GeminiProviderPreset[],
): GeminiPresetEntry[] =>
  presets.map((preset, index) => ({ id: `gemini-${index}`, preset }));
