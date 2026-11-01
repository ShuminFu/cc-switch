import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { History, Loader2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { extractErrorMessage } from "@/utils/errorUtils";
import { dbRecoveryApi, type RecoveryBackup } from "./api";

const fmtSize = (bytes: number) =>
  bytes < 1024 * 1024
    ? `${(bytes / 1024).toFixed(0)} KB`
    : `${(bytes / (1024 * 1024)).toFixed(1)} MB`;

/**
 * "Database too new" screen: backups this version can open (usually the one
 * taken before the newer version migrated), with one-click restore. The
 * newer database is kept in the backups folder and the app restarts.
 * Rendered outside the QueryClientProvider (the app never boots normally in
 * this state), so it loads with a plain effect.
 */
export function RecoveryBackupList({
  disabled = false,
}: {
  disabled?: boolean;
}) {
  const { t } = useTranslation();
  const [restoring, setRestoring] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [data, setData] = useState<RecoveryBackup[]>([]);
  useEffect(() => {
    let cancelled = false;
    dbRecoveryApi
      .list()
      .then((list) => {
        if (!cancelled) setData(list);
      })
      .catch(() => {
        // Listing is best effort; the upgrade path stays available.
      });
    return () => {
      cancelled = true;
    };
  }, []);
  const compatible = data.filter((b) => b.compatible);
  if (compatible.length === 0) return null;

  const restore = async (backup: RecoveryBackup) => {
    setRestoring(backup.filename);
    setError(null);
    try {
      await dbRecoveryApi.restore(backup.filename);
      // The backend restarts the app; keep the spinner until it does.
    } catch (e) {
      setRestoring(null);
      setError(extractErrorMessage(e) || String(e));
    }
  };

  return (
    <div className="space-y-2 rounded-lg border border-border/60 p-3">
      <p className="flex items-center gap-2 text-sm font-medium">
        <History className="h-4 w-4" />
        {t("workbench.recovery.title", {
          defaultValue: "Or restore a backup this version can open",
        })}
      </p>
      <p className="text-xs text-muted-foreground">
        {t("workbench.recovery.description", {
          defaultValue:
            "The current database is kept in the backups folder, so you can still upgrade later. Changes made after the backup are not in it.",
        })}
      </p>
      <ul className="space-y-1">
        {compatible.slice(0, 5).map((b) => (
          <li
            key={b.filename}
            className="flex flex-wrap items-center gap-2 text-xs"
          >
            <span className="break-all font-mono">{b.filename}</span>
            <span className="text-muted-foreground tabular-nums">
              v{b.schemaVersion} · {fmtSize(b.sizeBytes)} ·{" "}
              {b.createdAt ? new Date(b.createdAt).toLocaleString() : ""}
            </span>
            <Button
              size="sm"
              variant="outline"
              className="ml-auto h-7"
              disabled={disabled || restoring !== null}
              onClick={() => void restore(b)}
            >
              {restoring === b.filename && (
                <Loader2 className="mr-1 h-3.5 w-3.5 animate-spin" />
              )}
              {t("workbench.recovery.restore", {
                defaultValue: "Restore and restart",
              })}
            </Button>
          </li>
        ))}
      </ul>
      {error && (
        <p className="text-xs text-destructive" role="alert">
          {error}
        </p>
      )}
    </div>
  );
}
