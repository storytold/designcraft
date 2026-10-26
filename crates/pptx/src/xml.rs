//! XML text helpers and DrawingML units.

/// English Metric Units per point.
pub const EMU_PER_PT: f64 = 12_700.0;

/// Points → EMU, saturating (input-derived geometry may be huge or not finite).
pub fn emu(pt: f64) -> i64 {
    if !pt.is_finite() {
        return 0;
    }
    (pt * EMU_PER_PT).round().clamp(-27_273_042_316_900.0, 27_273_042_316_900.0) as i64
}

/// A non-negative extent in EMU (DrawingML rejects negative sizes).
pub fn emu_len(pt: f64) -> i64 {
    emu(pt).max(0)
}

/// Degrees → DrawingML angle units (60 000ths of a degree), normalised to 0..360°.
pub fn angle(deg: f64) -> i64 {
    if !deg.is_finite() {
        return 0;
    }
    (deg.rem_euclid(360.0) * 60_000.0).round() as i64 % 21_600_000
}

/// A 0..1 fraction → DrawingML percentage units (1000ths of a percent).
pub fn pct(v: f64) -> i64 {
    if !v.is_finite() {
        return 0;
    }
    (v * 100_000.0).round().clamp(-100_000_000.0, 100_000_000.0) as i64
}

/// Escape text for element content and attribute values. Characters XML 1.0 can't hold are
/// dropped.
pub fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&apos;"),
            '\t' | '\n' | '\r' => o.push(c),
            c if (c as u32) < 0x20 || c == '\u{FFFE}' || c == '\u{FFFF}' => {}
            c => o.push(c),
        }
    }
    o
}

/// sRGB 0..1 → `RRGGBB`.
pub fn hex(rgb: [f32; 3]) -> String {
    let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("{:02X}{:02X}{:02X}", q(rgb[0]), q(rgb[1]), q(rgb[2]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn units() {
        assert_eq!(emu(1.0), 12_700);
        assert_eq!(emu(f64::NAN), 0);
        assert_eq!(emu_len(-3.0), 0);
        assert_eq!(angle(-90.0), 16_200_000);
        assert_eq!(angle(360.0), 0);
        assert_eq!(pct(0.5), 50_000);
        assert_eq!(esc("a<b & \"c\"\u{1}"), "a&lt;b &amp; &quot;c&quot;");
        assert_eq!(hex([1.0, 0.0, 0.5]), "FF0080");
    }
}
