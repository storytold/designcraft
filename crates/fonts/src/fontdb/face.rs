use super::db::NEXT_ID;
use super::names::{aliases, face_names, name, norm, style_italic, style_weight, wws_names};
use super::outline::has_serifs;
use super::{Arc, DrawSettings, FileRef, FontGroup, GlyphId, Location, LocationRef, MetadataProvider, Ordering, OutlinePen, Size, StringId};

/// The family used when a requested family is unknown (the UI sans).
pub const FALLBACK_FAMILY: &str = "Source Sans 3";

/// The bundled fonts (OFL), as font file bytes. Japanese fonts are not bundled: they come from
/// the optional craft-fonts build input ([`crate::japanese_document_fonts`]).
pub fn bundled() -> &'static [&'static [u8]] {
    BUNDLED
}

pub(super) static BUNDLED: &[&[u8]] = &[
    include_bytes!("../../../../assets/fonts/SourceSans3-Regular.ttf"),
    include_bytes!("../../../../assets/fonts/SourceSans3-Semibold.ttf"),
    include_bytes!("../../../../assets/fonts/SourceSans3-Bold.ttf"),
    include_bytes!("../../../../assets/fonts/SourceSans3-It.ttf"),
    include_bytes!("../../../../assets/fonts/SourceSerif4-Regular.ttf"),
    include_bytes!("../../../../assets/fonts/SourceSerif4-It.ttf"),
    include_bytes!("../../../../assets/fonts/SourceSerif4-Semibold.ttf"),
    include_bytes!("../../../../assets/fonts/SourceSerif4-Bold.ttf"),
    include_bytes!("../../../../assets/fonts/SourceSerif4-BoldIt.ttf"),
    include_bytes!("../../../../assets/fonts/Inter-Regular.ttf"),
    include_bytes!("../../../../assets/fonts/Inter-Medium.ttf"),
    include_bytes!("../../../../assets/fonts/Inter-SemiBold.ttf"),
    include_bytes!("../../../../assets/fonts/JetBrainsMono-Regular.ttf"),
];

#[derive(Clone)]
pub(super) enum FontBytes {
    Static(&'static [u8]),
    Owned(Arc<Vec<u8>>),
}

/// Where a face's font came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FontSource {
    /// Compiled into DesignCraft.
    Bundled,
    /// Font data handed to [`FontDb::add_font`], with no file behind it.
    Memory,
    /// An installed font file (the system's or the user's font folders, or one the platform
    /// lists outside them).
    Installed(std::path::PathBuf),
    /// A font file in a document's `Document Fonts` folder ([`FontDb::load_document_fonts`]).
    Document(std::path::PathBuf),
}

/// The folder beside a document whose fonts the document brings with it (as File › Package
/// writes it).
pub const DOCUMENT_FONTS_FOLDER: &str = "Document Fonts";

/// Caps on what a `Document Fonts` folder (untrusted files) can make [`FontDb::load_document_fonts`]
/// read: font files read, bytes per file and bytes in all.
pub const MAX_DOCUMENT_FONT_FILES: usize = 200;

pub const MAX_DOCUMENT_FONT_BYTES: u64 = 256 << 20;

pub const MAX_DOCUMENT_FONTS_TOTAL: u64 = 512 << 20;

/// What [`FontDb::load_document_fonts`] did: the scope the document looks its fonts up in (0 when
/// the folder gave none), the faces in it, and a note for each file skipped.
#[derive(Debug, Default)]
pub struct DocumentFonts {
    pub scope: u32,
    pub faces: usize,
    pub skipped: Vec<String>,
}

