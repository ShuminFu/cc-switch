import { useTranslation } from "react-i18next";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import type { SessionMeta } from "@/types";
import { formatSessionTitle } from "./utils";

interface SessionDeleteDialogProps {
  deleteTargets: SessionMeta[] | null;
  isDeleting: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}

/** Confirmation for deleting one or several sessions (SESS-25). */
export function SessionDeleteDialog({
  deleteTargets,
  isDeleting,
  onConfirm,
  onCancel,
}: SessionDeleteDialogProps) {
  const { t } = useTranslation();
  return (
    <ConfirmDialog
      isOpen={Boolean(deleteTargets)}
      title={
        deleteTargets && deleteTargets.length > 1
          ? t("sessionManager.batchDeleteConfirmTitle", {
              defaultValue: "批量删除会话",
            })
          : t("sessionManager.deleteConfirmTitle", {
              defaultValue: "删除会话",
            })
      }
      message={
        deleteTargets && deleteTargets.length > 1
          ? t("sessionManager.batchDeleteConfirmMessage", {
              defaultValue:
                "将永久删除已选中的 {{count}} 个本地会话记录。\n\n此操作不可恢复。",
              count: deleteTargets.length,
            })
          : deleteTargets?.[0]
            ? t("sessionManager.deleteConfirmMessage", {
                defaultValue:
                  "将永久删除本地会话“{{title}}”\nSession ID: {{sessionId}}\n\n此操作不可恢复。",
                title: formatSessionTitle(deleteTargets[0]),
                sessionId: deleteTargets[0].sessionId,
              })
            : ""
      }
      confirmText={
        deleteTargets && deleteTargets.length > 1
          ? t("sessionManager.batchDeleteConfirmAction", {
              defaultValue: "删除所选会话",
            })
          : t("sessionManager.deleteConfirmAction", {
              defaultValue: "删除会话",
            })
      }
      cancelText={t("common.cancel", { defaultValue: "取消" })}
      variant="destructive"
      onConfirm={onConfirm}
      onCancel={() => {
        if (!isDeleting) {
          onCancel();
        }
      }}
    />
  );
}
