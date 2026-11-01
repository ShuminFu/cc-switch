import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import type { Provider } from "@/types";
import { KNOWN_APP_TYPES, type AppType } from "@/types/usage";
import { useProvidersQuery } from "@/lib/query/queries";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import { cn } from "@/lib/utils";
import { extractErrorMessage } from "@/utils/errorUtils";
import {
  computeBudgetProgress,
  formatUsd,
  getProviderLimits,
  hasProviderLimits,
  normalizeLimitInput,
  saveProviderLimits,
  useBudgetEnforcement,
  useProviderLimitStatus,
  useSetBudgetEnforcement,
  type BudgetProgress,
} from "./budgetLimits";

const BAR_CLASS = {
  ok: "bg-emerald-500",
  warn: "bg-amber-500",
  exceeded: "bg-red-500",
} as const;

function BudgetBar({
  label,
  progress,
}: {
  label: string;
  progress: BudgetProgress | null;
}) {
  if (!progress) return null;
  return (
    <div className="space-y-1">
      <div className="flex justify-between text-xs text-muted-foreground">
        <span>{label}</span>
        <span className="tabular-nums">
          {formatUsd(progress.usage)} / {formatUsd(progress.limit)} (
          {Math.round(progress.ratio * 100)}%)
        </span>
      </div>
      <div
        className="h-1.5 w-full overflow-hidden rounded-full bg-muted"
        role="progressbar"
        aria-label={label}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={progress.percent}
      >
        <div
          className={cn("h-full rounded-full", BAR_CLASS[progress.level])}
          style={{ width: `${progress.percent}%` }}
        />
      </div>
    </div>
  );
}

export function BudgetLimitRow({
  provider,
  appId,
}: {
  provider: Provider;
  appId: AppType;
}) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const limits = getProviderLimits(provider);
  const [daily, setDaily] = useState(limits.daily ?? "");
  const [monthly, setMonthly] = useState(limits.monthly ?? "");
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    setDaily(limits.daily ?? "");
    setMonthly(limits.monthly ?? "");
  }, [limits.daily, limits.monthly]);

  const hasLimits = hasProviderLimits(provider);
  const { data: status } = useProviderLimitStatus(
    provider.id,
    appId,
    hasLimits,
  );

  const normalizedDaily = normalizeLimitInput(daily);
  const normalizedMonthly = normalizeLimitInput(monthly);
  const invalid = normalizedDaily === null || normalizedMonthly === null;
  const dirty =
    (normalizedDaily ?? "") !== (limits.daily ?? "") ||
    (normalizedMonthly ?? "") !== (limits.monthly ?? "");

  const handleSave = async () => {
    if (invalid) return;
    setSaving(true);
    try {
      await saveProviderLimits(queryClient, provider, appId, {
        daily: normalizedDaily ?? undefined,
        monthly: normalizedMonthly ?? undefined,
      });
      toast.success(
        t("usage.budget.saved", { defaultValue: "Budget limits saved" }),
      );
    } catch (error) {
      toast.error(
        t("usage.budget.saveFailed", {
          defaultValue: "Failed to save budget limits: {{error}}",
          error: extractErrorMessage(error) || String(error),
        }),
      );
    } finally {
      setSaving(false);
    }
  };

  const dailyLabel = t("usage.budget.daily", {
    defaultValue: "Daily limit (USD)",
  });
  const monthlyLabel = t("usage.budget.monthly", {
    defaultValue: "Monthly limit (USD)",
  });

  return (
    <div
      className="space-y-2 rounded-lg border border-border/50 p-3"
      data-testid={`budget-row-${provider.id}`}
    >
      <div className="flex flex-wrap items-end gap-2">
        <div className="min-w-[120px] flex-1 truncate text-sm font-medium">
          {provider.name}
        </div>
        <Input
          aria-label={`${provider.name} ${dailyLabel}`}
          placeholder={dailyLabel}
          inputMode="decimal"
          value={daily}
          onChange={(e) => setDaily(e.target.value)}
          className={cn(
            "h-8 w-36 text-xs",
            normalizedDaily === null && "border-red-500",
          )}
        />
        <Input
          aria-label={`${provider.name} ${monthlyLabel}`}
          placeholder={monthlyLabel}
          inputMode="decimal"
          value={monthly}
          onChange={(e) => setMonthly(e.target.value)}
          className={cn(
            "h-8 w-36 text-xs",
            normalizedMonthly === null && "border-red-500",
          )}
        />
        <Button
          type="button"
          size="sm"
          className="h-8"
          disabled={!dirty || invalid || saving}
          onClick={() => void handleSave()}
        >
          {t("common.save", { defaultValue: "Save" })}
        </Button>
      </div>
      {invalid && (
        <p className="text-xs text-red-500">
          {t("usage.budget.invalid", {
            defaultValue:
              "Enter a positive amount in USD, or leave empty for no limit.",
          })}
        </p>
      )}
      {hasLimits && status && (
        <div className="grid gap-2 sm:grid-cols-2">
          <BudgetBar
            label={t("usage.budget.todayLabel", { defaultValue: "Today" })}
            progress={computeBudgetProgress(
              status.dailyUsage,
              status.dailyLimit,
            )}
          />
          <BudgetBar
            label={t("usage.budget.monthLabel", {
              defaultValue: "This month",
            })}
            progress={computeBudgetProgress(
              status.monthlyUsage,
              status.monthlyLimit,
            )}
          />
        </div>
      )}
    </div>
  );
}

