//! File menu: new, open, save, place, export, close.

use std::sync::Arc;

use designcraft_doc::build::{NewDocument, PRESETS};
use designcraft_doc::{Asset, AssetId, Content, Document, Graphic, Item, ItemId, PdfCrop, Selection, Shape};
use designcraft_geom::{Affine, Rect, shapes};
use serde_json::{Value, json};

use super::{CommandSpec, always, bad, cmd, f64_or, has_doc, ok, str_param};
use crate::{DocState, EngineError, Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(noundo "file.new", "Document…", ["File", "New"], Some("Cmd+N"),
            "{preset?: \"Letter\"|\"A4\"|… or a saved preset (file.savePreset), intent?: print|web|mobile, units?: points|picas|millimeters|…, width?, height?, pages?, startPage?, facingPages?, columns?, gutter?, margins?: number|{top,bottom,inside,outside}, bleed?, slug?: n | [top, bottom, inside, outside], title?, primaryTextFrame?: bool}",
            always, file_new),
        cmd!(noundo "file.newSample", "Sample Document", ["Help"], None, "{} — a multi-page magazine sample", always, file_sample),
        cmd!(query "file.presets", "Document Presets", [], None, "{} → the built-in presets [{name, intent, width, height, units}], then the saved ones (with `saved: true`)", always, |s, _| {
            let mut presets: Vec<Value> = PRESETS.iter().map(|p| serde_json::to_value(p).unwrap_or_default()).collect();
            presets.extend(
                s.prefs
                    .document_presets
                    .iter()
                    .map(|p| json!({"name": p.title, "intent": p.intent, "width": p.width, "height": p.height, "units": p.units, "saved": true})),
            );
            Ok(Value::Array(presets))
        }),
        cmd!(noundo "file.savePreset", "Save Document Preset", [], None,
            "{name, …the settings as file.new} — kept with the preferences; `file.new {preset: name}` uses it. A saved preset of that name is replaced; built-in names are refused",
            always, save_preset),
        cmd!(noundo "file.deletePreset", "Delete Document Preset", [], None, "{name} — a saved document preset", always, |s, p| {
            let name = str_param(p, "name").unwrap_or_default().trim().to_string();
            let before = s.prefs.document_presets.len();
            s.prefs.document_presets.retain(|x| !x.title.eq_ignore_ascii_case(&name));
            if s.prefs.document_presets.len() == before {
                return Err(bad("file.deletePreset", format!("no saved preset `{name}`")));
            }
            ok()
        }),
        cmd!(noundo "file.open", "Open…", ["File"], Some("Cmd+O"),
            "{path} — a DesignCraft or IDML file (recognised by its content, so the extension may be missing), or an InDesign .indd/.indt (recognised by its signature or extension, converted to IDML; its conversion warnings join `warnings`); the fonts in a `Document Fonts` folder beside it load first → {index, documentFonts: faces loaded, warnings: font files skipped, GREP styles whose pattern can't compile, Based on Parent links cleared and, for IDML and InDesign files, package parts missing}",
            always, file_open),
        cmd!(noundo "file.openBytes", "Open Bytes", [], None, "{name, base64} — DesignCraft JSON, an IDML package or an InDesign .indd/.indt → {index, warnings}", always, file_open_bytes),
        cmd!(noundo "file.openRecent", "Open Recent File", [], None,
            "{index} — reopen entry `index` of File › Open Recent (as file.open); a file that is gone is an error and stays listed",
            has_recent, file_open_recent),
        cmd!(query "file.recentFiles", "Recent Files", [], None, "{} → [{path, name, exists}] File › Open Recent, most recent first", always, |s, _| {
            Ok(Value::Array(s.prefs.recent_files.iter().map(|p| recent_entry(p)).collect()))
        }),
        cmd!(noundo "file.clearRecent", "Clear Recent Files", ["File", "Open Recent"], None, "{} — empty File › Open Recent", has_recent, |s, _| {
            s.prefs.recent_files.clear();
            ok()
        }),
        cmd!(noundo "file.save", "Save", ["File"], Some("Cmd+S"), "{path?} — a `path` without the .designcraft extension gets it → {path: the file written, bytes}", has_doc, file_save),
        cmd!(noundo "file.saveAs", "Save As…", ["File"], Some("Cmd+Shift+S"), "{path} — a path without the .designcraft extension gets it → {path: the file written, bytes}", has_doc, file_save),
        cmd!(noundo "file.saveACopy", "Save a Copy…", ["File"], None, "{path} — writes the document without changing which file it is or its unsaved state; a path without the .designcraft extension gets it → {path: the file written, bytes}", has_doc, |s, p| {
            let path = str_param(p, "path").ok_or_else(|| bad("file.saveACopy", "missing `path`"))?;
            let path = with_document_extension(path);
            let mut copy = (*s.doc()?.doc).clone();
            super::datamerge::refresh_relative_paths(&mut copy, std::path::Path::new(&path));
            let bytes = to_bytes_with_preview(&copy, &s.cache)?;
            #[cfg(not(target_arch = "wasm32"))]
            crate::io::write_file(&path, &bytes).map_err(EngineError::Other)?;
            Ok(json!({"path": path, "bytes": bytes.len()}))
        }),
        cmd!(noundo "file.revert", "Revert", ["File"], None, "{} — back to the last saved version (not undoable)", can_revert, file_revert),
        cmd!(query "file.recovery.list", "Recovery Data", [], None, "{dir?} → [{uid, path, title, saved}] unsaved documents left by a crash (default: the session's recovery folder)", always, |s, p| {
            let dir = recovery_dir(s, p, FolderAccess::Read)?;
            Ok(Value::Array(crate::recovery::list(&dir).into_iter().map(|(uid, mut m)| { m["uid"] = json!(uid); m }).collect()))
        }),
        cmd!(noundo "file.recovery.save", "Save Recovery Data", [], None, "{dir?} — write every unsaved document to the recovery folder (the app does this on a timer)", always, |s, p| {
            let dir = recovery_dir(s, p, FolderAccess::Write)?;
            Ok(json!({"saved": crate::recovery::save_tracked(s, &dir)?}))
        }),
        cmd!(noundo "file.recovery.open", "Recover Documents", [], None, "{dir?} — reopen documents left in the recovery folder (unsaved)", always, |s, p| {
            // Reopening removes the entries it read.
            let dir = recovery_dir(s, p, FolderAccess::ReadWrite)?;
            Ok(json!({"opened": crate::recovery::open(s, &dir)?}))
        }),
        cmd!(query "file.serialize", "Serialize", [], None, "{} → {base64, bytes} the .designcraft file", has_doc, |s, _| {
            let st = s.doc()?;
            // Preview fill is session state. Serialize the unfilled template.
            let doc = st.preview_stash.as_deref().unwrap_or(st.doc.as_ref());
            let bytes = to_bytes(doc)?;
            Ok(json!({"base64": base64_encode(&bytes), "bytes": bytes.len()}))
        }),
        cmd!(noundo "file.close", "Close", ["File"], Some("Cmd+W"), "{index?}", has_doc, |s, p| {
            let i = p.get("index").and_then(Value::as_u64).map(|v| v as usize).or(s.active_index()).unwrap_or(0);
            s.close_document(i);
            ok()
        }),
        cmd!(noundo "file.activate", "Activate Document", [], None, "{index}", has_doc, |s, p| {
            s.set_active(p.get("index").and_then(Value::as_u64).unwrap_or(0) as usize);
            ok()
        }),
        cmd!(query "snippet.export", "Export Selection as Snippet", ["File", "Export"], None,
            "{path?} — the selected items (with their stories, styles and images) as a .designcraft snippet; no path: {base64}", super::has_selection, snippet_export),
        cmd!(query "place.styles", "Import Options: Styles", [], None,
        "{path | base64, name} → {paragraph: [name], character: [name], conflicts: [name]} — the styles a Word/RTF file brings (for styleMap / styleConflicts)",
        has_doc, |s, p| {
            let (bytes, name, _) = read_source(p)?;
            let imp = designcraft_textimport::import(&name, &bytes).map_err(|e| bad("place.styles", e.to_string()))?;
            let st = &s.doc()?.doc.styles;
            let conflicts: Vec<&str> = imp
                .para_styles
                .iter()
                .filter(|x| st.para(&x.name).is_some())
                .chain(imp.char_styles.iter().filter(|x| st.char_style(&x.name).is_some()))
                .map(|x| x.name.as_str())
                .collect();
            Ok(json!({"paragraph": imp.para_styles.iter().map(|x| &x.name).collect::<Vec<_>>(), "character": imp.char_styles.iter().map(|x| &x.name).collect::<Vec<_>>(), "conflicts": conflicts}))
        }),
        cmd!(
            "snippet.place",
            "Place Snippet",
            [],
            None,
            "{path | base64, spread?, x?, y?} — items keep their positions unless x/y given (top-left)",
            has_doc,
            snippet_place
        ),
        cmd!(
            "place.load",
            "Load Place Cursor",
            [],
            None,
            "{path | base64, name?} — load a graphic into the place cursor (then click/drag with the placeGun tool)",
            has_doc,
            place_load
        ),
        cmd!("place.drop", "Place Loaded Graphic", [], None, "{spread?, x, y, rect?: [x0,y0,x1,y1], frame?: id}", has_doc, place_drop),
        cmd!(
            "file.place",
            "Place…",
            ["File"],
            Some("Cmd+D"),
            "{path?|base64?, name?, frame?: id (place into), spread?, x?, y?, width?: finite number > 0 (null: absent), height? (video/audio), pdfPage?: n (1-based, Image Import Options), pdfCrop?: crop|art|trim|bleed|media|contentVisible|contentAll (the box the graphic shows; `width` is the whole page's), loadCursor? (with no frame to fill and no x/y, an image loads into the place cursor, as File › Place), layoutPage?: n (IDML / .designcraft: that page's objects as a group), page?, rect?: [x0,y0,x1,y1] (text: a new frame there), autoflow? (threads one frame per page column from the first frame on, adding pages until the text fits), removeStyles?, styleMap?: {imported name: document style}, styleConflicts?: useExisting|redefine|autoRename, link? (keep the file linked in the Links panel; default: the linkTextFiles preference)} — places an image into `frame` when given; otherwise any explicit x/y/spread/width creates a new frame; otherwise uses the selected empty or graphic frame, or creates a new frame; text files (.txt, .docx, .rtf, .md) and Excel workbooks (.xlsx, as a table) go into the insertion point, the selected frame or a new frame on `page`/`rect`",
            has_doc,
            file_place
        ),
    ]
}

