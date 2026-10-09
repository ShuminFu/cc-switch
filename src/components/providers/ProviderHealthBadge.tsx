import { RotateCcw } from "lucide-react";
import { cn } from "@/lib/utils";
import { ProviderHealthStatus } from "@/types/proxy";
import type { CircuitBreakerStats, CircuitState } from "@/types/proxy";
import { useTranslation } from "react-i18next";

interface ProviderHealthBadgeProps {
  consecutiveFailures: number;
  isHealthy?: boolean;
  className?: string;
  /** Live breaker stats from the running proxy; null until the breaker exists */
  stats?: CircuitBreakerStats | null;
  /** Last upstream error recorded for this provider */
  lastError?: string | null;
  /** When provided, a reset control is shown while the circuit is open */
  onReset?: () => void;
  isResetting?: boolean;
}

const STATE_KEYS: Record<CircuitState, string> = {
  closed: "health.state.closed",
  open: "health.state.open",
  half_open: "health.state.halfOpen",
};

const STATE_FALLBACKS: Record<CircuitState, string> = {
  closed: "已闭合",
  open: "已熔断",
  half_open: "半开（探测中）",
};

const MAX_ERROR_CHARS = 200;

/**
 * 供应商健康状态徽章
 * 根据连续失败次数显示不同颜色的状态指示器；熔断时附带熔断器详情与手动重置入口
 */
export function ProviderHealthBadge({
  consecutiveFailures,
  isHealthy,
  className,
  stats,
  lastError,
  onReset,
  isResetting = false,
}: ProviderHealthBadgeProps) {
  const { t } = useTranslation();

  // 根据失败次数计算状态
  const getStatus = () => {
    if (consecutiveFailures === 0) {
      return {
        labelKey: "health.operational",
        labelFallback: "正常",
        status: ProviderHealthStatus.Healthy,
        color: "bg-green-500",
        // 使用更深/柔和的背景色，去除可能的白色内容感
        bgColor: "bg-green-500/10",
        textColor: "text-green-600 dark:text-green-400",
      };
    } else if (isHealthy !== false) {
      return {
        labelKey: "health.degraded",
        labelFallback: "降级",
        status: ProviderHealthStatus.Degraded,
        color: "bg-yellow-500",
        bgColor: "bg-yellow-500/10",
        textColor: "text-yellow-600 dark:text-yellow-400",
      };
    } else {
      return {
        labelKey: "health.circuitOpen",
        labelFallback: "熔断",
        status: ProviderHealthStatus.Failed,
        color: "bg-red-500",
        bgColor: "bg-red-500/10",
        textColor: "text-red-600 dark:text-red-400",
      };
    }
  };

  const statusConfig = getStatus();
  const label = t(statusConfig.labelKey, {
    defaultValue: statusConfig.labelFallback,
  });

  const tooltipLines = [
    t("health.consecutiveFailures", {
      count: consecutiveFailures,
      defaultValue: `连续失败 ${consecutiveFailures} 次`,
    }),
  ];
  if (stats) {
    tooltipLines.push(
      t("health.circuitState", {
        defaultValue: "熔断器：{{state}}",
        state: t(STATE_KEYS[stats.state], {
          defaultValue: STATE_FALLBACKS[stats.state],
        }),
      }),
    );
    if (stats.state === "open" && stats.retryAfterSeconds != null) {
      tooltipLines.push(
        t("health.retryIn", {
          defaultValue: "{{seconds}} 秒后再次探测",
          seconds: stats.retryAfterSeconds,
        }),
      );
    }
  }
  if (lastError) {
    const trimmed =
      lastError.length > MAX_ERROR_CHARS
        ? `${lastError.slice(0, MAX_ERROR_CHARS)}…`
        : lastError;
    tooltipLines.push(
      t("health.lastError", {
        defaultValue: "最近错误：{{error}}",
        error: trimmed,
      }),
    );
  }

  const showReset =
    typeof onReset === "function" &&
    statusConfig.status === ProviderHealthStatus.Failed;
  const resetLabel = t("health.reset", { defaultValue: "重置熔断器" });

  return (
    <span className="inline-flex items-center gap-1">
      <div
        className={cn(
          "inline-flex items-center gap-1.5 px-2 py-1 rounded-full text-xs font-medium",
          statusConfig.bgColor,
          statusConfig.textColor,
          className,
        )}
        title={tooltipLines.join("\n")}
      >
        <div className={cn("w-2 h-2 rounded-full", statusConfig.color)} />
        <span>{label}</span>
      </div>
      {showReset && (
        <button
          type="button"
          onClick={(event) => {
            event.stopPropagation();
            onReset?.();
          }}
          disabled={isResetting}
          className="inline-flex items-center justify-center rounded-full p-1 text-red-600 transition-colors hover:bg-red-500/20 disabled:opacity-50 dark:text-red-400"
          title={resetLabel}
          aria-label={resetLabel}
        >
          <RotateCcw className={cn("h-3 w-3", isResetting && "animate-spin")} />
        </button>
      )}
    </span>
  );
}
