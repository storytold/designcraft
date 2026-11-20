//! DesignCraft fonts: the font database (bundled OFL families + user/system fonts), vertical
//! metrics, glyph outlines and OpenType shaping.
//!
//! Shaping here is style-agnostic: [`shape`] turns a string in one face into glyph ids, clusters
//! and advances in font units. `designcraft-compose` applies sizes, tracking, scaling and
//! justification on top.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod features;
mod fontdb;
mod group;
mod kind;
#[cfg(target_os = "macos")]
mod mac_fonts;
pub mod psname;
#[cfg(any(test, feature = "testing"))]
pub mod testing;
mod vertical;

pub use features::{FontFeatures, font_features};
pub use fontdb::{
    DOCUMENT_FONTS_FOLDER, DocumentFonts, FALLBACK_FAMILY, FaceRef, FontDb, FontFace, FontSource, MAX_DOCUMENT_FONT_BYTES, MAX_DOCUMENT_FONT_FILES,
    MAX_DOCUMENT_FONTS_TOTAL, ReloadedFonts, ScopedFonts, base_style, bundled, same_style, sans_by_name, system_font_dirs,
};
pub use group::{FamilyInfo, FontGroup, sort_for_menu};
pub use harfrust::Feature;
use harfrust::{Direction, Language, ShapeOptions, Tag, UnicodeBuffer};
pub use kind::{FaceKind, FontClass, FontFormat, classify, panose_distance};
pub use kurbo::BezPath;
use skrifa::MetadataProvider;
use skrifa::instance::Size;

/// A font from the optional craft-fonts build input (https://github.com/storytold/craft-fonts;
/// empty unless built with `CRAFT_FONTS_DIR`, see `build.rs`).
pub struct CraftFont {
    pub family: &'static str,
    pub style: &'static str,
    /// ISO 15924 scripts the font is for, e.g. `"Jpan"`.
    pub scripts: &'static [&'static str],
    pub bytes: &'static [u8],
}

include!(concat!(env!("OUT_DIR"), "/craft_fonts.rs"));

/// The craft-fonts faces for Japanese (`"Jpan"`), in manifest order. Empty without craft-fonts.
pub fn japanese_fonts() -> impl Iterator<Item = &'static CraftFont> {
    CRAFT_FONTS.iter().filter(|f| f.scripts.contains(&"Jpan"))
}

/// Japanese faces in document-fallback order: Mincho (serif) families first, matching the
/// serif default text font, then the others; Regular before other styles.
pub fn japanese_document_fonts() -> Vec<&'static CraftFont> {
    let mut v: Vec<_> = japanese_fonts().collect();
    v.sort_by_key(|f| (!f.family.contains("Mincho"), f.style != "Regular"));
    v
}

/// Japanese faces in UI order: BIZ UDPGothic first (the UI face), then the others; `bold` puts
/// bold styles before regular ones.
pub fn japanese_ui_fonts(bold: bool) -> Vec<&'static CraftFont> {
    let mut v: Vec<_> = japanese_fonts().collect();
    v.sort_by_key(|f| (f.family != "BIZ UDPGothic", (f.style == "Bold") != bold));
    v
}

/// InDesign's default text font is a serif; ours is Source Serif 4.
pub const DEFAULT_FAMILY: &str = "Source Serif 4";

/// One shaped glyph, in font units (y up).
///
/// Horizontal runs ([`shape`]) advance by `x_advance` (`y_advance` is 0) and the offsets move the
/// glyph from its pen position. Vertical runs ([`shape_vertical`]) advance down the line by
/// `-y_advance` (`x_advance` is 0) and the offsets move the glyph from its vertical origin
/// ([`FontFace::v_origin`]) on the line's centre, as `vpal` or `vkrn` place it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShapedGlyph {
    pub gid: u32,
    /// Byte offset (in the shaped string) of the cluster this glyph belongs to.
    pub cluster: usize,
    pub x_advance: i32,
    pub y_advance: i32,
    pub x_offset: i32,
    pub y_offset: i32,
    pub safe_tatweel_before: bool,
    /// Direction used for shaping, including Unicode mirroring.
    pub rtl: bool,
}

/// Line metrics of the font at `index` of `data`, in font units: (units per em, ascent, descent
/// (negative below the baseline), line gap) — what text layouts such as egui's build lines from.
/// `None` if the font can't be read.
pub fn line_metrics(data: &[u8], index: u32) -> Option<(f32, f32, f32, f32)> {
    use skrifa::MetadataProvider;
    let f = skrifa::FontRef::from_index(data, index).ok()?;
    let m = f.metrics(skrifa::instance::Size::unscaled(), skrifa::instance::LocationRef::default());
    Some((f32::from(m.units_per_em.max(1)), m.ascent, m.descent, m.leading))
}

/// An OpenType feature setting: `"liga"`, `"-kern"`, `"ss01"`.
pub fn feature(tag: &str) -> Option<Feature> {
    let (on, t) = match tag.strip_prefix('-') {
        Some(r) => (false, r),
        None => (true, tag.strip_prefix('+').unwrap_or(tag)),
    };
    let b = t.as_bytes();
    if b.len() != 4 {
        return None;
    }
    Some(Feature::new(Tag::new(&[b[0], b[1], b[2], b[3]]), on as u32, ..))
}

/// A strong right-to-left character (Hebrew, Arabic, …)?
pub fn is_rtl(c: char) -> bool {
    use unicode_bidi::BidiClass::{AL, R};
    matches!(unicode_bidi::bidi_class(c), R | AL)
}

/// Resolve directional and script runs before choosing the OpenType shaper.
fn direction_runs(text: &str) -> Vec<(std::ops::Range<usize>, bool, unicode_script::Script)> {
    use unicode_script::{Script, UnicodeScript};
    let info = unicode_bidi::BidiInfo::new(text, None);
    let mut runs = Vec::new();
    let mut script =
        text.chars().map(|c| c.script()).find(|s| !matches!(s, Script::Common | Script::Inherited | Script::Unknown)).unwrap_or(Script::Common);
    let mut start = 0;
    let mut rtl = info.levels.first().is_some_and(|l| l.is_rtl());
    for (i, c) in text.char_indices() {
        let next_rtl = info.levels.get(i).is_some_and(|l| l.is_rtl());
        let next_script = match c.script() {
            Script::Common | Script::Inherited | Script::Unknown => script,
            s => s,
        };
        if i > start && (next_rtl != rtl || next_script != script) {
            runs.push((start..i, rtl, script));
            start = i;
        }
        rtl = next_rtl;
        script = next_script;
    }
    if start < text.len() {
        runs.push((start..text.len(), rtl, script));
    }
    runs
}

/// What surrounds a run being shaped: the text before and after it (so Arabic letters join across a
/// change of style) and its language, a BCP 47 tag such as `tr` or `zh-Hant` that picks the font's
/// localized forms (OpenType `locl`; `None` or a tag the shaper can't read: the font's defaults).
#[derive(Clone, Copy, Default)]
pub struct ShapeContext<'a> {
    pub before: &'a str,
    pub after: &'a str,
    pub language: Option<&'a str>,
}

