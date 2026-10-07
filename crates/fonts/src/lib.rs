//! DesignCraft fonts: the font database (bundled OFL families + user/system fonts), vertical
//! metrics, glyph outlines and OpenType shaping.
//!
//! Shaping here is style-agnostic: [`shape`] turns a string in one face into glyph ids, clusters
//! and advances in font units. `designcraft-compose` applies sizes, tracking, scaling and
//! justification on top.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod fontdb;

pub use fontdb::{FALLBACK_FAMILY, FaceRef, FontDb, FontFace, base_style, bundled, system_font_dirs};
pub use harfrust::Feature;
use harfrust::{Direction, ShapeOptions, Tag, UnicodeBuffer};
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

/// One shaped glyph, in font units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShapedGlyph {
    pub gid: u32,
    /// Byte offset (in the shaped string) of the cluster this glyph belongs to.
    pub cluster: usize,
    pub x_advance: i32,
    pub x_offset: i32,
    pub y_offset: i32,
    pub safe_tatweel_before: bool,
    /// Direction used for shaping, including Unicode mirroring.
    pub rtl: bool,
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

#[derive(Clone, Copy, Default)]
pub struct ShapeContext<'a> {
    pub before: &'a str,
    pub after: &'a str,
    pub language: &'a str,
}

/// Normalize the supported application language names while also accepting BCP 47 tags.
pub fn language_tag(language: &str) -> &str {
    match language.split(':').next().unwrap_or(language).trim() {
        "Arabic" => "ar",
        "Persian" | "Farsi" => "fa",
        "Urdu" => "ur",
        "Hebrew" => "he",
        "English" => "en",
        _ => language,
    }
}

/// Shape `text` with `face`. `chars` lets callers substitute characters (e.g. uppercase for All
/// Caps) while keeping clusters pointing into the original string. Glyphs come in logical
/// order: right-to-left runs are shaped right to left, then their clusters put back in text
/// order (each cluster's glyphs keep the shaper's order); line layout reorders them visually.
pub fn shape(face: &FontFace, text: &str, features: &[Feature], map: impl Fn(char) -> char) -> Vec<ShapedGlyph> {
    shape_with_context(face, text, features, map, ShapeContext::default())
}

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

fn shape_dir(
    face: &FontFace,
    text: &str,
    features: &[Feature],
    map: &impl Fn(char) -> char,
    rtl: bool,
    script: unicode_script::Script,
    context: ShapeContext<'_>,
) -> Vec<ShapedGlyph> {
    let mut out = Vec::with_capacity(text.len());
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
        if let Ok(language) = language_tag(context.language).parse() {
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
                x_offset: 0,
                y_offset: 0,
                safe_tatweel_before: false,
                rtl: false,
            });
        }
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
    fn arabic_script_runs_do_not_inherit_the_hebrew_shaper() {
        use unicode_script::Script;
        let text = "אב بب";
        let runs = direction_runs(text);
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0], (0..5, true, Script::Hebrew));
        assert_eq!(runs[1], (5..text.len(), true, Script::Arabic));
        assert_eq!(language_tag("Arabic"), "ar");
        assert_eq!(language_tag("Farsi"), "fa");
        assert_eq!(language_tag("Urdu"), "ur");
        assert_eq!(language_tag("ar-SA"), "ar-SA");
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
        let static_face = db.face(DEFAULT_FAMILY, "Regular {wght:700}");
        assert_eq!(static_face.style, "Regular", "static fonts ignore axis settings");
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
        // Japanese in a Latin face falls back to a craft-fonts Mincho, without system fonts.
        let latin = db.face(DEFAULT_FAMILY, "Regular");
        let fb = db.fallback_for('語', latin.id()).unwrap();
        assert!(fb.family.contains("Mincho"), "{fb:?}");
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
            assert!(db.fallback_for('語', latin.id()).is_none(), "no bundled Japanese font");
        }
    }
}
