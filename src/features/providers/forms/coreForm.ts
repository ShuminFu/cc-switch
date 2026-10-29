import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { formatRequestOverrideObject } from "@/lib/requestOverrides";
import type { ProviderFormData } from "@/lib/schemas/provider";
import { resolveManagedAccountId } from "@/lib/authBinding";
import { mergeProviderMeta } from "@/utils/providerMetaUtils";
import type { CustomEndpoint, ProviderMeta } from "@/types";
import { isNonNegativeDecimalString } from "@/types/usage";
import type { PricingModelSourceOption } from "@/components/providers/forms/ProviderAdvancedConfig";
import { normalizePricingSource } from "@/components/providers/forms/helpers/opencodeFormUtils";
import type { ActivePreset } from "./basePreset";
import type { ProviderFormInitialData, ProviderFormValues } from "./types";

/**
 * Claude / Codex / Gemini 编辑器共用的状态与保存逻辑（纯函数）。
 * 各应用的 fromProvider / toSettingsConfig 在此基础上补充自己的字段。
 */

export interface PricingConfig {
  enabled: boolean;
  costMultiplier?: string;
  pricingModelSource: PricingModelSourceOption;
}

export const pricingConfigFromMeta = (meta?: ProviderMeta): PricingConfig => ({
  enabled:
    meta?.costMultiplier !== undefined ||
    meta?.pricingModelSource !== undefined,
  costMultiplier: meta?.costMultiplier,
  pricingModelSource: normalizePricingSource(meta?.pricingModelSource),
});

/** 启用了计费配置但倍率不是非负小数时为 true，调用方应阻止保存 */
export const isPricingConfigInvalid = (pricing: PricingConfig): boolean => {
  const costMultiplier = pricing.costMultiplier?.trim();
  return Boolean(
    pricing.enabled &&
      costMultiplier &&
      !isNonNegativeDecimalString(costMultiplier),
  );
};

/** 编辑已有供应商时，各编辑器共用的初始状态 */
export interface CoreFormInitialState {
  endpointAutoSelect: boolean;
  pricing: PricingConfig;
  customUserAgent: string;
  localProxyHeadersOverride: string;
  localProxyBodyOverride: string;
  githubAccountId: string | null;
  codexAccountId: string | null;
  codexFastMode: boolean;
}

export const coreFormFromProvider = (
  initialData?: ProviderFormInitialData,
): CoreFormInitialState => {
  const meta = initialData?.meta;
  return {
    endpointAutoSelect: meta?.endpointAutoSelect ?? true,
    pricing: pricingConfigFromMeta(meta),
    customUserAgent: meta?.customUserAgent ?? "",
    localProxyHeadersOverride: formatRequestOverrideObject(
      meta?.localProxyRequestOverrides?.headers,
    ),
    localProxyBodyOverride: formatRequestOverrideObject(
      meta?.localProxyRequestOverrides?.body,
    ),
    githubAccountId: resolveManagedAccountId(meta, "github_copilot"),
    codexAccountId: resolveManagedAccountId(meta, "codex_oauth"),
    codexFastMode: meta?.codexFastMode ?? false,
  };
};

/** 编辑模式下 settingsConfig 的初始文本；新建时用应用的默认配置 */
export const formDefaultValues = (
  initialData: ProviderFormInitialData | undefined,
  defaultSettingsConfig: string,
): ProviderFormData => ({
  name: initialData?.name ?? "",
  websiteUrl: initialData?.websiteUrl ?? "",
  notes: initialData?.notes ?? "",
  settingsConfig: initialData?.settingsConfig
    ? JSON.stringify(initialData.settingsConfig, null, 2)
    : defaultSettingsConfig,
  icon: initialData?.icon ?? "",
  iconColor: initialData?.iconColor ?? "",
});

/** 预设分类在选择器中的显示名 */
export function usePresetCategoryLabels(): Record<string, string> {
  const { t } = useTranslation();
  return useMemo(
    () => ({
      official: t("providerForm.categoryOfficial"),
      cn_official: t("providerForm.categoryCnOfficial"),
      aggregator: t("providerForm.categoryAggregation"),
      third_party: t("providerForm.categoryThirdParty"),
      omo: "OMO",
    }),
    [t],
  );
}

/** 基础 payload：去掉首尾空白，带上当前选中预设的信息 */
export const corePayload = (
  values: ProviderFormData,
  settingsConfig: string,
  activePreset: ActivePreset | null,
): ProviderFormValues => {
  const payload: ProviderFormValues = {
    ...values,
    name: values.name.trim(),
    websiteUrl: values.websiteUrl?.trim() ?? "",
    settingsConfig,
  };
  if (activePreset) {
    payload.presetId = activePreset.id;
    if (activePreset.category) {
      payload.presetCategory = activePreset.category;
    }
    if (activePreset.isPartner) {
      payload.isPartner = activePreset.isPartner;
    }
  }
  return payload;
};

