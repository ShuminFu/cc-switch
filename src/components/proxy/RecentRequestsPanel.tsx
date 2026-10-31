import { useState } from "react";
import { useTranslation } from "react-i18next";
import { AlertTriangle, ListTree } from "lucide-react";
import { Switch } from "@/components/ui/switch";
import { Label } from "@/components/ui/label";
import { useRequestLogs } from "@/lib/query/usage";
import { RequestDetailPanel } from "@/components/usage/RequestDetailPanel";
import type { ProxyStatus } from "@/types/proxy";
import { cn } from "@/lib/utils";

/** Rows shown in the inspector; the full history lives in Usage. */
export const RECENT_REQUESTS_LIMIT = 10;
/** Refresh cadence while the proxy runs. */
const RECENT_REQUESTS_REFETCH_MS = 5000;

interface RecentRequestsPanelProps {
  status: ProxyStatus;
}

/**
 * Request inspector (FE-UI-39, first increment): the proxy's last error, the
 * latest request log rows (optionally errors only) and a detail dialog with
 * the stored metadata. Request and response bodies are never stored, so
 * there is no replay.
 */
export function RecentRequestsPanel({ status }: RecentRequestsPanelProps) {
  const { t, i18n } = useTranslation();
  const [errorsOnly, setErrorsOnly] = useState(false);
  const [selected, setSelected] = useState<string | null>(null);
  const { data } = useRequestLogs({
    filters: errorsOnly ? { statusMin: 400 } : {},
    range: { preset: "today" },
    page: 0,
    pageSize: RECENT_REQUESTS_LIMIT,
    options: { refetchInterval: RECENT_REQUESTS_REFETCH_MS },
  });
  const rows = data?.data ?? [];
  const time = (seconds: number) =>
    new Date(seconds * 1000).toLocaleTimeString(i18n.language);

  return (
    <div className="space-y-3 rounded-lg border border-border bg-muted/40 p-4">
      <div className="flex items-center justify-between gap-3">
        <div className="flex items-center gap-2 text-sm font-medium">
          <ListTree className="h-4 w-4" />
          {t("proxy.inspector.title", { defaultValue: "Recent requests" })}
        </div>
        <div className="flex items-center gap-2">
          <Switch
            id="proxy-inspector-errors-only"
            checked={errorsOnly}
            onCheckedChange={setErrorsOnly}
          />
          <Label
            htmlFor="proxy-inspector-errors-only"
            className="text-xs text-muted-foreground"
          >
            {t("proxy.inspector.errorsOnly", { defaultValue: "Errors only" })}
          </Label>
        </div>
      </div>

      {status.last_error && (
        <div
          role="alert"
          className="flex items-start gap-2 rounded-md border border-red-500/40 bg-red-500/10 p-2 text-xs text-red-600 dark:text-red-400"
        >
          <AlertTriangle className="mt-0.5 h-3.5 w-3.5 shrink-0" />
          <span className="break-all">
            {t("proxy.inspector.lastError", {
              defaultValue: "Last error: {{error}}",
              error: status.last_error,
            })}
          </span>
        </div>
      )}
      {status.last_request_at && (
        <p className="text-xs text-muted-foreground">
          {t("proxy.inspector.lastRequestAt", {
            defaultValue: "Last request: {{time}}",
            time: new Date(status.last_request_at).toLocaleString(
              i18n.language,
            ),
          })}
        </p>
      )}

      {rows.length === 0 ? (
        <p className="py-2 text-center text-xs text-muted-foreground">
          {t("proxy.inspector.empty", {
            defaultValue: "No requests today",
          })}
        </p>
      ) : (
        <ul className="divide-y divide-border/60 text-xs">
          {rows.map((row) => {
            const failed = row.statusCode >= 400;
            return (
              <li key={row.requestId}>
                <button
                  type="button"
                  onClick={() => setSelected(row.requestId)}
                  className="flex w-full items-center gap-3 px-1 py-1.5 text-left hover:bg-muted/60"
                  aria-label={t("proxy.inspector.openDetail", {
                    defaultValue: "Request {{status}} {{model}} at {{time}}",
                    status: row.statusCode,
                    model: row.model,
                    time: time(row.createdAt),
                  })}
                >
                  <span className="w-16 shrink-0 tabular-nums text-muted-foreground">
                    {time(row.createdAt)}
                  </span>
                  <span
                    className={cn(
                      "w-10 shrink-0 font-mono",
                      failed
                        ? "text-red-600 dark:text-red-400"
                        : "text-emerald-600 dark:text-emerald-400",
                    )}
                  >
                    {row.statusCode}
                  </span>
                  <span className="min-w-0 flex-1 truncate">
                    {row.providerName ?? row.providerId} · {row.model}
                    {failed && row.errorMessage ? ` — ${row.errorMessage}` : ""}
                  </span>
                  <span className="shrink-0 tabular-nums text-muted-foreground">
                    {row.latencyMs} ms
                  </span>
                </button>
              </li>
            );
          })}
        </ul>
      )}

      {selected && (
        <RequestDetailPanel
          requestId={selected}
          onClose={() => setSelected(null)}
        />
      )}
    </div>
  );
}
