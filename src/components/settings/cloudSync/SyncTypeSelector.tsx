import { useCallback, useEffect, useState } from "react";
import { AlertTriangle } from "lucide-react";
import { useTranslation } from "react-i18next";
import { useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { settingsApi } from "@/lib/api";
import type { S3SyncSettings, WebDavSyncSettings } from "@/types";
import { extractErrorMessage } from "@/utils/errorUtils";
import type { SyncBackendId } from "./types";
import { queryKeys } from "@/lib/query/keys";

/**
 * Which backend the panel shows. Only one backend may be enabled: switching
 * to the other one while the current one is enabled asks first, then
 * disables the current one (and its auto sync).
 */
export function useSyncTypeSelection(
  webdav: WebDavSyncSettings | undefined,
  s3: S3SyncSettings | undefined,
) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [syncType, setSyncType] = useState<SyncBackendId>(() =>
    s3?.enabled ? "s3" : "webdav",
  );
  const [pending, setPending] = useState<SyncBackendId | null>(null);

  // Settings load asynchronously; follow an enabled S3 config.
  useEffect(() => {
    if (s3?.enabled) setSyncType("s3");
  }, [s3?.enabled]);

  const request = useCallback(
    (next: SyncBackendId) => {
      if (next === syncType) return;
      const currentEnabled =
        next === "s3"
          ? webdav?.enabled !== false && Boolean(webdav?.baseUrl?.trim())
          : s3?.enabled === true;
      if (currentEnabled) setPending(next);
      else setSyncType(next);
    },
    [s3, syncType, webdav],
  );

  const confirm = useCallback(async () => {
    if (!pending) return;
    const next = pending;
    setPending(null);
    try {
      if (next === "s3") {
        await settingsApi.webdavSyncSaveSettings(
          { ...webdav, enabled: false, autoSync: false },
          false,
        );
      } else {
        await settingsApi.s3SyncSaveSettings(
          { ...s3, enabled: false, autoSync: false },
          false,
        );
      }
      await queryClient.invalidateQueries({ queryKey: queryKeys.settings.all });
      setSyncType(next);
    } catch (error) {
      toast.error(
        t("settings.s3Sync.mutualExclusionFailed", {
          error: extractErrorMessage(error) || String(error),
        }),
      );
    }
  }, [pending, queryClient, s3, t, webdav]);

  const cancel = useCallback(() => setPending(null), []);

  return { syncType, pending, request, confirm, cancel };
}

interface SyncTypeSelectorProps {
  selection: ReturnType<typeof useSyncTypeSelection>;
  disabled?: boolean;
}

export function SyncTypeSelector({
  selection,
  disabled,
}: SyncTypeSelectorProps) {
  const { t } = useTranslation();
  const { syncType, pending, request, confirm, cancel } = selection;

  return (
    <>
      <div className="flex items-center gap-4">
        <label className="w-40 text-xs font-medium text-foreground shrink-0">
          {t("settings.syncType.label")}
        </label>
        <Select
          value={syncType}
          onValueChange={(value) => request(value as SyncBackendId)}
          disabled={disabled}
        >
          <SelectTrigger className="text-xs flex-1">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="webdav">
              {t("settings.syncType.webdav")}
            </SelectItem>
            <SelectItem value="s3">{t("settings.syncType.s3")}</SelectItem>
          </SelectContent>
        </Select>
      </div>

      <Dialog
        open={pending !== null}
        onOpenChange={(open) => {
          if (!open) cancel();
        }}
      >
        <DialogContent className="max-w-sm" zIndex="alert">
          <DialogHeader className="space-y-3 border-b-0 bg-transparent pb-0">
            <DialogTitle className="flex items-center gap-2 text-lg font-semibold">
              <AlertTriangle className="h-5 w-5 text-destructive" />
              {t("settings.s3Sync.mutualExclusionTitle")}
            </DialogTitle>
            <DialogDescription asChild>
              <div className="space-y-3 text-sm leading-relaxed">
                <p>
                  {pending === "webdav"
                    ? t("settings.s3Sync.mutualExclusionMessageReverse")
                    : t("settings.s3Sync.mutualExclusionMessage")}
                </p>
              </div>
            </DialogDescription>
          </DialogHeader>
          <DialogFooter className="flex gap-2 border-t-0 bg-transparent pt-2 sm:justify-end">
            <Button variant="outline" onClick={cancel}>
              {t("common.cancel")}
            </Button>
            <Button variant="destructive" onClick={() => void confirm()}>
              {t("common.confirm")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  );
}
