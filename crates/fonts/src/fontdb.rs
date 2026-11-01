//! Font database: bundled OFL fonts, user fonts, the installed system fonts (cataloged the first
//! time a lookup needs them, native only), outline cache.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, PoisonError, RwLock};

use kurbo::BezPath;
use skrifa::instance::{Location, LocationRef, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::raw::FileRef;
use skrifa::string::StringId;
use skrifa::{GlyphId, MetadataProvider};

use crate::group::{FamilyInfo, FontGroup};

impl FontSource {
    /// The font file, if the face was loaded from one.
    pub fn path(&self) -> Option<&std::path::Path> {
        match self {
            FontSource::Installed(p) | FontSource::Document(p) => Some(p),
            FontSource::Bundled | FontSource::Memory => None,
        }
    }
}

impl FaceRef {
    /// The handle for a loaded face (memoised per face).
    pub fn of(face: &Arc<FontFace>) -> FaceRef {
        static LEAKED: RwLock<Vec<Option<&FontFace>>> = RwLock::new(Vec::new());
        let id = face.id as usize;
        if let Some(Some(f)) = LEAKED.read().unwrap_or_else(PoisonError::into_inner).get(id) {
            return FaceRef(f);
        }
        let mut w = LEAKED.write().unwrap_or_else(PoisonError::into_inner);
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
    /// Each installed font file read into the shared fonts, with its (length, modification time)
    /// when it was read: [`FontDb::reload_fonts`] reads again the files that changed since.
    #[cfg(not(target_arch = "wasm32"))]
    installed_files: Mutex<HashMap<std::path::PathBuf, FileStamp>>,
    /// The folders the system font scan reads (the platform's font folders for [`FontDb::global`]).
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    font_dirs: Vec<std::path::PathBuf>,
    /// Lists the font files outside `font_dirs` that the system font scan reads too, asked again by
    /// each scan ([`platform_font_files`] for [`FontDb::global`]).
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    font_files: fn() -> Vec<std::path::PathBuf>,
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

/// The font files outside the font folders that the platform has available, which
/// [`FontDb::global`]'s scan reads too: on macOS, those CoreText's font manager lists, such as the
/// fonts font services and font managers register from their own folders. None elsewhere.
#[cfg(target_os = "macos")]
use crate::mac_fonts::available_font_files as platform_font_files;

mod db;
mod face;
mod fallback;
mod names;
mod outline;
mod scoped;
mod system;

#[cfg(test)]
pub(crate) use face::last_resort_face;
pub use face::{
    DOCUMENT_FONTS_FOLDER, DocumentFonts, FALLBACK_FAMILY, FaceRef, FontFace, FontSource, MAX_DOCUMENT_FONT_BYTES, MAX_DOCUMENT_FONT_FILES,
    MAX_DOCUMENT_FONTS_TOTAL, bundled,
};
#[cfg(test)]
use face::{FontBytes, make_face};
pub(crate) use face::{measured_top, plausible};
#[cfg(all(test, target_os = "linux"))]
use fallback::FallbackSnapshotHook;
#[cfg(not(target_arch = "wasm32"))]
use fallback::SysFallback;
pub use fallback::sans_by_name;
pub use names::{base_style, same_style};
use outline::FlipPen;
pub use scoped::ScopedFonts;
#[cfg(all(not(target_arch = "wasm32"), test))]
use system::file_face_names;
#[cfg(all(test, not(any(target_os = "macos", windows))))]
use system::unix_font_dirs;
#[cfg(not(target_arch = "wasm32"))]
use system::{CatalogEntry, FileStamp};
pub use system::{ReloadedFonts, system_font_dirs};

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
mod tests;
