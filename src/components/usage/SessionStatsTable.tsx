import { useTranslation } from "react-i18next";
import { ListFilter } from "lucide-react";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { Button } from "@/components/ui/button";
import { useTopSessions } from "@/lib/query/usage";
import { fmtUsd, getLocaleFromLanguage } from "./format";
import type { UsageRangeSelection } from "@/types/usage";

interface SessionStatsTableProps {
  range: UsageRangeSelection;
  appType?: string;
  providerName?: string;
  model?: string;
  refreshIntervalMs: number;
  /** 跳到请求日志并按该会话筛选 */
  onShowRequests?: (sessionId: string) => void;
}

export function SessionStatsTable({
  range,
  appType,
  providerName,
  model,
  refreshIntervalMs,
  onShowRequests,
}: SessionStatsTableProps) {
  const { t, i18n } = useTranslation();
  const locale = getLocaleFromLanguage(
    i18n.resolvedLanguage || i18n.language || "en",
  );
  const { data: stats, isLoading } = useTopSessions(
    range,
    { appType, providerName, model },
    {
      refetchInterval: refreshIntervalMs > 0 ? refreshIntervalMs : false,
    },
  );

  if (isLoading) {
    return <div className="h-[400px] animate-pulse rounded bg-gray-100" />;
  }

  return (
    <div className="space-y-2">
      <p className="text-xs text-muted-foreground">
        {t("usage.sessionsHint", {
          defaultValue:
            "按会话 ID 归集的费用（代理与本地会话日志）；30 天前的明细已汇总，不再按会话统计。",
        })}
      </p>
      <div className="rounded-lg border border-border/50 bg-card/40 backdrop-blur-sm overflow-hidden">
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>{t("usage.sessionId", "会话 ID")}</TableHead>
              <TableHead className="text-right">
                {t("usage.requests", "请求数")}
              </TableHead>
              <TableHead className="text-right">
                {t("usage.tokens", "Tokens")}
              </TableHead>
              <TableHead className="text-right">
                {t("usage.cost", "成本")}
              </TableHead>
              <TableHead className="text-right">
                {t("usage.lastSeen", "最近活动")}
              </TableHead>
              {onShowRequests && <TableHead className="w-10" />}
            </TableRow>
          </TableHeader>
          <TableBody>
            {!stats || stats.length === 0 ? (
              <TableRow>
                <TableCell
                  colSpan={onShowRequests ? 6 : 5}
                  className="text-center text-muted-foreground"
                >
                  {t("usage.noData", "暂无数据")}
                </TableCell>
              </TableRow>
            ) : (
              stats.map((stat) => {
                const tokens =
                  stat.inputTokens +
                  stat.outputTokens +
                  stat.cacheReadTokens +
                  stat.cacheCreationTokens;
                return (
                  <TableRow
                    key={stat.sessionId}
                    data-testid={`session-stat-${stat.sessionId}`}
                  >
                    <TableCell
                      className="max-w-[260px] truncate font-mono text-xs"
                      title={stat.sessionId}
                    >
                      {stat.sessionId}
                    </TableCell>
                    <TableCell className="text-right">
                      {stat.requests.toLocaleString()}
                    </TableCell>
                    <TableCell className="text-right">
                      {tokens.toLocaleString()}
                    </TableCell>
                    <TableCell className="text-right">
                      {fmtUsd(stat.totalCostUsd, 4)}
                    </TableCell>
                    <TableCell className="text-right whitespace-nowrap">
                      {new Date(stat.lastSeenAt * 1000).toLocaleString(locale)}
                    </TableCell>
                    {onShowRequests && (
                      <TableCell className="text-right">
                        <Button
                          type="button"
                          variant="ghost"
                          size="icon"
                          className="h-7 w-7"
                          onClick={() => onShowRequests(stat.sessionId)}
                          title={t("usage.showRequests", {
                            defaultValue: "查看该会话的请求",
                          })}
                          aria-label={t("usage.showRequests", {
                            defaultValue: "查看该会话的请求",
                          })}
                        >
                          <ListFilter className="h-3.5 w-3.5" />
                        </Button>
                      </TableCell>
                    )}
                  </TableRow>
                );
              })
            )}
          </TableBody>
        </Table>
      </div>
    </div>
  );
}
