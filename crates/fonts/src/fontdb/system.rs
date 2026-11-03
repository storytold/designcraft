#[cfg(not(target_arch = "wasm32"))]
use super::db::NEXT_SCOPE;
use super::face::FontFace;
#[cfg(not(target_arch = "wasm32"))]
use super::face::{
    DocumentFonts, FontBytes, FontSource, MAX_DOCUMENT_FONT_BYTES, MAX_DOCUMENT_FONT_FILES, MAX_DOCUMENT_FONTS_TOTAL, enumerate_faces, make_face,
};
#[cfg(not(target_arch = "wasm32"))]
use super::fallback::SYSTEM_FALLBACKS;
#[cfg(not(target_arch = "wasm32"))]
use super::names::{aliases, face_names, in_catalog_family, name, norm, wws_names};
#[cfg(not(target_arch = "wasm32"))]
use super::{Arc, FontGroup, MetadataProvider, Ordering, PoisonError, StringId};
use super::{FontDb, ScopedFonts};

impl FontFace {
    /// When the font file was added (created, else last modified): the font menus' Show Recently
    /// Added. `None` for fonts without a file.
    pub fn added(&self) -> Option<std::time::SystemTime> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let meta = std::fs::metadata(self.source.path()?).ok()?;
            meta.created().or_else(|_| meta.modified()).ok()
        }
        #[cfg(target_arch = "wasm32")]
        {
            None
        }
    }
}

/// A font file's length and modification time.
#[cfg(not(target_arch = "wasm32"))]
pub(super) type FileStamp = (u64, Option<std::time::SystemTime>);

#[cfg(not(target_arch = "wasm32"))]
fn file_stamp(path: &std::path::Path) -> Option<FileStamp> {
    std::fs::metadata(path).ok().map(|m| (m.len(), m.modified().ok()))
}

/// What [`FontDb::reload_fonts`] did: the faces cataloged by the rescan, and the installed font
/// files that changed or were removed since they were read (their faces are read again, from the
/// new file, the next time they are looked up).
#[derive(Debug, Default, PartialEq)]
pub struct ReloadedFonts {
    pub cataloged: usize,
    pub changed: Vec<std::path::PathBuf>,
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Debug)]
pub(super) struct CatalogEntry {
    pub(super) family: String,
    pub(super) style: String,
    pub(super) wws: Option<(String, String)>,
    path: std::path::PathBuf,
    /// The font menu group and native family name, read by the scan.
    pub(super) group: FontGroup,
    pub(super) native: Option<String>,
    pub(super) aliases: Vec<String>,
    pub(super) postscript: Option<String>,
}

/// What the scan reads of one face in a font file: its names and font menu group.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Debug, PartialEq)]
pub(super) struct ScannedFace {
    pub(super) family: String,
    pub(super) style: String,
    wws: Option<(String, String)>,
    group: FontGroup,
    native: Option<String>,
    aliases: Vec<String>,
    postscript: Option<String>,
}

/// The tables glyph outlines are drawn from.
#[cfg(not(target_arch = "wasm32"))]
const OUTLINE_TABLES: [&[u8; 4]; 4] = [b"glyf", b"CFF ", b"CFF2", b"VARC"];

/// The names and font menu group of every face in the font file at `path`, reading only its table
/// directories and the tables that name and classify it (`name`, `OS/2`, `meta`; `cmap` only for a
/// font that declares no language or code page): a scan opens hundreds of font files, many of them
/// megabytes long. A variable font is cataloged under its default style; its named instances
/// appear once it loads. Faces without glyph outlines (none of [`OUTLINE_TABLES`]) are left out.
#[cfg(not(target_arch = "wasm32"))]
pub(super) fn file_face_names(path: &std::path::Path) -> Vec<ScannedFace> {
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
            // Faces drawn from other tables (Apple's `hvgl`, bitmaps) would draw nothing.
            if !records.as_chunks::<16>().0.iter().any(|r| OUTLINE_TABLES.iter().any(|t| r.starts_with(*t))) {
                return None;
            }
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
            let (group, native) = match crate::group::declared_group(&f, &family) {
                Some(g) => (g, crate::group::classify(&f, &family).1),
                None => {
                    found.extend(table(b"cmap", MAX_CMAP));
                    let font = crate::group::sfnt(found)?;
                    crate::group::classify(&skrifa::FontRef::new(&font).ok()?, &family)
                }
            };
            let postscript = name(&f, &[StringId::POSTSCRIPT_NAME]);
            let wws = wws_names(&f, &family, &style);
            Some(ScannedFace {
                family,
                style,
                wws,
                group,
                native,
                aliases: aliases(&f, &[StringId::TYPOGRAPHIC_FAMILY_NAME, StringId::FAMILY_NAME]),
                postscript,
            })
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
        let xdg_data_home = std::env::var_os("XDG_DATA_HOME").map(std::path::PathBuf::from);
        dirs.extend(unix_font_dirs(home.as_deref(), xdg_data_home.as_deref()));
    }
    dirs
}