#[cfg(test)]
thread_local! {
    /// Makes [`to_bytes`] fail on this thread, standing in for writer errors that need gigabytes to reach.
    pub(crate) static FAIL_TO_BYTES: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// The document as `.designcraft` bytes, or an error when it can't be written; never an empty file.
pub fn to_bytes(d: &Document) -> Result<Vec<u8>> {
    save_bytes(d, None)
}

/// [`to_bytes`] with `preview` (PNG bytes) stored as the file's first-page picture.
fn save_bytes(d: &Document, preview: Option<&[u8]>) -> Result<Vec<u8>> {
    #[cfg(test)]
    if FAIL_TO_BYTES.with(std::cell::Cell::get) {
        return Err(EngineError::Other("could not write the document: injected failure".into()));
    }
    designcraft_format::save_with_preview(d, preview).map_err(|e| EngineError::Other(format!("could not write the document: {e}")))
}

/// `path` ending in `.designcraft`: the extension is added when it is missing or another one, so
/// the file opens from the file manager. A path without a file name is left for the write to fail.
pub fn with_document_extension(path: &str) -> String {
    let p = std::path::Path::new(path);
    let ext = p.extension().and_then(std::ffi::OsStr::to_str);
    if p.file_name().is_none() || ext.is_some_and(|e| e.eq_ignore_ascii_case(designcraft_format::EXTENSION)) {
        return path.to_string();
    }
    format!("{path}.{}", designcraft_format::EXTENSION)
}

/// Whether `bytes` are a document `file.open` reads: a DesignCraft package or an IDML package.
/// The first [`DOCUMENT_SNIFF_BYTES`] of a file are enough for packages that start with their
/// `mimetype` entry, as both formats write them.
pub fn is_document(bytes: &[u8]) -> bool {
    designcraft_idml::is_idml(bytes) || designcraft_format::starts_designcraft(bytes) || designcraft_format::is_designcraft(bytes)
}

/// How much of a file [`is_document`] needs to recognise a package by its `mimetype` entry.
pub const DOCUMENT_SNIFF_BYTES: u64 = 64 << 10;

/// The longer side of the first-page picture saved in the file, in pixels.
const PREVIEW_PX: f64 = 320.0;

/// [`to_bytes`] with a picture of the first page, which the start screen shows next to the file
/// in its recent files. A page that can't be drawn just leaves the picture out.
fn to_bytes_with_preview(d: &Document, cache: &crate::Cache) -> Result<Vec<u8>> {
    let preview = d.page_loc(0).and_then(|(si, pi)| {
        let b = d.spreads.get(si)?.pages.get(pi)?.bounds();
        let side = b.width().max(b.height());
        (side.is_finite() && side > 0.0).then_some(PREVIEW_PX / side)
    });
    let png =
        preview.and_then(|scale| designcraft_render::Renderer::new().render_page(d, cache, 0, scale, false, &Default::default())).map(|r| r.to_png());
    save_bytes(d, png.as_deref())
}

pub fn from_bytes(b: &[u8]) -> Result<Document> {
    load_bytes(b).map(|(d, _)| d)
}

/// Read `.designcraft` bytes and repair what a hand-edited file can get wrong without failing the
/// open, with a warning for each repair.
fn load_bytes(b: &[u8]) -> Result<(Document, Vec<String>)> {
    let mut d = designcraft_format::load(b).map_err(|e| EngineError::Other(e.to_string()))?;
    let warnings = d.fix_parent_bases();
    Ok((d, warnings))
}

/// The longest saved preset name, and how many presets are kept.
const MAX_PRESET_NAME: usize = 100;
const MAX_PRESETS: usize = 500;

/// A built-in document preset or a saved one, by name (case ignored).
fn find_preset(s: &Session, name: &str) -> Option<NewDocument> {
    NewDocument::from_preset(name).or_else(|| s.prefs.document_presets.iter().find(|p| p.title.eq_ignore_ascii_case(name.trim())).cloned())
}

/// The settings `file.new` makes a document with for `p`: its preset, then the settings over it.
fn new_document_settings(s: &Session, p: &Value) -> Result<NewDocument> {
    let mut nd = match str_param(p, "preset") {
        Some(name) => find_preset(s, name).ok_or_else(|| bad("file.new", format!("unknown preset `{name}`")))?,
        None => NewDocument::default(),
    };
    apply_settings(&mut nd, p);
    check_size("file.new", &nd)?;
    Ok(nd)
}

/// A document as `file.new` makes it. Its Basic Paragraph doesn't hyphenate capitalized words, a
/// paragraph's last word or across a column. Opened and imported files keep their own values,
/// which is why `ParaProps` keeps the IDML defaults (on).
fn new_document(nd: &NewDocument) -> Document {
    let mut d = Document::new(nd);
    if let Some(st) = Arc::make_mut(&mut d.styles).para_mut(designcraft_doc::BASIC_PARAGRAPH) {
        st.para.hyph_capitalized = Some(false);
        st.para.hyph_last_word = Some(false);
        st.para.hyph_across_column = Some(false);
    }
    d
}

fn file_new(s: &mut Session, p: &Value) -> Result<Value> {
    let mut nd = new_document_settings(s, p)?;
    s.untitled += 1;
    nd.title = str_param(p, "title").map(str::to_string).unwrap_or_else(|| format!("Untitled-{}", s.untitled));
    let d = new_document(&nd);
    let i = s.add_document(DocState::new(d, None));
    Ok(json!({"index": i}))
}

impl Session {
    /// New Document's Preview: a document made as `file.new` would make it with `p`, shown while
    /// the New Document window is open. It replaces the previous preview document in its tab and
    /// takes no Untitled number, no undo step and no recovery entry. Settings `file.new` refuses
    /// leave the previous preview as it was. Returns the preview document's uid.
    pub fn preview_new_document(&mut self, p: &Value) -> Result<u64> {
        let mut nd = new_document_settings(self, p)?;
        let previous = match self.new_document_preview {
            Some((_, previous)) => previous,
            None => self.active().map(|d| d.uid),
        };
        let shown = self.new_document_preview.and_then(|(old, _)| self.docs.iter().position(|d| d.uid == old));
        // A preview edited or saved by a menu or script while the window is open has become a
        // document of its own: it stays, and the preview gets a tab of its own.
        let replace = shown.filter(|&i| self.docs.get(i).is_some_and(|d| !d.is_dirty() && d.path.is_none()));
        if replace.is_none() && shown.is_some() {
            self.untitled += 1;
        }
        nd.title = str_param(p, "title").map(str::to_string).unwrap_or_else(|| self.next_untitled_title());
        let st = DocState::new(new_document(&nd), None);
        let uid = st.uid;
        match replace.and_then(|i| self.docs.get_mut(i).map(|d| (i, d))) {
            Some((i, d)) => {
                *d = st;
                self.active = Some(i);
            }
            None => {
                // Pushed without `add_document`: the document that was active stays as it is.
                self.docs.push(st);
                self.active = Some(self.docs.len() - 1);
            }
        }
        self.new_document_preview = Some((uid, previous));
        Ok(uid)
    }

    /// The uid of New Document's preview document, while one is shown.
    pub fn new_document_preview(&self) -> Option<u64> {
        self.new_document_preview.map(|(uid, _)| uid)
    }

    /// End New Document's preview. Its document goes, and if it was still the active document,
    /// the one that was active before it is active again. A preview that a menu or script has
    /// edited or saved stays open as a document, with an Untitled number. Returns the uid of the
    /// document removed. Without a preview it does nothing.
    pub fn end_new_document_preview(&mut self) -> Option<u64> {
        let (uid, previous) = self.new_document_preview.take()?;
        let i = self.docs.iter().position(|d| d.uid == uid)?;
        let d = self.docs.get(i)?;
        if d.is_dirty() || d.path.is_some() {
            self.untitled += 1;
            return None;
        }
        self.docs.remove(i);
        self.active = match self.active {
            Some(a) if a == i => previous.and_then(|p| self.docs.iter().position(|d| d.uid == p)).or_else(|| self.docs.len().checked_sub(1)),
            Some(a) if a > i => Some(a - 1),
            a => a,
        };
        Some(uid)
    }

    /// Create in the New Document window, with the settings the preview shows: the preview
    /// document becomes the new document, as `file.new` would make it (the next Untitled number,
    /// no undo history). Returns its index, or `None` when there is no unedited preview to keep.
    pub fn keep_new_document_preview(&mut self) -> Option<usize> {
        let (uid, _) = self.new_document_preview?;
        let i = self.docs.iter().position(|d| d.uid == uid && !d.is_dirty() && d.path.is_none())?;
        self.new_document_preview = None;
        self.untitled += 1;
        if let Some(d) = self.docs.get_mut(i) {
            d.history.undo.clear();
            d.history.redo.clear();
        }
        self.active = Some(i);
        Some(i)
    }
}

fn save_preset(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").map(str::trim).filter(|n| !n.is_empty()).ok_or_else(|| bad("file.savePreset", "missing `name`"))?.to_string();
    if name.chars().count() > MAX_PRESET_NAME {
        return Err(bad("file.savePreset", format!("a preset name has at most {MAX_PRESET_NAME} characters")));
    }
    if NewDocument::from_preset(&name).is_some() {
        return Err(bad("file.savePreset", format!("`{name}` is a built-in preset")));
    }
    let mut nd = NewDocument::default();
    apply_settings(&mut nd, p);
    check_size("file.savePreset", &nd)?;
    nd.title = name.clone();
    let presets = &mut s.prefs.document_presets;
    presets.retain(|x| !x.title.eq_ignore_ascii_case(&name));
    if presets.len() >= MAX_PRESETS {
        return Err(bad("file.savePreset", format!("at most {MAX_PRESETS} saved presets")));
    }
    presets.push(nd);
    Ok(json!({"name": name}))
}

fn check_size(cmd: &str, nd: &NewDocument) -> Result<()> {
    if !(nd.width > 0.0 && nd.height > 0.0 && nd.width <= 15552.0 && nd.height <= 15552.0) {
        return Err(bad(cmd, "page size out of range (0 < size ≤ 216 in)"));
    }
    Ok(())
}

/// The document settings in `p` (as file.new takes them) over `nd`.
fn apply_settings(nd: &mut NewDocument, p: &Value) {
    if let Some(intent) = p.get("intent").and_then(|v| serde_json::from_value(v.clone()).ok()) {
        nd.intent = intent;
    }
    if let Some(units) = p.get("units").and_then(|v| serde_json::from_value(v.clone()).ok()) {
        nd.units = units;
    }
    nd.width = f64_or(p, "width", nd.width);
    nd.height = f64_or(p, "height", nd.height);
    nd.pages = p.get("pages").and_then(Value::as_u64).map(|v| v as usize).unwrap_or(nd.pages).clamp(1, 9999);
    nd.start_page = p.get("startPage").and_then(Value::as_u64).map(|v| v.clamp(1, 99_999) as u32).unwrap_or(nd.start_page);
    nd.facing_pages = p.get("facingPages").and_then(Value::as_bool).unwrap_or(nd.facing_pages);
    nd.columns = p.get("columns").and_then(Value::as_u64).map(|v| v.clamp(1, 216) as u32).unwrap_or(nd.columns);
    nd.gutter = f64_or(p, "gutter", nd.gutter);
    nd.primary_text_frame = p.get("primaryTextFrame").and_then(Value::as_bool).unwrap_or(nd.primary_text_frame);
    match p.get("margins") {
        Some(Value::Number(n)) => nd.margins = designcraft_doc::Margins::uniform(n.as_f64().unwrap_or(36.0)),
        Some(v @ Value::Object(_)) => {
            if let Ok(m) = serde_json::from_value(v.clone()) {
                nd.margins = m;
            }
        }
        _ => {}
    }
    if let Some(b) = edges(p, "bleed") {
        nd.bleed = b;
    }
    if let Some(b) = edges(p, "slug") {
        nd.slug = b;
    }
}

/// A bleed or slug parameter: one width for every edge, or [top, bottom, inside, outside].
fn edges(p: &Value, key: &str) -> Option<[f64; 4]> {
    let clean = |v: f64| if v.is_finite() { v.clamp(0.0, 1296.0) } else { 0.0 };
    match p.get(key)? {
        Value::Array(a) => match a.iter().map(Value::as_f64).collect::<Option<Vec<f64>>>()?.as_slice() {
            [t, b, i, o] => Some([clean(*t), clean(*b), clean(*i), clean(*o)]),
            _ => None,
        },
        v => v.as_f64().map(|b| [clean(b); 4]),
    }
}

fn file_sample(s: &mut Session, _p: &Value) -> Result<Value> {
    let d = crate::sample::magazine();
    let i = s.add_document(DocState::new(d, None));
    Ok(json!({"index": i}))
}

fn has_recent(s: &Session) -> std::result::Result<(), String> {
    if s.prefs.recent_files.is_empty() { Err("no recent files".into()) } else { Ok(()) }
}

/// A recent file's first-page picture as straight RGBA, for the start screen.
pub struct RecentPreview {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// The picture a `.designcraft` file saved of its first page; None when the file is gone, isn't
/// ours, or was saved before files had one (it gets one when saved again).
pub fn recent_preview(path: &str) -> Option<RecentPreview> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        // Only the zip directory and the preview entry are read, not the whole document.
        let png = designcraft_format::read_preview(std::io::BufReader::new(crate::io::open(path).ok()?))?;
        let img = designcraft_images::decode_rgba(&png)?;
        let (width, height) = img.dimensions();
        Some(RecentPreview { width, height, rgba: img.into_raw() })
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = path;
        None
    }
}

/// One File › Open Recent row: the path, its file name and whether it is still there.
fn recent_entry(path: &str) -> Value {
    let name = std::path::Path::new(path).file_name().map_or_else(|| path.to_string(), |n| n.to_string_lossy().to_string());
    #[cfg(not(target_arch = "wasm32"))]
    let exists = crate::io::is_file(path);
    #[cfg(target_arch = "wasm32")]
    let exists = false;
    json!({"path": path, "name": name, "exists": exists})
}

fn file_open_recent(s: &mut Session, p: &Value) -> Result<Value> {
    let index =
        p.get("index").and_then(Value::as_u64).and_then(|i| usize::try_from(i).ok()).ok_or_else(|| bad("file.openRecent", "missing `index`"))?;
    let path = s
        .prefs
        .recent_files
        .get(index)
        .cloned()
        .ok_or_else(|| bad("file.openRecent", format!("no recent file {index} (the list has {})", s.prefs.recent_files.len())))?;
    #[cfg(not(target_arch = "wasm32"))]
    if !crate::io::is_file(&path) {
        return Err(EngineError::Other(format!("{path} can't be found: it was moved, renamed or deleted, or its disk isn't connected")));
    }
    file_open(s, &json!({"path": path}))
}

/// `file.open`: the document opens and moves to the front of File › Open Recent.
fn file_open(s: &mut Session, p: &Value) -> Result<Value> {
    let r = open_path(s, p)?;
    if let Some(path) = str_param(p, "path") {
        s.prefs.note_recent_file(path);
    }
    Ok(r)
}

fn open_path(s: &mut Session, p: &Value) -> Result<Value> {
    let path = str_param(p, "path").ok_or_else(|| bad("file.open", "missing `path`"))?;
    #[cfg(not(target_arch = "wasm32"))]
    {
        let bytes = crate::io::read_file(path).map_err(EngineError::Other)?;
        // The content decides first: a file's extension may be missing or wrong. The extension
        // then routes files whose content isn't recognised to the reader that explains why.
        let ext = std::path::Path::new(path).extension().and_then(std::ffi::OsStr::to_str).unwrap_or_default().to_ascii_lowercase();
        let native = designcraft_format::is_designcraft(&bytes) || bytes.trim_ascii_start().starts_with(b"{");
        if designcraft_idml::is_idml(&bytes) || (!native && (super::interchange::is_indd(path, &bytes) || ext == "idml")) {
            return super::interchange::open_idml(s, p);
        }
        if !native && ext != designcraft_format::EXTENSION {
            return Err(bad("file.open", format!("{path}: not a DesignCraft or IDML file")));
        }
        let (mut d, mut warnings) = load_bytes(&bytes)?;
        super::interchange::resolve_packaged_links(&mut d, Some(std::path::Path::new(path)));
        super::datamerge::resolve_sources_on_open(&mut d, Some(std::path::Path::new(path)));
        let (fonts, faces, font_warnings) = load_document_fonts(&mut d, path);
        warnings.extend(font_warnings);
        warnings.extend(super::style::grep_style_warnings(&d));
        let mut st = DocState::new(d, Some(path.to_string()));
        st.fonts = fonts;
        let i = s.add_document(st);
        Ok(json!({"index": i, "documentFonts": faces, "warnings": warnings}))
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (s, path);
        Err(EngineError::Other("use file.openBytes on the web".into()))
    }
}

