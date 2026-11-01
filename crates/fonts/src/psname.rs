//! The PostScript name of a variable font's named instance (Adobe TN #5902).
//!
//! `name` ID 6 names the font's **default** instance, so every instance of a variable font embeds
//! under the same name: all the weights of Noto Sans KR went into a PDF as `NotoSansKR-Thin`, and
//! Acrobat's font list, preflight and a print shop's checks then report Thin for every weight
//! (#1690). The outlines were right; only the name was wrong.

use skrifa::raw::TableProvider;
use skrifa::{FontRef, MetadataProvider, raw::types::NameId};

/// Longest PostScript name `name` ID 6 may carry (OpenType), which is where this name goes.
///
/// TN #5902 works to 127 characters and prescribes a "last resort" name past that; the shorter
/// limit here is the one that matters for a name written into the font's own `name` table.
const MAX_LEN: usize = 63;

/// Axis values count as the same instance within this much (user units).
const AXIS_EPSILON: f32 = 1e-4;

/// The PostScript name for the instance of `font` at `coords`, or `None` when the font names no
/// such instance (a point in between, or a static font).
///
/// TN #5902: a named instance's own `postScriptNameID` wins; otherwise the name is the family
/// prefix (`name` ID 25, else the typographic family, else the family) joined to the instance's
/// subfamily with a hyphen, keeping only ASCII letters and digits and no more than 63 characters.
pub fn instance_postscript_name(font: &FontRef<'_>, coords: &[([u8; 4], f32)]) -> Option<String> {
    let axes = font.axes();
    // `fvar` coordinates are 16.16, so two instances can sit 1/65536 apart -- closer than
    // AXIS_EPSILON. Asking for an exact instance would then match whichever of them came first
    // and give the one asked for the other's name, so an exact match is looked for first and the
    // tolerance is only a fallback for a point that is not exactly on any instance (noted in
    // review).
    let at = |tolerance: f32| {
        let axes = &axes;
        move |ni: &skrifa::NamedInstance<'_>| {
            let mut values = axes.iter().zip(ni.user_coords());
            coords.len() == axes.len()
                && values.all(|(axis, value)| coords.iter().any(|(tag, want)| *tag == axis.tag().to_be_bytes() && (want - value).abs() <= tolerance))
        }
    };
    let exact = at(0.0);
    let near = at(AXIS_EPSILON);
    let instance = font.named_instances().iter().find(|ni| exact(ni)).or_else(|| font.named_instances().iter().find(|ni| near(ni)))?;
    if let Some(id) = instance.postscript_name_id()
        && let Some(name) = string(font, id)
    {
        // The font names this instance itself, so the name is used as it stands -- a hyphen is
        // legal in a PostScript name, and `Pretendard-Heavy` is the name, not a family and a
        // subfamily to join (noted in review). Only characters a PostScript name cannot hold are
        // dropped.
        let kept = legal(&name);
        if !kept.is_empty() {
            return Some(truncate(kept));
        }
    }
    let prefix = [NameId::VARIATIONS_POSTSCRIPT_NAME_PREFIX, NameId::TYPOGRAPHIC_FAMILY_NAME, NameId::FAMILY_NAME]
        .into_iter()
        .find_map(|id| string(font, id))
        .map(|s| keep(&s))
        .filter(|s| !s.is_empty())?;
    let subfamily = string(font, instance.subfamily_name_id()).map(|s| keep(&s)).filter(|s| !s.is_empty())?;
    Some(join(&prefix, &subfamily))
}

/// `prefix-subfamily` within [`MAX_LEN`], shortening the **prefix** when it does not fit.
///
/// The subfamily is what tells two instances apart, so cutting the end would let `Thin` and
/// `Thin Italic` -- or any two long subfamilies -- arrive at the same name, and a PDF would then
/// embed two different instances under one name. TN #5902 keeps the instance's identifier for the
/// same reason.
fn join(prefix: &str, subfamily: &str) -> String {
    let whole = format!("{prefix}-{subfamily}");
    if whole.chars().count() <= MAX_LEN {
        return whole;
    }
    // A subfamily long enough to fill the limit on its own leaves no tail to cut that would
    // still tell two of them apart: with one prefix character and a hyphen, two subfamilies
    // differing only at character 62 arrived at the same name, and a PDF would embed two
    // instances under it -- the collision this function exists to prevent (noted in review).
    //
    // The name then ends with a digest of the whole subfamily rather than its missing tail,
    // which is the shape TN #5902 prescribes as a "last resort" name for an instance whose name
    // cannot be formed inside the limit. Answering `None` instead would put the instance back
    // under the font's default `name` ID 6, which is the bug this file is about (#1690).
    let sub_len = subfamily.chars().count();
    if sub_len > MAX_SUBFAMILY {
        let head: String = subfamily.chars().take(MAX_LEN - 1 - DIGEST_LEN).collect();
        return format!("{head}-{:08x}", digest(subfamily));
    }
    // One character of prefix at least, so the name still says which family it belongs to.
    let room = MAX_LEN.saturating_sub(sub_len + 1).max(1);
    let short: String = prefix.chars().take(room).collect();
    truncate(format!("{short}-{subfamily}"))
}