/// Shape `text` with `face`. `chars` lets callers substitute characters (e.g. uppercase for All
/// Caps) while keeping clusters pointing into the original string. Glyphs come in logical
/// order: right-to-left runs are shaped right to left, then their clusters put back in text
/// order (each cluster's glyphs keep the shaper's order); line layout reorders them visually.
pub fn shape(face: &FontFace, text: &str, features: &[Feature], map: impl Fn(char) -> char) -> Vec<ShapedGlyph> {
    shape_with_context(face, text, features, map, ShapeContext::default())
}

/// [`shape`] `text` in its `context`: its language and the text around it.
pub fn shape_with_context(
    face: &FontFace,
    text: &str,
    features: &[Feature],
    map: impl Fn(char) -> char,
    context: ShapeContext<'_>,
) -> Vec<ShapedGlyph> {
    let mut out = Vec::with_capacity(text.len());
    for (r, rtl, script) in direction_runs(text) {
        let local = ShapeContext {
            before: if r.start == 0 { context.before } else { &text[..r.start] },
            after: if r.end == text.len() { context.after } else { &text[r.end..] },
            language: context.language,
        };
        let mut g = shape_dir(face, &text[r.clone()], features, &map, rtl, script, local);
        for x in &mut g {
            x.cluster += r.start;
        }
        if rtl {
            // Visual (clusters descending) → logical, cluster by cluster.
            let mut groups: Vec<Vec<ShapedGlyph>> = Vec::new();
            for x in g {
                match groups.last_mut() {
                    Some(last) if last[0].cluster == x.cluster => last.push(x),
                    _ => groups.push(vec![x]),
                }
            }
            groups.sort_by_key(|grp| grp[0].cluster);
            g = groups.into_iter().flatten().collect();
        }
        out.extend(g);
    }
    out
}

/// `features` as a run in `script` takes them. Contextual alternates are part of how Arabic joins
/// (connecting strokes, tatweel and letter variants): InDesign keeps them in Arabic text with
/// Contextual Alternates off, so `calt` off is dropped there.
fn script_features(features: &[Feature], script: unicode_script::Script) -> std::borrow::Cow<'_, [Feature]> {
    use unicode_script::Script;
    let calt = Tag::new(b"calt");
    let off = |f: &Feature| f.tag == calt && f.value == 0;
    if script == Script::Arabic && features.iter().any(off) {
        features.iter().filter(|f| !off(f)).copied().collect::<Vec<_>>().into()
    } else {
        features.into()
    }
}

fn shape_dir(
    face: &FontFace,
    text: &str,
    features: &[Feature],
    map: &impl Fn(char) -> char,
    rtl: bool,
    script: unicode_script::Script,
    context: ShapeContext<'_>,
) -> Vec<ShapedGlyph> {
    let (mut out, shaped) = shape_plain(face, text, features, map, rtl, script, context);
    if shaped {
        presentation_ligatures(face, text, &script_features(features, script), map, rtl, script, &mut out);
    }
    out
}

/// What the shaper gives for `text`, without the presentation-form ligatures of
/// [`presentation_ligatures`] (the probes that measure kerning shape this way, so their letters stay
/// letters), and whether the shaper could run; where it could not, the glyphs are the font's own,
/// one per character.
fn shape_plain(
    face: &FontFace,
    text: &str,
    features: &[Feature],
    map: &impl Fn(char) -> char,
    rtl: bool,
    script: unicode_script::Script,
    context: ShapeContext<'_>,
) -> (Vec<ShapedGlyph>, bool) {
    let mut out = Vec::with_capacity(text.len());
    let features = script_features(features, script);
    let features = &features[..];
    let shaped = face.hb().map(|hb| {
        let shaper = face.shaper.shaper(&hb).instance(face.instance.as_ref()).build();
        let mut buf = UnicodeBuffer::new();
        for (i, c) in text.char_indices() {
            buf.add(map(c), i as u32);
        }
        buf.set_pre_context(context.before);
        buf.set_post_context(context.after);
        if let Ok(script) = script.short_name().parse() {
            buf.set_script(script);
        }
        if let Some(language) = context.language.and_then(|t| t.parse::<Language>().ok()) {
            buf.set_language(language);
        }
        buf.set_flags(harfrust::BufferFlags::PRODUCE_SAFE_TO_INSERT_TATWEEL);
        buf.guess_segment_properties();
        buf.set_direction(if rtl { Direction::RightToLeft } else { Direction::LeftToRight });
        let gb = shaper.shape(buf, ShapeOptions::new().features(features));
        for (info, pos) in gb.glyph_infos().iter().zip(gb.glyph_positions()) {
            out.push(ShapedGlyph {
                gid: info.glyph_id,
                cluster: info.cluster as usize,
                x_advance: pos.x_advance,
                y_advance: 0,
                x_offset: pos.x_offset,
                y_offset: pos.y_offset,
                safe_tatweel_before: info.safe_to_insert_tatweel(),
                rtl,
            });
        }
    });
    if shaped.is_none()
        && let Some(f) = face.skrifa()
    {
        let cmap = f.charmap();
        let gm = f.glyph_metrics(Size::unscaled(), face.location());
        for (i, c) in text.char_indices() {
            let g = cmap.map(map(c)).unwrap_or_default();
            let adv = gm.advance_width(g).unwrap_or(face.upem as f32 * 0.5);
            out.push(ShapedGlyph {
                gid: g.to_u32(),
                cluster: i,
                x_advance: adv.round() as i32,
                y_advance: 0,
                x_offset: 0,
                y_offset: 0,
                safe_tatweel_before: false,
                rtl: false,
            });
        }
    }
    (out, shaped.is_some())
}

/// The standard ligatures a font has as presentation-form characters, longest first: the letters
/// and the character whose glyph is the ligature.
const PRESENTATION_LIGATURES: [(&str, char); 5] =
    [("ffi", '\u{FB03}'), ("ffl", '\u{FB04}'), ("ff", '\u{FB00}'), ("fi", '\u{FB01}'), ("fl", '\u{FB02}')];

