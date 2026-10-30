import { useTranslation } from "react-i18next";
import type { Provider } from "@/types";
import { cn } from "@/lib/utils";
import {
  computeBudgetProgress,
  formatUsd,
  hasProviderLimits,
  useProviderLimitStatus,
  worstBudgetProgress,
  type BudgetLevel,
} from "./budgetLimits";

const LEVEL_CLASS: Record<BudgetLevel, string> = {
  ok: "bg-emerald-100 text-emerald-700 dark:bg-emerald-900/40 dark:text-emerald-300",
  warn: "bg-amber-100 text-amber-700 dark:bg-amber-900/40 dark:text-amber-300",
  exceeded: "bg-red-100 text-red-700 dark:bg-red-900/40 dark:text-red-300",
};

interface BudgetLimitBadgeProps {
  provider: Provider;
  appId: string;
}

/** Compact budget pill for the provider card; renders nothing when no limit is set. */
export function BudgetLimitBadge({ provider, appId }: BudgetLimitBadgeProps) {
  const { t } = useTranslation();
  const hasLimits = hasProviderLimits(provider);
  const { data: status } = useProviderLimitStatus(
    provider.id,
    appId,
    hasLimits,
  );

  if (!hasLimits) return null;
  const worst = worstBudgetProgress(status);
  if (!worst) return null;

  const { period, progress } = worst;
  const periodLabel =
    period === "daily"
      ? t("usage.budget.today", { defaultValue: "today" })
      : t("usage.budget.thisMonth", { defaultValue: "this month" });

  const daily = computeBudgetProgress(status?.dailyUsage, status?.dailyLimit);
  const monthly = computeBudgetProgress(
    status?.monthlyUsage,
    status?.monthlyLimit,
  );
  const titleParts = [
    daily &&
      t("usage.budget.dailyTooltip", {
        defaultValue: "Daily: {{usage}} of {{limit}}",
        usage: formatUsd(daily.usage),
        limit: formatUsd(daily.limit),
      }),
    monthly &&
      t("usage.budget.monthlyTooltip", {
        defaultValue: "Monthly: {{usage}} of {{limit}}",
        usage: formatUsd(monthly.usage),
        limit: formatUsd(monthly.limit),
      }),
    progress.level === "exceeded" &&
      t("usage.budget.exceededTooltip", {
        defaultValue: "Budget limit reached",
      }),
  ].filter(Boolean);

  return (
    <span
      data-testid="budget-limit-badge"
      data-level={progress.level}
      className={cn(
        "inline-flex items-center rounded-md px-1.5 py-0.5 text-[10px] font-semibold tabular-nums",
        LEVEL_CLASS[progress.level],
      )}
      title={titleParts.join("\n")}
    >
      {`${formatUsd(progress.usage)} / ${formatUsd(progress.limit)} ${periodLabel}`}
    </span>
  );
}