pub(super) fn unix_font_dirs(home: Option<&std::path::Path>, xdg_data_home: Option<&std::path::Path>) -> Vec<std::path::PathBuf> {
    let mut dirs: Vec<std::path::PathBuf> = ["/usr/share/fonts", "/usr/local/share/fonts"].map(Into::into).into();
    if let Some(home) = home {
        dirs.push(home.join(".fonts"));
    }
    let data_home =
        xdg_data_home.filter(|path| path.is_absolute()).map(std::path::Path::to_path_buf).or_else(|| home.map(|path| path.join(".local/share")));
    dirs.extend(data_home.map(|path| path.join("fonts")));
    dirs
}

#[cfg(not(target_os = "macos"))]
pub(super) fn platform_font_files() -> Vec<std::path::PathBuf> {
    Vec::new()
}

/// The files in `listed` that aren't in `scanned` (canonical paths), each once and added to
/// `scanned`; paths that don't lead to a file (gone, unreadable folder) are left out.
#[cfg(not(target_arch = "wasm32"))]
fn unscanned_font_files(listed: Vec<std::path::PathBuf>, scanned: &mut std::collections::HashSet<std::path::PathBuf>) -> Vec<std::path::PathBuf> {
    listed
        .into_iter()
        .filter(|p| {
            let Ok(canonical) = std::fs::canonicalize(p) else { return false };
            canonical.is_file() && scanned.insert(canonical)
        })
        .collect()
}

