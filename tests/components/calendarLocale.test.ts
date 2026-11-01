import { describe, expect, it } from "vitest";
import {
  MAX_CUSTOM_RANGE_DAYS,
  exceedsMaxRange,
  getCalendarDays,
  getFirstDayOfWeek,
  getWeekdayLabels,
} from "@/components/usage/calendarLocale";

describe("usage calendar locale helpers (FE-UI-27)", () => {
  it("starts the week on Monday for zh-CN and Sunday for en-US", () => {
    expect(getFirstDayOfWeek("zh-CN")).toBe(1);
    expect(getFirstDayOfWeek("en-US")).toBe(0);
  });

  it("builds a 6-week grid starting on the requested weekday", () => {
    // March 2026 starts on a Sunday.
    const month = new Date(2026, 2, 1);
    const sundayFirst = getCalendarDays(month, 0);
    const mondayFirst = getCalendarDays(month, 1);
    expect(sundayFirst).toHaveLength(42);
    expect(sundayFirst[0].getDay()).toBe(0);
    expect(sundayFirst[0].getDate()).toBe(1);
    expect(mondayFirst[0].getDay()).toBe(1);
    // Monday-first: the 1st (a Sunday) is the last cell of the first row.
    expect(mondayFirst[6].getDate()).toBe(1);
  });

  it("orders weekday labels from the first day of the week", () => {
    const labels = getWeekdayLabels("en-US", 1);
    expect(labels).toHaveLength(7);
    expect(labels[0]).toBe("M");
    expect(labels[6]).toBe("S");
  });

  it("rejects ranges longer than the limit", () => {
    const day = 24 * 60 * 60;
    expect(exceedsMaxRange(0, MAX_CUSTOM_RANGE_DAYS * day)).toBe(false);
    expect(exceedsMaxRange(0, MAX_CUSTOM_RANGE_DAYS * day + 1)).toBe(true);
  });
});