/// One loaded font face.
pub struct FontFace {
    pub(super) id: u32,
    /// Typographic family name (e.g. "Source Sans 3").
    pub family: String,
    /// Typographic style name (e.g. "Semibold", "Italic").
    pub style: String,
    /// The WWS family and style names (name IDs 21 and 22) when they differ from the typographic
    /// ones: InDesign names an optical-size face by them ("Kepler Std Display" / "Light").
    pub wws: Option<(String, String)>,
    /// usWeightClass-style weight (400 = regular).
    pub weight: f32,
    pub italic: bool,
    /// Where the font came from (a file to copy when packaging, a document's own font).
    pub source: FontSource,
    pub(super) bytes: FontBytes,
    pub(super) index: u32,
    pub upem: f64,
    /// Ascender in font units (positive = up).
    pub ascent: f64,
    /// OS/2 typographic ascender in font units (the ascent when the font has no usable one): the
    /// Ascent first baseline offset.
    pub typo_ascent: f64,
    /// Descender in font units (positive = down).
    pub descent: f64,
    /// Cap height and x height in font units (without plausible OS/2 values, the cap height is the
    /// top of the H, and either is estimated from the ascent): the Cap Height and x Height first
    /// baseline offsets.
    pub cap_height: f64,
    pub x_height: f64,
    pub shaper: harfrust::ShaperData,
    /// Variable fonts: the named instance's axis settings (user units; empty = default instance),
    /// the normalised location and the shaper's view of it.
    pub coords: Vec<([u8; 4], f32)>,
    location: Location,
    pub instance: Option<harfrust::ShaperInstance>,
    /// Basic Multilingual Plane coverage bitset, built on first use.
    bmp: std::sync::OnceLock<Box<[u64]>>,
    /// The ideographic em box, read on first use.
    em: std::sync::OnceLock<((f64, f64), bool)>,
    /// The ideographic character face box, read on first use.
    icf: std::sync::OnceLock<(f64, f64)>,
    /// The font menu group and native family name, read on first use.
    group: std::sync::OnceLock<(FontGroup, Option<String>)>,
    pub(super) family_aliases: Vec<String>,
    pub(super) style_aliases: Vec<String>,
    /// Format, class and PANOSE numbers (the font menus), read on first use.
    kind: std::sync::OnceLock<crate::kind::FaceKind>,
    /// The PostScript name, read on first use.
    postscript: std::sync::OnceLock<Option<String>>,
}

