import { useCallback, useMemo } from "react";
import { queryKeys } from "@/lib/query/keys";
import {
  KeyboardSensor,
  PointerSensor,
  useSensor,
  useSensors,
  type DragEndEvent,
} from "@dnd-kit/core";
import { arrayMove, sortableKeyboardCoordinates } from "@dnd-kit/sortable";
import { useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";
import type { Provider } from "@/types";
import { providersApi, type AppId } from "@/lib/api";
import { providerSortLocale, sortProviders } from "@/utils/providerSort";

interface ProvidersCacheData {
  providers: Record<string, Provider>;
  currentProviderId?: string;
}

export function useDragSort(providers: Record<string, Provider>, appId: AppId) {
  const queryClient = useQueryClient();
  const { t, i18n } = useTranslation();

  const sortedProviders = useMemo(
    () => sortProviders(providers, providerSortLocale(i18n.language)),
    [providers, i18n.language],
  );

  const sensors = useSensors(
    useSensor(PointerSensor, {
      activationConstraint: { distance: 8 },
    }),
    useSensor(KeyboardSensor, {
      coordinateGetter: sortableKeyboardCoordinates,
    }),
  );

  const handleDragEnd = useCallback(
    async (event: DragEndEvent) => {
      const { active, over } = event;
      if (!over || active.id === over.id) {
        return;
      }

      const oldIndex = sortedProviders.findIndex(
        (provider) => provider.id === active.id,
      );
      const newIndex = sortedProviders.findIndex(
        (provider) => provider.id === over.id,
      );

      if (oldIndex === -1 || newIndex === -1) {
        return;
      }

      const reordered = arrayMove(sortedProviders, oldIndex, newIndex);
      const updates = reordered.map((provider, index) => ({
        id: provider.id,
        sortIndex: index,
      }));

      // Apply the new order to the cache first so the list does not snap
      // back until the refetch lands; roll back if the backend rejects it
      // (FE-PROV-13).
      const queryKey = queryKeys.providers.byApp(appId);
      await queryClient.cancelQueries({ queryKey });
      const previous = queryClient.getQueryData<ProvidersCacheData>(queryKey);
      if (previous?.providers) {
        const sortIndexById = new Map(
          updates.map((update) => [update.id, update.sortIndex]),
        );
        const nextProviders: Record<string, Provider> = {};
        for (const [id, provider] of Object.entries(previous.providers)) {
          const sortIndex = sortIndexById.get(id);
          nextProviders[id] =
            sortIndex === undefined ? provider : { ...provider, sortIndex };
        }
        queryClient.setQueryData<ProvidersCacheData>(queryKey, {
          ...previous,
          providers: nextProviders,
        });
      }

      try {
        await providersApi.updateSortOrder(updates, appId);
      } catch (error) {
        console.error("Failed to update provider sort order", error);
        if (previous) queryClient.setQueryData(queryKey, previous);
        toast.error(
          t("provider.sortUpdateFailed", {
            defaultValue: "Failed to update sort order",
          }),
        );
        return;
      }

      await queryClient.invalidateQueries({ queryKey });
      // 刷新故障转移队列（因为队列顺序依赖 sort_index）
      await queryClient.invalidateQueries({
        queryKey: queryKeys.failover.queue(appId),
      });

      // 更新托盘菜单以反映新的排序（失败不影响主操作）
      try {
        await providersApi.updateTrayMenu();
      } catch (trayError) {
        console.error("Failed to update tray menu after sort", trayError);
      }
    },
    [sortedProviders, appId, queryClient, t],
  );

  return {
    sortedProviders,
    sensors,
    handleDragEnd,
  };
}
