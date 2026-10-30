import { useQueryClient } from "@tanstack/react-query";
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
        await queryClient.invalidateQueries({ queryKey: ["settings"] });
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
        defaultValue: "配置存在以下问题",
      })}
      message={
        (issues ?? []).map((issue) => `• ${issue}`).join("\n") +
        "\n\n" +
        t("providerForm.softValidation.hint", {
          defaultValue:
            "仍要保存吗？保存后切换此供应商时可能失败，可以之后再补全。",
        })
      }
      confirmText={t("providerForm.softValidation.saveAnyway", {
        defaultValue: "仍要保存",
      })}
      cancelText={t("common.cancel")}
      onConfirm={onConfirm}
      onCancel={onCancel}
    />
  );
}
