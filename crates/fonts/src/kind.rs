//! What the font menus say about a face: its file format (the icon after the sample), its
//! classification (the Filters menu) and its PANOSE numbers (Show Similar Fonts).

use skrifa::raw::TableProvider;
use skrifa::raw::types::Tag;

/// A face's font format, as the font menus show it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FontFormat {
    /// TrueType outlines (`glyf`).
    TrueType,
    /// PostScript (CFF) outlines.
    OpenType,
    /// A variable font (`fvar`).
    Variable,
    /// Colour glyphs (COLR, SVG, bitmaps): emoji and colour fonts.
    Color,
}

/// The classes the font menus filter by.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FontClass {
    Serif,
    SlabSerif,
    SansSerif,
    Script,
    Monospaced,
    Decorative,
    Symbol,
    /// Nothing in the font says.
    Other,
}

impl FontClass {
    /// The classes, in the Filters menu's order.
    pub const ALL: [FontClass; 8] = [
        FontClass::Serif,
        FontClass::SlabSerif,
        FontClass::SansSerif,
        FontClass::Script,
        FontClass::Monospaced,
        FontClass::Decorative,
        FontClass::Symbol,
        FontClass::Other,
    ];

    /// The name the Filters menu shows (English; the UI translates it).
    pub fn label(self) -> &'static str {
        match self {
            FontClass::Serif => "Serif",
            FontClass::SlabSerif => "Slab Serif",
            FontClass::SansSerif => "Sans Serif",
            FontClass::Script => "Script",
            FontClass::Monospaced => "Monospaced",
            FontClass::Decorative => "Decorative",
            FontClass::Symbol => "Symbol",
            FontClass::Other => "Other",
        }
    }
}

/// A face's format, class and PANOSE numbers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FaceKind {
    pub format: FontFormat,
    pub class: FontClass,
    /// The OS/2 table's PANOSE classification (all zero when the font has none).
    pub panose: [u8; 10],
}

impl Default for FaceKind {
    fn default() -> Self {
        FaceKind { format: FontFormat::TrueType, class: FontClass::Other, panose: [0; 10] }
    }
}

/// Read a face's kind from its tables; `family` helps classify fonts whose tables don't say.
pub fn read_kind(f: &skrifa::FontRef<'_>, family: &str) -> FaceKind {
    let has = |tag: &[u8; 4]| f.table_data(Tag::new(tag)).is_some();
    let format = if [b"COLR", b"SVG ", b"CBDT", b"sbix"].iter().any(|t| has(t)) {
        FontFormat::Color
    } else if has(b"fvar") {
        FontFormat::Variable
    } else if has(b"CFF ") || has(b"CFF2") {
        FontFormat::OpenType
    } else {
        FontFormat::TrueType
    };
    let os2 = f.os2().ok();
    let panose: [u8; 10] = os2.as_ref().and_then(|t| t.panose_10().try_into().ok()).unwrap_or([0; 10]);
    let ibm = os2.as_ref().map_or(0, |t| (t.s_family_class() >> 8) as u8);
    let fixed = f.post().ok().is_some_and(|p| p.is_fixed_pitch() != 0);
    FaceKind { format, class: classify(&panose, ibm, fixed, family), panose }
}

/// The class from PANOSE (family kind, serif style, proportion), else the IBM family class in OS/2,
/// else the family name.
pub fn classify(panose: &[u8; 10], ibm_class: u8, fixed_pitch: bool, family: &str) -> FontClass {
    let name = family.to_lowercase();
    let named = |words: &[&str]| words.iter().any(|w| name.contains(w));
    // PANOSE proportion 9 is monospaced (Latin text).
    if fixed_pitch || (panose[0] == 2 && panose[3] == 9) || named(&["mono", "courier", " code"]) {
        return FontClass::Monospaced;
    }
    match panose[0] {
        3 => return FontClass::Script,
        4 => return FontClass::Decorative,
        5 => return FontClass::Symbol,
        2 => match panose[1] {
            // Square serifs are slabs; cove, bone, triangle… serifs are serifs.
            6 => return FontClass::SlabSerif,
            2..=10 => return FontClass::Serif,
            11..=15 => return FontClass::SansSerif,
            _ => {}
        },
        _ => {}
    }
    match ibm_class {
        1 | 2 | 3 | 7 => FontClass::Serif,
        4 | 5 => FontClass::SlabSerif,
        8 => FontClass::SansSerif,
        9 => FontClass::Decorative,
        10 => FontClass::Script,
        12 => FontClass::Symbol,
        _ if named(&["slab"]) => FontClass::SlabSerif,
        _ if named(&["sans", "grotesk", "grotesque", "gothic", "helvetica", "arial"]) => FontClass::SansSerif,
        _ if named(&["script", "hand", "brush"]) => FontClass::Script,
        _ if named(&["serif"]) => FontClass::Serif,
        _ => FontClass::Other,
    }
}

