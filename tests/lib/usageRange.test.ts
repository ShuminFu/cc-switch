import { describe, expect, it } from "vitest";
import { getUsageRangePresetLabel, resolveUsageRange } from "@/lib/usageRange";

// 使用本地时间构造基准时刻，避免时区影响月份边界
const NOW_MS = new Date(2026, 2, 15, 10, 30, 0).getTime(); // 2026-03-15 10:30

const localSeconds = (year: number, month: number, day: number) =>
  Math.floor(new Date(year, month, day).getTime() / 1000);

describe("resolveUsageRange month presets", () => {
  it("thisMonth starts at the first day of the current month and ends now", () => {
    const range = resolveUsageRange({ preset: "thisMonth" }, NOW_MS);
    expect(range.startDate).toBe(localSeconds(2026, 2, 1));
    expect(range.endDate).toBe(Math.floor(NOW_MS / 1000));
  });

  it("lastMonth spans the whole previous calendar month", () => {
    const range = resolveUsageRange({ preset: "lastMonth" }, NOW_MS);
    expect(range.startDate).toBe(localSeconds(2026, 1, 1));
    expect(range.endDate).toBe(localSeconds(2026, 2, 1) - 1);
  });

  it("lastMonth rolls back across the year boundary in January", () => {
    const january = new Date(2026, 0, 5, 8, 0, 0).getTime();
    const range = resolveUsageRange({ preset: "lastMonth" }, january);
    expect(range.startDate).toBe(localSeconds(2025, 11, 1));
    expect(range.endDate).toBe(localSeconds(2026, 0, 1) - 1);
  });

  it("keeps the 30d preset anchored to local midnight 29 days back", () => {
    const range = resolveUsageRange({ preset: "30d" }, NOW_MS);
    expect(range.startDate).toBe(localSeconds(2026, 1, 14));
    expect(range.endDate).toBe(Math.floor(NOW_MS / 1000));
  });

  it("labels the month presets", () => {
    const t = (key: string, options?: { defaultValue?: string }) =>
      options?.defaultValue ?? key;
    expect(getUsageRangePresetLabel("thisMonth", t)).toBe("本月");
    expect(getUsageRangePresetLabel("lastMonth", t)).toBe("上月");
  });
});
