import { useMemo, useState } from "react";
import {
  AlertCircle,
  CheckCircle2,
  FolderOpen,
  Loader2,
  RotateCcw,
  Save,
  XCircle,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { useTranslation } from "react-i18next";
import type { ImportStatus } from "@/hooks/useImportExport";

interface ImportExportSectionProps {
  status: ImportStatus;
  selectedFile: string;
  errorMessage: string | null;
  backupId: string | null;
  isImporting: boolean;
  onSelectFile: () => Promise<void>;
  onImport: () => Promise<void>;
  onExport: () => Promise<void>;
  onClear: () => void;
  /**
   * Restore the safety backup taken right before the last import
   * (FE-UI-55). The button is shown only when this is provided.
   */
  onRevertImport?: (backupId: string) => Promise<void>;
}

export function ImportExportSection({
  status,
  selectedFile,
  errorMessage,
  backupId,
  isImporting,
  onSelectFile,
  onImport,
  onExport,
  onClear,
  onRevertImport,
}: ImportExportSectionProps) {
  const { t } = useTranslation();
  const [confirmImportOpen, setConfirmImportOpen] = useState(false);
  const [confirmRevertOpen, setConfirmRevertOpen] = useState(false);
  const [isReverting, setIsReverting] = useState(false);
  const [revertedBackupId, setRevertedBackupId] = useState<string | null>(null);

  const selectedFileName = useMemo(() => {
    if (!selectedFile) return "";
    const segments = selectedFile.split(/[\\/]/);
    return segments[segments.length - 1] || selectedFile;
  }, [selectedFile]);

  return (
    <section className="space-y-4">
      <header className="space-y-2">
        <h3 className="text-base font-semibold text-foreground">
          {t("settings.importExport")}
        </h3>
        <p className="text-sm text-muted-foreground">
          {t("settings.importExportHint")}
        </p>
      </header>

      <div className="space-y-4 rounded-lg border border-border bg-muted/40 p-6">
        {/* Import and Export Buttons Side by Side */}
        <div className="grid grid-cols-2 gap-4 items-stretch">
          {/* Import Button */}
          <div className="relative">
            <Button
              type="button"
              className={`w-full h-auto py-3 px-4 bg-blue-500 hover:bg-blue-600 dark:bg-blue-600 dark:hover:bg-blue-700 text-white ${selectedFile && !isImporting ? "flex-col items-start" : "items-center"}`}
              onClick={
                !selectedFile ? onSelectFile : () => setConfirmImportOpen(true)
              }
              disabled={isImporting}
            >
              <div className="flex items-center gap-2 w-full justify-center">
                {isImporting ? (
                  <Loader2 className="h-4 w-4 animate-spin flex-shrink-0" />
                ) : selectedFile ? (
                  <CheckCircle2 className="h-4 w-4 flex-shrink-0" />
                ) : (
                  <FolderOpen className="h-4 w-4 flex-shrink-0" />
                )}
                <span className="font-medium">
                  {isImporting
                    ? t("settings.importing")
                    : selectedFile
                      ? t("settings.import")
                      : t("settings.selectConfigFile")}
                </span>
              </div>
              {selectedFile && !isImporting && (
                <div className="mt-2 w-full text-left">
                  <p className="text-xs font-mono text-white/80 truncate">
                    📄 {selectedFileName}
                  </p>
                </div>
              )}
            </Button>
            {selectedFile && (
              <button
                type="button"
                onClick={onClear}
                className="absolute -top-2 -right-2 h-6 w-6 rounded-full bg-red-500 hover:bg-red-600 text-white flex items-center justify-center shadow-lg transition-colors z-10"
                aria-label={t("common.clear")}
              >
                <XCircle className="h-4 w-4" />
              </button>
            )}
          </div>

          {/* Export Button */}
          <div>
            <Button
              type="button"
              className="w-full h-full py-3 px-4 bg-blue-500 hover:bg-blue-600 dark:bg-blue-600 dark:hover:bg-blue-700 text-white items-center"
              onClick={onExport}
            >
              <Save className="mr-2 h-4 w-4" />
              {t("settings.exportConfig")}
            </Button>
          </div>
        </div>

        <ImportStatusMessage
          status={status}
          errorMessage={errorMessage}
          backupId={backupId}
        />

        {onRevertImport &&
          backupId &&
          (status === "success" || status === "partial-success") &&
          revertedBackupId !== backupId && (
            <Button
              type="button"
              variant="outline"
              size="sm"
              disabled={isReverting}
              onClick={() => setConfirmRevertOpen(true)}
            >
              {isReverting ? (
                <Loader2 className="mr-2 h-4 w-4 animate-spin" />
              ) : (
                <RotateCcw className="mr-2 h-4 w-4" />
              )}
              {t("settings.revertImport", {
                defaultValue: "Revert import",
              })}
            </Button>
          )}
      </div>

      <ConfirmDialog
        isOpen={confirmImportOpen}
        title={t("settings.importConfirmTitle", {
          defaultValue: "Replace all current data?",
        })}
        message={t("settings.importConfirmMessage", {
          defaultValue:
            "Importing {{file}} replaces all providers, MCP servers, prompts and settings, and rewrites the live CLI config files. A safety backup is taken first so the import can be reverted.",
          file: selectedFileName,
        })}
        confirmText={t("settings.import")}
        onConfirm={() => {
          setConfirmImportOpen(false);
          void onImport();
        }}
        onCancel={() => setConfirmImportOpen(false)}
      />

      {onRevertImport && backupId && (
        <ConfirmDialog
          isOpen={confirmRevertOpen}
          title={t("settings.revertImportConfirmTitle", {
            defaultValue: "Revert the import?",
          })}
          message={t("settings.revertImportConfirmMessage", {
            defaultValue:
              "Restore the backup {{backupId}} taken right before the import? Changes made since the import are lost.",
            backupId,
          })}
          confirmText={t("settings.revertImport", {
            defaultValue: "Revert import",
          })}
          onConfirm={() => {
            setConfirmRevertOpen(false);
            setIsReverting(true);
            onRevertImport(backupId)
              .then(() => setRevertedBackupId(backupId))
              .catch(() => {
                // The caller reports the error; keep the button for a retry.
              })
              .finally(() => setIsReverting(false));
          }}
          onCancel={() => setConfirmRevertOpen(false)}
        />
      )}
    </section>
  );
}

interface ImportStatusMessageProps {
  status: ImportStatus;
  errorMessage: string | null;
  backupId: string | null;
}

function ImportStatusMessage({
  status,
  errorMessage,
  backupId,
}: ImportStatusMessageProps) {
  const { t } = useTranslation();

  if (status === "idle") {
    return null;
  }

  const baseClass =
    "flex items-start gap-3 rounded-xl border p-4 text-sm leading-relaxed backdrop-blur-sm";

  if (status === "importing") {
    return (
      <div
        className={`${baseClass} border-blue-500/30 bg-blue-500/10 text-blue-600 dark:text-blue-400`}
      >
        <Loader2 className="mt-0.5 h-5 w-5 flex-shrink-0 animate-spin" />
        <div>
          <p className="font-semibold">{t("settings.importing")}</p>
          <p className="text-blue-600/80 dark:text-blue-400/80">
            {t("common.loading")}
          </p>
        </div>
      </div>
    );
  }

  if (status === "success") {
    return (
      <div
        className={`${baseClass} border-green-500/30 bg-green-500/10 text-green-700 dark:text-green-400`}
      >
        <CheckCircle2 className="mt-0.5 h-5 w-5 flex-shrink-0" />
        <div className="space-y-1.5">
          <p className="font-semibold">{t("settings.importSuccess")}</p>
          {backupId ? (
            <p className="text-xs text-green-600/80 dark:text-green-400/80">
              {t("settings.backupId")}: {backupId}
            </p>
          ) : null}
          <p className="text-green-600/80 dark:text-green-400/80">
            {t("settings.autoReload")}
          </p>
        </div>
      </div>
    );
  }

  if (status === "partial-success") {
    return (
      <div
        className={`${baseClass} border-yellow-500/30 bg-yellow-500/10 text-yellow-700 dark:text-yellow-400`}
      >
        <AlertCircle className="mt-0.5 h-5 w-5 flex-shrink-0" />
        <div className="space-y-1.5">
          <p className="font-semibold">{t("settings.importPartialSuccess")}</p>
          <p className="text-yellow-600/80 dark:text-yellow-400/80">
            {t("settings.importPartialHint")}
          </p>
        </div>
      </div>
    );
  }

  const message = errorMessage || t("settings.importFailed");

  return (
    <div
      className={`${baseClass} border-red-500/30 bg-red-500/10 text-red-600 dark:text-red-400`}
    >
      <AlertCircle className="mt-0.5 h-5 w-5 flex-shrink-0" />
      <div className="space-y-1.5">
        <p className="font-semibold">{t("settings.importFailed")}</p>
        <p className="text-red-600/80 dark:text-red-400/80">{message}</p>
      </div>
    </div>
  );
}
