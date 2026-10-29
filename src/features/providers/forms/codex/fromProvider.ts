import type { ProviderFormData } from "@/lib/schemas/provider";
import type {
  ClaudeApiKeyField,
  CodexApiFormat,
  CodexChatReasoning,
  PromptCacheRoutingMode,
} from "@/types";
import {
  codexApiFormatFromWireApi,
  extractCodexWireApi,
} from "@/utils/providerConfigUtils";
import { CODEX_DEFAULT_CONFIG } from "@/components/providers/forms/helpers/opencodeFormUtils";
import {
  coreFormFromProvider,
  formDefaultValues,
  type CoreFormInitialState,
} from "../coreForm";
import type { ProviderFormInitialData } from "../types";

export interface CodexFormInitialState extends CoreFormInitialState {
  defaultValues: ProviderFormData;
  apiFormat: CodexApiFormat;
  /** Anthropic Messages 上游使用的认证字段（默认 Bearer 形式） */
  anthropicAuthField: ClaudeApiKeyField;
  /** 模拟 Claude Code 客户端：只有显式保存过 true 才开启 */
  impersonateClaudeCode: boolean;
  /** 输出上限覆写；空字符串表示使用默认值，保存时再解析 */
  maxOutputTokens: string;
  codexChatReasoning: CodexChatReasoning;
  promptCacheRouting: PromptCacheRoutingMode;
  isFullUrl: boolean;
}

/** meta.apiFormat 优先；没有时按 config.toml 的 wire_api 推断 */
export const codexApiFormatFromProvider = (
  initialData?: ProviderFormInitialData,
): CodexApiFormat => {
  const apiFormat = initialData?.meta?.apiFormat;
  if (
    apiFormat === "openai_chat" ||
    apiFormat === "anthropic" ||
    apiFormat === "openai_responses"
  ) {
    return apiFormat;
  }
  const config = initialData?.settingsConfig?.config;
  return (
    codexApiFormatFromWireApi(
      extractCodexWireApi(typeof config === "string" ? config : ""),
    ) ?? "openai_responses"
  );
};

/** 由已保存的供应商（或新建时的空值）得到 Codex 编辑器的初始状态 */
export const codexFormFromProvider = (
  initialData?: ProviderFormInitialData,
): CodexFormInitialState => {
  const meta = initialData?.meta;
  return {
    ...coreFormFromProvider(initialData),
    defaultValues: formDefaultValues(initialData, CODEX_DEFAULT_CONFIG),
    apiFormat: codexApiFormatFromProvider(initialData),
    anthropicAuthField:
      meta?.apiKeyField === "ANTHROPIC_API_KEY"
        ? "ANTHROPIC_API_KEY"
        : "ANTHROPIC_AUTH_TOKEN",
    impersonateClaudeCode: meta?.impersonateClaudeCode === true,
    maxOutputTokens:
      typeof meta?.maxOutputTokens === "number" && meta.maxOutputTokens > 0
        ? String(meta.maxOutputTokens)
        : "",
    codexChatReasoning: meta?.codexChatReasoning ?? {},
    promptCacheRouting: meta?.promptCacheRouting ?? "auto",
    isFullUrl: meta?.isFullUrl ?? false,
  };
};
