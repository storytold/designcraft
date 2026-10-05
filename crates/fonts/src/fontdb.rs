//! Font database: bundled OFL fonts, user fonts, optional system font catalog, outline cache.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use kurbo::BezPath;
use skrifa::instance::{Location, LocationRef, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::raw::FileRef;
use skrifa::string::StringId;
use skrifa::{GlyphId, MetadataProvider};

/// The family used when a requested family is unknown (the UI sans).
pub const FALLBACK_FAMILY: &str = "Source Sans 3";

/// Shippori Mincho (OFL): Japanese glyphs for documents and the UI without OS fonts. One copy,
/// shared with the UI's egui fonts (it is ~8.7 MB).
pub static JAPANESE_FALLBACK: &[u8] = include_bytes!("../../../assets/fonts/ShipporiMincho-Regular.ttf");

/// The bundled fonts (OFL), as font file bytes.
pub fn bundled() -> &'static [&'static [u8]] {
    BUNDLED
}

static BUNDLED: &[&[u8]] = &[
    include_bytes!("../../../assets/fonts/SourceSans3-Regular.ttf"),
    include_bytes!("../../../assets/fonts/SourceSans3-Semibold.ttf"),
    include_bytes!("../../../assets/fonts/SourceSans3-Bold.ttf"),
    include_bytes!("../../../assets/fonts/SourceSans3-It.ttf"),
    include_bytes!("../../../assets/fonts/SourceSerif4-Regular.ttf"),
    include_bytes!("../../../assets/fonts/SourceSerif4-It.ttf"),
    include_bytes!("../../../assets/fonts/SourceSerif4-Semibold.ttf"),
    include_bytes!("../../../assets/fonts/SourceSerif4-Bold.ttf"),
    include_bytes!("../../../assets/fonts/SourceSerif4-BoldIt.ttf"),
    include_bytes!("../../../assets/fonts/Inter-Regular.ttf"),
    include_bytes!("../../../assets/fonts/Inter-Medium.ttf"),
    include_bytes!("../../../assets/fonts/Inter-SemiBold.ttf"),
    include_bytes!("../../../assets/fonts/JetBrainsMono-Regular.ttf"),
    JAPANESE_FALLBACK,
];

#[derive(Clone)]
enum FontBytes {
    Static(&'static [u8]),
    Owned(Arc<Vec<u8>>),
}

/// One loaded font face.
pub struct FontFace {
    id: u32,
    /// Typographic family name (e.g. "Source Sans 3").
    pub family: String,
    /// Typographic style name (e.g. "Semibold", "Italic").
    pub style: String,
    /// usWeightClass-style weight (400 = regular).
    pub weight: f32,
    pub italic: bool,
    bytes: FontBytes,
    index: u32,
    pub upem: f64,
    /// Ascender in font units (positive = up).
    pub ascent: f64,
    /// Descender in font units (positive = down).
    pub descent: f64,
    /// Cap height and x height in font units (estimated from the ascent when the font has no OS/2
    /// values).
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
}

/// A cheap `Copy` handle to a face. Faces are never unloaded, so the handle lives for the rest of
/// the process; unlike cloning an `Arc`, copying it touches no shared reference count (which made
/// parallel composition scale negatively).
#[derive(Clone, Copy)]
pub struct FaceRef(&'static FontFace);

impl FaceRef {
    /// The handle for a loaded face (memoised per face).
    pub fn of(face: &Arc<FontFace>) -> FaceRef {
        static LEAKED: RwLock<Vec<Option<&'static FontFace>>> = RwLock::new(Vec::new());
        let id = face.id as usize;
        if let Some(Some(f)) = LEAKED.read().unwrap_or_else(|e| e.into_inner()).get(id) {
            return FaceRef(f);
        }
        let mut w = LEAKED.write().unwrap_or_else(|e| e.into_inner());
        if w.len() <= id {
            w.resize(id + 1, None);
        }
        let f: &'static FontFace = w[id].unwrap_or_else(|| {
            let keep: &'static Arc<FontFace> = Box::leak(Box::new(face.clone()));
            keep
        });
        w[id] = Some(f);
        FaceRef(f)
    }
    pub fn get(self) -> &'static FontFace {
        self.0
    }
}

impl std::ops::Deref for FaceRef {
    type Target = FontFace;
    fn deref(&self) -> &FontFace {
        self.0
    }
}