/// A cheap `Copy` handle to a face. Faces are never unloaded, so the handle lives for the rest of
/// the process; unlike cloning an `Arc`, copying it touches no shared reference count (which made
/// parallel composition scale negatively).
#[derive(Clone, Copy)]
pub struct FaceRef(pub(crate) &'static FontFace);

impl FontFace {
    pub fn data(&self) -> &[u8] {
        match &self.bytes {
            FontBytes::Static(b) => b,
            FontBytes::Owned(v) => v.as_slice(),
        }
    }

    pub fn skrifa(&self) -> Option<skrifa::FontRef<'_>> {
        skrifa::FontRef::from_index(self.data(), self.index).ok()
    }

    /// The face's format, class and PANOSE numbers, as the font menus show and filter them.
    pub fn kind(&self) -> crate::kind::FaceKind {
        *self.kind.get_or_init(|| {
            let mut kind = self.skrifa().map(|f| crate::kind::read_kind(&f, &self.family)).unwrap_or_default();
            // Nothing in the tables or the name: look at the letters.
            if kind.class == crate::kind::FontClass::Other
                && let Some(serifs) = has_serifs(self)
            {
                kind.class = if serifs { crate::kind::FontClass::Serif } else { crate::kind::FontClass::SansSerif };
            }
            kind
        })
    }

    pub fn hb(&self) -> Option<harfrust::FontRef<'_>> {
        harfrust::FontRef::from_index(self.data(), self.index).ok()
    }

    /// Where in the design space glyphs, metrics and outlines come from.
    pub fn location(&self) -> LocationRef<'_> {
        (&self.location).into()
    }

    /// Is this a named instance of a variable font?
    pub fn is_variable(&self) -> bool {
        !self.coords.is_empty()
    }

    /// Face index within the font file (collections); 0 for plain fonts.
    pub fn index(&self) -> u32 {
        self.index
    }

    /// Unique id of this face within the process.
    pub fn id(&self) -> u32 {
        self.id
    }

    /// System placeholder fonts draw generic boxes rather than the requested character.
    pub fn is_placeholder(&self) -> bool {
        matches!(norm(self.family.trim_start_matches('.')).as_str(), "lastresort")
    }

    /// Does the face map `c` to a real glyph?
    pub fn covers(&self, c: char) -> bool {
        if self.is_placeholder() {
            return false;
        }
        let cp = c as u32;
        if cp < 0x1_0000 {
            let bits = self.bmp.get_or_init(|| {
                let mut b = vec![0u64; 1024].into_boxed_slice();
                if let Some(f) = self.skrifa() {
                    for (cp, gid) in f.charmap().mappings() {
                        if cp < 0x1_0000 && gid.to_u32() != 0 {
                            b[(cp / 64) as usize] |= 1 << (cp % 64);
                        }
                    }
                }
                b
            });
            return bits[(cp / 64) as usize] & (1 << (cp % 64)) != 0;
        }
        self.skrifa().is_some_and(|f| f.charmap().map(c).is_some_and(|gid| gid.to_u32() != 0))
    }

    /// Units per em.
    pub fn units_per_em(&self) -> f64 {
        self.upem
    }

    /// (ascent, descent) in font units, both positive.
    pub fn vertical_metrics(&self) -> (f64, f64) {
        (self.ascent, self.descent)
    }

    /// Every mapped character and its glyph id, sorted by code point (the Glyphs panel).
    pub fn chars(&self) -> Vec<(char, u32)> {
        let Some(f) = self.skrifa() else { return vec![] };
        let mut v: Vec<(char, u32)> = f.charmap().mappings().filter_map(|(cp, g)| char::from_u32(cp).map(|c| (c, g.to_u32()))).collect();
        v.sort_unstable_by_key(|x| x.0);
        v.dedup_by_key(|x| x.0);
        v
    }

    /// Advance width of glyph `gid` in font units.
    pub fn advance(&self, gid: u32) -> f64 {
        self.skrifa()
            .and_then(|f| f.glyph_metrics(Size::unscaled(), self.location()).advance_width(GlyphId::new(gid)))
            .map(|a| a as f64)
            .unwrap_or(self.upem * 0.5)
    }

    /// Vertical advance (down the line) of glyph `gid` in font units: `vmtx`, else one em.
    pub fn v_advance(&self, gid: u32) -> f64 {
        crate::vertical::VMetrics::new(self).advance(GlyphId::new(gid))
    }

    /// Height of glyph `gid`'s vertical origin above its baseline in font units: `VORG`, else its
    /// top plus its `vmtx` top side bearing, else the em box top. Across, the origin is at half the
    /// horizontal advance. Upright glyphs in vertical text hang from it on the line's centre.
    pub fn v_origin(&self, gid: u32) -> f64 {
        crate::vertical::VMetrics::new(self).origin(GlyphId::new(gid))
    }

    /// The ideographic em box (top, bottom) in font units, y up, one em tall (Japanese fonts:
    /// typically 880, -120): `BASE` `idtp`/`ideo`; else, for a CJK font, from the OS/2 typo
    /// ascender when the typo ascender and descender span one em; else centred on half the cap
    /// height (OS/2, else the `H` glyph's top); else 0.88 em above the baseline.
    pub fn em_box(&self) -> (f64, f64) {
        self.em.get_or_init(|| crate::vertical::em_box(self)).0
    }

    /// [`Self::em_box`] when the font's tables give it, `None` for the 0.88 em fallback.
    pub fn declared_em_box(&self) -> Option<(f64, f64)> {
        let (b, declared) = *self.em.get_or_init(|| crate::vertical::em_box(self));
        declared.then_some(b)
    }

    /// The ideographic character face (ICF) box (top, bottom) in font units, y up: `BASE`
    /// `icft`/`icfb`, else the em box inset by 5% of the em at the top and bottom.
    pub fn icf_box(&self) -> (f64, f64) {
        *self.icf.get_or_init(|| crate::vertical::icf_box(self))
    }

    /// The font menu group the font is for and, for a CJK group, its family name in that language
    /// (see [`crate::FamilyInfo`]); read from the font once.
    pub fn menu_group(&self) -> (FontGroup, Option<&str>) {
        let (g, n) = self.group.get_or_init(|| self.skrifa().map(|f| crate::group::classify(&f, &self.family)).unwrap_or_default());
        (*g, n.as_deref())
    }

    /// The PostScript name (`name` ID 6), read from the font once. None for a variable font's
    /// named instance: the file's name belongs to its default instance.
    pub fn postscript_name(&self) -> Option<&str> {
        self.postscript
            .get_or_init(|| if self.is_variable() { None } else { self.skrifa().and_then(|f| name(&f, &[StringId::POSTSCRIPT_NAME])) })
            .as_deref()
    }

    /// Glyph id for `c` (0 = .notdef).
    pub fn glyph_for(&self, c: char) -> u32 {
        self.skrifa().and_then(|f| f.charmap().map(c)).map(|g| g.to_u32()).unwrap_or(0)
    }

    /// GDEF identifies marks even when shaping merges their source cluster with a base.
    pub fn glyph_is_mark(&self, gid: u32) -> bool {
        use skrifa::raw::TableProvider;
        self.skrifa()
            .and_then(|f| f.gdef().ok())
            .and_then(|gdef| gdef.glyph_class_def()?.ok())
            .is_some_and(|classes| classes.get(GlyphId::new(gid)) == 3)
    }

    /// Does the font's licence forbid passing it on? OS/2 `fsType` Restricted License embedding
    /// (bit 1) without a less restrictive bit (Preview & Print, Editable). Fonts without an OS/2
    /// table aren't restricted.
    pub fn restricted_licence(&self) -> bool {
        use skrifa::raw::TableProvider;
        let fs_type = self.skrifa().and_then(|f| f.os2().ok()).map_or(0, |t| t.fs_type());
        fs_type & 0x0002 != 0 && fs_type & 0x000C == 0
    }

    /// Does the font's licence allow embedding bitmaps only? OS/2 `fsType` bit 9: the outlines
    /// may not be passed on, so the font file can't be packaged whole.
    pub fn bitmap_only(&self) -> bool {
        use skrifa::raw::TableProvider;
        self.skrifa().and_then(|f| f.os2().ok()).is_some_and(|t| t.fs_type() & 0x0200 != 0)
    }
}

