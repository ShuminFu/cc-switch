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

  // 撤销最近一次切换（切换成功 toast 上的"撤销"按钮）
  const undoSwitch = useCallback(
    async (appId: AppId) => {
      try {
        await providersApi.revertLastSwitch(appId);
        await invalidateProviderGraph(queryClient, appId);
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
    [queryClient, t],
  );

  // Claude 插件同步逻辑
  const syncClaudePlugin = useCallback(
    async (provider: Provider) => {
      if (activeApp !== "claude") return;

      try {
        const settings = await settingsApi.get();
        if (!settings?.enableClaudePluginIntegration) {
          return;
        }

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
    [activeApp, t],
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
      if (activeApp === "openclaw" && provider.suggestedDefaults) {
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

      const isCopilotProvider =
        activeApp === "claude" &&
        provider.meta?.providerType === "github_copilot";
      const isCodexChatFormat =
        (activeApp === "codex" || activeApp === "grokbuild") &&
        (provider.meta?.apiFormat === "openai_chat" ||
          (typeof (provider.settingsConfig as Record<string, any>)?.config ===
            "string" &&
            isCodexChatWireApi(
              extractCodexWireApi(
                (provider.settingsConfig as Record<string, any>).config,
              ),
            )));
      const isCodexAnthropicFormat =
        (activeApp === "codex" || activeApp === "grokbuild") &&
        (provider.meta?.apiFormat === "anthropic" ||
          (typeof (provider.settingsConfig as Record<string, any>)?.config ===
            "string" &&
            isCodexAnthropicWireApi(
              extractCodexWireApi(
                (provider.settingsConfig as Record<string, any>).config,
              ),
            )));

      // Determine why this provider requires the proxy
      let proxyRequiredReason: string | null = null;
      if (!isProxyRunning && provider.category !== "official") {
        if (isCopilotProvider) {
          proxyRequiredReason = t("notifications.proxyReasonCopilot");
        } else if (
          provider.meta?.apiFormat === "openai_chat" &&
          activeApp === "claude"
        ) {
          proxyRequiredReason = t("notifications.proxyReasonOpenAIChat");
        } else if (
          provider.meta?.apiFormat === "openai_responses" &&
          activeApp === "claude"
        ) {
          proxyRequiredReason = t("notifications.proxyReasonOpenAIResponses");
        } else if (isCodexChatFormat) {
          proxyRequiredReason = t("notifications.proxyReasonOpenAIChat");
        } else if (isCodexAnthropicFormat) {
          proxyRequiredReason = t("notifications.proxyReasonAnthropicMessages");
        } else if (
          activeApp === "claude-desktop" &&
          provider.meta?.claudeDesktopMode === "proxy"
        ) {
          proxyRequiredReason = t("notifications.proxyReasonClaudeDesktop");
        } else if (
          provider.meta?.isFullUrl &&
          (activeApp === "claude" ||
            activeApp === "codex" ||
            activeApp === "grokbuild")
        ) {
          proxyRequiredReason = t("notifications.proxyReasonFullUrl");
        }
      }

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
          let messageKey: TranslationKey = "notifications.switchSuccess";
          if (activeApp === "codex") {
            messageKey = "notifications.codexRestartRequired";
          } else if (activeApp === "grokbuild") {
            messageKey = "notifications.grokBuildRestartRequired";
          } else if (activeApp === "claude-desktop") {
            if (provider.meta?.claudeDesktopMode === "proxy") {
              messageKey = "notifications.claudeDesktopProxyRestartRequired";
            } else {
              messageKey = "notifications.claudeDesktopRestartRequired";
            }
          } else if (activeApp === "opencode" || activeApp === "openclaw") {
            messageKey = "notifications.addToConfigSuccess";
          }
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
