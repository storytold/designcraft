//! Font database: bundled OFL fonts, user fonts, the installed system fonts (cataloged the first
//! time a lookup needs them, native only), outline cache.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use kurbo::BezPath;
use skrifa::instance::{Location, LocationRef, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::raw::FileRef;
use skrifa::string::StringId;
use skrifa::{GlyphId, MetadataProvider};

use crate::group::{FamilyInfo, FontGroup};

/// The family used when a requested family is unknown (the UI sans).
pub const FALLBACK_FAMILY: &str = "Source Sans 3";

/// The bundled fonts (OFL), as font file bytes. Japanese fonts are not bundled: they come from
/// the optional craft-fonts build input ([`crate::japanese_document_fonts`]).
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
];

#[derive(Clone)]
enum FontBytes {
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
    /// An installed font file (the system's or the user's font folders).
    Installed(std::path::PathBuf),
    /// A font file in a document's `Document Fonts` folder ([`FontDb::load_document_fonts`]).
    Document(std::path::PathBuf),
}

impl FontSource {
    /// The font file, if the face was loaded from one.
    pub fn path(&self) -> Option<&std::path::Path> {
        match self {
            FontSource::Installed(p) | FontSource::Document(p) => Some(p),
            FontSource::Bundled | FontSource::Memory => None,
        }
    }
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
    id: u32,
    /// Typographic family name (e.g. "Source Sans 3").
    pub family: String,
    /// Typographic style name (e.g. "Semibold", "Italic").
    pub style: String,
    /// usWeightClass-style weight (400 = regular).
    pub weight: f32,
    pub italic: bool,
    /// Where the font came from (a file to copy when packaging, a document's own font).
    pub source: FontSource,
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
    /// The ideographic em box, read on first use.
    em: std::sync::OnceLock<(f64, f64)>,
    /// The font menu group and native family name, read on first use.
    group: std::sync::OnceLock<(FontGroup, Option<String>)>,
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
    /// The ideographic em box (top, bottom) in font units, y up (Japanese fonts: typically 880,
    /// -120): `BASE` `idtp`/`ideo`, else the OS/2 typo ascender/descender, else the ascender and
    /// descender, the last two centred on one em.
    pub fn em_box(&self) -> (f64, f64) {
        *self.em.get_or_init(|| crate::vertical::em_box(self))
    }
    /// The font menu group the font is for and, for a CJK group, its family name in that language
    /// (see [`crate::FamilyInfo`]); read from the font once.
    pub fn menu_group(&self) -> (FontGroup, Option<&str>) {
        let (g, n) = self.group.get_or_init(|| self.skrifa().map(|f| crate::group::classify(&f, &self.family)).unwrap_or_default());
        (*g, n.as_deref())
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
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Debug)]
struct CatalogEntry {
    family: String,
    style: String,
    path: std::path::PathBuf,
    /// The font menu group and native family name, read by the scan.
    group: FontGroup,
    native: Option<String>,
}

/// What the scan reads of one face in a font file: its names and font menu group.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Debug, PartialEq)]
struct ScannedFace {
    family: String,
    style: String,
    group: FontGroup,
    native: Option<String>,
}

/// Process-wide font database: the fonts every document shares (bundled, installed, added), and
/// each open document's own fonts under its scope (see [`FontDb::scoped`]).
pub struct FontDb {
    faces: RwLock<Vec<Arc<FontFace>>>,
    /// Each open document's fonts (its `Document Fonts` folder), by scope.
    scopes: RwLock<HashMap<u32, Arc<[Arc<FontFace>]>>>,
    /// The document font files read, by path: (length, modification time, faces). A document
    /// opened again finds its unchanged files here instead of reading them again.
    #[cfg(not(target_arch = "wasm32"))]
    document_files: Mutex<HashMap<std::path::PathBuf, (u64, Option<std::time::SystemTime>, Vec<Arc<FontFace>>)>>,
    /// Variable font instances (`Style {wght:650}`), made on first use, by base face and style.
    instances: RwLock<HashMap<(u32, String), Arc<FontFace>>>,
    outlines: Mutex<HashMap<(u32, u32), Arc<BezPath>>>,
    #[cfg(not(target_arch = "wasm32"))]
    catalog: RwLock<Vec<CatalogEntry>>,
    /// The folders the system font scan reads (the platform's font folders for [`FontDb::global`]).
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    font_dirs: Vec<std::path::PathBuf>,
    /// Set once the font folders have been scanned. Lookups by family name wait for the first
    /// scan, so what they find doesn't depend on what ran before them.
    #[cfg(not(target_arch = "wasm32"))]
    cataloged: std::sync::OnceLock<()>,
    /// System fallback state: enabled, and characters no system font covers.
    #[cfg(not(target_arch = "wasm32"))]
    sys: Mutex<SysFallback>,
    /// Pause one fallback after copying catalog paths, with no database locks held.
    #[cfg(all(test, target_os = "linux"))]
    fallback_snapshot_hook: Mutex<Option<FallbackSnapshotHook>>,
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Default)]
struct SysFallback {
    enabled: bool,
    /// Rescan generation: an older in-flight lookup must not restore a cleared miss.
    generation: u64,
    misses: std::collections::HashSet<char>,
    /// CJK chain families already looked up in the catalog (loaded, or not installed).
    chain_tried: std::collections::HashSet<&'static str>,
}

