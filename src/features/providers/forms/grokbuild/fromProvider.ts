import type { ProviderFormData } from "@/lib/schemas/provider";
import { formatRequestOverrideObject } from "@/lib/requestOverrides";
import type {
  ClaudeApiKeyField,
  CodexApiFormat,
  CodexChatReasoning,
  PromptCacheRoutingMode,
  ProviderCategory,
} from "@/types";
import {
  buildGrokBuildConfig,
  parseGrokBuildConfig,
} from "@/utils/grokBuildConfig";
import { resolveProviderIcon } from "@/utils/providerIcon";
import type { ProviderFormInitialData } from "../types";

/** Codex 风格的 apiFormat 对应的 Grok Build api_backend */
export const grokApiBackendFromApiFormat = (format: CodexApiFormat): string => {
  if (format === "openai_chat") return "chat_completions";
  if (format === "anthropic") return "messages";
  return "responses";
};

export interface GrokBuildFormInitialState {
  defaultValues: ProviderFormData;
  category: ProviderCategory | undefined;
  isPartner: boolean;
  partnerPromotionKey?: string;
  /** config.toml 里的供应商名；表单名称为空时写回它 */
  configName: string;
  /** [models].default 选中的客户端可见 profile */
  profile: string;
  /** 实际发给上游的模型 */
  upstreamModel: string;
  baseUrl: string;
  apiKey: string;
  apiBackend: string;
  /** 输入框文本；保存时再解析 */
  contextWindow: string;
  rawConfig: string;
  apiFormat: CodexApiFormat;
  anthropicAuthField: ClaudeApiKeyField;
  impersonateClaudeCode: boolean;
  maxOutputTokens: string;
  codexChatReasoning: CodexChatReasoning;
  promptCacheRouting: PromptCacheRoutingMode;
  isFullUrl: boolean;
  customUserAgent: string;
  headersOverride: string;
  bodyOverride: string;
  endpointAutoSelect: boolean;
}

/** 由已保存的供应商（或新建时的默认 config.toml）得到 Grok Build 编辑器的初始状态 */
export const grokBuildFormFromProvider = (
  initialData?: ProviderFormInitialData,
): GrokBuildFormInitialState => {
  const meta = initialData?.meta;
  const initialConfigText =
    typeof initialData?.settingsConfig?.config === "string"
      ? initialData.settingsConfig.config
      : undefined;
  const config = parseGrokBuildConfig(initialConfigText, initialData?.name);
  const rawConfig = initialConfigText ?? buildGrokBuildConfig(config);

  return {
    defaultValues: {
      name: initialData?.name ?? config.name,
      websiteUrl: initialData?.websiteUrl ?? "",
      notes: initialData?.notes ?? "",
      settingsConfig: JSON.stringify({ config: rawConfig }),
      icon:
        resolveProviderIcon(
          "grokbuild",
          initialData?.icon,
          initialData?.iconColor,
        ) ?? "",
      iconColor: initialData?.iconColor ?? "",
    },
    category: initialData?.category ?? "custom",
    isPartner: meta?.isPartner ?? false,
    partnerPromotionKey: meta?.partnerPromotionKey,
    configName: config.name,
    profile: config.model,
    upstreamModel: config.upstreamModel ?? config.model,
    baseUrl: config.baseUrl,
    apiKey: config.apiKey,
    apiBackend: config.apiBackend,
    contextWindow: String(config.contextWindow),
    rawConfig,
    apiFormat:
      (meta?.apiFormat as CodexApiFormat | undefined) ?? "openai_responses",
    anthropicAuthField: meta?.apiKeyField ?? "ANTHROPIC_AUTH_TOKEN",
    impersonateClaudeCode: meta?.impersonateClaudeCode === true,
    maxOutputTokens: meta?.maxOutputTokens ? String(meta.maxOutputTokens) : "",
    codexChatReasoning: meta?.codexChatReasoning ?? {},
    promptCacheRouting: meta?.promptCacheRouting ?? "auto",
    isFullUrl: meta?.isFullUrl ?? false,
    customUserAgent: meta?.customUserAgent ?? "",
    headersOverride: formatRequestOverrideObject(
      meta?.localProxyRequestOverrides?.headers,
    ),
    bodyOverride: formatRequestOverrideObject(
      meta?.localProxyRequestOverrides?.body,
    ),
    endpointAutoSelect: meta?.endpointAutoSelect ?? true,
  };
};
