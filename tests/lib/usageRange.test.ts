import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { resolveUsageRange } from "@/lib/usageRange";

/** Local wall-clock components of a unix-seconds timestamp. */
function localParts(seconds: number) {
  const d = new Date(seconds * 1000);
  return [d.getFullYear(), d.getMonth() + 1, d.getDate(), d.getHours()];
}

// FE-ARCH-42: day presets must count calendar days, not 24h blocks.
describe("resolveUsageRange across DST", () => {
  const originalTz = process.env.TZ;

  afterEach(() => {
    process.env.TZ = originalTz;
  });

  describe("Europe/Berlin", () => {
    beforeEach(() => {
      process.env.TZ = "Europe/Berlin";
    });

    it("7d just after midnight after the spring-forward day starts 6 days back", () => {
      const now = Date.parse("2026-03-31T00:30:00+02:00");
      const { startDate, endDate } = resolveUsageRange({ preset: "7d" }, now);
      expect(localParts(startDate)).toEqual([2026, 3, 25, 0]);
      expect(endDate).toBe(Math.floor(now / 1000));
    });

    it("30d across the autumn fall-back starts at local midnight", () => {
      const now = Date.parse("2026-11-02T00:15:00+01:00");
      const { startDate } = resolveUsageRange({ preset: "30d" }, now);
      expect(localParts(startDate)).toEqual([2026, 10, 4, 0]);
    });
  });

  describe("America/New_York", () => {
    beforeEach(() => {
      process.env.TZ = "America/New_York";
    });

    it("14d across the March transition starts at local midnight", () => {
      const now = Date.parse("2026-03-10T00:20:00-04:00");
      const { startDate } = resolveUsageRange({ preset: "14d" }, now);
      expect(localParts(startDate)).toEqual([2026, 2, 25, 0]);
    });

    it("today starts at local midnight", () => {
      const now = Date.parse("2026-11-01T12:00:00-05:00");
      const { startDate } = resolveUsageRange({ preset: "today" }, now);
      expect(localParts(startDate)).toEqual([2026, 11, 1, 0]);
    });
  });
});