#[cfg(all(test, target_os = "linux"))]
struct FallbackSnapshotHook {
    reached: std::sync::mpsc::SyncSender<()>,
    resume: Arc<std::sync::Barrier>,
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

/// CJK fallback chains: the families tried first (user-added, else installed) for characters of
/// each language that the primary font lacks. Each lists macOS, then Windows, then Linux /
/// cross-platform families, serif first; a platform only has its own installed. Japanese tries
/// the craft-fonts Japanese faces before these ([`japanese_chain`]).
const JAPANESE_FALLBACKS: &[&str] = &[
    "Hiragino Mincho ProN",
    "YuMincho",
    "Hiragino Sans",
    "Yu Mincho",
    "MS Mincho",
    "Yu Gothic",
    "Noto Serif CJK JP",
    "Source Han Serif JP",
    "Noto Serif JP",
    "Noto Sans CJK JP",
    "Source Han Sans JP",
];
const SIMPLIFIED_CHINESE_FALLBACKS: &[&str] = &[
    "Songti SC",
    "STSong",
    "PingFang SC",
    "SimSun",
    "Microsoft YaHei",
    "Noto Serif CJK SC",
    "Source Han Serif SC",
    "Noto Serif SC",
    "Noto Sans CJK SC",
    "Source Han Sans SC",
];
const TRADITIONAL_CHINESE_FALLBACKS: &[&str] = &[
    "Songti TC",
    "PingFang TC",
    "PMingLiU",
    "MingLiU",
    "Microsoft JhengHei",
    "Noto Serif CJK TC",
    "Source Han Serif TC",
    "Noto Serif TC",
    "Noto Sans CJK TC",
    "Source Han Sans TC",
];
const KOREAN_FALLBACKS: &[&str] = &[
    "AppleMyungjo",
    "Apple SD Gothic Neo",
    "Batang",
    "Malgun Gothic",
    "Noto Serif CJK KR",
    "Source Han Serif K",
    "Noto Serif KR",
    "Noto Sans CJK KR",
    "Source Han Sans K",
];

/// The Japanese fallback chain: the craft-fonts Japanese faces' families in document order
/// (Mincho first; none without craft-fonts), then [`JAPANESE_FALLBACKS`].
fn japanese_chain() -> &'static [&'static str] {
    static CHAIN: std::sync::OnceLock<Vec<&'static str>> = std::sync::OnceLock::new();
    CHAIN.get_or_init(|| {
        let mut chain: Vec<&'static str> = Vec::new();
        for f in crate::japanese_document_fonts() {
            if !chain.contains(&f.family) {
                chain.push(f.family);
            }
        }
        chain.extend_from_slice(JAPANESE_FALLBACKS);
        chain
    })
}

/// The CJK fallback chain for `c` in `language` (a BCP 47 tag). The script decides where it is
/// unambiguous (Hangul is Korean, kana Japanese, Bopomofo Traditional Chinese); ideographs and CJK
/// punctuation follow the language, and have no chain without a CJK one.
fn cjk_chain(c: char, language: Option<&str>) -> Option<&'static [&'static str]> {
    match c as u32 {
        0x1100..=0x11FF | 0x3130..=0x318F | 0xA960..=0xA97F | 0xAC00..=0xD7FF | 0xFFA0..=0xFFDC => return Some(KOREAN_FALLBACKS),
        0x3040..=0x30FF | 0x31F0..=0x31FF | 0xFF66..=0xFF9F | 0x1B000..=0x1B16F => return Some(japanese_chain()),
        0x3100..=0x312F | 0x31A0..=0x31BF => return Some(TRADITIONAL_CHINESE_FALLBACKS),
        // Radicals, CJK symbols and punctuation, kanbun, strokes, enclosed and compatibility
        // characters, ideographs, compatibility and vertical forms, full-width forms.
        0x2E80..=0x2FDF
        | 0x3000..=0x303F
        | 0x3190..=0x319F
        | 0x31C0..=0x31EF
        | 0x3200..=0x33FF
        | 0x3400..=0x4DBF
        | 0x4E00..=0x9FFF
        | 0xF900..=0xFAFF
        | 0xFE10..=0xFE1F
        | 0xFE30..=0xFE4F
        | 0xFF00..=0xFFEF
        | 0x20000..=0x3FFFF => {}
        _ => return None,
    }
    // The language decides, whatever the region (`ja-JP`, `ko-KR`).
    let language = language?;
    match language.split('-').next()? {
        "ja" => Some(japanese_chain()),
        "ko" => Some(KOREAN_FALLBACKS),
        "zh" if language.starts_with("zh-Hant") => Some(TRADITIONAL_CHINESE_FALLBACKS),
        "zh" => Some(SIMPLIFIED_CHINESE_FALLBACKS),
        _ => None,
    }
}

static NEXT_ID: AtomicU32 = AtomicU32::new(1);
/// Scopes are never reused: a closed document's scope finds no fonts ever after.
#[cfg(not(target_arch = "wasm32"))]
static NEXT_SCOPE: AtomicU32 = AtomicU32::new(1);
const OUTLINE_CACHE_MAX: usize = 50_000;

fn name(font: &skrifa::FontRef<'_>, ids: &[StringId]) -> Option<String> {
    ids.iter().find_map(|id| font.localized_strings(*id).english_or_first().map(|s| s.to_string()).filter(|s| !s.is_empty()))
}

/// A face's family and style names (the default instance's style for a variable font).
/// Is a face of `family` and `style` among `faces`?
fn has_face(faces: &[Arc<FontFace>], family: &str, style: &str) -> bool {
    faces.iter().any(|f| f.family.eq_ignore_ascii_case(family) && f.style.eq_ignore_ascii_case(style))
}

fn face_names(f: &skrifa::FontRef<'_>) -> Option<(String, String)> {
    let family = name(f, &[StringId::TYPOGRAPHIC_FAMILY_NAME, StringId::FAMILY_NAME])?;
    let style = name(f, &[StringId::TYPOGRAPHIC_SUBFAMILY_NAME, StringId::SUBFAMILY_NAME]).unwrap_or_else(|| "Regular".into());
    Some((family, style))
}

