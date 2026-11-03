use super::{Document, Metadata, krilla};

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn now_unix() -> Option<i64> {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).ok().map(|d| d.as_secs() as i64)
}

#[cfg(target_arch = "wasm32")]
pub(super) fn now_unix() -> Option<i64> {
    None
}

/// The date an exported file records, when the caller gives none: `SOURCE_DATE_EPOCH` if it holds
/// a non-negative whole number of seconds, else the clock. Pinning it makes an export repeat its
/// bytes, which is otherwise impossible -- the only thing that differs between two exports of one
/// document is the timestamp, and the instance id krilla derives from the bytes follows it.
///
/// <https://reproducible-builds.org/specs/source-date-epoch/>
pub(super) fn export_time() -> Option<i64> {
    pinned_or(|name| std::env::var(name).ok(), now_unix)
}

/// [`export_time`] with the environment and the clock given, so a test can show which wins.
fn pinned_or(get: impl Fn(&str) -> Option<String>, clock: impl Fn() -> Option<i64>) -> Option<i64> {
    let pinned = get("SOURCE_DATE_EPOCH").and_then(|v| v.trim().parse::<i64>().ok()).filter(|secs| PDF_DATES.contains(secs));
    pinned.or_else(clock)
}

/// The dates a PDF can hold, as Unix seconds: `date_time` clamps the year to 0..=9999, so a value
/// outside this would be recorded as some other date rather than the one asked for. A value that
/// cannot be honoured is left to the clock, as a malformed one is.
const PDF_DATES: std::ops::RangeInclusive<i64> = 0..=253_402_300_799;

/// The document information from File Info (title, author, description, keywords). `title` and
/// `author` from the export options replace File Info's.
pub(super) fn file_info_metadata(doc: &Document, title: Option<&str>, author: Option<&str>) -> Metadata {
    let info = &doc.file_info;
    let description = info.description.trim();
    Metadata {
        creator: "DesignCraft".into(),
        producer: "DesignCraft".into(),
        title: Some(title.unwrap_or_else(|| doc.metadata_title()).to_string()),
        authors: author.or_else(|| doc.metadata_author()).map(str::to_string).into_iter().collect(),
        description: (!description.is_empty()).then(|| description.to_string()),
        keywords: info.keywords.clone(),
        created: None,
        modified: None,
    }
}

pub(super) fn date_time(t: i64) -> krilla::metadata::DateTime {
    let c = civil(t);
    krilla::metadata::DateTime::new(c.0.clamp(0, 9999) as u16)
        .month(c.1)
        .day(c.2)
        .hour(c.3)
        .minute(c.4)
        .second(c.5)
        .utc_offset_hour(0)
        .utc_offset_minute(0)
}

/// Unix seconds (UTC) → (year, month, day, hour, minute, second), proleptic Gregorian. A time
/// outside chrono's year range still converts (the calendar repeats every 400 years, so the
/// instant is moved into range by whole cycles and only the year is carried, matching the old
/// `civil`), rather than resetting to the epoch. `date_time` applies the PDF `0..=9999` year clamp.
pub(crate) fn civil(t: i64) -> (i64, u8, u8, u8, u8, u8) {
    use chrono::{Datelike, Timelike};
    let (y, t) = designcraft_doc::time::utc_parts(t, 0);
    (y, t.month() as u8, t.day() as u8, t.hour() as u8, t.minute() as u8, t.second() as u8)
}

#[cfg(test)]
mod export_time_tests {
    use super::pinned_or;

    /// What an export records when the caller gives no date: `SOURCE_DATE_EPOCH` when it holds a
    /// non-negative whole number of seconds, the clock whenever it does not. The environment and
    /// the clock are given here rather than set, so no test changes this process's environment --
    /// that is `unsafe` under edition 2024 and would reach every test beside it.
    #[test]
    fn a_pinned_epoch_is_preferred_and_the_clock_answers_otherwise() {
        let env = |v: Option<&'static str>| move |_: &str| v.map(str::to_owned);
        let clock = || Some(1_000_000);
        assert_eq!(pinned_or(env(Some("1791208980")), clock), Some(1_791_208_980), "the variable wins");
        assert_eq!(pinned_or(env(Some("0")), clock), Some(0), "the epoch itself counts");
        assert_eq!(pinned_or(env(Some("  1791208980  ")), clock), Some(1_791_208_980), "spaces around it are allowed");
        // A date a PDF cannot hold is left to the clock too: `date_time` would clamp the year and
        // record some other date. 253402300799 is 9999-12-31T23:59:59Z, the last one it can.
        assert_eq!(pinned_or(env(Some("253402300799")), clock), Some(253_402_300_799), "the last date a PDF can hold");
        for ignored in [
            None,
            Some(""),
            Some("   "),
            Some("-1"),
            Some("later"),
            Some("1791208980.5"),
            Some("99999999999999999999"),
            Some("253402300800"),
            Some("9223372036854775807"),
        ] {
            assert_eq!(pinned_or(env(ignored), clock), Some(1_000_000), "{ignored:?} leaves it to the clock");
        }
        // A platform with no clock and no variable records no date at all.
        assert_eq!(pinned_or(env(None), || None), None);
        assert_eq!(pinned_or(env(Some("1791208980")), || None), Some(1_791_208_980), "the variable needs no clock");
        // Only that variable is read.
        assert_eq!(pinned_or(|name: &str| (name == "TZ").then(|| "1791208980".to_owned()), clock), Some(1_000_000));
    }
}
