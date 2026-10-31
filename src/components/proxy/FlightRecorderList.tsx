import { useTranslation } from "react-i18next";
import { ClipboardCopy } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { proxyApi } from "@/lib/api/proxy";
import { copyText } from "@/lib/clipboard";
import { showError } from "@/lib/errors/showError";
import { useRecentProxyRequests } from "@/lib/query/proxy";
import type { ProxyAttemptRecord } from "@/types/proxy";
import { cn } from "@/lib/utils";

/** Rows shown; the recorder keeps up to 200 in memory. */
const FLIGHT_RECORDER_ROWS = 20;

const ATTEMPT_CLASS: Record<ProxyAttemptRecord["result"], string> = {
  served: "text-emerald-600 dark:text-emerald-400",
  failover: "text-amber-600 dark:text-amber-400",
  aborted: "text-red-600 dark:text-red-400",
  skipped: "text-muted-foreground",
};

/**
 * Live view of the proxy's flight recorder (PROXY-21): each recent request
 * with every provider it tried, including failovers and skips. Nothing is
 * persisted, and bodies, headers and credentials are never recorded.
 */
export function FlightRecorderList({ running }: { running: boolean }) {
  const { t, i18n } = useTranslation();
  const { data: records = [] } = useRecentProxyRequests(running);
  const rows = records.slice(0, FLIGHT_RECORDER_ROWS);

  const resultLabel = (result: ProxyAttemptRecord["result"]) =>
    ({
      served: t("proxy.flightRecorder.served", { defaultValue: "served" }),
      failover: t("proxy.flightRecorder.failover", {
        defaultValue: "failed over",
      }),
      aborted: t("proxy.flightRecorder.aborted", { defaultValue: "aborted" }),
      skipped: t("proxy.flightRecorder.skipped", { defaultValue: "skipped" }),
    })[result];

  const handleExport = async () => {
    try {
      const jsonl = await proxyApi.exportRecentRequests();
      await copyText(jsonl);
      toast.success(
        t("proxy.flightRecorder.copied", {
          defaultValue: "Recent requests copied as JSON Lines",
        }),
      );
    } catch (error) {
      showError({
        title: t("proxy.flightRecorder.copyFailed", {
          defaultValue: "Failed to copy the recent requests",
        }),
        error,
      });
    }
  };

  return (
    <div className="space-y-2">
      <div className="flex items-center justify-between gap-2">
        <p className="text-xs text-muted-foreground">
          {t("proxy.flightRecorder.hint", {
            defaultValue:
              "Every provider each request tried since the proxy started. Kept in memory only.",
          })}
        </p>
        <Button
          size="sm"
          variant="outline"
          className="h-7 shrink-0 text-xs"
          onClick={() => void handleExport()}
          disabled={records.length === 0}
        >
          <ClipboardCopy className="mr-1 h-3.5 w-3.5" />
          {t("proxy.flightRecorder.export", {
            defaultValue: "Copy as JSON Lines",
          })}
        </Button>
      </div>
      {rows.length === 0 ? (
        <p className="py-2 text-center text-xs text-muted-foreground">
          {t("proxy.flightRecorder.empty", {
            defaultValue: "No requests since the proxy started",
          })}
        </p>
      ) : (
        <ul
          className="divide-y divide-border/60 text-xs"
          data-testid="flight-recorder"
        >
          {rows.map((record) => (
            <li key={record.id} className="space-y-0.5 px-1 py-1.5">
              <div className="flex items-center gap-3">
                <span className="w-16 shrink-0 tabular-nums text-muted-foreground">
                  {new Date(record.startedAt).toLocaleTimeString(i18n.language)}
                </span>
                <span
                  className={cn(
                    "w-10 shrink-0 font-mono",
                    record.success
                      ? "text-emerald-600 dark:text-emerald-400"
                      : "text-red-600 dark:text-red-400",
                  )}
                >
                  {record.success
                    ? t("proxy.flightRecorder.ok", { defaultValue: "OK" })
                    : t("proxy.flightRecorder.fail", { defaultValue: "FAIL" })}
                </span>
                <span className="min-w-0 flex-1 truncate font-mono">
                  {record.appType} {record.method} {record.endpoint}
                </span>
                <span className="shrink-0 tabular-nums text-muted-foreground">
                  {record.durationMs} ms
                </span>
              </div>
              {record.attempts.length > 0 && (
                <ol className="ml-[4.75rem] flex flex-wrap items-center gap-x-1 text-[11px]">
                  {record.attempts.map((attempt, index) => (
                    <li
                      key={`${attempt.providerId}-${index}`}
                      className="flex items-center gap-1"
                      title={attempt.error ?? undefined}
                    >
                      {index > 0 && (
                        <span className="text-muted-foreground">→</span>
                      )}
                      <span className={ATTEMPT_CLASS[attempt.result]}>
                        {attempt.providerName || attempt.providerId}
                        {attempt.status ? ` ${attempt.status}` : ""} (
                        {resultLabel(attempt.result)})
                      </span>
                    </li>
                  ))}
                </ol>
              )}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
