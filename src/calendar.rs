//! Trading-calendar helpers + 15:30 America/Toronto wake scheduling.
//!
//! Holidays are handled with a small explicit list (US market closures that
//! matter for the 15:30 pre-close window). Weekends are always excluded. For a
//! fully authoritative calendar, wire in a holiday feed; the explicit list keeps
//! the daemon self-contained.

use chrono::{Datelike, Duration, NaiveDate, Weekday};
use chrono_tz::America::Toronto;

/// The concrete timezone type backing [`Toronto`].
pub type Tz = chrono_tz::Tz;

/// The third Friday of a given (year, month).
pub fn third_friday(year: i32, month: u32) -> NaiveDate {
    let first = NaiveDate::from_ymd_opt(year, month, 1).expect("valid first of month");
    let days_to_friday = (Weekday::Fri.num_days_from_monday() as i64
        - first.weekday().num_days_from_monday() as i64
        + 7)
        % 7;
    let first_friday = first + Duration::days(days_to_friday);
    first_friday + Duration::days(14) // +2 weeks -> third Friday
}

/// The last trading day of August for a year (Aug 31, or the prior Friday if
/// it falls on a weekend).
pub fn last_trading_day_of_august(year: i32) -> NaiveDate {
    let mut d = NaiveDate::from_ymd_opt(year, 8, 31).expect("valid Aug 31");
    while is_weekend(d) {
        d -= Duration::days(1);
    }
    d
}

pub fn is_weekend(d: NaiveDate) -> bool {
    d.weekday() == Weekday::Sat || d.weekday() == Weekday::Sun
}

/// A minimal, explicit US-market holiday list for a year. Best-effort: covers
/// the common NYSE closures. Extend as needed.
pub fn us_holidays(year: i32) -> Vec<NaiveDate> {
    let mut h = Vec::new();
    // New Year's Day (observed on the nearest weekday)
    push_observed(&mut h, NaiveDate::from_ymd_opt(year, 1, 1).unwrap());
    // MLK Day: 3rd Monday of January
    h.push(nth_weekday(year, 1, Weekday::Mon, 3));
    // Presidents' Day: 3rd Monday of February
    h.push(nth_weekday(year, 2, Weekday::Mon, 3));
    // Memorial Day: last Monday of May
    h.push(last_weekday(year, 5, Weekday::Mon));
    // Juneteenth (observed since 2021)
    if year >= 2021 {
        push_observed(&mut h, NaiveDate::from_ymd_opt(year, 6, 19).unwrap());
    }
    // Independence Day
    push_observed(&mut h, NaiveDate::from_ymd_opt(year, 7, 4).unwrap());
    // Labor Day: 1st Monday of September
    h.push(nth_weekday(year, 9, Weekday::Mon, 1));
    // Thanksgiving: 4th Thursday of November
    h.push(nth_weekday(year, 11, Weekday::Thu, 4));
    // Christmas
    push_observed(&mut h, NaiveDate::from_ymd_opt(year, 12, 25).unwrap());
    h
}

fn push_observed(out: &mut Vec<NaiveDate>, d: NaiveDate) {
    let mut d = d;
    while is_weekend(d) {
        d += Duration::days(1);
    }
    out.push(d);
}

fn nth_weekday(year: i32, month: u32, wd: Weekday, n: u32) -> NaiveDate {
    let first = NaiveDate::from_ymd_opt(year, month, 1).unwrap();
    let offset =
        (wd.num_days_from_monday() as i64 - first.weekday().num_days_from_monday() as i64 + 7) % 7;
    first + Duration::days(offset + ((n - 1) as i64) * 7)
}

fn last_weekday(year: i32, month: u32, wd: Weekday) -> NaiveDate {
    // last day of month
    let next_month_first = if month == 12 {
        NaiveDate::from_ymd_opt(year + 1, 1, 1).unwrap()
    } else {
        NaiveDate::from_ymd_opt(year, month + 1, 1).unwrap()
    };
    let last = next_month_first - Duration::days(1);
    let offset =
        (last.weekday().num_days_from_monday() as i64 - wd.num_days_from_monday() as i64 + 7) % 7;
    last - Duration::days(offset)
}

/// True if `d` is a trading day (not a weekend, not a holiday).
pub fn is_trading_day(d: NaiveDate) -> bool {
    if is_weekend(d) {
        return false;
    }
    !us_holidays(d.year()).contains(&d)
}

/// Parse "HH:MM" into (hour, minute).
pub fn parse_eval_time(s: &str) -> Option<(u32, u32)> {
    let (h, m) = s.split_once(':')?;
    let h: u32 = h.trim().parse().ok()?;
    let m: u32 = m.trim().parse().ok()?;
    if h < 24 && m < 60 { Some((h, m)) } else { None }
}

/// The next 15:30-style eval instant at or after `now` (America/Toronto).
pub fn next_eval(
    now: chrono::DateTime<Tz>,
    hour: u32,
    minute: u32,
) -> Option<chrono::DateTime<Tz>> {
    let today = now
        .date_naive()
        .and_hms_opt(hour, minute, 0)?
        .and_local_timezone(Toronto)
        .single()?;
    if today > now {
        Some(today)
    } else {
        Some(today + Duration::days(1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Timelike};

    #[test]
    fn third_friday_jan_2026() {
        // Jan 1 2026 = Thursday; first Friday Jan 2, third Friday Jan 16.
        assert_eq!(
            third_friday(2026, 1),
            NaiveDate::from_ymd_opt(2026, 1, 16).unwrap()
        );
    }

    #[test]
    fn third_friday_jul_2026() {
        // Jul 1 2026 = Wednesday; first Friday Jul 3, third Friday Jul 17.
        assert_eq!(
            third_friday(2026, 7),
            NaiveDate::from_ymd_opt(2026, 7, 17).unwrap()
        );
    }

    #[test]
    fn last_trading_day_of_august_weekday() {
        // Aug 31 2026 is a Monday.
        assert_eq!(
            last_trading_day_of_august(2026),
            NaiveDate::from_ymd_opt(2026, 8, 31).unwrap()
        );
    }

    #[test]
    fn last_trading_day_of_august_weekend() {
        // Aug 31 2025 is a Sunday -> last trading day Aug 29 (Fri).
        assert_eq!(
            last_trading_day_of_august(2025),
            NaiveDate::from_ymd_opt(2025, 8, 29).unwrap()
        );
    }

    #[test]
    fn is_trading_day_excludes_weekends() {
        let sat = NaiveDate::from_ymd_opt(2026, 7, 11).unwrap(); // Saturday
        assert!(!is_trading_day(sat));
        let mon = NaiveDate::from_ymd_opt(2026, 7, 13).unwrap(); // Monday
        assert!(is_trading_day(mon));
    }

    #[test]
    fn parse_eval_time_ok() {
        assert_eq!(parse_eval_time("15:30"), Some((15, 30)));
        assert_eq!(parse_eval_time("9:05"), Some((9, 5)));
        assert_eq!(parse_eval_time("25:00"), None);
    }

    #[test]
    fn next_eval_advances_a_day_when_past() {
        // 2026-07-09 16:00 Toronto -> next 15:30 is tomorrow.
        let now = Toronto.with_ymd_and_hms(2026, 7, 9, 16, 0, 0).unwrap();
        let nxt = next_eval(now, 15, 30).unwrap();
        assert_eq!(nxt.day(), 10);
        assert_eq!(nxt.hour(), 15);
    }
}
