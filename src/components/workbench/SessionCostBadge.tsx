import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import { Coins } from "lucide-react";
import { queryKeys } from "@/lib/query/keys";
import { formatTokens, formatUsd, sessionCostApi } from "./api";

/**
 * Cost and tokens recorded for one CLI session (request logs carry the
 * session id). Renders nothing when no request of the session was logged.
 */
export function SessionCostBadge({ sessionId }: { sessionId?: string }) {
  const { t } = useTranslation();
  const { data } = useQuery({
    queryKey: queryKeys.workbench.sessionUsage(sessionId ?? ""),
    queryFn: () => sessionCostApi.session(sessionId!),
    enabled: Boolean(sessionId),
    staleTime: 30_000,
  });
  if (!data) return null;
  const tokens =
    data.inputTokens +
    data.outputTokens +
    data.cacheReadTokens +
    data.cacheCreationTokens;
  const title = t("workbench.sessionCost.title", {
    defaultValue:
      "{{requests}} logged requests · {{input}} input / {{output}} output tokens",
    requests: data.requests,
    input: formatTokens(data.inputTokens),
    output: formatTokens(data.outputTokens),
  });
  return (
    <span
      className="inline-flex items-center gap-1 rounded-md border border-border/60 px-2 py-1 text-xs tabular-nums text-muted-foreground"
      title={title}
      aria-label={title}
    >
      <Coins className="h-3.5 w-3.5" />
      {formatUsd(data.totalCostUsd)} · {formatTokens(tokens)}
    </span>
  );
}
