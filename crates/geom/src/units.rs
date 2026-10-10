//! Measurement units and InDesign-style measurement fields.
//!
//! Every length in the document model is in **points** (1/72 in). Fields in the UI accept any unit
//! and simple arithmetic, the way InDesign's do: `12`, `1p6` (1 pica 6 points), `p6`, `12mm`,
//! `3in`, `2c` (2 ciceros), `1c4` (1 cicero 4 didot points), `10px`, `18Q`, `4ag`, `24pt+2mm`,
//! `100/3`, `50%` (relative to a base value). A bare number takes the field's default unit.

use serde::{Deserialize, Serialize};

/// Points per unit.
pub const PT_PER_INCH: f64 = 72.0;
pub const PT_PER_PICA: f64 = 12.0;
pub const PT_PER_MM: f64 = 72.0 / 25.4;
pub const PT_PER_CM: f64 = 72.0 / 2.54;
/// One didot point is 0.376065 mm; a cicero is 12 didot points.
pub const PT_PER_DIDOT: f64 = 0.376_065 * PT_PER_MM;
pub const PT_PER_CICERO: f64 = 12.0 * PT_PER_DIDOT;
/// Agate (newspaper column measure): 1/14 in.
pub const PT_PER_AGATE: f64 = 72.0 / 14.0;
/// Q (Japanese type size) and Ha (Japanese leading): 0.25 mm.
pub const PT_PER_Q: f64 = 0.25 * PT_PER_MM;

/// A measurement unit (Preferences → Units & Increments).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Unit {
    #[default]
    Points,
    Picas,
    Inches,
    InchesDecimal,
    Millimeters,
    Centimeters,
    Ciceros,
    Agates,
    Pixels,
    Q,
    Ha,
}

impl Unit {
    pub const ALL: [Unit; 11] = [
        Unit::Points,
        Unit::Picas,
        Unit::Inches,
        Unit::InchesDecimal,
        Unit::Millimeters,
        Unit::Centimeters,
        Unit::Ciceros,
        Unit::Agates,
        Unit::Pixels,
        Unit::Q,
        Unit::Ha,
    ];

    /// Points in one of this unit.
    pub fn points(self) -> f64 {
        match self {
            Unit::Points | Unit::Pixels => 1.0,
            Unit::Picas => PT_PER_PICA,
            Unit::Inches | Unit::InchesDecimal => PT_PER_INCH,
            Unit::Millimeters => PT_PER_MM,
            Unit::Centimeters => PT_PER_CM,
            Unit::Ciceros => PT_PER_CICERO,
            Unit::Agates => PT_PER_AGATE,
            Unit::Q | Unit::Ha => PT_PER_Q,
        }
    }

    pub fn to_pt(self, v: f64) -> f64 {
        v * self.points()
    }

    pub fn from_pt(self, pt: f64) -> f64 {
        pt / self.points()
    }

