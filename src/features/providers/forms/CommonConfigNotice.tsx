import { useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { settingsApi } from "@/lib/api";
import { useSettingsQuery } from "@/lib/query";

/**
 * 首次打开供应商表单时的"通用配置"说明。确认或关闭都记为已读
 * （settings.commonConfigConfirmed），之后不再弹出。
 */
export function CommonConfigNotice() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const { data: settingsData } = useSettingsQuery();
  const showNotice =
    settingsData != null && settingsData.commonConfigConfirmed !== true;

  const handleConfirm = async () => {
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
      isOpen={showNotice}
      variant="info"
      title={t("confirm.commonConfig.title")}
      message={t("confirm.commonConfig.message")}
      confirmText={t("confirm.commonConfig.confirm")}
      onConfirm={() => void handleConfirm()}
      onCancel={() => void handleConfirm()}
    />
  );
}
