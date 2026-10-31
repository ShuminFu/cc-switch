import type { QueryClient } from "@tanstack/react-query";
import { queryKeys } from "./keys";

/** Refresh the Hermes live-config caches after a provider change. */
export function invalidateHermesProviderCaches(queryClient: QueryClient) {
  return Promise.all([
    queryClient.invalidateQueries({
      queryKey: queryKeys.hermes.liveProviderIds,
    }),
    queryClient.invalidateQueries({ queryKey: queryKeys.hermes.modelConfig }),
  ]);
}
