import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { History, RotateCcw } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { queryKeys } from "@/lib/query/keys";
import { extractErrorMessage } from "@/utils/errorUtils";
import {
  formatHistoryValue,
  historyApi,
  type HistorySummary,
  type HistoryVersion,
} from "./api";

const formatTs = (ts: number) => new Date(ts).toLocaleString();

/**
 * Provider change history: every add/edit/delete, the live-config backfill a
 * switch performs, and imports, with a leaf-level diff and "restore this
 * version". Secrets show as `<redacted:…>` markers; a restore keeps the
 * provider's current key.
 */
export function ConfigHistoryPanel() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [selected, setSelected] = useState<HistorySummary | null>(null);
  const [restoring, setRestoring] = useState<HistoryVersion | null>(null);
  const { data: entities = [] } = useQuery({
    queryKey: queryKeys.workbench.history,
    queryFn: historyApi.list,
  });
  const { data: versions = [] } = useQuery({
    queryKey: queryKeys.workbench.providerHistory(
      selected?.app ?? "",
      selected?.entityId ?? "",
    ),
    queryFn: () => historyApi.provider(selected!.app, selected!.entityId),
    enabled: selected !== null,
  });

  const restore = useMutation({
    mutationFn: (v: HistoryVersion) =>
      historyApi.restore(selected!.app, selected!.entityId, v.versionId),
    onSuccess: async (outcome) => {
      if (outcome.missingSecrets.length > 0) {
        toast.warning(
          t("workbench.history.restoredMissingKeys", {
            defaultValue:
              "Version restored. Re-enter the API key: it is not kept in the history.",
          }),
        );
      } else {
        toast.success(
          t("workbench.history.restored", {
            defaultValue: "Version restored",
          }),
        );
      }
      await Promise.all([
        queryClient.invalidateQueries({
          queryKey: queryKeys.workbench.history,
        }),
        queryClient.invalidateQueries({ queryKey: queryKeys.providers.all }),
      ]);
    },
    onError: (error) =>
      toast.error(extractErrorMessage(error) || String(error)),
    onSettled: () => setRestoring(null),
  });

  return (
    <div className="space-y-3">
      <p className="text-xs text-muted-foreground">
        {t("workbench.history.description", {
          defaultValue:
            "The last 50 versions of each provider, including changes written back from the live config when you switch. API keys are never stored in the history.",
        })}
      </p>
      {entities.length === 0 ? (
        <p className="text-sm text-muted-foreground">
          {t("workbench.history.empty", {
            defaultValue: "No changes recorded yet.",
          })}
        </p>
      ) : (
        <div className="grid gap-3 md:grid-cols-[minmax(0,14rem)_1fr]">
          <ul
            className="max-h-80 space-y-1 overflow-y-auto"
            aria-label={t("workbench.history.entities", {
              defaultValue: "Providers with history",
            })}
          >
            {entities.map((e) => {
              const isSelected =
                selected?.app === e.app && selected?.entityId === e.entityId;
              return (
                <li key={`${e.app}:${e.entityId}`}>
                  <button
                    type="button"
                    aria-pressed={isSelected}
                    onClick={() => setSelected(e)}
                    className={`w-full rounded-md px-2 py-1.5 text-left text-sm hover:bg-muted focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring ${
                      isSelected ? "bg-muted" : ""
                    }`}
                  >
                    <span className="block truncate font-medium">
                      {e.name || e.entityId}
                    </span>
                    <span className="block text-xs text-muted-foreground">
                      {e.app} ·{" "}
                      {t("workbench.history.versions", {
                        defaultValue: "{{count}} versions",
                        count: e.versions,
                      })}
                      {e.deleted &&
                        ` · ${t("workbench.history.deleted", { defaultValue: "deleted" })}`}
                    </span>
                  </button>
                </li>
              );
            })}
          </ul>
          <div className="max-h-80 space-y-2 overflow-y-auto">
            {selected === null ? (
              <p className="flex items-center gap-2 text-sm text-muted-foreground">
                <History className="h-4 w-4" />
                {t("workbench.history.pick", {
                  defaultValue: "Pick a provider to see its versions.",
                })}
              </p>
            ) : (
              versions.map((v, index) => (
                <div
                  key={v.versionId}
                  className="space-y-1 rounded-md border border-border/60 p-2"
                >
                  <div className="flex flex-wrap items-center gap-2 text-xs">
                    <Badge variant="secondary">{v.action}</Badge>
                    <span className="text-muted-foreground">{v.source}</span>
                    <span className="text-muted-foreground">
                      {formatTs(v.ts)}
                    </span>
                    {v.snapshot !== null && index > 0 && (
                      <Button
                        size="sm"
                        variant="outline"
                        className="ml-auto h-7"
                        onClick={() => setRestoring(v)}
                      >
                        <RotateCcw className="mr-1 h-3.5 w-3.5" />
                        {t("workbench.history.restore", {
                          defaultValue: "Restore this version",
                        })}
                      </Button>
                    )}
                  </div>
                  {v.changes.length > 0 && (
                    <ul className="space-y-0.5 font-mono text-[11px]">
                      {v.changes.slice(0, 20).map((c) => (
                        <li key={c.path} className="break-all">
                          <span className="text-muted-foreground">
                            {c.path}
                          </span>{" "}
                          <span className="text-red-600 line-through dark:text-red-400">
                            {formatHistoryValue(c.before)}
                          </span>{" "}
                          →{" "}
                          <span className="text-emerald-700 dark:text-emerald-400">
                            {formatHistoryValue(c.after)}
                          </span>
                        </li>
                      ))}
                      {v.changes.length > 20 && (
                        <li className="text-muted-foreground">
                          {t("workbench.history.moreChanges", {
                            defaultValue: "+{{count}} more",
                            count: v.changes.length - 20,
                          })}
                        </li>
                      )}
                    </ul>
                  )}
                </div>
              ))
            )}
          </div>
        </div>
      )}
      <ConfirmDialog
        isOpen={restoring !== null}
        title={t("workbench.history.restore", {
          defaultValue: "Restore this version",
        })}
        message={t("workbench.history.restoreConfirm", {
          defaultValue:
            "Replace the provider with the version from {{when}}? If it is the active provider, its live config is rewritten. The current API key is kept.",
          when: restoring ? formatTs(restoring.ts) : "",
        })}
        isPending={restore.isPending}
        onConfirm={() => restoring && restore.mutate(restoring)}
        onCancel={() => setRestoring(null)}
      />
    </div>
  );
}
