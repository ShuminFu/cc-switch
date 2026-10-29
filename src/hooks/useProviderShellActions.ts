import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { useQueryClient, type QueryKey } from "@tanstack/react-query";
import type { Provider } from "@/types";
import { providersApi, settingsApi, type AppId } from "@/lib/api";
import type { useProviderActions } from "@/hooks/useProviderActions";
import { openclawKeys } from "@/hooks/useOpenClaw";
import { hermesKeys } from "@/hooks/useHermes";
import {
  useDisableCurrentOmo,
  useDisableCurrentOmoSlim,
} from "@/lib/query/omo";
import { useAppCaps } from "@/entities/app/registry";
import { providerKeys } from "@/entities/provider/keys";
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

/** 累加模式应用读取 live 配置中已有供应商 ID 的查询 */
const LIVE_PROVIDER_IDS_QUERIES: Partial<
  Record<AppId, { queryKey: QueryKey; queryFn: () => Promise<string[]> }>
> = {
  opencode: {
    queryKey: providerKeys.opencodeLiveIds,
    queryFn: () => providersApi.getOpenCodeLiveProviderIds(),
  },
  openclaw: {
    queryKey: openclawKeys.liveProviderIds,
    queryFn: () => providersApi.getOpenClawLiveProviderIds(),
  },
  hermes: {
    queryKey: hermesKeys.liveProviderIds,
    queryFn: () => providersApi.getHermesLiveProviderIds(),
  },
};

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
      toast.success(t("omo.disabled", { defaultValue: "OMO 已停用" }));
    },
    onError: (error: Error) => {
      toast.error(
        t("omo.disableFailed", {
          defaultValue: "停用 OMO 失败: {{error}}",
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
        extractErrorMessage(error) ||
        t("notifications.openLinkFailed", {
          defaultValue: "链接打开失败",
        });
      toast.error(detail);
    }
  };

  const loadLiveProviderIds = (): Promise<string[]> => {
    const query = LIVE_PROVIDER_IDS_QUERIES[activeApp];
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
          t("provider.duplicateLiveIdsLoadFailed", {
            defaultValue: "读取配置中的供应商标识失败，请先修复配置后再试",
          }) + (errorMessage ? `: ${errorMessage}` : ""),
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
          toast.error(
            t("provider.sortUpdateFailed", {
              defaultValue: "排序更新失败",
            }),
          );
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
      toast.success(
        t("provider.terminalOpened", {
          defaultValue: "终端已打开",
        }),
      );
    } catch (error) {
      console.error("[App] Failed to open terminal", error);
      const errorMessage = extractErrorMessage(error);
      toast.error(
        t("provider.terminalOpenFailed", {
          defaultValue: "打开终端失败",
        }) + (errorMessage ? `: ${errorMessage}` : ""),
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
