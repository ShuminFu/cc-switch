import { useQueryClient } from "@tanstack/react-query";
import { queryKeys } from "@/lib/query/keys";
import { useTranslation } from "react-i18next";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { settingsApi } from "@/lib/api";
import { useSettingsQuery } from "@/lib/query";

/**
 * One-time notice explaining the "common config" snippet. Rendered only by
 * adapters that have the feature (FE-PROV-27).
 */
export function CommonConfigNotice({ enabled }: { enabled: boolean }) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const { data: settingsData } = useSettingsQuery();
  const isOpen =
    enabled &&
    settingsData != null &&
    settingsData.commonConfigConfirmed !== true;

  const confirm = async () => {
    try {
      if (settingsData) {
        const { webdavSync: _, ...rest } = settingsData;
        await settingsApi.save({ ...rest, commonConfigConfirmed: true });
        await queryClient.invalidateQueries({
          queryKey: queryKeys.settings.all,
        });
      }
    } catch (error) {
      console.error("Failed to save commonConfigConfirmed:", error);
    }
  };

  return (
    <ConfirmDialog
      isOpen={isOpen}
      variant="info"
      title={t("confirm.commonConfig.title")}
      message={t("confirm.commonConfig.message")}
      confirmText={t("confirm.commonConfig.confirm")}
      onConfirm={() => void confirm()}
      onCancel={() => void confirm()}
    />
  );
}

interface SoftIssuesDialogProps {
  issues: string[] | null;
  onConfirm: () => void | Promise<void>;
  onCancel: () => void;
}

/** "Save anyway?" confirmation for business-rule (soft) issues. */
export function SoftIssuesDialog({
  issues,
  onConfirm,
  onCancel,
}: SoftIssuesDialogProps) {
  const { t } = useTranslation();
  return (
    <ConfirmDialog
      isOpen={issues !== null && issues.length > 0}
      variant="info"
      title={t("providerForm.softValidation.title", {
        defaultValue: "Configuration has the following issues",
      })}
      message={
        (issues ?? []).map((issue) => `• ${issue}`).join("\n") +
        "\n\n" +
        t("providerForm.softValidation.hint", {
          defaultValue:
            "Save anyway? Switching to this provider may fail; you can fill these in later.",
        })
      }
      confirmText={t("providerForm.softValidation.saveAnyway", {
        defaultValue: "Save anyway",
      })}
      cancelText={t("common.cancel")}
      onConfirm={onConfirm}
      onCancel={onCancel}
    />
  );
}
