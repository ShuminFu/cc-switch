import {
  useMutation,
  useQueryClient,
  type QueryClient,
} from "@tanstack/react-query";
import { appHas } from "@/config/apps";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { providersApi, sessionsApi, settingsApi, type AppId } from "@/lib/api";
import type { DeleteSessionOptions } from "@/lib/api/sessions";
import { sessionTrashApi } from "@/lib/api/sessionTrash";
import type { SwitchResult } from "@/lib/api/providers";
import type { Provider, SessionMeta, Settings } from "@/types";
import { extractErrorMessage } from "@/utils/errorUtils";
import { redactSecrets } from "@/utils/redact";
import { generateUUID } from "@/utils/uuid";
import { invalidateHermesProviderCaches } from "./hermes";
import { queryKeys } from "./keys";
import { CODEX_OFFICIAL_PROVIDER_ID } from "@/utils/providerCapabilities";

/**
 * Which provider caches a provider mutation affects:
 * - `membership`: a provider was added or deleted;
 * - `update`: a provider's settings changed;
 * - `switch`: the current provider changed.
 */
type ProviderChange = "membership" | "update" | "switch";

/**
 * Invalidate the provider list of `appId` plus the app-specific caches that
 * derive from it, in parallel (FE-ARCH-05: no serial awaits).
 */
export async function invalidateProviderCaches(
  queryClient: QueryClient,
  appId: AppId,
  change: ProviderChange,
): Promise<void> {
  const keys: (readonly unknown[])[] = [queryKeys.providers.byApp(appId)];

  if (appId === "claude-desktop" && change === "switch") {
    keys.push(queryKeys.proxy.status, queryKeys.providers.claudeDesktopStatus);
  }
  if (appHas(appId, "omo") && change !== "update") {
    keys.push(
      queryKeys.omo.currentProviderId,
      queryKeys.omoSlim.currentProviderId,
    );
    if (change === "membership") {
      keys.push(queryKeys.omo.providerCount, queryKeys.omoSlim.providerCount);
    }
  }
  if (appId === "opencode" && change === "switch") {
    keys.push(queryKeys.providers.opencodeLiveIds);
  }
  if (appId === "openclaw") {
    if (change === "switch") {
      keys.push(
        queryKeys.openclaw.liveProviderIds,
        queryKeys.openclaw.defaultModel,
      );
    }
    keys.push(queryKeys.openclaw.health);
  }

  await Promise.all([
    ...keys.map((queryKey) => queryClient.invalidateQueries({ queryKey })),
    appId === "hermes"
      ? invalidateHermesProviderCaches(queryClient)
      : undefined,
  ]);
}

