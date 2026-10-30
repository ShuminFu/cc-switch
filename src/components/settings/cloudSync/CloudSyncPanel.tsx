import { useEffect, type ReactNode } from "react";
import { Check, DownloadCloud, Link2, Save, UploadCloud } from "lucide-react";
import { useTranslation } from "react-i18next";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import type { WebDavSyncStatus } from "@/types";
import { ActionButton } from "./ActionButton";
import { RemoteSnapshotDialog } from "./RemoteSnapshotDialog";
import { SyncConflictDialog } from "./SyncConflictDialog";
import { SyncFormFields } from "./SyncFormFields";
import type { CloudSyncBackendAdapter, SyncFormValues } from "./types";
import {
  useCloudSyncForm,
  type UseCloudSyncFormOptions,
} from "./useCloudSyncForm";

interface CloudSyncPanelProps<
  F extends SyncFormValues,
  C extends { status?: WebDavSyncStatus },
> extends UseCloudSyncFormOptions {
  adapter: CloudSyncBackendAdapter<F, C>;
  config?: C;
  /** Backend-specific extra options rendered under the form fields. */
  extraFields?: ReactNode;
  /** Reports whether an action (test/save/transfer) is running. */
  onBusyChange?: (busy: boolean) => void;
}

/** Backend-agnostic cloud sync form: config, status, upload/download. */
export function CloudSyncPanel<
  F extends SyncFormValues,
  C extends { status?: WebDavSyncStatus },
>({
  adapter,
  config,
  extraFields,
  onBusyChange,
  ...options
}: CloudSyncPanelProps<F, C>) {
  const { t } = useTranslation();
  const sync = useCloudSyncForm(adapter, config, options);
  const m = adapter.messages;
  const { actionState, dialog } = sync;
  const busy = actionState !== "idle";
  const hasSavedConfig = adapter.hasSavedConfig(config);

  useEffect(() => {
    onBusyChange?.(busy);
  }, [busy, onBusyChange]);

  const lastSyncAt = config?.status?.lastSyncAt;
  const lastSyncDisplay = lastSyncAt
    ? new Date(lastSyncAt * 1000).toLocaleString()
    : null;
  const lastError = config?.status?.lastError?.trim();
  const showAutoSyncError =
    !!lastError && config?.status?.lastErrorSource === "auto";
  const fetching = actionState === "fetching_remote";

  return (
    <div className="space-y-4 rounded-lg border border-border bg-muted/40 p-6">
      <SyncFormFields
        adapter={adapter}
        form={sync.form}
        disabled={busy}
        onChange={sync.setField}
        onBlur={sync.blurField}
        onPresetChange={sync.changePreset}
      />
      {extraFields}

      {lastSyncDisplay && (
        <p className="text-xs text-muted-foreground">
          {t(m.lastSync, { time: lastSyncDisplay })}
        </p>
      )}
      {showAutoSyncError && (
        <div className="rounded-lg border border-red-300/70 bg-red-50/80 px-3 py-2 text-xs text-red-900 dark:border-red-500/50 dark:bg-red-950/30 dark:text-red-200">
          <p className="font-medium">{t(m.autoSyncLastErrorTitle)}</p>
          <p className="mt-1 break-all whitespace-pre-wrap">{lastError}</p>
          <p className="mt-1 text-[11px] text-red-700/90 dark:text-red-300/80">
            {t(m.autoSyncLastErrorHint)}
          </p>
        </div>
      )}

      <div className="flex flex-wrap items-center gap-3 pt-2">
        <ActionButton
          type="button"
          variant="outline"
          size="sm"
          onClick={sync.test}
          actionState={actionState}
          targetState="testing"
          icon={Link2}
          activeLabel={t(m.testing)}
          idleLabel={t(m.test)}
        />
        <ActionButton
          type="button"
          variant="outline"
          size="sm"
          onClick={sync.save}
          actionState={actionState}
          targetState="saving"
          icon={Save}
          activeLabel={t(m.saving)}
          idleLabel={t(m.save)}
        />
        {sync.dirty && (
          <span className="inline-flex items-center gap-1.5 text-xs text-amber-500 dark:text-amber-400 animate-in fade-in duration-200">
            <span className="h-1.5 w-1.5 rounded-full bg-amber-500 dark:bg-amber-400" />
            {t(m.unsaved)}
          </span>
        )}
        {!sync.dirty && sync.justSaved && (
          <span className="inline-flex items-center gap-1.5 text-xs text-emerald-600 dark:text-emerald-400 animate-in fade-in duration-200">
            <Check className="h-3 w-3" />
            {t(m.saved)}
          </span>
        )}
      </div>

      <div className="flex flex-wrap items-center gap-3 border-t border-border pt-4">
        <ActionButton
          type="button"
          size="sm"
          onClick={sync.startUpload}
          disabled={!hasSavedConfig}
          actionState={actionState}
          targetState="uploading"
          alsoActiveFor={["fetching_remote"]}
          icon={UploadCloud}
          activeLabel={t(fetching ? m.fetchingRemote : m.uploading)}
          idleLabel={t(m.upload)}
        />
        <ActionButton
          type="button"
          variant="secondary"
          size="sm"
          onClick={sync.startDownload}
          disabled={!hasSavedConfig}
          actionState={actionState}
          targetState="downloading"
          alsoActiveFor={["fetching_remote"]}
          icon={DownloadCloud}
          activeLabel={t(fetching ? m.fetchingRemote : m.downloading)}
          idleLabel={t(m.download)}
        />
      </div>
      {!hasSavedConfig && (
        <p className="text-xs text-muted-foreground">{t(m.saveBeforeSync)}</p>
      )}

      <RemoteSnapshotDialog
        mode="upload"
        open={dialog?.type === "upload"}
        info={dialog?.type === "upload" ? dialog.info : null}
        messages={m}
        targetPath={adapter.buildRemotePath(sync.form)}
        insecureTransportWarning={sync.insecureTransportWarning}
        onConfirm={sync.confirmUpload}
        onCancel={sync.closeDialog}
      />
      <RemoteSnapshotDialog
        mode="download"
        open={dialog?.type === "download"}
        info={dialog?.type === "download" ? dialog.info : null}
        messages={m}
        insecureTransportWarning={sync.insecureTransportWarning}
        onConfirm={sync.confirmDownload}
        onCancel={sync.closeDialog}
      />
      {dialog?.type === "conflict" && (
        <SyncConflictDialog
          open
          operation={dialog.operation}
          conflict={dialog.conflict}
          onOverwrite={sync.forceTransfer}
          onCancel={sync.closeDialog}
        />
      )}
      <ConfirmDialog
        isOpen={sync.showAutoSyncConfirm}
        variant="info"
        title={t("confirm.autoSync.title")}
        message={t("confirm.autoSync.message")}
        confirmText={t("confirm.autoSync.confirm")}
        onConfirm={() => void sync.confirmAutoSync()}
        onCancel={sync.cancelAutoSync}
      />
    </div>
  );
}