/// One face found in a font file: index, family, style and (variable fonts) the named instance's
/// axis settings.
pub(super) type Found = (u32, String, String, Vec<([u8; 4], f32)>);

/// Parse every face in `data` (a font file or collection); a variable font yields one face per
/// named instance (InDesign lists them as styles).
pub(super) fn enumerate_faces(data: &[u8]) -> Vec<Found> {
    /// A collection's count is input: what a damaged one can make the loop try.
    const MAX_FACES: u32 = 1024;
    let count = match FileRef::new(data) {
        Ok(FileRef::Font(_)) => 1,
        Ok(FileRef::Collection(c)) => c.len().min(MAX_FACES),
        Err(_) => 0,
    };
    let mut out = Vec::new();
    for i in 0..count {
        let Ok(f) = skrifa::FontRef::from_index(data, i) else { continue };
        let Some((family, style)) = face_names(&f) else { continue };
        let axes = f.axes();
        let mut named = Vec::new();
        for ni in f.named_instances().iter() {
            let Some(style) = name(&f, &[ni.subfamily_name_id()]) else { continue };
            if named.iter().any(|(_, s, _): &(u32, String, _)| s.eq_ignore_ascii_case(&style)) {
                continue;
            }
            let coords: Vec<([u8; 4], f32)> = axes.iter().zip(ni.user_coords()).map(|(a, v)| (a.tag().to_be_bytes(), v)).collect();
            named.push((i, style, coords));
        }
        if named.is_empty() {
            out.push((i, family, style, Vec::new()));
        } else {
            out.extend(named.into_iter().map(|(i, s, c)| (i, family.clone(), s, c)));
        }
    }
    out
}

/// The compiled-in Source Sans 3 Regular, for a database that somehow has no faces at all.
pub(crate) fn last_resort_face() -> Arc<FontFace> {
    static FACE: std::sync::OnceLock<Arc<FontFace>> = std::sync::OnceLock::new();
    FACE.get_or_init(|| {
        // The font is compiled in (`include_bytes!`), so parsing it can't depend on input; the
        // `last_resort_face_parses` test proves it on every run.
        #[allow(clippy::expect_used)]
        let face = make_face(FontBytes::Static(BUNDLED[0]), FontSource::Bundled, 0, FALLBACK_FAMILY.into(), "Regular".into(), Vec::new())
            .expect("the compiled-in Source Sans 3 Regular parses");
        Arc::new(face)
    })
    .clone()
}

