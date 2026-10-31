import { useCallback } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";
import type { TranslationKey } from "@/i18n/keys";
import { providersApi, settingsApi, openclawApi, type AppId } from "@/lib/api";
import type {
  Provider,
  UsageScript,
  OpenClawProviderConfig,
  OpenClawDefaultModel,
} from "@/types";
import type { OpenClawSuggestedDefaults } from "@/config/openclawProviderPresets";
import { injectCodingPlanUsageScript } from "@/config/codingPlanProviders";
import {
  useAddProviderMutation,
  useUpdateProviderMutation,
  useDeleteProviderMutation,
  useSwitchProviderMutation,
} from "@/lib/query";
import { usageKeys } from "@/lib/query/usage";
import { extractErrorMessage } from "@/utils/errorUtils";
import { openclawKeys } from "@/hooks/useOpenClaw";
import {
  extractCodexWireApi,
  isCodexAnthropicWireApi,
  isCodexChatWireApi,
} from "@/utils/providerConfigUtils";
import { supportsOfficialProxyTakeover } from "@/utils/providerCapabilities";
import { providerKeys } from "@/entities/provider/keys";
import { invalidateProviderGraph } from "@/entities/provider/invalidate";
import { appCaps } from "@/entities/app/registry";

/** Codex 兼容配置（TOML config + wire_api）的供应商需要本地代理的原因 */
function codexProxyReason(provider: Provider): TranslationKey | null {
  const config = (provider.settingsConfig as Record<string, any>)?.config;
  const wireApi =
    typeof config === "string" ? extractCodexWireApi(config) : undefined;
  if (
    provider.meta?.apiFormat === "openai_chat" ||
    isCodexChatWireApi(wireApi)
  ) {
    return "notifications.proxyReasonOpenAIChat";
  }
  if (
    provider.meta?.apiFormat === "anthropic" ||
    isCodexAnthropicWireApi(wireApi)
  ) {
    return "notifications.proxyReasonAnthropicMessages";
  }
  if (provider.meta?.isFullUrl) {
    return "notifications.proxyReasonFullUrl";
  }
  return null;
}

/** 非官方供应商在代理未运行时无法直接使用的原因（按应用判定，按优先级排列） */
const PROXY_REQUIRED_REASONS: Partial<
  Record<AppId, (provider: Provider) => TranslationKey | null>
> = {
  claude: (provider) => {
    if (provider.meta?.providerType === "github_copilot") {
      return "notifications.proxyReasonCopilot";
    }
    if (provider.meta?.apiFormat === "openai_chat") {
      return "notifications.proxyReasonOpenAIChat";
    }
    if (provider.meta?.apiFormat === "openai_responses") {
      return "notifications.proxyReasonOpenAIResponses";
    }
    if (provider.meta?.isFullUrl) {
      return "notifications.proxyReasonFullUrl";
    }
    return null;
  },
  codex: codexProxyReason,
  grokbuild: codexProxyReason,
  "claude-desktop": (provider) =>
    provider.meta?.claudeDesktopMode === "proxy"
      ? "notifications.proxyReasonClaudeDesktop"
      : null,
};

/** 切换成功后的提示（需要重启应用 / 已加入配置）；缺省为通用切换成功 */
const SWITCH_SUCCESS_MESSAGES: Partial<
  Record<AppId, (provider: Provider) => TranslationKey>
> = {
  codex: () => "notifications.codexRestartRequired",
  grokbuild: () => "notifications.grokBuildRestartRequired",
  "claude-desktop": (provider) =>
    provider.meta?.claudeDesktopMode === "proxy"
      ? "notifications.claudeDesktopProxyRestartRequired"
      : "notifications.claudeDesktopRestartRequired",
  opencode: () => "notifications.addToConfigSuccess",
  openclaw: () => "notifications.addToConfigSuccess",
};

/**
 * Hook for managing provider actions (add, update, delete, switch)
 * Extracts business logic from App.tsx
 */
