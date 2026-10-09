import { Coins, ListFilter } from "lucide-react";
import { useTranslation } from "react-i18next";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { formatTokensShort } from "@/components/usage/format";
import type { SessionUsageStat } from "@/types/usage";
import { formatSessionCost } from "./utils";

interface SessionUsageSummaryProps {
  usage?: SessionUsageStat;
  /** 打开用量页并按该会话筛选请求日志 */
  onViewRequests?: () => void;
}

/** 会话详情头部的费用归集（无记录时不渲染） */
export function SessionUsageSummary({
  usage,
  onViewRequests,
}: SessionUsageSummaryProps) {
  const { t, i18n } = useTranslation();
  if (!usage) return null;

  const language = i18n.resolvedLanguage || i18n.language || "en";
  const totalTokens =
    usage.inputTokens +
    usage.outputTokens +
    usage.cacheReadTokens +
    usage.cacheCreationTokens;

  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <span
          className="flex items-center gap-1 cursor-default"
          data-testid="session-usage-summary"
        >
          <Coins className="size-3" />
          <span>
            {t("sessionManager.usageSummary", {
              defaultValue:
                "{{cost}} · {{requests}} 次请求 · {{tokens}} tokens",
              cost: formatSessionCost(usage.totalCostUsd),
              requests: usage.requests,
              tokens: formatTokensShort(totalTokens, language),
            })}
          </span>
        </span>
      </TooltipTrigger>
      {onViewRequests && (
        <button
          type="button"
          onClick={onViewRequests}
          className="flex items-center gap-1 hover:text-foreground transition-colors"
          title={t("sessionManager.viewRequests", {
            defaultValue: "在用量页查看该会话的请求",
          })}
          aria-label={t("sessionManager.viewRequests", {
            defaultValue: "在用量页查看该会话的请求",
          })}
          data-testid="session-view-requests"
        >
          <ListFilter className="size-3" />
        </button>
      )}
      <TooltipContent side="bottom" className="max-w-xs">
        <p>
          {t("sessionManager.usageTooltip", {
            defaultValue:
              "按代理与本地会话日志归集的估算费用；30 天前的明细已汇总，不再按会话统计。",
          })}
        </p>
      </TooltipContent>
    </Tooltip>
  );
}
