import type { ProviderFormData } from "@/lib/schemas/provider";
import type {
  ClaudeApiFormat,
  ClaudeApiKeyField,
  LocalProxyRequestOverrides,
  ProviderCategory,
} from "@/types";
import type { AppProviderMeta } from "../coreForm";

/** Claude 直接保存 JSON 编辑器里的 settings.json（去掉首尾空白） */
export const toClaudeSettingsConfig = (values: ProviderFormData): string =>
  values.settingsConfig.trim();

/** 本地代理的请求覆写与 User-Agent 只对非官方供应商生效 */
export const claudeAppliesLocalProxyOverrides = (
  category: ProviderCategory | undefined,
): boolean => category !== "official";

export interface ClaudeMetaInput {
  category: ProviderCategory | undefined;
  commonConfigEnabled: boolean;
  apiFormat: ClaudeApiFormat;
  apiKeyField: ClaudeApiKeyField;
  isFullUrl: boolean;
  customUserAgent: string;
  localProxyRequestOverrides?: LocalProxyRequestOverrides;
}

/** Claude 管理的 meta 字段；Codex 专属字段写 undefined */
export const toClaudeAppMeta = ({
  category,
  commonConfigEnabled,
  apiFormat,
  apiKeyField,
  isFullUrl,
  customUserAgent,
  localProxyRequestOverrides,
}: ClaudeMetaInput): AppProviderMeta => {
  const isOfficial = category === "official";
  return {
    commonConfigEnabled,
    codexChatReasoning: undefined,
    promptCacheRouting: undefined,
    customUserAgent: isOfficial
      ? undefined
      : customUserAgent.trim() || undefined,
    localProxyRequestOverrides: claudeAppliesLocalProxyOverrides(category)
      ? localProxyRequestOverrides
      : undefined,
    apiFormat: isOfficial ? undefined : apiFormat,
    apiKeyField:
      !isOfficial && apiKeyField !== "ANTHROPIC_AUTH_TOKEN"
        ? apiKeyField
        : undefined,
    impersonateClaudeCode: undefined,
    maxOutputTokens: undefined,
    isFullUrl: !isOfficial && isFullUrl ? true : undefined,
  };
};
