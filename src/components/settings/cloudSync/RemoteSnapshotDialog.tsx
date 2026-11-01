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
import type { CloudRemoteSnapshotInfo } from "@/lib/api/settings";
import type { CloudSyncMessages } from "./types";
import { formatDbCompatVersion } from "./useCloudSyncForm";

/** Format an RFC 3339 date string for display; falls back to raw string. */
function formatDate(rfc3339: string): string {
  const d = new Date(rfc3339);
  return Number.isNaN(d.getTime()) ? rfc3339 : d.toLocaleString();
}

interface RemoteSnapshotDialogProps {
  mode: "upload" | "download";
  open: boolean;
  /** Remote snapshot; `null` for an upload to an empty remote. */
  info: CloudRemoteSnapshotInfo | null;
  messages: CloudSyncMessages;
  /** Upload only: where the upload writes. */
  targetPath?: string;
  insecureTransportWarning: string;
  onConfirm: () => void;
  onCancel: () => void;
}

function Code({ children }: { children: React.ReactNode }) {
  return (
    <code className="text-xs bg-muted px-1.5 py-0.5 rounded">{children}</code>
  );
}

/** Preview of the remote snapshot before an upload or a download (restore). */
export function RemoteSnapshotDialog({
  mode,
  open,
  info,
  messages,
  targetPath,
  insecureTransportWarning,
  onConfirm,
  onCancel,
}: RemoteSnapshotDialogProps) {
  const { t } = useTranslation();
  const m =
    mode === "upload" ? messages.confirmUpload : messages.confirmDownload;
  const dbCompat = formatDbCompatVersion(info?.dbCompatVersion);
  const isLegacy = info?.layout === "legacy";

  const details = info && (
    <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1.5 text-xs text-muted-foreground">
      <dt className="font-medium text-foreground">{t(m.deviceName)}</dt>
      <dd>
        <Code>{info.deviceName}</Code>
      </dd>
      <dt className="font-medium text-foreground">{t(m.createdAt)}</dt>
      <dd>{formatDate(info.createdAt)}</dd>
      {info.remotePath && (
        <>
          <dt className="font-medium text-foreground">{t(m.path)}</dt>
          <dd>
            <Code>{info.remotePath}</Code>
          </dd>
        </>
      )}
      {dbCompat && (
        <>
          <dt className="font-medium text-foreground">{t(m.dbCompat)}</dt>
          <dd>{dbCompat}</dd>
        </>
      )}
      {mode === "download" && (
        <>
          <dt className="font-medium text-foreground">
            {t(messages.confirmDownload.artifacts)}
          </dt>
          <dd>{info.artifacts.join(", ")}</dd>
        </>
      )}
    </dl>
  );

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
            {t(m.title)}
          </DialogTitle>
          <DialogDescription asChild>
            <div className="space-y-3 text-sm leading-relaxed">
              {mode === "upload" && (
                <>
                  <p>{t(messages.confirmUpload.content)}</p>
                  <ul className="list-disc pl-5 space-y-1 text-muted-foreground">
                    <li>{t(messages.confirmUpload.dbItem)}</li>
                    <li>{t(messages.confirmUpload.skillsItem)}</li>
                  </ul>
                  <p className="text-muted-foreground">
                    {t(messages.confirmUpload.targetPath)}
                    {": "}
                    <code className="ml-1 text-xs bg-muted px-1.5 py-0.5 rounded">
                      {targetPath}
                    </code>
                  </p>
                </>
              )}
              {mode === "upload" && info ? (
                <div className="rounded-lg border border-border bg-muted/50 p-3 space-y-2">
                  <p className="text-xs font-medium text-foreground">
                    {t(messages.confirmUpload.existingData)}
                  </p>
                  {details}
                </div>
              ) : (
                details
              )}
              {mode === "upload" && info?.remoteChangedSinceLastSync && (
                <p className="font-medium text-amber-600 dark:text-amber-400">
                  {t("settings.cloudSync.remoteChangedNotice", {
                    defaultValue:
                      "Another device uploaded since this device last synced. You will be asked before that data is overwritten.",
                  })}
                </p>
              )}
              {info && isLegacy && (
                <p className="font-medium text-amber-600 dark:text-amber-400">
                  {t(m.legacyNotice)}
                </p>
              )}
              {info?.insecureTransport && (
                <p className="font-medium text-amber-600 dark:text-amber-400">
                  {insecureTransportWarning}
                </p>
              )}
              {(mode === "download" || (info && !isLegacy)) && (
                <p className="text-destructive font-medium">{t(m.warning)}</p>
              )}
            </div>
          </DialogDescription>
        </DialogHeader>
        <DialogFooter className="flex gap-2 border-t-0 bg-transparent pt-2 sm:justify-end">
          <Button variant="outline" onClick={onCancel}>
            {t("common.cancel")}
          </Button>
          <Button variant="destructive" onClick={onConfirm}>
            {t(m.confirm)}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
