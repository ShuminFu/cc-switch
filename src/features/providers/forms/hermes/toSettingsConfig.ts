import type { ProviderFormData } from "@/lib/schemas/provider";
import type { ProviderMeta } from "@/types";
import { additiveProviderMeta } from "../additiveForm";
import type { CoreFormInitialState } from "../coreForm";
import type { ProviderFormInitialData } from "../types";

/** Hermes 的 settingsConfig 就是 JSON 编辑器里的文本（字段编辑已同步写回） */
export const toHermesSettingsConfig = (values: ProviderFormData): string =>
  values.settingsConfig.trim();

/** 保存时的 meta：沿用已有 meta，应用相关字段一律清掉 */
export const toHermesProviderMeta = (
  initialData: ProviderFormInitialData | undefined,
  initial: CoreFormInitialState,
): ProviderMeta => additiveProviderMeta(initialData, initial);