impl std::fmt::Debug for FaceRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl PartialEq for FaceRef {
    fn eq(&self, o: &FaceRef) -> bool {
        self.0.id == o.0.id
    }
}

impl std::fmt::Debug for FontFace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "FontFace({} {})", self.family, self.style)
    }
}

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
    /// Does the face map `c` to a glyph?
    pub fn covers(&self, c: char) -> bool {
        let cp = c as u32;
        if cp < 0x1_0000 {
            let bits = self.bmp.get_or_init(|| {
                let mut b = vec![0u64; 1024].into_boxed_slice();
                if let Some(f) = self.skrifa() {
                    for (cp, _) in f.charmap().mappings() {
                        if cp < 0x1_0000 {
                            b[(cp / 64) as usize] |= 1 << (cp % 64);
                        }
                    }
                }
                b
            });
            return bits[(cp / 64) as usize] & (1 << (cp % 64)) != 0;
        }
        self.skrifa().is_some_and(|f| f.charmap().map(c).is_some())
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
    /// Glyph id for `c` (0 = .notdef).
    pub fn glyph_for(&self, c: char) -> u32 {
        self.skrifa().and_then(|f| f.charmap().map(c)).map(|g| g.to_u32()).unwrap_or(0)
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Debug)]
struct CatalogEntry {
    family: String,
    style: String,
    path: std::path::PathBuf,
}

/// Process-wide font database.
pub struct FontDb {
    faces: RwLock<Vec<Arc<FontFace>>>,
    outlines: Mutex<HashMap<(u32, u32), Arc<BezPath>>>,
    #[cfg(not(target_arch = "wasm32"))]
    catalog: RwLock<Vec<CatalogEntry>>,
    /// System fallback state: scanned yet, and characters no system font covers.
    #[cfg(not(target_arch = "wasm32"))]
    sys: Mutex<SysFallback>,
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Default)]
struct SysFallback {
    enabled: bool,
    scanned: bool,
    misses: std::collections::HashSet<char>,
}

/// Families tried (when installed) for characters the loaded fonts lack: CJK, symbols, emoji.
#[cfg(not(target_arch = "wasm32"))]
const SYSTEM_FALLBACKS: &[&str] = &[
    "Helvetica Neue",
    "Arial",
    "Segoe UI",
    "Noto Sans",
    "DejaVu Sans",
    "PingFang SC",
    "Hiragino Sans",
    "Hiragino Kaku Gothic ProN",
    "Apple SD Gothic Neo",
    "Heiti SC",
    "STHeiti",
    "Microsoft YaHei",
    "Yu Gothic",
    "Malgun Gothic",
    "Noto Sans CJK SC",
    "Noto Sans CJK JP",
    "Arial Unicode MS",
    "Apple Symbols",
    "Segoe UI Symbol",
    "Noto Sans Symbols",
    "Noto Sans Symbols2",
    "Noto Emoji",
    "Segoe UI Emoji",
    "Apple Color Emoji",
    "Noto Color Emoji",
];

static NEXT_ID: AtomicU32 = AtomicU32::new(1);
const OUTLINE_CACHE_MAX: usize = 50_000;

fn name(font: &skrifa::FontRef<'_>, ids: &[StringId]) -> Option<String> {
    ids.iter().find_map(|id| font.localized_strings(*id).english_or_first().map(|s| s.to_string()).filter(|s| !s.is_empty()))
}

/// One face found in a font file: index, family, style and (variable fonts) the named instance's
/// axis settings.
type Found = (u32, String, String, Vec<([u8; 4], f32)>);

/// Parse every face in `data` (a font file or collection); a variable font yields one face per
/// named instance (InDesign lists them as styles).
fn enumerate_faces(data: &[u8]) -> Vec<Found> {
    let count = match FileRef::new(data) {
        Ok(FileRef::Font(_)) => 1,
        Ok(FileRef::Collection(c)) => c.len(),
        Err(_) => 0,
    };
    let mut out = Vec::new();
    for i in 0..count {
        let Ok(f) = skrifa::FontRef::from_index(data, i) else { continue };
        let Some(family) = name(&f, &[StringId::TYPOGRAPHIC_FAMILY_NAME, StringId::FAMILY_NAME]) else { continue };
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
            let style = name(&f, &[StringId::TYPOGRAPHIC_SUBFAMILY_NAME, StringId::SUBFAMILY_NAME]).unwrap_or_else(|| "Regular".into());
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
        let face = make_face(FontBytes::Static(BUNDLED[0]), 0, FALLBACK_FAMILY.into(), "Regular".into(), Vec::new())
            .expect("the compiled-in Source Sans 3 Regular parses");
        Arc::new(face)
    })
    .clone()
}