/// InDesign sets the standard ligatures (ff, fi, fl, ffi, ffl) in Latin text, with Ligatures on,
/// from the font's presentation-form glyphs (U+FB00 to U+FB04) when the font's own `liga` leaves
/// them as letters: Georgia and Verdana have no `liga` at all, and the `liga` of Times New Roman
/// and Arial is not Latin. Where the shaper already ligated, the letters are gone and this finds
/// nothing. Left-to-right Latin runs only, and not with `liga` off (the same switch turns off the
/// shaper's own).
fn presentation_ligatures(
    face: &FontFace,
    text: &str,
    features: &[Feature],
    map: &dyn Fn(char) -> char,
    rtl: bool,
    script: unicode_script::Script,
    glyphs: &mut Vec<ShapedGlyph>,
) {
    let liga = Tag::new(b"liga");
    if rtl || script != unicode_script::Script::Latin || features.iter().any(|f| f.tag == liga && f.value == 0) {
        return;
    }
    let Some(font) = face.skrifa() else { return };
    let cmap = font.charmap();
    let metrics = font.glyph_metrics(Size::unscaled(), face.location());
    let advance = |g: u32| Some(metrics.advance_width(skrifa::GlyphId::new(g))?.round() as i32);
    merge_presentation_ligatures(
        text,
        glyphs,
        map,
        |c| cmap.map(c).map(|g| g.to_u32()).filter(|&g| g != 0),
        advance,
        // What the shaper's kerning of two characters adds to the first one's advance. The probe is
        // shaped without this pass: a neighbour that maps to an `f` would otherwise make "ff" a
        // ligature here and the probe would find no pair.
        |pair| match shape_plain(face, pair, features, &|c| c, false, script, ShapeContext::default()).0.as_slice() {
            [first, _] => Some(first.x_advance - advance(first.gid)?),
            _ => None,
        },
    );
}

/// Replace each run of glyphs for the letters of a [`PRESENTATION_LIGATURES`] entry, taking the
/// longest entry whose character the font has (`glyph_of` gives a character's glyph), with that
/// character's glyph and its `advance`. The ligature takes the first letter's cluster, as the
/// shaper's own ligatures do. A ligature is substituted before kerning, so the glyph before it is
/// kerned against the ligature, and the ligature against the glyph after it, rather than the letters
/// against theirs (`pair_kern` gives what shaping two characters kerns the first by): the glyph before
/// trades its kerning against the first letter for its kerning against the ligature, and the ligature
/// takes the kerning against what follows. The glyph before keeps its other adjustments, such as the
/// second value of the pair before it. A run counts only if it is the letters themselves of `text`, one glyph each in consecutive
/// clusters, and nothing else belongs to the run's last cluster (a mark after the last letter would
/// be left placed for a glyph that is gone): a joiner between them or a stylistic alternate leaves
/// them as they are. The letters' own kerning (some fonts give it as an offset of the second letter)
/// goes with them.
fn merge_presentation_ligatures(
    text: &str,
    glyphs: &mut Vec<ShapedGlyph>,
    map: &dyn Fn(char) -> char,
    glyph_of: impl Fn(char) -> Option<u32>,
    advance: impl Fn(u32) -> Option<i32>,
    pair_kern: impl Fn(&str) -> Option<i32>,
) {
    let table: Vec<(&str, Vec<u32>, u32, char)> = PRESENTATION_LIGATURES
        .iter()
        .filter_map(|&(letters, lig)| Some((letters, letters.chars().map(&glyph_of).collect::<Option<Vec<_>>>()?, glyph_of(lig)?, lig)))
        .collect();
    if table.is_empty() {
        return;
    }
    let mut out: Vec<ShapedGlyph> = Vec::with_capacity(glyphs.len());
    // The ligature just made, as the story byte it ends at and its character.
    let mut last_lig: Option<(usize, char)> = None;
    let mut i = 0;
    while i < glyphs.len() {
        let hit = table.iter().find(|(letters, base, _, _)| {
            let run = glyphs.get(i..i + base.len());
            run.is_some_and(|run| {
                let start = run[0].cluster;
                text.get(start..start + letters.len()) == Some(*letters)
                    && run.iter().zip(base).enumerate().all(|(k, (g, &b))| g.gid == b && g.cluster == start + k)
                    && glyphs.get(i + base.len()).is_none_or(|next| next.cluster != start + base.len() - 1)
            })
        });
        match hit {
            Some((letters, base, lig, lig_char)) => {
                let first = glyphs[i];
                let end = first.cluster + letters.len();
                if let Some(prev) = out.last_mut() {
                    if let Some((_, prev_lig)) = last_lig.filter(|&(prev_end, _)| prev_end == first.cluster) {
                        // The ligature made just before: it was kerned against the first letter of this
                        // run, and is kerned against this ligature instead.
                        let with_f = pair_kern(&format!("{prev_lig}f")).unwrap_or(0);
                        let with_lig = pair_kern(&format!("{prev_lig}{lig_char}")).unwrap_or(0);
                        prev.x_advance += with_lig - with_f;
                    } else {
                        // The previous glyph, when it is the one character just before the first letter:
                        // its kerning against f gives way to its kerning against the ligature.
                        let before = text.get(prev.cluster..).and_then(|t| t.chars().next());
                        if let Some(c) = before.filter(|c| prev.cluster + c.len_utf8() == first.cluster).map(map)
                            && let Some(with_f) = pair_kern(&format!("{c}f"))
                        {
                            let with_lig = pair_kern(&format!("{c}{lig_char}")).unwrap_or(0);
                            prev.x_advance += with_lig - with_f;
                        }
                    }
                }
                // The ligature is kerned against the character after it.
                let after = text.get(end..).and_then(|t| t.chars().next()).map(map).and_then(|c| pair_kern(&format!("{lig_char}{c}"))).unwrap_or(0);
                out.push(ShapedGlyph {
                    gid: *lig,
                    x_advance: advance(*lig).map_or(first.x_advance, |a| a + after),
                    x_offset: 0,
                    y_offset: 0,
                    ..first
                });
                last_lig = Some((end, *lig_char));
                i += base.len();
            }
            None => {
                out.push(glyphs[i]);
                last_lig = None;
                i += 1;
            }
        }
    }
    *glyphs = out;
}

