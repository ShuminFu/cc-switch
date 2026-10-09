import { useTranslation } from "react-i18next";
import { cn } from "@/lib/utils";
import type { ProviderLimitStatus } from "@/types/usage";

interface ProviderSpendLimitBadgeProps {
  status: ProviderLimitStatus;
  className?: string;
}

const formatUsd = (value: string | undefined) => {
  const parsed = Number.parseFloat(value ?? "");
  return Number.isFinite(parsed) ? `$${parsed.toFixed(2)}` : "$0.00";
};

const ratio = (usage: string, limit: string | undefined) => {
  const used = Number.parseFloat(usage);
  const cap = Number.parseFloat(limit ?? "");
  if (!Number.isFinite(used) || !Number.isFinite(cap) || cap <= 0) return 0;
  return used / cap;
};

/**
 * 供应商支出上限徽章
 *
 * 显示今日（优先）或本月支出与上限的比例；70% 以上转黄，达到上限转红。
 * 后端 check_provider_limits 只做统计不拦截，因此这里只是提示。
 */
export function ProviderSpendLimitBadge({
  status,
  className,
}: ProviderSpendLimitBadgeProps) {
  const { t } = useTranslation();

  const hasDaily = Boolean(status.dailyLimit);
  const hasMonthly = Boolean(status.monthlyLimit);
  if (!hasDaily && !hasMonthly) return null;

  const exceeded = status.dailyExceeded || status.monthlyExceeded;
  const dailyRatio = hasDaily ? ratio(status.dailyUsage, status.dailyLimit) : 0;
  const monthlyRatio = hasMonthly
    ? ratio(status.monthlyUsage, status.monthlyLimit)
    : 0;
  const worst = Math.max(dailyRatio, monthlyRatio);

  const tone = exceeded
    ? "bg-red-500/10 text-red-600 dark:text-red-400"
    : worst >= 0.7
      ? "bg-yellow-500/10 text-yellow-600 dark:text-yellow-400"
      : "bg-emerald-500/10 text-emerald-600 dark:text-emerald-400";

  const primary = hasDaily
    ? `${formatUsd(status.dailyUsage)} / ${formatUsd(status.dailyLimit)}`
    : `${formatUsd(status.monthlyUsage)} / ${formatUsd(status.monthlyLimit)}`;

  const tooltipLines: string[] = [];
  if (hasDaily) {
    tooltipLines.push(
      t("spendLimit.daily", {
        defaultValue: "今日 {{used}} / {{limit}}",
        used: formatUsd(status.dailyUsage),
        limit: formatUsd(status.dailyLimit),
      }),
    );
  }
  if (hasMonthly) {
    tooltipLines.push(
      t("spendLimit.monthly", {
        defaultValue: "本月 {{used}} / {{limit}}",
        used: formatUsd(status.monthlyUsage),
        limit: formatUsd(status.monthlyLimit),
      }),
    );
  }
  if (exceeded) {
    tooltipLines.push(
      t("spendLimit.exceeded", { defaultValue: "已超出支出上限" }),
    );
  }

  return (
    <span
      className={cn(
        "inline-flex items-center gap-1 rounded-md px-1.5 py-0.5 text-[10px] font-semibold tabular-nums",
        tone,
        className,
      )}
      title={tooltipLines.join("\n")}
      data-testid="spend-limit-badge"
    >
      {exceeded && (
        <span>{t("spendLimit.overBudget", { defaultValue: "超支" })}</span>
      )}
      <span>{primary}</span>
    </span>
  );
}
