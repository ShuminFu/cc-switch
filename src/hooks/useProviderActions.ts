import { useCallback } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";
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
import { queryKeys } from "@/lib/query/keys";

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

  const addProviderMutation = useAddProviderMutation(activeApp);
  const updateProviderMutation = useUpdateProviderMutation(activeApp);
  const deleteProviderMutation = useDeleteProviderMutation(activeApp);
  const switchProviderMutation = useSwitchProviderMutation(activeApp);

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
          t("notifications.syncClaudePluginFailed", {
            defaultValue: "Sync Claude plugin failed",
          });
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
      const created = await addProviderMutation.mutateAsync(enhanced);

      // OpenClaw: register models to allowlist after adding provider
      if (activeApp === "openclaw" && provider.suggestedDefaults) {
        const suggested = provider.suggestedDefaults;
        const register = async (): Promise<void> => {
          try {
            const { model, modelCatalog } = suggested;
            let modelsRegistered = false;

            // 1. Merge model catalog (allowlist)
            if (modelCatalog && Object.keys(modelCatalog).length > 0) {
              const existingCatalog =
                (await openclawApi.getModelCatalog()) || {};
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

            if (modelsRegistered) {
              toast.success(
                t("notifications.openclawModelsRegistered", {
                  defaultValue: "Models have been registered to /model list",
                }),
                { closeButton: true },
              );
            }
          } catch (error) {
            // The provider itself is saved; tell the user the models are
            // missing from OpenClaw's /model list and offer a retry instead
            // of only logging (FE-PROV-50).
            console.warn(
              "[OpenClaw] Failed to register models to allowlist:",
              error,
            );
            toast.warning(
              t("notifications.openclawModelsRegisterFailed", {
                defaultValue:
                  "Provider added, but its models could not be registered in OpenClaw: {{error}}",
                error: extractErrorMessage(error),
              }),
              {
                closeButton: true,
                action: {
                  label: t("common.retry", { defaultValue: "Retry" }),
                  onClick: () => void register(),
                },
              },
            );
          }
        };
        await register();
      }
      return created;
    },
    [addProviderMutation, activeApp, queryClient, t],
  );

  // 更新供应商
  const updateProvider = useCallback(
    async (provider: Provider, originalId?: string) => {
      await updateProviderMutation.mutateAsync({ provider, originalId });

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
    [updateProviderMutation],
  );

  // 切换供应商
  const switchProvider = useCallback(
    async (provider: Provider) => {
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
          proxyRequiredReason = t("notifications.proxyReasonCopilot", {
            defaultValue: "uses GitHub Copilot as a Claude provider",
          });
        } else if (
          provider.meta?.apiFormat === "openai_chat" &&
          activeApp === "claude"
        ) {
          proxyRequiredReason = t("notifications.proxyReasonOpenAIChat", {
            defaultValue: "uses OpenAI Chat API format",
          });
        } else if (
          provider.meta?.apiFormat === "openai_responses" &&
          activeApp === "claude"
        ) {
          proxyRequiredReason = t("notifications.proxyReasonOpenAIResponses", {
            defaultValue: "uses OpenAI Responses API format",
          });
        } else if (isCodexChatFormat) {
          proxyRequiredReason = t("notifications.proxyReasonOpenAIChat", {
            defaultValue: "uses OpenAI Chat API format",
          });
        } else if (isCodexAnthropicFormat) {
          proxyRequiredReason = t(
            "notifications.proxyReasonAnthropicMessages",
            {
              defaultValue: "uses Anthropic Messages API format",
            },
          );
        } else if (
          activeApp === "claude-desktop" &&
          provider.meta?.claudeDesktopMode === "proxy"
        ) {
          proxyRequiredReason = t("notifications.proxyReasonClaudeDesktop", {
            defaultValue: "uses Claude Desktop local routing mode",
          });
        } else if (
          provider.meta?.isFullUrl &&
          (activeApp === "claude" ||
            activeApp === "codex" ||
            activeApp === "grokbuild")
        ) {
          proxyRequiredReason = t("notifications.proxyReasonFullUrl", {
            defaultValue: "has full URL connection mode enabled",
          });
        }
      }

      if (proxyRequiredReason) {
        toast.warning(
          t("notifications.proxyRequiredForSwitch", {
            reason: proxyRequiredReason,
            defaultValue:
              "This provider {{reason}}, requires the routing service to work properly. Start routing first.",
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
        toast.error(
          t("notifications.officialBlockedByProxy", {
            defaultValue:
              "Cannot switch to official provider while local routing is active. Using routing with official APIs may cause account bans.",
          }),
          { duration: 6000 },
        );
        return;
      }

      try {
        const result = await switchProviderMutation.mutateAsync(provider.id);
        await syncClaudePlugin(provider);

        // A backfill warning is folded into the success toast so one switch
        // shows one toast (FE-ARCH-36).
        const backfillWarning = result?.warnings?.length
          ? t("notifications.backfillWarning", {
              defaultValue:
                "Switched successfully, but failed to save changes back to the previous provider",
            })
          : undefined;
        if (backfillWarning && proxyRequiredReason) {
          toast.warning(backfillWarning, { duration: 5000 });
        }

        // 若已弹过 proxyRequired 警告则不再弹 success
        if (!proxyRequiredReason) {
          let messageKey = "notifications.switchSuccess";
          let defaultMessage = "切换成功！";
          if (activeApp === "codex") {
            messageKey = "notifications.codexRestartRequired";
            defaultMessage = "切换成功，请重启客户端以生效";
          } else if (activeApp === "grokbuild") {
            messageKey = "notifications.grokBuildRestartRequired";
            defaultMessage = "切换成功，请重启 Grok Build 以生效";
          } else if (activeApp === "claude-desktop") {
            if (provider.meta?.claudeDesktopMode === "proxy") {
              messageKey = "notifications.claudeDesktopProxyRestartRequired";
              defaultMessage =
                "切换成功，请保持 CC Switch 运行，并重启 Claude Desktop 后生效";
            } else {
              messageKey = "notifications.claudeDesktopRestartRequired";
              defaultMessage = "切换成功，重启 Claude Desktop 后生效";
            }
          } else if (activeApp === "opencode" || activeApp === "openclaw") {
            messageKey = "notifications.addToConfigSuccess";
            defaultMessage = "已添加到配置";
          }
          if (backfillWarning) {
            toast.warning(t(messageKey, { defaultValue: defaultMessage }), {
              description: backfillWarning,
              duration: 5000,
              closeButton: true,
            });
          } else {
            toast.success(t(messageKey, { defaultValue: defaultMessage }), {
              closeButton: true,
            });
          }
        }
      } catch {
        // 错误提示由 mutation 处理
      }
    },
    [
      switchProviderMutation,
      syncClaudePlugin,
      activeApp,
      isProxyRunning,
      isProxyTakeover,
      t,
    ],
  );

  // 删除供应商
  const deleteProvider = useCallback(
    async (id: string) => {
      await deleteProviderMutation.mutateAsync(id);
    },
    [deleteProviderMutation],
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
          queryKey: queryKeys.providers.byApp(activeApp),
        });
        // 🔧 保存用量脚本后，也应该失效该 provider 的用量查询缓存
        // 这样主页列表会使用新配置重新查询，而不是使用测试时的缓存
        await queryClient.invalidateQueries({
          queryKey: usageKeys.script(provider.id, activeApp),
        });
        await queryClient.invalidateQueries({
          queryKey: queryKeys.subscription.quota(activeApp),
        });
        toast.success(
          t("provider.usageSaved", {
            defaultValue: "Usage query configuration saved",
          }),
          { closeButton: true },
        );
      } catch (error) {
        const detail =
          extractErrorMessage(error) ||
          t("provider.usageSaveFailed", {
            defaultValue: "Failed to save usage query configuration",
          });
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
        toast.error(
          t("notifications.openclawNoModels", {
            defaultValue: "No models configured",
          }),
        );
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
        toast.success(
          t("notifications.openclawDefaultModelSet", {
            defaultValue: "Set as default model",
          }),
          { closeButton: true },
        );
      } catch (error) {
        const detail =
          extractErrorMessage(error) ||
          t("notifications.openclawDefaultModelSetFailed", {
            defaultValue: "Failed to set default model",
          });
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
    isLoading:
      addProviderMutation.isPending ||
      updateProviderMutation.isPending ||
      deleteProviderMutation.isPending ||
      switchProviderMutation.isPending,
  };
}
