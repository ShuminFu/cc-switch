/**
 * Calendar helpers for the usage date-range picker (FE-UI-27).
 */

/** Longest custom range the dashboard accepts, in days. */
export const MAX_CUSTOM_RANGE_DAYS = 366;

/** Locales whose week starts on Monday when `Intl.Locale` has no week info. */
const MONDAY_FIRST_FALLBACK =
  /^(zh-cn|zh-hans|zh-sg|de|fr|es|it|ru|nl|pl|sv|nb|da|fi|en-gb|ko-kp)\b/i;

interface WeekInfoLocale {
  getWeekInfo?: () => { firstDay: number };
  weekInfo?: { firstDay: number };
}

/**
 * First day of the week for `locale` as a JS weekday (0 = Sunday,
 * 1 = Monday). Uses CLDR data from `Intl.Locale` week info when the runtime
 * provides it (CLDR `firstDay` is 1..7 with 7 = Sunday).
 */
export function getFirstDayOfWeek(locale: string): number {
  try {
    const intlLocale = new Intl.Locale(locale) as unknown as WeekInfoLocale;
    const info = intlLocale.getWeekInfo?.() ?? intlLocale.weekInfo;
    if (info && Number.isInteger(info.firstDay)) {
      return info.firstDay % 7;
    }
  } catch {
    // Unknown locale tag: fall through to the table.
  }
  return MONDAY_FIRST_FALLBACK.test(locale) ? 1 : 0;
}

/** 6-week grid for `month`, starting on `firstDayOfWeek`. */
export function getCalendarDays(month: Date, firstDayOfWeek = 0): Date[] {
  const first = new Date(month.getFullYear(), month.getMonth(), 1);
  const offset = (first.getDay() - firstDayOfWeek + 7) % 7;
  const gridStart = new Date(first);
  gridStart.setDate(first.getDate() - offset);
  return Array.from({ length: 42 }, (_, i) => {
    const d = new Date(gridStart);
    d.setDate(gridStart.getDate() + i);
    return d;
  });
}

/** Narrow weekday labels in display order for `locale`. */
export function getWeekdayLabels(
  locale: string,
  firstDayOfWeek: number,
): string[] {
  const format = new Intl.DateTimeFormat(locale, { weekday: "narrow" });
  // 2024-01-07 is a Sunday.
  return Array.from({ length: 7 }, (_, i) =>
    format.format(new Date(2024, 0, 7 + ((firstDayOfWeek + i) % 7))),
  );
}

/** Whether a custom range (unix seconds) exceeds the accepted length. */
export function exceedsMaxRange(startTs: number, endTs: number): boolean {
  return endTs - startTs > MAX_CUSTOM_RANGE_DAYS * 24 * 60 * 60;
}
