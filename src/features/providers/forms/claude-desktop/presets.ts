import type { ClaudeDesktopProviderPreset } from "@/config/claudeDesktopProviderPresets";
import { createPresetLoader } from "../lazyPresets";

export interface ClaudeDesktopPresetEntry {
  id: string;
  preset: ClaudeDesktopProviderPreset;
}

/** Claude Desktop 预设表，按需加载 */
export const claudeDesktopPresetLoader = createPresetLoader(() =>
  import("@/config/claudeDesktopProviderPresets").then(
    (m) => m.claudeDesktopProviderPresets,
  ),
);

export const claudeDesktopPresetEntries = (
  presets: readonly ClaudeDesktopProviderPreset[],
): ClaudeDesktopPresetEntry[] =>
  presets.map((preset, index) => ({ id: `claude-desktop-${index}`, preset }));