    /// Label as shown in Preferences and the ruler context menu.
    pub fn label(self) -> &'static str {
        match self {
            Unit::Points => "Points",
            Unit::Picas => "Picas",
            Unit::Inches => "Inches",
            Unit::InchesDecimal => "Inches Decimal",
            Unit::Millimeters => "Millimeters",
            Unit::Centimeters => "Centimeters",
            Unit::Ciceros => "Ciceros",
            Unit::Agates => "Agates",
            Unit::Pixels => "Pixels",
            Unit::Q => "Q",
            Unit::Ha => "Ha",
        }
    }

    /// Suffix used when formatting (`pt`, `mm`, `in`…). Picas and ciceros use their own notation.
    pub fn suffix(self) -> &'static str {
        match self {
            Unit::Points => " pt",
            Unit::Picas => "p",
            Unit::Inches | Unit::InchesDecimal => " in",
            Unit::Millimeters => " mm",
            Unit::Centimeters => " cm",
            Unit::Ciceros => "c",
            Unit::Agates => " ag",
            Unit::Pixels => " px",
            Unit::Q => " Q",
            Unit::Ha => " H",
        }
    }

    /// Ruler tick spacing (major, subdivisions) in points for a given zoom (screen px per pt).
    pub fn ruler_ticks(self, zoom: f64) -> (f64, u32) {
        // A non-positive zoom can keep the major-spacing loop growing forever. Clamp
        // extreme positive values too, so saved/corrupt viewport state cannot overflow
        // the spacing calculation before the view restores a usable zoom.
        let zoom = if zoom.is_finite() && zoom > 0.0 { zoom.clamp(1e-6, 1e6) } else { 1.0 };
        let (base, subs): (f64, &[u32]) = match self {
            Unit::Inches | Unit::InchesDecimal => (PT_PER_INCH, &[8, 4, 2]),
            Unit::Picas => (PT_PER_PICA, &[6, 3, 2]),
            Unit::Ciceros => (PT_PER_CICERO, &[6, 3, 2]),
            Unit::Millimeters => (10.0 * PT_PER_MM, &[10, 5, 2]),
            Unit::Centimeters => (PT_PER_CM, &[10, 5, 2]),
            Unit::Agates => (PT_PER_AGATE * 14.0, &[14, 7, 2]),
            Unit::Q | Unit::Ha => (40.0 * PT_PER_Q, &[4, 2]),
            Unit::Points | Unit::Pixels => (72.0, &[8, 4, 2]),
        };
        // Choose a major spacing (base × 1, 2, 5, 10…) at least ~60 px apart, or base / 2^n when zoomed in.
        let mut major = base;
        while major * zoom < 50.0 {
            major *= if (major / base).log10().fract().abs() < 1e-9 { 2.0 } else { 2.5 };
        }
        while major * zoom > 160.0 && major > base / 64.0 {
            major /= 2.0;
        }
        let px = major * zoom;
        let sub = subs.iter().copied().find(|s| px / *s as f64 >= 6.0).unwrap_or(1);
        (major, sub)
    }
}

/// Error for an unparseable measurement.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
#[error("invalid measurement `{0}`")]
pub struct MeasureError(pub String);

/// Parse a measurement field into points. Bare numbers use `default`. Supports `+ - * /`
/// (left-to-right with `*`/`/` binding tighter), and `%` (of `percent_base`).
pub fn parse_measure(input: &str, default: Unit) -> Result<f64, MeasureError> {
    parse_measure_rel(input, default, None)
}

/// Like [`parse_measure`] but `%` is resolved against `percent_base` (points).
pub fn parse_measure_rel(input: &str, default: Unit, percent_base: Option<f64>) -> Result<f64, MeasureError> {
    let err = || MeasureError(input.to_string());
    let s: String = input.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_ascii_lowercase();
    if s.is_empty() {
        return Err(err());
    }
    // Tokenize into terms separated by + and - (a leading sign belongs to the first term).
    let mut terms: Vec<(f64, String)> = Vec::new();
    let mut sign = 1.0;
    let mut cur = String::new();
    for (i, ch) in s.char_indices() {
        if (ch == '+' || ch == '-') && i > 0 && !cur.is_empty() && !cur.ends_with(['*', '/']) {
            terms.push((sign, std::mem::take(&mut cur)));
            sign = if ch == '-' { -1.0 } else { 1.0 };
        } else if (ch == '+' || ch == '-') && cur.is_empty() {
            sign *= if ch == '-' { -1.0 } else { 1.0 };
        } else {
            cur.push(ch);
        }
    }
    if cur.is_empty() {
        return Err(err());
    }
    terms.push((sign, cur));
    let mut total = 0.0;
    for (sign, term) in terms {
        total += sign * parse_product(&term, default, percent_base).ok_or_else(err)?;
    }
    if total.is_finite() { Ok(total) } else { Err(err()) }
}

