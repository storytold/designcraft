//! A package handed over as files rather than as a folder on disk: the web build can't look beside
//! a file it opens, so the user hands it the whole package (a folder, or a zip of one) and its
//! `Document Fonts` load for the document from there, as the desktop app loads them from beside
//! the file ([`super::open::load_document_fonts`]).

use std::io::Read;

use designcraft_fonts::{DOCUMENT_FONTS_FOLDER, DocumentFonts, FontDb, MAX_DOCUMENT_FONT_BYTES, is_document_font_name};

use super::bytes::{base64_decode, base64_encode};
use super::{Arc, EngineError, Result, Session, Value, bad, json};

/// At most this many files are looked at in a package (a folder's listing or a zip's directory).
const MAX_PACKAGE_FILES: usize = 20_000;
/// The document read out of a zipped package is at most this large.
const MAX_PACKAGE_DOCUMENT_BYTES: u64 = 1 << 30;

/// The file extensions a package's document may have.
const DOCUMENT_EXTENSIONS: [&str; 4] = ["designcraft", "idml", "indd", "indt"];

/// The parts of a `/`-separated package path (a browser gives folder files paths such as
/// `Issue 12/Document Fonts/Palatino.ttf`; `\` from a zip made on Windows counts too).
fn parts(path: &str) -> Vec<&str> {
    path.split(['/', '\\']).filter(|p| !p.is_empty() && *p != ".").collect()
}

/// Whether `path` is a document a package opens: a DesignCraft, IDML or InDesign file, not a
/// macOS resource fork (`._Issue.idml`) or anything under `__MACOSX/`.
fn is_package_document(path: &str) -> bool {
    let p = parts(path);
    let Some(name) = p.last() else { return false };
    if name.starts_with("._") || p.iter().any(|c| c.eq_ignore_ascii_case("__MACOSX")) {
        return false;
    }
    let ext = std::path::Path::new(name).extension().and_then(std::ffi::OsStr::to_str).map(str::to_ascii_lowercase);
    ext.is_some_and(|e| DOCUMENT_EXTENSIONS.contains(&e.as_str()))
}

/// The package's document among `paths`: the one nearest the top (a package keeps it beside its
/// `Document Fonts` and `Links` folders), then the first by name.
fn package_document<'a>(paths: impl Iterator<Item = &'a str>) -> Option<&'a str> {
    paths.filter(|p| is_package_document(p)).min_by(|a, b| parts(a).len().cmp(&parts(b).len()).then_with(|| a.cmp(b)))
}

/// Whether `path` is a font file in the `Document Fonts` folder (any case) beside `document`.
fn beside_document(document: &str, path: &str) -> bool {
    let (d, p) = (parts(document), parts(path));
    let folder = &d[..d.len().saturating_sub(1)];
    is_document_font_name(path)
        && p.len() == folder.len() + 2
        && p.iter().zip(folder).all(|(a, b)| a == b)
        && p.get(folder.len()).is_some_and(|f| f.eq_ignore_ascii_case(DOCUMENT_FONTS_FOLDER))
        && !p.last().is_some_and(|n| n.starts_with("._"))
}

/// The fonts a folder handed over gives a document: those in a `Document Fonts` folder (any case)
/// when there is one among `paths`, else every font file (the `Document Fonts` folder itself was
/// chosen).
fn folder_fonts<'a>(paths: impl Iterator<Item = &'a str> + Clone) -> Vec<&'a str> {
    let in_folder = |p: &&str| {
        let p = parts(p);
        p.len() >= 2 && p.get(p.len() - 2).is_some_and(|f| f.eq_ignore_ascii_case(DOCUMENT_FONTS_FOLDER))
    };
    let fonts = paths.filter(|p| is_document_font_name(p) && !parts(p).last().is_some_and(|n| n.starts_with("._")));
    let named: Vec<&str> = fonts.clone().filter(in_folder).collect();
    if named.is_empty() { fonts.collect() } else { named }
}