/**
 * 新建时把测速弹窗里添加的端点并入 meta.custom_endpoints，并带上合作伙伴标记。
 * 没有新增端点（或处于编辑模式）时返回 undefined，沿用已有 meta。
 */
export const draftEndpointsMeta = ({
  isEditMode,
  draftCustomEndpoints,
  initialMeta,
  activePreset,
  now = Date.now(),
}: {
  isEditMode: boolean;
  draftCustomEndpoints: string[];
  initialMeta?: ProviderMeta;
  activePreset: ActivePreset | null;
  now?: number;
}): ProviderMeta | undefined => {
  if (isEditMode || draftCustomEndpoints.length === 0) return undefined;

  const customEndpoints = draftCustomEndpoints.reduce<
    Record<string, CustomEndpoint>
  >((acc, url) => {
    acc[url] = { url, addedAt: now, lastUsed: undefined };
    return acc;
  }, {});

  let merged = mergeProviderMeta(initialMeta, customEndpoints);
  if (activePreset?.isPartner) {
    merged = { ...(merged ?? {}), isPartner: true };
  }
  if (activePreset?.partnerPromotionKey) {
    merged = {
      ...(merged ?? {}),
      partnerPromotionKey: activePreset.partnerPromotionKey,
    };
  }
  return merged;
};

/** 托管账号（GitHub Copilot / Codex OAuth）的识别结果与选中的账号 */
export interface ManagedAuthState {
  isCopilotProvider: boolean;
  isCodexOauthProvider: boolean;
  githubAccountId: string | null;
  codexAccountId: string | null;
  codexFastMode: boolean;
}

/** 由应用决定的 meta 字段；未给出的字段写 undefined（清掉旧值） */
export type AppProviderMeta = Pick<
  ProviderMeta,
  | "commonConfigEnabled"
  | "codexChatReasoning"
  | "promptCacheRouting"
  | "customUserAgent"
  | "localProxyRequestOverrides"
  | "apiFormat"
  | "apiKeyField"
  | "impersonateClaudeCode"
  | "maxOutputTokens"
  | "isFullUrl"
>;

/**
 * 保存时的 meta：在已有 meta 上覆盖各编辑器管理的字段。
 * 字段顺序与取值和拆分前的 ProviderForm 一致。
 */
export const coreProviderMeta = ({
  baseMeta,
  endpointAutoSelect,
  providerType,
  managedAuth,
  pricing,
  app,
}: {
  baseMeta?: ProviderMeta;
  endpointAutoSelect: boolean;
  providerType?: ProviderMeta["providerType"];
  managedAuth: ManagedAuthState;
  pricing: PricingConfig;
  app: AppProviderMeta;
}): ProviderMeta => {
  const {
    isCopilotProvider,
    isCodexOauthProvider,
    githubAccountId,
    codexAccountId,
    codexFastMode,
  } = managedAuth;
  const meta: ProviderMeta = {
    ...(baseMeta ?? {}),
    commonConfigEnabled: app.commonConfigEnabled,
    endpointAutoSelect,
    claudeDesktopMode: undefined,
    // 保存 providerType（用于识别 Copilot / Codex OAuth 等特殊供应商）
    providerType,
    authBinding: isCopilotProvider
      ? {
          source: "managed_account",
          authProvider: "github_copilot",
          accountId: githubAccountId ?? undefined,
        }
      : isCodexOauthProvider
        ? {
            source: "managed_account",
            authProvider: "codex_oauth",
            accountId: codexAccountId ?? undefined,
          }
        : undefined,
    // GitHub Copilot 多账号：保存关联的账号 ID
    githubAccountId:
      isCopilotProvider && githubAccountId ? githubAccountId : undefined,
    codexFastMode: isCodexOauthProvider ? codexFastMode : undefined,
    codexChatReasoning: app.codexChatReasoning,
    promptCacheRouting: app.promptCacheRouting,
    customUserAgent: app.customUserAgent,
    localProxyRequestOverrides: app.localProxyRequestOverrides,
    costMultiplier: pricing.enabled ? pricing.costMultiplier : undefined,
    pricingModelSource:
      pricing.enabled && pricing.pricingModelSource !== "inherit"
        ? pricing.pricingModelSource
        : undefined,
    apiFormat: app.apiFormat,
    apiKeyField: app.apiKeyField,
    impersonateClaudeCode: app.impersonateClaudeCode,
    maxOutputTokens: app.maxOutputTokens,
    isFullUrl: app.isFullUrl,
  };

  if (!isCodexOauthProvider && "codexFastMode" in meta) {
    delete meta.codexFastMode;
  }
  return meta;
};
