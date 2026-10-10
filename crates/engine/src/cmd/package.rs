//! File › Package and Links › Copy Links To: gather a document with the files it uses.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use designcraft_compose::ComposedStory;
use designcraft_doc::{AssetId, Document};
use designcraft_fonts::{FaceRef, FontFace, FontSource};
use serde_json::{Value, json};

use super::{CommandSpec, bad, cmd, has_doc, str_param};
use crate::{EngineError, Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            noundo "file.package",
            "Package…",
            ["File"],
            None,
            "{dir: folder to create, idml?: true, pdf?: false, instructions?: text} — the document (its placed files relinked to Links/), Links/, the font files that draw its text in Document Fonts/ (fallback fonts included; unless their licence restricts it), an IDML copy, an optional PDF and a report → {dir, files, report}",
            has_doc,
            package
        ),
        cmd!(
            "links.copyTo",
            "Copy Link(s) To…",
            [],
            None,
            "{dir, assets?: [asset ids] (default: all)} — write the placed files to `dir` and relink to the copies → {copied}",
            has_doc,
            copy_links
        ),
    ]
}

/// A file name for asset `a` in a Links folder, unique among `taken`.
fn link_name(name: &str, id: AssetId, taken: &mut Vec<String>) -> String {
    let base =
        Path::new(name).file_name().map(|n| n.to_string_lossy().to_string()).filter(|n| !n.is_empty()).unwrap_or_else(|| format!("asset-{}", id.0));
    unique_name(base, taken)
}

/// `base`, or `base` numbered ("logo 2.png"), unique among `taken` (case-insensitively).
fn unique_name(base: String, taken: &mut Vec<String>) -> String {
    let (stem, ext) = match base.rsplit_once('.') {
        Some((s, e)) => (s.to_string(), format!(".{e}")),
        None => (base.clone(), String::new()),
    };
    let mut n = base;
    let mut k = 2;
    while taken.iter().any(|t| t.eq_ignore_ascii_case(&n)) {
        n = format!("{stem} {k}{ext}");
        k += 1;
    }
    taken.push(n.clone());
    n
}

/// Write each asset's bytes into `dir`; returns the relinked document and the paths written.
fn write_links(d: &Document, dir: &Path, only: Option<&[AssetId]>) -> Result<(Document, Vec<PathBuf>)> {
    std::fs::create_dir_all(dir).map_err(|e| EngineError::Other(format!("{}: {e}", dir.display())))?;
    let mut out = d.clone();
    let mut taken = Vec::new();
    let mut written = Vec::new();
    let mut ids: Vec<AssetId> = d.assets.keys().copied().collect();
    ids.sort_by_key(|i| i.0);
    for id in ids {
        if only.is_some_and(|o| !o.contains(&id)) {
            continue;
        }
        let a = &d.assets[&id];
        let name = link_name(a.link.as_deref().unwrap_or(&a.name), id, &mut taken);
        let path = dir.join(&name);
        std::fs::write(&path, a.data.as_slice()).map_err(|e| EngineError::Other(format!("{}: {e}", path.display())))?;
        let mut na = (**a).clone();
        na.link = Some(path.to_string_lossy().to_string());
        out.assets.insert(id, Arc::new(na));
        written.push(path);
    }
    Ok((out, written))
}

/// Copy the font file of `face` into `dir` (created when first needed) unless its licence
/// restricts it or it is already there (`copied`: the source files copied) → what the package
/// report adds after the font's name (nothing when copied).
fn copy_font(face: &FontFace, dir: &Path, copied: &mut Vec<PathBuf>, taken: &mut Vec<String>, files: &mut Vec<PathBuf>) -> String {
    let from = match &face.source {
        FontSource::Bundled => return " — included with DesignCraft".into(),
        FontSource::Memory => return " — not copied: no font file".into(),
        FontSource::Installed(p) | FontSource::Document(p) => p,
    };
    if face.restricted_licence() {
        return " — not copied: its licence doesn't allow it".into();
    }
    if copied.contains(from) {
        return String::new();
    }
    let name = from.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "font".into());
    let to = dir.join(unique_name(name, taken));
    // Packaging into the folder the font is in: it is there already (and copying a file onto
    // itself empties it on some systems).
    let same = std::fs::canonicalize(&to).is_ok_and(|t| std::fs::canonicalize(from).is_ok_and(|f| f == t));
    match std::fs::create_dir_all(dir).and_then(|_| if same { Ok(0) } else { std::fs::copy(from, &to) }) {
        Ok(_) => {
            copied.push(from.clone());
            files.push(to);
            String::new()
        }
        Err(e) => format!(" — not copied: {e}"),
    }
}

