import type { ProviderFormData } from "@/lib/schemas/provider";
import { GEMINI_DEFAULT_CONFIG } from "@/components/providers/forms/helpers/opencodeFormUtils";
import {
  coreFormFromProvider,
  formDefaultValues,
  type CoreFormInitialState,
} from "../coreForm";
import type { ProviderFormInitialData } from "../types";

const PRIORITY_ENV_KEYS = [
  "GOOGLE_GEMINI_BASE_URL",
  "GEMINI_API_KEY",
  "GEMINI_MODEL",
];

/**
 * 把 settingsConfig.env 对象转成 .env 文本：已知 key 按固定顺序在前（空值跳过），
 * 其余字符串变量保留在后。
 */
export const geminiEnvToString = (envObj: Record<string, unknown>): string => {
  const lines: string[] = [];
  const addedKeys = new Set<string>();

  for (const key of PRIORITY_ENV_KEYS) {
    if (typeof envObj[key] === "string" && envObj[key]) {
      lines.push(`${key}=${envObj[key]}`);
      addedKeys.add(key);
    }
  }

  for (const [key, value] of Object.entries(envObj)) {
    if (!addedKeys.has(key) && typeof value === "string") {
      lines.push(`${key}=${value}`);
    }
  }

  return lines.join("\n");
};

export interface GeminiFormInitialState extends CoreFormInitialState {
  defaultValues: ProviderFormData;
}

/** 由已保存的供应商（或新建时的空值）得到 Gemini 编辑器的初始状态 */
export const geminiFormFromProvider = (
  initialData?: ProviderFormInitialData,
): GeminiFormInitialState => ({
  ...coreFormFromProvider(initialData),
  defaultValues: formDefaultValues(initialData, GEMINI_DEFAULT_CONFIG),
});
