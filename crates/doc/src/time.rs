//! Unix time → proleptic Gregorian calendar, exact for every `i64`.
//!
//! chrono is the only calendar logic. `NaiveDate` covers about ±262143 years, but clocks and files
//! can hold instants past that, so these extend its range: the Gregorian calendar repeats every
//! 400 years (146_097 days), so the instant is moved into chrono's range by whole cycles, chrono
//! gives the month, day and time of day, and only the year is carried (in i128, clamped to `i64`).
//! Callers own only their output format and any year clamp.

use chrono::Datelike;

/// Days since 1970-01-01 → (proleptic Gregorian year, the same month and day as a chrono date
/// moved into range by whole 400-year cycles; its own year is not the real one).
pub fn date_parts(days: i64) -> (i64, chrono::NaiveDate) {
    let (cycles, rest) = (days.div_euclid(146_097), days.rem_euclid(146_097));
    // `rest + 719_163` (1970-01-01 is CE day 719_163) is always a valid CE day.
    match i32::try_from(rest + 719_163).ok().and_then(chrono::NaiveDate::from_num_days_from_ce_opt) {
        Some(d) => {
            let y = i128::from(d.year()) + i128::from(cycles) * 400;
            (i64::try_from(y).unwrap_or(if y < 0 { i64::MIN } else { i64::MAX }), d)
        }
        None => (1970, chrono::NaiveDate::default()), // unreachable
    }
}

/// Unix seconds (+ nanos, clamped to 999_999_999) → (proleptic Gregorian year, the instant moved
/// into chrono's range by whole 400-year cycles). Month, day and time of day come from the
/// `NaiveDateTime`; its year is not the real one.
pub fn utc_parts(secs: i64, nanos: u32) -> (i64, chrono::NaiveDateTime) {
    let (days, sod) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let (y, d) = date_parts(days);
    // `sod` < 86_400, so the time is always valid.
    match u32::try_from(sod).ok().and_then(|s| chrono::NaiveTime::from_num_seconds_from_midnight_opt(s, nanos.min(999_999_999))) {
        Some(t) => (y, d.and_time(t)),
        None => (1970, chrono::NaiveDateTime::default()), // unreachable
    }
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
        // Days (not seconds) at the extremes: whole cycles still fit, so no year saturates.
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