/// Add the faces that draw the visible glyphs of `cs` (table cells and footnotes included) to
/// `out`, each once.
fn drawn_faces(cs: &ComposedStory, depth: usize, out: &mut Vec<FaceRef>) {
    // Cells and footnotes hold composed stories of their own; tables nest only so deep.
    if depth > 16 {
        return;
    }
    for f in &cs.frames {
        for g in f.lines.iter().flat_map(|l| &l.glyphs).filter(|g| g.visible) {
            if !out.iter().any(|o| o.id() == g.face.id()) {
                out.push(g.face);
            }
        }
        for c in f.tables.iter().flat_map(|t| &t.cells) {
            drawn_faces(&c.text, depth + 1, out);
        }
        for n in &f.notes {
            drawn_faces(&n.text, depth + 1, out);
        }
    }
}

fn package(s: &mut Session, p: &Value) -> Result<Value> {
    let dir = PathBuf::from(str_param(p, "dir").ok_or_else(|| bad("file.package", "missing `dir`"))?);
    let st = s.doc()?;
    let title = st.doc.title.clone();
    let fonts = s.execute("font.list", &json!({}))?;
    let links = s.execute("links.list", &json!({}))?;
    let pre = s.execute("preflight.run", &json!({})).unwrap_or(Value::Null);
    let d = s.doc()?.doc.clone();
    let (packed, mut files) = write_links(&d, &dir.join("Links"), None)?;
    let doc_path = dir.join(format!("{title}.designcraft"));
    std::fs::write(&doc_path, super::to_bytes(&packed)).map_err(|e| EngineError::Other(format!("{}: {e}", doc_path.display())))?;
    files.insert(0, doc_path);
    // The font files, and what the report says about each font.
    let db = designcraft_fonts::FontDb::global().scoped(d.font_scope);
    let (fonts_dir, mut copied, mut taken) = (dir.join(designcraft_fonts::DOCUMENT_FONTS_FOLDER), Vec::new(), Vec::new());
    let (mut font_lines, mut named) = (Vec::new(), Vec::new());
    for f in fonts.as_array().into_iter().flatten() {
        let (family, style) = (f["family"].as_str().unwrap_or(""), f["style"].as_str().unwrap_or(""));
        let missing = f["missing"].as_bool().unwrap_or(false) || f["styleMissing"].as_bool().unwrap_or(false);
        // A missing font's text is drawn in its substitute, which isn't a fallback font.
        let face = db.face(family, style);
        named.push(face.id());
        let note = if missing { " — MISSING".to_string() } else { copy_font(&face, &fonts_dir, &mut copied, &mut taken, &mut files) };
        font_lines.push(format!("  {family} {style}{note}\n"));
    }
    // Fallback fonts: the faces that draw characters the named fonts lack.
    let mut drawn = Vec::new();
    for sid in d.stories.keys() {
        drawn_faces(&s.cache.get(&d, *sid, None), 0, &mut drawn);
    }
    drawn.retain(|f| !named.contains(&f.id()));
    drawn.sort_by(|a, b| (&a.family, &a.style).cmp(&(&b.family, &b.style)));
    let fallback_lines: Vec<String> =
        drawn.iter().map(|f| format!("  {} {}{}\n", f.family, f.style, copy_font(f, &fonts_dir, &mut copied, &mut taken, &mut files))).collect();
    if p.get("idml").and_then(Value::as_bool).unwrap_or(true) {
        let path = dir.join(format!("{title}.idml"));
        std::fs::write(&path, designcraft_idml::export_idml(&packed)).map_err(|e| EngineError::Other(format!("{}: {e}", path.display())))?;
        files.push(path);
    }
    if p.get("pdf").and_then(Value::as_bool).unwrap_or(false) {
        let path = dir.join(format!("{title}.pdf"));
        let bytes = designcraft_pdf::export_pdf(&packed, &s.cache, &Default::default()).map_err(|e| EngineError::Other(e.to_string()))?;
        std::fs::write(&path, bytes).map_err(|e| EngineError::Other(format!("{}: {e}", path.display())))?;
        files.push(path);
    }
    // The report: fonts (missing first), links, preflight, the user's instructions.
    let mut r = format!("Package report: {title}\n\n");
    if let Some(t) = str_param(p, "instructions").filter(|t| !t.trim().is_empty()) {
        r += &format!("Instructions\n{t}\n\n");
    }
    r += "Fonts\n";
    r += &font_lines.concat();
    if !fallback_lines.is_empty() {
        r += "\nFallback fonts (for characters the fonts above lack)\n";
        r += &fallback_lines.concat();
    }
    r += "\nLinks\n";
    for l in links.as_array().into_iter().flatten() {
        r += &format!("  {} ({})\n", l["name"].as_str().unwrap_or(""), l["status"].as_str().unwrap_or(""));
    }
    if let Some(issues) = pre.get("issues").and_then(Value::as_array) {
        r += &format!("\nPreflight: {} issue(s)\n", issues.len());
        for i in issues {
            r += &format!("  {}\n", i.get("message").and_then(Value::as_str).unwrap_or(""));
        }
    }
    let report = dir.join("Instructions.txt");
    std::fs::write(&report, &r).map_err(|e| EngineError::Other(format!("{}: {e}", report.display())))?;
    files.push(report);
    Ok(json!({"dir": dir.to_string_lossy(), "files": files.iter().map(|f| f.to_string_lossy().to_string()).collect::<Vec<_>>(), "report": r}))
}