/// The names and font menu group of every face in the font file at `path`, reading only its table
/// directories and the tables that name and classify it (`name`, `OS/2`, `meta`; `cmap` only for a
/// font that declares no language or code page): a scan opens hundreds of font files, many of them
/// megabytes long. A variable font is cataloged under its default style; its named instances
/// appear once it loads.
#[cfg(not(target_arch = "wasm32"))]
fn file_face_names(path: &std::path::Path) -> Vec<ScannedFace> {
    use std::io::{Read, Seek, SeekFrom};
    /// Caps on what a (possibly damaged) file can make the scan read.
    const MAX_FACES: u32 = 256;
    const MAX_NAME_TABLE: u32 = 1 << 20;
    const MAX_SMALL_TABLE: u32 = 64 << 10;
    const MAX_CMAP: u32 = 4 << 20;
    let Ok(mut file) = std::fs::File::open(path) else { return vec![] };
    let mut read_at = |offset: u64, len: usize| -> Option<Vec<u8>> {
        let mut buf = vec![0; len];
        file.seek(SeekFrom::Start(offset)).ok()?;
        file.read_exact(&mut buf).ok()?;
        Some(buf)
    };
    let be32 = |b: &[u8], at: usize| b.get(at..at + 4).and_then(|s| s.try_into().ok()).map(u32::from_be_bytes);
    let Some(head) = read_at(0, 12) else { return vec![] };
    // A collection lists where each face's table directory starts.
    let starts: Vec<u32> = if head.starts_with(b"ttcf") {
        let n = be32(&head, 8).unwrap_or(0).min(MAX_FACES) as usize;
        read_at(12, n * 4).map(|b| b.as_chunks::<4>().0.iter().map(|c| u32::from_be_bytes(*c)).collect()).unwrap_or_default()
    } else {
        vec![0]
    };
    starts
        .into_iter()
        .filter_map(|start| {
            let dir = read_at(start.into(), 12)?;
            let tables = u16::from_be_bytes(dir.get(4..6)?.try_into().ok()?) as usize;
            let records = read_at(u64::from(start) + 12, tables * 16)?;
            // Table `tag`, unless missing or over `max` bytes.
            let mut table = |tag: &[u8; 4], max: u32| -> Option<([u8; 4], Vec<u8>)> {
                let rec = records.as_chunks::<16>().0.iter().find(|r| r.starts_with(tag))?;
                let (offset, len) = (be32(rec, 8)?, be32(rec, 12)?);
                if len > max {
                    return None;
                }
                Some((*tag, read_at(offset.into(), len as usize)?))
            };
            // A font holding just these tables, to read them as the font itself would.
            let mut found = vec![table(b"name", MAX_NAME_TABLE)?];
            found.extend(table(b"OS/2", MAX_SMALL_TABLE));
            found.extend(table(b"meta", MAX_SMALL_TABLE));
            let font = crate::group::sfnt(found.clone())?;
            let f = skrifa::FontRef::new(&font).ok()?;
            let (family, style) = face_names(&f)?;
            let (group, native) = match crate::group::declared_group(&f) {
                Some(g) => (g, crate::group::classify(&f, &family).1),
                None => {
                    found.extend(table(b"cmap", MAX_CMAP));
                    let font = crate::group::sfnt(found)?;
                    crate::group::classify(&skrifa::FontRef::new(&font).ok()?, &family)
                }
            };
            Some(ScannedFace { family, style, group, native })
        })
        .collect()
}

/// The platform's font folders (the system's and the user's), scanned by [`FontDb::global`].
/// Empty on wasm.
pub fn system_font_dirs() -> Vec<std::path::PathBuf> {
    let mut dirs: Vec<std::path::PathBuf> = Vec::new();
    if cfg!(target_arch = "wasm32") {
        return dirs;
    }
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    if cfg!(target_os = "macos") {
        dirs.extend(["/System/Library/Fonts", "/Library/Fonts"].map(Into::into));
        if let Some(h) = &home {
            dirs.push(h.join("Library/Fonts"));
        }
        // Fonts macOS ships as assets (PingFang, Yu Mincho, …): com_apple_MobileAsset_Font<N>.
        for parent in ["/System/Library/AssetsV2", "/System/Library/AssetsV2/PreinstalledAssetsV2/InstallWithOs"] {
            let Ok(entries) = std::fs::read_dir(parent) else { continue };
            for e in entries.flatten() {
                if e.file_name().to_string_lossy().starts_with("com_apple_MobileAsset_Font") {
                    dirs.push(e.path());
                }
            }
        }
        // Microsoft Office bundles its fonts (Calibri, Cambria, …) inside each app; read them in
        // place from the first installed one (they share the same set).
        let office = ["Microsoft Word", "Microsoft Excel", "Microsoft PowerPoint", "Microsoft Outlook"]
            .iter()
            .map(|app| std::path::PathBuf::from(format!("/Applications/{app}.app/Contents/Resources/DFonts")))
            .find(|p| p.is_dir());
        dirs.extend(office);
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
    dirs
}

/// One face found in a font file: index, family, style and (variable fonts) the named instance's
/// axis settings.
type Found = (u32, String, String, Vec<([u8; 4], f32)>);

/// Parse every face in `data` (a font file or collection); a variable font yields one face per
/// named instance (InDesign lists them as styles).
fn enumerate_faces(data: &[u8]) -> Vec<Found> {
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

fn make_face(bytes: FontBytes, source: FontSource, index: u32, family: String, style: String, coords: Vec<([u8; 4], f32)>) -> Option<FontFace> {
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
        source,
        bytes,
        index,
        bmp: std::sync::OnceLock::new(),
        em: std::sync::OnceLock::new(),
        group: std::sync::OnceLock::new(),
    })
}

