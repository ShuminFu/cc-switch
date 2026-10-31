import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { profilesApi, providersApi } from "@/lib/api";
import type { ProfileScope } from "@/lib/api/profiles";
import { extractErrorMessage } from "@/utils/errorUtils";
import { queryKeys } from "./keys";

const updateTrayMenuSafely = async () => {
  try {
    await providersApi.updateTrayMenu();
  } catch (trayError) {
    console.error("Failed to update tray menu after profile change", trayError);
  }
};

export const useProfilesQuery = () => {
  return useQuery({
    queryKey: queryKeys.profiles.all,
    queryFn: () => profilesApi.list(),
  });
};

export const useCreateProfileMutation = () => {
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  return useMutation({
    mutationFn: ({ name, scope }: { name: string; scope: ProfileScope }) =>
      profilesApi.create(name, scope),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: queryKeys.profiles.all });
      await updateTrayMenuSafely();
      toast.success(t("profiles.createSuccess"), { closeButton: true });
    },
    onError: (error: unknown) => {
      const detail = extractErrorMessage(error) || t("common.unknown");
      toast.error(t("profiles.createFailed", { detail }), {
        closeButton: true,
      });
    },
  });
};

export const useUpdateProfileMutation = () => {
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  return useMutation({
    mutationFn: ({
      id,
      name,
      resnapshot,
      scope,
    }: {
      id: string;
      name?: string;
      resnapshot?: boolean;
      scope?: ProfileScope;
    }) => profilesApi.update(id, { name, resnapshot, scope }),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: queryKeys.profiles.all });
      await updateTrayMenuSafely();
      toast.success(t("profiles.updateSuccess"), { closeButton: true });
    },
    onError: (error: unknown) => {
      const detail = extractErrorMessage(error) || t("common.unknown");
      toast.error(t("profiles.updateFailed", { detail }), {
        closeButton: true,
      });
    },
  });
};

export const useDeleteProfileMutation = () => {
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  return useMutation({
    mutationFn: (id: string) => profilesApi.delete(id),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: queryKeys.profiles.all });
      await updateTrayMenuSafely();
      toast.success(t("profiles.deleteSuccess"), { closeButton: true });
    },
    onError: (error: unknown) => {
      const detail = extractErrorMessage(error) || t("common.unknown");
      toast.error(t("profiles.deleteFailed", { detail }), {
        closeButton: true,
      });
    },
  });
};

export const useClearProfileMutation = () => {
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  return useMutation({
    mutationFn: (scope: ProfileScope) => profilesApi.clearCurrent(scope),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: queryKeys.profiles.all });
      await updateTrayMenuSafely();
      toast.success(t("profiles.clearSuccess"), { closeButton: true });
    },
    onError: (error: unknown) => {
      const detail = extractErrorMessage(error) || t("common.unknown");
      toast.error(t("profiles.applyFailed", { detail }), {
        closeButton: true,
      });
    },
  });
};

export const useApplyProfileMutation = () => {
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  return useMutation({
    mutationFn: ({ id, scope }: { id: string; scope: ProfileScope }) =>
      profilesApi.apply(id, scope),
    onSuccess: async (warnings) => {
      await Promise.all(
        [
          queryKeys.profiles.all,
          queryKeys.providers.byApp("claude"),
          queryKeys.providers.byApp("claude-desktop"),
          queryKeys.providers.byApp("codex"),
          queryKeys.mcp.all,
          queryKeys.skills.all,
        ].map((queryKey) => queryClient.invalidateQueries({ queryKey })),
      );
      await updateTrayMenuSafely();

      if (warnings.length > 0) {
        toast.warning(
          t("profiles.applyWarnings", {
            warningCount: warnings.length,
            details: warnings.join("\n"),
          }),
          { closeButton: true, duration: 10000 },
        );
      } else {
        toast.success(t("profiles.applySuccess"), { closeButton: true });
      }
    },
    onError: (error: unknown) => {
      const detail = extractErrorMessage(error) || t("common.unknown");
      toast.error(t("profiles.applyFailed", { detail }), {
        closeButton: true,
      });
    },
  });
};
