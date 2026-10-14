import type { UsageRangePreset, UsageRangeSelection } from "@/types/usage";

const DAY_SECONDS = 24 * 60 * 60;

export interface ResolvedUsageRange {
  startDate: number;
  endDate: number;
}

/**
 * Clock used to resolve a range. Both fields are optional so production code
 * can pass nothing (real clock, system time zone) while tests inject a fixed
 * instant and an IANA zone instead of mutating `process.env.TZ`.
 */
export interface UsageRangeClock {
  /** Current instant in epoch milliseconds (default: `Date.now()`). */
  now?: number;
  /** IANA time zone for calendar-day math (default: the system zone). */
  tz?: string;
}

interface CalendarDay {
  year: number;
  /** 0-based, like `Date#getMonth` */
  month: number;
  day: number;
}

const formatterCache = new Map<string, Intl.DateTimeFormat>();

function zonedFormatter(tz: string): Intl.DateTimeFormat {
  let formatter = formatterCache.get(tz);
  if (!formatter) {
    formatter = new Intl.DateTimeFormat("en-US", {
      timeZone: tz,
      hourCycle: "h23",
      year: "numeric",
      month: "numeric",
      day: "numeric",
      hour: "numeric",
      minute: "numeric",
      second: "numeric",
    });
    formatterCache.set(tz, formatter);
  }
  return formatter;
}

/** Wall-clock fields of `ms` in `tz`, expressed as a UTC timestamp. */
function zonedWallClockAsUtc(ms: number, tz: string): number {
  const parts: Record<string, number> = {};
  for (const part of zonedFormatter(tz).formatToParts(new Date(ms))) {
    if (part.type !== "literal") parts[part.type] = Number(part.value);
  }
  return Date.UTC(
    parts.year,
    parts.month - 1,
    parts.day,
    parts.hour,
    parts.minute,
    parts.second,
  );
}

function calendarDayOf(ms: number, tz: string | undefined): CalendarDay {
  if (tz) {
    const wall = new Date(zonedWallClockAsUtc(ms, tz));
    return {
      year: wall.getUTCFullYear(),
      month: wall.getUTCMonth(),
      day: wall.getUTCDate(),
    };
  }
  const local = new Date(ms);
  return {
    year: local.getFullYear(),
    month: local.getMonth(),
    day: local.getDate(),
  };
}

/**
 * Epoch milliseconds of local midnight starting `{year, month, day}` in `tz`.
 * Day overflow (e.g. day = -3) is normalized, so callers can step back by
 * calendar days without worrying about month or DST boundaries.
 */
function startOfCalendarDay(
  { year, month, day }: CalendarDay,
  tz: string | undefined,
): number {
  if (!tz) {
    return new Date(year, month, day).getTime();
  }
  const wallMidnight = Date.UTC(year, month, day);
  // Offset = wall clock - UTC. Resolve twice: the first guess may sit on the
  // other side of a DST transition from the real midnight.
  let ts = wallMidnight;
  for (let i = 0; i < 2; i++) {
    ts = wallMidnight - (zonedWallClockAsUtc(ts, tz) - ts);
  }
  return ts;
}

function presetDayCount(preset: "7d" | "14d" | "30d"): number {
  return preset === "7d" ? 7 : preset === "14d" ? 14 : 30;
}

export function resolveUsageRange(
  selection: UsageRangeSelection,
  clock: UsageRangeClock = {},
): ResolvedUsageRange {
  const nowMs = clock.now ?? Date.now();
  const { tz } = clock;
  const endDate = Math.floor(nowMs / 1000);

  switch (selection.preset) {
    case "today":
      return {
        startDate: Math.floor(
          startOfCalendarDay(calendarDayOf(nowMs, tz), tz) / 1000,
        ),
        endDate,
      };
    case "1d":
      return {
        startDate: endDate - DAY_SECONDS,
        endDate,
      };
    case "7d":
    case "14d":
    case "30d": {
      // Step back by calendar days rather than fixed 24h blocks so DST
      // transitions inside the window cannot shift the start by a day.
      const today = calendarDayOf(nowMs, tz);
      const start = startOfCalendarDay(
        { ...today, day: today.day - (presetDayCount(selection.preset) - 1) },
        tz,
      );
      return {
        startDate: Math.floor(start / 1000),
        endDate,
      };
    }
    case "custom": {
      const startDate = selection.customStartDate ?? endDate - DAY_SECONDS;
      const customEndDate = selection.liveEndTime
        ? endDate
        : (selection.customEndDate ?? endDate);
      return {
        startDate,
        endDate: customEndDate,
      };
    }
  }
}

export function getUsageRangePresetLabel(
  preset: UsageRangePreset,
  t: (key: string, options?: { defaultValue?: string }) => string,
): string {
  switch (preset) {
    case "today":
      return t("usage.presetToday", { defaultValue: "当天" });
    case "1d":
      return t("usage.preset1d", { defaultValue: "1d" });
    case "7d":
      return t("usage.preset7d", { defaultValue: "7d" });
    case "14d":
      return t("usage.preset14d", { defaultValue: "14d" });
    case "30d":
      return t("usage.preset30d", { defaultValue: "30d" });
    case "custom":
      return t("usage.customRange", { defaultValue: "日历筛选" });
  }
}
