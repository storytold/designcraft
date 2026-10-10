//! File › Package and Links › Copy Links To: gather a document with the files it uses.

use std::fs::Metadata;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[cfg(not(target_arch = "wasm32"))]
use std::fs::File;
#[cfg(not(target_arch = "wasm32"))]
use std::io::Write;

use super::{CommandSpec, bad, cmd, has_doc, str_param};
use crate::{EngineError, Result, Session};
use designcraft_compose::ComposedStory;
use designcraft_doc::{AssetId, Document};
use designcraft_fonts::{FaceRef, FontFace, FontSource};
use serde_json::{Value, json};

const MAX_OUTPUT_COMPONENT_BYTES: usize = 200;

fn path_error(path: &Path, message: impl std::fmt::Display) -> EngineError {
    EngineError::Other(format!("{}: {message}", path.display()))
}

fn is_redirect(metadata: &Metadata) -> bool {
    // Rust reports Windows name-surrogate reparse points (including directory junctions) as
    // symlinks, while leaving non-redirecting reparse metadata such as cloud placeholders alone.
    metadata.file_type().is_symlink()
}

fn ensure_real_directory(path: &Path, ancestor: bool) -> Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => {
            if is_redirect(&metadata) {
                // An existing folder above the package may be reached through a link the user
                // set up (`/tmp` on macOS is one); the package folder itself may not.
                if ancestor && std::fs::metadata(path).is_ok_and(|m| m.is_dir()) {
                    return Ok(());
                }
                return Err(path_error(path, "refusing a redirected output directory"));
            }
            if !metadata.is_dir() {
                return Err(path_error(path, "output path is not a directory"));
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty())
                && parent != path
            {
                ensure_real_directory(parent, true)?;
            }
            if let Err(e) = std::fs::create_dir(path)
                && e.kind() != std::io::ErrorKind::AlreadyExists
            {
                return Err(path_error(path, e));
            }
            let metadata = std::fs::symlink_metadata(path).map_err(|e| path_error(path, e))?;
            if is_redirect(&metadata) || !metadata.is_dir() {
                return Err(path_error(path, "output directory was redirected while it was being created"));
            }
        }
        Err(e) => return Err(path_error(path, e)),
    }
    Ok(())
}

/// A selected output directory and its resolved identity. Every package path is checked against
/// it before use. Outputs are staged in the same directory and atomically persisted over an
/// existing regular file, preserving overwrite behavior without following file links.
struct OutputRoot {
    path: PathBuf,
    canonical: PathBuf,
}

impl OutputRoot {
    fn new(path: PathBuf) -> Result<Self> {
        ensure_real_directory(&path, false)?;
        let canonical = std::fs::canonicalize(&path).map_err(|e| path_error(&path, e))?;
        Ok(Self { path, canonical })
    }

    fn validate_directory(&self, path: &Path) -> Result<()> {
        if !path.starts_with(&self.path) {
            return Err(path_error(path, "output path is outside the selected directory"));
        }
        ensure_real_directory(path, false)?;
        let canonical = std::fs::canonicalize(path).map_err(|e| path_error(path, e))?;
        if !canonical.starts_with(&self.canonical) {
            return Err(path_error(path, "output directory resolves outside the selected directory"));
        }
        Ok(())
    }

    fn child(&self, name: &str) -> Result<PathBuf> {
        let path = self.path.join(name);
        if path.parent() != Some(self.path.as_path()) {
            return Err(path_error(&path, "invalid output directory name"));
        }
        Ok(path)
    }

