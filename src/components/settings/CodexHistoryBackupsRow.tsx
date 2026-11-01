import { useState } from "react";
import { HardDrive, Trash2 } from "lucide-react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import {
  useCodexHistoryBackupUsage,
  usePurgeCodexHistoryBackups,
} from "@/lib/query/adapters";
import { showError } from "@/lib/errors/showError";

export function formatBackupBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

/**
 * Disk used by the Codex history migration backups, with a purge button
 * (CFG-44). The official-history restore ledger is kept unless the user
 * ticks the checkbox in the confirm dialog.
 */
export function CodexHistoryBackupsRow() {
  const { t } = useTranslation();
  const { data: usage } = useCodexHistoryBackupUsage();
  const purge = usePurgeCodexHistoryBackups();
  const [confirmOpen, setConfirmOpen] = useState(false);

  if (!usage) return null;

  const purgeable = usage.migrationBytes + usage.restoreBackupBytes;

  const handlePurge = async (includeRestoreLedger: boolean) => {
    try {
      const after = await purge.mutateAsync(includeRestoreLedger);
      setConfirmOpen(false);
      toast.success(
        t("settings.codexHistoryBackups.purged", {
          defaultValue: "Deleted {{size}} of Codex history backups",
          size: formatBackupBytes(
            Math.max(0, usage.totalBytes - after.totalBytes),
          ),
        }),
      );
    } catch (error) {
      showError({
        title: t("settings.codexHistoryBackups.purgeFailed", {
          defaultValue: "Failed to delete the Codex history backups",
        }),
        error,
      });
    }
  };

  return (
    <div
      className="flex items-center justify-between gap-3 rounded-lg border border-border/50 px-3 py-2"
      data-testid="codex-history-backups"
    >
      <div className="flex min-w-0 items-start gap-2">
        <HardDrive className="mt-0.5 h-4 w-4 flex-shrink-0 text-muted-foreground" />
        <div className="min-w-0 space-y-0.5">
          <p className="text-sm font-medium">
            {t("settings.codexHistoryBackups.title", {
              defaultValue: "History migration backups",
            })}
          </p>
          <p className="text-xs text-muted-foreground">
            {usage.totalBytes === 0
              ? t("settings.codexHistoryBackups.empty", {
                  defaultValue: "No backups on disk.",
                })
              : t("settings.codexHistoryBackups.usage", {
                  defaultValue:
                    "{{total}} on disk: {{purgeable}} safe to delete, {{ledger}} restore ledger.",
                  total: formatBackupBytes(usage.totalBytes),
                  purgeable: formatBackupBytes(purgeable),
                  ledger: formatBackupBytes(usage.restoreLedgerBytes),
                })}
          </p>
        </div>
      </div>
      {usage.totalBytes > 0 && (
        <Button
          type="button"
          size="sm"
          variant="outline"
          onClick={() => setConfirmOpen(true)}
          disabled={purge.isPending}
        >
          <Trash2 className="mr-1 h-3.5 w-3.5" />
          {t("settings.codexHistoryBackups.purge", {
            defaultValue: "Delete backups",
          })}
        </Button>
      )}

      <ConfirmDialog
        isOpen={confirmOpen}
        title={t("settings.codexHistoryBackups.confirmTitle", {
          defaultValue: "Delete Codex history backups?",
        })}
        message={t("settings.codexHistoryBackups.confirmMessage", {
          defaultValue:
            "This deletes the backups taken by the Codex history migrations ({{size}}). Your sessions are not affected.",
          size: formatBackupBytes(purgeable),
        })}
        checkboxLabel={
          usage.restoreLedgerBytes > 0
            ? t("settings.codexHistoryBackups.includeLedger", {
                defaultValue:
                  "Also delete the restore ledger ({{size}}). Turning history unification off can then no longer restore the original history.",
                size: formatBackupBytes(usage.restoreLedgerBytes),
              })
            : undefined
        }
        confirmText={t("settings.codexHistoryBackups.purge", {
          defaultValue: "Delete backups",
        })}
        isPending={purge.isPending}
        onConfirm={(includeLedger) => void handlePurge(includeLedger)}
        onCancel={() => setConfirmOpen(false)}
      />
    </div>
  );
}
