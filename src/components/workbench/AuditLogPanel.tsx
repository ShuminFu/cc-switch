import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery } from "@tanstack/react-query";
import { toast } from "sonner";
import { FileDown, RefreshCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { queryKeys } from "@/lib/query/keys";
import { extractErrorMessage } from "@/utils/errorUtils";
import { auditApi } from "./api";
import { matchesAuditFilter } from "./helpers";

const LIMIT = 500;

/**
 * Local audit log: who changed what and from where (UI, CLI, tray, deep
 * link, import, backfill). Events carry hashes, never config content.
 */
export function AuditLogPanel() {
  const { t } = useTranslation();
  const [filter, setFilter] = useState("");
  const {
    data = [],
    refetch,
    isFetching,
  } = useQuery({
    queryKey: queryKeys.workbench.audit(LIMIT),
    queryFn: () => auditApi.list(LIMIT),
  });
  const visible = useMemo(
    () => data.filter((e) => matchesAuditFilter(e, filter)),
    [data, filter],
  );
  const exportLog = useMutation({
    mutationFn: auditApi.export,
    onSuccess: (path) => {
      if (path) {
        toast.success(
          t("workbench.audit.exported", {
            defaultValue: "Audit log saved to {{path}}",
            path,
          }),
        );
      }
    },
    onError: (error) =>
      toast.error(extractErrorMessage(error) || String(error)),
  });

  return (
    <div className="space-y-3">
      <div className="flex flex-wrap items-center gap-2">
        <Input
          className="h-8 max-w-xs"
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
          placeholder={t("workbench.audit.filter", {
            defaultValue: "Filter by action, actor or id",
          })}
          aria-label={t("workbench.audit.filter", {
            defaultValue: "Filter by action, actor or id",
          })}
        />
        <Button
          size="sm"
          variant="outline"
          onClick={() => void refetch()}
          disabled={isFetching}
        >
          <RefreshCw className="mr-1 h-3.5 w-3.5" />
          {t("common.refresh", { defaultValue: "Refresh" })}
        </Button>
        <Button
          size="sm"
          variant="outline"
          onClick={() => exportLog.mutate()}
          disabled={exportLog.isPending}
        >
          <FileDown className="mr-1 h-3.5 w-3.5" />
          {t("workbench.audit.export", { defaultValue: "Export JSONL" })}
        </Button>
      </div>
      {visible.length === 0 ? (
        <p className="text-sm text-muted-foreground">
          {t("workbench.audit.empty", {
            defaultValue: "No events recorded yet.",
          })}
        </p>
      ) : (
        <div className="max-h-72 overflow-y-auto rounded-md border border-border/60">
          <table className="w-full text-left text-xs">
            <thead className="sticky top-0 bg-muted text-muted-foreground">
              <tr>
                <th className="px-2 py-1 font-medium">
                  {t("workbench.audit.time", { defaultValue: "Time" })}
                </th>
                <th className="px-2 py-1 font-medium">
                  {t("workbench.audit.actor", { defaultValue: "Source" })}
                </th>
                <th className="px-2 py-1 font-medium">
                  {t("workbench.audit.action", { defaultValue: "Action" })}
                </th>
                <th className="px-2 py-1 font-medium">
                  {t("workbench.audit.entity", { defaultValue: "Entity" })}
                </th>
              </tr>
            </thead>
            <tbody>
              {visible.map((e, i) => (
                <tr
                  key={`${e.ts}-${i}`}
                  className="border-t border-border/40 align-top"
                >
                  <td className="whitespace-nowrap px-2 py-1 tabular-nums">
                    {new Date(e.ts).toLocaleString()}
                  </td>
                  <td className="px-2 py-1">{e.actor}</td>
                  <td className="px-2 py-1 font-mono">{e.action}</td>
                  <td className="break-all px-2 py-1">
                    {e.app ? `${e.app}/` : ""}
                    {e.entityId}
                    {e.detail && (
                      <span className="block text-muted-foreground">
                        {e.detail}
                      </span>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}
