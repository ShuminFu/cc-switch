import { useEffect, useMemo } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Wallet } from "lucide-react";
import type { Provider } from "@/types";
import { KNOWN_APP_TYPES, type AppType } from "@/types/usage";
import { useProvidersQuery } from "@/lib/query/queries";
import { cn } from "@/lib/utils";
import {
  claimBudgetExceededAlert,
  formatUsd,
  hasProviderLimits,
  useProviderLimitStatus,
  worstBudgetProgress,
  type BudgetLevel,
} from "./budgetLimits";

/**
 * Budget summary under the usage hero (FE-UI-26): one progress chip per
 * provider that has a daily/monthly limit, coloured by how close it is, and
 * a one-time warning toast per session when a limit is reached.
 */

const LEVEL_BAR: Record<BudgetLevel, string> = {
  ok: "bg-emerald-500",
  warn: "bg-amber-500",
  exceeded: "bg-red-500",
};

interface BudgetHeroStripProps {
  appType?: AppType;
}

export function BudgetHeroStrip({ appType }: BudgetHeroStripProps) {
  const apps = appType ? [appType] : KNOWN_APP_TYPES;
  return (
    <div className="flex flex-wrap gap-2" data-testid="budget-hero-strip">
      {apps.map((app) => (
        <AppBudgets key={app} appId={app} />
      ))}
    </div>
  );
}

function AppBudgets({ appId }: { appId: AppType }) {
  const { data } = useProvidersQuery(appId);
  const limited = useMemo(
    () => Object.values(data?.providers ?? {}).filter(hasProviderLimits),
    [data?.providers],
  );
  if (limited.length === 0) return null;
  return (
    <>
      {limited.map((provider) => (
        <BudgetChip key={provider.id} provider={provider} appId={appId} />
      ))}
    </>
  );
}

function BudgetChip({
  provider,
  appId,
}: {
  provider: Provider;
  appId: string;
}) {
  const { t } = useTranslation();
  const { data: status } = useProviderLimitStatus(provider.id, appId);
  const worst = worstBudgetProgress(status);

  const exceededKey =
    worst?.progress.level === "exceeded"
      ? `${appId}:${provider.id}:${worst.period}`
      : null;
  useEffect(() => {
    if (!exceededKey || !claimBudgetExceededAlert(exceededKey)) return;
    toast.warning(
      t("usage.budget.exceededToast", {
        defaultValue: "{{provider}} reached its {{period}} budget",
        provider: provider.name,
        period:
          worst?.period === "daily"
            ? t("usage.budget.daily", { defaultValue: "daily" })
            : t("usage.budget.monthly", { defaultValue: "monthly" }),
      }),
      { closeButton: true },
    );
  }, [exceededKey, provider.name, t, worst?.period]);

  if (!worst) return null;
  const { progress, period } = worst;
  return (
    <div
      className="flex min-w-[180px] items-center gap-2 rounded-lg border border-border/50 bg-card/40 px-3 py-2 text-xs"
      data-testid={`budget-chip-${provider.id}`}
      data-level={progress.level}
    >
      <Wallet className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
      <div className="min-w-0 flex-1 space-y-1">
        <div className="flex items-center justify-between gap-2">
          <span className="truncate font-medium">{provider.name}</span>
          <span
            className={cn(
              "tabular-nums",
              progress.level === "exceeded" && "font-semibold text-red-600",
            )}
          >
            {`${formatUsd(progress.usage)} / ${formatUsd(progress.limit)}`}
          </span>
        </div>
        <div
          className="h-1.5 w-full overflow-hidden rounded-full bg-muted"
          role="progressbar"
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={progress.percent}
          aria-label={
            period === "daily"
              ? t("usage.budget.today", { defaultValue: "today" })
              : t("usage.budget.thisMonth", { defaultValue: "this month" })
          }
        >
          <div
            className={cn("h-full", LEVEL_BAR[progress.level])}
            style={{ width: `${progress.percent}%` }}
          />
        </div>
        {progress.level === "exceeded" && (
          <span className="text-[10px] font-semibold uppercase text-red-600">
            {t("usage.budget.exceededBadge", { defaultValue: "Exceeded" })}
          </span>
        )}
      </div>
    </div>
  );
}
