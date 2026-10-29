import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { providersApi, sessionsApi, settingsApi, type AppId } from "@/lib/api";
import type { DeleteSessionOptions } from "@/lib/api/sessions";
import type { SwitchResult } from "@/lib/api/providers";
import type { Provider, SessionMeta, Settings } from "@/types";
import { extractErrorMessage } from "@/utils/errorUtils";
import { getIpcErrorAction } from "@/lib/errors/errorActions";
import { generateUUID } from "@/utils/uuid";
import { invalidateProviderGraph } from "@/entities/provider/invalidate";
import { CODEX_OFFICIAL_PROVIDER_ID } from "@/utils/providerCapabilities";
import { appCaps } from "@/entities/app/registry";

export type AddProviderInput = Omit<Provider, "id"> & {
  providerKey?: string;
  addToLive?: boolean;
  ensureClaudeDesktopOfficialSeed?: boolean;
  ensureCodexOfficialSeed?: boolean;
};

/**
 * 供应商变更类 mutation 的目标应用必须随 variables 传入，而不是在 hook 创建时
 * 绑定：TanStack Query v5 会把最新的 options 交给仍在进行中的 mutation，若绑定
 * activeApp，切换标签后 onSuccess 会失效新应用的缓存，漏掉原应用的派生缓存。
 */
export interface AddProviderVariables {
  appId: AppId;
  provider: AddProviderInput;
}

export interface UpdateProviderVariables {
  appId: AppId;
  provider: Provider;
  originalId?: string;
}

export interface ProviderIdVariables {
  appId: AppId;
  providerId: string;
}

export const useAddProviderMutation = () => {
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  return useMutation({
    mutationFn: async ({
      appId,
      provider: providerInput,
    }: AddProviderVariables) => {
      const {
        providerKey: _providerKey,
        addToLive,
        ensureClaudeDesktopOfficialSeed,
        ensureCodexOfficialSeed,
        ...rest
      } = providerInput;

      if (appId === "claude-desktop" && ensureClaudeDesktopOfficialSeed) {
        await providersApi.ensureClaudeDesktopOfficialProvider();
        const providers = await providersApi.getAll(appId);
        const officialProvider = providers["claude-desktop-official"];
        if (!officialProvider) {
          throw new Error("Claude Desktop official provider was not created");
        }
        return officialProvider;
      }

      if (appId === "codex" && ensureCodexOfficialSeed) {
        await providersApi.ensureCodexOfficialProvider();
        const providers = await providersApi.getAll(appId);
        const officialProvider = providers[CODEX_OFFICIAL_PROVIDER_ID];
        if (!officialProvider) {
          throw new Error("Codex official provider was not created");
        }
        return officialProvider;
      }

      let id: string;

      if (appCaps(appId).additive) {
        if (
          providerInput.category === "omo" ||
          providerInput.category === "omo-slim"
        ) {
          const prefix = providerInput.category === "omo" ? "omo" : "omo-slim";
          id = `${prefix}-${generateUUID()}`;
        } else {
          if (!providerInput.providerKey) {
            throw new Error(`Provider key is required for ${appId}`);
          }
          id = providerInput.providerKey;
        }
      } else {
        id = generateUUID();
      }

      const newProvider: Provider = {
        ...rest,
        id,
        createdAt: Date.now(),
      };
      delete (newProvider as any).providerKey;

      await providersApi.add(newProvider, appId, addToLive);
      return newProvider;
    },
    onSuccess: async (_provider, { appId }) => {
      await invalidateProviderGraph(queryClient, appId);

      try {
        await providersApi.updateTrayMenu();
      } catch (trayError) {
        console.error(
          "Failed to update tray menu after adding provider",
          trayError,
        );
      }

      toast.success(t("notifications.providerAdded"), {
        closeButton: true,
      });
    },
    onError: (error: Error) => {
      const detail = extractErrorMessage(error) || t("common.unknown");
      toast.error(
        t("notifications.addFailed", {
          error: detail,
        }),
      );
    },
  });
};