impl FontDb {
    /// Catalog the installed fonts unless that has been done: the first caller scans the font
    /// folders, any other waits for that scan to finish.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn ensure_catalog(&self) {
        self.cataloged.get_or_init(|| {
            self.scan_font_dirs();
        });
    }

    /// Whether the installed fonts are cataloged: a lookup by family name won't wait for the
    /// scan (always true on wasm, which has none). See [`FontDb::scan_in_background`].
    pub fn system_fonts_cataloged(&self) -> bool {
        #[cfg(target_arch = "wasm32")]
        return true;
        #[cfg(not(target_arch = "wasm32"))]
        self.cataloged.get().is_some()
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
        let mut sys = self.sys.lock().unwrap_or_else(PoisonError::into_inner);
        sys.generation = sys.generation.wrapping_add(1);
        sys.misses.clear();
        sys.chain_tried.clear();
        count
    }

    /// Fonts installed, changed or removed since they were read (#328): scan the font folders
    /// again (see [`FontDb::load_system_fonts`]), then let go of the shared faces whose installed
    /// file changed (length or modification time) or is gone, so the next lookup reads the file as
    /// it is now, or finds the font missing. What was composed with the old faces must be composed
    /// again. Faces are never freed (see [`FaceRef`]): each reload of a changed file keeps the old
    /// copy. Bundled, added and document fonts are left alone. Nothing to do on wasm.
    pub fn reload_fonts(&self) -> ReloadedFonts {
        #[cfg(not(target_arch = "wasm32"))]
        {
            // Rescan first: a lookup after the faces are dropped finds the new files.
            let cataloged = self.load_system_fonts();
            let changed: Vec<std::path::PathBuf> = {
                let mut files = self.installed_files.lock().unwrap_or_else(PoisonError::into_inner);
                let mut changed: Vec<_> = files.iter().filter(|(p, stamp)| file_stamp(p).as_ref() != Some(*stamp)).map(|(p, _)| p.clone()).collect();
                changed.sort();
                for p in &changed {
                    files.remove(p);
                }
                changed
            };
            if !changed.is_empty() {
                let stale = |f: &FontFace| matches!(&f.source, FontSource::Installed(p) if changed.contains(p));
                let mut dropped = std::collections::HashSet::new();
                self.faces.write().unwrap_or_else(PoisonError::into_inner).retain(|f| {
                    let keep = !stale(f);
                    if !keep {
                        dropped.insert(f.id);
                    }
                    keep
                });
                self.instances.write().unwrap_or_else(PoisonError::into_inner).retain(|(base, _), f| !dropped.contains(base) && !stale(f));
                self.outlines.lock().unwrap_or_else(PoisonError::into_inner).retain(|(face, _), _| !dropped.contains(face));
                log::debug!("reloading {} changed font files ({} faces)", changed.len(), dropped.len());
            }
            ReloadedFonts { cataloged, changed }
        }
        #[cfg(target_arch = "wasm32")]
        ReloadedFonts::default()
    }

    /// Read an installed font file, noting its length and modification time as they were before
    /// the read (a change during it is then found by the next reload).
    #[cfg(not(target_arch = "wasm32"))]
    fn read_installed(&self, path: &std::path::Path) -> Option<Vec<u8>> {
        let stamp = file_stamp(path)?;
        let data = std::fs::read(path).ok()?;
        self.installed_files.lock().unwrap_or_else(PoisonError::into_inner).insert(path.to_path_buf(), stamp);
        Some(data)
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn scan_font_dirs(&self) -> usize {
        let mut found = Vec::new();
        let mut stack = self.font_dirs.clone();
        // Each folder once, however links lead back to it.
        let mut visited = std::collections::HashSet::new();
        // The font files read, by canonical path, so the platform's list adds only the others.
        let mut scanned = std::collections::HashSet::new();
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
                let ext = p.extension().and_then(std::ffi::OsStr::to_str).map(str::to_ascii_lowercase);
                if !matches!(ext.as_deref(), Some("ttf" | "otf" | "ttc" | "otc")) {
                    continue;
                }
                scanned.insert(std::fs::canonicalize(&p).unwrap_or_else(|_| p.clone()));
                for f in file_face_names(&p) {
                    found.push(CatalogEntry {
                        family: f.family,
                        style: f.style,
                        wws: f.wws,
                        path: p.clone(),
                        group: f.group,
                        native: f.native,
                        aliases: f.aliases,
                        postscript: f.postscript,
                    });
                }
            }
        }
        // Font services name their files as they like (no extension, say): the face names read
        // decide what is a font.
        for p in unscanned_font_files((self.font_files)(), &mut scanned) {
            for f in file_face_names(&p) {
                found.push(CatalogEntry {
                    family: f.family,
                    style: f.style,
                    wws: f.wws,
                    path: p.clone(),
                    group: f.group,
                    native: f.native,
                    aliases: f.aliases,
                    postscript: f.postscript,
                });
            }
        }
        let n = found.len();
        log::debug!("cataloged {n} system font faces");
        *self.catalog.write().unwrap_or_else(PoisonError::into_inner) = found;
        n
    }

    /// Load the files of the installed `family`. Returns whether any face was added.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn load_cataloged(&self, family: &str) -> bool {
        let paths: Vec<std::path::PathBuf> = {
            let cat = self.read_catalog();
            let mut p: Vec<_> = cat.iter().filter(|c| in_catalog_family(c, family)).map(|c| c.path.clone()).collect();
            p.sort();
            p.dedup();
            p
        };
        // The whole family arrives at once (see `insert_faces`).
        let mut faces = Vec::new();
        for p in paths {
            if let Some(data) = self.read_installed(&p) {
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
    ///
    /// The folder is named by a document, so this crate does not trust it: `allow` (`Ok`, or why a
    /// path may not be read) is asked about
    /// the folder and about every font file in it before either is opened, and a path it refuses
    /// is skipped with its message (the engine passes its automation-root check; a host with no
    /// roots passes `&|_| Ok(())`).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn load_document_fonts(&self, folder: &std::path::Path, allow: &dyn Fn(&std::path::Path) -> Result<(), String>) -> DocumentFonts {
        let mut out = DocumentFonts::default();
        if let Err(why) = allow(folder) {
            out.skipped.push(why);
            return out;
        }
        let Ok(entries) = std::fs::read_dir(folder) else { return out };
        let mut files: Vec<std::path::PathBuf> = Vec::new();
        for path in entries.flatten().map(|e| e.path()) {
            let ext = path.extension().and_then(std::ffi::OsStr::to_str).map(str::to_ascii_lowercase);
            if !matches!(ext.as_deref(), Some("ttf" | "otf" | "ttc" | "otc")) {
                continue;
            }
            // Each file is vetted before it is touched at all, a link out of the folder included.
            match allow(&path) {
                Ok(()) if path.is_file() => files.push(path),
                Ok(()) => {}
                Err(why) => out.skipped.push(why),
            }
        }
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
            let listed = meta.as_ref().map_or(0, std::fs::Metadata::len);
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
            self.scopes.write().unwrap_or_else(PoisonError::into_inner).insert(out.scope, own.into());
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
        if let Some((len, m, faces)) = self.document_files.lock().unwrap_or_else(PoisonError::into_inner).get(path)
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
        self.document_files.lock().unwrap_or_else(PoisonError::into_inner).insert(path.to_path_buf(), (listed, modified, faces.clone()));
        Ok((faces, len))
    }

    /// Load a system font covering `c` (preferred fallback families first, then any cataloged
    /// file under 40 MB). Returns true if one was loaded. Misses are remembered.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn system_fallback(&self, c: char) -> bool {
        if c.is_control() || c.is_whitespace() {
            return false;
        }
        let generation = {
            let sys = self.sys.lock().unwrap_or_else(PoisonError::into_inner);
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
        let mut paths: Vec<std::path::PathBuf> = self.catalog.read().unwrap_or_else(PoisonError::into_inner).iter().map(|e| e.path.clone()).collect();
        paths.sort();
        paths.dedup();
        #[cfg(all(test, target_os = "linux"))]
        {
            let hook = self.fallback_snapshot_hook.lock().unwrap_or_else(PoisonError::into_inner).take();
            if let Some(hook) = hook {
                let _ = hook.reached.send(());
                hook.resume.wait();
            }
        }
        for p in paths {
            if std::fs::metadata(&p).map(|m| m.len() > 40 << 20).unwrap_or(true) {
                continue;
            }
            let Some(data) = self.read_installed(&p) else { continue };
            let hit =
                enumerate_faces(&data).iter().any(|(i, _, _, _)| skrifa::FontRef::from_index(&data, *i).is_ok_and(|f| f.charmap().map(c).is_some()));
            if hit && self.add_faces(data, FontSource::Installed(p)) > 0 && covered(self) {
                return true;
            }
        }
        let mut sys = self.sys.lock().unwrap_or_else(PoisonError::into_inner);
        if sys.generation == generation {
            sys.misses.insert(c);
        }
        false
    }
}