/// Longest subfamily that still leaves room for a prefix character and the hyphen before it.
const MAX_SUBFAMILY: usize = MAX_LEN - 2;

/// Hex characters the last-resort digest takes.
const DIGEST_LEN: usize = 8;

/// FNV-1a over the whole subfamily, so two subfamilies that agree as far as the limit still get
/// different names. Not a security property -- only that two different strings rarely collide.
fn digest(s: &str) -> u32 {
    s.bytes().fold(0x811c_9dc5_u32, |h, b| (h ^ u32::from(b)).wrapping_mul(0x0100_0193))
}

/// The characters a PostScript name may hold: printable ASCII except the delimiters `[](){}<>/%`
/// and space (OpenType `name` ID 6). Used for a name the font supplies itself.
fn legal(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_graphic() && !matches!(c, '[' | ']' | '(' | ')' | '{' | '}' | '<' | '>' | '/' | '%')).collect()
}

/// A `name` table string, preferring the US English Windows record a PDF reader looks at.
///
/// Picking the first record of the right platform took whichever language came first, so a font
/// whose subfamily is listed in another language before English generated its name from that
/// one -- or from a string whose characters all drop out, leaving no name at all (noted in
/// review). US English (0x0409 on Windows, 0 on Macintosh) is tried first.
fn string(font: &FontRef<'_>, id: NameId) -> Option<String> {
    let name = font.name().ok()?;
    let records = name.name_record();
    let pick = |platform: u16, language: Option<u16>| {
        records.iter().find(|r| r.name_id() == id && r.platform_id() == platform && language.is_none_or(|want| r.language_id() == want))
    };
    let record = pick(3, Some(0x0409))
        .or_else(|| pick(1, Some(0)))
        .or_else(|| pick(3, None))
        .or_else(|| pick(1, None))
        .or_else(|| records.iter().find(|r| r.name_id() == id))?;
    let text = record.string(name.string_data()).ok()?.chars().collect::<String>();
    (!text.trim().is_empty()).then_some(text)
}

/// ASCII letters and digits of `s`, which is what a PostScript name may hold.
fn keep(s: &str) -> String {
    s.chars().filter(char::is_ascii_alphanumeric).collect()
}