/// Shape upright text top to bottom (`text`, `features`, `map` and `context` as in
/// [`shape_with_context`]): the shaper applies `vert` (vertical forms), and `vkrn` and `vpal` when
/// `features` turn them on. Advances and origins are the face's vertical metrics
/// ([`FontFace::v_advance`], [`FontFace::v_origin`]).
pub fn shape_vertical(face: &FontFace, text: &str, features: &[Feature], map: impl Fn(char) -> char, context: ShapeContext<'_>) -> Vec<ShapedGlyph> {
    let mut funcs = vertical::VerticalFuncs { face, metrics: vertical::VMetrics::new(face) };
    let mut out = Vec::with_capacity(text.len());
    let mut origins = Vec::with_capacity(text.len());
    let shaped = face.hb().map(|hb| {
        let shaper = face.shaper.shaper(&hb).instance(face.instance.as_ref()).build();
        let mut buf = UnicodeBuffer::new();
        for (i, c) in text.char_indices() {
            buf.add(map(c), i as u32);
        }
        buf.set_pre_context(context.before);
        buf.set_post_context(context.after);
        if let Some(language) = context.language.and_then(|t| t.parse::<Language>().ok()) {
            buf.set_language(language);
        }
        buf.guess_segment_properties();
        buf.set_direction(Direction::TopToBottom);
        let gb = shaper.shape(buf, ShapeOptions::new().features(features).font_funcs(Some(&mut funcs)));
        for (info, pos) in gb.glyph_infos().iter().zip(gb.glyph_positions()) {
            out.push(ShapedGlyph {
                gid: info.glyph_id,
                cluster: info.cluster as usize,
                x_advance: pos.x_advance,
                y_advance: pos.y_advance,
                x_offset: pos.x_offset,
                y_offset: pos.y_offset,
                safe_tatweel_before: false,
                rtl: false,
            });
        }
    });
    if shaped.is_none()
        && let Some(f) = face.skrifa()
    {
        let cmap = f.charmap();
        for (i, c) in text.char_indices() {
            let gid = cmap.map(map(c)).unwrap_or_default().to_u32();
            let (x, y) = funcs.origin(gid);
            let adv = -(face.v_advance(gid).round() as i32);
            out.push(ShapedGlyph {
                gid,
                cluster: i,
                x_advance: 0,
                y_advance: adv,
                x_offset: -x,
                y_offset: -y,
                safe_tatweel_before: false,
                rtl: false,
            });
        }
    }
    // The shaper hangs each glyph from its vertical origin; keep only what GPOS moved it by.
    origins.extend(out.iter().map(|g| funcs.origin(g.gid)));
    for (g, (x, y)) in out.iter_mut().zip(origins) {
        g.x_offset = g.x_offset.saturating_add(x);
        g.y_offset = g.y_offset.saturating_add(y);
    }
    out
}

