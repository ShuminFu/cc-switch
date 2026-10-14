//! Pure calendar/time-window helpers for usage statistics.
//!
//! Everything here takes the current instant (and therefore its time zone) as
//! a parameter instead of calling `Local::now()` / SQLite's `'now'` inline, so
//! "today", "this month" and rollup cutoffs can be tested deterministically
//! across DST transitions and month boundaries. Production callers pass
//! `Local::now()`.

use chrono::{DateTime, Datelike, Duration, LocalResult, NaiveDate, TimeZone};

use crate::error::AppError;

/// First instant of `day` in `tz`.
///
/// Ambiguous midnights (fall-back at 00:00) resolve to the earlier instant; a
/// midnight that falls inside a DST gap resolves to one hour later, which is
/// the first wall-clock time that exists on that day.
pub(crate) fn local_day_start<Tz: TimeZone>(tz: &Tz, day: NaiveDate) -> Option<DateTime<Tz>> {
    let midnight = day.and_hms_opt(0, 0, 0)?;
    match tz.from_local_datetime(&midnight) {
        LocalResult::Single(dt) => Some(dt),
        LocalResult::Ambiguous(earliest, _) => Some(earliest),
        LocalResult::None => match tz.from_local_datetime(&(midnight + Duration::hours(1))) {
            LocalResult::Single(dt) => Some(dt),
            LocalResult::Ambiguous(earliest, _) => Some(earliest),
            LocalResult::None => None,
        },
    }
}

fn day_start_ts<Tz: TimeZone>(tz: &Tz, day: NaiveDate, what: &str) -> Result<i64, AppError> {
    local_day_start(tz, day)
        .map(|dt| dt.timestamp())
        .ok_or_else(|| AppError::Database(format!("{what}: no local midnight for {day}")))
}

/// Rollup/prune cutoff: the local midnight that starts the day after
/// `now - retain_days`. Rows strictly older than this are complete local days
/// and can be rolled up.
pub(crate) fn rollup_cutoff_ts<Tz: TimeZone>(
    now: &DateTime<Tz>,
    retain_days: i64,
) -> Result<i64, AppError> {
    let target_day = now
        .clone()
        .checked_sub_signed(Duration::days(retain_days))
        .ok_or_else(|| AppError::Database("rollup cutoff overflow".to_string()))?
        .date_naive();
    let next_day = target_day
        .succ_opt()
        .ok_or_else(|| AppError::Database("rollup cutoff next-day overflow".to_string()))?;
    day_start_ts(&now.timezone(), next_day, "rollup cutoff")
}

/// Half-open `[start, end)` windows for the provider spend limits, in both
/// epoch seconds (for `proxy_request_logs.created_at`) and `YYYY-MM-DD` dates
/// (for `usage_daily_rollups.date`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LimitWindows {
    pub day_start: i64,
    pub day_end: i64,
    pub day: String,
    pub month_start: i64,
    pub month_end: i64,
    pub month_first_day: String,
    pub next_month_first_day: String,
}

/// "Today" and "this month" in `now`'s time zone.
pub(crate) fn provider_limit_windows<Tz: TimeZone>(
    now: &DateTime<Tz>,
) -> Result<LimitWindows, AppError> {
    let tz = now.timezone();
    let today = now.date_naive();
    let tomorrow = today
        .succ_opt()
        .ok_or_else(|| AppError::Database("limit window day overflow".to_string()))?;
    let month_first = today
        .with_day(1)
        .ok_or_else(|| AppError::Database("limit window month start".to_string()))?;
    let next_month_first = if today.month() == 12 {
        NaiveDate::from_ymd_opt(today.year() + 1, 1, 1)
    } else {
        NaiveDate::from_ymd_opt(today.year(), today.month() + 1, 1)
    }
    .ok_or_else(|| AppError::Database("limit window month overflow".to_string()))?;

    let fmt = |d: NaiveDate| d.format("%Y-%m-%d").to_string();
    Ok(LimitWindows {
        day_start: day_start_ts(&tz, today, "limit window")?,
        day_end: day_start_ts(&tz, tomorrow, "limit window")?,
        day: fmt(today),
        month_start: day_start_ts(&tz, month_first, "limit window")?,
        month_end: day_start_ts(&tz, next_month_first, "limit window")?,
        month_first_day: fmt(month_first),
        next_month_first_day: fmt(next_month_first),
    })
}

