import type { DailyStats } from "@/types/usage";
import { parseFiniteNumber } from "./format";

/** Upper bound on rendered points; longer series are aggregated (PERF-21). */
export const MAX_TREND_POINTS = 180;

export interface TrendPoint {
  rawDate: string;
  label: string;
  inputTokens: number;
  outputTokens: number;
  cacheCreationTokens: number;
  cacheReadTokens: number;
  cost: number | null;
}

const SUMMED_KEYS = [
  "inputTokens",
  "outputTokens",
  "cacheCreationTokens",
  "cacheReadTokens",
] as const;

export function buildTrendPoints(
  trends: DailyStats[] | undefined,
  isHourly: boolean,
  dateLocale: string,
): TrendPoint[] {
  if (!trends) return [];
  return trends.map((stat) => {
    const pointDate = new Date(stat.date);
    return {
      rawDate: stat.date,
      label: isHourly
        ? pointDate.toLocaleString(dateLocale, {
            month: "2-digit",
            day: "2-digit",
            hour: "2-digit",
            minute: "2-digit",
          })
        : pointDate.toLocaleDateString(dateLocale, {
            month: "2-digit",
            day: "2-digit",
          }),
      inputTokens: stat.totalInputTokens,
      outputTokens: stat.totalOutputTokens,
      cacheCreationTokens: stat.totalCacheCreationTokens,
      cacheReadTokens: stat.totalCacheReadTokens,
      cost: parseFiniteNumber(stat.totalCost),
    };
  });
}

/**
 * Aggregate consecutive buckets so at most `maxPoints` remain. Token counts
 * and cost are summed (totals are preserved); each group keeps the label of
 * its first bucket.
 */
export function downsampleTrendPoints(
  points: TrendPoint[],
  maxPoints: number = MAX_TREND_POINTS,
): TrendPoint[] {
  if (points.length <= maxPoints || maxPoints < 1) return points;
  const groupSize = Math.ceil(points.length / maxPoints);
  const result: TrendPoint[] = [];
  for (let i = 0; i < points.length; i += groupSize) {
    const group = points.slice(i, i + groupSize);
    const merged: TrendPoint = { ...group[0] };
    for (const key of SUMMED_KEYS) {
      merged[key] = group.reduce((sum, p) => sum + (p[key] || 0), 0);
    }
    const costs = group
      .map((p) => p.cost)
      .filter((c): c is number => c != null);
    merged.cost = costs.length ? costs.reduce((a, b) => a + b, 0) : null;
    result.push(merged);
  }
  return result;
}

function trimFixed(value: number, digits: number): string {
  return value.toFixed(digits).replace(/\.?0+$/, "");
}

/** Compact token axis tick: `0`, `500`, `1.5K`, `1.5M`, `2B`. */
export function formatTokenTick(value: number): string {
  if (!Number.isFinite(value) || value === 0) return "0";
  const sign = value < 0 ? "-" : "";
  const abs = Math.abs(value);
  if (abs >= 1e9) return `${sign}${trimFixed(abs / 1e9, 1)}B`;
  if (abs >= 1e6) return `${sign}${trimFixed(abs / 1e6, 1)}M`;
  if (abs >= 1e3) return `${sign}${trimFixed(abs / 1e3, 1)}K`;
  return `${sign}${Math.round(abs)}`;
}

/** Cost axis tick without long float tails: `$0`, `$0.0012`, `$1.5`, `$120`. */
export function formatCostTick(value: number): string {
  if (!Number.isFinite(value) || value === 0) return "$0";
  const abs = Math.abs(value);
  const digits = abs >= 100 ? 0 : abs >= 1 ? 2 : 4;
  return `${value < 0 ? "-" : ""}$${trimFixed(abs, digits)}`;
}