/// Glyph id of the first of `chars` the face has (0 = .notdef).
pub fn first_glyph(face: &FontFace, chars: &[char]) -> u32 {
    chars.iter().map(|c| face.glyph_for(*c)).find(|g| *g != 0).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arabic_keeps_contextual_alternates() {
        use unicode_script::Script;
        let off = feature("-calt").unwrap();
        let liga = feature("-liga").unwrap();
        let tags = |s: Script| script_features(&[off, liga], s).iter().map(|f| (f.tag, f.value)).collect::<Vec<_>>();
        assert_eq!(tags(Script::Arabic), vec![(liga.tag, 0)]);
        assert_eq!(tags(Script::Latin), vec![(off.tag, 0), (liga.tag, 0)]);
    }

    #[test]
    fn arabic_script_runs_do_not_inherit_the_hebrew_shaper() {
        use unicode_script::Script;
        let text = "אב بب";
        let runs = direction_runs(text);
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0], (0..5, true, Script::Hebrew));
        assert_eq!(runs[1], (5..text.len(), true, Script::Arabic));
    }

    #[test]
    fn last_resort_face_parses() {
        let f = fontdb::last_resort_face();
        assert_eq!(f.family, fontdb::FALLBACK_FAMILY);
        assert!(f.upem > 0.0);
    }

    #[test]
    fn bundled_families_load() {
        let db = FontDb::global();
        let fams = db.families();
        assert!(fams.iter().any(|f| f == DEFAULT_FAMILY), "{fams:?}");
        assert!(fams.iter().any(|f| f == "Source Sans 3"));
        let styles = db.styles(DEFAULT_FAMILY);
        for s in ["Regular", "Italic", "Bold", "Semibold"] {
            assert!(styles.iter().any(|x| x == s), "{s} in {styles:?}");
        }
    }

    #[test]
    fn style_linking_finds_bold_and_italic_in_the_family() {
        let db = FontDb::with_font_dirs(vec![]);
        let link = |cur: &str, bold: Option<bool>, italic: Option<bool>| db.linked_style("Source Serif 4", cur, bold, italic);
        assert_eq!(link("Regular", Some(true), None).as_deref(), Some("Bold"));
        assert_eq!(link("Italic", Some(true), None).as_deref(), Some("Bold Italic"), "keeps the italic");
        assert_eq!(link("Bold", None, Some(true)).as_deref(), Some("Bold Italic"), "keeps the weight");
        assert_eq!(link("Bold Italic", Some(false), None).as_deref(), Some("Italic"));
        assert_eq!(link("Bold Italic", None, Some(false)).as_deref(), Some("Bold"));
        assert_eq!(link("Semibold", Some(false), None).as_deref(), Some("Regular"));
        assert_eq!(db.traits_of("Source Serif 4", "Bold Italic"), (700.0, true));
        // Source Sans 3 is bundled without a bold italic.
        assert_eq!(db.linked_style("Source Sans 3", "Italic", Some(true), None), None);
        assert_eq!(db.linked_style("No Such Family", "Regular", Some(true), None), None);
    }

    #[test]
    fn shaping_produces_clusters_and_advances() {
        let face = FontDb::global().face(DEFAULT_FAMILY, "Regular");
        let g = shape(&face, "Hello", &[], |c| c);
        assert_eq!(g.len(), 5);
        assert_eq!(g.iter().map(|g| g.cluster).collect::<Vec<_>>(), vec![0, 1, 2, 3, 4]);
        assert!(g.iter().all(|g| g.x_advance > 0 && g.gid != 0));
    }

    #[test]
    fn ligatures_can_be_disabled() {
        let face = FontDb::global().face(DEFAULT_FAMILY, "Regular");
        let on = shape(&face, "office", &[], |c| c);
        let off = shape(&face, "office", &[feature("-liga").unwrap()], |c| c);
        assert!(on.len() <= off.len());
        assert_eq!(off.len(), 6);
    }

    /// Glyph ids of a fake font: the letters of "office fine flow" and the presentation forms.
    fn fake_glyph(c: char) -> Option<u32> {
        Some(match c {
            'o' => 1,
            'c' => 2,
            'e' => 3,
            'n' => 4,
            't' => 5,
            'f' => 10,
            'i' => 11,
            'l' => 12,
            '\u{FB00}' => 20,
            '\u{FB01}' => 21,
            '\u{FB02}' => 22,
            '\u{FB03}' => 23,
            '\u{FB04}' => 24,
            _ => return None,
        })
    }

    /// The glyphs the shaper leaves for `text` in the fake font: one per letter, 600 units wide.
    fn fake_glyphs(text: &str) -> Vec<ShapedGlyph> {
        text.char_indices()
            .map(|(i, c)| ShapedGlyph {
                gid: fake_glyph(c).unwrap_or(99),
                cluster: i,
                x_advance: 600,
                y_advance: 0,
                x_offset: 0,
                y_offset: 0,
                safe_tatweel_before: false,
                rtl: false,
            })
            .collect()
    }

    /// A letter is 600 units wide in the fake font, a ligature 500 plus its glyph id.
    fn fake_advance(gid: u32) -> Option<i32> {
        Some(if gid >= 20 { 500 + gid as i32 } else { 600 })
    }

    /// The fake font kerns nothing.
    fn fake_kern(_: &str) -> Option<i32> {
        None
    }

    fn merged(text: &str, has: impl Fn(char) -> bool) -> Vec<(u32, usize, i32)> {
        let mut g = fake_glyphs(text);
        merge_presentation_ligatures(text, &mut g, &|c| c, |c| fake_glyph(c).filter(|_| has(c) || c.is_ascii()), fake_advance, fake_kern);
        g.iter().map(|g| (g.gid, g.cluster, g.x_advance)).collect()
    }

    #[test]
    fn a_font_with_all_five_presentation_forms_sets_them_longest_first() {
        // "office": o, ffi (one glyph at the first f's cluster), c, e.
        assert_eq!(merged("office", |_| true), vec![(1, 0, 600), (23, 1, 523), (2, 4, 600), (3, 5, 600)]);
        assert_eq!(merged("ffl", |_| true), vec![(24, 0, 524)]);
        assert_eq!(merged("off", |_| true), vec![(1, 0, 600), (20, 1, 520)]);
        assert_eq!(merged("fine", |_| true), vec![(21, 0, 521), (4, 2, 600), (3, 3, 600)]);
        assert_eq!(merged("flow", |_| true)[0], (22, 0, 522));
    }

    #[test]
    fn a_font_with_only_fi_and_fl_sets_those_and_leaves_ff_as_letters() {
        // Times New Roman, Arial and Verdana: "office" is o, f, fi, c, e.
        let only = |c: char| matches!(c, '\u{FB01}' | '\u{FB02}');
        assert_eq!(merged("office", only), vec![(1, 0, 600), (10, 1, 600), (21, 2, 521), (2, 4, 600), (3, 5, 600)]);
        assert_eq!(merged("ff", only).len(), 2);
    }

    #[test]
    fn the_glyph_before_a_ligature_loses_its_kerning_against_the_first_letter() {
        // Times-like font: "ffi" is f, fi. The shaper kerned the first f 40 units against the second;
        // the second is now part of the ligature, which the font does not kern, so the first f is
        // back at its own advance. A letter before a ligature that was never kerned is unchanged.
        let text = "offi";
        let mut g = fake_glyphs(text);
        g[1].x_advance = 560;
        merge_presentation_ligatures(
            text,
            &mut g,
            &|c| c,
            |c| fake_glyph(c).filter(|_| c.is_ascii() || matches!(c, '\u{FB01}' | '\u{FB02}')),
            fake_advance,
            // The shaper kerned f against the f after it by -40.
            |pair| (pair == "ff").then_some(-40),
        );
        assert_eq!(g.iter().map(|g| (g.gid, g.x_advance)).collect::<Vec<_>>(), vec![(1, 600), (10, 600), (21, 521)]);
        // A kerned pair that does not become a ligature keeps its kerning.
        let mut g = fake_glyphs("ft");
        g[0].x_advance = 560;
        merge_presentation_ligatures("ft", &mut g, &|c| c, fake_glyph, fake_advance, |_| Some(-40));
        assert_eq!(g[0].x_advance, 560);
    }

    #[test]
    fn the_glyph_before_a_ligature_keeps_the_adjustments_of_its_own_pair() {
        // "AVfi": the A/V pair took 30 units off V's advance (the second value of the pair), and V/f
        // took 20 more. Only the 20 goes with the f that became part of the ligature.
        let text = "AVfi";
        let mut g = fake_glyphs(text);
        g[1].x_advance = 600 - 30 - 20;
        merge_presentation_ligatures(
            text,
            &mut g,
            &|c| c,
            |c| fake_glyph(c).filter(|_| c.is_ascii() || c == '\u{FB01}'),
            fake_advance,
            |pair| (pair == "Vf").then_some(-20),
        );
        assert_eq!(g.iter().map(|g| (g.gid, g.x_advance)).collect::<Vec<_>>(), vec![(99, 600), (99, 570), (21, 521)]);
        // A glyph that is not the single character just before the first letter is left alone.
        let mut g = fake_glyphs("ofi");
        g[0].x_advance = 580;
        merge_presentation_ligatures("ofi", &mut g, &|c| c, fake_glyph, fake_advance, |pair| (pair == "of").then_some(-40));
        assert_eq!(g[0].x_advance, 620, "the character right before the ligature");
        let text = "of\u{0301}fi";
        let mut g = fake_glyphs(text);
        g.remove(2);
        g[1].x_advance = 580;
        let before = g[1].x_advance;
        merge_presentation_ligatures(text, &mut g, &|c| c, fake_glyph, fake_advance, |_| Some(-40));
        assert_eq!(g[1].x_advance, before, "a mark stands between f and the ligature: not the pair");
    }

    #[test]
    fn a_ligature_is_kerned_against_its_neighbours_not_the_letters_against_theirs() {
        // "Vfi.": V was kerned -20 against f; against the fi glyph the font kerns it -15. The ligature
        // is kerned -25 against the full stop, which was the i's kerning to it.
        let text = "Vfi.";
        let mut g = fake_glyphs(text);
        g[0].x_advance = 600 - 20;
        g[2].x_advance = 600 - 25;
        merge_presentation_ligatures(
            text,
            &mut g,
            &|c| c,
            |c| fake_glyph(c).filter(|_| c.is_ascii() || c == '\u{FB01}'),
            fake_advance,
            |pair| match pair {
                "Vf" => Some(-20),
                "V\u{FB01}" => Some(-15),
                "\u{FB01}." => Some(-25),
                _ => None,
            },
        );
        assert_eq!(g.iter().map(|g| (g.gid, g.x_advance)).collect::<Vec<_>>(), vec![(99, 585), (21, 521 - 25), (99, 600)]);
    }

    #[test]
    fn a_ligature_next_to_a_ligature_is_kerned_against_it_not_against_the_f() {
        // "fifi": the first fi was kerned -25 against the f after it; against the second fi the
        // font kerns it -10. The second fi has no kerning against what follows.
        let text = "fifi";
        let mut g = fake_glyphs(text);
        g[1].x_advance = 600 - 25;
        merge_presentation_ligatures(
            text,
            &mut g,
            &|c| c,
            |c| fake_glyph(c).filter(|_| c.is_ascii() || c == '\u{FB01}'),
            fake_advance,
            |pair| match pair {
                "\u{FB01}f" => Some(-25),
                "\u{FB01}\u{FB01}" => Some(-10),
                _ => None,
            },
        );
        assert_eq!(g.iter().map(|g| (g.gid, g.x_advance)).collect::<Vec<_>>(), vec![(21, 521 - 10), (21, 521)]);
    }

    #[test]
    fn the_neighbours_of_a_ligature_are_kerned_as_the_shaper_saw_them_after_the_character_mapping() {
        // The mapping turns V into A and leaves f and i alone; the glyph for the V of the text is the
        // one for an A, so the pairs are the ligature with A and A with f, never with V.
        let map = |c: char| if c == 'V' { 'A' } else { c };
        let has = |c: char| fake_glyph(c).filter(|_| c.is_ascii() || c == '\u{FB01}');
        let kern = |pair: &str| match pair {
            "\u{FB01}A" => Some(-30),
            "Af" => Some(-20),
            "A\u{FB01}" => Some(-15),
            _ => Some(-99),
        };
        let text = "fiV";
        let mut g = fake_glyphs(text);
        merge_presentation_ligatures(text, &mut g, &map, has, fake_advance, kern);
        assert_eq!(g.iter().map(|g| (g.gid, g.x_advance)).collect::<Vec<_>>(), vec![(21, 521 - 30), (99, 600)]);
        let text = "Vfi";
        let mut g = fake_glyphs(text);
        g[0].x_advance = 600 - 20;
        merge_presentation_ligatures(text, &mut g, &map, has, fake_advance, kern);
        assert_eq!(g.iter().map(|g| (g.gid, g.x_advance)).collect::<Vec<_>>(), vec![(99, 600 - 20 + (-15 + 20)), (21, 521)]);
    }

    #[test]
    fn a_run_is_not_merged_when_a_mark_still_belongs_to_its_last_cluster() {
        // "ff" and a combining mark: the shaper gives f@0, f@1 and the mark in the second f's cluster.
        let text = "ff\u{0331}";
        let glyphs = || {
            let mut g = fake_glyphs("ff");
            g.push(ShapedGlyph { gid: 50, cluster: 1, ..g[1] });
            g
        };
        let mut g = glyphs();
        merge_presentation_ligatures(text, &mut g, &|c| c, fake_glyph, fake_advance, fake_kern);
        assert_eq!(g.iter().map(|g| g.gid).collect::<Vec<_>>(), vec![10, 10, 50], "the mark stays with its letter");
        // Plain "ff" is still a ligature, and so is one followed by a letter of its own cluster.
        let mut plain = fake_glyphs("ff");
        merge_presentation_ligatures("ff", &mut plain, &|c| c, fake_glyph, fake_advance, fake_kern);
        assert_eq!(plain.iter().map(|g| g.gid).collect::<Vec<_>>(), vec![20]);
        let mut more = fake_glyphs("ffe");
        merge_presentation_ligatures("ffe", &mut more, &|c| c, fake_glyph, fake_advance, fake_kern);
        assert_eq!(more.iter().map(|g| g.gid).collect::<Vec<_>>(), vec![20, 3]);
    }

    #[test]
    fn other_f_pairs_and_other_letters_stay_letters() {
        // ft and fj are not standard ligatures; a font without any presentation form changes nothing.
        assert_eq!(merged("ft", |_| true).len(), 2);
        assert_eq!(merged("fine", |_| false).len(), 4);
        assert_eq!(merged("fine", |_| false)[0], (10, 0, 600));
    }

    #[test]
    fn a_ligature_is_not_set_across_a_joiner_or_for_other_letters() {
        // A zero width non-joiner between the letters (the shaper keeps it as a glyph or drops it).
        let text = "f\u{200c}i";
        let mut g = fake_glyphs(text);
        g.remove(1);
        merge_presentation_ligatures(text, &mut g, &|c| c, fake_glyph, |_| Some(500), fake_kern);
        assert_eq!(g.len(), 2, "clusters 0 and 4 are not adjacent");
        // A kerning offset on a letter (the second f's, in Times New Roman) goes with the letters.
        let mut g = fake_glyphs("ffi");
        g[1].x_offset = -18;
        merge_presentation_ligatures("ffi", &mut g, &|c| c, |c| fake_glyph(c).filter(|_| c.is_ascii() || c == '\u{FB01}'), fake_advance, fake_kern);
        assert_eq!(g.iter().map(|g| (g.gid, g.x_offset)).collect::<Vec<_>>(), vec![(10, 0), (21, 0)]);
        // Glyphs that are the right ones for other letters (a stylistic alternate of "fi", say).
        let mut g = fake_glyphs("FI");
        g[0].gid = 10;
        g[1].gid = 11;
        merge_presentation_ligatures("FI", &mut g, &|c| c, fake_glyph, |_| Some(500), fake_kern);
        assert_eq!(g.len(), 2, "the text is not the letters f and i");
    }

    #[test]
    fn fonts_that_ligate_themselves_are_left_alone_and_liga_off_turns_the_fallback_off() {
        // Source Serif 4's own `liga` makes one glyph of "fi": the fallback finds no letters.
        let face = FontDb::global().face(DEFAULT_FAMILY, "Regular");
        let on = shape(&face, "office fine flow", &[], |c| c);
        let off = shape(&face, "office fine flow", &[feature("-liga").unwrap()], |c| c);
        assert_eq!(off.len(), 16);
        assert!(on.len() < off.len());
        let mut again = on.clone();
        merge_presentation_ligatures(
            "office fine flow",
            &mut again,
            &|c| c,
            |c| face.skrifa().and_then(|f| f.charmap().map(c)).map(|g| g.to_u32()),
            |_| Some(0),
            fake_kern,
        );
        assert_eq!(again, on, "letters were already ligated by the font's liga");
    }

    #[test]
    fn language_picks_localized_forms() {
        // Source Serif 4's `locl` has a Turkish `i`.
        let face = FontDb::global().face(DEFAULT_FAMILY, "Regular");
        let gid = |language| shape_with_context(&face, "i", &[], |c| c, ShapeContext { language, ..Default::default() })[0].gid;
        assert_eq!(gid(None), face.glyph_for('i'));
        assert_eq!(gid(Some("en-US")), face.glyph_for('i'));
        assert_ne!(gid(Some("tr")), face.glyph_for('i'));
        assert_eq!(gid(Some("not a tag")), face.glyph_for('i'));
    }

    #[test]
    fn mapping_keeps_clusters() {
        let face = FontDb::global().face(DEFAULT_FAMILY, "Regular");
        let g = shape(&face, "ab", &[], |c| c.to_ascii_uppercase());
        assert_eq!(g[0].gid, face.glyph_for('A'));
        assert_eq!(g[1].cluster, 1);
    }

    #[test]
    fn variable_font_named_instances_are_styles() {
        // Uses a variable system font when one is installed (macOS ships several).
        let Some(data) =
            ["/System/Library/Fonts/Supplemental/Skia.ttf", "/System/Library/Fonts/NewYork.ttf", "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"]
                .iter()
                .find_map(|p| std::fs::read(p).ok())
        else {
            return;
        };
        let db = FontDb::global();
        db.add_font(data.clone());
        let Some(fam) = skrifa::FontRef::new(&data).ok().and_then(|f| {
            (f.named_instances().len() > 1)
                .then(|| f.localized_strings(skrifa::string::StringId::FAMILY_NAME).english_or_first().map(|s| s.to_string()))?
        }) else {
            return;
        };
        let styles = db.styles(&fam);
        assert!(styles.len() > 1, "{styles:?}");
        let faces: Vec<_> = styles.iter().map(|s| db.face(&fam, s)).filter(|f| f.is_variable()).collect();
        let light = faces.iter().min_by(|a, b| a.weight.total_cmp(&b.weight)).unwrap();
        let heavy = faces.iter().max_by(|a, b| a.weight.total_cmp(&b.weight)).unwrap();
        assert!(heavy.weight > light.weight, "{light:?} {heavy:?}");
        let ink = |f: &FontFace| {
            let g = shape(f, "H", &[], |c| c);
            let b = kurbo::Shape::bounding_box(&*db.outline(f, g[0].gid));
            ((b.area() * 100.0).round() as i64, g[0].x_advance)
        };
        assert_ne!(ink(light), ink(heavy), "instances draw differently");
        // Free axis values: a style with settings makes that instance.
        let axes = db.axes(&fam, &light.style);
        let wght = axes.iter().find(|a| a.0 == "wght").expect("a weight axis");
        let mid = (wght.2 + wght.4) / 2.0;
        let custom = db.face(&fam, &format!("{} {{wght:{mid}}}", light.style));
        assert!(custom.is_variable() && custom.coords.iter().any(|(t, v)| t == b"wght" && *v == mid));
        assert!(!db.styles(&fam).iter().any(|s| s.contains('{')), "instances aren't listed");
        // Settings for axes the font hasn't got are dropped, however many a style string names.
        let bogus: Vec<String> = (0..20_000).map(|i| format!("q{:03}:1", i % 1000)).collect();
        let t = std::time::Instant::now();
        let odd = db.face(&fam, &format!("{} {{wght:{mid},{}}}", light.style, bogus.join(",")));
        assert!(t.elapsed() < std::time::Duration::from_secs(2), "{:?}", t.elapsed());
        assert_eq!(odd.coords.len(), custom.coords.len(), "only the font's own axes are kept");
        let static_face = db.face(DEFAULT_FAMILY, "Regular {wght:700}");
        assert_eq!(static_face.style, "Regular", "static fonts ignore axis settings");
    }

    #[test]
    fn an_exact_family_name_wins_over_the_name_without_its_format_suffix() {
        let db = FontDb::with_font_dirs(Vec::new());
        db.set_system_fallback(false);
        db.add_font(testing::font_with("DC Test Suffix (OTF)", &['a']).unwrap());
        assert_eq!(db.face("DC Test Suffix (OTF)", "Regular").family, "DC Test Suffix (OTF)");
        assert_eq!(db.face(&format!("{DEFAULT_FAMILY} (TT)"), "Regular").family, DEFAULT_FAMILY);
        assert!(db.has_family(&format!("{DEFAULT_FAMILY} (T1)")));
        assert!(!db.has_family(&format!("{DEFAULT_FAMILY} (Bold)")));
        assert!(!db.has_family("DC Test Suffix"));
    }

    /// IDML files from InDesign can name a family with its `$ID/` prefix (`$ID/Arial`).
    #[test]
    fn an_indesign_id_prefix_names_the_plain_family() {
        let db = FontDb::with_font_dirs(Vec::new());
        db.set_system_fallback(false);
        let prefixed = format!("$ID/{DEFAULT_FAMILY}");
        assert_eq!(db.face(&prefixed, "Bold").family, DEFAULT_FAMILY);
        assert!(db.has_family(&prefixed));
        assert_eq!(db.styles(&prefixed), db.styles(DEFAULT_FAMILY));
        assert!(db.has_family(&format!("$ID/{DEFAULT_FAMILY} (OTF)")));
        assert!(!db.has_family("$ID/"));
        assert!(!db.has_family("$ID/DC No Such Family"));
    }

    #[test]
    fn a_family_name_keeps_its_spaces_and_falls_back_to_the_trimmed_name() {
        let db = FontDb::with_font_dirs(Vec::new());
        db.set_system_fallback(false);
        db.add_font(testing::font_with("DC Test Space ", &['a']).unwrap());
        db.add_font(testing::font_with("DC Test Space", &['a']).unwrap());
        assert_eq!(db.face("DC Test Space ", "Regular").family, "DC Test Space ");
        assert_eq!(db.face("DC Test Space", "Regular").family, "DC Test Space");
        assert_eq!(db.face(&format!(" {DEFAULT_FAMILY}\n"), "Regular").family, DEFAULT_FAMILY);
        assert!(db.has_family(&format!("{DEFAULT_FAMILY} ")));
        assert_eq!(db.styles(&format!("{DEFAULT_FAMILY} ")), db.styles(DEFAULT_FAMILY));
    }

    #[test]
    fn outlines_and_metrics() {
        let db = FontDb::global();
        let face = db.face(DEFAULT_FAMILY, "Bold");
        assert_eq!(face.style, "Bold");
        let o = db.outline(&face, face.glyph_for('O'));
        assert!(!o.elements().is_empty());
        assert!(face.ascent > 0.0 && face.descent > 0.0 && face.cap_height > face.x_height);
        assert!(feature("abc").is_none());
    }

    #[test]
    fn a_notdef_without_an_outline_is_drawn_as_a_box() {
        let db = FontDb::with_font_dirs(Vec::new());
        db.set_system_fallback(false);
        let font = testing::font_with("DC Test Empty Notdef", &['a']).unwrap();
        db.add_font(testing::with_empty_notdef(font).unwrap());
        let face = db.face("DC Test Empty Notdef", "Regular");
        assert!(face.skrifa().unwrap().outline_glyphs().get(skrifa::GlyphId::NOTDEF).is_some_and(|g| {
            let mut pen = kurbo::BezPath::new();
            let _ = g.draw(skrifa::outline::DrawSettings::unhinted(skrifa::instance::Size::unscaled(), face.location()), &mut BezPen(&mut pen));
            pen.elements().is_empty()
        }));
        // The box spans the glyph's advance, above the baseline (outlines are y-down).
        let b = kurbo::Shape::bounding_box(&*db.outline(&face, 0));
        assert!(b.width() > 0.0 && b.width() <= face.advance(0), "{b:?}");
        assert!(b.y0 < 0.0 && b.y1 <= 0.0, "{b:?}");
        assert!(db.missing_box(&face).is_some());
        // A font's own .notdef stays as drawn.
        let serif = db.face(DEFAULT_FAMILY, "Regular");
        assert!(db.missing_box(&serif).is_none());
        assert!(!db.outline(&serif, 0).elements().is_empty());
    }

    struct BezPen<'a>(&'a mut kurbo::BezPath);

    impl skrifa::outline::OutlinePen for BezPen<'_> {
        fn move_to(&mut self, x: f32, y: f32) {
            self.0.move_to((x as f64, y as f64));
        }
        fn line_to(&mut self, x: f32, y: f32) {
            self.0.line_to((x as f64, y as f64));
        }
        fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
            self.0.quad_to((cx0 as f64, cy0 as f64), (x as f64, y as f64));
        }
        fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
            self.0.curve_to((cx0 as f64, cy0 as f64), (cx1 as f64, cy1 as f64), (x as f64, y as f64));
        }
        fn close(&mut self) {
            self.0.close_path();
        }
    }

    #[test]
    fn craft_fonts_japanese_faces_cover_japanese_and_have_vertical_forms() {
        if CRAFT_FONTS.is_empty() {
            eprintln!("skipped: built without craft-fonts (set CRAFT_FONTS_DIR to a checkout)");
            return;
        }
        let fonts = japanese_document_fonts();
        assert!(!fonts.is_empty(), "craft-fonts has Japanese fonts");
        assert!(fonts[0].family.contains("Mincho"), "documents fall back to a Mincho first");
        assert_eq!(japanese_ui_fonts(false)[0].family, "BIZ UDPGothic");
        assert_eq!(japanese_ui_fonts(true)[0].style, "Bold");
        let db = FontDb::with_font_dirs(Vec::new());
        db.set_system_fallback(false);
        for cf in fonts {
            // Inspect the embedded face directly so this cannot pass through an OS fallback.
            let font = skrifa::FontRef::new(cf.bytes).unwrap();
            for ch in "日本語縦書き横書きルビ、。「」".chars() {
                assert_ne!(font.charmap().map(ch).unwrap_or_default(), skrifa::GlyphId::NOTDEF, "{} lacks {ch}", cf.family);
            }
            let face = db.face(cf.family, cf.style);
            assert_eq!(face.family, cf.family);
            let horizontal = shape(&face, "「」、。", &[], |c| c);
            let vertical = shape(&face, "「」、。", &[feature("vert").unwrap(), feature("vrt2").unwrap()], |c| c);
            assert_eq!(horizontal.len(), vertical.len());
            assert!(horizontal.iter().zip(&vertical).any(|(h, v)| h.gid != v.gid), "{} has vertical forms", cf.family);
        }
        // Japanese in a Latin face falls back to a craft-fonts Mincho, without system fonts, in
        // Japanese text (the Japanese chain) and without a language (the first loaded face).
        let latin = db.face(DEFAULT_FAMILY, "Regular");
        for language in [Some("ja"), None] {
            let fb = db.fallback_for('語', latin.id(), language).unwrap();
            assert!(fb.family.contains("Mincho"), "{language:?}: {fb:?}");
        }
    }

    #[test]
    fn works_without_craft_fonts() {
        // Always true without CRAFT_FONTS_DIR; with it, the bundled fonts must still all load.
        let db = FontDb::with_font_dirs(Vec::new());
        db.set_system_fallback(false);
        assert_eq!(db.face(DEFAULT_FAMILY, "Regular").family, DEFAULT_FAMILY);
        assert!(!shape(&db.face(DEFAULT_FAMILY, "Regular"), "Hello", &[], |c| c).is_empty());
        if CRAFT_FONTS.is_empty() {
            assert!(japanese_fonts().next().is_none() && japanese_document_fonts().is_empty());
            let latin = db.face(DEFAULT_FAMILY, "Regular");
            for language in [Some("ja"), None] {
                assert!(db.fallback_for('語', latin.id(), language).is_none(), "no bundled Japanese font: {language:?}");
            }
        }
    }
}

