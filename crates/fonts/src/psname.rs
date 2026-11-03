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

/// The PostScript name for the instance of `font` at `coords`, or `None` for a static font and for
/// the default instance, which `name` ID 6 already names.
///
/// TN #5902: a named instance's own `postScriptNameID` wins; otherwise the name is the family
/// prefix (`name` ID 25, else the typographic family, else the family -- OpenType reads a missing
/// ID 16 as ID 1) joined to the instance's subfamily with a hyphen, keeping only ASCII letters and
/// digits. A point no named instance covers is named by its axis values
/// ([`arbitrary_instance_name`]). No more than 63 characters either way, the limit of the `name`
/// ID 6 this goes into, where the note allows 127.
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
    let Some(instance) = font.named_instances().iter().find(|ni| exact(ni)).or_else(|| font.named_instances().iter().find(|ni| near(ni))) else {
        return arbitrary_instance_name(font, coords);
    };
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
    let prefix = family_prefix(font)?;
    let subfamily = string(font, instance.subfamily_name_id()).map(|s| keep(&s)).filter(|s| !s.is_empty())?;
    Some(join(&prefix, &subfamily))
}

/// TN #5902's family prefix: `name` ID 25, else the typographic family (ID 16), else the family
/// (ID 1, which OpenType says stands in for a missing ID 16), keeping ASCII letters and digits.
fn family_prefix(font: &FontRef<'_>) -> Option<String> {
    [NameId::VARIATIONS_POSTSCRIPT_NAME_PREFIX, NameId::TYPOGRAPHIC_FAMILY_NAME, NameId::FAMILY_NAME]
        .into_iter()
        .find_map(|id| string(font, id))
        .map(|s| keep(&s))
        .filter(|s| !s.is_empty())
}

/// TN #5902's name for a point no named instance covers: the family prefix, then `_<value><tag>`
/// for each axis away from its default, in `fvar` order -- `AndreVar_900wght_5.5wdth`. Axes at
/// their default are left out, as the note allows, so the default point itself has no
/// descriptors and gets `None`: `name` ID 6 is its name.
///
/// Returning `None` for every such point, which this did first, left all of them embedding under
/// the default instance's name -- the bug this module is for, at any weight that is not a named
/// one (noted in review).
fn arbitrary_instance_name(font: &FontRef<'_>, coords: &[([u8; 4], f32)]) -> Option<String> {
    let axes = font.axes();
    if axes.is_empty() {
        return None;
    }
    let prefix = family_prefix(font)?;
    let mut name = prefix.clone();
    for axis in axes.iter() {
        let tag = axis.tag().to_be_bytes();
        let asked = coords.iter().find(|(t, _)| *t == tag).map_or(axis.default_value(), |(_, v)| *v);
        // Where the font draws it: inside the axis. Compared, not `clamp`ed -- `fvar` is font data,
        // and `f32::clamp` panics on a range whose minimum is past its maximum.
        let (min, max) = (axis.min_value(), axis.max_value());
        let value = if asked < min {
            min
        } else if asked > max {
            max
        } else {
            asked
        };
        let fixed = to_fixed(value);
        if fixed == to_fixed(axis.default_value()) {
            continue;
        }
        name.push('_');
        name.push_str(&fixed_decimal(fixed));
        // Trailing spaces are dropped from a tag (`ital`, `wd  `); anything else not a letter or
        // digit cannot be in the name either.
        name.push_str(&keep(&String::from_utf8_lossy(&tag)));
    }
    if name == prefix {
        return None;
    }
    if name.chars().count() <= MAX_LEN {
        return Some(name);
    }
    // TN #5902's last resort: the prefix, `-`, an identifier for this instance, and `...`.
    let id = format!("{:08X}", digest(&name));
    let room = MAX_LEN - 1 - id.len() - 3;
    Some(format!("{}-{id}...", prefix.chars().take(room).collect::<String>()))
}