/// Load the fonts in the `Document Fonts` folder beside the document file at `path` for `d` alone
/// (`d.font_scope`) → (the scope, for the document's state to hold; faces; a warning for each
/// font file skipped). Nothing on the web (no folders).
pub(crate) fn load_document_fonts(d: &mut Document, path: &str) -> (Option<Arc<crate::FontScope>>, usize, Vec<String>) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use designcraft_fonts::DOCUMENT_FONTS_FOLDER;
        let Some(folder) = std::path::Path::new(path).parent().map(document_fonts_folder) else { return (None, 0, Vec::new()) };
        // A document's folder is the agent's to name (a recovery entry or a book lists paths too):
        // the fonts crate asks the automation roots about the folder and each font file, so fonts
        // outside the read root stay unloaded.
        let r = designcraft_fonts::FontDb::global().load_document_fonts(&folder, &|p| crate::file_access::check_read(p));
        d.font_scope = r.scope;
        let scope = (r.scope != 0).then(|| Arc::new(crate::FontScope(r.scope)));
        (scope, r.faces, r.skipped.into_iter().map(|s| format!("{DOCUMENT_FONTS_FOLDER}: {s}")).collect())
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (d, path);
        (None, 0, Vec::new())
    }
}

/// The `Document Fonts` folder in `dir`, matched without regard to case: packages name it
/// `Document fonts`, and file systems on Linux are case-sensitive. The exact name wins.
#[cfg(not(target_arch = "wasm32"))]
fn document_fonts_folder(dir: &std::path::Path) -> std::path::PathBuf {
    use designcraft_fonts::DOCUMENT_FONTS_FOLDER;
    let exact = dir.join(DOCUMENT_FONTS_FOLDER);
    if crate::io::is_dir(&exact) {
        return exact;
    }
    crate::io::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .find(|e| e.file_name().to_str().is_some_and(|n| n.eq_ignore_ascii_case(DOCUMENT_FONTS_FOLDER)) && crate::io::is_dir(e.path()))
        .map_or(exact, |e| e.path())
}

fn file_open_bytes(s: &mut Session, p: &Value) -> Result<Value> {
    let b = base64_decode(str_param(p, "base64").unwrap_or(""));
    // IDML packages are recognised by their stored `mimetype` first entry, INDD files by their
    // signature (or their name).
    if designcraft_idml::is_idml(&b) || super::interchange::is_indd(str_param(p, "name").unwrap_or_default(), &b) {
        return super::interchange::open_idml(s, p);
    }
    let (mut d, warnings) = load_bytes(&b)?;
    super::datamerge::resolve_sources_on_open(&mut d, None);
    if let Some(n) = str_param(p, "name") {
        d.title = n.to_string();
    }
    let i = s.add_document(DocState::new(d, None));
    Ok(json!({"index": i, "warnings": warnings}))
}

fn file_save(s: &mut Session, p: &Value) -> Result<Value> {
    let st = s.doc_mut()?;
    // Saving to the document's own file keeps its name, even one opened without the extension.
    let path = str_param(p, "path")
        .map(with_document_extension)
        .or_else(|| st.path.clone())
        .ok_or_else(|| bad("file.save", "missing `path` (document has never been saved)"))?;
    // Before anything about the document changes: a refused save leaves it as it was.
    crate::file_access::check_write(&path).map_err(EngineError::Other)?;
    // Prepare paths for the destination without changing the live document until persistence succeeds.
    let mut saved = (*st.doc).clone();
    super::datamerge::refresh_relative_paths(&mut saved, std::path::Path::new(&path));
    let paths_changed = saved.data_merge != st.doc.data_merge;
    let saved = if paths_changed { Arc::new(saved) } else { st.doc.clone() };
    let bytes = to_bytes_with_preview(&saved, &s.cache)?;
    #[cfg(not(target_arch = "wasm32"))]
    crate::io::write_file(&path, &bytes).map_err(EngineError::Other)?;
    let st = s.doc_mut()?;
    if paths_changed {
        st.doc = saved;
        st.bump_revision();
    }
    st.path = Some(path.clone());
    st.saved_revision = st.revision;
    st.saved_doc = st.doc.clone();
    #[cfg(not(target_arch = "wasm32"))]
    {
        let recovery_dir = s.recovery_dir.clone();
        let index = s.active_index().ok_or(EngineError::NoDocument)?;
        crate::recovery::discard_document(s, index, recovery_dir.as_deref());
        s.prefs.note_recent_file(&path);
    }
    Ok(json!({"path": path, "bytes": bytes.len()}))
}

fn file_place(s: &mut Session, p: &Value) -> Result<Value> {
    // The file's size and time before it is read (a linked text file remembers them).
    let mut before = None;
    let (bytes, name, link) = if let Some(b) = str_param(p, "base64") {
        (base64_decode(b), str_param(p, "name").unwrap_or("image").to_string(), None)
    } else if let Some(path) = str_param(p, "path") {
        before = super::datamerge::fingerprint_from_path(path);
        #[cfg(not(target_arch = "wasm32"))]
        let b = crate::io::read_file(path).map_err(EngineError::Other)?;
        #[cfg(target_arch = "wasm32")]
        let b: Vec<u8> = vec![];
        let name = std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| path.into());
        (b, name, Some(path.to_string()))
    } else {
        s.ui_requests.push(crate::UiRequest::Pick { purpose: "place".into(), params: json!({}) });
        return ok();
    };
    // A page of another layout (IDML or DesignCraft): its objects, as one group.
    let lname = name.to_lowercase();
    if lname.ends_with(".idml") || lname.ends_with(".designcraft") {
        return place_layout_page(s, p, &name, &bytes);
    }
    // Keep the EPS source for link comparison; rendering uses its preview at the bounding box size.
    let eps_size = eps_size(&bytes, "file.place")?;
    // Video and sound: a media frame (Window › Interactive › Media).
    if let Some(mime) = designcraft_doc::media_mime(&name) {
        return super::media::place_media(s, p, name, mime, bytes, link);
    }
    // Text files (plain, Word, RTF) flow into frames.
    if designcraft_textimport::is_text_file(&name) {
        return super::place_text::place_text(s, p, &name, &bytes, link, before);
    }
    let want_w = p
        .get("width")
        .filter(|value| !value.is_null())
        .map(|value| {
            value
                .as_f64()
                .filter(|width| width.is_finite() && *width > 0.0)
                .ok_or_else(|| bad("file.place", "`width` must be a finite number greater than zero"))
        })
        .transpose()?;
    // Image Import Options: which page of a PDF (1-based).
    if designcraft_render::is_pdf(&bytes) {
        designcraft_render::check_pdf_input_size(bytes.len()).map_err(|e| bad("file.place", e.to_string()))?;
    }
    let pdf_page = match p.get("pdfPage").and_then(Value::as_u64) {
        Some(n) if designcraft_render::is_pdf(&bytes) => {
            let count = designcraft_render::pdf_page_count(&bytes).unwrap_or(1);
            let invalid_page = || bad("file.place", format!("the PDF has {count} page(s)"));
            let one_based = usize::try_from(n).map_err(|_| invalid_page())?;
            if one_based == 0 || one_based > count {
                return Err(invalid_page());
            }
            let zero_based = one_based.checked_sub(1).ok_or_else(invalid_page)?;
            u32::try_from(zero_based).map_err(|_| invalid_page())?
        }
        _ => 0,
    };
    let (pw, ph) = match pdf_page {
        0 => designcraft_render::image_size(&bytes).ok_or_else(|| bad("file.place", "unsupported or corrupt image"))?,
        n => designcraft_render::pdf_page_size(&bytes, n as usize)
            .map(|(w, h)| (w.round().max(1.0) as u32, h.round().max(1.0) as u32))
            .ok_or_else(|| bad("file.place", "can't read that PDF page"))?,
    };
    // 72 ppi by default unless the file says otherwise; scale so it fits the page when huge. PDF pages
    // and SVGs keep their exact size: rounding it to whole points would stretch them unevenly.
    let (nw, nh) = eps_size.or_else(|| designcraft_render::natural_size(&bytes, pdf_page as usize)).unwrap_or((pw as f64, ph as f64));
    // Image Import Options › Crop to: the graphic spans that box of the PDF page.
    let is_pdf = designcraft_render::is_pdf(&bytes);
    let pdf_crop = match str_param(p, "pdfCrop") {
        Some(k) if is_pdf => PdfCrop::from_name(k)
            .ok_or_else(|| bad("file.place", format!("unknown pdfCrop `{k}` (crop, art, trim, bleed, media, contentVisible, contentAll)")))?,
        _ => PdfCrop::Crop,
    };
    let mime = designcraft_render::image_mime(&bytes).to_string();
    let mut asset = Asset { page: pdf_page, name, mime, link, data: Arc::new(bytes), pixels: Some((pw, ph)), pdf_crop, ..Default::default() };
    // The graphic's natural size: the box in points (the whole page when the box can't be found,
    // and then the document says so).
    let (bw, bh) = match crop_pdf(&mut asset) {
        Some(size) => size,
        None => {
            asset.pdf_crop = PdfCrop::Crop;
            (nw, nh)
        }
    };
    let target_frame = super::id_param(p, "frame").or_else(|| {
        if ["x", "y", "spread", "width"].iter().any(|key| p.get(*key).is_some_and(|value| !value.is_null())) {
            return None;
        }
        let st = s.active()?;
        st.selection.items.iter().copied().find(|i| st.doc.item(*i).is_some_and(|it| matches!(it.content, Content::Unassigned | Content::Graphic(_))))
    });
    // `"spread": null` counts as omitted, like the other placement parameters.
    let spread = match p.get("spread") {
        Some(Value::Null) => designcraft_doc::SpreadRef::Doc(0),
        _ => super::spread_param(p, "file.place")?,
    };
    let lid = s.doc()?.active_layer;
    let px = p.get("x").and_then(Value::as_f64);
    let py = p.get("y").and_then(Value::as_f64);
    if target_frame.is_none() && px.is_none() && py.is_none() && p.get("loadCursor").and_then(Value::as_bool) == Some(true) {
        return load_cursor(s, asset, (bw, bh));
    }
    s.edit(|d, sel| {
        let fresh = AssetId(d.alloc());
        let aid = d.add_asset(Asset { id: fresh, ..asset });
        // The width the whole page (or image) gets; the height follows the proportions.
        let w = match want_w {
            Some(w) => w,
            None => {
                let page_w = d.settings.page_width * 0.6;
                nw.min(page_w)
            }
        };
        let id = if let Some(fid) = target_frame {
            let it = d.item_mut(fid).ok_or(designcraft_doc::DocError::NoItem(fid))?;
            let r = it.inner_bounds();
            // Fill the frame proportionally once. Auto-Fit stays off, so resizing the frame crops.
            let k = (r.width() / bw).max(r.height() / bh);
            let (gw, gh) = (bw * k, bh * k);
            let xf = Affine::translate((r.x0 + (r.width() - gw) / 2.0, r.y0 + (r.height() - gh) / 2.0)) * Affine::scale(k);
            validate_placement((bw, bh), r, xf, "file.place")?;
            it.content = Content::Graphic(Graphic {
                layer_options: None,
                asset: aid,
                size: (bw, bh),
                xf,
                clip: None,
                auto_fit: Default::default(),
                fit_align: 4,
                crop: [0.0; 4],
                wrap: Default::default(),
                fill: Default::default(),
            });
            fid
        } else {
            let sp = d.spread(spread).ok_or_else(|| bad("file.place", "no such spread"))?;
            let pr = sp.pages.first().map(|pg| pg.margin_rect()).unwrap_or(Rect::new(36.0, 36.0, 300.0, 300.0));
            let (x, y) = (px.unwrap_or(pr.x0), py.unwrap_or(pr.y0));
            let id = ItemId(d.alloc());
            let k = w / nw;
            let r = super::finite_rect("file.place", Rect::new(x, y, x + bw * k, y + bh * k))?;
            let xf = Affine::translate((x, y)) * Affine::scale(k);
            validate_placement((bw, bh), r, xf, "file.place")?;
            let mut it = Item::new(id, lid, Shape::Rectangle, shapes::rectangle(r));
            it.object_style = d.styles.default_graphic_frame.clone();
            if let Some(e) = d.styles.object_effects(&it.object_style) {
                it.effects = e.clone();
            }
            it.content = Content::Graphic(Graphic {
                layer_options: None,
                asset: aid,
                size: (bw, bh),
                xf,
                clip: None,
                auto_fit: Default::default(),
                fit_align: 4,
                crop: [0.0; 4],
                wrap: Default::default(),
                fill: Default::default(),
            });
            d.insert_item(spread, it, None)?;
            id
        };
        *sel = Selection::items(vec![id]);
        Ok(json!({"id": id.0, "asset": aid.0}))
    })
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn base64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        out.push(B64[(n >> 18) as usize & 63] as char);
        out.push(B64[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { B64[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { B64[n as usize & 63] as char } else { '=' });
    }
    out
}

pub fn base64_decode(s: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let mut buf = 0u32;
    let mut bits = 0;
    for b in s.bytes() {
        let v = match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => continue,
        } as u32;
        buf = buf << 6 | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
        }
    }
    out
}

