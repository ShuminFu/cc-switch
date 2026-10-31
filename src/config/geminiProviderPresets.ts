import type { ProviderCategory } from "@/types";
import { registerPresetCatalog } from "./presetCatalog";
import geminiPresetData from "./presets/gemini.json";
import { geminiPresetSchema, loadPresetTable } from "./presets/schema";

/**
 * Gemini 预设供应商的视觉主题配置
 */
export interface GeminiPresetTheme {
  /** 图标类型：'gemini' | 'generic' */
  icon?: "gemini" | "generic";
  /** 背景色（选中状态），支持 hex 颜色 */
  backgroundColor?: string;
  /** 文字色（选中状态），支持 hex 颜色 */
  textColor?: string;
}

export interface GeminiProviderPreset {
  name: string;
  nameKey?: string; // i18n key for localized display name
  websiteUrl: string;
  apiKeyUrl?: string;
  settingsConfig: object;
  baseURL?: string;
  model?: string;
  description?: string;
  category?: ProviderCategory;
  isPartner?: boolean;
  primePartner?: boolean; // 置顶合作伙伴（顶级）：徽章显示为心形
  partnerPromotionKey?: string;
  endpointCandidates?: string[];
  theme?: GeminiPresetTheme;
  // 图标配置
  icon?: string; // 图标名称
  iconColor?: string; // 图标颜色
}

/**
 * The built-in Gemini presets are data: `presets/gemini.json`, validated by
 * `geminiPresetSchema` when loaded (FE-ARCH-34). Invalid entries are logged
 * and skipped, never fatal.
 */
export const geminiProviderPresets: GeminiProviderPreset[] = loadPresetTable(
  "gemini",
  geminiPresetSchema,
  geminiPresetData,
);

export function getGeminiPresetByName(
  name: string,
): GeminiProviderPreset | undefined {
  return geminiProviderPresets.find((preset) => preset.name === name);
}

export function getGeminiPresetByUrl(
  url: string,
): GeminiProviderPreset | undefined {
  if (!url) return undefined;
  return geminiProviderPresets.find(
    (preset) =>
      preset.baseURL &&
      url.toLowerCase().includes(preset.baseURL.toLowerCase()),
  );
}

// Remote signed catalog overlay (PRODUCT-07); the list above stays the default.
registerPresetCatalog("gemini", geminiProviderPresets);