pub(super) fn make_face(
    bytes: FontBytes,
    source: FontSource,
    index: u32,
    family: String,
    style: String,
    coords: Vec<([u8; 4], f32)>,
) -> Option<FontFace> {
    let data: &[u8] = match &bytes {
        FontBytes::Static(b) => b,
        FontBytes::Owned(v) => v.as_slice(),
    };
    let f = skrifa::FontRef::from_index(data, index).ok()?;
    let settings: Vec<(skrifa::Tag, f32)> = coords.iter().map(|(t, v)| (skrifa::Tag::new(t), *v)).collect();
    let location = if coords.is_empty() { Location::default() } else { f.axes().location(settings.iter().copied()) };
    let m = f.metrics(Size::unscaled(), &location);
    let upem = m.units_per_em.max(1) as f64;
    let a = f.attributes();
    let wws = wws_names(&f, &family, &style);
    let hb = harfrust::FontRef::from_index(data, index).ok()?;
    let shaper = harfrust::ShaperData::new(&hb);
    let instance = (!coords.is_empty())
        .then(|| harfrust::ShaperInstance::from_variations(&hb, settings.iter().map(|(t, v)| harfrust::Variation { tag: *t, value: *v })));
    let axis = |tag: &[u8; 4]| coords.iter().find(|(t, _)| t == tag).map(|(_, v)| *v);
    let weight = axis(b"wght").unwrap_or(if coords.is_empty() { a.weight.value() } else { style_weight(&style) });
    let italic = axis(b"ital")
        .map(|v| v >= 0.5)
        .or(axis(b"slnt").map(|v| v.abs() > 0.1))
        .unwrap_or(!matches!(a.style, skrifa::attribute::Style::Normal) || style_italic(&style));
    let family_aliases = aliases(&f, &[StringId::TYPOGRAPHIC_FAMILY_NAME, StringId::FAMILY_NAME]);
    let style_aliases = aliases(&f, &[StringId::TYPOGRAPHIC_SUBFAMILY_NAME, StringId::SUBFAMILY_NAME]);
    Some(FontFace {
        id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
        family,
        style,
        wws,
        weight,
        italic,
        upem,
        ascent: m.ascent as f64,
        typo_ascent: typo_ascent(&f, &location, upem).unwrap_or(m.ascent as f64),
        descent: -(m.descent as f64),
        // Fonts with an OS/2 table older than version 2 don't declare these: measure them from
        // flat-topped letters before falling back to a share of the ascent, which can be far off
        // (drop caps are sized from the cap height).
        cap_height: plausible(m.cap_height.map(f64::from), upem)
            .or_else(|| measured_top(&f, (&location).into(), &['H', 'I', 'E', 'T'], upem))
            .unwrap_or(m.ascent as f64 * 0.72),
        x_height: plausible(m.x_height.map(f64::from), upem)
            .or_else(|| measured_top(&f, (&location).into(), &['x', 'z', 'v'], upem))
            .unwrap_or(m.ascent as f64 * 0.5),
        shaper,
        coords,
        location,
        instance,
        source,
        bytes,
        index,
        bmp: std::sync::OnceLock::new(),
        em: std::sync::OnceLock::new(),
        family_aliases,
        style_aliases,
        icf: std::sync::OnceLock::new(),
        group: std::sync::OnceLock::new(),
        kind: std::sync::OnceLock::new(),
        postscript: std::sync::OnceLock::new(),
    })
}

/// A font's height above the baseline (font units) if it is plausible: above 0 and at most 4 em.
pub(crate) fn plausible(v: Option<f64>, upem: f64) -> Option<f64> {
    v.filter(|v| *v > 0.0 && *v <= 4.0 * upem)
}

/// The top of the first of `chars` the font has a non-empty outline for, in font units: the
/// highest point of its unhinted outline at `location`, if [`plausible`]. For flat-topped letters
/// that is the cap height (`H`) or x-height (`x`).
pub(crate) fn measured_top(f: &skrifa::FontRef<'_>, location: LocationRef<'_>, chars: &[char], upem: f64) -> Option<f64> {
    struct Top(f32);
    impl OutlinePen for Top {
        fn move_to(&mut self, _x: f32, y: f32) {
            self.0 = self.0.max(y);
        }
        fn line_to(&mut self, _x: f32, y: f32) {
            self.0 = self.0.max(y);
        }
        fn quad_to(&mut self, _cx: f32, _cy: f32, _x: f32, y: f32) {
            self.0 = self.0.max(y);
        }
        fn curve_to(&mut self, _cx0: f32, _cy0: f32, _cx1: f32, _cy1: f32, _x: f32, y: f32) {
            self.0 = self.0.max(y);
        }
        fn close(&mut self) {}
    }
    let cmap = f.charmap();
    let glyphs = f.outline_glyphs();
    chars.iter().find_map(|&c| {
        let g = glyphs.get(cmap.map(c)?)?;
        let mut pen = Top(f32::NEG_INFINITY);
        g.draw(DrawSettings::unhinted(Size::unscaled(), location), &mut pen).ok()?;
        plausible(Some(f64::from(pen.0)), upem)
    })
}