fn make_face(bytes: FontBytes, index: u32, family: String, style: String, coords: Vec<([u8; 4], f32)>) -> Option<FontFace> {
    let data: &[u8] = match &bytes {
        FontBytes::Static(b) => b,
        FontBytes::Owned(v) => v.as_slice(),
    };
    let f = skrifa::FontRef::from_index(data, index).ok()?;
    let settings: Vec<(skrifa::Tag, f32)> = coords.iter().map(|(t, v)| (skrifa::Tag::new(t), *v)).collect();
    let location = if coords.is_empty() { Location::default() } else { f.axes().location(settings.iter().copied()) };
    let m = f.metrics(Size::unscaled(), &location);
    let a = f.attributes();
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
    Some(FontFace {
        id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
        family,
        style,
        weight,
        italic,
        upem: m.units_per_em.max(1) as f64,
        ascent: m.ascent as f64,
        descent: -(m.descent as f64),
        cap_height: m.cap_height.map(|v| v as f64).filter(|v| *v > 0.0).unwrap_or(m.ascent as f64 * 0.72),
        x_height: m.x_height.map(|v| v as f64).filter(|v| *v > 0.0).unwrap_or(m.ascent as f64 * 0.5),
        shaper,
        coords,
        location,
        instance,
        bytes,
        index,
        bmp: std::sync::OnceLock::new(),
    })
}

/// The named style part of a style with axis settings (`Bold {wght:650}` → `Bold`).
pub fn base_style(style: &str) -> &str {
    style.split('{').next().unwrap_or(style).trim()
}

fn norm(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(|c| c.to_lowercase()).collect()
}

/// Weight implied by a style name.
fn style_weight(style: &str) -> f32 {
    let s = norm(style);
    const TABLE: &[(&str, f32)] = &[
        ("extralight", 200.0),
        ("ultralight", 200.0),
        ("semibold", 600.0),
        ("demibold", 600.0),
        ("extrabold", 800.0),
        ("ultrabold", 800.0),
        ("hairline", 100.0),
        ("thin", 100.0),
        ("light", 300.0),
        ("medium", 500.0),
        ("bold", 700.0),
        ("black", 900.0),
        ("heavy", 900.0),
    ];
    TABLE.iter().find(|(k, _)| s.contains(k)).map(|(_, w)| *w).unwrap_or(400.0)
}

fn style_italic(style: &str) -> bool {
    let s = norm(style);
    s.contains("italic") || s.contains("oblique") || s == "it"
}

impl FontDb {
    fn new_bundled() -> Self {
        let mut faces = Vec::new();
        for data in BUNDLED {
            for (i, family, style, coords) in enumerate_faces(data) {
                if let Some(f) = make_face(FontBytes::Static(data), i, family, style, coords) {
                    faces.push(Arc::new(f));
                }
            }
        }
        Self {
            faces: RwLock::new(faces),
            outlines: Mutex::new(HashMap::new()),
            #[cfg(not(target_arch = "wasm32"))]
            catalog: RwLock::new(Vec::new()),
            #[cfg(not(target_arch = "wasm32"))]
            sys: Mutex::new(SysFallback { enabled: true, ..Default::default() }),
        }
    }