fn copy_links(s: &mut Session, p: &Value) -> Result<Value> {
    let dir = PathBuf::from(str_param(p, "dir").ok_or_else(|| bad("links.copyTo", "missing `dir`"))?);
    let only: Option<Vec<AssetId>> = p.get("assets").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_u64).map(AssetId).collect());
    let d = s.doc()?.doc.clone();
    let (relinked, written) = write_links(&d, &dir, only.as_deref())?;
    s.edit(|doc, _| {
        doc.assets = relinked.assets;
        Ok(())
    })?;
    Ok(json!({"copied": written.len()}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relocated_packages_rebind_embedded_image_links_on_open() {
        let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("dc-package-relocation-{}-{nonce}", std::process::id()));
        let original = dir.join("original");
        let moved = dir.join("moved");
        let png = designcraft_render::Rendered { width: 4, height: 3, pixels: [30, 90, 180, 255].repeat(12) }.to_png();
        let mut s = Session::new();
        s.execute("file.new", &json!({"title": "OwnedPackage", "width": 72, "height": 72})).unwrap();
        s.execute("file.place", &json!({"base64": super::super::base64_encode(&png), "name": "owned.png", "x": 5, "y": 5, "width": 16})).unwrap();
        s.execute("file.package", &json!({"dir": original.to_string_lossy(), "idml": true, "pdf": false})).unwrap();
        std::fs::rename(&original, &moved).unwrap();
        let expected = std::path::absolute(moved.join("Links").join("owned.png")).unwrap();
        for (command, extension) in [("file.open", "designcraft"), ("file.openIdml", "idml")] {
            let mut opened = Session::new();
            opened.execute(command, &json!({"path": moved.join(format!("OwnedPackage.{extension}")).to_string_lossy()})).unwrap();
            let asset = opened.doc().unwrap().doc.assets.values().next().unwrap();
            assert_eq!(*asset.data, png, "{extension}: preserve embedded pixels");
            assert_eq!(asset.link.as_deref(), expected.to_str(), "{extension}: use the relocated Links file");
            let links = opened.execute("links.list", &json!({})).unwrap();
            assert_eq!(links[0]["status"], "ok", "{extension}");
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn package_writes_document_links_and_report() {
        let dir = std::env::temp_dir().join(format!("dc-package-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let svg = dir.join("logo.svg");
        std::fs::write(&svg, r##"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="40"><rect width="40" height="40"/></svg>"##).unwrap();
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("file.place", &json!({"path": svg.to_string_lossy(), "x": 100, "y": 100})).unwrap();
        s.execute("frame.create", &json!({"rect": [72, 300, 400, 400], "content": "text", "text": "Hello"})).unwrap();
        let out = dir.join("Pkg");
        let r = s.execute("file.package", &json!({"dir": out.to_string_lossy(), "pdf": true, "instructions": "Print on matte."})).unwrap();
        let files: Vec<String> = r["files"].as_array().unwrap().iter().map(|f| f.as_str().unwrap().to_string()).collect();
        assert!(files.iter().any(|f| f.ends_with(".designcraft")));
        assert!(files.iter().any(|f| f.ends_with(".idml")));
        assert!(files.iter().any(|f| f.ends_with(".pdf")));
        assert!(out.join("Links").join("logo.svg").exists());
        let report = std::fs::read_to_string(out.join("Instructions.txt")).unwrap();
        assert!(report.contains("Print on matte.") && report.contains("logo.svg"), "{report}");
        assert!(report.contains("Preflight:"), "{report}");
        // The packaged document links to its Links folder.
        let doc_file = files.iter().find(|f| f.ends_with(".designcraft")).unwrap();
        let packed = super::super::from_bytes(&std::fs::read(doc_file).unwrap()).unwrap();
        let link = packed.assets.values().next().unwrap().link.clone().unwrap();
        assert!(link.contains("Links"), "{link}");
        // Copy Links To relinks the open document.
        let r = s.execute("links.copyTo", &json!({"dir": dir.join("Copies").to_string_lossy()})).unwrap();
        assert_eq!(r["copied"], 1);
        assert!(s.doc().unwrap().doc.assets.values().next().unwrap().link.as_deref().unwrap().contains("Copies"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn package_copies_the_fonts_their_licences_allow() {
        use designcraft_fonts::DOCUMENT_FONTS_FOLDER;
        use designcraft_fonts::testing::{font_with, with_fs_type};
        const FREE: &str = "DocFont Package Free";
        const RESTRICTED: &str = "DocFont Package Restricted";
        let dir = std::env::temp_dir().join(format!("dc-package-fonts-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let fonts = dir.join("src").join(DOCUMENT_FONTS_FOLDER);
        std::fs::create_dir_all(&fonts).unwrap();
        std::fs::write(fonts.join("free.ttf"), font_with(FREE, &['F']).unwrap()).unwrap();
        std::fs::write(fonts.join("restricted.otf"), with_fs_type(font_with(RESTRICTED, &['R']).unwrap(), 0x0002).unwrap()).unwrap();
        // Text in both fonts, and in the bundled default font.
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 400, 300], "content": "text", "text": "Free Restricted Default"})).unwrap();
        for (a, b, family) in [(0, 4, FREE), (5, 15, RESTRICTED)] {
            s.execute("text.select", &json!({"story": r["story"], "anchor": a, "focus": b})).unwrap();
            s.execute("type.char", &json!({"fontFamily": family, "fontStyle": "Regular"})).unwrap();
        }
        let src = dir.join("src").join("Fonts.designcraft");
        s.execute("file.save", &json!({"path": src.to_string_lossy()})).unwrap();
        let mut s = Session::new();
        assert_eq!(s.execute("file.open", &json!({"path": src.to_string_lossy()})).unwrap()["documentFonts"], 2);

        let out = dir.join("Pkg");
        let r = s.execute("file.package", &json!({"dir": out.to_string_lossy(), "idml": false})).unwrap();
        let packed_fonts = out.join(DOCUMENT_FONTS_FOLDER);
        assert!(packed_fonts.join("free.ttf").exists());
        assert!(!packed_fonts.join("restricted.otf").exists(), "a restricted licence keeps the font out");
        assert!(r["files"].to_string().contains("free.ttf"), "{}", r["files"]);
        let report = r["report"].as_str().unwrap();
        assert!(report.contains(&format!("{RESTRICTED} Regular — not copied: its licence doesn't allow it")), "{report}");
        assert!(report.contains(&format!("{FREE} Regular\n")), "{report}");
        assert!(report.contains(&format!("{} Regular — included with DesignCraft", designcraft_fonts::DEFAULT_FAMILY)), "{report}");
        // The packaged document opens with its fonts.
        let doc = r["files"][0].as_str().unwrap().to_string();
        assert_eq!(Session::new().execute("file.open", &json!({"path": doc})).unwrap()["documentFonts"], 1);
        // Packaging into the folder the fonts come from leaves them whole.
        let len = std::fs::metadata(fonts.join("free.ttf")).unwrap().len();
        s.execute("file.package", &json!({"dir": dir.join("src").to_string_lossy(), "idml": false})).unwrap();
        assert_eq!(std::fs::metadata(fonts.join("free.ttf")).unwrap().len(), len);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn package_copies_the_fallback_fonts_that_draw_text() {
        use designcraft_fonts::DOCUMENT_FONTS_FOLDER;
        use designcraft_fonts::testing::{font_with, with_fs_type};
        const NAMED: &str = "DocFont Fallback Named";
        const HELPER: &str = "DocFont Fallback Helper";
        const RESTRICTED: &str = "DocFont Fallback Restricted";
        // Characters no other font has: the named font lacks both, the document's other fonts
        // draw them.
        let (helped, restricted) = ('\u{F0A41}', '\u{F0A42}');
        let dir = std::env::temp_dir().join(format!("dc-package-fallback-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let fonts = dir.join("src").join(DOCUMENT_FONTS_FOLDER);
        std::fs::create_dir_all(&fonts).unwrap();
        std::fs::write(fonts.join("named.ttf"), font_with(NAMED, &['N']).unwrap()).unwrap();
        std::fs::write(fonts.join("helper.ttf"), font_with(HELPER, &[helped]).unwrap()).unwrap();
        std::fs::write(fonts.join("restricted.otf"), with_fs_type(font_with(RESTRICTED, &[restricted]).unwrap(), 0x0002).unwrap()).unwrap();
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("document.preferences", &json!({"glyphFallback": true})).unwrap();
        let text = format!("N{helped}{restricted}");
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 400, 300], "content": "text", "text": text})).unwrap();
        s.execute("text.select", &json!({"story": r["story"], "anchor": 0, "focus": text.len()})).unwrap();
        s.execute("type.char", &json!({"fontFamily": NAMED, "fontStyle": "Regular"})).unwrap();
        let src = dir.join("src").join("Fallback.designcraft");
        s.execute("file.save", &json!({"path": src.to_string_lossy()})).unwrap();
        let mut s = Session::new();
        assert_eq!(s.execute("file.open", &json!({"path": src.to_string_lossy()})).unwrap()["documentFonts"], 3);

        let out = dir.join("Pkg");
        let r = s.execute("file.package", &json!({"dir": out.to_string_lossy(), "idml": false})).unwrap();
        let packed_fonts = out.join(DOCUMENT_FONTS_FOLDER);
        assert!(packed_fonts.join("named.ttf").exists());
        assert!(packed_fonts.join("helper.ttf").exists(), "the fallback font that draws text is copied");
        assert!(!packed_fonts.join("restricted.otf").exists(), "a restricted licence keeps a fallback font out too");
        let report = r["report"].as_str().unwrap();
        let fallback = report.split("Fallback fonts").nth(1).unwrap_or_else(|| panic!("{report}"));
        assert!(fallback.contains(&format!("{HELPER} Regular\n")), "{report}");
        assert!(fallback.contains(&format!("{RESTRICTED} Regular — not copied: its licence doesn't allow it")), "{report}");
        assert!(!fallback.contains(NAMED), "{report}");
        // The packaged document draws the same text with its fonts.
        let doc = r["files"][0].as_str().unwrap().to_string();
        assert_eq!(Session::new().execute("file.open", &json!({"path": doc})).unwrap()["documentFonts"], 2);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