/// Sets `a.pdf_box` to where its `pdf_crop` box sits on its page and returns the box's size in
/// points. The crop box is the whole page and has none; nor has a page that can't be read, which
/// keeps its crop choice (a missing link relinked later gets its box then).
pub(crate) fn crop_pdf(a: &mut Asset) -> Option<(f64, f64)> {
    a.pdf_box = None;
    if a.pdf_crop == PdfCrop::Crop || !designcraft_render::is_pdf(&a.data) {
        return None;
    }
    let (frac, size) = designcraft_render::pdf_crop_box(&a.data, a.page as usize, a.pdf_crop)?;
    a.pdf_box = Some(frac);
    Some(size)
}

pub(crate) fn read_source(p: &Value) -> Result<(Vec<u8>, String, Option<String>)> {
    if let Some(b) = str_param(p, "base64") {
        return Ok((base64_decode(b), str_param(p, "name").unwrap_or("image").to_string(), None));
    }
    let path = str_param(p, "path").ok_or_else(|| bad("place", "missing `path` or `base64`"))?;
    #[cfg(not(target_arch = "wasm32"))]
    let b = crate::io::read_file(path).map_err(EngineError::Other)?;
    #[cfg(target_arch = "wasm32")]
    let b: Vec<u8> = vec![];
    let name = std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| path.into());
    Ok((b, name, Some(path.to_string())))
}

fn eps_size(bytes: &[u8], command: &str) -> Result<Option<(f64, f64)>> {
    if !designcraft_images::is_eps(bytes) {
        return Ok(None);
    }
    designcraft_images::eps_size(bytes)
        .map(Some)
        .ok_or_else(|| bad(command, "the EPS has a missing or invalid bounding box (dimensions must be 0.001–20,000 points)"))
}

/// Fitting can overflow even with finite inputs. Never persist NaN/Inf as JSON nulls.
pub(crate) fn validate_placement(size: (f64, f64), frame: Rect, xf: Affine, command: &str) -> Result<()> {
    let valid_rect = |r: Rect| [r.x0, r.y0, r.x1, r.y1, r.width(), r.height()].iter().all(|n| n.is_finite()) && r.width() >= 0.0 && r.height() >= 0.0;
    if !size.0.is_finite()
        || !size.1.is_finite()
        || size.0 <= 0.0
        || size.1 <= 0.0
        || !valid_rect(frame)
        || !xf.as_coeffs().iter().all(|n| n.is_finite())
        || !valid_rect(xf.transform_rect_bbox(Rect::new(0.0, 0.0, size.0, size.1)))
    {
        return Err(bad(command, "image placement exceeds finite geometry limits"));
    }
    Ok(())
}

fn place_load(s: &mut Session, p: &Value) -> Result<Value> {
    let (bytes, name, link) = read_source(p)?;
    let (pw, ph) = designcraft_render::image_size(&bytes).ok_or_else(|| bad("place.load", "unsupported or corrupt image"))?;
    let natural = eps_size(&bytes, "place.load")?.or_else(|| designcraft_render::natural_size(&bytes, 0)).unwrap_or((pw as f64, ph as f64));
    let mime = designcraft_render::image_mime(&bytes).to_string();
    let asset = Asset { name, mime, link, data: Arc::new(bytes), pixels: Some((pw, ph)), ..Default::default() };
    load_cursor(s, asset, natural)
}

/// Adds `asset` to the document (with a fresh id) and loads it into the place cursor.
fn load_cursor(s: &mut Session, mut asset: Asset, size: (f64, f64)) -> Result<Value> {
    let name = asset.name.clone();
    let (pw, ph) = asset.pixels.unwrap_or_default();
    let aid = s.edit(|d, _| {
        asset.id = AssetId(d.alloc());
        Ok(d.add_asset(asset))
    })?;
    s.loaded = Some(crate::LoadedGraphic { asset: aid, size });
    s.set_tool("placeGun");
    Ok(json!({"asset": aid.0, "name": name, "width": pw, "height": ph, "loaded": true}))
}

fn place_drop(s: &mut Session, p: &Value) -> Result<Value> {
    let crate::LoadedGraphic { asset: aid, size: (nw, nh) } = s.loaded.ok_or_else(|| bad("place.drop", "nothing loaded in the place cursor"))?;
    // Undo can remove the loaded graphic's asset.
    if !s.doc()?.doc.assets.contains_key(&aid) {
        s.loaded = None;
        return Err(bad("place.drop", "the loaded graphic is no longer in the document"));
    }
    let sr = super::spread_param(p, "place.drop")?;
    let lid = s.doc()?.active_layer;
    let frame = super::id_param(p, "frame");
    let rect = super::rect_param(p, "rect");
    let (x, y) = (super::f64_or(p, "x", 0.0), super::f64_or(p, "y", 0.0));
    let d = &s.doc()?.doc;
    let (frame_rect, xf, auto_fit) = if let Some(fid) = frame {
        let r = d.item(fid).ok_or(designcraft_doc::DocError::NoItem(fid))?.inner_bounds();
        let k = (r.width() / nw).max(r.height() / nh);
        let xf = Affine::translate((r.x0 + (r.width() - nw * k) / 2.0, r.y0 + (r.height() - nh * k) / 2.0)) * Affine::scale(k);
        // Fill the frame proportionally once. Auto-Fit stays off, so resizing the frame crops.
        (r, xf, Default::default())
    } else {
        if d.spread(sr).is_none() {
            return Err(bad("place.drop", "no such spread"));
        }
        // Drag: fit proportionally; click: actual size at the point.
        let (r, k) = match rect {
            Some(r) if r.width() > 2.0 && r.height() > 2.0 => {
                let k = (r.width() / nw).min(r.height() / nh);
                (Rect::new(r.x0, r.y0, r.x0 + nw * k, r.y0 + nh * k), k)
            }
            _ => (Rect::new(x, y, x + nw, y + nh), 1.0),
        };
        (r, Affine::translate((r.x0, r.y0)) * Affine::scale(k), Default::default())
    };
    validate_placement((nw, nh), frame_rect, xf, "place.drop")?;
    let r = s.edit(|d, sel| {
        let graphic = Content::Graphic(Graphic {
            layer_options: None,
            asset: aid,
            size: (nw, nh),
            xf,
            clip: None,
            auto_fit,
            fit_align: 4,
            crop: [0.0; 4],
            wrap: Default::default(),
            fill: Default::default(),
        });
        let id = if let Some(fid) = frame {
            d.item_mut(fid).ok_or(designcraft_doc::DocError::NoItem(fid))?.content = graphic;
            fid
        } else {
            let id = ItemId(d.alloc());
            let mut it = Item::new(id, lid, Shape::Rectangle, shapes::rectangle(frame_rect));
            it.object_style = d.styles.default_graphic_frame.clone();
            if let Some(e) = d.styles.object_effects(&it.object_style) {
                it.effects = e.clone();
            }
            it.content = graphic;
            d.insert_item(sr, it, None)?;
            id
        };
        *sel = Selection::items(vec![id]);
        Ok(json!({"id": id.0}))
    })?;
    s.loaded = None;
    s.set_tool("selection");
    Ok(r)
}

/// The selection as `.designcraft` snippet bytes.
pub(crate) fn snippet_bytes(s: &mut Session) -> Result<Vec<u8>> {
    to_bytes(&super::edit::clip_doc(s)?)
}

fn snippet_export(s: &mut Session, p: &Value) -> Result<Value> {
    let d = super::edit::clip_doc(s)?;
    let bytes = to_bytes(&d)?;
    match str_param(p, "path") {
        Some(path) => {
            #[cfg(not(target_arch = "wasm32"))]
            crate::io::write_file(path, &bytes).map_err(EngineError::Other)?;
            Ok(json!({"path": path, "bytes": bytes.len(), "items": d.spreads.first().map(|s| s.items.len()).unwrap_or(0)}))
        }
        None => Ok(json!({"base64": base64_encode(&bytes), "bytes": bytes.len()})),
    }
}

/// File › Place of an IDML or DesignCraft document: the objects of one page (`layoutPage`,
/// 1-based) grouped, top-left at x/y (default: the source position on the first page).
fn place_layout_page(s: &mut Session, p: &Value, name: &str, bytes: &[u8]) -> Result<Value> {
    const ID: &str = "file.place";
    let src = if name.to_lowercase().ends_with(".idml") {
        super::interchange::import_idml_confined(bytes, str_param(p, "path").map(std::path::Path::new)).map_err(|e| bad(ID, e.to_string()))?
    } else {
        from_bytes(bytes)?
    };
    let n = src.page_count();
    let page = p.get("layoutPage").and_then(Value::as_u64).unwrap_or(1) as usize;
    if page == 0 || page > n {
        return Err(bad(ID, format!("the document has {n} page(s)")));
    }
    let (si, pi) = src.page_loc(page - 1).ok_or_else(|| bad(ID, "no such page"))?;
    let sp = &src.spreads[si];
    let pr = sp.pages[pi].bounds();
    let ids: Vec<ItemId> = sp.items.iter().filter(|it| !it.hidden && sp.page_at_x(it.bounds().center().x) == Some(pi)).map(|it| it.id).collect();
    if ids.is_empty() {
        return Err(bad(ID, format!("page {page} has no objects")));
    }
    let to = super::spread_param(p, ID)?;
    let off = match (p.get("x").and_then(Value::as_f64), p.get("y").and_then(Value::as_f64)) {
        (Some(x), Some(y)) => designcraft_geom::Vec2::new(x - pr.x0, y - pr.y0),
        _ => designcraft_geom::Vec2::new(-pr.x0, -pr.y0),
    };
    let placed = s.edit(|d, sel| {
        for sw in &src.swatches {
            if d.swatch(&sw.name).is_none() {
                d.swatches.push(sw.clone());
            }
        }
        let missing_p: Vec<_> = src.styles.paragraph.iter().filter(|ps| d.styles.para(&ps.name).is_none()).cloned().collect();
        let missing_c: Vec<_> = src.styles.character.iter().filter(|cs| d.styles.char_style(&cs.name).is_none()).cloned().collect();
        if !missing_p.is_empty() || !missing_c.is_empty() {
            let st = d.styles_mut();
            st.paragraph.extend(missing_p);
            st.character.extend(missing_c);
        }
        let new = super::object::duplicate_from(d, &src, &ids, to, off)?;
        *sel = Selection::items(new.clone());
        Ok(new)
    })?;
    if placed.len() > 1 {
        let g = s.execute("object.group", &json!({"ids": placed.iter().map(|i| i.0).collect::<Vec<_>>()}))?;
        return Ok(json!({"id": g["id"], "items": placed.len()}));
    }
    Ok(json!({"id": placed[0].0, "items": 1}))
}

pub(crate) fn snippet_place(s: &mut Session, p: &Value) -> Result<Value> {
    let (bytes, _, _) = read_source(p)?;
    let src = from_bytes(&bytes)?;
    let ids: Vec<ItemId> = src.spreads.first().map(|sp| sp.items.iter().map(|i| i.id).collect()).unwrap_or_default();
    if ids.is_empty() {
        return Err(bad("snippet.place", "the snippet is empty"));
    }
    let sr = super::spread_param(p, "snippet.place")?;
    let bounds = ids.iter().filter_map(|i| src.item(*i).map(|it| it.bounds())).reduce(|a, b| a.union(b)).unwrap_or(Rect::ZERO);
    let off = match (p.get("x").and_then(Value::as_f64), p.get("y").and_then(Value::as_f64)) {
        (Some(x), Some(y)) => designcraft_geom::Vec2::new(x - bounds.x0, y - bounds.y0),
        _ => designcraft_geom::Vec2::ZERO,
    };
    s.edit(|d, sel| {
        // Bring over styles and swatches the snippet uses that this document lacks.
        for sw in &src.swatches {
            if d.swatch(&sw.name).is_none() {
                d.swatches.push(sw.clone());
            }
        }
        let missing_p: Vec<_> = src.styles.paragraph.iter().filter(|ps| d.styles.para(&ps.name).is_none()).cloned().collect();
        let missing_c: Vec<_> = src.styles.character.iter().filter(|cs| d.styles.char_style(&cs.name).is_none()).cloned().collect();
        if !missing_p.is_empty() || !missing_c.is_empty() {
            let st = d.styles_mut();
            st.paragraph.extend(missing_p);
            st.character.extend(missing_c);
        }
        let new = super::object::duplicate_from(d, &src, &ids, sr, off)?;
        *sel = Selection::items(new.clone());
        Ok(json!({"ids": new.iter().map(|i| i.0).collect::<Vec<_>>()}))
    })
}

/// Which access a recovery command needs to the folder it is given.
#[derive(Clone, Copy)]
enum FolderAccess {
    Read,
    Write,
    ReadWrite,
}

/// The recovery folder: `dir` when the command names one, else the session's own (set by the
/// host application, so not the automation roots' business). A named folder is checked against
/// the roots for `access`.
fn recovery_dir(s: &Session, p: &Value, access: FolderAccess) -> Result<std::path::PathBuf> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (s, p, access);
        Err(EngineError::Other("no recovery folder on the web".into()))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let Some(named) = str_param(p, "dir") else {
            return s.recovery_dir.clone().ok_or_else(|| bad("file.recovery", "no recovery folder (give `dir`)"));
        };
        if matches!(access, FolderAccess::Read | FolderAccess::ReadWrite) {
            crate::file_access::check_read(named).map_err(EngineError::Other)?;
        }
        if matches!(access, FolderAccess::Write | FolderAccess::ReadWrite) {
            crate::file_access::check_write(named).map_err(EngineError::Other)?;
        }
        Ok(std::path::PathBuf::from(named))
    }
}

fn can_revert(s: &Session) -> std::result::Result<(), String> {
    match s.active() {
        Some(d) if d.path.is_some() && d.is_dirty() => Ok(()),
        Some(d) if d.path.is_none() => Err("the document has never been saved".into()),
        Some(_) => Err("no changes since the last save".into()),
        None => Err("no document open".into()),
    }
}