/// How unlike two faces look by their PANOSE numbers (0 = alike), counting only the digits both
/// fonts set; `None` when they share none.
pub fn panose_distance(a: &[u8; 10], b: &[u8; 10]) -> Option<u32> {
    let pairs: Vec<(u8, u8)> = a.iter().zip(b).skip(1).filter(|(x, y)| **x > 1 && **y > 1).map(|(x, y)| (*x, *y)).collect();
    if pairs.is_empty() {
        return None;
    }
    Some(pairs.iter().map(|(x, y)| u32::from(x.abs_diff(*y))).sum::<u32>() * 9 / pairs.len() as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classes_come_from_panose_then_the_ibm_class_then_the_name() {
        let latin = |serif: u8, proportion: u8| [2, serif, 5, proportion, 5, 0, 0, 0, 0, 0];
        assert_eq!(classify(&latin(2, 3), 0, false, "Garamond"), FontClass::Serif);
        assert_eq!(classify(&latin(6, 3), 0, false, "Rockwell"), FontClass::SlabSerif);
        assert_eq!(classify(&latin(11, 3), 0, false, "Inter"), FontClass::SansSerif);
        assert_eq!(classify(&latin(11, 9), 0, false, "Inter"), FontClass::Monospaced, "proportion 9");
        assert_eq!(classify(&[3, 0, 0, 0, 0, 0, 0, 0, 0, 0], 0, false, "Brush"), FontClass::Script);
        assert_eq!(classify(&[0; 10], 8, false, "Untitled"), FontClass::SansSerif, "IBM class 8");
        assert_eq!(classify(&[0; 10], 0, true, "Untitled"), FontClass::Monospaced, "post.isFixedPitch");
        assert_eq!(classify(&[0; 10], 0, false, "JetBrains Mono"), FontClass::Monospaced);
        assert_eq!(classify(&[0; 10], 0, false, "Roboto Slab"), FontClass::SlabSerif);
        assert_eq!(classify(&[0; 10], 0, false, "Plain"), FontClass::Other);
    }

    #[test]
    fn bundled_fonts_are_classified() {
        let db = crate::FontDb::global();
        for (family, class) in [
            ("Source Serif 4", FontClass::Serif),
            ("Source Sans 3", FontClass::SansSerif),
            ("Inter", FontClass::SansSerif),
            ("JetBrains Mono", FontClass::Monospaced),
        ] {
            let kind = db.face(family, "Regular").kind();
            assert_eq!(kind.class, class, "{family}");
            assert_eq!(kind.format, FontFormat::TrueType, "{family}");
        }
        let serif = db.face("Source Serif 4", "Regular").kind().panose;
        let sans = db.face("Source Sans 3", "Regular").kind().panose;
        let serif_bold = db.face("Source Serif 4", "Bold").kind().panose;
        if let (Some(near), Some(far)) = (panose_distance(&serif, &serif_bold), panose_distance(&serif, &sans)) {
            assert!(near < far, "the bold serif is nearer the serif than the sans is ({near} vs {far})");
        }
    }

    #[test]
    fn panose_distance_counts_shared_digits() {
        let a = [2, 2, 5, 3, 5, 4, 5, 2, 2, 4];
        assert_eq!(panose_distance(&a, &a), Some(0));
        let b = [2, 11, 5, 3, 5, 4, 5, 2, 2, 4];
        assert!(panose_distance(&a, &b).is_some_and(|d| d > 0));
        assert_eq!(panose_distance(&a, &[0; 10]), None, "no PANOSE: nothing to compare");
    }
}
