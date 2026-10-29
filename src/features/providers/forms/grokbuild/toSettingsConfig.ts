import { buildLocalProxyRequestOverrides } from "@/lib/requestOverrides";
import type {
  ClaudeApiKeyField,
  CodexApiFormat,
  CodexChatReasoning,
  CustomEndpoint,
  PromptCacheRoutingMode,
  ProviderMeta,
} from "@/types";
import {
  parseGrokBuildConfig,
  updateGrokBuildConfig,
  validateGrokBuildConfig,
} from "@/utils/grokBuildConfig";

export interface GrokBuildSaveInput {
  name: string;
  /** 已有供应商的 id；新建时为空，只有新建才写入测速弹窗里添加的端点 */
  providerId?: string;
  initialMeta?: ProviderMeta;
  rawConfig: string;
  profile: string;
  upstreamModel: string;
  baseUrl: string;
  apiKey: string;
  apiBackend: string;
  contextWindow: string;
  apiFormat: CodexApiFormat;
  anthropicAuthField: ClaudeApiKeyField;
  isFullUrl: boolean;
  endpointAutoSelect: boolean;
  isPartner: boolean;
  partnerPromotionKey?: string;
  impersonateClaudeCode: boolean;
  promptCacheRouting: PromptCacheRoutingMode;
  codexChatReasoning: CodexChatReasoning;
  customUserAgent: string;
  headersOverride: string;
  bodyOverride: string;
  maxOutputTokens: string;
  draftCustomEndpoints: string[];
  now?: number;
}

export type GrokBuildSaveError =
  | { key: "providerForm.requiredFields" }
  | { key: "grokBuild.contextWindowInvalid" }
  | { key: "grokBuild.invalidToml"; error: string }
  /** 请求覆写 JSON 不合法：直接显示解析错误 */
  | { key: null; error: string };

export type GrokBuildSaveResult =
  | { ok: true; settingsConfig: string; meta: ProviderMeta }
  | { ok: false; error: GrokBuildSaveError };

/**
 * Grok Build 的 settingsConfig 是 `{ config }`（config.toml 文本）：结构化字段
 * 写回原始 TOML（保留用户手写的其它表），meta 保存代理层转换选项。
 */
export const toGrokBuildSave = (
  input: GrokBuildSaveInput,
): GrokBuildSaveResult => {
  const { name, rawConfig, profile, upstreamModel, baseUrl, apiKey } = input;
  const contextWindow = Number.parseInt(input.contextWindow, 10);
  const envKey = parseGrokBuildConfig(rawConfig).envKey?.trim();
  if (
    !name ||
    !baseUrl.trim() ||
    (!apiKey.trim() && !envKey) ||
    !profile.trim()
  ) {
    return { ok: false, error: { key: "providerForm.requiredFields" } };
  }
  if (!Number.isInteger(contextWindow) || contextWindow <= 0) {
    return { ok: false, error: { key: "grokBuild.contextWindowInvalid" } };
  }

  const finalConfig = updateGrokBuildConfig(rawConfig, {
    model: profile,
    upstreamModel,
    baseUrl,
    name,
    apiKey,
    apiBackend: input.apiBackend,
    contextWindow,
  });
  const configError = validateGrokBuildConfig(finalConfig);
  if (configError) {
    return {
      ok: false,
      error: { key: "grokBuild.invalidToml", error: configError },
    };
  }

  const requestOverrides = buildLocalProxyRequestOverrides(
    input.headersOverride,
    input.bodyOverride,
  );
  if (requestOverrides.error) {
    return { ok: false, error: { key: null, error: requestOverrides.error } };
  }

  const now = input.now ?? Date.now();
  const customEndpoints = Object.fromEntries(
    input.draftCustomEndpoints.map((url): [string, CustomEndpoint] => [
      url,
      { url, addedAt: now, lastUsed: undefined },
    ]),
  );
  const maxOutputTokens = Number.parseInt(input.maxOutputTokens, 10);
  const initialMeta = { ...(input.initialMeta ?? {}) };
  delete initialMeta.custom_endpoints;
  const meta: ProviderMeta = {
    ...initialMeta,
    apiFormat: input.apiFormat,
    apiKeyField: input.anthropicAuthField,
    isFullUrl: input.isFullUrl,
    endpointAutoSelect: input.endpointAutoSelect,
    isPartner: input.isPartner,
    partnerPromotionKey: input.partnerPromotionKey,
    impersonateClaudeCode: input.impersonateClaudeCode,
    promptCacheRouting: input.promptCacheRouting,
    codexChatReasoning: input.codexChatReasoning,
    customUserAgent: input.customUserAgent.trim() || undefined,
    localProxyRequestOverrides: requestOverrides.overrides,
    maxOutputTokens:
      Number.isInteger(maxOutputTokens) && maxOutputTokens > 0
        ? maxOutputTokens
        : undefined,
  };
  if (!input.providerId && Object.keys(customEndpoints).length > 0) {
    meta.custom_endpoints = customEndpoints;
  }

  return {
    ok: true,
    settingsConfig: JSON.stringify({ config: finalConfig }),
    meta,
  };
};
