import type { ProviderFormData } from "@/lib/schemas/provider";
import type { ClaudeApiFormat, ClaudeApiKeyField } from "@/types";
import { CLAUDE_DEFAULT_CONFIG } from "@/components/providers/forms/helpers/opencodeFormUtils";
import {
  coreFormFromProvider,
  formDefaultValues,
  type CoreFormInitialState,
} from "../coreForm";
import type { ProviderFormInitialData } from "../types";

export interface ClaudeFormInitialState extends CoreFormInitialState {
  defaultValues: ProviderFormData;
  apiKeyField: ClaudeApiKeyField;
  apiFormat: ClaudeApiFormat;
  isFullUrl: boolean;
}

/**
 * meta.apiKeyField 优先；没有时按 env 推断：存在 ANTHROPIC_API_KEY 键
 * （即使值为空）就用它，否则用 ANTHROPIC_AUTH_TOKEN。
 */
export const claudeApiKeyFieldFromProvider = (
  initialData?: ProviderFormInitialData,
): ClaudeApiKeyField => {
  if (initialData?.meta?.apiKeyField) return initialData.meta.apiKeyField;
  const env = (initialData?.settingsConfig as Record<string, unknown>)?.env as
    | Record<string, unknown>
    | undefined;
  if (env?.ANTHROPIC_API_KEY !== undefined) return "ANTHROPIC_API_KEY";
  return "ANTHROPIC_AUTH_TOKEN";
};

/** 由已保存的供应商（或新建时的空值）得到 Claude 编辑器的初始状态 */
export const claudeFormFromProvider = (
  initialData?: ProviderFormInitialData,
): ClaudeFormInitialState => ({
  ...coreFormFromProvider(initialData),
  defaultValues: formDefaultValues(initialData, CLAUDE_DEFAULT_CONFIG),
  apiKeyField: claudeApiKeyFieldFromProvider(initialData),
  apiFormat: initialData?.meta?.apiFormat ?? "anthropic",
  isFullUrl: initialData?.meta?.isFullUrl ?? false,
});
