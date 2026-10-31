import { useCallback, useRef } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import type { Provider } from "@/types";
import { providersApi, type AppId } from "@/lib/api";
import { liveProviderIdsKey, queryKeys } from "@/lib/query/keys";
import { appHas } from "@/config/apps";
import { extractErrorMessage } from "@/utils/errorUtils";
import {
  buildDuplicateProvider,
  generateUniqueProviderCopyKey,
  sortShiftForInsert,
  type DuplicateProviderInput,
} from "@/utils/providerDuplicate";

const liveProviderIdsFetcher = (appId: AppId) => {
  switch (appId) {
    case "opencode":
      return () => providersApi.getOpenCodeLiveProviderIds();
    case "openclaw":
      return () => providersApi.getOpenClawLiveProviderIds();
    case "hermes":
      return () => providersApi.getHermesLiveProviderIds();
    default:
      return null;
  }
};

/**
 * Duplicate a provider (FE-ARCH-43). The copy is added first; sibling sort
 * indices are shifted only after the add succeeded, against a fresh provider
 * list, so a failed add leaves the order untouched. Duplicates run one at a
 * time so two quick clicks cannot race on the same stale map.
 */
export function useDuplicateProvider(
  activeApp: AppId,
  providers: Record<string, Provider>,
  addProvider: (provider: DuplicateProviderInput) => Promise<Provider | void>,
) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const chain = useRef<Promise<unknown>>(Promise.resolve());

  const run = useCallback(
    async (provider: Provider) => {
      const duplicated = buildDuplicateProvider(
        provider,
        t("provider.duplicateName", {
          name: provider.name,
          defaultValue: "{{name}} copy",
        }),
      );

      const liveKey = liveProviderIdsKey(activeApp);
      const liveFetcher = liveProviderIdsFetcher(activeApp);
      if (appHas(activeApp, "additiveLiveConfig") && liveKey && liveFetcher) {
        let liveProviderIds: string[] = [];
        try {
          liveProviderIds = await queryClient.ensureQueryData({
            queryKey: liveKey,
            queryFn: liveFetcher,
          });
        } catch (error) {
          console.error(
            "[Providers] Failed to load live provider IDs for duplication",
            error,
          );
          const errorMessage = extractErrorMessage(error);
          toast.error(
            t("provider.duplicateLiveIdsLoadFailed", {
              defaultValue:
                "Failed to read provider keys from the config. Fix the config and try again",
            }) + (errorMessage ? `: ${errorMessage}` : ""),
          );
          return;
        }
        duplicated.providerKey = generateUniqueProviderCopyKey(
          provider.id,
          Array.from(new Set([...Object.keys(providers), ...liveProviderIds])),
        );
        duplicated.addToLive = false;
      }

      // addProvider toasts its own failure; nothing else has changed yet.
      let created: Provider | void;
      try {
        created = await addProvider(duplicated);
      } catch {
        return;
      }

      const insertedIndex = duplicated.sortIndex;
      if (insertedIndex === undefined) return;

      try {
        const fresh = await providersApi.getAll(activeApp);
        const excludeIds = [provider.id];
        if (created && created.id) excludeIds.push(created.id);
        const updates = sortShiftForInsert(fresh, insertedIndex, excludeIds);
        if (updates.length > 0) {
          await providersApi.updateSortOrder(updates, activeApp);
          await queryClient.invalidateQueries({
            queryKey: queryKeys.providers.byApp(activeApp),
          });
        }
      } catch (error) {
        console.error("[Providers] Failed to update sort order", error);
        toast.error(
          t("provider.sortUpdateFailed", {
            defaultValue: "Failed to update the sort order",
          }),
        );
      }
    },
    [activeApp, addProvider, providers, queryClient, t],
  );

  return useCallback(
    (provider: Provider) => {
      const next = chain.current.then(() => run(provider));
      chain.current = next.catch(() => undefined);
      return next;
    },
    [run],
  );
}