#[cfg(test)]
mod resolution_tests {
    use super::*;

    #[test]
    fn localized_family_and_style_names_resolve_without_false_missing_status() {
        let bytes = testing::font_with("Alias Face", &['A']).unwrap();
        let bytes = testing::with_names(
            &bytes,
            &[(3, 1, 0x409, 1, "Alias Face"), (3, 1, 0x804, 1, "别名字体"), (3, 1, 0x409, 2, "Regular"), (3, 1, 0x804, 2, "常规")],
        )
        .unwrap();
        let db = FontDb::with_font_dirs(Vec::new());
        db.add_font(bytes);
        let scope = db.scoped(0);
        assert!(scope.has_family("别名字体"));
        assert_eq!(scope.face("别名字体", "常规").family, "Alias Face");
        assert!(scope.has_style("别名字体", "常规"));
        assert!(!scope.has_style("别名字体", "Unreal Style"));
        assert_eq!(scope.styles("别名字体"), ["Regular"]);
    }

    #[test]
    fn a_style_name_beats_another_face_s_legacy_subfamily() {
        // Static families often name each weight's legacy (RIBBI) family "Legacy Sans Black" with
        // subfamily "Regular": that alias must not shadow the face actually styled Regular.
        let face =
            |records: &[(u16, u16, u16, u16, &str)]| testing::with_names(&testing::font_with("Legacy Sans", &['A']).unwrap(), records).unwrap();
        let db = FontDb::with_font_dirs(Vec::new());
        db.add_font(face(&[
            (3, 1, 0x409, 1, "Legacy Sans Black"),
            (3, 1, 0x409, 2, "Regular"),
            (3, 1, 0x409, 16, "Legacy Sans"),
            (3, 1, 0x409, 17, "Black"),
        ]));
        db.add_font(face(&[(3, 1, 0x409, 1, "Legacy Sans"), (3, 1, 0x409, 2, "Regular")]));
        let scope = db.scoped(0);
        assert_eq!(scope.face("Legacy Sans", "Regular").style, "Regular");
        assert_eq!(scope.face("Legacy Sans", "Black").style, "Black");
    }