/// The `files` parameter: `[{path, base64}]` → (path, bytes), at most [`MAX_PACKAGE_FILES`].
fn files_param(cmd: &str, p: &Value) -> Result<Vec<(String, String)>> {
    let files = p.get("files").and_then(Value::as_array).ok_or_else(|| bad(cmd, "missing `files` ([{path, base64}])"))?;
    if files.len() > MAX_PACKAGE_FILES {
        return Err(bad(cmd, format!("too many files ({}; at most {MAX_PACKAGE_FILES})", files.len())));
    }
    files
        .iter()
        .map(|f| {
            let path = f.get("path").and_then(Value::as_str).ok_or_else(|| bad(cmd, "a file without `path`"))?;
            let b64 = f.get("base64").and_then(Value::as_str).unwrap_or_default();
            Ok((path.to_string(), b64.to_string()))
        })
        .collect()
}

/// Make `fonts` the document fonts of document `index`, in place of any it had: its text is set
/// in them from now on. Not an edit: the document stays saved if it was.
fn attach_fonts(s: &mut Session, index: usize, fonts: &DocumentFonts) -> Result<()> {
    let st = s.docs.get_mut(index).ok_or(EngineError::NoDocument)?;
    let saved = Arc::ptr_eq(&st.doc, &st.saved_doc);
    Arc::make_mut(&mut st.doc).font_scope = fonts.scope;
    if saved {
        st.saved_doc = st.doc.clone();
    }
    // Dropping the scope it had closes it.
    st.fonts = (fonts.scope != 0).then(|| Arc::new(crate::FontScope(fonts.scope)));
    st.bump_revision();
    // A new cache: layout composed with the fonts it had isn't reused.
    s.cache = Arc::new(designcraft_compose::Cache::new());
    Ok(())
}

/// Load `fonts` (path, bytes) as document `index`'s fonts → (faces, a warning per file skipped).
fn load_fonts(s: &mut Session, index: usize, fonts: Vec<(String, Vec<u8>)>) -> Result<(usize, Vec<String>)> {
    let r = FontDb::global().load_document_font_files(fonts);
    attach_fonts(s, index, &r)?;
    Ok((r.faces, r.skipped.into_iter().map(|w| format!("{DOCUMENT_FONTS_FOLDER}: {w}")).collect()))
}

/// `file.openPackage {files: [{path, base64}]}`: open the package's document (see
/// [`package_document`]) with the fonts of the `Document Fonts` folder beside it.
pub(super) fn file_open_package(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "file.openPackage";
    let files = files_param(ID, p)?;
    let document = package_document(files.iter().map(|(p, _)| p.as_str()))
        .ok_or_else(|| bad(ID, "no DesignCraft, IDML or InDesign document in the package"))?
        .to_string();
    let fonts: Vec<(String, Vec<u8>)> =
        files.iter().filter(|(p, _)| beside_document(&document, p)).map(|(p, b)| (p.clone(), base64_decode(b))).collect();
    let doc_b64 = files.iter().find(|(p, _)| *p == document).map(|(_, b)| b.as_str()).unwrap_or_default();
    open_with_fonts(s, &document, doc_b64, fonts)
}

/// Open the document `path` (`base64`) handed over with `fonts`, its document fonts.
fn open_with_fonts(s: &mut Session, path: &str, base64: &str, fonts: Vec<(String, Vec<u8>)>) -> Result<Value> {
    let name = parts(path).last().map_or_else(|| path.to_string(), |n| (*n).to_string());
    let mut r = super::open::file_open_bytes(s, &json!({"name": name, "base64": base64}))?;
    let index = r.get("index").and_then(Value::as_u64).and_then(|i| usize::try_from(i).ok()).ok_or(EngineError::NoDocument)?;
    let (faces, skipped) = load_fonts(s, index, fonts)?;
    r["documentFonts"] = json!(faces);
    let mut warnings: Vec<Value> = r.get("warnings").and_then(Value::as_array).cloned().unwrap_or_default();
    warnings.extend(skipped.into_iter().map(Value::String));
    r["warnings"] = Value::Array(warnings);
    r["document"] = json!(path);
    Ok(r)
}

