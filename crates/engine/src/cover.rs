//! Spine widths for Resize Book Cover and New Book Cover.

/// Who prints the book, and how. `Lulu` is a paperback (a formula); `LuluHardback` a casewrap
/// hardback (Lulu's table). `Other` has no rule, so its spine is always typed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Printer {
    Lulu,
    LuluHardback,
    Other,
}

impl Printer {
    pub fn parse(s: &str) -> Option<Printer> {
        match s.trim().to_ascii_lowercase().as_str() {
            "lulu" => Some(Printer::Lulu),
            "hardback" | "lulu-hardback" => Some(Printer::LuluHardback),
            "other" => Some(Printer::Other),
            _ => None,
        }
    }
}

/// The widest spine that is accepted, in points.
pub const MAX_SPINE_PT: f64 = 1000.0;
/// Lulu's published paperback rule: `pages / 444 + 0.06` inches. Source: Lulu's help article "How is
/// spine width calculated?" (https://help.api.lulu.com/en/support/solutions/articles/64000254616-how-is-spine-width-calculated-).
const LULU_PAGES_PER_INCH: f64 = 444.0;
const LULU_BASE_INCHES: f64 = 0.06;

/// Lulu's paperback spine for `pages` pages, in points (inches rounded to four decimals first).
pub fn lulu_spine_pt(pages: u32) -> Result<f64, String> {
    if pages == 0 {
        return Err("The page count must be at least 1.".into());
    }
    let inches = f64::from(pages) / LULU_PAGES_PER_INCH + LULU_BASE_INCHES;
    Ok((inches * 10_000.0).round() / 10_000.0 * 72.0)
}

/// Lulu's casewrap hardback spine bands, from the table in the same help article: the last page count of a band and its spine in inches.
/// Lulu's table prints some counts in two bands (84, 140, 800); the first band that holds the count
/// is taken, so 84 pages is 0.25 in and 800 pages is 2.0625 in.
const LULU_HARDBACK: [(u32, f64); 26] = [
    (84, 0.25),
    (140, 0.5),
    (168, 0.625),
    (194, 0.6875),
    (222, 0.75),
    (250, 0.8125),
    (278, 0.875),
    (306, 0.9375),
    (334, 1.0),
    (360, 1.0625),
    (388, 1.125),
    (416, 1.1875),
    (444, 1.25),
    (472, 1.3125),
    (500, 1.375),
    (528, 1.4375),
    (556, 1.5),
    (582, 1.5625),
    (610, 1.625),
    (638, 1.6875),
    (666, 1.75),
    (694, 1.8125),
    (722, 1.875),
    (750, 1.9375),
    (778, 2.0),
    (800, 2.0625),
];

/// Lulu's casewrap hardback spine for `pages` pages, in points. Hardbacks take 24 to 800 pages.
pub fn lulu_hardback_spine_pt(pages: u32) -> Result<f64, String> {
    if pages < 24 {
        return Err("A Lulu hardback needs at least 24 pages.".into());
    }
    LULU_HARDBACK
        .iter()
        .find(|(last, _)| pages <= *last)
        .map(|(_, inches)| inches * 72.0)
        .ok_or_else(|| "A Lulu hardback takes up to 800 pages.".to_string())
}

/// The spine to use: a typed value wins (it must be 0 to 1000 pt); otherwise Lulu's rule from
/// `pages`. `Other` has no rule.
pub fn spine_pt(printer: Printer, pages: Option<u32>, typed: Option<f64>) -> Result<f64, String> {
    if let Some(v) = typed {
        return if v.is_finite() && (0.0..=MAX_SPINE_PT).contains(&v) {
            Ok(v)
        } else {
            Err(format!("The spine must be between 0 and {MAX_SPINE_PT} pt."))
        };
    }
    match (printer, pages) {
        (Printer::Lulu, Some(p)) => {
            let v = lulu_spine_pt(p)?;
            if v > MAX_SPINE_PT { Err(format!("{p} pages gives a spine over {MAX_SPINE_PT} pt. Check the page count.")) } else { Ok(v) }
        }
        (Printer::Lulu | Printer::LuluHardback, None) => Err("Give the page count, or type the spine.".into()),
        (Printer::LuluHardback, Some(p)) => lulu_hardback_spine_pt(p),
        (Printer::Other, _) => Err("Type the spine for this printer.".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lulu_spine_follows_the_published_rule() {
        // 224 / 444 + 0.06 = 0.5645 in; 230 / 444 + 0.06 = 0.5780 in; 1 page = 0.0623 in.
        assert!((lulu_spine_pt(224).unwrap() - 40.644).abs() < 1e-9);
        assert!((lulu_spine_pt(230).unwrap() - 41.616).abs() < 1e-9);
        assert!((lulu_spine_pt(1).unwrap() - 4.4856).abs() < 1e-9);
        assert!(lulu_spine_pt(0).is_err());
    }

    #[test]
    fn a_typed_spine_wins_and_is_checked() {
        assert_eq!(spine_pt(Printer::Lulu, Some(224), Some(40.5)), Ok(40.5));
        assert_eq!(spine_pt(Printer::Other, None, Some(30.0)), Ok(30.0));
        for bad in [-1.0, 1000.5, f64::NAN, f64::INFINITY] {
            assert!(spine_pt(Printer::Lulu, Some(224), Some(bad)).is_err(), "{bad}");
        }
    }

    #[test]
    fn without_a_typed_spine_lulu_needs_pages_and_other_is_refused() {
        assert!((spine_pt(Printer::Lulu, Some(224), None).unwrap() - 40.644).abs() < 1e-9);
        assert!(spine_pt(Printer::Lulu, None, None).is_err());
        assert!(spine_pt(Printer::Other, Some(224), None).is_err());
    }

    #[test]
    fn a_page_count_that_gives_a_spine_over_the_limit_is_refused_in_plain_words() {
        let e = spine_pt(Printer::Lulu, Some(23_000), None).unwrap_err();
        assert!(e.contains("1000"), "{e}");
        assert!(spine_pt(Printer::Lulu, Some(5_000), None).is_ok(), "5000 pages is 815 pt, under the limit");
    }

    #[test]
    fn printer_names_are_read_in_any_case() {
        assert_eq!(Printer::parse("Lulu"), Some(Printer::Lulu));
        assert_eq!(Printer::parse("OTHER"), Some(Printer::Other));
        assert_eq!(Printer::parse("blurb"), None, "Blurb has no rule to build in: its spine is typed under Other");
        assert_eq!(Printer::parse("Hardback"), Some(Printer::LuluHardback));
        assert_eq!(Printer::parse("kdp"), None);
    }

    #[test]
    fn lulu_hardback_follows_the_published_table() {
        assert_eq!(lulu_hardback_spine_pt(24), Ok(18.0));
        assert_eq!(lulu_hardback_spine_pt(84), Ok(18.0), "84 is printed in two bands: the first is taken");
        assert_eq!(lulu_hardback_spine_pt(85), Ok(36.0));
        assert_eq!(lulu_hardback_spine_pt(224), Ok(58.5));
        assert_eq!(lulu_hardback_spine_pt(800), Ok(148.5));
        assert!(lulu_hardback_spine_pt(23).unwrap_err().contains("24"));
        assert!(lulu_hardback_spine_pt(801).unwrap_err().contains("800"));
        assert_eq!(spine_pt(Printer::LuluHardback, Some(224), None), Ok(58.5));
        assert_eq!(spine_pt(Printer::LuluHardback, Some(224), Some(60.0)), Ok(60.0), "a typed spine wins");
        assert!(spine_pt(Printer::LuluHardback, None, None).is_err());
    }
}
