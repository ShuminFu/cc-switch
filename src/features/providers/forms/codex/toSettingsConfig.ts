import type {
  ClaudeApiKeyField,
  CodexApiFormat,
  CodexCatalogModel,
  CodexChatReasoning,
  LocalProxyRequestOverrides,
  PromptCacheRoutingMode,
  ProviderCategory,
} from "@/types";
import {
  extractCodexModelName,
  setCodexModelName,
  setCodexWireApi,
} from "@/utils/providerConfigUtils";
import type { AppProviderMeta } from "../coreForm";
import {
  normalizeCodexCatalogModelsForSave,
  normalizeCodexChatReasoningForSave,
} from "../saveHelpers";

export interface CodexSettingsInput {
  /** 已解析的 auth.json（解析失败时调用方必须先阻止保存） */
  auth: Record<string, unknown>;
  config: string;
  catalogModels: CodexCatalogModel[];
  category: ProviderCategory | undefined;
}

/**
 * Codex 的 settingsConfig：`{ auth, config, modelCatalog? }`。
 * 非官方供应商的 wire_api 固定为 responses（apiFormat 只决定代理层转换）。
 */
export const toCodexSettingsConfig = ({
  auth,
  config,
  catalogModels,
  category,
}: CodexSettingsInput): string => {
  const isOfficial = category === "official";
  let normalizedConfig =
    !isOfficial && config.trim()
      ? setCodexWireApi(config, "responses")
      : config;
  // 模型映射与「路由接管」解耦：对所有非官方供应商，填了就持久化
  //（Chat 生成兼容路由、原生 Responses 生成 model-catalogs.json），
  // 留空归一化为 [] 即不写。后端只看 modelCatalog.models 是否非空。
  const models = isOfficial
    ? []
    : normalizeCodexCatalogModelsForSave(catalogModels);
  // The default-model field writes the top-level `model` into the TOML
  // as the user types; only when it was left empty fall back to the
  // first catalog row so "fill mapping only" keeps its old behavior.
  if (models.length > 0 && !extractCodexModelName(normalizedConfig)) {
    normalizedConfig = setCodexModelName(normalizedConfig, models[0].model);
  }
  const settings: {
    auth: unknown;
    config: string;
    modelCatalog?: { models: CodexCatalogModel[] };
  } = { auth, config: normalizedConfig };
  if (models.length > 0) {
    settings.modelCatalog = { models };
  }
  return JSON.stringify(settings);
};

/** 本地代理的请求覆写与 User-Agent 只对非官方供应商生效 */
export const codexAppliesLocalProxyOverrides = (
  category: ProviderCategory | undefined,
): boolean => category !== "official";

export interface CodexMetaInput {
  category: ProviderCategory | undefined;
  commonConfigEnabled: boolean;
  apiFormat: CodexApiFormat;
  anthropicAuthField: ClaudeApiKeyField;
  impersonateClaudeCode: boolean;
  maxOutputTokens: string;
  codexChatReasoning: CodexChatReasoning;
  promptCacheRouting: PromptCacheRoutingMode;
  isFullUrl: boolean;
  customUserAgent: string;
  localProxyRequestOverrides?: LocalProxyRequestOverrides;
}

/**
 * Codex 管理的 meta 字段。Chat 转换相关字段只在 openai_chat 下保存，
 * Anthropic 上游相关字段只在 anthropic 下保存；官方供应商都不保存。
 */
export const toCodexAppMeta = ({
  category,
  commonConfigEnabled,
  apiFormat,
  anthropicAuthField,
  impersonateClaudeCode,
  maxOutputTokens,
  codexChatReasoning,
  promptCacheRouting,
  isFullUrl,
  customUserAgent,
  localProxyRequestOverrides,
}: CodexMetaInput): AppProviderMeta => {
  const isOfficial = category === "official";
  const isChat = !isOfficial && apiFormat === "openai_chat";
  const isAnthropic = !isOfficial && apiFormat === "anthropic";
  return {
    commonConfigEnabled,
    codexChatReasoning: isChat
      ? normalizeCodexChatReasoningForSave(codexChatReasoning)
      : undefined,
    promptCacheRouting:
      isChat && promptCacheRouting !== "auto" ? promptCacheRouting : undefined,
    customUserAgent: isOfficial
      ? undefined
      : customUserAgent.trim() || undefined,
    localProxyRequestOverrides: codexAppliesLocalProxyOverrides(category)
      ? localProxyRequestOverrides
      : undefined,
    apiFormat: isOfficial ? undefined : apiFormat,
    apiKeyField:
      isAnthropic && anthropicAuthField !== "ANTHROPIC_AUTH_TOKEN"
        ? anthropicAuthField
        : undefined,
    // Off by default; persist true only for codex+anthropic when the user explicitly enables it
    impersonateClaudeCode:
      isAnthropic && impersonateClaudeCode ? true : undefined,
    // Persist only for codex+anthropic when a positive value was entered
    maxOutputTokens:
      isAnthropic &&
      maxOutputTokens.trim() !== "" &&
      Number(maxOutputTokens) > 0
        ? Number(maxOutputTokens)
        : undefined,
    isFullUrl: !isOfficial && isFullUrl ? true : undefined,
  };
};