/// `font.loadDocumentFonts {files: [{path, base64}]}`: the fonts of a folder handed over become
/// the active document's fonts (see [`folder_fonts`]): for a document opened by itself, in a
/// browser that can't look beside it.
pub(super) fn load_document_fonts(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "font.loadDocumentFonts";
    let files = files_param(ID, p)?;
    let chosen = folder_fonts(files.iter().map(|(p, _)| p.as_str()));
    let fonts: Vec<(String, Vec<u8>)> =
        files.iter().filter(|(p, _)| chosen.contains(&p.as_str())).map(|(p, b)| (p.clone(), base64_decode(b))).collect();
    if fonts.is_empty() {
        return Err(bad(ID, format!("no font files (.ttf, .otf, .ttc, .otc) in the folder or its {DOCUMENT_FONTS_FOLDER} folder")));
    }
    let index = s.active_index().ok_or(EngineError::NoDocument)?;
    let (faces, warnings) = load_fonts(s, index, fonts)?;
    Ok(json!({"documentFonts": faces, "warnings": warnings}))
}

/// Whether `bytes` are a zipped package: a zip that is neither an IDML package nor a DesignCraft
/// file, holding a document.
pub(super) fn is_zipped_package(bytes: &[u8]) -> bool {
    bytes.starts_with(b"PK\x03\x04")
        && !designcraft_idml::is_idml(bytes)
        && !designcraft_format::is_designcraft(bytes)
        && zip::ZipArchive::new(std::io::Cursor::new(bytes)).is_ok_and(|z| z.len() <= MAX_PACKAGE_FILES && package_document(z.file_names()).is_some())
}

/// Read entry `name` of `z`, at most `max` bytes (a size the zip states is input: one byte past
/// the limit catches an entry larger than it says).
fn read_entry(z: &mut zip::ZipArchive<std::io::Cursor<&[u8]>>, name: &str, max: u64) -> std::result::Result<Vec<u8>, String> {
    let entry = z.by_name(name).map_err(|e| e.to_string())?;
    if entry.size() > max {
        return Err(format!("larger than {} MB", max >> 20));
    }
    let mut data = Vec::new();
    entry.take(max + 1).read_to_end(&mut data).map_err(|e| e.to_string())?;
    if data.len() as u64 > max {
        return Err(format!("larger than {} MB", max >> 20));
    }
    Ok(data)
}

/// Open a zipped package (see [`is_zipped_package`]): its document with the fonts of the
/// `Document Fonts` folder beside it. Only those entries are read.
pub(super) fn open_zipped_package(s: &mut Session, bytes: &[u8]) -> Result<Value> {
    const ID: &str = "file.openBytes";
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| bad(ID, format!("can't read the zip: {e}")))?;
    if z.len() > MAX_PACKAGE_FILES {
        return Err(bad(ID, format!("too many files in the zip ({}; at most {MAX_PACKAGE_FILES})", z.len())));
    }
    let names: Vec<String> = z.file_names().map(str::to_string).collect();
    let document = package_document(names.iter().map(String::as_str))
        .ok_or_else(|| bad(ID, "no DesignCraft, IDML or InDesign document in the zip"))?
        .to_string();
    let doc = read_entry(&mut z, &document, MAX_PACKAGE_DOCUMENT_BYTES).map_err(|e| bad(ID, format!("{document}: {e}")))?;
    let mut fonts = Vec::new();
    let mut warnings = Vec::new();
    for name in names.iter().filter(|n| beside_document(&document, n)) {
        // The font loader applies the total and the count; each file is capped as it's read.
        match read_entry(&mut z, name, MAX_DOCUMENT_FONT_BYTES) {
            Ok(data) => fonts.push((name.to_string(), data)),
            Err(e) => warnings.push(format!("{DOCUMENT_FONTS_FOLDER}: {name}: {e}")),
        }
    }
    let mut r = open_with_fonts(s, &document, &base64_encode(&doc), fonts)?;
    if let Some(w) = r.get_mut("warnings").and_then(Value::as_array_mut) {
        w.extend(warnings.into_iter().map(Value::String));
    }
    Ok(r)
}

#[cfg(test)]
mod tests;