/// `family` without the font-format suffix layout apps append when a family is installed in
/// several formats (`Minion Pro (OTF)` → `Minion Pro`), if it has one.
fn without_format_suffix(family: &str) -> Option<&str> {
    const FORMATS: [&str; 5] = ["OTF", "TT", "TTF", "TTC", "T1"];
    let (base, suffix) = family.trim_end().strip_suffix(')')?.rsplit_once('(')?;
    let base = base.trim_end();
    (!base.is_empty() && FORMATS.iter().any(|f| f.eq_ignore_ascii_case(suffix.trim()))).then_some(base)
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
    /// A database holding the bundled fonts, then the craft-fonts Japanese faces (Mincho first,
    /// so they are the fallback for Japanese text after the requested and bundled fonts; empty
    /// without `CRAFT_FONTS_DIR`), whose system font scan reads `font_dirs`.
    pub fn with_font_dirs(font_dirs: Vec<std::path::PathBuf>) -> Self {
        let mut faces = Vec::new();
        let craft = crate::japanese_document_fonts().into_iter().map(|f| f.bytes);
        for data in BUNDLED.iter().copied().chain(craft) {
            for (i, family, style, coords) in enumerate_faces(data) {
                if let Some(f) = make_face(FontBytes::Static(data), FontSource::Bundled, i, family, style, coords) {
                    faces.push(Arc::new(f));
                }
            }
        }
        Self {
            faces: RwLock::new(faces),
            scopes: RwLock::new(HashMap::new()),
            #[cfg(not(target_arch = "wasm32"))]
            document_files: Mutex::new(HashMap::new()),
            instances: RwLock::new(HashMap::new()),
            outlines: Mutex::new(HashMap::new()),
            #[cfg(not(target_arch = "wasm32"))]
            catalog: RwLock::new(Vec::new()),
            font_dirs,
            #[cfg(not(target_arch = "wasm32"))]
            cataloged: std::sync::OnceLock::new(),
            #[cfg(not(target_arch = "wasm32"))]
            sys: Mutex::new(SysFallback { enabled: true, ..Default::default() }),
            #[cfg(all(test, target_os = "linux"))]
            fallback_snapshot_hook: Mutex::new(None),
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

    /// Process-wide database: the bundled fonts, then the installed system fonts, cataloged the
    /// first time a lookup by family name needs them (or ahead of time by
    /// [`FontDb::scan_in_background`]).
    pub fn global() -> &'static FontDb {
        static DB: std::sync::OnceLock<FontDb> = std::sync::OnceLock::new();
        DB.get_or_init(|| FontDb::with_font_dirs(system_font_dirs()))
    }

    fn read_faces(&self) -> std::sync::RwLockReadGuard<'_, Vec<Arc<FontFace>>> {
        self.faces.read().unwrap_or_else(|e| e.into_inner())
    }

    /// The system font catalog, scanned first if it hasn't been yet.
    #[cfg(not(target_arch = "wasm32"))]
    fn read_catalog(&self) -> std::sync::RwLockReadGuard<'_, Vec<CatalogEntry>> {
        self.ensure_catalog();
        self.catalog.read().unwrap_or_else(|e| e.into_inner())
    }

    /// What the document with font scope `scope` sees: its own fonts ahead of the shared ones.
    /// Scope 0 (a document without fonts of its own) or a closed scope sees the shared fonts.
    pub fn scoped(&self, scope: u32) -> ScopedFonts<'_> {
        let own = if scope == 0 { None } else { self.scopes.read().unwrap_or_else(|e| e.into_inner()).get(&scope).cloned() };
        ScopedFonts { db: self, own }
    }

    /// The shared fonts' family names (see [`ScopedFonts::families`]).
    pub fn families(&self) -> Vec<String> {
        self.scoped(0).families()
    }

    /// The shared fonts' styles of `family` (see [`ScopedFonts::styles`]).
    pub fn styles(&self, family: &str) -> Vec<String> {
        self.scoped(0).styles(family)
    }

    /// Add a user font (TTF/OTF/TTC bytes). Returns the number of faces added (0 if unparseable or
    /// every face was already present).
    pub fn add_font(&self, bytes: Vec<u8>) -> usize {
        self.add_faces(bytes, FontSource::Memory)
    }

    /// Add the faces of a font file from `source` to the shared fonts, skipping the family and
    /// style already loaded.
    fn add_faces(&self, bytes: Vec<u8>, source: FontSource) -> usize {
        let faces = self.new_faces(bytes, source);
        self.insert_faces(faces)
    }

    /// The faces of a font file from `source` whose family and style aren't loaded yet.
    fn new_faces(&self, bytes: Vec<u8>, source: FontSource) -> Vec<FontFace> {
        let data = Arc::new(bytes);
        enumerate_faces(&data)
            .into_iter()
            .filter(|(_, family, style, _)| !has_face(&self.read_faces(), family, style))
            .filter_map(|(i, family, style, coords)| make_face(FontBytes::Owned(data.clone()), source.clone(), i, family, style, coords))
            .collect()
    }

    /// Add `faces` to the shared fonts all at once, so a lookup sees all of them or none (not a
    /// family's bold without its regular), skipping a family and style another thread has added
    /// meanwhile. Returns the number added.
    fn insert_faces(&self, new: Vec<FontFace>) -> usize {
        let mut faces = self.faces.write().unwrap_or_else(|e| e.into_inner());
        let mut added = 0;
        for f in new {
            if !has_face(&faces, &f.family, &f.style) {
                faces.push(Arc::new(f));
                added += 1;
            }
        }
        added
    }

    /// Catalog the installed fonts unless that has been done: the first caller scans the font
    /// folders, any other waits for that scan to finish.
    #[cfg(not(target_arch = "wasm32"))]
    fn ensure_catalog(&self) {
        self.cataloged.get_or_init(|| {
            self.scan_font_dirs();
        });
    }

    /// Catalog the installed fonts on a background thread, so the first lookup by family name
    /// (opening a file, the font menus) doesn't wait for the scan. A no-op once they are
    /// cataloged, and on wasm.
    pub fn scan_in_background(&'static self) {
        #[cfg(not(target_arch = "wasm32"))]
        if self.cataloged.get().is_none() {
            // A failed spawn leaves the scan to the first lookup that needs it.
            let _ = std::thread::Builder::new().name("font-scan".into()).spawn(move || self.ensure_catalog());
        }
    }

    /// Scan the font folders again (fonts installed or removed since), cataloging the faces
    /// found (native only). Font data is loaded when a cataloged family is first resolved.
    /// Returns the number of faces cataloged.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn load_system_fonts(&self) -> usize {
        // The first scan, or a rescan once it is done (never both at once).
        let mut first = None;
        self.cataloged.get_or_init(|| first = Some(self.scan_font_dirs()));
        let count = first.unwrap_or_else(|| self.scan_font_dirs());
        // Retry characters and CJK chain families that newly installed fonts can now cover.
        // Reset outside scan_font_dirs: a chain lookup can hold sys during the initial scan.
        let mut sys = self.sys.lock().unwrap_or_else(|e| e.into_inner());
        sys.generation = sys.generation.wrapping_add(1);
        sys.misses.clear();
        sys.chain_tried.clear();
        count
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn scan_font_dirs(&self) -> usize {
        let mut found = Vec::new();
        let mut stack = self.font_dirs.clone();
        // Each folder once, however links lead back to it.
        let mut visited = std::collections::HashSet::new();
        while let Some(d) = stack.pop() {
            if !visited.insert(std::fs::canonicalize(&d).unwrap_or_else(|_| d.clone())) {
                continue;
            }
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
                for f in file_face_names(&p) {
                    found.push(CatalogEntry { family: f.family, style: f.style, path: p.clone(), group: f.group, native: f.native });
                }
            }
        }
        let n = found.len();
        log::debug!("cataloged {n} system font faces");
        *self.catalog.write().unwrap_or_else(|e| e.into_inner()) = found;
        n
    }

    /// Load the files of the installed `family`. Returns whether any face was added.
    #[cfg(not(target_arch = "wasm32"))]
    fn load_cataloged(&self, family: &str) -> bool {
        let paths: Vec<std::path::PathBuf> = {
            let cat = self.read_catalog();
            let mut p: Vec<_> = cat.iter().filter(|c| c.family.eq_ignore_ascii_case(family)).map(|c| c.path.clone()).collect();
            p.sort();
            p.dedup();
            p
        };
        // The whole family arrives at once (see `insert_faces`).
        let mut faces = Vec::new();
        for p in paths {
            if let Ok(data) = std::fs::read(&p) {
                faces.extend(self.new_faces(data, FontSource::Installed(p)));
            }
        }
        self.insert_faces(faces) > 0
    }

    /// Load the font files (.ttf, .otf, .ttc, .otc; not in subfolders) of a document's `Document
    /// Fonts` folder under a new scope, for that document alone: looked up through
    /// [`FontDb::scoped`], they come before the shared faces of the same family and style, until
    /// [`FontDb::close_scope`]. A file loaded before and unchanged since (same length and
    /// modification time) isn't read again: its faces are shared. The files are untrusted: past
    /// [`MAX_DOCUMENT_FONT_FILES`] (in name order), files over [`MAX_DOCUMENT_FONT_BYTES`] or past
    /// [`MAX_DOCUMENT_FONTS_TOTAL`] in all, and files that aren't fonts are skipped with a note. A
    /// missing folder loads nothing (scope 0).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn load_document_fonts(&self, folder: &std::path::Path) -> DocumentFonts {
        let mut out = DocumentFonts::default();
        let Ok(entries) = std::fs::read_dir(folder) else { return out };
        let mut files: Vec<std::path::PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                let ext = p.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase());
                matches!(ext.as_deref(), Some("ttf" | "otf" | "ttc" | "otc")) && p.is_file()
            })
            .collect();
        files.sort();
        let mut total = 0u64;
        let mut own: Vec<Arc<FontFace>> = Vec::new();
        for (n, path) in files.iter().enumerate() {
            if n == MAX_DOCUMENT_FONT_FILES {
                out.skipped.push(format!("{} more font files not loaded (at most {MAX_DOCUMENT_FONT_FILES} are)", files.len() - n));
                break;
            }
            let name = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
            // The listed length skips oversized files unread.
            let meta = std::fs::metadata(path).ok();
            let listed = meta.as_ref().map_or(0, |m| m.len());
            let faces = if listed > MAX_DOCUMENT_FONT_BYTES {
                Err(too_large())
            } else if total.saturating_add(listed) > MAX_DOCUMENT_FONTS_TOTAL {
                Err(over_total())
            } else {
                let modified = meta.and_then(|m| m.modified().ok());
                self.document_file(path, listed, modified, MAX_DOCUMENT_FONTS_TOTAL.saturating_sub(total))
            };
            match faces {
                Err(r) => out.skipped.push(format!("{name}: {r}")),
                Ok((faces, len)) => {
                    total = total.saturating_add(len);
                    // A family and style the folder already gave stays with its first file.
                    for f in faces {
                        if !own.iter().any(|o| o.family.eq_ignore_ascii_case(&f.family) && o.style.eq_ignore_ascii_case(&f.style)) {
                            own.push(f);
                        }
                    }
                }
            }
        }
        for s in &out.skipped {
            log::warn!("{}: {s}", folder.display());
        }
        if !own.is_empty() {
            out.scope = NEXT_SCOPE.fetch_add(1, Ordering::Relaxed);
            out.faces = own.len();
            self.scopes.write().unwrap_or_else(|e| e.into_inner()).insert(out.scope, own.into());
        }
        out
    }

    /// The faces of the document font file at `path` (`listed` bytes long, modified at
    /// `modified`): those read before if it hasn't changed since, else read now, within `room`
    /// bytes of the folder's total → (faces, bytes), or why the file is skipped.
    #[cfg(not(target_arch = "wasm32"))]
    fn document_file(
        &self,
        path: &std::path::Path,
        listed: u64,
        modified: Option<std::time::SystemTime>,
        room: u64,
    ) -> Result<(Vec<Arc<FontFace>>, u64), String> {
        use std::io::Read;
        if let Some((len, m, faces)) = self.document_files.lock().unwrap_or_else(|e| e.into_inner()).get(path)
            && (*len, *m) == (listed, modified)
        {
            return Ok((faces.clone(), listed));
        }
        // One byte past the limit catches a file that grew since it was listed.
        let mut data = Vec::new();
        std::fs::File::open(path)
            .and_then(|f| f.take(MAX_DOCUMENT_FONT_BYTES + 1).read_to_end(&mut data))
            .map_err(|e| format!("can't be read ({e})"))?;
        let len = data.len() as u64;
        if len > MAX_DOCUMENT_FONT_BYTES {
            return Err(too_large());
        }
        if listed.max(len) > room {
            return Err(over_total());
        }
        let data = Arc::new(data);
        let faces: Vec<Arc<FontFace>> = enumerate_faces(&data)
            .into_iter()
            .filter_map(|(i, family, style, coords)| {
                make_face(FontBytes::Owned(data.clone()), FontSource::Document(path.to_path_buf()), i, family, style, coords)
            })
            .map(Arc::new)
            .collect();
        if faces.is_empty() {
            return Err("not a font DesignCraft can read".into());
        }
        self.document_files.lock().unwrap_or_else(|e| e.into_inner()).insert(path.to_path_buf(), (listed, modified, faces.clone()));
        Ok((faces, len))
    }

    /// Close a document's font scope: its fonts are no longer found (in it or anywhere). Their
    /// files stay read, for the document opening again.
    pub fn close_scope(&self, scope: u32) {
        self.scopes.write().unwrap_or_else(|e| e.into_inner()).remove(&scope);
    }

    /// The shared fonts' face for `family` + `style` (see [`ScopedFonts::face`]).
    pub fn face(&self, family: &str, style: &str) -> Arc<FontFace> {
        self.scoped(0).face(family, style)
    }

    /// Is `family` among the shared fonts (loaded, or installed on the system)?
    pub fn has_family(&self, family: &str) -> bool {
        self.scoped(0).has_family(family)
    }

    fn is_loaded(&self, family: &str) -> bool {
        self.read_faces().iter().any(|f| f.family.eq_ignore_ascii_case(family))
    }

    /// A shared variable font's axes (see [`ScopedFonts::axes`]).
    pub fn axes(&self, family: &str, style: &str) -> Vec<(String, String, f32, f32, f32)> {
        self.scoped(0).axes(family, style)
    }

    /// A shared face other than `exclude` that covers `c` (see [`ScopedFonts::fallback_for`]).
    pub fn fallback_for(&self, c: char, exclude: u32, language: Option<&str>) -> Option<Arc<FontFace>> {
        self.scoped(0).fallback_for(c, exclude, language)
    }

    /// Load a system font covering `c` (preferred fallback families first, then any cataloged
    /// file under 40 MB). Returns true if one was loaded. Misses are remembered.
    #[cfg(not(target_arch = "wasm32"))]
    fn system_fallback(&self, c: char) -> bool {
        if c.is_control() || c.is_whitespace() {
            return false;
        }
        let generation = {
            let sys = self.sys.lock().unwrap_or_else(|e| e.into_inner());
            if !sys.enabled || sys.misses.contains(&c) {
                return false;
            }
            sys.generation
        };
        let covered = |db: &FontDb| db.read_faces().iter().any(|f| f.covers(c));
        let cataloged: Vec<String> = self.read_catalog().iter().map(|e| e.family.clone()).collect();
        for fam in SYSTEM_FALLBACKS {
            if !self.is_loaded(fam) && cataloged.iter().any(|f| f.eq_ignore_ascii_case(fam)) {
                self.load_cataloged(fam);
                if covered(self) {
                    return true;
                }
            }
        }
        let mut paths: Vec<std::path::PathBuf> = self.catalog.read().unwrap_or_else(|e| e.into_inner()).iter().map(|e| e.path.clone()).collect();
        paths.sort();
        paths.dedup();
        #[cfg(all(test, target_os = "linux"))]
        {
            let hook = self.fallback_snapshot_hook.lock().unwrap_or_else(|e| e.into_inner()).take();
            if let Some(hook) = hook {
                let _ = hook.reached.send(());
                hook.resume.wait();
            }
        }
        for p in paths {
            if std::fs::metadata(&p).map(|m| m.len() > 40 << 20).unwrap_or(true) {
                continue;
            }
            let Ok(data) = std::fs::read(&p) else { continue };
            let hit =
                enumerate_faces(&data).iter().any(|(i, _, _, _)| skrifa::FontRef::from_index(&data, *i).is_ok_and(|f| f.charmap().map(c).is_some()));
            if hit && self.add_faces(data, FontSource::Installed(p)) > 0 && covered(self) {
                return true;
            }
        }
        let mut sys = self.sys.lock().unwrap_or_else(|e| e.into_inner());
        if sys.generation == generation {
            sys.misses.insert(c);
        }
        false
    }

    /// The box drawn for `face`'s missing glyph (.notdef) when the font's own has no outline, in
    /// font units, y-down: a hollow rectangle across the glyph's advance, cap height tall. `None`
    /// when the font draws its own.
    pub fn missing_box(&self, face: &FontFace) -> Option<BezPath> {
        if !font_outline(face, 0).elements().is_empty() {
            return None;
        }
        let adv = Some(face.advance(0)).filter(|a| a.is_finite() && *a > 0.0).unwrap_or(face.upem * 0.5);
        let top = Some(face.cap_height).filter(|h| h.is_finite() && *h > 0.0).unwrap_or(face.upem * 0.7);
        let outer = kurbo::Rect::new(adv * 0.1, -top, adv * 0.9, 0.0);
        let t = (face.upem * 0.05).min(outer.width() / 4.0).min(top / 4.0);
        let inner = outer.inset(-t);
        let mut p = BezPath::new();
        // The inner rectangle runs the other way round, so filling leaves it empty.
        p.move_to((outer.x0, outer.y0));
        p.line_to((outer.x1, outer.y0));
        p.line_to((outer.x1, outer.y1));
        p.line_to((outer.x0, outer.y1));
        p.close_path();
        p.move_to((inner.x0, inner.y0));
        p.line_to((inner.x0, inner.y1));
        p.line_to((inner.x1, inner.y1));
        p.line_to((inner.x1, inner.y0));
        p.close_path();
        Some(p)
    }

    /// Glyph outline in font units, y-down (flipped), cached per (face, glyph). A `.notdef`
    /// without an outline is drawn as [`FontDb::missing_box`], so a missing character never
    /// disappears.
    pub fn outline(&self, face: &FontFace, gid: u32) -> Arc<BezPath> {
        let key = (face.id, gid);
        if let Some(p) = self.outlines.lock().unwrap_or_else(|e| e.into_inner()).get(&key) {
            return p.clone();
        }
        let drawn = font_outline(face, gid);
        let p = Arc::new(if gid == 0 && drawn.elements().is_empty() { self.missing_box(face).unwrap_or(drawn) } else { drawn });
        let mut cache = self.outlines.lock().unwrap_or_else(|e| e.into_inner());
        if cache.len() >= OUTLINE_CACHE_MAX {
            cache.clear();
        }
        cache.insert(key, p.clone());
        p
    }
}