/// `s` at most [`MAX_LEN`] characters, cut on a character boundary (the name is ASCII here, so
/// this is a byte cut, but the font's own string need not have been).
fn truncate(s: String) -> String {
    match s.char_indices().nth(MAX_LEN) {
        Some((at, _)) => s[..at].to_string(),
        None => s,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A variable font named like Noto Sans KR: a `wght` axis and four named instances, with the
    /// family prefix in name ID 25.
    fn variable(entries: &[(u16, &str)], instances: &[(u16, Option<u16>, Vec<f32>)]) -> Vec<u8> {
        let base = crate::testing::font_with_names(entries, &['A']).expect("a font with these names");
        crate::testing::with_fvar(&base, &[(*b"wght", 100.0, 100.0, 900.0, 256)], instances).expect("an fvar")
    }

    fn at(font: &[u8], weight: f32) -> Option<String> {
        let f = skrifa::FontRef::new(font).expect("the fixture parses");
        instance_postscript_name(&f, &[(*b"wght", weight)])
    }

    /// `name` ID 6 names the default instance, so every weight embedded under it: the four
    /// weights of Noto Sans KR all went into the PDF as `NotoSansKR-Thin` (#1690).
    #[test]
    fn each_named_instance_has_its_own_postscript_name() {
        // 1 family, 6 the default instance's PostScript name, 25 the variations prefix,
        // 257.. the instances' subfamily names.
        let font = variable(
            &[(1, "Noto Sans KR"), (6, "NotoSansKR-Thin"), (25, "NotoSansKR"), (257, "Thin"), (258, "Regular"), (259, "Bold"), (260, "Black")],
            &[(257, None, vec![100.0]), (258, None, vec![400.0]), (259, None, vec![700.0]), (260, None, vec![900.0])],
        );
        assert_eq!(at(&font, 100.0).as_deref(), Some("NotoSansKR-Thin"));
        assert_eq!(at(&font, 400.0).as_deref(), Some("NotoSansKR-Regular"));
        assert_eq!(at(&font, 700.0).as_deref(), Some("NotoSansKR-Bold"));
        assert_eq!(at(&font, 900.0).as_deref(), Some("NotoSansKR-Black"));
        // A point between the named instances is not a named instance.
        assert_eq!(at(&font, 550.0), None);
    }

    /// An instance that names its own PostScript name uses it **as it stands**: the font is
    /// naming the instance, not handing over a family and a subfamily to join, and a hyphen is
    /// legal in a PostScript name (noted in review).
    #[test]
    fn an_instances_own_postscript_name_is_used_as_it_stands() {
        let font = variable(
            &[(1, "Pretendard Variable"), (6, "PretendardVariable-Regular"), (25, "PretendardVariable"), (257, "Bold"), (300, "Pretendard-Heavy")],
            &[(257, Some(300), vec![700.0])],
        );
        assert_eq!(at(&font, 700.0).as_deref(), Some("Pretendard-Heavy"), "the hyphen the font wrote is kept");

        // Only the characters a PostScript name cannot hold are dropped: the delimiters and
        // spaces, not punctuation a name may carry.
        let odd = variable(&[(1, "Fixture"), (25, "Fixture"), (257, "Bold"), (300, "Fix (ture)/Bold+2_x")], &[(257, Some(300), vec![700.0])]);
        assert_eq!(at(&odd, 700.0).as_deref(), Some("FixtureBold+2_x"));
    }

    /// Two instances must not arrive at the same name. Cutting the end of a long name would do
    /// exactly that, so the family prefix gives way and the subfamily -- the part that tells the
    /// instances apart -- survives whole (noted in review; TN #5902 keeps the identifier for the
    /// same reason).
    #[test]
    fn a_long_name_keeps_the_subfamily_that_tells_instances_apart() {
        let long_prefix = "P".repeat(70);
        let font = variable(
            &[(1, "Fixture"), (25, &long_prefix), (257, "CondensedExtraBoldItalic"), (258, "CondensedExtraBoldOblique")],
            &[(257, None, vec![700.0]), (258, None, vec![800.0])],
        );
        let (a, b) = (at(&font, 700.0).expect("a name"), at(&font, 800.0).expect("a name"));
        assert!(a.len() <= MAX_LEN && b.len() <= MAX_LEN, "within the limit: {} and {}", a.len(), b.len());
        assert!(a.ends_with("-CondensedExtraBoldItalic"), "the subfamily survives: {a}");
        assert!(b.ends_with("-CondensedExtraBoldOblique"), "the subfamily survives: {b}");
        assert_ne!(a, b, "two instances never share a name");
    }

    /// A name listed in another language before English must not be the one the name is built
    /// from (noted in review).
    #[test]
    fn the_english_record_is_preferred() {
        // `with_names` writes the whole table, so every record this needs is in the one call:
        // the subfamily is listed in Japanese *before* US English for the same id.
        let base = crate::testing::font_with("Fixture", &['A']).unwrap();
        let named = crate::testing::with_names(
            &base,
            &[
                (3, 1, 0x0411, 1, "Fixture"),
                (3, 1, 0x0409, 1, "Fixture"),
                (3, 1, 0x0409, 25, "Fixture"),
                (3, 1, 0x0411, 257, "\u{592A}\u{5B57}"),
                (3, 1, 0x0409, 257, "Bold"),
            ],
        )
        .expect("a name table with both languages");
        let font = crate::testing::with_fvar(&named, &[(*b"wght", 100.0, 100.0, 900.0, 256)], &[(257, None, vec![700.0])]).unwrap();
        let f = skrifa::FontRef::new(&font).unwrap();
        assert_eq!(instance_postscript_name(&f, &[(*b"wght", 700.0)]).as_deref(), Some("Fixture-Bold"));
    }

    /// An instance with no PostScript name of its own, beside one that has one: `fvar` writes
    /// 0xFFFF for "none", and 0 would be the copyright notice's id (noted in review).
    #[test]
    fn an_instance_without_its_own_name_falls_back_to_the_generated_one() {
        let font = variable(
            &[(0, "Copyright 2026 Nobody"), (1, "Fixture"), (25, "Fixture"), (257, "Bold"), (258, "Black"), (300, "Fixture-TheBold")],
            &[(257, Some(300), vec![700.0]), (258, None, vec![900.0])],
        );
        assert_eq!(at(&font, 700.0).as_deref(), Some("Fixture-TheBold"), "the instance that names itself");
        assert_eq!(at(&font, 900.0).as_deref(), Some("Fixture-Black"), "and the one that does not is generated");
    }

    /// Without ID 25 the typographic family is the prefix, then the family; and a name a
    /// PostScript name cannot hold keeps only its letters and digits.
    #[test]
    fn the_prefix_falls_back_and_the_name_is_sanitised() {
        let typographic = variable(&[(1, "Fixture"), (16, "Fixture Display"), (257, "Semi Bold")], &[(257, None, vec![600.0])]);
        assert_eq!(at(&typographic, 600.0).as_deref(), Some("FixtureDisplay-SemiBold"));

        let family_only = variable(&[(1, "Fixture (Beta)"), (257, "Extra Light")], &[(257, None, vec![200.0])]);
        assert_eq!(at(&family_only, 200.0).as_deref(), Some("FixtureBeta-ExtraLight"));
    }

    /// `name` ID 6 carries at most 63 characters.
    #[test]
    fn a_name_stays_within_the_limit() {
        let long = "L".repeat(80);
        let font = variable(&[(1, "Fixture"), (25, &long), (257, "Regular")], &[(257, None, vec![400.0])]);
        let name = at(&font, 400.0).expect("a name");
        assert!(name.len() <= MAX_LEN, "within the limit: {name}");
        assert!(name.ends_with("-Regular"), "and the subfamily is the part kept: {name}");
    }

    /// A static font has no named instances, so it keeps its own `name` ID 6.
    #[test]
    fn a_static_font_has_no_instance_name() {
        let base = crate::testing::font_with_names(&[(1, "Fixture"), (6, "Fixture-Regular")], &['A']).unwrap();
        let f = skrifa::FontRef::new(&base).unwrap();
        assert_eq!(instance_postscript_name(&f, &[]), None);
    }

    /// `fvar` coordinates are 16.16, so two instances can sit 1/65536 apart -- inside
    /// AXIS_EPSILON. Asking for one of them matched whichever came first and returned the
    /// other's name (noted in review).
    #[test]
    fn two_instances_closer_than_the_tolerance_keep_their_own_names() {
        // 1.0 and 1.0 + 1/65536, one 16.16 step apart: distinct in the file and 1.5e-5 apart,
        // which is inside AXIS_EPSILON (1e-4). The axis has to be a small-numbered one for the
        // two to be different f32s at all -- at a weight of 400 a step of 1/65536 is below half
        // an ulp, so the file cannot hold two such instances there in the first place.
        let step = 1.0 / 65536.0;
        let base = crate::testing::font_with_names(&[(1, "Test"), (6, "Test-Regular"), (25, "Test"), (257, "Regular"), (258, "RegularPlus")], &['A'])
            .expect("a font with these names");
        let font = crate::testing::with_fvar(&base, &[(*b"slnt", 0.0, 1.0, 2.0, 256)], &[(257, None, vec![1.0]), (258, None, vec![1.0 + step])])
            .expect("an fvar");
        let slnt = |font: &[u8], v: f32| {
            let f = skrifa::FontRef::new(font).expect("the fixture parses");
            instance_postscript_name(&f, &[(*b"slnt", v)])
        };
        assert_ne!(1.0f32, 1.0 + step, "the two coordinates really are different numbers");
        assert_eq!(slnt(&font, 1.0).as_deref(), Some("Test-Regular"), "the exact instance wins");
        assert_eq!(slnt(&font, 1.0 + step).as_deref(), Some("Test-RegularPlus"), "and so does the one next to it");
        // A point on neither still falls back to the tolerance, which is what it is for.
        assert_eq!(slnt(&font, 1.0 + step / 2.0).as_deref(), Some("Test-Regular"), "a point in between takes the nearest match");
    }

    /// A subfamily that fills the limit on its own left one prefix character and a cut subfamily,
    /// so two subfamilies differing only past the limit arrived at the same name -- the collision
    /// `join` exists to prevent (noted in review).
    #[test]
    fn two_subfamilies_that_agree_as_far_as_the_limit_still_differ() {
        let head = "A".repeat(61);
        let (one, two) = (format!("{head}B"), format!("{head}C"));
        let a = join("Family", &one);
        let b = join("Family", &two);
        assert_ne!(a, b, "{a} and {b} name different instances");
        assert!(a.chars().count() <= MAX_LEN, "{} characters", a.chars().count());
        assert!(b.chars().count() <= MAX_LEN, "{} characters", b.chars().count());
        // A subfamily that does fit keeps every character of itself, prefix shortened instead.
        let fits = join("Family", &"A".repeat(MAX_SUBFAMILY));
        assert!(fits.ends_with(&"A".repeat(MAX_SUBFAMILY)), "the subfamily is kept whole: {fits}");
        assert_eq!(fits.chars().count(), MAX_LEN);
        // And an ordinary name is untouched.
        assert_eq!(join("NotoSansKR", "Thin"), "NotoSansKR-Thin");
    }
}