/// `a*b/c`: the first factor carries the unit; later factors are plain numbers (or measurements,
/// in which case only the first operand's unit counts, matching InDesign's arithmetic).
fn parse_product(term: &str, default: Unit, percent_base: Option<f64>) -> Option<f64> {
    let mut val: Option<f64> = None;
    let mut op = '*';
    let mut tok = String::new();
    let flush = |tok: &str, val: Option<f64>, op: char| -> Option<f64> {
        let x = if val.is_none() { parse_single(tok, default, percent_base)? } else { parse_number_or_measure(tok, default)? };
        Some(match (val, op) {
            (None, _) => x,
            (Some(v), '*') => v * x,
            (Some(v), _) => {
                if x == 0.0 {
                    return None;
                }
                v / x
            }
        })
    };
    for ch in term.chars() {
        if ch == '*' || ch == '/' {
            val = Some(flush(&tok, val, op)?);
            tok.clear();
            op = ch;
        } else {
            tok.push(ch);
        }
    }
    flush(&tok, val, op)
}

fn parse_number_or_measure(tok: &str, _default: Unit) -> Option<f64> {
    // Multipliers/divisors are unitless numbers.
    tok.parse::<f64>().ok()
}

fn parse_single(tok: &str, default: Unit, percent_base: Option<f64>) -> Option<f64> {
    if tok.is_empty() {
        return None;
    }
    if let Some(n) = tok.strip_suffix('%') {
        return Some(n.parse::<f64>().ok()? / 100.0 * percent_base?);
    }
    // Picas `1p6`, `p6`, `1p`; ciceros `1c4`, `c4`, `2c`.
    for (sep, unit_pt, sub_pt) in [('p', PT_PER_PICA, 1.0), ('c', PT_PER_CICERO, PT_PER_DIDOT)] {
        if let Some(pos) = tok.find(sep) {
            let (a, b) = (&tok[..pos], &tok[pos + 1..]);
            let num_ok = |s: &str| s.is_empty() || s.parse::<f64>().is_ok();
            // Exclude `pt`, `px`, `cm`.
            if num_ok(a) && num_ok(b) && !(a.is_empty() && b.is_empty()) {
                let a = if a.is_empty() { 0.0 } else { a.parse::<f64>().ok()? };
                let b = if b.is_empty() { 0.0 } else { b.parse::<f64>().ok()? };
                return Some(a * unit_pt + b * sub_pt);
            }
        }
    }
    let split = tok.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(tok.len());
    let (num, suffix) = tok.split_at(split);
    let n: f64 = num.parse().ok()?;
    let unit = match suffix {
        "" => default,
        "pt" | "pts" | "point" | "points" => Unit::Points,
        "px" | "pixel" | "pixels" => Unit::Pixels,
        "in" | "inch" | "inches" | "\"" | "i" => Unit::Inches,
        "mm" | "millimeter" | "millimeters" => Unit::Millimeters,
        "cm" | "centimeter" | "centimeters" => Unit::Centimeters,
        "ag" | "agate" | "agates" => Unit::Agates,
        "q" => Unit::Q,
        "h" | "ha" => Unit::Ha,
        _ => return None,
    };
    Some(unit.to_pt(n))
}

/// Format a value (points) in `unit` the way InDesign fields show it: up to 3 decimals, trailing
/// zeros trimmed; picas as `3p6`, ciceros as `2c4`.
pub fn format_measure(pt: f64, unit: Unit) -> String {
    format_measure_prec(pt, unit, 3)
}

