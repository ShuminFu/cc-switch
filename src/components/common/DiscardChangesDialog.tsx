import { useTranslation } from "react-i18next";
import { ConfirmDialog } from "@/components/ConfirmDialog";

interface DiscardChangesDialogProps {
  isOpen: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}

/** "Discard unsaved changes?" confirmation shown by `useCloseGuard`. */
export function DiscardChangesDialog({
  isOpen,
  onConfirm,
  onCancel,
}: DiscardChangesDialogProps) {
  const { t } = useTranslation();
  return (
    <ConfirmDialog
      isOpen={isOpen}
      title={t("common.discardChangesTitle")}
      message={t("common.discardChangesMessage")}
      confirmText={t("common.discardChanges")}
      zIndex="top"
      onConfirm={() => onConfirm()}
      onCancel={onCancel}
    />
  );
}
