import { useCallback, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { ConfirmDialog } from "@/components/ConfirmDialog";

export interface UnsavedChangesGuard {
  /** 传给 ProviderForm 的 onDirtyChange */
  onDirtyChange: (isDirty: boolean) => void;
  /** 用户主动关闭（Esc、返回、取消）时调用：有未保存修改则先弹确认 */
  requestClose: () => void;
  /** 确认框，需渲染在面板内部，使其叠在面板之上并优先响应 Esc */
  confirmDialog: ReactNode;
}

/**
 * 添加/编辑供应商面板的未保存修改守卫。
 * 保存成功后的关闭不经过 requestClose，直接调用 onClose 即可。
 */
export function useUnsavedChangesGuard(
  onClose: () => void,
): UnsavedChangesGuard {
  const { t } = useTranslation();
  const [isDirty, setIsDirty] = useState(false);
  const [isConfirmOpen, setIsConfirmOpen] = useState(false);

  const requestClose = useCallback(() => {
    if (isDirty) {
      setIsConfirmOpen(true);
      return;
    }
    onClose();
  }, [isDirty, onClose]);

  const confirmDialog = (
    <ConfirmDialog
      isOpen={isConfirmOpen}
      title={t("providerForm.unsavedChanges.title")}
      message={t("providerForm.unsavedChanges.message")}
      confirmText={t("providerForm.unsavedChanges.discard")}
      cancelText={t("providerForm.unsavedChanges.keepEditing")}
      onConfirm={() => {
        setIsConfirmOpen(false);
        setIsDirty(false);
        onClose();
      }}
      onCancel={() => setIsConfirmOpen(false)}
    />
  );

  return { onDirtyChange: setIsDirty, requestClose, confirmDialog };
}
