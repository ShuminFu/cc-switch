import { useCallback } from "react";
import {
  useMutation,
  useQuery,
  useQueryClient,
  type QueryKey,
} from "@tanstack/react-query";

/**
 * Load and save a backend config blob through React Query (FE-UI-10).
 *
 * `config` stays `undefined` until the backend answered, so callers can keep
 * their controls disabled instead of showing (and later writing back) local
 * defaults when the fetch failed or is still in flight. `update` merges the
 * change into the cached value, saves it, and rolls the cache back when the
 * save fails.
 */
export function useBackendConfig<T extends object>(
  queryKey: QueryKey,
  load: () => Promise<T>,
  save: (config: T) => Promise<unknown>,
  onSaveError?: (error: unknown) => void,
) {
  const queryClient = useQueryClient();
  const query = useQuery({ queryKey, queryFn: load, retry: false });

  const mutation = useMutation({
    mutationFn: save,
    onMutate: async (next: T) => {
      await queryClient.cancelQueries({ queryKey });
      const previous = queryClient.getQueryData<T>(queryKey);
      queryClient.setQueryData(queryKey, next);
      return { previous };
    },
    onError: (error, _next, context) => {
      if (context?.previous !== undefined) {
        queryClient.setQueryData(queryKey, context.previous);
      }
      onSaveError?.(error);
    },
  });

  const { mutate } = mutation;
  const update = useCallback(
    (updates: Partial<T>) => {
      const current = queryClient.getQueryData<T>(queryKey);
      // Never build a payload from defaults: without loaded data there is
      // nothing to merge into.
      if (current === undefined) return;
      mutate({ ...current, ...updates });
    },
    [mutate, queryClient, queryKey],
  );

  return {
    config: query.data,
    isLoading: query.isLoading,
    error: query.error,
    refetch: query.refetch,
    isSaving: mutation.isPending,
    update,
    saveAsync: mutation.mutateAsync,
  };
}
