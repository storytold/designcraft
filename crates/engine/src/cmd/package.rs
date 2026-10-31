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

/// The folder of a package that holds the files its document places.
const LINKS_FOLDER: &str = "Links";

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
        // Before anything is created: the folder must be inside the automation write root.
        crate::file_access::check_write(&path).map_err(EngineError::Other)?;
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

            // Like fs::write for a new output: request 0666 and let the process umask decide the
            // effective mode (0644 with the usual 022, 0664 with 002).
            builder.permissions(std::fs::Permissions::from_mode(0o666)); // NOSONAR
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
            "{dir, assets?: [asset ids] (default: all)} — write the placed files to `dir` and relink to the copies → {copied, files: [path]}",
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
/// `relative_to` names the folder `dir` is, beside the saved document: the links then read
/// `Links/logo.png`, so a package holds no path from the machine that made it (#340). `None` links
/// the copies by their full paths (Links › Copy Link(s) To, which relinks the open document).
fn write_links(d: &Document, root: &OutputRoot, dir: &Path, only: Option<&[AssetId]>, relative_to: Option<&str>) -> Result<(Document, Vec<PathBuf>)> {
    root.validate_directory(dir)?;
    let mut out = d.clone();
    let mut taken = Vec::new();
    let mut written = Vec::new();
    // Only files something places; the saved document leaves the others out.
    let ids = d.used_assets();
    for id in ids {
        if only.is_some_and(|o| !o.contains(&id)) {
            continue;
        }
        let Some(a) = d.assets.get(&id) else { continue };
        // A missing link has no bytes: an empty file under its name would read as present, so it
        // keeps its old (missing) path instead.
        if a.data.is_empty() {
            continue;
        }
        let name = link_name(a.link.as_deref().unwrap_or(&a.name), id, &mut taken);
        let path = dir.join(&name);
        root.write(&path, a.data.as_slice())?;
        let mut na = (**a).clone();
        na.link = Some(match relative_to {
            Some(folder) => format!("{folder}/{name}"),
            None => path.to_string_lossy().to_string(),
        });
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
    let to = dir.join(unique_name(font_file_name(name, face.data()), taken));
    match root.validate_directory(dir).and_then(|_| root.copy(from, &to)) {
        Ok(()) => {
            copied.push(from.clone());
            files.push(to);
            String::new()
        }
        Err(e) => format!(" — not copied: {e}"),
    }
}

/// `name` with a font extension when it has none: font services name their files as they like,
/// and a package's `Document Fonts` load only .ttf, .otf, .ttc and .otc files. The extension
/// comes from the file's first four bytes; a leading dot (a hidden file) is dropped.
fn font_file_name(name: String, data: &[u8]) -> String {
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase());
    if matches!(ext.as_deref(), Some("ttf" | "otf" | "ttc" | "otc")) {
        return name;
    }
    let add = match data.get(..4) {
        Some(b"OTTO") => "otf",
        Some(b"ttcf") => "ttc",
        Some([0, 1, 0, 0] | b"true") => "ttf",
        _ => return name,
    };
    let stem = name.trim_start_matches('.');
    format!("{}.{add}", if stem.is_empty() { "font" } else { stem })
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
    let links_dir = root.child(LINKS_FOLDER)?;
    let (packed, mut files) = write_links(&d, &root, &links_dir, None, Some(LINKS_FOLDER))?;
    let doc_path = package_output_path(&dir, &title, "designcraft")?;
    root.write(&doc_path, &super::to_bytes(&packed)?)?;
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
    // The IDML copy links the placed files, so a per-placement PDF layer choice cannot travel in
    // it (an external-only IDML export rejects one for the same reason). Say so in the report
    // instead of leaving a silent difference between the package's copies.
    let mut idml_layer_note = "";
    if p.get("idml").and_then(Value::as_bool).unwrap_or(true) {
        let path = package_output_path(&dir, &title, "idml")?;
        // The images are in Links/ beside it: link them there, without embedded copies.
        let opts = designcraft_idml::ExportOptions { embed_images: false };
        root.write(&path, &designcraft_idml::export_idml_with(&packed, &opts))?;
        files.push(path);
        if super::interchange::has_applicable_pdf_layer_overrides(&packed) {
            idml_layer_note = "\nPlaced PDF layers\nThe IDML copy links the placed PDFs, so it shows their file defaults. The DesignCraft document keeps each placement's Object Layer Options choice.\n";
        }
    }
    if p.get("pdf").and_then(Value::as_bool).unwrap_or(false) {
        let path = package_output_path(&dir, &title, "pdf")?;
        let bytes = designcraft_pdf::export_pdf(&super::interchange::with_pdf_boxes(&packed), &s.cache, &Default::default())
            .map_err(|e| EngineError::Other(e.to_string()))?;
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
    r += idml_layer_note;
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
    let (relinked, written) = write_links(&d, &root, &dir, only.as_deref(), None)?;
    s.edit(|doc, _| {
        doc.assets = relinked.assets;
        Ok(())
    })?;
    // The paths as well as the count: a caller that reports every file a run writes cannot
    // report files the command does not name, and `write_links` already knows them (noted in
    // review of #1214).
    Ok(json!({"copied": written.len(), "files": written.iter().map(|f| f.to_string_lossy().to_string()).collect::<Vec<_>>()}))
}

#[cfg(test)]
mod tests;