/** Usage-dashboard panel: per-provider daily / monthly limits and the enforcement switch. */
export function BudgetLimitsPanel() {
  const { t } = useTranslation();
  const [appId, setAppId] = useState<AppType>("claude");
  const { data, isLoading } = useProvidersQuery(appId);
  const { data: enforcement } = useBudgetEnforcement();
  const setEnforcement = useSetBudgetEnforcement();

  const providers = useMemo(
    () => Object.values(data?.providers ?? {}),
    [data?.providers],
  );

  return (
    <div className="space-y-4">
      <div className="flex items-start justify-between gap-4 rounded-lg border border-border/50 p-3">
        <div className="space-y-1">
          <p className="text-sm font-medium">
            {t("usage.budget.enforceTitle", {
              defaultValue: "Block requests when a limit is reached",
            })}
          </p>
          <p className="text-xs text-muted-foreground">
            {t("usage.budget.enforceDescription", {
              defaultValue:
                "Off: limits are shown as progress only. On: the local proxy stops sending requests to a provider once its daily or monthly limit is reached and returns a clear error (or fails over to the next provider in the queue). Costs are estimates based on your pricing table.",
            })}
          </p>
        </div>
        <Switch
          aria-label={t("usage.budget.enforceTitle", {
            defaultValue: "Block requests when a limit is reached",
          })}
          checked={enforcement === true}
          disabled={enforcement === undefined || setEnforcement.isPending}
          onCheckedChange={(checked) =>
            setEnforcement.mutate(checked, {
              onError: (error) =>
                toast.error(extractErrorMessage(error) || String(error)),
            })
          }
        />
      </div>

      <div className="flex flex-wrap gap-1">
        {KNOWN_APP_TYPES.map((type) => (
          <Button
            key={type}
            type="button"
            size="sm"
            variant={type === appId ? "default" : "outline"}
            className="h-7 text-xs"
            onClick={() => setAppId(type)}
          >
            {t(`usage.appFilter.${type}`, { defaultValue: type })}
          </Button>
        ))}
      </div>

      {isLoading ? (
        <div className="h-24 animate-pulse rounded bg-muted" />
      ) : providers.length === 0 ? (
        <p className="text-sm text-muted-foreground">
          {t("usage.budget.noProviders", {
            defaultValue: "No providers configured for this app.",
          })}
        </p>
      ) : (
        <div className="space-y-2">
          {providers.map((provider) => (
            <BudgetLimitRow
              key={provider.id}
              provider={provider}
              appId={appId}
            />
          ))}
        </div>
      )}
    </div>
  );
}
