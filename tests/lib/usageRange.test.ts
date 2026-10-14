import { describe, expect, it } from "vitest";
import { getUsageRangePresetLabel, resolveUsageRange } from "@/lib/usageRange";

// Every test injects both the instant and the IANA zone, so results do not
// depend on the machine's TZ and nothing mutates process.env.TZ.
const utc = (iso: string) => Date.parse(iso);
const sec = (iso: string) => Math.floor(Date.parse(iso) / 1000);

describe("resolveUsageRange", () => {
  const shanghai = {
    now: utc("2026-05-20T07:30:45.500Z"),
    tz: "Asia/Shanghai",
  }; // 15:30:45 local
  const nowSec = Math.floor(shanghai.now / 1000);

  it("today starts at midnight in the given zone and ends now", () => {
    expect(resolveUsageRange({ preset: "today" }, shanghai)).toEqual({
      startDate: sec("2026-05-19T16:00:00Z"), // 2026-05-20 00:00 +08:00
      endDate: nowSec,
    });
  });

  it("today uses the zone's calendar day, not UTC's", () => {
    // 2026-05-20 01:00 in Shanghai is still 2026-05-19 in UTC
    const range = resolveUsageRange(
      { preset: "today" },
      { now: utc("2026-05-19T17:00:00Z"), tz: "Asia/Shanghai" },
    );
    expect(range.startDate).toBe(sec("2026-05-19T16:00:00Z"));
  });

  it("1d is a rolling 24h window, not calendar aligned", () => {
    expect(resolveUsageRange({ preset: "1d" }, shanghai)).toEqual({
      startDate: nowSec - 86400,
      endDate: nowSec,
    });
  });

  it.each([
    ["7d", "2026-05-13T16:00:00Z"], // local 05-14
    ["14d", "2026-05-06T16:00:00Z"], // local 05-07
    ["30d", "2026-04-20T16:00:00Z"], // local 04-21
  ] as const)("%s covers N calendar days including today", (preset, start) => {
    const range = resolveUsageRange({ preset }, shanghai);
    expect(range.startDate).toBe(sec(start));
    expect(range.endDate).toBe(nowSec);
  });

  it("30d crosses month boundaries by calendar day", () => {
    const range = resolveUsageRange(
      { preset: "30d" },
      { now: utc("2026-03-03T01:00:00Z"), tz: "Asia/Shanghai" }, // 03-03 09:00
    );
    expect(range.startDate).toBe(sec("2026-02-01T16:00:00Z")); // local 02-02
  });

  it("custom without bounds defaults to last 24h", () => {
    expect(resolveUsageRange({ preset: "custom" }, shanghai)).toEqual({
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
    expect(resolveUsageRange(selection, shanghai)).toEqual({
      startDate: 1000,
      endDate: 2000,
    });
    expect(
      resolveUsageRange({ ...selection, liveEndTime: true }, shanghai),
    ).toEqual({ startDate: 1000, endDate: nowSec });
  });

  it("custom treats epoch 0 as a real start, not a missing value", () => {
    expect(
      resolveUsageRange({ preset: "custom", customStartDate: 0 }, shanghai)
        .startDate,
    ).toBe(0);
  });

  it("uses the system zone when only `now` is injected", () => {
    const now = new Date(2026, 4, 20, 15, 30).getTime(); // system-local
    expect(resolveUsageRange({ preset: "7d" }, { now }).startDate).toBe(
      Math.floor(new Date(2026, 4, 14).getTime() / 1000),
    );
  });

  it("defaults to the real clock when no context is given", () => {
    const before = Math.floor(Date.now() / 1000);
    const { endDate } = resolveUsageRange({ preset: "1d" });
    expect(endDate).toBeGreaterThanOrEqual(before);
    expect(endDate).toBeLessThanOrEqual(Math.floor(Date.now() / 1000));
  });

  describe("across DST transitions (America/New_York)", () => {
    const tz = "America/New_York";

    it("7d starts 6 calendar days back after spring-forward", () => {
      // 2026-03-08 is spring-forward; the window is one hour short of 7*24h.
      const range = resolveUsageRange(
        { preset: "7d" },
        { now: utc("2026-03-10T04:30:00Z"), tz }, // 03-10 00:30 EDT
      );
      expect(range.startDate).toBe(sec("2026-03-04T05:00:00Z")); // 03-04 00:00 EST
    });

    it("7d starts 6 calendar days back after fall-back", () => {
      // 2026-11-01 is fall-back; the window is one hour longer than 7*24h.
      const range = resolveUsageRange(
        { preset: "7d" },
        { now: utc("2026-11-04T04:30:00Z"), tz }, // 11-03 23:30 EST
      );
      expect(range.startDate).toBe(sec("2026-10-28T04:00:00Z")); // 10-28 00:00 EDT
    });

    it("today on the transition day starts at that day's midnight", () => {
      const springDay = resolveUsageRange(
        { preset: "today" },
        { now: utc("2026-03-08T18:00:00Z"), tz },
      );
      expect(springDay.startDate).toBe(sec("2026-03-08T05:00:00Z")); // EST
      const fallDay = resolveUsageRange(
        { preset: "today" },
        { now: utc("2026-11-01T18:00:00Z"), tz },
      );
      expect(fallDay.startDate).toBe(sec("2026-11-01T04:00:00Z")); // EDT
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