/// Deterministic time zones for tests (no tz database dependency).
#[cfg(test)]
pub(crate) mod test_tz {
    use chrono::{FixedOffset, LocalResult, NaiveDate, NaiveDateTime, Offset, TimeZone};

    /// US Eastern rules for 2026 only: EDT (UTC-4) from 2026-03-08 02:00 EST
    /// (07:00Z) until 2026-11-01 02:00 EDT (06:00Z); EST (UTC-5) otherwise.
    #[derive(Debug, Clone, Copy)]
    pub struct Eastern2026;

    #[derive(Debug, Clone, Copy)]
    pub struct EasternOffset(FixedOffset);

    impl Offset for EasternOffset {
        fn fix(&self) -> FixedOffset {
            self.0
        }
    }

    fn est() -> EasternOffset {
        EasternOffset(FixedOffset::west_opt(5 * 3600).unwrap())
    }
    fn edt() -> EasternOffset {
        EasternOffset(FixedOffset::west_opt(4 * 3600).unwrap())
    }
    fn at(y: i32, m: u32, d: u32, h: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(y, m, d)
            .unwrap()
            .and_hms_opt(h, 0, 0)
            .unwrap()
    }

    impl TimeZone for Eastern2026 {
        type Offset = EasternOffset;

        fn from_offset(_: &EasternOffset) -> Self {
            Eastern2026
        }

        fn offset_from_local_date(&self, local: &NaiveDate) -> LocalResult<EasternOffset> {
            self.offset_from_local_datetime(&local.and_hms_opt(12, 0, 0).unwrap())
        }

        fn offset_from_local_datetime(&self, local: &NaiveDateTime) -> LocalResult<EasternOffset> {
            let (gap_start, gap_end) = (at(2026, 3, 8, 2), at(2026, 3, 8, 3));
            let (overlap_start, overlap_end) = (at(2026, 11, 1, 1), at(2026, 11, 1, 2));
            if *local >= gap_start && *local < gap_end {
                LocalResult::None
            } else if *local >= overlap_start && *local < overlap_end {
                LocalResult::Ambiguous(edt(), est())
            } else if *local >= gap_end && *local < overlap_start {
                LocalResult::Single(edt())
            } else {
                LocalResult::Single(est())
            }
        }

        fn offset_from_utc_date(&self, utc: &NaiveDate) -> EasternOffset {
            self.offset_from_utc_datetime(&utc.and_hms_opt(0, 0, 0).unwrap())
        }

        fn offset_from_utc_datetime(&self, utc: &NaiveDateTime) -> EasternOffset {
            if *utc >= at(2026, 3, 8, 7) && *utc < at(2026, 11, 1, 6) {
                edt()
            } else {
                est()
            }
        }
    }