/// A user coordinate as `fvar`'s 16.16 fixed point. A value past `i32` saturates and NaN is 0, as
/// `as` does; neither reaches here from a font that parsed.
fn to_fixed(v: f32) -> i32 {
    (f64::from(v) * 65536.0).round() as i32
}

/// The shortest decimal that reads back as the 16.16 value `v` (TN #5902 and its appendix): no
/// trailing zeroes, no decimal point for a whole number, `0` for zero and no `+`. A value under
/// one keeps the `0` before its point (`0.5`), the zero the integer part needs, not padding.
fn fixed_decimal(v: i32) -> String {
    let x = f64::from(v) / 65536.0;
    // Sixteen fractional bits need at most six decimal places to come back exactly.
    for places in 0..=6 {
        let s = format!("{x:.places$}");
        let back = s.parse::<f64>().map(|b| (b * 65536.0).round() as i64);
        if back == Ok(i64::from(v)) {
            let s = if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s };
            return if s == "-0" { "0".into() } else { s };
        }
    }
    format!("{x}")
}

/// `prefix-subfamily`, or, when that is past [`MAX_LEN`], both halves shortened with a digest of
/// the whole after them.
///
/// Both halves matter. The subfamily is what tells two instances of one family apart, so cutting
/// its end lets `Thin` and `Thin Italic` meet; the prefix is what tells two families apart, so
/// cutting *its* end lets `FamilyOne` and `FamilyTwo` meet. Neither can be shortened on its own
/// and stay unique, so a name that does not fit keeps a readable piece of each and ends in a
/// digest of `prefix-subfamily` entire -- the shape TN #5902 prescribes as a "last resort" name
/// for an instance that cannot be named inside the limit. A PDF embedding two instances under one
/// name is the thing this function exists to stop.
///
/// Shortening only one half -- what this did before -- collided two ways, both noted in review: a
/// 62-character subfamily took a digest of the subfamily alone, so every family reached the same
/// head and the same digest, and a 60-character one left room for two prefix characters, so
/// `FamilyOne` and `FamilyTwo` both shortened to `Fa`.
fn join(prefix: &str, subfamily: &str) -> String {
    let whole = format!("{prefix}-{subfamily}");
    if whole.chars().count() <= MAX_LEN {
        return whole;
    }
    // The subfamily is usually the short half, so it is served first, down to the floor the
    // prefix keeps for itself; the prefix takes what is left. The digest, not either piece, is
    // what actually keeps this name off another one.
    let sub_room = (VISIBLE - PREFIX_MIN - 1).min(subfamily.chars().count());
    let short_prefix: String = prefix.chars().take(VISIBLE - 1 - sub_room).collect();
    let short_subfamily: String = subfamily.chars().take(sub_room).collect();
    format!("{short_prefix}-{short_subfamily}-{:08x}", digest(&whole))
}

/// Characters a shortened name's readable `prefix-subfamily` gets, the rest going to the digest
/// and the hyphen before it.
const VISIBLE: usize = MAX_LEN - DIGEST_LEN - 1;

/// A shortened name keeps at least this much of the prefix, so it still says which family it is.
const PREFIX_MIN: usize = 8;

/// Hex characters the last-resort digest takes.
const DIGEST_LEN: usize = 8;