impl ScopedFonts<'_> {
    /// `family`'s kind (format, class, PANOSE) and when its file was added, read from its Regular
    /// face (else its first), for the font menus' filters, which look at every family. A face
    /// already loaded is asked; an installed family that isn't loaded is read from its file, which
    /// is then let go rather than kept loaded. None for an unknown family or an unreadable file.
    pub fn family_kind(&self, family: &str) -> Option<(crate::kind::FaceKind, Option<std::time::SystemTime>)> {
        let styles = self.styles(family);
        let style = styles.iter().find(|s| *s == "Regular").or(styles.first())?.clone();
        if self.own().iter().chain(self.db.read_faces().iter()).any(|f| f.family.eq_ignore_ascii_case(family)) {
            let f = self.find(family, &style)?;
            return Some((f.kind(), f.added()));
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let path = {
                let cat = self.db.read_catalog();
                let of_family = |c: &&CatalogEntry| c.family.eq_ignore_ascii_case(family);
                let entry = cat.iter().filter(of_family).find(|c| norm(&c.style) == norm(&style)).or_else(|| cat.iter().find(of_family));
                entry.map(|c| c.path.clone())?
            };
            if std::fs::metadata(&path).ok()?.len() > MAX_DOCUMENT_FONT_BYTES {
                return None;
            }
            let data = Arc::new(std::fs::read(&path).ok()?);
            let found = enumerate_faces(&data);
            let pick = found
                .iter()
                .find(|(_, f, st, _)| f.eq_ignore_ascii_case(family) && norm(st) == norm(&style))
                .or_else(|| found.iter().find(|(_, f, _, _)| f.eq_ignore_ascii_case(family)))?
                .clone();
            let (index, fam, st, coords) = pick;
            let face = make_face(FontBytes::Owned(data.clone()), FontSource::Installed(path), index, fam, st, coords)?;
            Some((face.kind(), face.added()))
        }
        #[cfg(target_arch = "wasm32")]
        None
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
