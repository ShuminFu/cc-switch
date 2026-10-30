import { useQuery } from "@tanstack/react-query";
import { CheckCircle2, Loader2, MinusCircle, XCircle } from "lucide-react";
import { useTranslation } from "react-i18next";
import { settingsApi } from "@/lib/api/settings";
import type { StartupStepRecord } from "@/shared/bindings";
import { extractErrorMessage } from "@/utils/errorUtils";

export const startupReportQueryKey = ["startupReport"] as const;

function StatusIcon({ status }: { status: StartupStepRecord["status"] }) {
  switch (status) {
    case "ok":
      return <CheckCircle2 className="h-4 w-4 shrink-0 text-emerald-500" />;
    case "skipped":
      return <MinusCircle className="h-4 w-4 shrink-0 text-muted-foreground" />;
    case "failed":
      return <XCircle className="h-4 w-4 shrink-0 text-red-500" />;
  }
}

/** 启动诊断：逐步列出启动流水线（迁移、live 导入等）的结果，失败项带错误码 */
export function StartupReportPanel() {
  const { t } = useTranslation();
  const {
    data: report,
    isLoading,
    error,
  } = useQuery({
    queryKey: startupReportQueryKey,
    queryFn: () => settingsApi.getStartupReport(),
  });

  if (isLoading) {
    return (
      <div className="flex items-center gap-2 text-sm text-muted-foreground">
        <Loader2 className="h-4 w-4 animate-spin" />
      </div>
    );
  }

  if (error || !report) {
    return (
      <p className="text-sm text-red-500">
        {t("settings.advanced.startupReport.loadFailed")}
        {error ? `: ${extractErrorMessage(error)}` : null}
      </p>
    );
  }

  const failedCount = report.steps.filter((s) => s.status === "failed").length;

  return (
    <div className="space-y-4">
      <p
        className={
          failedCount > 0
            ? "text-sm font-medium text-red-500"
            : "text-sm text-muted-foreground"
        }
      >
        {failedCount > 0
          ? t("settings.advanced.startupReport.failedSummary", {
              count: failedCount,
            })
          : t("settings.advanced.startupReport.allOk")}
      </p>
      <ul className="space-y-2">
        {report.steps.map((step, index) => (
          <li
            key={`${step.id}-${index}`}
            data-testid={`startup-step-${step.id}`}
            className="flex items-start gap-3 text-sm"
          >
            <StatusIcon status={step.status} />
            <div className="min-w-0 flex-1 space-y-0.5">
              <div className="flex flex-wrap items-center gap-2">
                <span className="font-mono">{step.id}</span>
                <span className="text-xs text-muted-foreground">
                  {t(`settings.advanced.startupReport.status.${step.status}`)}
                </span>
                {step.code ? (
                  <span className="rounded bg-red-500/10 px-1.5 py-0.5 font-mono text-xs text-red-500">
                    {step.code}
                  </span>
                ) : null}
              </div>
              {step.message ? (
                <p className="break-words text-xs text-muted-foreground">
                  {step.message}
                </p>
              ) : null}
            </div>
          </li>
        ))}
      </ul>
    </div>
  );
}