/// FNV-1a over the whole `prefix-subfamily`, so two names that agree as far as the limit still
/// differ. Not a security property -- only that two different strings rarely collide.
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
        // A point between the named instances is not one of them, and is named by its value
        // (TN #5902, arbitrary instances).
        assert_eq!(at(&font, 550.0).as_deref(), Some("NotoSansKR_550wght"));
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
    /// instances apart -- is kept whole where there is room (noted in review; TN #5902 keeps the
    /// identifier for the same reason). The digest after it is what keeps the name off another
    /// family's, which shortening the prefix alone did not.
    #[test]
    fn a_long_name_keeps_the_subfamily_that_tells_instances_apart() {
        let long_prefix = "P".repeat(70);
        let font = variable(
            &[(1, "Fixture"), (25, &long_prefix), (257, "CondensedExtraBoldItalic"), (258, "CondensedExtraBoldOblique")],
            &[(257, None, vec![700.0]), (258, None, vec![800.0])],
        );
        let (a, b) = (at(&font, 700.0).expect("a name"), at(&font, 800.0).expect("a name"));
        assert!(a.len() <= MAX_LEN && b.len() <= MAX_LEN, "within the limit: {} and {}", a.len(), b.len());
        assert!(a.contains("-CondensedExtraBoldItalic-"), "the subfamily survives: {a}");
        assert!(b.contains("-CondensedExtraBoldOblique-"), "the subfamily survives: {b}");
        assert!(a.starts_with(&long_prefix[..8]) && b.starts_with(&long_prefix[..8]), "and so does enough of the family: {a}");
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
        // Both halves are kept, the subfamily whole here because it is short, and the digest
        // after them is what tells this name from another family's (noted in review).
        assert!(name.contains("-Regular-"), "and the subfamily is kept: {name}");
        assert!(name.starts_with(&long[..8]), "with enough of the family to name it: {name}");
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

    /// Two instances must never arrive at one name: a PDF would then embed two different ones
    /// under it. Both halves of the name can be what tells them apart, and shortening either in
    /// isolation collided -- the subfamily's tail for two instances of a family, the prefix's for
    /// two families (both noted in review).
    #[test]
    fn two_names_that_agree_as_far_as_the_limit_still_differ() {
        // Same family, subfamilies differing only past the limit.
        let head = "A".repeat(61);
        let (a, b) = (join("Family", &format!("{head}B")), join("Family", &format!("{head}C")));
        assert_ne!(a, b, "{a} and {b} name different instances of one family");

        // Different families, one subfamily. 62 characters took the digest path, which digested
        // the subfamily alone, so every family reached the same head and the same digest; 60 left
        // room for two prefix characters, so `FamilyOne` and `FamilyTwo` both became `Fa`.
        for len in [60, 62] {
            let sub = "A".repeat(len);
            let (one, two) = (join("FamilyOne", &sub), join("FamilyTwo", &sub));
            assert_ne!(one, two, "subfamily of {len}: {one} and {two} name different families");
        }

        // A one-character difference anywhere in the prefix is enough, however far past the cut.
        let long = "P".repeat(80);
        let (p, q) = (join(&format!("{long}A"), "Thin"), join(&format!("{long}B"), "Thin"));
        assert_ne!(p, q, "{p} and {q} name different families");

        // Every shortened name is inside the limit, and is a name a PDF can hold.
        for name in [a, b, p, q, join("FamilyOne", &"A".repeat(62))] {
            assert!(name.chars().count() <= MAX_LEN, "{} characters: {name}", name.chars().count());
            assert_eq!(legal(&name), name, "{name} has a character a PostScript name may not hold");
        }

        // An ordinary name is untouched, and so is one that fits exactly.
        assert_eq!(join("NotoSansKR", "Thin"), "NotoSansKR-Thin");
        let exact = join("Family", &"A".repeat(MAX_LEN - "Family-".len()));
        assert_eq!(exact.chars().count(), MAX_LEN);
        assert!(exact.starts_with("Family-") && exact.ends_with('A'), "kept whole: {exact}");
    }

    /// TN #5902's own examples for arbitrary instances, and the rules beside them: axes at their
    /// default are left out, the default point itself keeps `name` ID 6, and values are the
    /// shortest decimal that reads back as the same 16.16 number (noted in review).
    #[test]
    fn a_point_no_instance_names_is_named_by_its_axis_values() {
        // 16 "Andre Var" and no prefix of its own; `wght` and `wdth`, no named instances.
        let base = crate::testing::font_with_names(&[(1, "Andre Var"), (16, "Andre Var")], &['A']).expect("a font with these names");
        let font =
            crate::testing::with_fvar(&base, &[(*b"wght", -10.0, 400.0, 1000.0, 256), (*b"wdth", -10.0, 100.0, 200.0, 257)], &[]).expect("an fvar");
        let f = skrifa::FontRef::new(&font).expect("the fixture parses");
        let name = |wght: f32, wdth: f32| instance_postscript_name(&f, &[(*b"wght", wght), (*b"wdth", wdth)]);
        assert_eq!(name(900.0, 5.5).as_deref(), Some("AndreVar_900wght_5.5wdth"), "the note's example");
        assert_eq!(name(-2.9, -1.4).as_deref(), Some("AndreVar_-2.9wght_-1.4wdth"), "and its negative one");
        assert_eq!(name(900.0, 100.0).as_deref(), Some("AndreVar_900wght"), "an axis at its default is left out");
        assert_eq!(name(400.0, 100.0), None, "the default point is the one name ID 6 names");
        assert_eq!(name(5000.0, 100.0).as_deref(), Some("AndreVar_1000wght"), "a value past the axis is where the font draws it");
        // An axis the caller does not mention is at its default.
        assert_eq!(instance_postscript_name(&f, &[(*b"wdth", 150.0)]).as_deref(), Some("AndreVar_150wdth"));

        // The shortest decimal that comes back as the same fixed value.
        assert_eq!(fixed_decimal(0), "0");
        assert_eq!(fixed_decimal(to_fixed(0.5)), "0.5");
        assert_eq!(fixed_decimal(to_fixed(-2.9)), "-2.9");
        assert_eq!(fixed_decimal(1), "0.00002", "one 65536th");
        assert_eq!(fixed_decimal(to_fixed(100.0)), "100");
        for v in [1, -1, 3, 32768, 65535, 65537, -190054, 0x7fff_ffff, -0x7fff_ffff] {
            let s = fixed_decimal(v);
            assert_eq!((s.parse::<f64>().unwrap() * 65536.0).round() as i64, i64::from(v), "{v} came back from {s}");
            assert!(!s.ends_with('0') || !s.contains('.'), "no trailing zero: {s}");
        }
    }

    /// Too many axes for the limit: TN #5902's last resort, the prefix, an identifier and `...`,
    /// still one name per point.
    #[test]
    fn a_point_too_long_to_describe_gets_the_last_resort_name() {
        let tags: Vec<[u8; 4]> = (0..12u8).map(|i| [b'a', b'x', b'0' + i / 10, b'0' + i % 10]).collect();
        let axes: Vec<_> = tags.iter().map(|t| (*t, 0.0, 0.0, 1000.0, 256)).collect();
        let base = crate::testing::font_with_names(&[(16, "Andre Var"), (25, "AndreVar")], &['A']).expect("names");
        let font = crate::testing::with_fvar(&base, &axes, &[]).expect("an fvar");
        let f = skrifa::FontRef::new(&font).expect("the fixture parses");
        let point = |v: f32| instance_postscript_name(&f, &tags.iter().map(|t| (*t, v)).collect::<Vec<_>>()).expect("a name");
        let (a, b) = (point(123.25), point(456.5));
        for n in [&a, &b] {
            assert!(n.starts_with("AndreVar-") && n.ends_with("..."), "the last resort's shape: {n}");
            assert!(n.chars().count() <= MAX_LEN, "within the limit: {n}");
            assert_eq!(legal(n), *n, "only characters a PostScript name may hold: {n}");
        }
        assert_ne!(a, b, "two points, two names");
    }

    /// `fvar` is font data: an axis whose minimum is past its maximum names a point without a
    /// panic (`f32::clamp` would have panicked).
    #[test]
    fn a_malformed_axis_range_does_not_panic() {
        let base = crate::testing::font_with_names(&[(16, "Odd")], &['A']).expect("names");
        let font = crate::testing::with_fvar(&base, &[(*b"wght", 900.0, 400.0, 100.0, 256)], &[]).expect("an fvar");
        let Ok(f) = skrifa::FontRef::new(&font) else { return };
        let _ = instance_postscript_name(&f, &[(*b"wght", 550.0)]);
        let _ = instance_postscript_name(&f, &[(*b"wght", f32::NAN)]);
    }
}
