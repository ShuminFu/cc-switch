import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { KeyRound, Loader2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { settingsApi } from "@/lib/api";

/** 与后端 `MIN_SYNC_PASSPHRASE_CHARS` 保持一致（按去除首尾空白后的字符数计） */
export const MIN_SYNC_KEY_CHARS = 8;

export const SYNC_SNAPSHOT_KEY_STATUS_QUERY_KEY = [
  "sync-snapshot-key-status",
] as const;

const errorText = (error: unknown) =>
  (error as Error)?.message ?? String(error);

/**
 * 快照签名密钥（WebDAV 与 S3 共用）：每台设备输入相同的口令，
 * 上传的快照带签名，下载时校验，防止远端快照被篡改或降级为未签名版本。
 */
export function SyncSnapshotKeyField() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [key, setKey] = useState("");
  const [showClearConfirm, setShowClearConfirm] = useState(false);

  const statusQuery = useQuery({
    queryKey: SYNC_SNAPSHOT_KEY_STATUS_QUERY_KEY,
    queryFn: () => settingsApi.getSyncSnapshotKeyStatus(),
  });

  const mutation = useMutation({
    mutationFn: (next: string | null) => settingsApi.setSyncSnapshotKey(next),
    onSuccess: async (status, next) => {
      queryClient.setQueryData(SYNC_SNAPSHOT_KEY_STATUS_QUERY_KEY, status);
      if (next === null) {
        // 清除密钥会复位 WebDAV/S3 的 signedSync，刷新设置里的同步状态
        await queryClient.invalidateQueries({ queryKey: ["settings"] });
        toast.success(t("settings.webdavSync.syncKey.clearSuccess"));
      } else {
        setKey("");
        toast.success(t("settings.webdavSync.syncKey.setSuccess"));
      }
    },
    onError: (error, next) => {
      toast.error(
        next === null
          ? t("settings.webdavSync.syncKey.clearFailed", {
              error: errorText(error),
            })
          : t("settings.webdavSync.syncKey.setFailed", {
              error: errorText(error),
            }),
      );
    },
  });

  const status = statusQuery.data;
  const configured = status?.configured ?? false;
  const trimmedLength = Array.from(key.trim()).length;
  const tooShort = trimmedLength > 0 && trimmedLength < MIN_SYNC_KEY_CHARS;
  const busy = mutation.isPending;

  const handleSet = () => {
    if (trimmedLength < MIN_SYNC_KEY_CHARS) {
      toast.error(
        t("settings.webdavSync.syncKey.tooShort", {
          min: MIN_SYNC_KEY_CHARS,
        }),
      );
      return;
    }
    mutation.mutate(key);
  };

  const handleClearConfirm = () => {
    setShowClearConfirm(false);
    mutation.mutate(null);
  };

  let statusText: string;
  if (statusQuery.isLoading) {
    statusText = t("settings.webdavSync.syncKey.statusLoading");
  } else if (statusQuery.isError) {
    statusText = t("settings.webdavSync.syncKey.statusFailed", {
      error: errorText(statusQuery.error),
    });
  } else if (configured) {
    statusText = t("settings.webdavSync.syncKey.statusConfigured");
  } else {
    statusText = t("settings.webdavSync.syncKey.statusNotSet");
  }

  return (
    <div
      className="space-y-3 rounded-lg border border-border bg-muted/40 p-4"
      data-testid="sync-snapshot-key-field"
    >
      <div className="space-y-1">
        <div className="flex items-center gap-2 text-xs font-medium text-foreground">
          <KeyRound className="h-3.5 w-3.5" />
          {t("settings.webdavSync.syncKey.title")}
        </div>
        <p className="text-xs text-muted-foreground">
          {t("settings.webdavSync.syncKey.description")}
        </p>
        <p
          className="text-xs text-muted-foreground"
          data-testid="sync-snapshot-key-status"
        >
          {statusText}
          {configured && status?.keyId ? (
            <code className="ml-1 font-mono text-foreground">
              {status.keyId}
            </code>
          ) : null}
        </p>
      </div>

      <div className="flex items-center gap-2">
        <Input
          type="password"
          autoComplete="new-password"
          value={key}
          onChange={(event) => setKey(event.target.value)}
          placeholder={t("settings.webdavSync.syncKey.placeholder", {
            min: MIN_SYNC_KEY_CHARS,
          })}
          aria-label={t("settings.webdavSync.syncKey.title")}
          aria-invalid={tooShort}
          disabled={busy}
          className="text-xs flex-1"
        />
        <Button
          type="button"
          size="sm"
          onClick={handleSet}
          disabled={busy || trimmedLength < MIN_SYNC_KEY_CHARS}
        >
          {busy && mutation.variables !== null ? (
            <Loader2 className="mr-1 h-3.5 w-3.5 animate-spin" />
          ) : null}
          {t("settings.webdavSync.syncKey.set")}
        </Button>
        <Button
          type="button"
          size="sm"
          variant="outline"
          onClick={() => setShowClearConfirm(true)}
          disabled={busy || !configured}
        >
          {t("settings.webdavSync.syncKey.clear")}
        </Button>
      </div>
      {tooShort && (
        <p className="text-xs text-destructive">
          {t("settings.webdavSync.syncKey.tooShort", {
            min: MIN_SYNC_KEY_CHARS,
          })}
        </p>
      )}

      <ConfirmDialog
        isOpen={showClearConfirm}
        title={t("settings.webdavSync.syncKey.clearConfirm.title")}
        message={t("settings.webdavSync.syncKey.clearConfirm.message")}
        confirmText={t("settings.webdavSync.syncKey.clearConfirm.confirm")}
        onConfirm={handleClearConfirm}
        onCancel={() => setShowClearConfirm(false)}
      />
    </div>
  );
}