/// One document's view of the [`FontDb`] ([`FontDb::scoped`]): the fonts of its `Document Fonts`
/// folder, then the shared fonts (bundled, installed, added). Other documents' fonts aren't in it.
#[derive(Clone)]
pub struct ScopedFonts<'a> {
    db: &'a FontDb,
    /// The document's own faces, in the folder's file order (none without a scope).
    own: Option<Arc<[Arc<FontFace>]>>,
}

impl ScopedFonts<'_> {
    fn own(&self) -> &[Arc<FontFace>] {
        self.own.as_deref().unwrap_or(&[])
    }

    /// Family names available (the document's, loaded and cataloged system fonts), sorted and
    /// deduplicated.
    pub fn families(&self) -> Vec<String> {
        let mut v: Vec<String> = self.own().iter().chain(self.db.read_faces().iter()).map(|f| f.family.clone()).collect();
        #[cfg(not(target_arch = "wasm32"))]
        v.extend(self.db.read_catalog().iter().map(|c| c.family.clone()));
        v.sort_by_key(|a| a.to_lowercase());
        v.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
        v
    }

    /// The available families as the font menus list them (see [`ScopedFonts::families`]), each
    /// with its group and native name, unsorted ([`crate::sort_for_menu`] orders them).
    pub fn family_infos(&self) -> Vec<FamilyInfo> {
        let mut out: Vec<FamilyInfo> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for f in self.own().iter().chain(self.db.read_faces().iter()) {
            if seen.insert(f.family.to_lowercase()) {
                let (group, native) = f.menu_group();
                out.push(FamilyInfo { family: f.family.clone(), group, native: native.map(str::to_string) });
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        for c in self.db.read_catalog().iter() {
            if seen.insert(c.family.to_lowercase()) {
                out.push(FamilyInfo { family: c.family.clone(), group: c.group, native: c.native.clone() });
            }
        }
        out
    }

    /// Style names available for `family` (Regular first, then by weight).
    pub fn styles(&self, family: &str) -> Vec<String> {
        let v = self.exact_styles(family);
        match without_format_suffix(family) {
            Some(base) if v.is_empty() => self.exact_styles(base),
            _ => v,
        }
    }

    fn exact_styles(&self, family: &str) -> Vec<String> {
        let mut v: Vec<(bool, f32, String)> = self
            .own()
            .iter()
            .chain(self.db.read_faces().iter())
            .filter(|f| f.family.eq_ignore_ascii_case(family) && !f.style.contains('{'))
            .map(|f| (f.italic, f.weight, f.style.clone()))
            .collect();
        #[cfg(not(target_arch = "wasm32"))]
        for c in self.db.read_catalog().iter() {
            if c.family.eq_ignore_ascii_case(family) && !v.iter().any(|(_, _, s)| s.eq_ignore_ascii_case(&c.style)) {
                v.push((style_italic(&c.style), style_weight(&c.style), c.style.clone()));
            }
        }
        v.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)).then(a.2.cmp(&b.2)));
        v.dedup_by(|a, b| a.2 == b.2);
        v.into_iter().map(|t| t.2).collect()
    }

    /// Resolve a family + style to a face, falling back to the closest style of the family, then to
    /// Source Sans 3 Regular. Installed system fonts are found by name whatever ran before. A
    /// family with a format suffix (`Minion Pro (OTF)`) that isn't found is looked up without it.
    pub fn face(&self, family: &str, style: &str) -> Arc<FontFace> {
        if let Some(f) = self.find_or_load(family, style).or_else(|| without_format_suffix(family).and_then(|base| self.find_or_load(base, style))) {
            return f;
        }
        self.find(FALLBACK_FAMILY, style)
            .or_else(|| self.find(FALLBACK_FAMILY, "Regular"))
            .or_else(|| self.db.read_faces().first().cloned())
            .unwrap_or_else(last_resort_face)
    }

    fn find_or_load(&self, family: &str, style: &str) -> Option<Arc<FontFace>> {
        if let Some(f) = self.find(family, style) {
            return Some(f);
        }
        #[cfg(not(target_arch = "wasm32"))]
        if self.db.load_cataloged(family) {
            return self.find(family, style);
        }
        None
    }

    /// Is `family` available (the document's, loaded, or installed on the system), as named or
    /// without a format suffix (see [`ScopedFonts::face`])?
    pub fn has_family(&self, family: &str) -> bool {
        self.has_exact_family(family) || without_format_suffix(family).is_some_and(|base| self.has_exact_family(base))
    }

    fn has_exact_family(&self, family: &str) -> bool {
        if self.own().iter().any(|f| f.family.eq_ignore_ascii_case(family)) || self.db.is_loaded(family) {
            return true;
        }
        #[cfg(not(target_arch = "wasm32"))]
        if self.db.read_catalog().iter().any(|c| c.family.eq_ignore_ascii_case(family)) {
            return true;
        }
        false
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

    /// Glyph outline in font units, y-down (flipped), cached per (face, glyph); see
    /// [`FontDb::outline`].
    pub fn outline(&self, face: &FontFace, gid: u32) -> Arc<BezPath> {
        self.db.outline(face, gid)
    }

    /// See [`FontDb::missing_box`].
    pub fn missing_box(&self, face: &FontFace) -> Option<BezPath> {
        self.db.missing_box(face)
    }

    /// `Style {wght:650,wdth:90}`: the named style's font at those axis values, made on first use.
    fn instance(&self, family: &str, style: &str) -> Option<Arc<FontFace>> {
        let open = style.find('{')?;
        let base = self.find(family, style[..open].trim())?;
        if base.skrifa().is_none_or(|f| f.axes().is_empty()) {
            return Some(base);
        }
        let key = (base.id, style.to_string());
        if let Some(f) = self.db.instances.read().unwrap_or_else(|e| e.into_inner()).get(&key) {
            return Some(f.clone());
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
        let f = make_face(base.bytes.clone(), base.source.clone(), base.index, base.family.clone(), style.to_string(), coords)?;
        Some(self.db.instances.write().unwrap_or_else(|e| e.into_inner()).entry(key).or_insert_with(|| Arc::new(f)).clone())
    }

    fn find(&self, family: &str, style: &str) -> Option<Arc<FontFace>> {
        if style.contains('{') {
            return self.instance(family, style);
        }
        let faces = self.db.read_faces();
        // The document's fonts first: they stand in for other fonts of the same name.
        let named = |f: &&Arc<FontFace>| f.family.eq_ignore_ascii_case(family);
        let cands: Vec<&Arc<FontFace>> = self.own().iter().filter(named).chain(faces.iter().filter(named)).collect();
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

    /// A face other than `exclude` that covers `c` in `language` (a BCP 47 tag, `None` when
    /// unknown). CJK characters try their language's chain first (see [`cjk_chain`]); then the
    /// first face (fallback family first, then load order, the document's fonts last) that covers
    /// `c`. On native, system fonts are cataloged and loaded lazily the first time no loaded face
    /// covers a character.
    pub fn fallback_for(&self, c: char, exclude: u32, language: Option<&str>) -> Option<Arc<FontFace>> {
        if let Some(chain) = cjk_chain(c, language)
            && let Some(f) = chain.iter().find_map(|family| self.chain_face(family, c, exclude))
        {
            return Some(f);
        }
        if let Some(f) = self.loaded_fallback(c, exclude) {
            return Some(f);
        }
        #[cfg(not(target_arch = "wasm32"))]
        if self.db.system_fallback(c) {
            return self.loaded_fallback(c, exclude);
        }
        None
    }

    /// The face of `family` (the document's first, regular first) that covers `c`, loading the
    /// installed family the first time a chain asks for it (native, while the system fallback is
    /// on).
    fn chain_face(&self, family: &'static str, c: char, exclude: u32) -> Option<Arc<FontFace>> {
        #[cfg(not(target_arch = "wasm32"))]
        if !self.db.is_loaded(family) {
            // Held while loading, so a parallel lookup waits for the font instead of passing it by.
            let mut sys = self.db.sys.lock().unwrap_or_else(|e| e.into_inner());
            if sys.enabled && sys.chain_tried.insert(family) {
                self.db.load_cataloged(family);
            }
        }
        let faces = self.db.read_faces();
        let mut order: Vec<&Arc<FontFace>> =
            self.own().iter().chain(faces.iter()).filter(|f| f.id != exclude && f.family.eq_ignore_ascii_case(family)).collect();
        order.sort_by_key(|f| (f.italic, (f.weight - 400.0).abs() as i32));
        order.into_iter().find(|f| f.covers(c)).cloned()
    }

    fn loaded_fallback(&self, c: char, exclude: u32) -> Option<Arc<FontFace>> {
        let faces = self.db.read_faces();
        let mut order: Vec<&Arc<FontFace>> = faces.iter().chain(self.own().iter()).filter(|f| f.id != exclude).collect();
        order.sort_by_key(|f| (!f.family.eq_ignore_ascii_case(FALLBACK_FAMILY), f.italic, (f.weight - 400.0).abs() as i32));
        order.into_iter().find(|f| f.covers(c)).cloned()
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn too_large() -> String {
    format!("larger than {} MB", MAX_DOCUMENT_FONT_BYTES >> 20)
}

#[cfg(not(target_arch = "wasm32"))]
fn over_total() -> String {
    format!("over the {} MB the folder's fonts may take in all", MAX_DOCUMENT_FONTS_TOTAL >> 20)
}

/// Glyph `gid`'s outline as the font draws it, in font units, y-down (flipped).
fn font_outline(face: &FontFace, gid: u32) -> BezPath {
    let mut pen = FlipPen(BezPath::new());
    if let Some(f) = face.skrifa()
        && let Some(g) = f.outline_glyphs().get(GlyphId::new(gid))
    {
        let _ = g.draw(DrawSettings::unhinted(Size::unscaled(), face.location()), &mut pen);
    }
    pen.0
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

#[cfg(test)]
#[cfg(not(target_arch = "wasm32"))]
#[path = "tests_sysfonts.rs"]
mod tests_sysfonts;

#[cfg(test)]
#[cfg(not(target_arch = "wasm32"))]
#[path = "tests_fallback.rs"]
mod tests_fallback;

#[cfg(test)]
#[cfg(not(target_arch = "wasm32"))]
#[path = "tests_docfonts.rs"]
mod tests_docfonts;

#[cfg(test)]
#[path = "tests_vertical.rs"]
mod tests_vertical;
