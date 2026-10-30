import { AlertTriangle } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import type { SyncConflictKind } from "@/lib/errors/syncConflict";
import type { TransferKind } from "./useCloudSyncForm";

interface SyncConflictDialogProps {
  open: boolean;
  operation: TransferKind;
  conflict: SyncConflictKind;
  onOverwrite: () => void;
  onCancel: () => void;
}

/**
 * Shown when a non-forced manual transfer was refused because the other side
 * changed since the last sync. Overwriting retries with `force: true`.
 */
export function SyncConflictDialog({
  open,
  operation,
  conflict,
  onOverwrite,
  onCancel,
}: SyncConflictDialogProps) {
  const { t } = useTranslation();

  const message =
    conflict === "remote_changed"
      ? t("settings.cloudSync.conflict.remoteChanged", {
          defaultValue:
            "The cloud data was updated by another device since this device last synced. Uploading now replaces that data with this device's data. To keep it, cancel and download first.",
        })
      : t("settings.cloudSync.conflict.localChanged", {
          defaultValue:
            "Local data changed since the last sync. Downloading now replaces those local changes with the cloud data. To keep them, cancel and upload first.",
        });

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next) onCancel();
      }}
    >
      <DialogContent className="max-w-sm" zIndex="alert">
        <DialogHeader className="space-y-3 border-b-0 bg-transparent pb-0">
          <DialogTitle className="flex items-center gap-2 text-lg font-semibold">
            <AlertTriangle className="h-5 w-5 text-destructive" />
            {t("settings.cloudSync.conflict.title", {
              defaultValue: "Sync conflict",
            })}
          </DialogTitle>
          <DialogDescription asChild>
            <div className="space-y-3 text-sm leading-relaxed">
              <p>{message}</p>
            </div>
          </DialogDescription>
        </DialogHeader>
        <DialogFooter className="flex gap-2 border-t-0 bg-transparent pt-2 sm:justify-end">
          <Button variant="outline" onClick={onCancel}>
            {t("common.cancel")}
          </Button>
          <Button variant="destructive" onClick={onOverwrite}>
            {operation === "upload"
              ? t("settings.cloudSync.conflict.overwriteRemote", {
                  defaultValue: "Overwrite cloud data",
                })
              : t("settings.cloudSync.conflict.overwriteLocal", {
                  defaultValue: "Overwrite local data",
                })}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