pub fn format_measure_prec(pt: f64, unit: Unit, decimals: usize) -> String {
    let trim = |v: f64| {
        let s = format!("{v:.decimals$}");
        let s = if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s };
        if s == "-0" { "0".to_string() } else { s }
    };
    match unit {
        Unit::Picas | Unit::Ciceros => {
            let (big, small) = if unit == Unit::Picas { (PT_PER_PICA, 1.0) } else { (PT_PER_CICERO, PT_PER_DIDOT) };
            let neg = pt < 0.0;
            let a = pt.abs();
            let mut whole = (a / big).floor();
            let mut rest = (a - whole * big) / small;
            if (rest - 12.0).abs() < 1e-6 || rest > 12.0 - 1e-6 {
                whole += 1.0;
                rest = 0.0;
            }
            let sep = if unit == Unit::Picas { 'p' } else { 'c' };
            format!("{}{}{}{}", if neg { "-" } else { "" }, whole as i64, sep, trim(rest))
        }
        _ => format!("{}{}", trim(unit.from_pt(pt)), unit.suffix()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> f64 {
        parse_measure(s, Unit::Points).unwrap()
    }
    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    #[test]
    fn plain_and_units() {
        assert!(close(p("12"), 12.0));
        assert!(close(p("12pt"), 12.0));
        assert!(close(p("1in"), 72.0));
        assert!(close(p("25.4mm"), 72.0));
        assert!(close(p("2.54 cm"), 72.0));
        assert!(close(p("14ag"), 72.0));
        assert!(close(p("4Q"), PT_PER_MM));
        assert!(close(parse_measure("3", Unit::Millimeters).unwrap(), 3.0 * PT_PER_MM));
    }

    #[test]
    fn picas_and_ciceros() {
        assert!(close(p("1p6"), 18.0));
        assert!(close(p("p6"), 6.0));
        assert!(close(p("3p"), 36.0));
        assert!(close(p("1p6.5"), 18.5));
        assert!(close(p("1c"), PT_PER_CICERO));
        assert!(close(p("1c4"), PT_PER_CICERO + 4.0 * PT_PER_DIDOT));
    }

    #[test]
    fn arithmetic() {
        assert!(close(p("12+6"), 18.0));
        assert!(close(p("1in-36"), 36.0));
        assert!(close(p("10*3"), 30.0));
        assert!(close(p("100/4"), 25.0));
        assert!(close(p("1p+6pt"), 18.0));
        assert!(close(p("-6"), -6.0));
        assert!(close(p("2*3+4"), 10.0));
        assert!(close(parse_measure_rel("50%", Unit::Points, Some(40.0)).unwrap(), 20.0));
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_measure("", Unit::Points).is_err());
        assert!(parse_measure("abc", Unit::Points).is_err());
        assert!(parse_measure("12zz", Unit::Points).is_err());
        assert!(parse_measure("4/0", Unit::Points).is_err());
        assert!(parse_measure("50%", Unit::Points).is_err());
    }

    #[test]
    fn formatting() {
        assert_eq!(format_measure(18.0, Unit::Picas), "1p6");
        assert_eq!(format_measure(36.0, Unit::Picas), "3p0");
        assert_eq!(format_measure(12.5, Unit::Points), "12.5 pt");
        assert_eq!(format_measure(72.0, Unit::Inches), "1 in");
        assert_eq!(format_measure(72.0, Unit::Millimeters), "25.4 mm");
        assert_eq!(format_measure(-18.0, Unit::Picas), "-1p6");
        assert_eq!(format_measure(0.0, Unit::Points), "0 pt");
    }

    #[test]
    fn format_parse_roundtrip() {
        for u in Unit::ALL {
            for v in [0.0, 1.0, 18.0, 72.0, 612.0, 123.456] {
                let s = format_measure_prec(v, u, 6);
                let back = parse_measure(&s, u).unwrap_or_else(|e| panic!("{u:?} {s}: {e}"));
                assert!((back - v).abs() < 1e-3, "{u:?}: {v} -> {s} -> {back}");
            }
        }
    }

    #[test]
    fn ruler_ticks_reject_invalid_zoom_without_hanging_or_overflowing() {
        for unit in Unit::ALL {
            let fallback = unit.ruler_ticks(1.0);
            for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0, -0.0, 0.0] {
                assert_eq!(unit.ruler_ticks(bad), fallback, "{unit:?}: {bad}");
            }
            for extreme in [f64::MIN_POSITIVE, 1e-300, 1e300, f64::MAX] {
                let (major, subdivisions) = unit.ruler_ticks(extreme);
                assert!(major.is_finite() && major > 0.0 && subdivisions > 0, "{unit:?}: {extreme}");
            }
        }
    }

    #[test]
    fn ruler_ticks_are_reasonable() {
        for u in Unit::ALL {
            for z in [0.1, 0.5, 1.0, 2.0, 8.0] {
                let (major, sub) = u.ruler_ticks(z);
                assert!(major > 0.0 && sub >= 1, "{u:?} {z}");
                assert!(major * z >= 20.0, "{u:?} {z} {major}");
            }
        }
    }
}
