import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { queryKeys } from "@/lib/query/keys";
import { formatTokens, formatUsd, sessionCostApi } from "./api";

const RANGES = [7, 30, 90];

const baseName = (dir: string) =>
  dir.split(/[\\/]/).filter(Boolean).pop() ?? dir;

/**
 * Usage grouped by project (git root of each session's working directory).
 * Requests whose session is unknown are listed as "unattributed" so the rows
 * add up to the request-log total.
 */
export function ProjectUsagePanel() {
  const { t } = useTranslation();
  const [days, setDays] = useState(30);
  const { data, isLoading } = useQuery({
    queryKey: queryKeys.workbench.projectUsage(days),
    queryFn: () => sessionCostApi.byProject(days),
    staleTime: 60_000,
  });

  return (
    <div className="space-y-3 rounded-lg border border-border/60 p-4">
      <div className="flex flex-wrap items-center gap-3">
        <div>
          <h4 className="text-sm font-medium">
            {t("workbench.projectUsage.title", {
              defaultValue: "Usage by project",
            })}
          </h4>
          <p className="text-xs text-muted-foreground">
            {t("workbench.projectUsage.description", {
              defaultValue:
                "Requests are matched to sessions and grouped by the session's git repository.",
            })}
          </p>
        </div>
        <Select value={String(days)} onValueChange={(v) => setDays(Number(v))}>
          <SelectTrigger
            className="ml-auto w-32"
            aria-label={t("workbench.projectUsage.range", {
              defaultValue: "Time range",
            })}
          >
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {RANGES.map((n) => (
              <SelectItem key={n} value={String(n)}>
                {t("workbench.projectUsage.lastDays", {
                  defaultValue: "Last {{count}} days",
                  count: n,
                })}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>
      {isLoading || !data ? (
        <p className="text-sm text-muted-foreground">
          {t("common.loading", { defaultValue: "Loading…" })}
        </p>
      ) : data.total.requests === 0 ? (
        <p className="text-sm text-muted-foreground">
          {t("workbench.projectUsage.empty", {
            defaultValue: "No requests logged in this period.",
          })}
        </p>
      ) : (
        <table className="w-full text-left text-xs">
          <thead className="text-muted-foreground">
            <tr>
              <th className="py-1 font-medium">
                {t("workbench.projectUsage.project", {
                  defaultValue: "Project",
                })}
              </th>
              <th className="py-1 text-right font-medium">
                {t("workbench.projectUsage.sessions", {
                  defaultValue: "Sessions",
                })}
              </th>
              <th className="py-1 text-right font-medium">
                {t("workbench.projectUsage.requests", {
                  defaultValue: "Requests",
                })}
              </th>
              <th className="py-1 text-right font-medium">
                {t("workbench.projectUsage.tokens", {
                  defaultValue: "Tokens",
                })}
              </th>
              <th className="py-1 text-right font-medium">
                {t("workbench.projectUsage.cost", { defaultValue: "Cost" })}
              </th>
            </tr>
          </thead>
          <tbody className="tabular-nums">
            {data.projects.map((p) => (
              <tr key={p.projectDir} className="border-t border-border/40">
                <td className="py-1" title={p.projectDir}>
                  {baseName(p.projectDir)}
                </td>
                <td className="py-1 text-right">{p.sessions}</td>
                <td className="py-1 text-right">{p.requests}</td>
                <td className="py-1 text-right">
                  {formatTokens(p.inputTokens + p.outputTokens)}
                </td>
                <td className="py-1 text-right">{formatUsd(p.totalCostUsd)}</td>
              </tr>
            ))}
            {data.unattributed.requests > 0 && (
              <tr className="border-t border-border/40 text-muted-foreground">
                <td className="py-1">
                  {t("workbench.projectUsage.unattributed", {
                    defaultValue: "Unattributed",
                  })}
                </td>
                <td className="py-1 text-right">–</td>
                <td className="py-1 text-right">
                  {data.unattributed.requests}
                </td>
                <td className="py-1 text-right">
                  {formatTokens(
                    data.unattributed.inputTokens +
                      data.unattributed.outputTokens,
                  )}
                </td>
                <td className="py-1 text-right">
                  {formatUsd(data.unattributed.totalCostUsd)}
                </td>
              </tr>
            )}
            <tr className="border-t border-border font-medium">
              <td className="py-1">
                {t("workbench.projectUsage.total", { defaultValue: "Total" })}
              </td>
              <td />
              <td className="py-1 text-right">{data.total.requests}</td>
              <td className="py-1 text-right">
                {formatTokens(data.total.inputTokens + data.total.outputTokens)}
              </td>
              <td className="py-1 text-right">
                {formatUsd(data.total.totalCostUsd)}
              </td>
            </tr>
          </tbody>
        </table>
      )}
    </div>
  );
}
