#[cfg(test)]
use super::bytes::{PREVIEW_PX, to_bytes};
use super::bytes::{base64_decode, load_bytes};
use super::{Arc, DocState, Document, EngineError, Result, Session, Value, bad, json, str_param};

pub(super) fn has_recent(s: &Session) -> std::result::Result<(), String> {
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
pub(super) fn recent_entry(path: &str) -> Value {
    let name = std::path::Path::new(path).file_name().map_or_else(|| path.to_string(), |n| n.to_string_lossy().to_string());
    #[cfg(not(target_arch = "wasm32"))]
    let exists = crate::io::is_file(path);
    #[cfg(target_arch = "wasm32")]
    let exists = false;
    json!({"path": path, "name": name, "exists": exists})
}

pub(super) fn file_open_recent(s: &mut Session, p: &Value) -> Result<Value> {
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
pub(super) fn file_open(s: &mut Session, p: &Value) -> Result<Value> {
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
        if designcraft_idml::is_idml(&bytes) || (!native && (crate::cmd::interchange::is_indd(path, &bytes) || ext == "idml")) {
            return crate::cmd::interchange::open_idml(s, p);
        }
        if !native && ext != designcraft_format::EXTENSION {
            return Err(bad("file.open", format!("{path}: not a DesignCraft or IDML file")));
        }
        let (mut d, mut warnings) = load_bytes(&bytes)?;
        crate::cmd::interchange::resolve_packaged_links(&mut d, Some(std::path::Path::new(path)));
        crate::cmd::datamerge::resolve_sources_on_open(&mut d, Some(std::path::Path::new(path)));
        let (fonts, faces, font_warnings) = load_document_fonts(&mut d, path);
        warnings.extend(font_warnings);
        warnings.extend(crate::cmd::style::grep_style_warnings(&d));
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

pub(super) fn file_open_bytes(s: &mut Session, p: &Value) -> Result<Value> {
    let b = base64_decode(str_param(p, "base64").unwrap_or(""));
    // IDML packages are recognised by their stored `mimetype` first entry, INDD files by their
    // signature (or their name).
    if designcraft_idml::is_idml(&b) || crate::cmd::interchange::is_indd(str_param(p, "name").unwrap_or_default(), &b) {
        return crate::cmd::interchange::open_idml(s, p);
    }
    let (mut d, warnings) = load_bytes(&b)?;
    crate::cmd::datamerge::resolve_sources_on_open(&mut d, None);
    if let Some(n) = str_param(p, "name") {
        d.title = n.to_string();
    }
    let i = s.add_document(DocState::new(d, None));
    Ok(json!({"index": i, "warnings": warnings}))
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