fn file_revert(s: &mut Session, _: &Value) -> Result<Value> {
    let path = s.doc()?.path.clone().ok_or_else(|| bad("file.revert", "the document has never been saved"))?;
    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut d = if path.to_ascii_lowercase().ends_with(".idml") {
            let bytes = crate::io::read_file(&path).map_err(EngineError::Other)?;
            super::interchange::import_idml_confined(&bytes, Some(std::path::Path::new(&path))).map_err(|e| EngineError::Other(e.to_string()))?
        } else {
            from_bytes(&crate::io::read_file(&path).map_err(EngineError::Other)?)?
        };
        // As opened again: the fonts beside it now (unchanged files are shared, not read again).
        let (fonts, _, _) = load_document_fonts(&mut d, &path);
        let uid = s.doc()?.uid;
        let recovery_dir = s.recovery_dir.clone();
        let index = s.active_index().ok_or(EngineError::NoDocument)?;
        let mut fresh = DocState::new(d, Some(path.clone()));
        fresh.fonts = fonts;
        fresh.uid = uid;
        // A new state has a revision no state had before, so caches don't show the old document.
        crate::recovery::discard_document(s, index, recovery_dir.as_deref());
        *s.doc_mut()? = fresh;
        Ok(json!({"path": path}))
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (s, path);
        Err(EngineError::Other("revert isn't available on the web".into()))
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod image_place_tests {
    use designcraft_doc::{Content, ItemId};
    use designcraft_geom::Rect;
    use serde_json::{Value, json};

    use crate::Session;

    pub(super) fn image_file() -> std::io::Result<tempfile::NamedTempFile> {
        image_file_with_color(30)
    }

    fn image_file_with_color(red: u8) -> std::io::Result<tempfile::NamedTempFile> {
        let file = tempfile::Builder::new().suffix(".png").tempfile()?;
        let png = designcraft_render::Rendered { width: 40, height: 20, pixels: [red, 90, 180, 255].repeat(800) }.to_png();
        std::fs::write(file.path(), png)?;
        Ok(file)
    }

    #[test]
    fn consecutive_path_placements_create_distinct_graphics() {
        let images = [image_file_with_color(30).unwrap(), image_file_with_color(31).unwrap(), image_file_with_color(32).unwrap()];
        let mut session = Session::new();
        session.execute("file.new", &json!({"pages": 3, "facingPages": false})).unwrap();
        let mut placements = Vec::new();
        let mut originals = Vec::new();

        for ((spread, x, y), image) in [(0, 0.0, 0.0), (1, 70.0, 90.0), (2, 150.0, 210.0)].into_iter().zip(&images) {
            let placed = session.execute("file.place", &json!({"path": image.path(), "spread": spread, "x": x, "y": y})).unwrap();
            let id = ItemId(placed["id"].as_u64().unwrap());
            assert!(
                placements.iter().all(|previous: &Value| previous["id"] != placed["id"]),
                "each explicit placement needs a distinct frame: {placed}"
            );
            let document = &session.doc().unwrap().doc;
            let item = document.item(id).unwrap();
            assert_eq!(item.bounds(), Rect::new(x, y, x + 40.0, y + 20.0));
            assert!(document.spreads[spread].items.iter().any(|item| item.id == id));
            originals.push(serde_json::to_value(item).unwrap());
            placements.push(placed);
        }

        let document = &session.doc().unwrap().doc;
        assert_eq!(document.spreads.iter().flat_map(|spread| &spread.items).filter(|item| matches!(item.content, Content::Graphic(_))).count(), 3);
        assert_eq!(document.assets.len(), 3);
        for (placed, original) in placements.iter().zip(originals) {
            assert_eq!(serde_json::to_value(document.item(ItemId(placed["id"].as_u64().unwrap())).unwrap()).unwrap(), original);
        }
        let links = session.execute("links.list", &json!({})).unwrap();
        assert_eq!(links.as_array().unwrap().len(), 3);
        for (placed, image) in placements.into_iter().zip(&images) {
            let row = links.as_array().unwrap().iter().find(|row| row["asset"] == placed["asset"]).unwrap();
            assert_eq!(row["path"], json!(image.path()));
            assert_eq!(row["uses"].as_array().unwrap().len(), 1);
            assert_eq!(row["uses"][0]["id"], placed["id"]);
        }
    }

    #[test]
    fn individual_placement_parameters_bypass_selected_frames() {
        let image = image_file().unwrap();
        for graphic in [false, true] {
            for (key, value, expected) in [
                ("x", 0, Rect::new(0.0, 36.0, 40.0, 56.0)),
                ("y", 0, Rect::new(36.0, 0.0, 76.0, 20.0)),
                ("spread", 0, Rect::new(36.0, 36.0, 76.0, 56.0)),
                ("width", 0, Rect::ZERO),
                ("width", 80, Rect::new(36.0, 36.0, 116.0, 76.0)),
            ] {
                let mut session = Session::new();
                session.execute("file.new", &json!({})).unwrap();
                let selected = session.execute("frame.create", &json!({"rect": [100, 120, 200, 220]})).unwrap();
                if graphic {
                    session.execute("file.place", &json!({"path": image.path()})).unwrap();
                }
                let id = ItemId(selected["id"].as_u64().unwrap());
                let original = serde_json::to_value(session.doc().unwrap().doc.item(id).unwrap()).unwrap();
                let mut params = json!({"path": image.path()});
                params[key] = json!(value);

                let result = session.execute("file.place", &params);
                if key == "width" && value == 0 {
                    assert!(result.is_err(), "zero width must be rejected, graphic={graphic}");
                    continue;
                }
                let placed = result.unwrap();

                assert_ne!(placed["id"], selected["id"], "{key}={value}, graphic={graphic}");
                let document = &session.doc().unwrap().doc;
                assert_eq!(serde_json::to_value(document.item(id).unwrap()).unwrap(), original);
                let item = document.item(ItemId(placed["id"].as_u64().unwrap())).unwrap();
                assert_eq!(item.bounds(), expected);
                let Content::Graphic(content) = &item.content else { panic!("placed frame must be graphic") };
                let [scale_x, _, _, scale_y, _, _] = content.xf.as_coeffs();
                assert!(scale_x > 0.0 && scale_y > 0.0);
            }
        }
    }

    #[test]
    fn explicit_frame_wins_over_placement_and_selection() {
        let image = image_file().unwrap();
        let mut session = Session::new();
        session.execute("file.new", &json!({})).unwrap();
        let target = session.execute("frame.create", &json!({"rect": [100, 120, 200, 220]})).unwrap();
        let selected = session.execute("frame.create", &json!({"rect": [300, 320, 400, 420]})).unwrap();

        let placed =
            session.execute("file.place", &json!({"path": image.path(), "frame": target["id"], "x": 0, "y": 0, "spread": 99, "width": 80})).unwrap();

        assert_eq!(placed["id"], target["id"]);
        let document = &session.doc().unwrap().doc;
        let item = document.item(ItemId(target["id"].as_u64().unwrap())).unwrap();
        assert_eq!(item.bounds(), Rect::new(100.0, 120.0, 200.0, 220.0));
        assert!(matches!(item.content, Content::Graphic(_)));
        assert!(matches!(document.item(ItemId(selected["id"].as_u64().unwrap())).unwrap().content, Content::Unassigned));
        assert_eq!(document.spreads[0].items.len(), 2);
    }

    fn assert_width_rejected(width: Value) -> std::result::Result<(), Box<dyn std::error::Error>> {
        let image = image_file()?;
        for graphic in [false, true] {
            for explicit_frame in [false, true] {
                let mut session = Session::new();
                session.execute("file.new", &json!({}))?;
                let target = session.execute("frame.create", &json!({"rect": [100, 120, 200, 220]}))?;
                if graphic {
                    session.execute("file.place", &json!({"path": image.path()}))?;
                }
                let mut params = json!({"path": image.path(), "width": width});
                if explicit_frame {
                    params["frame"] = target["id"].clone();
                    session.execute("frame.create", &json!({"rect": [300, 320, 400, 420]}))?;
                }
                let before = session.doc()?;
                let document = before.doc.clone();
                let selection = before.selection.clone();
                let revision = before.revision;
                let assets = document.assets.len();

                let result = session.execute("file.place", &params);

                assert!(
                    matches!(&result, Err(crate::EngineError::BadParams { .. })),
                    "width={width}, graphic={graphic}, explicit_frame={explicit_frame}: expected BadParams, got {result:?}"
                );
                let error = result.err().ok_or_else(|| crate::EngineError::Other("expected placement error".into()))?;
                let crate::EngineError::BadParams { cmd, msg } = error else { return Err(error.into()) };
                assert_eq!(cmd, "file.place");
                assert!(msg.contains("width") && msg.contains("finite") && msg.contains("greater than zero"), "{msg}");
                let after = session.doc()?;
                assert!(std::sync::Arc::ptr_eq(&document, &after.doc));
                assert_eq!(after.selection, selection);
                assert_eq!(after.revision, revision);
                assert_eq!(after.doc.assets.len(), assets);
            }
        }
        Ok(())
    }

    #[test]
    fn width_rejected_when_zero() {
        assert_width_rejected(json!(0)).unwrap();
    }

    #[test]
    fn width_rejected_when_negative() {
        assert_width_rejected(json!(-80)).unwrap();
    }

    #[test]
    fn width_rejected_when_non_numeric() {
        for width in [json!("80"), json!(true), json!([80]), json!({"value": 80})] {
            assert_width_rejected(width).unwrap();
        }
    }

    fn assert_null_placement_uses_selection(keys: &[&str]) -> std::result::Result<(), Box<dyn std::error::Error>> {
        let image = image_file()?;
        let previous_image = image_file_with_color(31)?;
        for graphic in [false, true] {
            let mut session = Session::new();
            session.execute("file.new", &json!({}))?;
            let selected = session.execute("frame.create", &json!({"rect": [100, 120, 200, 220]}))?;
            let previous = graphic.then(|| session.execute("file.place", &json!({"path": previous_image.path()}))).transpose()?;
            let mut params = json!({"path": image.path()});
            for key in keys {
                params[*key] = Value::Null;
            }

            let placed = session.execute("file.place", &params)?;

            assert_eq!(placed["id"], selected["id"], "keys={keys:?}, graphic={graphic}");
            if let Some(previous) = previous {
                assert_ne!(placed["asset"], previous["asset"]);
            }
            let document = &session.doc()?.doc;
            let id = ItemId(selected["id"].as_u64().ok_or_else(|| crate::EngineError::Other("frame.create returned no numeric id".into()))?);
            let item = document.item(id).ok_or(designcraft_doc::DocError::NoItem(id))?;
            assert_eq!(item.bounds(), Rect::new(100.0, 120.0, 200.0, 220.0));
            assert!(matches!(&item.content, Content::Graphic(_)), "placed frame must be graphic");
            let Content::Graphic(content) = &item.content else {
                return Err(crate::EngineError::Other("placed frame must be graphic".into()).into());
            };
            assert_eq!(json!(content.asset.0), placed["asset"]);
            assert_eq!(document.spreads[0].items.len(), 1);
        }
        Ok(())
    }

    #[test]
    fn selection_used_when_x_null() {
        assert_null_placement_uses_selection(&["x"]).unwrap();
    }

    #[test]
    fn selection_used_when_y_null() {
        assert_null_placement_uses_selection(&["y"]).unwrap();
    }

    #[test]
    fn selection_used_when_spread_null() {
        assert_null_placement_uses_selection(&["spread"]).unwrap();
    }

    #[test]
    fn selection_used_when_width_null() {
        assert_null_placement_uses_selection(&["width"]).unwrap();
    }

    #[test]
    fn selection_used_when_all_placement_parameters_null() {
        assert_null_placement_uses_selection(&["x", "y", "spread", "width"]).unwrap();
    }

    #[test]
    fn implicit_placement_fills_empty_and_replaces_graphic_selection() {
        let image = image_file().unwrap();
        let previous_image = image_file_with_color(31).unwrap();
        for graphic in [false, true] {
            let mut session = Session::new();
            session.execute("file.new", &json!({})).unwrap();
            let selected = session.execute("frame.create", &json!({"rect": [100, 120, 200, 220]})).unwrap();
            let previous = graphic.then(|| session.execute("file.place", &json!({"path": previous_image.path()})).unwrap());

            let placed = session.execute("file.place", &json!({"path": image.path()})).unwrap();

            assert_eq!(placed["id"], selected["id"]);
            if let Some(previous) = previous {
                assert_ne!(placed["asset"], previous["asset"]);
            }
            let document = &session.doc().unwrap().doc;
            let item = document.item(ItemId(placed["id"].as_u64().unwrap())).unwrap();
            assert_eq!(item.bounds(), Rect::new(100.0, 120.0, 200.0, 220.0));
            let Content::Graphic(content) = &item.content else { panic!("placed frame must be graphic") };
            assert_eq!(json!(content.asset.0), placed["asset"]);
            assert_eq!(document.spreads[0].items.len(), 1);
        }
    }
}

#[cfg(test)]
mod new_document_tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn saved_presets_make_documents_by_name() {
        let mut s = Session::new();
        let newsletter = json!({"name": "Newsletter", "units": "millimeters", "width": 595.2755905511812, "height": 841.8897637795277,
            "pages": 8, "facingPages": false, "columns": 3, "margins": {"top": 20, "bottom": 30, "inside": 40, "outside": 50},
            "bleed": [9, 9, 0, 9], "primaryTextFrame": true});
        assert_eq!(s.execute("file.savePreset", &newsletter).unwrap()["name"], "Newsletter");
        let listed = s.execute("file.presets", &json!({})).unwrap();
        let saved: Vec<_> = listed.as_array().unwrap().iter().filter(|p| p["saved"] == true).collect();
        assert_eq!(saved.len(), 1);
        assert_eq!((&saved[0]["name"], &saved[0]["units"]), (&json!("Newsletter"), &json!("millimeters")));
        // By name, case ignored; parameters still override the preset.
        s.execute("file.new", &json!({"preset": "newsletter", "pages": 4})).unwrap();
        let d = &s.doc().unwrap().doc;
        assert_eq!(d.page_count(), 4);
        assert!(!d.settings.facing_pages && d.settings.primary_text_frame);
        assert_eq!(d.settings.horizontal_units, designcraft_geom::Unit::Millimeters);
        assert_eq!(d.settings.bleed, [9.0, 9.0, 0.0, 9.0]);
        assert_eq!(d.page(0).unwrap().margins.outside, 50.0);
        assert!(d.title.starts_with("Untitled-"), "the preset's name isn't the document's");
        // The same name replaces it.
        s.execute("file.savePreset", &json!({"name": "NEWSLETTER", "pages": 2})).unwrap();
        assert_eq!(s.prefs.document_presets.len(), 1);
        s.execute("file.new", &json!({"preset": "Newsletter"})).unwrap();
        assert_eq!(s.doc().unwrap().doc.page_count(), 2);
        // Built-in names, empty and overlong names and impossible sizes are refused.
        for bad in [json!({"name": "a4"}), json!({"name": "  "}), json!({}), json!({"name": "x".repeat(101)}), json!({"name": "Huge", "width": 1e9})]
        {
            assert!(s.execute("file.savePreset", &bad).is_err(), "{bad}");
        }
        s.execute("file.deletePreset", &json!({"name": "newsletter"})).unwrap();
        assert!(s.execute("file.deletePreset", &json!({"name": "newsletter"})).is_err());
        assert!(s.execute("file.new", &json!({"preset": "Newsletter"})).is_err());
    }

    #[test]
    fn new_document_takes_intent_units_start_page_bleed_and_slug() {
        let mut s = Session::new();
        let p = json!({"preset": "A4", "intent": "web", "units": "pixels", "startPage": 4, "bleed": [9, 0, 3, 6], "slug": 18});
        s.execute("file.new", &p).unwrap();
        let d = &s.doc().unwrap().doc;
        assert_eq!(d.settings.intent, designcraft_doc::Intent::Web);
        assert_eq!(d.settings.horizontal_units, designcraft_geom::Unit::Pixels);
        assert_eq!(d.settings.bleed, [9.0, 0.0, 3.0, 6.0]);
        assert_eq!(d.settings.slug, [18.0; 4]);
        assert_eq!(d.page_name(0), "4");
        // Malformed edges are ignored, hostile ones bounded.
        s.execute("file.new", &json!({"bleed": [1, 2], "slug": [-5, 1e12, 0, 0]})).unwrap();
        let d = &s.doc().unwrap().doc;
        assert_eq!(d.settings.bleed, [0.0; 4]);
        assert_eq!(d.settings.slug, [0.0, 1296.0, 0.0, 0.0]);
    }

    #[test]
    fn a_new_document_preview_replaces_itself_and_leaves_the_session_as_it_was() {
        let dir = std::env::temp_dir().join(format!("dc-new-doc-preview-{}", std::process::id()));
        let mut s = Session::new();
        s.recovery_dir = Some(dir.clone());
        s.execute("file.new", &json!({})).unwrap();
        let first = s.doc().unwrap().uid;
        let a = s.preview_new_document(&json!({"width": 300})).unwrap();
        assert_eq!((s.doc().unwrap().uid, s.doc().unwrap().doc.title.as_str()), (a, "Untitled-2"));
        let b = s.preview_new_document(&json!({"width": 400, "columns": 3})).unwrap();
        assert_eq!(s.documents().len(), 2, "the preview replaces itself");
        assert!(s.preview_new_document(&json!({"width": 0})).is_err());
        let d = s.doc().unwrap();
        assert_eq!((d.uid, d.doc.settings.page_width, d.doc.page(0).unwrap().columns.count), (b, 400.0, 3), "a refused size keeps it");
        let listed = s.execute("document.list", &json!({})).unwrap();
        assert_eq!((listed[0].get("preview"), &listed[1]["preview"]), (None, &json!(true)));
        assert_eq!(s.end_new_document_preview(), Some(b));
        let d = s.doc().unwrap();
        assert_eq!((s.documents().len(), d.uid, d.is_dirty(), d.history.undo.len()), (1, first, false, 0));
        assert_eq!(s.end_new_document_preview(), None, "without a preview it does nothing");
        // Edited by a script while shown, it writes no recovery data while it is a preview, and
        // stays as a document when the window closes, with its Untitled number.
        let c = s.preview_new_document(&json!({})).unwrap();
        s.execute("frame.create", &json!({"rect": [10, 10, 100, 100]})).unwrap();
        assert_eq!(crate::recovery::save(&s, &dir).unwrap(), 0);
        // The app's timer saves through the command.
        assert_eq!(s.execute("file.recovery.save", &json!({})).unwrap()["saved"], 0);
        assert!(crate::recovery::list(&dir).is_empty());
        assert_eq!(s.end_new_document_preview(), None);
        assert_eq!((s.documents().len(), s.doc().unwrap().uid, s.doc().unwrap().doc.title.as_str()), (2, c, "Untitled-2"));
        s.execute("file.new", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().doc.title, "Untitled-3");
        // Create with the previewed settings keeps the preview as the document.
        let d = s.preview_new_document(&json!({"pages": 2})).unwrap();
        assert_eq!(s.keep_new_document_preview(), Some(3));
        let st = s.doc().unwrap();
        assert_eq!((st.uid, st.doc.title.as_str(), s.new_document_preview()), (d, "Untitled-4", None));
        s.execute("file.new", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().doc.title, "Untitled-5");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
pub(crate) mod eps_place_tests;

#[cfg(test)]
mod pdf_crop_tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn place_pdf_cropped_to_its_trim_box() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("layout.documentSetup", &json!({"bleed": 9})).unwrap();
        let b64 = s.execute("file.exportPdf", &json!({"bleed": true})).unwrap()["base64"].as_str().unwrap().to_string();
        let mut t = Session::new();
        t.execute("file.new", &json!({})).unwrap();
        let size = |t: &mut Session, crop: &str| {
            t.execute("edit.deselectAll", &json!({})).unwrap();
            let r = t.execute("file.place", &json!({"base64": b64, "name": "a.pdf", "pdfCrop": crop, "width": 630, "x": 0, "y": 0})).unwrap();
            let id = designcraft_doc::ItemId(r["id"].as_u64().unwrap());
            t.doc().unwrap().doc.item(id).unwrap().bounds()
        };
        let full = size(&mut t, "crop");
        let trim = size(&mut t, "trim");
        assert!((full.width() - 630.0).abs() < 1e-6, "{full:?}");
        assert!((trim.width() - 612.0).abs() < 1e-3 && (trim.height() - 792.0).abs() < 1e-3, "{trim:?}");
        assert!(t.execute("file.place", &json!({"base64": b64, "name": "a.pdf", "pdfCrop": "nope"})).is_err());
    }

    /// Placing with `trim` stores the choice; the graphic spans the trim box and draws only it,
    /// even in a frame larger than the graphic.
    #[test]
    fn a_pdf_placed_with_its_trim_box_stores_and_draws_that_box() {
        // Letter with a 9 pt bleed: black in the bleed's top-left corner and a black square inside.
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("layout.documentSetup", &json!({"bleed": 9})).unwrap();
        for rect in [[-9, -9, 0, 0], [10, 10, 30, 30]] {
            let id = s.execute("frame.create", &json!({"rect": rect})).unwrap()["id"].clone();
            s.execute("object.fill", &json!({"ids": [id], "swatch": "[Black]"})).unwrap();
        }
        let b64 = s.execute("file.exportPdf", &json!({"bleed": true})).unwrap()["base64"].as_str().unwrap().to_string();

        let mut t = Session::new();
        t.execute("file.new", &json!({})).unwrap();
        // `width` is the whole page's (630 pt with its bleed): half size.
        let r = t.execute("file.place", &json!({"base64": b64, "name": "a.pdf", "pdfCrop": "trim", "width": 315, "x": 100, "y": 100})).unwrap();
        let id = designcraft_doc::ItemId(r["id"].as_u64().unwrap());
        let d = t.doc().unwrap().doc.clone();
        let it = d.item(id).unwrap();
        let g = it.graphic().unwrap().clone();
        let a = &d.assets[&g.asset];
        assert_eq!(a.pdf_crop, designcraft_doc::PdfCrop::Trim);
        let b = a.shown_box().unwrap();
        let want = [9.0 / 630.0, 9.0 / 810.0, 621.0 / 630.0, 801.0 / 810.0];
        assert!(b.iter().zip(want).all(|(x, y)| (x - y).abs() < 1e-4), "{b:?}");
        assert!((g.size.0 - 612.0).abs() < 1e-3 && (g.size.1 - 792.0).abs() < 1e-3, "{:?}", g.size);
        let shown = (it.xf * g.xf).transform_rect_bbox(designcraft_geom::Rect::new(0.0, 0.0, g.size.0, g.size.1));
        let fb = it.bounds();
        for (x, y) in [(shown.x0, 100.0), (shown.y0, 100.0), (shown.x1, 406.0), (shown.y1, 496.0), (fb.x1, 406.0), (fb.y1, 496.0)] {
            assert!((x - y).abs() < 1e-3, "{shown:?} {fb:?}");
        }

        // A frame larger than the graphic still shows only the trim box.
        t.edit(|d, _| {
            d.item_mut(id).unwrap().path = designcraft_geom::shapes::rectangle(designcraft_geom::Rect::new(90.0, 90.0, 416.0, 506.0));
            Ok(())
        })
        .unwrap();
        let d = t.doc().unwrap().doc.clone();
        let mut rr = designcraft_render::Renderer::new();
        rr.threads = 0;
        let img = rr.render_page(&d, &t.cache, 0, 1.0, false, &Default::default()).unwrap();
        let dark = |x: u32, y: u32| img.pixel(x, y)[0] < 60;
        // The page sits at 95.5: its bleed corner (95.5–100) is cut off, the square (105–115) shows
        // where it is on the page.
        assert!(!dark(97, 97), "the bleed is outside the trim box");
        assert!(!dark(103, 103), "the page is offset by its bleed");
        assert!(dark(110, 110), "the square inside the trim box");
    }

    #[test]
    fn placed_pdf_keeps_its_exact_page_size() {
        // A 100.4 x 50.4 pt page placed 100.4 pt wide is 50.4 pt tall, not stretched to whole points.
        let mut s = Session::new();
        s.execute("file.new", &json!({"width": 100.4, "height": 50.4, "pages": 1})).unwrap();
        let b64 = s.execute("file.exportPdf", &json!({})).unwrap()["base64"].as_str().unwrap().to_string();
        let mut t = Session::new();
        t.execute("file.new", &json!({})).unwrap();
        let r = t.execute("file.place", &json!({"base64": b64, "name": "a.pdf", "width": 100.4, "x": 0, "y": 0})).unwrap();
        let id = designcraft_doc::ItemId(r["id"].as_u64().unwrap());
        let d = &t.doc().unwrap().doc;
        let it = d.item(id).unwrap();
        let b = it.bounds();
        assert!((b.width() - 100.4).abs() < 1e-3 && (b.height() - 50.4).abs() < 1e-3, "{b:?}");
        let designcraft_doc::Content::Graphic(g) = &it.content else { panic!("not a graphic") };
        assert!((g.size.0 - 100.4).abs() < 1e-3 && (g.size.1 - 50.4).abs() < 1e-3, "{:?}", g.size);
    }
}

