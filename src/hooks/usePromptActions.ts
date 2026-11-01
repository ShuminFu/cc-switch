import { useCallback, useEffect, useMemo } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { promptsApi, type Prompt, type AppId } from "@/lib/api";
import { queryKeys } from "@/lib/query/keys";

type PromptMap = Record<string, Prompt>;

/** Prompt list for one app (FE-UI-18: React Query instead of local state). */
export function usePromptsQuery(appId: AppId) {
  return useQuery({
    queryKey: queryKeys.prompts.byApp(appId),
    queryFn: () => promptsApi.getPrompts(appId),
  });
}

/**
 * Prompt list + mutations for one app.
 *
 * - Loading is `true` only for the first load; background refetches keep the
 *   list on screen (FE-UI-52).
 * - The enable/disable toggle is optimistic, reads the current cache entry
 *   (not a stale render closure) and rolls back from the snapshot on error.
 *   It never rejects to the caller, so a failure cannot leak an unhandled
 *   promise rejection.
 */
export function usePromptActions(appId: AppId) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const key = queryKeys.prompts.byApp(appId);

  const query = usePromptsQuery(appId);
  const prompts = useMemo<PromptMap>(() => query.data ?? {}, [query.data]);

  useEffect(() => {
    if (query.isError) toast.error(t("prompts.loadFailed"));
    // Toast once per failed fetch, not on every render.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [query.isError, query.errorUpdatedAt]);

  const invalidate = useCallback(
    () =>
      queryClient.invalidateQueries({
        queryKey: queryKeys.prompts.byApp(appId),
      }),
    [queryClient, appId],
  );

  const reload = useCallback(async () => {
    await invalidate();
  }, [invalidate]);

  const saveMutation = useMutation({
    mutationFn: ({ id, prompt }: { id: string; prompt: Prompt }) =>
      promptsApi.upsertPrompt(appId, id, prompt),
    onSuccess: async () => {
      await invalidate();
      toast.success(t("prompts.saveSuccess"), { closeButton: true });
    },
    onError: () => {
      toast.error(t("prompts.saveFailed"));
    },
  });

  const deleteMutation = useMutation({
    mutationFn: (id: string) => promptsApi.deletePrompt(appId, id),
    onSuccess: async () => {
      await invalidate();
      toast.success(t("prompts.deleteSuccess"), { closeButton: true });
    },
    onError: () => {
      toast.error(t("prompts.deleteFailed"));
    },
  });

  const enableMutation = useMutation({
    mutationFn: (id: string) => promptsApi.enablePrompt(appId, id),
    onSuccess: async () => {
      await invalidate();
      toast.success(t("prompts.enableSuccess"), { closeButton: true });
    },
    onError: () => {
      toast.error(t("prompts.enableFailed"));
    },
  });

  const toggleMutation = useMutation({
    mutationFn: async ({ id, enabled }: { id: string; enabled: boolean }) => {
      if (enabled) {
        await promptsApi.enablePrompt(appId, id);
        return;
      }
      // Disable: upsert the cached prompt with enabled=false. The cache is
      // read at call time (FE-UI-52) rather than from a render closure.
      const current = queryClient.getQueryData<PromptMap>(key)?.[id];
      if (!current) throw new Error(`prompt ${id} not found`);
      await promptsApi.upsertPrompt(appId, id, { ...current, enabled: false });
    },
    onMutate: async ({ id, enabled }) => {
      await queryClient.cancelQueries({ queryKey: key });
      const previous = queryClient.getQueryData<PromptMap>(key);
      if (previous) {
        const next: PromptMap = {};
        for (const [promptId, prompt] of Object.entries(previous)) {
          if (enabled) {
            // Enabling is exclusive.
            next[promptId] = { ...prompt, enabled: promptId === id };
          } else {
            next[promptId] =
              promptId === id ? { ...prompt, enabled: false } : prompt;
          }
        }
        queryClient.setQueryData(key, next);
      }
      return { previous };
    },
    onSuccess: (_data, { enabled }) => {
      toast.success(
        enabled ? t("prompts.enableSuccess") : t("prompts.disableSuccess"),
        { closeButton: true },
      );
    },
    onError: (_error, { enabled }, context) => {
      if (context?.previous) queryClient.setQueryData(key, context.previous);
      toast.error(
        enabled ? t("prompts.enableFailed") : t("prompts.disableFailed"),
      );
    },
    onSettled: () => invalidate(),
  });

  const importMutation = useMutation({
    mutationFn: () => promptsApi.importFromFile(appId),
    onSuccess: async () => {
      await invalidate();
      toast.success(t("prompts.importSuccess"), { closeButton: true });
    },
    onError: () => {
      toast.error(t("prompts.importFailed"));
    },
  });

  const savePrompt = useCallback(
    async (id: string, prompt: Prompt) => {
      await saveMutation.mutateAsync({ id, prompt });
    },
    [saveMutation],
  );

  const deletePrompt = useCallback(
    async (id: string) => {
      await deleteMutation.mutateAsync(id);
    },
    [deleteMutation],
  );

  const enablePrompt = useCallback(
    async (id: string) => {
      await enableMutation.mutateAsync(id);
    },
    [enableMutation],
  );

  const toggleEnabled = useCallback(
    (id: string, enabled: boolean) => {
      // Errors are reported (toast) and rolled back in the mutation.
      toggleMutation.mutate({ id, enabled });
    },
    [toggleMutation],
  );

  const importFromFile = useCallback(
    () => importMutation.mutateAsync(),
    [importMutation],
  );

  return {
    prompts,
    /** First load only; background refetches keep the list rendered. */
    loading: query.isLoading,
    reload,
    savePrompt,
    deletePrompt,
    enablePrompt,
    toggleEnabled,
    isToggling: toggleMutation.isPending,
    importFromFile,
  };
}
