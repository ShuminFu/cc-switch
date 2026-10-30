import { invoke } from "@tauri-apps/api/core";
import {
  useMutation,
  useQuery,
  useQueryClient,
  type QueryClient,
} from "@tanstack/react-query";
import type { Provider } from "@/types";
import type { ProviderLimitStatus } from "@/types/usage";
import { providersApi, type AppId } from "@/lib/api";
import { usageApi } from "@/lib/api/usage";
import { usageKeys } from "@/lib/query/usage";
import { queryKeys } from "@/lib/query/keys";

/**
 * Budget limits (PRODUCT-05).
 *
 * Limits are stored in provider meta as decimal strings (`limitDailyUsd` /
 * `limitMonthlyUsd`), which the backend `check_provider_limits` already reads.
 * The enforcement switch lives in the backend settings table and is read by
 * the proxy through `services::budget::check_hard_limit`.
 */

export const LIMIT_DAILY_KEY = "limitDailyUsd";
export const LIMIT_MONTHLY_KEY = "limitMonthlyUsd";

/** Usage ratio from which a limit is shown as "close to the limit". */
export const BUDGET_WARN_RATIO = 0.8;

export interface ProviderBudgetLimits {
  daily?: string;
  monthly?: string;
}

export type BudgetLevel = "ok" | "warn" | "exceeded";

export interface BudgetProgress {
  usage: number;
  limit: number;
  /** usage / limit, not clamped (can exceed 1). */
  ratio: number;
  /** Clamped 0-100, for progress bars. */
  percent: number;
  level: BudgetLevel;
}

/** Parse a stored limit string; anything that is not a positive number means "no limit". */
export function parseLimit(value: unknown): number | null {
  if (typeof value !== "string" && typeof value !== "number") return null;
  const parsed = typeof value === "number" ? value : Number(value.trim());
  if (!Number.isFinite(parsed) || parsed <= 0) return null;
  return parsed;
}

/**
 * Validate a limit typed by the user.
 * Empty input clears the limit (`undefined`); invalid input returns `null`.
 */
export function normalizeLimitInput(input: string): string | undefined | null {
  const trimmed = input.trim();
  if (trimmed === "") return undefined;
  if (!/^\d+(\.\d+)?$/.test(trimmed)) return null;
  const parsed = Number(trimmed);
  if (!Number.isFinite(parsed) || parsed <= 0) return null;
  return trimmed;
}

/** Read the configured limits from provider meta. */
export function getProviderLimits(
  provider: Pick<Provider, "meta">,
): ProviderBudgetLimits {
  const meta = (provider.meta ?? {}) as Record<string, unknown>;
  const daily = parseLimit(meta[LIMIT_DAILY_KEY]);
  const monthly = parseLimit(meta[LIMIT_MONTHLY_KEY]);
  return {
    daily: daily != null ? String(meta[LIMIT_DAILY_KEY]).trim() : undefined,
    monthly:
      monthly != null ? String(meta[LIMIT_MONTHLY_KEY]).trim() : undefined,
  };
}

export function hasProviderLimits(provider: Pick<Provider, "meta">): boolean {
  const limits = getProviderLimits(provider);
  return limits.daily != null || limits.monthly != null;
}

/** Return a copy of the provider with the given limits written to (or removed from) meta. */
export function applyProviderLimits(
  provider: Provider,
  limits: ProviderBudgetLimits,
): Provider {
  const meta = { ...(provider.meta ?? {}) } as Record<string, unknown>;
  if (limits.daily) meta[LIMIT_DAILY_KEY] = limits.daily;
  else delete meta[LIMIT_DAILY_KEY];
  if (limits.monthly) meta[LIMIT_MONTHLY_KEY] = limits.monthly;
  else delete meta[LIMIT_MONTHLY_KEY];
  return { ...provider, meta: meta as Provider["meta"] };
}

export function computeBudgetProgress(
  usage: string | number | undefined,
  limit: string | number | undefined | null,
): BudgetProgress | null {
  const parsedLimit = parseLimit(limit ?? undefined);
  if (parsedLimit == null) return null;
  const parsedUsage = Number(usage ?? 0);
  const safeUsage = Number.isFinite(parsedUsage) ? Math.max(parsedUsage, 0) : 0;
  const ratio = safeUsage / parsedLimit;
  const level: BudgetLevel =
    ratio >= 1 ? "exceeded" : ratio >= BUDGET_WARN_RATIO ? "warn" : "ok";
  return {
    usage: safeUsage,
    limit: parsedLimit,
    ratio,
    percent: Math.min(100, Math.round(ratio * 100)),
    level,
  };
}

/** The most severe of the daily / monthly progress entries, for compact badges. */
export function worstBudgetProgress(
  status: ProviderLimitStatus | undefined,
): { period: "daily" | "monthly"; progress: BudgetProgress } | null {
  if (!status) return null;
  const daily = computeBudgetProgress(status.dailyUsage, status.dailyLimit);
  const monthly = computeBudgetProgress(
    status.monthlyUsage,
    status.monthlyLimit,
  );
  if (daily && monthly) {
    return daily.ratio >= monthly.ratio
      ? { period: "daily", progress: daily }
      : { period: "monthly", progress: monthly };
  }
  if (daily) return { period: "daily", progress: daily };
  if (monthly) return { period: "monthly", progress: monthly };
  return null;
}

export function formatUsd(value: number): string {
  return `$${value.toFixed(2)}`;
}

// ---------------------------------------------------------------------------
// API + query hooks
// ---------------------------------------------------------------------------

export const budgetApi = {
  getEnforcement: (): Promise<boolean> => invoke("get_budget_enforcement"),
  setEnforcement: (enabled: boolean): Promise<boolean> =>
    invoke("set_budget_enforcement", { enabled }),
};

export const budgetKeys = queryKeys.budget;

const LIMITS_REFETCH_MS = 60_000;

export function useProviderLimitStatus(
  providerId: string,
  appType: string,
  enabled = true,
) {
  return useQuery({
    queryKey: usageKeys.limits(providerId, appType),
    queryFn: () => usageApi.checkProviderLimits(providerId, appType),
    enabled: enabled && !!providerId && !!appType,
    refetchInterval: enabled ? LIMITS_REFETCH_MS : false,
    refetchIntervalInBackground: false,
  });
}

export function useBudgetEnforcement() {
  return useQuery({
    queryKey: budgetKeys.enforcement,
    queryFn: budgetApi.getEnforcement,
  });
}

export function useSetBudgetEnforcement() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: budgetApi.setEnforcement,
    onSuccess: (enabled) => {
      queryClient.setQueryData(budgetKeys.enforcement, enabled);
    },
  });
}

export async function saveProviderLimits(
  queryClient: QueryClient,
  provider: Provider,
  appId: AppId,
  limits: ProviderBudgetLimits,
): Promise<Provider> {
  const next = applyProviderLimits(provider, limits);
  await providersApi.update(next, appId);
  await Promise.all([
    queryClient.invalidateQueries({
      queryKey: queryKeys.providers.byApp(appId),
    }),
    queryClient.invalidateQueries({
      queryKey: usageKeys.limits(provider.id, appId),
    }),
  ]);
  return next;
}
