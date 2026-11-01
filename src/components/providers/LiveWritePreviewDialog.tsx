import { useQuery } from "@tanstack/react-query";
import { Loader2 } from "lucide-react";
import { useTranslation } from "react-i18next";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { adaptersApi } from "@/lib/api/adapters";
import type { AppId } from "@/lib/api";
import { queryKeys } from "@/lib/query/keys";
import { cn } from "@/lib/utils";
import { extractErrorMessage } from "@/utils/errorUtils";

/**
 * Apps whose adapter can plan a live write (`plan_live`). Only Claude Code
 * has a planner today (docs/dev/config-adapters.md); the preview action is
 * hidden for every other app.
 */
export const LIVE_PREVIEW_APPS: readonly AppId[] = ["claude"];

export const supportsLivePreview = (appId: AppId | undefined): boolean =>
  !!appId && LIVE_PREVIEW_APPS.includes(appId);

interface LiveWritePreviewDialogProps {
  open: boolean;
  appId: AppId;
  providerId: string;
  providerName: string;
  onOpenChange: (open: boolean) => void;
  /** Shown as the primary action when given ("Switch"). */
  onConfirm?: () => void;
  confirmLabel?: string;
}

function diffLineClass(line: string): string {
  if (line.startsWith("+")) {
    return "bg-emerald-500/10 text-emerald-700 dark:text-emerald-300";
  }
  if (line.startsWith("-")) {
    return "bg-red-500/10 text-red-700 dark:text-red-300";
  }
  return "text-muted-foreground";
}

/**
 * "Preview changes": what switching to a provider would write to the app's
 * live files, rendered as a line diff (`preview_live_write`, CFG-22).
 * Nothing is written until the user confirms.
 */
export function LiveWritePreviewDialog({
  open,
  appId,
  providerId,
  providerName,
  onOpenChange,
  onConfirm,
  confirmLabel,
}: LiveWritePreviewDialogProps) {
  const { t } = useTranslation();
  const query = useQuery({
    queryKey: queryKeys.adapters.livePreview(appId, providerId),
    queryFn: () => adaptersApi.previewLiveWrite(appId, providerId),
    enabled: open,
    staleTime: 0,
    gcTime: 0,
  });

  const plan = query.data;
  const changedFiles = plan?.files.filter((file) => file.changed) ?? [];

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-3xl" zIndex="nested">
        <DialogHeader>
          <DialogTitle>
            {t("provider.preview.title", {
              defaultValue: "Preview changes",
            })}
          </DialogTitle>
          <DialogDescription>
            {t("provider.preview.description", {
              defaultValue:
                "What switching to {{name}} writes to the live config files. Nothing is written until you switch.",
              name: providerName,
            })}
          </DialogDescription>
        </DialogHeader>

        <div className="max-h-[60vh] space-y-4 overflow-y-auto px-6 py-4">
          {query.isLoading && (
            <div className="flex items-center gap-2 text-sm text-muted-foreground">
              <Loader2 className="h-4 w-4 animate-spin" />
              {t("common.loading", { defaultValue: "Loading..." })}
            </div>
          )}
          {query.isError && (
            <p role="alert" className="text-sm text-destructive">
              {t("provider.preview.failed", {
                defaultValue: "Could not compute the preview: {{error}}",
                error: extractErrorMessage(query.error),
              })}
            </p>
          )}
          {plan && !plan.supported && (
            <p className="text-sm text-muted-foreground">
              {t("provider.preview.unsupported", {
                defaultValue: "A preview is not available for this app yet.",
              })}
            </p>
          )}
          {plan?.supported && changedFiles.length === 0 && (
            <p className="text-sm text-muted-foreground">
              {t("provider.preview.noChanges", {
                defaultValue: "Switching does not change any live file.",
              })}
            </p>
          )}
          {changedFiles.map((file) => (
            <section key={file.path} className="space-y-1">
              <h3 className="break-all font-mono text-xs font-medium">
                {file.path}
                {file.before === null && (
                  <span className="ml-2 rounded bg-muted px-1.5 py-0.5 font-sans text-[10px] uppercase text-muted-foreground">
                    {t("provider.preview.newFile", {
                      defaultValue: "new file",
                    })}
                  </span>
                )}
              </h3>
              <pre
                data-testid="live-preview-diff"
                className="overflow-x-auto rounded-md border border-border-default bg-muted/30 p-2 font-mono text-xs leading-5"
              >
                {file.unifiedDiff.split("\n").map((line, index) => (
                  <div
                    key={index}
                    className={cn("whitespace-pre", diffLineClass(line))}
                  >
                    {line || " "}
                  </div>
                ))}
              </pre>
            </section>
          ))}
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            {onConfirm
              ? t("common.cancel")
              : t("common.close", { defaultValue: "Close" })}
          </Button>
          {onConfirm && (
            <Button
              disabled={query.isLoading || query.isError}
              onClick={() => {
                onOpenChange(false);
                onConfirm();
              }}
            >
              {confirmLabel ?? t("provider.enable")}
            </Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