export const useAddProviderMutation = (appId: AppId) => {
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  return useMutation({
    mutationFn: async (
      providerInput: Omit<Provider, "id"> & {
        providerKey?: string;
        addToLive?: boolean;
        ensureClaudeDesktopOfficialSeed?: boolean;
        ensureCodexOfficialSeed?: boolean;
      },
    ) => {
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

      if (appId === "opencode" || appId === "openclaw" || appId === "hermes") {
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

      await providersApi.add(newProvider, appId, addToLive);
      return newProvider;
    },
    onSuccess: async () => {
      await invalidateProviderCaches(queryClient, appId, "membership");

      try {
        await providersApi.updateTrayMenu();
      } catch (trayError) {
        console.error(
          "Failed to update tray menu after adding provider",
          trayError,
        );
      }

      toast.success(
        t("notifications.providerAdded", {
          defaultValue: "Provider added",
        }),
        {
          closeButton: true,
        },
      );
    },
    onError: (error: unknown) => {
      const detail = extractErrorMessage(error) || t("common.unknown");
      toast.error(
        t("notifications.addFailed", {
          defaultValue: "Failed to add provider: {{error}}",
          error: detail,
        }),
      );
    },
  });
};

export const useUpdateProviderMutation = (appId: AppId) => {
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  return useMutation({
    mutationFn: async ({
      provider,
      originalId,
    }: {
      provider: Provider;
      originalId?: string;
    }) => {
      await providersApi.update(provider, appId, originalId);
      return provider;
    },
    onSuccess: async (provider, variables) => {
      await Promise.all([
        invalidateProviderCaches(queryClient, appId, "update"),
        queryClient.invalidateQueries({
          queryKey: queryKeys.usageScript.byProvider(provider.id, appId),
        }),
        variables.originalId && variables.originalId !== provider.id
          ? queryClient.invalidateQueries({
              queryKey: queryKeys.usageScript.byProvider(
                variables.originalId,
                appId,
              ),
            })
          : undefined,
      ]);
      toast.success(
        t("notifications.updateSuccess", {
          defaultValue: "Provider updated successfully",
        }),
        {
          closeButton: true,
        },
      );
    },
    onError: (error: unknown) => {
      const detail = extractErrorMessage(error) || t("common.unknown");
      toast.error(
        t("notifications.updateFailed", {
          defaultValue: "Failed to update provider: {{error}}",
          error: detail,
        }),
      );
    },
  });
};

interface ProvidersCache {
  providers: Record<string, Provider>;
  currentProviderId: string;
}

/**
 * Re-create a deleted provider from its snapshot (same id, settingsConfig,
 * meta and sortIndex). Only offered for apps whose providers live in the
 * database alone; additive apps also edit their live config on delete.
 */
const canUndoDelete = (appId: AppId) => !appHas(appId, "additiveLiveConfig");

export const useDeleteProviderMutation = (appId: AppId) => {
  const queryClient = useQueryClient();
  const { t } = useTranslation();
  const cacheKey = queryKeys.providers.byApp(appId);

  return useMutation({
    mutationFn: async (providerId: string) => {
      await providersApi.delete(providerId, appId);
    },
    // Optimistic: drop the card at once, put it back if the delete fails
    // (FE-ARCH-18).
    onMutate: async (providerId: string) => {
      await queryClient.cancelQueries({ queryKey: cacheKey });
      const previous = queryClient.getQueryData<ProvidersCache>(cacheKey);
      if (previous?.providers?.[providerId]) {
        const { [providerId]: _removed, ...rest } = previous.providers;
        void _removed;
        queryClient.setQueryData<ProvidersCache>(cacheKey, {
          ...previous,
          providers: rest,
        });
      }
      return { previous, deleted: previous?.providers?.[providerId] };
    },
    onSuccess: async (_data, _providerId, context) => {
      await invalidateProviderCaches(queryClient, appId, "membership");

      try {
        await providersApi.updateTrayMenu();
      } catch (trayError) {
        console.error(
          "Failed to update tray menu after deleting provider",
          trayError,
        );
      }

      const deleted = context?.deleted;
      toast.success(
        t("notifications.deleteSuccess", {
          defaultValue: "Provider deleted",
        }),
        {
          closeButton: true,
          // Undo re-adds the exact snapshot (FE-ARCH-17, first increment).
          ...(deleted && canUndoDelete(appId)
            ? {
                duration: 8000,
                action: {
                  label: t("common.undo", { defaultValue: "Undo" }),
                  onClick: () => {
                    void (async () => {
                      try {
                        await providersApi.add(deleted, appId, false);
                        await invalidateProviderCaches(
                          queryClient,
                          appId,
                          "membership",
                        );
                        await providersApi.updateTrayMenu().catch(() => false);
                      } catch (error) {
                        toast.error(
                          t("notifications.undoDeleteFailed", {
                            defaultValue:
                              "Could not restore the provider: {{error}}",
                            error: extractErrorMessage(error),
                          }),
                        );
                      }
                    })();
                  },
                },
              }
            : {}),
        },
      );
    },
    onError: (error: unknown, _providerId, context) => {
      if (context?.previous) {
        queryClient.setQueryData(cacheKey, context.previous);
      }
      void queryClient.invalidateQueries({ queryKey: cacheKey });
      const detail = extractErrorMessage(error) || t("common.unknown");
      toast.error(
        t("notifications.deleteFailed", {
          defaultValue: "Failed to delete provider: {{error}}",
          error: detail,
        }),
      );
    },
  });
};

export const useSwitchProviderMutation = (appId: AppId) => {
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  return useMutation({
    mutationFn: async (providerId: string): Promise<SwitchResult> => {
      return await providersApi.switch(providerId, appId);
    },
    // Optimistic: highlight the new current provider immediately and roll
    // back on failure (FE-ARCH-18). Additive apps add to the live config
    // instead of changing a single current provider, so they keep waiting.
    onMutate: async (providerId: string) => {
      if (appHas(appId, "additiveLiveConfig")) return { previous: undefined };
      const cacheKey = queryKeys.providers.byApp(appId);
      await queryClient.cancelQueries({ queryKey: cacheKey });
      const previous = queryClient.getQueryData<ProvidersCache>(cacheKey);
      if (previous) {
        queryClient.setQueryData<ProvidersCache>(cacheKey, {
          ...previous,
          currentProviderId: providerId,
        });
      }
      return { previous };
    },
    onSuccess: async () => {
      await invalidateProviderCaches(queryClient, appId, "switch");

      try {
        await providersApi.updateTrayMenu();
      } catch (trayError) {
        console.error(
          "Failed to update tray menu after switching provider",
          trayError,
        );
      }
    },
    onError: (error: unknown, _providerId, context) => {
      if (context?.previous) {
        queryClient.setQueryData(
          queryKeys.providers.byApp(appId),
          context.previous,
        );
      }
      const detail = extractErrorMessage(error) || t("common.unknown");

      toast.error(
        t("notifications.switchFailedTitle", { defaultValue: "Switch failed" }),
        {
          description: t("notifications.switchFailed", {
            defaultValue: "Switch failed: {{error}}",
            error: detail,
          }),
          duration: 6000,
          action: {
            label: t("common.copy", { defaultValue: "Copy" }),
            onClick: () => {
              navigator.clipboard
                ?.writeText(redactSecrets(detail))
                .catch(() => undefined);
            },
          },
        },
      );
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
      queryClient.setQueryData<SessionMeta[]>(
        queryKeys.sessions.all,
        (current) =>
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
        queryKey: queryKeys.sessions.messages(
          input.providerId,
          input.sourcePath,
        ),
      });

      await queryClient.invalidateQueries({
        queryKey: queryKeys.sessions.all,
      });

      toast.success(
        t("sessionManager.sessionDeleted", {
          defaultValue: "Session deleted",
        }),
        {
          // SESS-41: file-based sessions go to the trash; offer to put it back.
          action: {
            label: t("sessionManager.undoDelete", { defaultValue: "Undo" }),
            onClick: () => {
              void sessionTrashApi
                .undoDelete(input.providerId, input.sessionId)
                .then(() =>
                  queryClient.invalidateQueries({
                    queryKey: queryKeys.sessions.all,
                  }),
                )
                .catch((error: unknown) =>
                  toast.error(
                    extractErrorMessage(error) ||
                      t("sessionManager.restoreFailed", {
                        defaultValue: "Could not restore the session",
                      }),
                  ),
                );
            },
          },
        },
      );
    },
    onError: (error: unknown) => {
      const detail = extractErrorMessage(error) || t("common.unknown");
      toast.error(
        t("sessionManager.deleteFailed", {
          defaultValue: "Failed to delete session: {{error}}",
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
      await queryClient.invalidateQueries({
        queryKey: queryKeys.settings.all,
      });
    },
  });
};