    fn validate_file_entry(&self, path: &Path) -> Result<bool> {
        let parent = path.parent().ok_or_else(|| path_error(path, "output file has no parent directory"))?;
        self.validate_directory(parent)?;
        if !path.starts_with(&self.path) {
            return Err(path_error(path, "output file is outside the selected directory"));
        }
        match std::fs::symlink_metadata(path) {
            Ok(metadata) => {
                if is_redirect(&metadata) {
                    return Err(path_error(path, "refusing to replace a redirected output file"));
                }
                if !metadata.is_file() {
                    return Err(path_error(path, "output path is not a regular file"));
                }
                Ok(true)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(path_error(path, e)),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn temporary_file(&self, path: &Path, permissions: Option<std::fs::Permissions>) -> Result<tempfile::NamedTempFile> {
        let parent = path.parent().ok_or_else(|| path_error(path, "output file has no parent directory"))?;
        self.validate_directory(parent)?;
        let mut builder = tempfile::Builder::new();
        builder.prefix(".designcraft-package-").suffix(".tmp");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            // Match fs::write for a new output: 0666 restricted by the caller's umask.
            builder.permissions(std::fs::Permissions::from_mode(0o666));
        }
        let file = builder.tempfile_in(parent).map_err(|e| path_error(path, e))?;
        #[cfg(unix)]
        if let Some(permissions) = permissions {
            // Overwrites retain their prior mode; copied fonts retain their source mode.
            file.as_file().set_permissions(permissions).map_err(|e| path_error(path, e))?;
        }
        #[cfg(not(unix))]
        // The replacement inherits its directory's ACL. Rust's portable permission API cannot
        // reproduce a destination file's Windows DACL without platform-specific code.
        let _ = permissions;
        Ok(file)
    }

    fn write(&self, path: &Path, bytes: &[u8]) -> Result<()> {
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (path, bytes);
            return Err(EngineError::Other("package files are unavailable on the web".into()));
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let existed = self.validate_file_entry(path)?;
            let permissions = if existed { Some(std::fs::symlink_metadata(path).map_err(|e| path_error(path, e))?.permissions()) } else { None };
            let mut file = self.temporary_file(path, permissions)?;
            file.write_all(bytes).map_err(|e| path_error(path, e))?;
            // Recheck immediately before replacement. If a name is raced after this check,
            // persist replaces that directory entry rather than following it.
            self.validate_file_entry(path)?;
            file.persist(path).map_err(|e| path_error(path, e.error))?;
            Ok(())
        }
    }

    fn copy(&self, from: &Path, to: &Path) -> Result<()> {
        if self.validate_file_entry(to)?
            && std::fs::canonicalize(to).is_ok_and(|target| std::fs::canonicalize(from).is_ok_and(|source| source == target))
        {
            return Ok(());
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = from;
            return Err(EngineError::Other("package files are unavailable on the web".into()));
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let mut source = File::open(from).map_err(|e| path_error(from, e))?;
            let permissions = Some(source.metadata().map_err(|e| path_error(from, e))?.permissions());
            let mut target = self.temporary_file(to, permissions)?;
            std::io::copy(&mut source, &mut target).map_err(|e| path_error(to, e))?;
            self.validate_file_entry(to)?;
            target.persist(to).map_err(|e| path_error(to, e.error))?;
            Ok(())
        }
    }
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            noundo "file.package",
            "Package…",
            ["File"],
            None,
            "{dir: folder to create, idml?: true, pdf?: false, instructions?: text} — the document (its placed files relinked to Links/), Links/, the font files that draw its text in Document Fonts/ (fallback fonts included; unless their licence restricts it), an IDML copy (linking the files in Links/, nothing embedded), an optional PDF and a report → {dir, files, report}",
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

fn is_reserved_windows_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or(name).trim_end_matches([' ', '.']);
    ["con", "prn", "aux", "nul"].into_iter().any(|reserved| caseless::compatibility_caseless_match_str(stem, reserved))
        || (1..=9).any(|n| {
            caseless::compatibility_caseless_match_str(stem, &format!("com{n}"))
                || caseless::compatibility_caseless_match_str(stem, &format!("lpt{n}"))
        })
}

fn portable_names_equal(a: &str, b: &str) -> bool {
    caseless::canonical_caseless_match_str(a, b)
}

/// A portable single path component made from document metadata.
fn safe_component(name: &str, fallback: &str) -> String {
    let leaf = name.rsplit(['/', '\\']).find(|part| !part.is_empty()).unwrap_or("");
    let mut safe = String::new();
    for ch in leaf.trim().chars() {
        let replacement = ch.is_control() || matches!(ch, '<' | '>' | ':' | '"' | '|' | '?' | '*' | '/' | '\\');
        let ch = if replacement { '_' } else { ch };
        if safe.len() + ch.len_utf8() > MAX_OUTPUT_COMPONENT_BYTES {
            break;
        }
        safe.push(ch);
    }
    safe.truncate(safe.trim_end_matches([' ', '.']).len());
    if safe.is_empty() || safe == "." || safe == ".." {
        safe = fallback.to_string();
    }
    if is_reserved_windows_name(&safe) {
        safe.insert(0, '_');
    }
    safe
}

fn package_output_path(dir: &Path, title: &str, extension: &str) -> Result<PathBuf> {
    let path = dir.join(format!("{title}.{extension}"));
    if path.parent() != Some(dir) {
        return Err(bad("file.package", "an output filename would be outside the selected directory"));
    }
    Ok(path)
}

/// A file name for asset `a` in a Links folder, unique among `taken`.
fn link_name(name: &str, id: AssetId, taken: &mut Vec<String>) -> String {
    let fallback = format!("asset-{}", id.0);
    unique_name(safe_component(name, &fallback), taken)
}

/// `base`, or `base` numbered ("logo 2.png"), unique under portable Unicode normalization and
/// case-insensitive comparison.
fn unique_name(base: String, taken: &mut Vec<String>) -> String {
    let (stem, ext) = match base.rsplit_once('.') {
        Some((s, e)) => (s.to_string(), format!(".{e}")),
        None => (base.clone(), String::new()),
    };
    let mut n = base;
    let mut k = 2;
    while taken.iter().any(|t| portable_names_equal(t, &n)) {
        n = format!("{stem} {k}{ext}");
        k += 1;
    }
    taken.push(n.clone());
    n
}

/// Write each asset's bytes into `dir`; returns the relinked document and the paths written.
fn write_links(d: &Document, root: &OutputRoot, dir: &Path, only: Option<&[AssetId]>) -> Result<(Document, Vec<PathBuf>)> {
    root.validate_directory(dir)?;
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
        root.write(&path, a.data.as_slice())?;
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
fn copy_font(root: &OutputRoot, face: &FontFace, dir: &Path, copied: &mut Vec<PathBuf>, taken: &mut Vec<String>, files: &mut Vec<PathBuf>) -> String {
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
    let name = safe_component(&from.file_name().map(|n| n.to_string_lossy()).unwrap_or_default(), "font");
    let to = dir.join(unique_name(name, taken));
    match root.validate_directory(dir).and_then(|_| root.copy(from, &to)) {
        Ok(()) => {
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
    let root = OutputRoot::new(dir.clone())?;
    let display_title = st.doc.title.clone();
    let title = safe_component(&display_title, "Untitled");
    let fonts = s.execute("font.list", &json!({}))?;
    let links = s.execute("links.list", &json!({}))?;
    let pre = s.execute("preflight.run", &json!({})).unwrap_or(Value::Null);
    let d = s.doc()?.doc.clone();
    let links_dir = root.child("Links")?;
    let (packed, mut files) = write_links(&d, &root, &links_dir, None)?;
    let doc_path = package_output_path(&dir, &title, "designcraft")?;
    root.write(&doc_path, &super::to_bytes(&packed))?;
    files.insert(0, doc_path);
    // The font files, and what the report says about each font.
    let db = designcraft_fonts::FontDb::global().scoped(d.font_scope);
    let fonts_dir = root.child(designcraft_fonts::DOCUMENT_FONTS_FOLDER)?;
    let (mut copied, mut taken) = (Vec::new(), Vec::new());
    let (mut font_lines, mut named) = (Vec::new(), Vec::new());
    for f in fonts.as_array().into_iter().flatten() {
        let (family, style) = (f["family"].as_str().unwrap_or(""), f["style"].as_str().unwrap_or(""));
        let missing = f["missing"].as_bool().unwrap_or(false) || f["styleMissing"].as_bool().unwrap_or(false);
        // A missing font's text is drawn in its substitute, which isn't a fallback font.
        let face = db.face(family, style);
        named.push(face.id());
        let note = if missing { " — MISSING".to_string() } else { copy_font(&root, &face, &fonts_dir, &mut copied, &mut taken, &mut files) };
        font_lines.push(format!("  {family} {style}{note}\n"));
    }
    // Fallback fonts: the faces that draw characters the named fonts lack.
    let mut drawn = Vec::new();
    for sid in d.stories.keys() {
        drawn_faces(&s.cache.get(&d, *sid, None), 0, &mut drawn);
    }
    drawn.retain(|f| !named.contains(&f.id()));
    drawn.sort_by(|a, b| (&a.family, &a.style).cmp(&(&b.family, &b.style)));
    let fallback_lines: Vec<String> = drawn
        .iter()
        .map(|f| format!("  {} {}{}\n", f.family, f.style, copy_font(&root, f, &fonts_dir, &mut copied, &mut taken, &mut files)))
        .collect();
    if p.get("idml").and_then(Value::as_bool).unwrap_or(true) {
        let path = package_output_path(&dir, &title, "idml")?;
        // The images are in Links/ beside it: link them there, without embedded copies.
        let opts = designcraft_idml::ExportOptions { embed_images: false };
        root.write(&path, &designcraft_idml::export_idml_with(&packed, &opts))?;
        files.push(path);
    }
    if p.get("pdf").and_then(Value::as_bool).unwrap_or(false) {
        let path = package_output_path(&dir, &title, "pdf")?;
        let bytes = designcraft_pdf::export_pdf(&packed, &s.cache, &Default::default()).map_err(|e| EngineError::Other(e.to_string()))?;
        root.write(&path, &bytes)?;
        files.push(path);
    }
    // The report: fonts (missing first), links, preflight, the user's instructions.
    let mut r = format!("Package report: {display_title}\n\n");
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
    root.write(&report, r.as_bytes())?;
    files.push(report);
    Ok(json!({"dir": dir.to_string_lossy(), "files": files.iter().map(|f| f.to_string_lossy().to_string()).collect::<Vec<_>>(), "report": r}))
}

fn copy_links(s: &mut Session, p: &Value) -> Result<Value> {
    let dir = PathBuf::from(str_param(p, "dir").ok_or_else(|| bad("links.copyTo", "missing `dir`"))?);
    let only: Option<Vec<AssetId>> = p.get("assets").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_u64).map(AssetId).collect());
    let d = s.doc()?.doc.clone();
    let root = OutputRoot::new(dir.clone())?;
    let (relinked, written) = write_links(&d, &root, &dir, only.as_deref())?;
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
            assert_eq!(*asset.data, png, "{extension}: the packaged pixels");
            assert_eq!(asset.link.as_deref(), expected.to_str(), "{extension}: use the relocated Links file");
            let links = opened.execute("links.list", &json!({})).unwrap();
            assert_eq!(links[0]["status"], "ok", "{extension}");
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// The package's IDML links its images to the files in the package's Links folder and embeds
    /// no copies of them.
    #[test]
    fn package_idml_links_the_links_folder_and_embeds_nothing() {
        let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("dc-package-idml-links-{}-{nonce}", std::process::id()));
        let png = designcraft_render::Rendered { width: 4, height: 3, pixels: [30, 90, 180, 255].repeat(12) }.to_png();
        let mut s = Session::new();
        s.execute("file.new", &json!({"title": "LinkedPackage", "width": 72, "height": 72})).unwrap();
        s.execute("file.place", &json!({"base64": super::super::base64_encode(&png), "name": "my photo.png", "x": 5, "y": 5, "width": 16})).unwrap();
        s.execute("file.package", &json!({"dir": dir.to_string_lossy(), "idml": true, "pdf": false})).unwrap();
        let idml = std::fs::read(dir.join("LinkedPackage.idml")).unwrap();
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(idml.as_slice())).unwrap();
        let mut spreads = String::new();
        for i in 0..zip.len() {
            let mut f = zip.by_index(i).unwrap();
            if f.name().starts_with("Spreads/") {
                std::io::Read::read_to_string(&mut f, &mut spreads).unwrap();
            }
        }
        let linked = dir.join("Links").join("my photo.png");
        let uri = format!("file:{}", linked.to_string_lossy().replace(' ', "%20"));
        assert!(spreads.contains(&format!(r#"LinkResourceURI="{uri}""#)), "{spreads}");
        assert!(!spreads.contains("<Contents") && !spreads.contains(r#"StoredState="Embedded""#), "no image is embedded");
        let back = designcraft_idml::import_idml_with(&idml, &|_| None).unwrap();
        let asset = back.assets.values().next().unwrap();
        assert!(asset.data.is_empty(), "no image is embedded");
        assert_eq!(asset.link.as_deref(), linked.to_str());
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
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            std::fs::set_permissions(fonts.join("free.ttf"), std::fs::Permissions::from_mode(0o640)).unwrap();
        }
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
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            assert_eq!(
                std::fs::metadata(packed_fonts.join("free.ttf")).unwrap().permissions().mode() & 0o777,
                std::fs::metadata(fonts.join("free.ttf")).unwrap().permissions().mode() & 0o777
            );
        }
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

    #[test]
    fn package_uses_portable_names_inside_the_selected_directory() {
        assert_eq!(safe_component("Archive/Annual: Review", "Untitled"), "Annual_ Review");
        assert_eq!(safe_component(".", "Untitled"), "Untitled");
        assert_eq!(safe_component("CON", "Untitled"), "_CON");
        assert_eq!(safe_component("COM¹.txt", "Untitled"), "_COM¹.txt");
        assert_eq!(safe_component("LPT³", "Untitled"), "_LPT³");
        for name in ["COM¹", "COM²", "COM³", "LPT¹", "LPT²", "LPT³"] {
            assert!(is_reserved_windows_name(name), "{name}");
        }
        let mut taken = Vec::new();
        assert_eq!(unique_name("Logo.svg".into(), &mut taken), "Logo.svg");
        assert_eq!(unique_name("logo.svg".into(), &mut taken), "logo 2.svg");
        assert_eq!(unique_name("Résumé.pdf".into(), &mut taken), "Résumé.pdf");
        assert_eq!(unique_name("Re\u{301}sume\u{301}.pdf".into(), &mut taken), "Re\u{301}sume\u{301} 2.pdf");
        assert_eq!(unique_name("ΟΣ.txt".into(), &mut taken), "ΟΣ.txt");
        assert_eq!(unique_name("ος.txt".into(), &mut taken), "ος 2.txt");

        let dir = std::env::temp_dir().join(format!("dc-package-safe-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut s = Session::new();
        s.execute("file.new", &json!({"title": "Archive/Annual: Review"})).unwrap();
        let r = s.execute("file.package", &json!({"dir": dir.to_string_lossy(), "idml": false})).unwrap();
        let files = r["files"].as_array().unwrap();
        assert!(files.iter().all(|path| Path::new(path.as_str().unwrap()).starts_with(&dir)));
        assert!(dir.join("Annual_ Review.designcraft").is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn package_overwrites_existing_regular_outputs() {
        let dir = std::env::temp_dir().join(format!("dc-package-overwrite-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let document = dir.join("Annual.designcraft");
        std::fs::write(&document, b"previous package").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            std::fs::set_permissions(&document, std::fs::Permissions::from_mode(0o640)).unwrap();
        }

        let mut s = Session::new();
        s.execute("file.new", &json!({"title": "Annual"})).unwrap();
        s.execute("file.package", &json!({"dir": dir.to_string_lossy(), "idml": false})).unwrap();
        s.execute("file.package", &json!({"dir": dir.to_string_lossy(), "idml": false})).unwrap();
        assert!(super::super::from_bytes(&std::fs::read(&document).unwrap()).is_ok());
        assert!(!dir.join(designcraft_fonts::DOCUMENT_FONTS_FOLDER).exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            assert_eq!(std::fs::metadata(&document).unwrap().permissions().mode() & 0o777, 0o640);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn package_accepts_a_relative_destination() {
        let dir = PathBuf::from(format!(".dc-package-relative-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut s = Session::new();
        s.execute("file.new", &json!({"title": "Relative"})).unwrap();
        s.execute("file.package", &json!({"dir": dir.to_string_lossy(), "idml": false})).unwrap();
        assert!(dir.join("Relative.designcraft").is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn package_refuses_redirected_destinations() {
        use std::os::unix::fs::symlink;

        let base = std::env::temp_dir().join(format!("dc-package-redirects-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let outside = base.join("separate");
        std::fs::create_dir_all(&outside).unwrap();
        let mut s = Session::new();
        s.execute("file.new", &json!({"title": "Annual"})).unwrap();

        let redirected_root = base.join("redirected-package");
        symlink(&outside, &redirected_root).unwrap();
        assert!(s.execute("file.package", &json!({"dir": redirected_root.to_string_lossy(), "idml": false})).is_err());
        assert!(!outside.join("Annual.designcraft").exists());
        std::fs::remove_file(&redirected_root).unwrap();

        // A linked folder above the package is followed, as `/tmp` is on macOS.
        let linked_parent = base.join("linked-parent");
        symlink(&outside, &linked_parent).unwrap();
        let nested_package = linked_parent.join("nested").join("Package");
        s.execute("file.package", &json!({"dir": nested_package.to_string_lossy(), "idml": false})).unwrap();
        assert!(outside.join("nested").join("Package").join("Annual.designcraft").is_file());
        std::fs::remove_file(&linked_parent).unwrap();

        let package = base.join("Package");
        std::fs::create_dir_all(&package).unwrap();
        symlink(&outside, package.join("Links")).unwrap();
        assert!(s.execute("file.package", &json!({"dir": package.to_string_lossy(), "idml": false})).is_err());
        assert!(!outside.join("Annual.designcraft").exists());
        std::fs::remove_file(package.join("Links")).unwrap();
        std::fs::create_dir(package.join("Links")).unwrap();

        let kept = outside.join("kept.designcraft");
        std::fs::write(&kept, b"kept").unwrap();
        symlink(&kept, package.join("Annual.designcraft")).unwrap();
        assert!(s.execute("file.package", &json!({"dir": package.to_string_lossy(), "idml": false})).is_err());
        assert_eq!(std::fs::read(&kept).unwrap(), b"kept");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[cfg(windows)]
    #[test]
    fn package_refuses_windows_directory_redirects() {
        use std::os::windows::fs::symlink_dir;

        let base = std::env::temp_dir().join(format!("dc-package-windows-redirect-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let outside = base.join("separate");
        std::fs::create_dir_all(&outside).unwrap();
        let redirected = base.join("Package");
        match symlink_dir(&outside, &redirected) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                let _ = std::fs::remove_dir_all(&base);
                return;
            }
            Err(e) => panic!("could not create directory redirect: {e}"),
        }
        let mut s = Session::new();
        s.execute("file.new", &json!({"title": "Annual"})).unwrap();
        assert!(s.execute("file.package", &json!({"dir": redirected.to_string_lossy(), "idml": false})).is_err());
        assert!(!outside.join("Annual.designcraft").exists());
        std::fs::remove_dir(&redirected).unwrap();
        let _ = std::fs::remove_dir_all(&base);
    }
}
