import { useCallback } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import {
  EMPTY_SESSION_ANNOTATION,
  sessionAnnotationsApi,
  type SessionAnnotation,
  type SessionAnnotations,
} from "@/lib/api/sessionAnnotations";
import { queryKeys } from "@/lib/query/keys";
import { extractErrorMessage } from "@/utils/errorUtils";

/** Pin / archive / tag state for the session list (SESS-18). */
export function useSessionAnnotations() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const query = useQuery({
    queryKey: queryKeys.sessions.annotations,
    queryFn: () => sessionAnnotationsApi.list(),
    staleTime: Infinity,
  });

  const mutation = useMutation({
    mutationFn: ({
      sessionKey,
      annotation,
    }: {
      sessionKey: string;
      annotation: SessionAnnotation;
    }) => sessionAnnotationsApi.set(sessionKey, annotation),
    onMutate: async ({ sessionKey, annotation }) => {
      await queryClient.cancelQueries({
        queryKey: queryKeys.sessions.annotations,
      });
      const previous = queryClient.getQueryData<SessionAnnotations>(
        queryKeys.sessions.annotations,
      );
      queryClient.setQueryData<SessionAnnotations>(
        queryKeys.sessions.annotations,
        (current) => ({ ...(current ?? {}), [sessionKey]: annotation }),
      );
      return { previous };
    },
    onError: (error: unknown, _vars, context) => {
      if (context?.previous !== undefined) {
        queryClient.setQueryData(
          queryKeys.sessions.annotations,
          context.previous,
        );
      }
      toast.error(
        extractErrorMessage(error) ||
          t("sessionManager.annotations.saveFailed", {
            defaultValue: "Could not save the session label",
          }),
      );
    },
    onSettled: () =>
      queryClient.invalidateQueries({
        queryKey: queryKeys.sessions.annotations,
      }),
  });

  const annotations = query.data;
  const get = useCallback(
    (sessionKey: string): SessionAnnotation =>
      annotations?.[sessionKey] ?? EMPTY_SESSION_ANNOTATION,
    [annotations],
  );

  const update = useCallback(
    (sessionKey: string, patch: Partial<SessionAnnotation>) => {
      const current = annotations?.[sessionKey] ?? EMPTY_SESSION_ANNOTATION;
      mutation.mutate({ sessionKey, annotation: { ...current, ...patch } });
    },
    [annotations, mutation],
  );

  return { annotations, get, update };
}