export function useProviderActions(
  activeApp: AppId,
  isProxyRunning?: boolean,
  isProxyTakeover?: boolean,
) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();

  const addProviderMutation = useAddProviderMutation();
  const updateProviderMutation = useUpdateProviderMutation();
  const deleteProviderMutation = useDeleteProviderMutation();
  const switchProviderMutation = useSwitchProviderMutation();

  // 正在切换中的供应商 ID（取自 mutation variables，并按 appId 绑定：
  // 切到其他应用后不会把本应用的 pending 状态带过去）
  const switchingProviderId =
    switchProviderMutation.isPending &&
    switchProviderMutation.variables?.appId === activeApp
      ? switchProviderMutation.variables.providerId
      : undefined;

  // Claude 插件同步逻辑
  // appId 显式传入：撤销按钮可能在切换到其他应用后才被点击
  const syncClaudePluginFor = useCallback(
    async (
      appId: AppId,
      resolveProvider: () => Promise<Provider | undefined>,
    ) => {
      if (!appCaps(appId).pluginSync) return;

      try {
        const settings = await settingsApi.get();
        if (!settings?.enableClaudePluginIntegration) {
          return;
        }

        const provider = await resolveProvider();
        if (!provider) return;
        const isOfficial = provider.category === "official";
        await settingsApi.applyClaudePluginConfig({ official: isOfficial });

        // 静默执行，不显示成功通知
      } catch (error) {
        const detail =
          extractErrorMessage(error) ||
          t("notifications.syncClaudePluginFailed");
        toast.error(detail, { duration: 4200 });
      }
    },
    [t],
  );

  const syncClaudePlugin = useCallback(
    (provider: Provider) =>
      syncClaudePluginFor(activeApp, async () => provider),
    [activeApp, syncClaudePluginFor],
  );

  // 撤销最近一次切换（切换成功 toast 上的"撤销"按钮）
  const undoSwitch = useCallback(
    async (appId: AppId) => {
      try {
        const reverted = await providersApi.revertLastSwitch(appId);
        await invalidateProviderGraph(queryClient, appId);
        // Claude 插件配置不在 live 日志内，按恢复后的供应商重新同步
        const restoredId = reverted?.providerId;
        if (restoredId) {
          await syncClaudePluginFor(
            appId,
            async () => (await providersApi.getAll(appId))[restoredId],
          );
        }
        try {
          await providersApi.updateTrayMenu();
        } catch (trayError) {
          console.error(
            "Failed to update tray menu after reverting switch",
            trayError,
          );
        }
        toast.success(t("notifications.switchReverted"), {
          closeButton: true,
        });
      } catch (error) {
        toast.error(
          t("notifications.switchRevertFailed", {
            error: extractErrorMessage(error) || t("common.unknown"),
          }),
          { duration: 6000 },
        );
      }
    },
    [queryClient, syncClaudePluginFor, t],
  );

  // 添加供应商
  const addProvider = useCallback(
    async (
      provider: Omit<Provider, "id"> & {
        providerKey?: string;
        suggestedDefaults?: OpenClawSuggestedDefaults;
        addToLive?: boolean;
        ensureClaudeDesktopOfficialSeed?: boolean;
        ensureCodexOfficialSeed?: boolean;
      },
    ) => {
      const enhanced = injectCodingPlanUsageScript(activeApp, provider);
      await addProviderMutation.mutateAsync({
        appId: activeApp,
        provider: enhanced,
      });

      // OpenClaw: register models to allowlist after adding provider
      if (appCaps(activeApp).defaultModel && provider.suggestedDefaults) {
        const { model, modelCatalog } = provider.suggestedDefaults;
        let modelsRegistered = false;

        try {
          // 1. Merge model catalog (allowlist)
          if (modelCatalog && Object.keys(modelCatalog).length > 0) {
            const existingCatalog = (await openclawApi.getModelCatalog()) || {};
            const mergedCatalog = { ...existingCatalog, ...modelCatalog };
            await openclawApi.setModelCatalog(mergedCatalog);
            await queryClient.invalidateQueries({
              queryKey: openclawKeys.health,
            });
            modelsRegistered = true;
          }

          // 2. Set default model (only if not already set)
          if (model) {
            const existingDefault = await openclawApi.getDefaultModel();
            if (!existingDefault?.primary) {
              await openclawApi.setDefaultModel(model);
              await queryClient.invalidateQueries({
                queryKey: openclawKeys.health,
              });
            }
          }

          // Show success toast if models were registered
          if (modelsRegistered) {
            toast.success(t("notifications.openclawModelsRegistered"), {
              closeButton: true,
            });
          }
        } catch (error) {
          // Log warning but don't block main flow - provider config is already saved
          console.warn(
            "[OpenClaw] Failed to register models to allowlist:",
            error,
          );
        }
      }
    },
    [addProviderMutation, activeApp, queryClient, t],
  );

  // 更新供应商
  const updateProvider = useCallback(
    async (provider: Provider, originalId?: string) => {
      await updateProviderMutation.mutateAsync({
        appId: activeApp,
        provider,
        originalId,
      });

      // 更新托盘菜单（失败不影响主操作）
      try {
        await providersApi.updateTrayMenu();
      } catch (trayError) {
        console.error(
          "Failed to update tray menu after updating provider",
          trayError,
        );
      }
    },
    [updateProviderMutation, activeApp],
  );

  // 切换供应商
  const switchProvider = useCallback(
    async (provider: Provider) => {
      // 同一应用已有切换在途时忽略重复触发（UI 也会禁用其余按钮）
      if (switchingProviderId !== undefined) return;

      // Determine why this provider requires the proxy
      const proxyRequiredReasonKey =
        !isProxyRunning && provider.category !== "official"
          ? (PROXY_REQUIRED_REASONS[activeApp]?.(provider) ?? null)
          : null;
      const proxyRequiredReason = proxyRequiredReasonKey
        ? t(proxyRequiredReasonKey)
        : null;

      if (proxyRequiredReason) {
        toast.warning(
          t("notifications.proxyRequiredForSwitch", {
            reason: proxyRequiredReason,
          }),
        );
      }

      // The built-in Codex official provider can reuse Codex's native ChatGPT
      // login through local routing. Other official providers remain blocked.
      const officialSupportsTakeover = supportsOfficialProxyTakeover(
        activeApp,
        provider,
      );
      if (
        isProxyTakeover &&
        provider.category === "official" &&
        !officialSupportsTakeover
      ) {
        toast.error(t("notifications.officialBlockedByProxy"), {
          duration: 6000,
        });
        return;
      }

      try {
        const result = await switchProviderMutation.mutateAsync({
          appId: activeApp,
          providerId: provider.id,
        });
        await syncClaudePlugin(provider);

        // Show backfill warning if present
        if (result?.warnings?.length) {
          toast.warning(t("notifications.backfillWarning"), { duration: 5000 });
        }

        // 若已弹过 proxyRequired 警告则不再弹 success
        if (!proxyRequiredReason) {
          const messageKey: TranslationKey =
            SWITCH_SUCCESS_MESSAGES[activeApp]?.(provider) ??
            "notifications.switchSuccess";
          const appId = activeApp;
          toast.success(t(messageKey), {
            closeButton: true,
            ...(result?.undoable
              ? {
                  action: {
                    label: t("notifications.undoSwitch"),
                    onClick: () => void undoSwitch(appId),
                  },
                }
              : {}),
          });
        }
      } catch {
        // 错误提示由 mutation 处理
      }
    },
    [
      switchProviderMutation,
      switchingProviderId,
      syncClaudePlugin,
      undoSwitch,
      activeApp,
      isProxyRunning,
      isProxyTakeover,
      t,
    ],
  );

  // 删除供应商
  const deleteProvider = useCallback(
    async (id: string) => {
      await deleteProviderMutation.mutateAsync({
        appId: activeApp,
        providerId: id,
      });
    },
    [deleteProviderMutation, activeApp],
  );

  // 保存用量脚本
  const saveUsageScript = useCallback(
    async (provider: Provider, script: UsageScript) => {
      try {
        const updatedProvider: Provider = {
          ...provider,
          meta: {
            ...provider.meta,
            usage_script: script,
          },
        };

        await providersApi.update(updatedProvider, activeApp);
        await queryClient.invalidateQueries({
          queryKey: providerKeys.list(activeApp),
        });
        // 🔧 保存用量脚本后，也应该失效该 provider 的用量查询缓存
        // 这样主页列表会使用新配置重新查询，而不是使用测试时的缓存
        await queryClient.invalidateQueries({
          queryKey: usageKeys.script(provider.id, activeApp),
        });
        await queryClient.invalidateQueries({
          queryKey: ["subscription", "quota", activeApp],
        });
        toast.success(t("provider.usageSaved"), { closeButton: true });
      } catch (error) {
        const detail =
          extractErrorMessage(error) || t("provider.usageSaveFailed");
        toast.error(detail);
      }
    },
    [activeApp, queryClient, t],
  );

  // Set provider as default model (OpenClaw only)
  const setAsDefaultModel = useCallback(
    async (provider: Provider) => {
      const config = provider.settingsConfig as OpenClawProviderConfig;
      if (!config.models || config.models.length === 0) {
        toast.error(t("notifications.openclawNoModels"));
        return;
      }

      const model: OpenClawDefaultModel = {
        primary: `${provider.id}/${config.models[0].id}`,
        fallbacks: config.models.slice(1).map((m) => `${provider.id}/${m.id}`),
      };

      try {
        await openclawApi.setDefaultModel(model);
        await queryClient.invalidateQueries({
          queryKey: openclawKeys.defaultModel,
        });
        await queryClient.invalidateQueries({
          queryKey: openclawKeys.health,
        });
        toast.success(t("notifications.openclawDefaultModelSet"), {
          closeButton: true,
        });
      } catch (error) {
        const detail =
          extractErrorMessage(error) ||
          t("notifications.openclawDefaultModelSetFailed");
        toast.error(detail);
      }
    },
    [queryClient, t],
  );

  return {
    addProvider,
    updateProvider,
    switchProvider,
    deleteProvider,
    saveUsageScript,
    setAsDefaultModel,
    switchingProviderId,
    isLoading:
      addProviderMutation.isPending ||
      updateProviderMutation.isPending ||
      deleteProviderMutation.isPending ||
      switchProviderMutation.isPending,
  };
}
