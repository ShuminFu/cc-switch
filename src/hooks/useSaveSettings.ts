import { useCallback } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { useQueryClient } from "@tanstack/react-query";
import { useSaveSettingsMutation } from "@/lib/query";
import type { Settings } from "@/types";
import { sanitizeDir } from "./settingsDirectoryFields";
import {
  buildSettingsPayload,
  type SettingsFormState,
} from "./settingsPayload";
import { runSettingsSideEffects, type SaveScope } from "./settingsSideEffects";

export interface SaveResult {
  requiresRestart: boolean;
}

export interface SaveSettingsRequest {
  scope: SaveScope;
  form: SettingsFormState | null;
  updates?: Partial<SettingsFormState>;
  /** Closure copy of the settings query, used when the cache is empty. */
  data?: Settings | null;
  /** Current and last-persisted app config dir override (full scope). */
  appConfigDir?: string;
  initialAppConfigDir?: string;
  silent?: boolean;
}

/**
 * The single settings save pipeline shared by autosave and explicit save:
 * build payload → persist → run the side-effect table for the scope.
 */
export function useSaveSettings() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const saveMutation = useSaveSettingsMutation();

  const save = useCallback(
    async (request: SaveSettingsRequest): Promise<SaveResult | null> => {
      const { scope, data } = request;
      const updates = request.updates ?? {};
      const form = request.form ? { ...request.form, ...updates } : null;
      if (!form) return null;

      try {
        // Capture the persisted state from the live cache before mutating:
        // the closure's `data` can lag behind quick successive toggles.
        const prev =
          queryClient.getQueryData<Settings>(["settings"]) ?? data ?? null;
        // Autosave keeps the persisted directory overrides: the Advanced
        // tab's inputs are drafts until saved explicitly.
        const payload = buildSettingsPayload(
          form,
          scope === "full" ? form : prev,
        );
        const appConfigDir = sanitizeDir(request.appConfigDir);

        await saveMutation.mutateAsync(payload);
        await runSettingsSideEffects({
          scope,
          next: payload,
          prev,
          updates,
          appConfigDir,
          t,
          providersSynced: false,
        });

        if (scope === "auto") return { requiresRestart: false };

        const requiresRestart =
          appConfigDir !== (request.initialAppConfigDir ?? undefined);
        if (!request.silent) {
          toast.success(
            t("notifications.settingsSaved", { defaultValue: "设置已保存" }),
            { closeButton: true },
          );
        }
        return { requiresRestart };
      } catch (error) {
        console.error(
          scope === "auto"
            ? "[useSettings] Failed to auto-save settings"
            : "[useSettings] Failed to save settings",
          error,
        );
        toast.error(
          t("notifications.settingsSaveFailed", {
            defaultValue: "保存设置失败: {{error}}",
            error: (error as Error)?.message ?? String(error),
          }),
        );
        throw error;
      }
    },
    [queryClient, saveMutation, t],
  );

  return { save, isSaving: saveMutation.isPending };
}
