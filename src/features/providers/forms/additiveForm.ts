import type { TFunction } from "i18next";
import type { ProviderMeta } from "@/types";
import {
  PROVIDER_KEY_PATTERN,
  providerKeyMessageKeys,
  type ProviderKeyI18nPrefix,
} from "./AdditiveProviderKeyField";
import {
  coreProviderMeta,
  type CoreFormInitialState,
  type ManagedAuthState,
} from "./coreForm";
import type { ProviderFormInitialData } from "./types";

/**
 * 累加应用（OpenCode / OpenClaw / Hermes）编辑器共用的保存逻辑（纯函数）。
 * 这三个应用以 providerKey 为主键写入 live 配置，端点、计费、托管账号等
 * 不在界面上编辑，保存时沿用已有 meta。
 */

/** 不在这三个应用的界面上编辑的 meta 字段：写 undefined（清掉旧值） */
export const ADDITIVE_APP_META = {
  commonConfigEnabled: undefined,
  codexChatReasoning: undefined,
  promptCacheRouting: undefined,
  customUserAgent: undefined,
  localProxyRequestOverrides: undefined,
  apiFormat: undefined,
  apiKeyField: undefined,
  impersonateClaudeCode: undefined,
  maxOutputTokens: undefined,
  isFullUrl: undefined,
} as const;

/** 已被其他供应商占用的 key（数据库 + live 配置），去重并排除自身 */
export const additiveExistingKeys = (
  providerId: string | undefined,
  ...sources: readonly string[][]
): string[] =>
  Array.from(new Set(sources.flat().filter((key) => key !== providerId)));

/**
 * providerKey 是否已写入 live 配置（写入后不可再改名）。
 * 只在编辑模式下有意义；新建时既不锁定也不等待加载。
 */
export const providerKeyLockState = ({
  isEditMode,
  providerId,
  liveProviderIds,
  isLiveProviderIdsLoading,
}: {
  isEditMode: boolean;
  providerId?: string;
  liveProviderIds: readonly string[];
  isLiveProviderIdsLoading: boolean;
}): { isLocked: boolean; isLockStateLoading: boolean } => ({
  isLocked:
    isEditMode && Boolean(providerId) && liveProviderIds.includes(providerId!),
  isLockStateLoading: isEditMode && isLiveProviderIdsLoading,
});

/**
 * providerKey 的阻断错误。providerKey 是主键 ID：空 / 格式不合法 / 状态加载中 /
 * 重复都属于完整性约束，硬拒绝（mutations 层也会 throw，软化只会让错误更晦涩）。
 */
export const additiveProviderKeyError = (
  {
    key,
    i18nPrefix,
    isLocked,
    isLockStateLoading,
    existingKeys,
  }: {
    key: string;
    i18nPrefix: ProviderKeyI18nPrefix;
    isLocked: boolean;
    isLockStateLoading: boolean;
    existingKeys: readonly string[];
  },
  t: TFunction,
): string | null => {
  const keys = providerKeyMessageKeys(i18nPrefix);
  if (!key.trim()) return t(keys.required);
  if (!PROVIDER_KEY_PATTERN.test(key)) return t(keys.invalid);
  if (isLockStateLoading) return t("providerForm.providerKeyStatusLoading");
  if (!isLocked && existingKeys.includes(key)) return t(keys.duplicate);
  return null;
};

/** 托管账号（导入的 Copilot / Codex OAuth 供应商）只能来自已保存的 providerType */
export const additiveManagedAuth = (
  initialData: ProviderFormInitialData | undefined,
  initial: CoreFormInitialState,
): ManagedAuthState => ({
  isCopilotProvider: initialData?.meta?.providerType === "github_copilot",
  isCodexOauthProvider: initialData?.meta?.providerType === "codex_oauth",
  githubAccountId: initial.githubAccountId,
  codexAccountId: initial.codexAccountId,
  codexFastMode: initial.codexFastMode,
});

/** 托管账号未登录时的阻断错误（token 根本不存在，保存了也没法建立） */
export const managedAuthLoginError = (
  managedAuth: ManagedAuthState,
  status: {
    isCopilotAuthenticated: boolean;
    isCodexOauthAuthenticated: boolean;
  },
  t: TFunction,
): string | null => {
  if (managedAuth.isCopilotProvider && !status.isCopilotAuthenticated) {
    return t("copilot.loginRequired");
  }
  if (managedAuth.isCodexOauthProvider && !status.isCodexOauthAuthenticated) {
    return t("codexOauth.loginRequired");
  }
  return null;
};

/** 保存时的 meta：沿用已有 meta，应用相关字段一律清掉 */
export const additiveProviderMeta = (
  initialData: ProviderFormInitialData | undefined,
  initial: CoreFormInitialState,
): ProviderMeta =>
  coreProviderMeta({
    baseMeta: initialData?.meta ? { ...initialData.meta } : undefined,
    endpointAutoSelect: initial.endpointAutoSelect,
    providerType: initialData?.meta?.providerType,
    managedAuth: additiveManagedAuth(initialData, initial),
    pricing: initial.pricing,
    app: ADDITIVE_APP_META,
  });
