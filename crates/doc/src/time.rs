//! Unix time → proleptic Gregorian calendar, exact for every `i64`.
//!
//! chrono is the only calendar logic. `NaiveDate` covers about ±262143 years, but clocks and files
//! can hold instants past that, so these extend its range: the Gregorian calendar repeats every
//! 400 years (146_097 days), so the instant is moved into chrono's range by whole cycles, chrono
//! gives the month, day and time of day, and the cycles are added back to the year (at most about
//! 2.5e16, so it fits an `i64`). Callers own only their output format and any year clamp.

use chrono::{Datelike, NaiveDate, NaiveDateTime, NaiveTime};

/// Days since 1970-01-01 → (proleptic Gregorian year, a chrono date with the same month and day;
/// its own year is not the real one).
pub fn date_parts(days: i64) -> (i64, NaiveDate) {
    let (cycles, rest) = (days.div_euclid(146_097), days.rem_euclid(146_097));
    // 1970-01-01 is CE day 719_163; one cycle on from it is well inside chrono's range.
    let d = NaiveDate::from_num_days_from_ce_opt(rest as i32 + 719_163).unwrap_or_default();
    (i64::from(d.year()) + cycles * 400, d)
}

/// Unix seconds (+ nanos, clamped to 999_999_999) → (proleptic Gregorian year, a chrono date and
/// time with the same month, day and time of day; its own year is not the real one).
pub fn utc_parts(secs: i64, nanos: u32) -> (i64, NaiveDateTime) {
    let (days, sod) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let (y, d) = date_parts(days);
    let t = NaiveTime::from_num_seconds_from_midnight_opt(sod as u32, nanos.min(999_999_999)).unwrap_or_default();
    (y, d.and_time(t))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Timelike;

    fn ymdhms(secs: i64) -> (i64, u32, u32, u32, u32, u32) {
        let (y, t) = utc_parts(secs, 0);
        (y, t.month(), t.day(), t.hour(), t.minute(), t.second())
    }

    #[test]
    fn epoch_and_ordinary_dates() {
        assert_eq!(ymdhms(0), (1970, 1, 1, 0, 0, 0));
        assert_eq!(ymdhms(951_782_400), (2000, 2, 29, 0, 0, 0));
        assert_eq!(ymdhms(1_791_504_000 + 3_723), (2026, 10, 9, 1, 2, 3));
    }

    #[test]
    fn before_1970() {
        assert_eq!(ymdhms(-1), (1969, 12, 31, 23, 59, 59));
        assert_eq!(ymdhms(-86_400), (1969, 12, 31, 0, 0, 0));
        assert_eq!(ymdhms(-10_000_000_000_000), (-314918, 8, 13, 6, 13, 20));
    }

    #[test]
    fn year_9999_rolls_into_10000() {
        assert_eq!(ymdhms(253_402_300_799), (9999, 12, 31, 23, 59, 59));
        assert_eq!(ymdhms(253_402_300_800), (10000, 1, 1, 0, 0, 0));
    }

    #[test]
    fn i64_extremes_stay_exact() {
        assert_eq!(ymdhms(i64::MAX), (292277026596, 12, 4, 15, 30, 7));
        assert_eq!(ymdhms(i64::MIN), (-292277022657, 1, 27, 8, 29, 52));
        // Days (not seconds) at the extremes: whole cycles still fit, so the year can't overflow.
        let ((hi, _), (lo, _)) = (date_parts(i64::MAX), date_parts(i64::MIN));
        assert!(hi > 25_000_000_000_000_000 && hi < i64::MAX && lo < -25_000_000_000_000_000 && lo > i64::MIN);
    }

    #[test]
    fn nanos_past_a_second_are_clamped() {
        assert_eq!(utc_parts(0, 999_999_999).1.nanosecond(), 999_999_999);
        assert_eq!(utc_parts(0, 1_500_000_000).1.nanosecond(), 999_999_999);
        assert_eq!(utc_parts(0, u32::MAX).1.second(), 0);
    }
}
