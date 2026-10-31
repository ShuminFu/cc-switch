import { render } from "@testing-library/react";
import type { ReactNode } from "react";
import { describe, expect, it, vi, beforeEach } from "vitest";
import type { DailyStats } from "@/types/usage";
import {
  buildTrendPoints,
  downsampleTrendPoints,
  formatCostTick,
  formatTokenTick,
  MAX_TREND_POINTS,
} from "@/components/usage/trendChartData";

const areaChartRenders = vi.hoisted(() => ({ count: 0 }));
const trendsState = vi.hoisted(() => ({
  data: [] as DailyStats[],
}));

vi.mock("recharts", () => {
  const Pass = ({ children }: { children?: ReactNode }) => <>{children}</>;
  return {
    ResponsiveContainer: Pass,
    AreaChart: ({ children }: { children?: ReactNode }) => {
      areaChartRenders.count += 1;
      return <div data-testid="area-chart">{children}</div>;
    },
    Area: () => null,
    XAxis: () => null,
    YAxis: () => null,
    CartesianGrid: () => null,
    Tooltip: () => null,
    Legend: () => null,
  };
});

vi.mock("@/lib/query/usage", () => ({
  useUsageTrends: () => ({ data: trendsState.data, isLoading: false }),
}));

const stableT = vi.hoisted(() => (key: string) => key);
const stableI18n = vi.hoisted(() => ({
  resolvedLanguage: "en",
  language: "en",
}));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: stableT, i18n: stableI18n }),
}));

import { UsageTrendChart } from "@/components/usage/UsageTrendChart";

const stat = (date: string, input: number, cost = "0.5"): DailyStats => ({
  date,
  requestCount: 1,
  totalCost: cost,
  totalTokens: input,
  totalInputTokens: input,
  totalOutputTokens: 1,
  totalCacheCreationTokens: 0,
  totalCacheReadTokens: 0,
});

describe("trend chart helpers (PERF-21 / FE-UI-28)", () => {
  it("formats token ticks compactly", () => {
    expect(formatTokenTick(0)).toBe("0");
    expect(formatTokenTick(500)).toBe("500");
    expect(formatTokenTick(1500)).toBe("1.5K");
    expect(formatTokenTick(1_500_000)).toBe("1.5M");
    expect(formatTokenTick(2_000_000_000)).toBe("2B");
  });

  it("formats cost ticks without float tails", () => {
    expect(formatCostTick(0)).toBe("$0");
    expect(formatCostTick(0.1 + 0.2)).toBe("$0.3");
    expect(formatCostTick(0.00123456)).toBe("$0.0012");
    expect(formatCostTick(12.345)).toBe("$12.35");
    expect(formatCostTick(1234.5)).toBe("$1235");
  });

  it("downsamples long series and preserves totals", () => {
    const points = buildTrendPoints(
      Array.from({ length: 365 }, (_, i) =>
        stat(new Date(Date.UTC(2025, 0, 1 + i)).toISOString(), 10, "1"),
      ),
      false,
      "en-US",
    );
    const sampled = downsampleTrendPoints(points);
    expect(sampled.length).toBeLessThanOrEqual(MAX_TREND_POINTS);
    expect(sampled.reduce((s, p) => s + p.inputTokens, 0)).toBe(3650);
    expect(sampled.reduce((s, p) => s + (p.cost ?? 0), 0)).toBeCloseTo(365);
  });

  it("leaves short series untouched", () => {
    const points = buildTrendPoints([stat("2025-01-01", 5)], false, "en-US");
    expect(downsampleTrendPoints(points)).toBe(points);
  });
});

describe("UsageTrendChart rendering", () => {
  beforeEach(() => {
    areaChartRenders.count = 0;
  });

  const props = {
    range: { preset: "7d" as const },
    rangeLabel: "7d",
    refreshIntervalMs: 0,
  };

  it("does not re-render the chart when a refetch returns the same data", () => {
    trendsState.data = [stat("2025-01-01", 5), stat("2025-01-02", 6)];
    const { rerender } = render(<UsageTrendChart {...props} />);
    const afterMount = areaChartRenders.count;
    expect(afterMount).toBeGreaterThan(0);

    rerender(<UsageTrendChart {...props} />);
    rerender(<UsageTrendChart {...props} />);
    expect(areaChartRenders.count).toBe(afterMount);

    trendsState.data = [...trendsState.data, stat("2025-01-03", 7)];
    rerender(<UsageTrendChart {...props} />);
    expect(areaChartRenders.count).toBe(afterMount + 1);
  });
});
