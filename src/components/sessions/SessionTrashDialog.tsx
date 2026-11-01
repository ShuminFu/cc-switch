import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { History, RotateCcw } from "lucide-react";
import {
  sessionTrashApi,
  type SessionTrashEntry,
} from "@/lib/api/sessionTrash";
import { queryKeys } from "@/lib/query/keys";
import { extractErrorMessage } from "@/utils/errorUtils";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { ProviderIcon } from "@/components/ProviderIcon";

/** Last path segment of a trashed file (the session file or directory name). */
function entryLabel(entry: SessionTrashEntry): string {
  const first = entry.items[0]?.originalPath ?? entry.sessionId;
  const parts = first.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] ?? entry.sessionId;
}

/**
 * "Recently deleted" view for the session trash (SESS-41): lists deleted
 * file-based sessions (kept for 30 days) and restores one on request.
 */
export function SessionTrashDialog() {
  const { t, i18n } = useTranslation();
  const queryClient = useQueryClient();
  const [open, setOpen] = useState(false);

  const trashQuery = useQuery({
    queryKey: queryKeys.sessions.trash,
    queryFn: () => sessionTrashApi.list(),
    enabled: open,
  });

  const restoreMutation = useMutation({
    mutationFn: (id: string) => sessionTrashApi.restore(id),
    onSuccess: async () => {
      // `sessions.all` is a prefix of `sessions.trash`: refreshes both lists.
      await queryClient.invalidateQueries({ queryKey: queryKeys.sessions.all });
      toast.success(
        t("sessionManager.trash.restored", {
          defaultValue: "Session restored",
        }),
      );
    },
    onError: (error: unknown) => {
      toast.error(
        extractErrorMessage(error) ||
          t("sessionManager.restoreFailed", {
            defaultValue: "Could not restore the session",
          }),
      );
    },
  });

  const entries = trashQuery.data ?? [];
  const formatter = new Intl.DateTimeFormat(i18n.language, {
    dateStyle: "medium",
    timeStyle: "short",
  });

  return (
    <>
      <Tooltip>
        <TooltipTrigger asChild>
          <Button
            variant="ghost"
            size="icon"
            className="size-7"
            onClick={() => setOpen(true)}
            aria-label={t("sessionManager.trash.open", {
              defaultValue: "Recently deleted",
            })}
          >
            <History className="size-3.5" />
          </Button>
        </TooltipTrigger>
        <TooltipContent>
          {t("sessionManager.trash.open", {
            defaultValue: "Recently deleted",
          })}
        </TooltipContent>
      </Tooltip>
      <Dialog open={open} onOpenChange={setOpen}>
        <DialogContent className="max-w-lg">
          <DialogHeader>
            <DialogTitle>
              {t("sessionManager.trash.title", {
                defaultValue: "Recently deleted sessions",
              })}
            </DialogTitle>
            <DialogDescription>
              {t("sessionManager.trash.description", {
                defaultValue:
                  "Deleted sessions are kept for 30 days. Restore one to move it back to where it was.",
              })}
            </DialogDescription>
          </DialogHeader>
          {trashQuery.isLoading ? (
            <p className="py-6 text-center text-sm text-muted-foreground">
              {t("common.loading")}
            </p>
          ) : entries.length === 0 ? (
            <p className="py-6 text-center text-sm text-muted-foreground">
              {t("sessionManager.trash.empty", {
                defaultValue: "No recently deleted sessions",
              })}
            </p>
          ) : (
            <ul className="max-h-[60vh] divide-y overflow-y-auto rounded-md border">
              {entries.map((entry) => (
                <li
                  key={entry.id}
                  className="flex items-center gap-3 px-3 py-2"
                  data-testid="session-trash-entry"
                >
                  <ProviderIcon
                    icon={entry.providerId}
                    name={entry.providerId}
                    size={16}
                  />
                  <div className="min-w-0 flex-1">
                    <div className="truncate text-sm font-medium">
                      {entryLabel(entry)}
                    </div>
                    <div className="text-xs text-muted-foreground">
                      {formatter.format(new Date(entry.deletedAt))}
                    </div>
                  </div>
                  <Button
                    variant="outline"
                    size="sm"
                    disabled={restoreMutation.isPending}
                    onClick={() => restoreMutation.mutate(entry.id)}
                  >
                    <RotateCcw className="mr-1 size-3.5" />
                    {t("sessionManager.trash.restore", {
                      defaultValue: "Restore",
                    })}
                  </Button>
                </li>
              ))}
            </ul>
          )}
        </DialogContent>
      </Dialog>
    </>
  );
}
