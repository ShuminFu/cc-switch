import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { getUsageRangePresetLabel, resolveUsageRange } from "@/lib/usageRange";

const sec = (d: Date) => Math.floor(d.getTime() / 1000);

describe("resolveUsageRange", () => {
  const now = new Date(2026, 4, 20, 15, 30, 45, 500); // local time
  const nowMs = now.getTime();
  const nowSec = Math.floor(nowMs / 1000);

  it("today starts at local midnight and ends now", () => {
    expect(resolveUsageRange({ preset: "today" }, nowMs)).toEqual({
      startDate: sec(new Date(2026, 4, 20)),
      endDate: nowSec,
    });
  });

  it("1d is a rolling 24h window, not calendar aligned", () => {
    expect(resolveUsageRange({ preset: "1d" }, nowMs)).toEqual({
      startDate: nowSec - 86400,
      endDate: nowSec,
    });
  });

  it.each([
    ["7d", new Date(2026, 4, 14)],
    ["14d", new Date(2026, 4, 7)],
    ["30d", new Date(2026, 3, 21)],
  ] as const)("%s covers N calendar days including today", (preset, start) => {
    const range = resolveUsageRange({ preset }, nowMs);
    expect(range.startDate).toBe(sec(start));
    expect(range.endDate).toBe(nowSec);
  });

  it("30d crosses month boundaries by calendar day", () => {
    const march3 = new Date(2026, 2, 3, 9, 0).getTime();
    expect(resolveUsageRange({ preset: "30d" }, march3).startDate).toBe(
      sec(new Date(2026, 1, 2)),
    );
  });

  it("custom without bounds defaults to last 24h", () => {
    expect(resolveUsageRange({ preset: "custom" }, nowMs)).toEqual({
      startDate: nowSec - 86400,
      endDate: nowSec,
    });
  });

  it("custom keeps fixed bounds unless liveEndTime is set", () => {
    const selection = {
      preset: "custom" as const,
      customStartDate: 1000,
      customEndDate: 2000,
    };
    expect(resolveUsageRange(selection, nowMs)).toEqual({
      startDate: 1000,
      endDate: 2000,
    });
    expect(
      resolveUsageRange({ ...selection, liveEndTime: true }, nowMs),
    ).toEqual({ startDate: 1000, endDate: nowSec });
  });

  it("custom treats epoch 0 as a real start, not a missing value", () => {
    expect(
      resolveUsageRange({ preset: "custom", customStartDate: 0 }, nowMs)
        .startDate,
    ).toBe(0);
  });

  describe("across a DST transition", () => {
    const originalTz = process.env.TZ;
    beforeEach(() => {
      process.env.TZ = "America/New_York";
    });
    afterEach(() => {
      if (originalTz === undefined) delete process.env.TZ;
      else process.env.TZ = originalTz;
    });

    it("7d starts exactly 6 calendar days back shortly after midnight", () => {
      // 2026-03-08 is the spring-forward day in New York, so the preceding
      // week is one hour shorter than 7 * 24h.
      const justAfterMidnight = new Date(2026, 2, 10, 0, 30).getTime();
      const range = resolveUsageRange({ preset: "7d" }, justAfterMidnight);
      expect(new Date(range.startDate * 1000).getHours()).toBe(0);
      expect(range.startDate).toBe(sec(new Date(2026, 2, 4)));
    });
  });
});

describe("getUsageRangePresetLabel", () => {
  it("passes an i18n key with a default for every preset", () => {
    const t = (key: string, opts?: { defaultValue?: string }) =>
      `${key}|${opts?.defaultValue}`;
    expect(getUsageRangePresetLabel("7d", t)).toBe("usage.preset7d|7d");
    expect(getUsageRangePresetLabel("custom", t)).toBe(
      "usage.customRange|日历筛选",
    );
  });
});
