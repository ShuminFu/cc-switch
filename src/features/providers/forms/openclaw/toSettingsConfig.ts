import type { ProviderFormData } from "@/lib/schemas/provider";
import type { ProviderMeta } from "@/types";
import {
  rebaseOpenClawSuggestedDefaults,
  type OpenClawSuggestedDefaults,
} from "@/config/openclawProviderPresets";
import { additiveProviderMeta } from "../additiveForm";
import type { CoreFormInitialState } from "../coreForm";
import type { ProviderFormInitialData } from "../types";

/** OpenClaw 的 settingsConfig 就是 JSON 编辑器里的文本（字段编辑已同步写回） */
export const toOpenClawSettingsConfig = (values: ProviderFormData): string =>
  values.settingsConfig.trim();

/**
 * 预设建议的默认模型引用写的是预设自己的 key，提交时改成实际的 providerKey。
 * 没选预设（或预设没有建议）时返回 undefined。
 */
export const toOpenClawSuggestedDefaults = (
  suggestedDefaults: OpenClawSuggestedDefaults | undefined,
  providerKey: string | undefined,
): OpenClawSuggestedDefaults | undefined => {
  if (!suggestedDefaults) return undefined;
  return providerKey
    ? rebaseOpenClawSuggestedDefaults(suggestedDefaults, providerKey)
    : suggestedDefaults;
};

/** 保存时的 meta：沿用已有 meta，应用相关字段一律清掉 */
export const toOpenClawProviderMeta = (
  initialData: ProviderFormInitialData | undefined,
  initial: CoreFormInitialState,
): ProviderMeta => additiveProviderMeta(initialData, initial);