export const useUpdateProviderMutation = () => {
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  return useMutation({
    mutationFn: async ({
      appId,
      provider,
      originalId,
    }: UpdateProviderVariables) => {
      await providersApi.update(provider, appId, originalId);
      return provider;
    },
    onSuccess: async (provider, variables) => {
      await invalidateProviderGraph(queryClient, variables.appId, {
        providerIds: [provider.id, variables.originalId].filter(
          (id): id is string => !!id,
        ),
      });
      toast.success(t("notifications.updateSuccess"), {
        closeButton: true,
      });
    },
    onError: (error: Error) => {
      const detail = extractErrorMessage(error) || t("common.unknown");
      toast.error(
        t("notifications.updateFailed", {
          error: detail,
        }),
      );
    },
  });
};

export const useDeleteProviderMutation = () => {
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  return useMutation({
    mutationFn: async ({ appId, providerId }: ProviderIdVariables) => {
      await providersApi.delete(providerId, appId);
    },
    onSuccess: async (_data, { appId }) => {
      await invalidateProviderGraph(queryClient, appId);

      try {
        await providersApi.updateTrayMenu();
      } catch (trayError) {
        console.error(
          "Failed to update tray menu after deleting provider",
          trayError,
        );
      }

      toast.success(t("notifications.deleteSuccess"), {
        closeButton: true,
      });
    },
    onError: (error: Error) => {
      const detail = extractErrorMessage(error) || t("common.unknown");
      toast.error(
        t("notifications.deleteFailed", {
          error: detail,
        }),
      );
    },
  });
};

export const useSwitchProviderMutation = () => {
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  return useMutation({
    mutationFn: async ({
      appId,
      providerId,
    }: ProviderIdVariables): Promise<SwitchResult> => {
      return await providersApi.switch(providerId, appId);
    },
    onSuccess: async (_result, { appId }) => {
      await invalidateProviderGraph(queryClient, appId);

      try {
        await providersApi.updateTrayMenu();
      } catch (trayError) {
        console.error(
          "Failed to update tray menu after switching provider",
          trayError,
        );
      }
    },
    onError: (error: Error) => {
      const detail = extractErrorMessage(error) || t("common.unknown");

      toast.error(t("notifications.switchFailedTitle"), {
        description: t("notifications.switchFailed", {
          error: detail,
        }),
        duration: 6000,
        // 可修复的错误码优先给出修复按钮，否则提供复制
        action: getIpcErrorAction(error, t) ?? {
          label: t("common.copy"),
          onClick: () => {
            navigator.clipboard?.writeText(detail).catch(() => undefined);
          },
        },
      });
    },
  });
};

export const useDeleteSessionMutation = () => {
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  return useMutation({
    mutationFn: async (input: DeleteSessionOptions) => {
      await sessionsApi.delete(input);
      return input;
    },
    onSuccess: async (input) => {
      queryClient.setQueryData<SessionMeta[]>(["sessions"], (current) =>
        (current ?? []).filter(
          (session) =>
            !(
              session.providerId === input.providerId &&
              session.sessionId === input.sessionId &&
              session.sourcePath === input.sourcePath
            ),
        ),
      );
      queryClient.removeQueries({
        queryKey: ["sessionMessages", input.providerId, input.sourcePath],
      });

      await queryClient.invalidateQueries({ queryKey: ["sessions"] });

      toast.success(t("sessionManager.sessionDeleted"));
    },
    onError: (error: Error) => {
      const detail = extractErrorMessage(error) || t("common.unknown");
      toast.error(
        t("sessionManager.deleteFailed", {
          error: detail,
        }),
      );
    },
  });
};

export const useSaveSettingsMutation = () => {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: async (settings: Settings) => {
      await settingsApi.save(settings);
    },
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: ["settings"] });
    },
  });
};