#[cfg(test)]
mod layout_place_tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn place_a_page_of_an_idml_file_as_a_group() {
        let mut a = Session::new();
        a.execute("file.new", &json!({"pages": 2})).unwrap();
        a.execute("frame.create", &json!({"rect": [100, 100, 200, 150], "content": "text", "text": "From page one", "caret": false})).unwrap();
        a.execute("frame.create", &json!({"rect": [300, 400, 350, 450]})).unwrap();
        let idml = designcraft_idml::export_idml(&a.doc().unwrap().doc);
        let b64 = super::base64_encode(&idml);
        let mut b = Session::new();
        b.execute("file.new", &json!({})).unwrap();
        let r = b.execute("file.place", &json!({"base64": b64, "name": "src.idml", "layoutPage": 1, "x": 0, "y": 0})).unwrap();
        assert_eq!(r["items"], 2);
        let d = &b.doc().unwrap().doc;
        let g = d.item(designcraft_doc::ItemId(r["id"].as_u64().unwrap())).unwrap();
        assert_eq!(g.children().len(), 2, "one group");
        let bb = g.bounds();
        assert!((bb.x0 - 100.0).abs() < 1e-6 && (bb.y0 - 100.0).abs() < 1e-6, "page top-left at 0,0: {bb:?}");
        assert!(d.stories.values().any(|st| st.text == "From page one"), "stories come along");
        assert!(b.execute("file.place", &json!({"base64": b64, "name": "src.idml", "layoutPage": 2})).is_err(), "page 2 is empty");
    }
}

#[cfg(test)]
mod save_tests {
    use serde_json::json;

    use crate::Session;