/// The OS/2 typographic ascender in font units at `location`, if the font has an OS/2 table and
/// the value is [`plausible`].
fn typo_ascent(f: &skrifa::FontRef<'_>, location: &Location, upem: f64) -> Option<f64> {
    use skrifa::raw::TableProvider;
    let mut v = f.os2().ok()?.s_typo_ascender() as f64;
    if !location.coords().is_empty()
        && let Ok(mvar) = f.mvar()
    {
        v += mvar.metric_delta(skrifa::raw::tables::mvar::tags::HASC, location.coords()).map_or(0.0, |d| d.to_f64());
    }
    plausible(Some(v), upem)
}

#[cfg(test)]
mod tests_heights {
    use super::*;
    use crate::testing::{font_mapping, with_table_u16};

    fn face_of(bytes: Vec<u8>) -> FontFace {
        make_face(FontBytes::Owned(Arc::new(bytes)), FontSource::Memory, 0, "Test".into(), "Regular".into(), Vec::new()).unwrap()
    }

    /// `font` with OS/2 sxHeight and sCapHeight set to 0 (undeclared).
    fn undeclared(font: Vec<u8>) -> Vec<u8> {
        with_table_u16(with_table_u16(font, b"OS/2", 86, 0).unwrap(), b"OS/2", 88, 0).unwrap()
    }

    /// The top of Source Sans 3's glyph for `c` from its bounding box (font units).
    fn bbox_top(c: char) -> f64 {
        let f = skrifa::FontRef::new(BUNDLED[0]).unwrap();
        f.glyph_metrics(Size::unscaled(), LocationRef::default()).bounds(f.charmap().map(c).unwrap()).unwrap().y_max as f64
    }

    /// A font that doesn't declare its cap height and x-height gets them measured from its
    /// flat-topped letters, also when it lacks the first choices `H` and `x`; a font that
    /// declares them keeps them.
    #[test]
    fn undeclared_heights_are_measured_from_outlines() {
        let glyphs = [('H', 'H'), ('x', 'x'), ('E', 'E'), ('z', 'z')];
        for (glyphs, cap, x) in [(&glyphs[..], 'H', 'x'), (&glyphs[2..], 'E', 'z')] {
            let face = face_of(undeclared(font_mapping("Heights", glyphs).unwrap()));
            let (cap, x) = (bbox_top(cap), bbox_top(x));
            assert!(cap > x && x > 0.0);
            assert!((face.cap_height - cap).abs() <= 1.0, "{glyphs:?}: cap height {} vs {cap}", face.cap_height);
            assert!((face.x_height - x).abs() <= 1.0, "{glyphs:?}: x-height {} vs {x}", face.x_height);
        }
        let declared = face_of(font_mapping("Heights", &glyphs).unwrap());
        assert_eq!((declared.cap_height, declared.x_height), (660.0, 486.0), "Source Sans 3 Regular's OS/2 values");
        // Declared values win over the outlines: `H` drawn as `x` doesn't lower the cap height.
        let face = face_of(font_mapping("Heights", &[('H', 'x'), ('x', 'H')]).unwrap());
        assert_eq!((face.cap_height, face.x_height), (declared.cap_height, declared.x_height));
    }

    /// Without any of the letters, or with only empty outlines, the heights fall back to a share
    /// of the ascent.
    #[test]
    fn heights_without_measurable_letters_fall_back_to_the_ascent() {
        for glyphs in [&[('a', 'a')][..], &[('H', ' '), ('x', ' ')]] {
            let face = face_of(undeclared(font_mapping("Heights", glyphs).unwrap()));
            assert_eq!((face.cap_height, face.x_height), (face.ascent * 0.72, face.ascent * 0.5), "{glyphs:?}");
        }
    }
}
