import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { useQueryClient } from "@tanstack/react-query";
import type { Provider } from "@/types";
import { providersApi, settingsApi, type AppId } from "@/lib/api";
import type { useProviderActions } from "@/hooks/useProviderActions";
import {
  useDisableCurrentOmo,
  useDisableCurrentOmoSlim,
} from "@/lib/query/omo";
import { useAppCaps } from "@/entities/app/registry";
import { providerKeys } from "@/entities/provider/keys";
import { liveProviderIdsQuery } from "@/entities/provider/liveIds";
import { extractErrorMessage } from "@/utils/errorUtils";
import { deepClone } from "@/utils/deepClone";

type NewProvider = Omit<Provider, "id" | "createdAt"> & {
  providerKey?: string;
  addToLive?: boolean;
};

/** 生成不与现有 key 冲突的副本 key：`id-copy`、`id-copy-2`… */
export function generateUniqueProviderCopyKey(
  originalKey: string,
  existingKeys: string[],
): string {
  const baseKey = `${originalKey}-copy`;

  if (!existingKeys.includes(baseKey)) {
    return baseKey;
  }

  let counter = 2;
  while (existingKeys.includes(`${baseKey}-${counter}`)) {
    counter++;
  }
  return `${baseKey}-${counter}`;
}

interface ProviderShellActionsOptions {
  activeApp: AppId;
  providers: Record<string, Provider>;
  addProvider: ReturnType<typeof useProviderActions>["addProvider"];
  refetchProviders: () => Promise<unknown>;
}

/**
 * 主窗口供应商列表上的非 CRUD 操作：复制、打开终端 / 网站、导入后刷新、停用 OMO。
 */
export function useProviderShellActions({
  activeApp,
  providers,
  addProvider,
  refetchProviders,
}: ProviderShellActionsOptions) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const activeAppCaps = useAppCaps(activeApp);

  const disableOmoMutation = useDisableCurrentOmo();
  const disableOmoSlimMutation = useDisableCurrentOmoSlim();

  const disableOmoCallbacks = {
    onSuccess: () => {
      toast.success(t("omo.disabled"));
    },
    onError: (error: Error) => {
      toast.error(
        t("omo.disableFailed", {
          error: extractErrorMessage(error),
        }),
      );
    },
  };

  const disableOmo = () =>
    disableOmoMutation.mutate(undefined, disableOmoCallbacks);
  const disableOmoSlim = () =>
    disableOmoSlimMutation.mutate(undefined, disableOmoCallbacks);

  const openWebsite = async (url: string) => {
    try {
      await settingsApi.openExternal(url);
    } catch (error) {
      const detail =
        extractErrorMessage(error) || t("notifications.openLinkFailed");
      toast.error(detail);
    }
  };

  const loadLiveProviderIds = (): Promise<string[]> => {
    const query = liveProviderIdsQuery(activeApp);
    return query
      ? queryClient.ensureQueryData(query)
      : Promise.resolve<string[]>([]);
  };

  const duplicateProvider = async (provider: Provider) => {
    const newSortIndex =
      provider.sortIndex !== undefined ? provider.sortIndex + 1 : undefined;

    const duplicatedProvider: NewProvider = {
      name: `${provider.name} copy`,
      settingsConfig: deepClone(provider.settingsConfig),
      websiteUrl: provider.websiteUrl,
      category: provider.category,
      sortIndex: newSortIndex, // 复制原 sortIndex + 1
      meta: provider.meta ? deepClone(provider.meta) : undefined,
      icon: provider.icon,
      iconColor: provider.iconColor,
    };

    if (activeAppCaps.additive) {
      let liveProviderIds: string[] = [];
      try {
        liveProviderIds = await loadLiveProviderIds();
      } catch (error) {
        console.error(
          "[App] Failed to load live provider IDs for duplication",
          error,
        );
        const errorMessage = extractErrorMessage(error);
        toast.error(
          t("provider.duplicateLiveIdsLoadFailed") +
            (errorMessage ? `: ${errorMessage}` : ""),
        );
        return;
      }
      const existingKeys = Array.from(
        new Set([...Object.keys(providers), ...liveProviderIds]),
      );
      duplicatedProvider.providerKey = generateUniqueProviderCopyKey(
        provider.id,
        existingKeys,
      );
      duplicatedProvider.addToLive = false;
    }

    if (provider.sortIndex !== undefined) {
      const updates = Object.values(providers)
        .filter(
          (p) =>
            p.sortIndex !== undefined &&
            p.sortIndex >= newSortIndex! &&
            p.id !== provider.id,
        )
        .map((p) => ({
          id: p.id,
          sortIndex: p.sortIndex! + 1,
        }));

      if (updates.length > 0) {
        try {
          await providersApi.updateSortOrder(updates, activeApp);
        } catch (error) {
          console.error("[App] Failed to update sort order", error);
          toast.error(t("provider.sortUpdateFailed"));
          return; // 如果排序更新失败，不继续添加
        }
      }
    }

    await addProvider(duplicatedProvider);
  };

  const openTerminal = async (provider: Provider) => {
    try {
      const selectedDir = await settingsApi.pickDirectory();
      if (!selectedDir) {
        return;
      }

      await providersApi.openTerminal(provider.id, activeApp, {
        cwd: selectedDir,
      });
      toast.success(t("provider.terminalOpened"));
    } catch (error) {
      console.error("[App] Failed to open terminal", error);
      const errorMessage = extractErrorMessage(error);
      toast.error(
        t("provider.terminalOpenFailed") +
          (errorMessage ? `: ${errorMessage}` : ""),
      );
    }
  };

  const refreshAfterImport = async () => {
    try {
      await queryClient.invalidateQueries({
        queryKey: providerKeys.all,
        refetchType: "all",
      });
      await queryClient.refetchQueries({
        queryKey: providerKeys.all,
        type: "all",
      });
    } catch (error) {
      console.error("[App] Failed to refresh providers after import", error);
      await refetchProviders();
    }
    try {
      await providersApi.updateTrayMenu();
    } catch (error) {
      console.error("[App] Failed to refresh tray menu", error);
    }
  };

  return {
    disableOmo,
    disableOmoSlim,
    openWebsite,
    duplicateProvider,
    openTerminal,
    refreshAfterImport,
  };
}