    /// Saving over an existing document replaces it whole and leaves nothing beside it.
    #[test]
    fn save_replaces_the_existing_file_without_leftovers() {
        let dir = std::env::temp_dir().join(format!("dc-save-atomic-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("doc.designcraft");
        std::fs::write(&path, vec![b'x'; 1 << 20]).unwrap();
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("frame.create", &json!({"rect": [36, 36, 300, 200], "content": "text", "text": "Saved whole", "caret": false})).unwrap();
        s.execute("file.save", &json!({"path": path.to_string_lossy()})).unwrap();
        s.execute("file.saveACopy", &json!({"path": path.to_string_lossy()})).unwrap();
        let saved = super::from_bytes(&std::fs::read(&path).unwrap()).unwrap();
        assert!(saved.stories.values().any(|st| st.text == "Saved whole"));
        let names: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(names, ["doc.designcraft"], "no temporary file is left");
        // A folder that doesn't exist: an error naming the path.
        let missing = dir.join("missing").join("doc.designcraft");
        let e = s.execute("file.save", &json!({"path": missing.to_string_lossy()})).unwrap_err();
        assert!(e.to_string().contains("missing"), "{e}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A document that can't be written is an error: the file on disk, the unsaved state and the
    /// recovery checkpoint all stay as they were.
    #[test]
    fn a_save_that_cannot_write_the_document_changes_nothing() {
        let dir = std::env::temp_dir().join(format!("dc-save-fails-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("doc.designcraft");
        let recovery = dir.join("recovery");
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("file.save", &json!({"path": path.to_string_lossy()})).unwrap();
        let before = std::fs::read(&path).unwrap();
        s.execute("frame.create", &json!({"rect": [36, 36, 300, 200], "content": "text", "text": "Unsaved", "caret": false})).unwrap();
        s.execute("file.recovery.save", &json!({"dir": recovery.to_string_lossy()})).unwrap();
        let checkpoint = recovery.join(format!("{}.designcraft", s.doc().unwrap().uid));
        assert!(checkpoint.exists());

        super::FAIL_TO_BYTES.with(|f| f.set(true));
        let save = s.execute("file.save", &json!({}));
        let copy = s.execute("file.saveACopy", &json!({"path": dir.join("copy.designcraft").to_string_lossy()}));
        let save_as = s.execute("file.saveAs", &json!({"path": dir.join("other.designcraft").to_string_lossy()}));
        let serialized = s.execute("file.serialize", &json!({}));
        super::FAIL_TO_BYTES.with(|f| f.set(false));

        let e = save.unwrap_err();
        assert!(e.to_string().contains("could not write the document"), "{e}");
        assert!(copy.is_err() && serialized.is_err(), "no empty copy and no empty download");
        assert_eq!(std::fs::read(&path).unwrap(), before, "the saved file is untouched");
        assert!(!dir.join("copy.designcraft").exists());
        assert!(save_as.is_err() && !dir.join("other.designcraft").exists());
        assert!(!s.prefs.recent_files.iter().any(|p| p.ends_with("other.designcraft")), "a failed Save As isn't a recent file");
        assert!(s.doc().unwrap().is_dirty(), "the document still has unsaved changes");
        assert!(checkpoint.exists(), "the recovery checkpoint stays");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// #1050: a placement whose coordinates overflow to infinity is refused, and Save refuses a
    /// document holding a non-finite rect anyway, leaving the file on disk as it was.
    #[test]
    fn non_finite_coordinates_are_refused_and_never_saved() {
        let image = super::image_place_tests::image_file().unwrap();
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let e = s.execute("file.place", &json!({"path": image.path(), "x": 1e308, "width": 1e308})).unwrap_err();
        assert!(e.to_string().contains("finite"), "{e}");
        assert!(s.doc().unwrap().doc.all_items().is_empty(), "nothing is placed");

        let dir = std::env::temp_dir().join(format!("dc-save-non-finite-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("doc.designcraft");
        s.execute("file.save", &json!({"path": path.to_string_lossy()})).unwrap();
        let before = std::fs::read(&path).unwrap();
        let id = designcraft_doc::ItemId(s.execute("frame.create", &json!({"rect": [36, 36, 300, 200]})).unwrap()["id"].as_u64().unwrap());
        let st = s.doc_mut().unwrap();
        let infinite = designcraft_geom::Rect::new(36.0, 36.0, f64::INFINITY, 200.0);
        std::sync::Arc::make_mut(&mut st.doc).item_mut(id).unwrap().path = designcraft_geom::shapes::rectangle(infinite);
        let e = s.execute("file.save", &json!({})).unwrap_err();
        assert!(e.to_string().contains("not a finite number"), "{e}");
        assert_eq!(std::fs::read(&path).unwrap(), before, "the saved file is untouched");
        assert!(s.doc().unwrap().is_dirty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod document_fonts_tests {
    use std::path::{Path, PathBuf};

    use designcraft_fonts::testing::font_with;
    use designcraft_fonts::{DOCUMENT_FONTS_FOLDER, FaceRef, FontDb, FontSource};
    use serde_json::{Value, json};

    use crate::Session;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("dc-open-docfonts-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(DOCUMENT_FONTS_FOLDER)).unwrap();
        dir
    }

    /// A new document in `s` with a text frame holding `text` set in `family`.
    fn new_document(s: &mut Session, family: &str, text: &str) {
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 400, 300], "content": "text", "text": text})).unwrap();
        s.execute("text.select", &json!({"story": r["story"], "anchor": 0, "focus": text.len()})).unwrap();
        s.execute("type.char", &json!({"fontFamily": family, "fontStyle": "Regular"})).unwrap();
    }

    /// A document whose text ("Hello") is set in `family`, written to `path` (`.idml`: as IDML).
    fn write_document(path: &Path, family: &str) {
        write_document_with(path, family, "Hello");
    }

    fn write_document_with(path: &Path, family: &str, text: &str) {
        let mut s = Session::new();
        new_document(&mut s, family, text);
        if path.extension().is_some_and(|e| e == "idml") {
            std::fs::write(path, designcraft_idml::export_idml(&s.doc().unwrap().doc)).unwrap();
        } else {
            s.execute("file.save", &json!({"path": path.to_string_lossy()})).unwrap();
        }
    }

    fn font_entry(s: &mut Session, family: &str) -> Value {
        let l = s.execute("font.list", &json!({})).unwrap();
        l.as_array().unwrap().iter().find(|f| f["family"] == family).cloned().unwrap_or_else(|| panic!("{family} in {l}"))
    }

    /// Each character of the active document's text with the face it is drawn in.
    fn glyph_faces(s: &Session) -> Vec<(char, FaceRef)> {
        let st = s.doc().unwrap();
        let mut out = Vec::new();
        for (sid, story) in &st.doc.stories {
            let cs = s.cache.get(&st.doc, *sid, None);
            for g in cs.frames.iter().flat_map(|ft| &ft.lines).flat_map(|l| &l.glyphs).filter(|g| g.visible && g.len > 0) {
                out.push((story.text[g.byte..].chars().next().unwrap(), g.face));
            }
        }
        out
    }

    fn open(s: &mut Session, path: &Path) -> Value {
        s.execute("file.open", &json!({"path": path.to_string_lossy()})).unwrap()
    }

    #[test]
    fn opening_a_document_loads_its_document_fonts() {
        const FAMILY: &str = "DocFont Open Test";
        let dir = temp_dir("designcraft");
        let fonts = dir.join(DOCUMENT_FONTS_FOLDER);
        std::fs::write(fonts.join("open.ttf"), font_with(FAMILY, &['H', 'e', 'l', 'o']).unwrap()).unwrap();
        std::fs::write(fonts.join("broken.otf"), b"not a font").unwrap();
        let path = dir.join("Brochure.designcraft");
        write_document(&path, FAMILY);
        assert!(!FontDb::global().has_family(FAMILY));

        let mut s = Session::new();
        let r = s.execute("file.open", &json!({"path": path.to_string_lossy()})).unwrap();
        assert_eq!(r["documentFonts"], 1, "{r}");
        assert!(r["warnings"].to_string().contains("broken.otf"), "a damaged font is reported, not fatal: {r}");
        let f = font_entry(&mut s, FAMILY);
        assert_eq!((f["missing"].clone(), f["source"].clone()), (json!(false), json!("document")), "{f}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn opening_an_idml_file_loads_its_document_fonts() {
        const FAMILY: &str = "DocFont IDML Test";
        let dir = temp_dir("idml");
        std::fs::write(dir.join(DOCUMENT_FONTS_FOLDER).join("idml.ttf"), font_with(FAMILY, &['H']).unwrap()).unwrap();
        let path = dir.join("Brochure.idml");
        write_document(&path, FAMILY);

        let mut s = Session::new();
        let r = s.execute("file.open", &json!({"path": path.to_string_lossy()})).unwrap();
        assert_eq!(r["documentFonts"], 1, "{r}");
        assert_eq!(font_entry(&mut s, FAMILY)["missing"], false);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_indesign_file_says_how_to_bring_it_over() {
        // The extension decides (any case), not the bytes: no InDesign-made file is needed.
        let dir = temp_dir("indd");
        let path = dir.join("Brochure.INDD");
        std::fs::write(&path, b"not a DesignCraft document").unwrap();
        let mut s = Session::new();
        let e = s.execute("file.open", &json!({"path": path.to_string_lossy()})).unwrap_err().to_string();
        assert!(e.contains("InDesign Markup (IDML)") && !e.contains("expected value"), "{e}");
        let e = s.execute("file.openBytes", &json!({"name": "Brochure.indt", "base64": ""})).unwrap_err().to_string();
        assert!(e.contains("InDesign Markup (IDML)"), "{e}");
        assert!(s.doc().is_err(), "nothing was opened");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_document_fonts_folder_is_found_whatever_its_case() {
        const FAMILY: &str = "DocFont Case Test";
        let dir = std::env::temp_dir().join(format!("dc-open-docfonts-case-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let fonts = dir.join("Document fonts");
        std::fs::create_dir_all(&fonts).unwrap();
        std::fs::write(fonts.join("case.ttf"), font_with(FAMILY, &['H']).unwrap()).unwrap();
        let path = dir.join("Brochure.idml");
        write_document(&path, FAMILY);

        let mut s = Session::new();
        let r = open(&mut s, &path);
        assert_eq!(r["documentFonts"], 1, "{r}");
        assert_eq!(font_entry(&mut s, FAMILY)["missing"], false);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_document_without_the_folder_opens_as_before() {
        let dir = std::env::temp_dir().join(format!("dc-open-docfonts-none-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("Plain.designcraft");
        write_document(&path, designcraft_fonts::DEFAULT_FAMILY);
        let r = Session::new().execute("file.open", &json!({"path": path.to_string_lossy()})).unwrap();
        assert_eq!((r["documentFonts"].clone(), r["warnings"].clone()), (json!(0), json!([])), "{r}");
        let _ = std::fs::remove_dir_all(&dir);
    }
    #[test]
    fn document_fonts_belong_to_their_document() {
        const FAMILY: &str = "DocFont Scoped";
        let (a, b) = (temp_dir("scoped-a"), temp_dir("scoped-b"));
        std::fs::write(a.join(DOCUMENT_FONTS_FOLDER).join("scoped.ttf"), font_with(FAMILY, &['H', 'e', 'l', 'o']).unwrap()).unwrap();
        write_document(&a.join("A.designcraft"), FAMILY);
        write_document(&b.join("B.designcraft"), FAMILY);

        let mut s = Session::new();
        assert_eq!(open(&mut s, &a.join("A.designcraft"))["documentFonts"], 1);
        assert_eq!(open(&mut s, &b.join("B.designcraft"))["documentFonts"], 0);
        // B, open at the same time, doesn't have A's font.
        assert_eq!(font_entry(&mut s, FAMILY)["missing"], true);
        assert!(glyph_faces(&s).iter().all(|(_, f)| f.family != FAMILY), "{:?}", glyph_faces(&s));
        s.execute("file.activate", &json!({"index": 0})).unwrap();
        assert_eq!(font_entry(&mut s, FAMILY)["source"], "document");
        assert!(glyph_faces(&s).iter().all(|(_, f)| f.family == FAMILY), "{:?}", glyph_faces(&s));
        let _ = std::fs::remove_dir_all(&a);
        let _ = std::fs::remove_dir_all(&b);
    }

    #[test]
    fn each_document_sets_its_own_font_of_a_name() {
        const FAMILY: &str = "DocFont Same";
        let (a, b) = (temp_dir("same-a"), temp_dir("same-b"));
        let (a_font, b_font) = (a.join(DOCUMENT_FONTS_FOLDER).join("same.ttf"), b.join(DOCUMENT_FONTS_FOLDER).join("same.ttf"));
        // A's font has both letters, B's only `b`.
        std::fs::write(&a_font, font_with(FAMILY, &['a', 'b']).unwrap()).unwrap();
        std::fs::write(&b_font, font_with(FAMILY, &['b']).unwrap()).unwrap();
        // The same words in both: the word cache must not hand one document's shaping to the other.
        write_document_with(&a.join("A.designcraft"), FAMILY, "ab ab ab");
        write_document_with(&b.join("B.designcraft"), FAMILY, "ab ab ab");

        let mut s = Session::new();
        open(&mut s, &a.join("A.designcraft"));
        open(&mut s, &b.join("B.designcraft"));
        // Fallback fonts for the `a` B's font lacks: never A's font.
        s.execute("document.preferences", &json!({"glyphFallback": true})).unwrap();
        let in_b = glyph_faces(&s);
        s.execute("file.activate", &json!({"index": 0})).unwrap();
        let in_a = glyph_faces(&s);
        assert_eq!((in_a.len(), in_b.len()), (8, 8), "{in_a:?} {in_b:?}");
        for (c, f) in &in_a {
            assert_eq!(f.source, FontSource::Document(a_font.clone()), "`{c}` in A");
        }
        for (c, f) in &in_b {
            match c {
                'a' => assert!(f.family != FAMILY, "B's font lacks `a`, and A's isn't B's: {f:?}"),
                _ => assert_eq!(f.source, FontSource::Document(b_font.clone()), "`{c}` in B"),
            }
        }
        let _ = std::fs::remove_dir_all(&a);
        let _ = std::fs::remove_dir_all(&b);
    }

    #[test]
    fn closing_a_document_forgets_its_fonts_and_reopening_reuses_them() {
        const FAMILY: &str = "DocFont Closed";
        let dir = temp_dir("closed");
        std::fs::write(dir.join(DOCUMENT_FONTS_FOLDER).join("closed.ttf"), font_with(FAMILY, &['H', 'e', 'l', 'o']).unwrap()).unwrap();
        let path = dir.join("Closed.designcraft");
        write_document(&path, FAMILY);

        let mut s = Session::new();
        open(&mut s, &path);
        let first = glyph_faces(&s)[0].1;
        assert_eq!(first.family, FAMILY);
        s.execute("file.close", &json!({})).unwrap();
        // A new document can't use the closed document's font.
        new_document(&mut s, FAMILY, "Hello");
        assert_eq!(font_entry(&mut s, FAMILY)["missing"], true);
        assert!(glyph_faces(&s).iter().all(|(_, f)| f.family != FAMILY));
        // Reopened, the document has its font again: the face loaded the first time.
        assert_eq!(open(&mut s, &path)["documentFonts"], 1);
        assert!(glyph_faces(&s).iter().all(|(_, f)| *f == first), "{:?}", glyph_faces(&s));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_reverted_document_keeps_its_fonts() {
        const FAMILY: &str = "DocFont Reverted";
        let dir = temp_dir("revert");
        std::fs::write(dir.join(DOCUMENT_FONTS_FOLDER).join("revert.ttf"), font_with(FAMILY, &['H', 'e', 'l', 'o']).unwrap()).unwrap();
        let path = dir.join("Revert.designcraft");
        write_document(&path, FAMILY);

        let mut s = Session::new();
        open(&mut s, &path);
        s.execute("frame.create", &json!({"rect": [72, 400, 200, 500], "content": "text", "text": "x"})).unwrap();
        s.execute("file.revert", &json!({})).unwrap();
        assert_eq!(font_entry(&mut s, FAMILY)["source"], "document");
        assert!(glyph_faces(&s).iter().all(|(_, f)| f.family == FAMILY));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn bold_and_italic_link_the_document_fonts_styles() {
        const FAMILY: &str = "DocFont Linked";
        let dir = temp_dir("linked");
        std::fs::write(dir.join(DOCUMENT_FONTS_FOLDER).join("linked.ttf"), font_with(FAMILY, &['H', 'e', 'l', 'o']).unwrap()).unwrap();
        let path = dir.join("Linked.designcraft");
        write_document(&path, FAMILY);

        let mut s = Session::new();
        open(&mut s, &path);
        let sid = *s.doc().unwrap().doc.stories.keys().next().unwrap();
        s.execute("text.select", &json!({"story": sid.0, "anchor": 0, "focus": 0})).unwrap();
        // The family is the document's alone: its Regular is the style without bold or italic.
        let r = s.execute("type.bold", &json!({"on": false})).unwrap();
        assert_eq!(r["fontStyle"], "Regular", "{r}");
        let r = s.execute("type.italic", &json!({"on": false})).unwrap();
        assert_eq!(r["fontStyle"], "Regular", "{r}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod extension_tests {
    use serde_json::json;

    use super::{is_document, with_document_extension};
    use crate::Session;

    #[test]
    fn the_document_extension_is_added_when_missing_or_different() {
        assert_eq!(with_document_extension("/a/Brochure"), "/a/Brochure.designcraft");
        assert_eq!(with_document_extension("/a/Brochure.idml"), "/a/Brochure.idml.designcraft");
        assert_eq!(with_document_extension("/a/v1.2 Brochure"), "/a/v1.2 Brochure.designcraft");
        assert_eq!(with_document_extension("/a/Brochure.DesignCraft"), "/a/Brochure.DesignCraft");
        assert_eq!(with_document_extension("/a/.."), "/a/..");
    }

    #[test]
    fn save_adds_the_extension_and_open_recognises_files_by_content() {
        let dir = std::env::temp_dir().join(format!("dc-file-extension-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();

        let bare = dir.join("Brochure");
        let written = dir.join("Brochure.designcraft");
        let r = s.execute("file.saveAs", &json!({"path": bare.to_string_lossy()})).unwrap();
        assert_eq!(r["path"], json!(written.to_string_lossy()), "{r}");
        assert!(written.is_file() && !bare.exists());
        assert_eq!(s.doc().unwrap().path.as_deref(), written.to_str());
        let r = s.execute("file.saveACopy", &json!({"path": dir.join("Copy").to_string_lossy()})).unwrap();
        assert_eq!(r["path"], json!(dir.join("Copy.designcraft").to_string_lossy()), "{r}");

        // A native file without the extension opens, and Save writes back to that same file.
        std::fs::rename(&written, &bare).unwrap();
        let mut t = Session::new();
        t.execute("file.open", &json!({"path": bare.to_string_lossy()})).unwrap();
        let r = t.execute("file.save", &json!({})).unwrap();
        assert_eq!(r["path"], json!(bare.to_string_lossy()), "{r}");
        assert!(!written.exists());

        // An IDML package without the extension opens as IDML, unsaved.
        let idml = dir.join("Package");
        std::fs::write(&idml, designcraft_idml::export_idml(&s.doc().unwrap().doc)).unwrap();
        assert!(is_document(&std::fs::read(&idml).unwrap()) && is_document(&std::fs::read(&bare).unwrap()));
        // The start of the file is enough.
        for path in [&idml, &bare] {
            let bytes = std::fs::read(path).unwrap();
            assert!(is_document(&bytes[..bytes.len().min(128)]), "{path:?}");
        }
        t.execute("file.open", &json!({"path": idml.to_string_lossy()})).unwrap();
        assert_eq!(t.doc().unwrap().path, None);

        let notes = dir.join("Notes");
        std::fs::write(&notes, b"plain text").unwrap();
        assert!(!is_document(b"plain text"));
        let e = t.execute("file.open", &json!({"path": notes.to_string_lossy()})).unwrap_err();
        assert!(e.to_string().contains("not a DesignCraft or IDML file"), "{e}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod recent_tests {
    use serde_json::json;

    use crate::Session;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("dc-recent-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn saved(s: &mut Session, path: &std::path::Path) -> String {
        s.execute("file.new", &json!({})).unwrap();
        s.execute("file.saveAs", &json!({"path": path.to_string_lossy()})).unwrap();
        path.to_string_lossy().to_string()
    }

    /// Opening and saving put a file at the front once; Save a Copy doesn't count.
    #[test]
    fn open_and_save_order_the_list_without_duplicates() {
        let dir = temp_dir("order");
        let mut s = Session::new();
        let a = saved(&mut s, &dir.join("a.designcraft"));
        let b = saved(&mut s, &dir.join("b.designcraft"));
        assert_eq!(s.prefs.recent_files, [b.clone(), a.clone()]);
        s.execute("file.open", &json!({"path": a})).unwrap();
        assert_eq!(s.prefs.recent_files, [a.clone(), b.clone()], "reopening moves it to the front, once");
        s.execute("file.saveACopy", &json!({"path": dir.join("copy.designcraft").to_string_lossy()})).unwrap();
        assert_eq!(s.prefs.recent_files, [a.clone(), b.clone()], "a copy isn't a recent file");
        let rows = s.execute("file.recentFiles", &json!({})).unwrap();
        assert_eq!(rows[0], json!({"path": a, "name": "a.designcraft", "exists": true}));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn open_recent_reopens_and_a_missing_file_is_an_error_that_keeps_it_listed() {
        let dir = temp_dir("open");
        let mut s = Session::new();
        let a = saved(&mut s, &dir.join("a.designcraft"));
        let b = saved(&mut s, &dir.join("b.designcraft"));
        let r = s.execute("file.openRecent", &json!({"index": 1})).unwrap();
        assert_eq!(s.documents()[r["index"].as_u64().unwrap() as usize].path.as_deref(), Some(a.as_str()));
        assert_eq!(s.prefs.recent_files, [a.clone(), b.clone()]);
        std::fs::remove_file(&b).unwrap();
        let e = s.execute("file.openRecent", &json!({"index": 1})).unwrap_err();
        assert!(e.to_string().contains("can't be found"), "{e}");
        assert_eq!(s.prefs.recent_files, [a.clone(), b.clone()], "a disk that isn't connected comes back");
        assert_eq!(s.execute("file.recentFiles", &json!({})).unwrap()[1]["exists"], false);
        // Hostile indices are errors, never panics.
        for index in [json!(2), json!(u64::MAX), json!(-1), json!("0"), json!(null)] {
            assert!(s.execute("file.openRecent", &json!({"index": index})).is_err(), "{index}");
        }
        s.execute("file.clearRecent", &json!({})).unwrap();
        assert!(s.prefs.recent_files.is_empty());
        assert!(s.execute("file.openRecent", &json!({"index": 0})).is_err(), "disabled when empty");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Saving puts a small picture of the first page in the file, which the start screen reads
    /// without opening the document.
    #[test]
    fn saved_files_carry_a_first_page_preview() {
        let dir = temp_dir("preview");
        let mut s = Session::new();
        let a = saved(&mut s, &dir.join("a.designcraft"));
        let p = super::recent_preview(&a).expect("a preview");
        assert_eq!(p.width.max(p.height), super::PREVIEW_PX as u32);
        assert_eq!(p.rgba.len(), (p.width * p.height * 4) as usize);
        assert!(p.rgba.as_chunks::<4>().0.iter().any(|px| px[3] == 255), "the page is drawn, not left transparent");
        let copy = dir.join("copy.designcraft");
        s.execute("file.saveACopy", &json!({"path": copy.to_string_lossy()})).unwrap();
        assert!(super::recent_preview(&copy.to_string_lossy()).is_some(), "Save a Copy has one too");
        // Files without one (saved before previews), missing files and other files have none.
        let old = dir.join("old.designcraft");
        std::fs::write(&old, super::to_bytes(&s.doc().unwrap().doc).unwrap()).unwrap();
        for path in [old, dir.join("gone.designcraft"), dir.clone()] {
            assert!(super::recent_preview(&path.to_string_lossy()).is_none(), "{path:?}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_list_keeps_at_most_the_preferred_count() {
        let mut s = Session::new();
        for i in 0..30 {
            s.prefs.note_recent_file(&format!("/work/{i}.designcraft"));
        }
        assert_eq!(s.prefs.recent_files.len(), 20, "default: 20");
        assert_eq!(s.prefs.recent_files[0], "/work/29.designcraft");
        s.execute("prefs.set", &json!({"recentFilesMax": 3})).unwrap();
        assert_eq!(s.prefs.recent_files, ["/work/29.designcraft", "/work/28.designcraft", "/work/27.designcraft"]);
        assert_eq!(s.execute("prefs.set", &json!({"recentFilesMax": 4_000_000_000u64})).unwrap()["recentFilesMax"], crate::MAX_RECENT_FILES);
        assert!(s.execute("prefs.set", &json!({"recentFilesMax": -1})).is_err());
        s.execute("prefs.set", &json!({"recentFilesMax": 0})).unwrap();
        s.prefs.note_recent_file("/work/new.designcraft");
        assert!(s.prefs.recent_files.is_empty(), "0 keeps no list");
    }

    /// The list is a preference: it survives the round trip through prefs.json.
    #[test]
    fn the_list_survives_a_restart() {
        let mut s = Session::new();
        s.prefs.note_recent_file("/work/a.designcraft");
        let saved = serde_json::to_vec(&s.prefs).unwrap();
        let back: crate::Prefs = serde_json::from_slice(&saved).unwrap();
        assert_eq!(back.recent_files, ["/work/a.designcraft"]);
        assert_eq!(back.recent_files_max, 20);
        let old: crate::Prefs = serde_json::from_slice(br#"{"recoveryMinutes": 2.0}"#).unwrap();
        assert!(old.recent_files.is_empty(), "older prefs.json without the list still load");
    }
}

#[cfg(test)]
mod hyphenation_default_tests {
    use std::sync::Arc;

    use designcraft_doc::BASIC_PARAGRAPH;
    use designcraft_doc::build::NewDocument;
    use serde_json::json;

    use crate::Session;

    fn hyph_flags(d: &designcraft_doc::Document) -> (bool, bool, bool) {
        let (pp, _) = d.styles.resolve_para(&designcraft_doc::ParaFormat::default());
        (pp.hyph_capitalized, pp.hyph_last_word, pp.hyph_across_column)
    }

    #[test]
    fn new_documents_hyphenate_no_capitalized_last_or_column_end_words_and_imports_keep_theirs() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        assert_eq!(hyph_flags(&s.doc().unwrap().doc), (false, false, false));

        let mut d = designcraft_doc::Document::new(&NewDocument::default());
        let st = Arc::make_mut(&mut d.styles).para_mut(BASIC_PARAGRAPH).unwrap();
        st.para.hyph_capitalized = Some(true);
        st.para.hyph_last_word = Some(true);
        st.para.hyph_across_column = Some(true);
        let b64 = super::base64_encode(&designcraft_idml::export_idml(&d));
        s.execute("file.openIdml", &json!({"base64": b64, "name": "in.idml"})).unwrap();
        assert_eq!(hyph_flags(&s.doc().unwrap().doc), (true, true, true));
    }
}