    #[test]
    fn known_substitution_preserves_missing_original_status() {
        let db = FontDb::with_font_dirs(Vec::new());
        db.add_font(testing::font_with("Carlito", &['A']).unwrap());
        let scope = db.scoped(0);
        assert_eq!(scope.substitute_family("Calibri"), "Carlito");
        assert!(scope.canonical_family("An Unavailable Original Family").is_none());
        assert_eq!(scope.face("An Unavailable Original Family", "Regular").family, FALLBACK_FAMILY);
    }
}

#[cfg(test)]
mod coverage_recommendation_tests {
    use super::*;

    #[test]
    fn placeholder_fonts_do_not_satisfy_glyph_coverage() {
        let db = FontDb::with_font_dirs(Vec::new());
        db.add_font(testing::font_with(".LastResort", &['\u{10fffd}']).unwrap());
        let face = db.face(".LastResort", "Regular");
        assert!(face.is_placeholder());
        assert!(!face.covers('\u{10fffd}'));
        assert!(db.fallback_for('\u{10fffd}', u32::MAX, None).is_none());
    }

    #[test]
    fn arabic_replacement_also_covers_surrounding_punctuation() {
        let db = FontDb::with_font_dirs(Vec::new());
        db.add_font(testing::font_with("Geeza Pro", &['ع']).unwrap());
        db.add_font(testing::font_with("Al Bayan", &['ع', '“']).unwrap());
        let scope = db.scoped(0);
        assert_eq!(scope.face("Traditional Arabic", "Regular").family, "Geeza Pro");
        assert_eq!(scope.replacement_face("Traditional Arabic", "Regular", &['ع', '“']).family, "Al Bayan");
        assert!(!scope.has_family("Traditional Arabic"));
    }
}