    /// Enable or disable the lazy system-font fallback for characters the loaded fonts lack
    /// (native only; on by default).
    pub fn set_system_fallback(&self, on: bool) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.sys.lock().unwrap_or_else(|e| e.into_inner()).enabled = on;
        }
        #[cfg(target_arch = "wasm32")]
        let _ = on;
    }

    /// Process-wide database preloaded with the bundled fonts.
    pub fn global() -> &'static FontDb {
        static DB: std::sync::OnceLock<FontDb> = std::sync::OnceLock::new();
        DB.get_or_init(FontDb::new_bundled)
    }

    fn read_faces(&self) -> std::sync::RwLockReadGuard<'_, Vec<Arc<FontFace>>> {
        self.faces.read().unwrap_or_else(|e| e.into_inner())
    }

    /// Family names available (loaded plus cataloged system fonts), sorted and deduplicated.
    pub fn families(&self) -> Vec<String> {
        let mut v: Vec<String> = self.read_faces().iter().map(|f| f.family.clone()).collect();
        #[cfg(not(target_arch = "wasm32"))]
        v.extend(self.catalog.read().unwrap_or_else(|e| e.into_inner()).iter().map(|c| c.family.clone()));
        v.sort_by_key(|a| a.to_lowercase());
        v.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
        v
    }

    /// Style names available for `family` (Regular first, then by weight).
    pub fn styles(&self, family: &str) -> Vec<String> {
        let mut v: Vec<(bool, f32, String)> = self
            .read_faces()
            .iter()
            .filter(|f| f.family.eq_ignore_ascii_case(family) && !f.style.contains('{'))
            .map(|f| (f.italic, f.weight, f.style.clone()))
            .collect();
        #[cfg(not(target_arch = "wasm32"))]
        for c in self.catalog.read().unwrap_or_else(|e| e.into_inner()).iter() {
            if c.family.eq_ignore_ascii_case(family) && !v.iter().any(|(_, _, s)| s.eq_ignore_ascii_case(&c.style)) {
                v.push((style_italic(&c.style), style_weight(&c.style), c.style.clone()));
            }
        }
        v.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)).then(a.2.cmp(&b.2)));
        v.dedup_by(|a, b| a.2 == b.2);
        v.into_iter().map(|t| t.2).collect()
    }

    /// Add a user font (TTF/OTF/TTC bytes). Returns the number of faces added (0 if unparseable or
    /// every face was already present).
    pub fn add_font(&self, bytes: Vec<u8>) -> usize {
        let data = Arc::new(bytes);
        let mut added = 0;
        for (i, family, style, coords) in enumerate_faces(&data) {
            if self.read_faces().iter().any(|f| f.family.eq_ignore_ascii_case(&family) && f.style.eq_ignore_ascii_case(&style)) {
                continue;
            }
            if let Some(f) = make_face(FontBytes::Owned(data.clone()), i, family, style, coords) {
                self.faces.write().unwrap_or_else(|e| e.into_inner()).push(Arc::new(f));
                added += 1;
            }
        }
        added
    }

    /// Scan the platform's font directories and catalog the faces found (native only; not called by
    /// default). Font data is loaded lazily when a cataloged family is first resolved. Returns the
    /// number of faces cataloged.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn load_system_fonts(&self) -> usize {
        let mut dirs: Vec<std::path::PathBuf> = Vec::new();
        let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
        if cfg!(target_os = "macos") {
            dirs.extend(["/System/Library/Fonts", "/Library/Fonts"].map(Into::into));
            if let Some(h) = &home {
                dirs.push(h.join("Library/Fonts"));
            }
        } else if cfg!(windows) {
            let root = std::env::var_os("WINDIR").map(std::path::PathBuf::from).unwrap_or_else(|| "C:\\Windows".into());
            dirs.push(root.join("Fonts"));
            if let Some(l) = std::env::var_os("LOCALAPPDATA") {
                dirs.push(std::path::PathBuf::from(l).join("Microsoft\\Windows\\Fonts"));
            }
        } else {
            dirs.extend(["/usr/share/fonts", "/usr/local/share/fonts"].map(Into::into));
            if let Some(h) = &home {
                dirs.push(h.join(".fonts"));
                dirs.push(h.join(".local/share/fonts"));
            }
        }
        let mut found = Vec::new();
        let mut stack = dirs;
        while let Some(d) = stack.pop() {
            let Ok(rd) = std::fs::read_dir(&d) else { continue };
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                    continue;
                }
                let ext = p.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase());
                if !matches!(ext.as_deref(), Some("ttf" | "otf" | "ttc" | "otc")) {
                    continue;
                }
                let Ok(data) = std::fs::read(&p) else { continue };
                for (_, family, style, _) in enumerate_faces(&data) {
                    found.push(CatalogEntry { family, style, path: p.clone() });
                }
            }
        }
        let n = found.len();
        log::debug!("cataloged {n} system font faces");
        *self.catalog.write().unwrap_or_else(|e| e.into_inner()) = found;
        n
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn load_cataloged(&self, family: &str) -> bool {
        let paths: Vec<std::path::PathBuf> = {
            let cat = self.catalog.read().unwrap_or_else(|e| e.into_inner());
            let mut p: Vec<_> = cat.iter().filter(|c| c.family.eq_ignore_ascii_case(family)).map(|c| c.path.clone()).collect();
            p.dedup();
            p
        };
        let mut any = false;
        for p in paths {
            if let Ok(data) = std::fs::read(&p) {
                any |= self.add_font(data) > 0;
            }
        }
        any
    }

    /// Resolve a family + style to a face, falling back to the closest style of the family, then to
    /// Source Sans 3 Regular.
    pub fn face(&self, family: &str, style: &str) -> Arc<FontFace> {
        if let Some(f) = self.find(family, style) {
            return f;
        }
        #[cfg(not(target_arch = "wasm32"))]
        if self.load_cataloged(family)
            && let Some(f) = self.find(family, style)
        {
            return f;
        }
        self.find(FALLBACK_FAMILY, style)
            .or_else(|| self.find(FALLBACK_FAMILY, "Regular"))
            .or_else(|| self.read_faces().first().cloned())
            .unwrap_or_else(last_resort_face)
    }

    /// Is `family` available (loaded)?
    pub fn has_family(&self, family: &str) -> bool {
        self.read_faces().iter().any(|f| f.family.eq_ignore_ascii_case(family))
    }

    /// A variable font's axes: (tag, name, min, default, max), empty for static fonts.
    pub fn axes(&self, family: &str, style: &str) -> Vec<(String, String, f32, f32, f32)> {
        let face = self.face(family, base_style(style));
        let Some(f) = face.skrifa() else { return vec![] };
        f.axes()
            .iter()
            .map(|a| {
                let tag = String::from_utf8_lossy(&a.tag().to_be_bytes()).to_string();
                let name = f.localized_strings(a.name_id()).english_or_first().map(|s| s.to_string()).unwrap_or_else(|| tag.clone());
                (tag, name, a.min_value(), a.default_value(), a.max_value())
            })
            .collect()
    }

    /// `Style {wght:650,wdth:90}`: the named style's font at those axis values, made on first use.
    fn instance(&self, family: &str, style: &str) -> Option<Arc<FontFace>> {
        let open = style.find('{')?;
        let base = self.find(family, style[..open].trim())?;
        if base.skrifa().is_none_or(|f| f.axes().is_empty()) {
            return Some(base);
        }
        let mut coords = base.coords.clone();
        for kv in style[open + 1..].trim_end_matches('}').split(',') {
            let (k, v) = kv.split_once(':')?;
            let k = k.trim().as_bytes();
            let v: f32 = v.trim().parse().ok()?;
            if k.len() != 4 {
                return None;
            }
            let tag = [k[0], k[1], k[2], k[3]];
            match coords.iter_mut().find(|(t, _)| *t == tag) {
                Some(c) => c.1 = v,
                None => coords.push((tag, v)),
            }
        }
        let f = make_face(base.bytes.clone(), base.index, base.family.clone(), style.to_string(), coords)?;
        let f = Arc::new(f);
        self.faces.write().unwrap_or_else(|e| e.into_inner()).push(f.clone());
        Some(f)
    }

    fn find(&self, family: &str, style: &str) -> Option<Arc<FontFace>> {
        if style.contains('{') {
            {
                let faces = self.read_faces();
                if let Some(f) = faces.iter().find(|f| f.family.eq_ignore_ascii_case(family) && f.style == style) {
                    return Some(f.clone());
                }
            }
            return self.instance(family, style);
        }
        let faces = self.read_faces();
        let cands: Vec<&Arc<FontFace>> = faces.iter().filter(|f| f.family.eq_ignore_ascii_case(family)).collect();
        if cands.is_empty() {
            return None;
        }
        let ns = norm(style);
        if let Some(f) = cands.iter().find(|f| norm(&f.style) == ns) {
            return Some((*f).clone());
        }
        let (tw, ti) = (style_weight(style), style_italic(style));
        cands
            .iter()
            .min_by(|a, b| {
                let sa = (a.weight - tw).abs() + if a.italic != ti { 1000.0 } else { 0.0 };
                let sb = (b.weight - tw).abs() + if b.italic != ti { 1000.0 } else { 0.0 };
                sa.total_cmp(&sb)
            })
            .map(|f| (*f).clone())
    }

    /// First face (fallback family first, then load order) that covers `c`; on native, system
    /// fonts are cataloged and loaded lazily the first time no loaded face covers a character.
    pub fn fallback_for(&self, c: char, exclude: u32) -> Option<Arc<FontFace>> {
        if let Some(f) = self.loaded_fallback(c, exclude) {
            return Some(f);
        }
        #[cfg(not(target_arch = "wasm32"))]
        if self.system_fallback(c) {
            return self.loaded_fallback(c, exclude);
        }
        None
    }

    fn loaded_fallback(&self, c: char, exclude: u32) -> Option<Arc<FontFace>> {
        let faces = self.read_faces();
        let mut order: Vec<&Arc<FontFace>> = faces.iter().filter(|f| f.id != exclude).collect();
        order.sort_by_key(|f| (!f.family.eq_ignore_ascii_case(FALLBACK_FAMILY), f.italic, (f.weight - 400.0).abs() as i32));
        order.into_iter().find(|f| f.covers(c)).cloned()
    }

    /// Load a system font covering `c` (preferred fallback families first, then any cataloged
    /// file under 40 MB). Returns true if one was loaded. Misses are remembered.
    #[cfg(not(target_arch = "wasm32"))]
    fn system_fallback(&self, c: char) -> bool {
        if c.is_control() || c.is_whitespace() {
            return false;
        }
        {
            let mut sys = self.sys.lock().unwrap_or_else(|e| e.into_inner());
            if !sys.enabled || sys.misses.contains(&c) {
                return false;
            }
            if !sys.scanned {
                sys.scanned = true;
                if self.catalog.read().unwrap_or_else(|e| e.into_inner()).is_empty() {
                    drop(sys);
                    self.load_system_fonts();
                }
            }
        }
        let covered = |db: &FontDb| db.read_faces().iter().any(|f| f.covers(c));
        let cataloged: Vec<String> = self.catalog.read().unwrap_or_else(|e| e.into_inner()).iter().map(|e| e.family.clone()).collect();
        for fam in SYSTEM_FALLBACKS {
            if !self.has_family(fam) && cataloged.iter().any(|f| f.eq_ignore_ascii_case(fam)) {
                self.load_cataloged(fam);
                if covered(self) {
                    return true;
                }
            }
        }
        let mut paths: Vec<std::path::PathBuf> = self.catalog.read().unwrap_or_else(|e| e.into_inner()).iter().map(|e| e.path.clone()).collect();
        paths.sort();
        paths.dedup();
        for p in paths {
            if std::fs::metadata(&p).map(|m| m.len() > 40 << 20).unwrap_or(true) {
                continue;
            }
            let Ok(data) = std::fs::read(&p) else { continue };
            let hit =
                enumerate_faces(&data).iter().any(|(i, _, _, _)| skrifa::FontRef::from_index(&data, *i).is_ok_and(|f| f.charmap().map(c).is_some()));
            if hit && self.add_font(data) > 0 && covered(self) {
                return true;
            }
        }
        self.sys.lock().unwrap_or_else(|e| e.into_inner()).misses.insert(c);
        false
    }

    /// Glyph outline in font units, y-down (flipped), cached per (face, glyph).
    pub fn outline(&self, face: &FontFace, gid: u32) -> Arc<BezPath> {
        let key = (face.id, gid);
        if let Some(p) = self.outlines.lock().unwrap_or_else(|e| e.into_inner()).get(&key) {
            return p.clone();
        }
        let mut pen = FlipPen(BezPath::new());
        if let Some(f) = face.skrifa()
            && let Some(g) = f.outline_glyphs().get(GlyphId::new(gid))
        {
            let _ = g.draw(DrawSettings::unhinted(Size::unscaled(), face.location()), &mut pen);
        }
        let p = Arc::new(pen.0);
        let mut cache = self.outlines.lock().unwrap_or_else(|e| e.into_inner());
        if cache.len() >= OUTLINE_CACHE_MAX {
            cache.clear();
        }
        cache.insert(key, p.clone());
        p
    }
}

struct FlipPen(BezPath);

impl OutlinePen for FlipPen {
    fn move_to(&mut self, x: f32, y: f32) {
        self.0.move_to((x as f64, -y as f64));
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.0.line_to((x as f64, -y as f64));
    }
    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        self.0.quad_to((cx0 as f64, -cy0 as f64), (x as f64, -y as f64));
    }
    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        self.0.curve_to((cx0 as f64, -cy0 as f64), (cx1 as f64, -cy1 as f64), (x as f64, -y as f64));
    }
    fn close(&mut self) {
        self.0.close_path();
    }
}
