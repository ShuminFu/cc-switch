import { useEffect, useState } from "react";
import { Loader2, Radar } from "lucide-react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import {
  useCircuitBreakerStats,
  useProbeCircuitBreaker,
} from "@/lib/query/failover";
import type { CircuitBreakerStats } from "@/types/proxy";
import { extractErrorMessage } from "@/utils/errorUtils";

interface CircuitBreakerBadgeProps {
  providerId: string;
  appType: string;
  className?: string;
}

/**
 * Open 状态剩余秒数：以最近一次轮询为基准每秒本地倒计时，
 * 避免两次轮询（5 秒）之间数字停住不动。
 */
function useOpenCountdown(
  stats: CircuitBreakerStats | null | undefined,
  updatedAt: number,
): number | null {
  const remaining = stats?.openRemainingSecs ?? null;
  const [now, setNow] = useState(() => Date.now());

  useEffect(() => {
    if (remaining === null) return;
    setNow(Date.now());
    const timer = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(timer);
  }, [remaining, updatedAt]);

  if (remaining === null) return null;
  const elapsed = Math.max(0, Math.floor((now - updatedAt) / 1000));
  return Math.max(0, remaining - elapsed);
}

/**
 * 熔断器实时状态徽章（Closed / Open 剩余时间 / HalfOpen），
 * Open 与 HalfOpen 时提供"立即探测"按钮。
 *
 * 熔断器只存在于运行中的代理内存里：尚无实例（代理未运行或该供应商还没有请求）时不渲染。
 */
export function CircuitBreakerBadge({
  providerId,
  appType,
  className,
}: CircuitBreakerBadgeProps) {
  const { t } = useTranslation();
  const { data: stats, dataUpdatedAt } = useCircuitBreakerStats(
    providerId,
    appType,
  );
  const probe = useProbeCircuitBreaker();
  const remaining = useOpenCountdown(stats, dataUpdatedAt);

  if (!stats) return null;

  const view = {
    closed: {
      label: t("health.circuitClosed"),
      dot: "bg-green-500",
      tone: "bg-green-500/10 text-green-600 dark:text-green-400",
    },
    open: {
      label: t("health.circuitOpenRemaining", { seconds: remaining ?? 0 }),
      dot: "bg-red-500",
      tone: "bg-red-500/10 text-red-600 dark:text-red-400",
    },
    half_open: {
      label: t("health.circuitHalfOpen"),
      dot: "bg-yellow-500",
      tone: "bg-yellow-500/10 text-yellow-600 dark:text-yellow-400",
    },
  }[stats.state];

  const handleProbe = async () => {
    try {
      const result = await probe.mutateAsync({ providerId, appType });
      if (!result) {
        toast.error(t("health.probeProxyStopped"));
      } else if (!result.probed) {
        toast.info(t("health.probeSkipped", { detail: result.message ?? "" }));
      } else if (result.success) {
        toast.success(t("health.probeSucceeded"));
      } else {
        toast.error(t("health.probeFailed", { detail: result.message ?? "" }));
      }
    } catch (error) {
      toast.error(
        t("health.probeFailed", { detail: extractErrorMessage(error) }),
      );
    }
  };

  return (
    <div className={cn("inline-flex items-center gap-1.5", className)}>
      <div
        role="status"
        data-state={stats.state}
        className={cn(
          "inline-flex items-center gap-1.5 px-2 py-1 rounded-full text-xs font-medium",
          view.tone,
        )}
        title={t("health.circuitStateTitle", { state: stats.state })}
      >
        <div className={cn("w-2 h-2 rounded-full", view.dot)} />
        <span>{view.label}</span>
      </div>
      {stats.state !== "closed" && (
        <Button
          type="button"
          size="sm"
          variant="outline"
          className="h-6 px-2 text-xs"
          disabled={probe.isPending}
          onClick={handleProbe}
        >
          {probe.isPending ? (
            <Loader2 className="h-3 w-3 animate-spin" />
          ) : (
            <Radar className="h-3 w-3" />
          )}
          {probe.isPending ? t("health.probing") : t("health.probeNow")}
        </Button>
      )}
    </div>
  );
}