    /// Asia/Shanghai: fixed UTC+8, no DST.
    pub fn shanghai() -> FixedOffset {
        FixedOffset::east_opt(8 * 3600).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::test_tz::{shanghai, Eastern2026};
    use super::*;
    use chrono::Offset;

    fn utc(s: &str) -> i64 {
        DateTime::parse_from_rfc3339(s).unwrap().timestamp()
    }

    fn eastern(s: &str) -> DateTime<Eastern2026> {
        Eastern2026.timestamp_opt(utc(s), 0).single().unwrap()
    }

    #[test]
    fn test_tz_models_2026_us_eastern_transitions() {
        // Sanity-check the fixture itself.
        assert_eq!(
            eastern("2026-03-08T06:59:59Z")
                .offset()
                .fix()
                .local_minus_utc(),
            -5 * 3600
        );
        assert_eq!(
            eastern("2026-03-08T07:00:00Z")
                .offset()
                .fix()
                .local_minus_utc(),
            -4 * 3600
        );
        assert_eq!(
            eastern("2026-11-01T05:59:59Z")
                .offset()
                .fix()
                .local_minus_utc(),
            -4 * 3600
        );
        assert_eq!(
            eastern("2026-11-01T06:00:00Z")
                .offset()
                .fix()
                .local_minus_utc(),
            -5 * 3600
        );
    }

    #[test]
    fn local_day_start_on_transition_days() {
        let day = |d: u32, m: u32| NaiveDate::from_ymd_opt(2026, m, d).unwrap();
        // Spring-forward day: midnight is still EST
        assert_eq!(
            local_day_start(&Eastern2026, day(8, 3))
                .unwrap()
                .timestamp(),
            utc("2026-03-08T05:00:00Z")
        );
        // Day after: EDT midnight
        assert_eq!(
            local_day_start(&Eastern2026, day(9, 3))
                .unwrap()
                .timestamp(),
            utc("2026-03-09T04:00:00Z")
        );
        // Fall-back day: midnight is still EDT
        assert_eq!(
            local_day_start(&Eastern2026, day(1, 11))
                .unwrap()
                .timestamp(),
            utc("2026-11-01T04:00:00Z")
        );
    }

    #[test]
    fn limit_windows_today_and_month_in_fixed_zone() {
        // 2026-05-31 23:30 in Shanghai is 15:30Z, still May there
        let now = shanghai()
            .timestamp_opt(utc("2026-05-31T15:30:00Z"), 0)
            .unwrap();
        let w = provider_limit_windows(&now).unwrap();
        assert_eq!(w.day, "2026-05-31");
        assert_eq!(w.day_start, utc("2026-05-30T16:00:00Z"));
        assert_eq!(w.day_end, utc("2026-05-31T16:00:00Z"));
        assert_eq!(w.month_first_day, "2026-05-01");
        assert_eq!(w.next_month_first_day, "2026-06-01");
        assert_eq!(w.month_start, utc("2026-04-30T16:00:00Z"));
        assert_eq!(w.month_end, utc("2026-05-31T16:00:00Z"));
    }

    #[test]
    fn limit_windows_roll_over_year_end() {
        let now = shanghai()
            .timestamp_opt(utc("2026-12-31T20:00:00Z"), 0) // 2027-01-01 04:00 local
            .unwrap();
        let w = provider_limit_windows(&now).unwrap();
        assert_eq!(w.day, "2027-01-01");
        assert_eq!(w.month_first_day, "2027-01-01");
        assert_eq!(w.next_month_first_day, "2027-02-01");

        let dec = shanghai()
            .timestamp_opt(utc("2026-12-15T00:00:00Z"), 0)
            .unwrap();
        let w = provider_limit_windows(&dec).unwrap();
        assert_eq!(w.next_month_first_day, "2027-01-01");
        assert_eq!(w.month_end, utc("2026-12-31T16:00:00Z"));
    }

    #[test]
    fn limit_day_window_is_23h_on_spring_forward_and_25h_on_fall_back() {
        let spring = provider_limit_windows(&eastern("2026-03-08T15:00:00Z")).unwrap();
        assert_eq!(spring.day, "2026-03-08");
        assert_eq!(spring.day_end - spring.day_start, 23 * 3600);

        let fall = provider_limit_windows(&eastern("2026-11-01T15:00:00Z")).unwrap();
        assert_eq!(fall.day, "2026-11-01");
        assert_eq!(fall.day_end - fall.day_start, 25 * 3600);
    }

    #[test]
    fn limit_month_window_spans_dst_change() {
        // March 2026 contains spring-forward: one hour shorter than 31 days
        let w = provider_limit_windows(&eastern("2026-03-20T12:00:00Z")).unwrap();
        assert_eq!(w.month_start, utc("2026-03-01T05:00:00Z"));
        assert_eq!(w.month_end, utc("2026-04-01T04:00:00Z"));
        assert_eq!(w.month_end - w.month_start, 31 * 86400 - 3600);
    }

    #[test]
    fn limit_windows_use_local_day_just_after_midnight() {
        // 23:30 EDT on 2026-06-30 is already 2026-07-01 in UTC; the local day
        // and month (June) must win.
        let w = provider_limit_windows(&eastern("2026-07-01T03:30:00Z")).unwrap();
        assert_eq!(w.day, "2026-06-30");
        assert_eq!(w.month_first_day, "2026-06-01");
        assert_eq!(w.next_month_first_day, "2026-07-01");
    }

    #[test]
    fn rollup_cutoff_aligns_to_next_local_midnight() {
        let now = shanghai()
            .timestamp_opt(utc("2026-04-16T06:32:17Z"), 0) // 14:32:17 local
            .unwrap();
        // target 2026-03-17 → cutoff 2026-03-18 00:00 +08:00
        assert_eq!(
            rollup_cutoff_ts(&now, 30).unwrap(),
            utc("2026-03-17T16:00:00Z")
        );
    }
}
