//! The native `.designcraft` format: a zip archive holding
//!
//! - `mimetype` (stored first, uncompressed): `application/vnd.designcraft+zip`
//! - `document.json`: the [`Document`] (pretty JSON, documented by the serde model in `designcraft-doc`)
//! - `assets/<id>.<ext>`: embedded placed files, byte-for-byte
//! - `meta.json`: format version and generator
//!
//! Older single-file JSON documents (assets as base64 in `assetData`) still open.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

use std::collections::HashSet;
use std::io::{Cursor, Read, Write};
use std::sync::Arc;

use designcraft_doc::{AssetId, Document};
use serde_json::{Value, json};

pub const MIME: &str = "application/vnd.designcraft+zip";
pub const VERSION: u32 = 1;

const MAX_ARCHIVE_BYTES: usize = 512 * 1024 * 1024;
const MAX_ARCHIVE_ENTRIES: usize = 4_096;
const MAX_CENTRAL_DIRECTORY_BYTES: u64 = 16 * 1024 * 1024;
const MAX_ENTRY_BYTES: u64 = 256 * 1024 * 1024;
const MAX_EXPANDED_BYTES: u64 = 512 * 1024 * 1024;
const MAX_META_BYTES: usize = 1024 * 1024;
const MAX_DOCUMENT_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone, Copy)]
struct ArchiveLimits {
    entries: usize,
    entry_bytes: u64,
    expanded_bytes: u64,
}

const ARCHIVE_LIMITS: ArchiveLimits =
    ArchiveLimits { entries: MAX_ARCHIVE_ENTRIES, entry_bytes: MAX_ENTRY_BYTES, expanded_bytes: MAX_EXPANDED_BYTES };

const PREFLIGHT_LIMITS: designcraft_archive::Limits =
    designcraft_archive::Limits { max_entries: MAX_ARCHIVE_ENTRIES as u64, max_central_directory_bytes: MAX_CENTRAL_DIRECTORY_BYTES };

#[derive(Clone, Copy)]
struct SaveLimits {
    entries: usize,
    entry_bytes: u64,
    expanded_bytes: u64,
    meta_bytes: usize,
    document_bytes: usize,
}

const SAVE_LIMITS: SaveLimits = SaveLimits {
    entries: MAX_ARCHIVE_ENTRIES,
    entry_bytes: MAX_ENTRY_BYTES,
    expanded_bytes: MAX_EXPANDED_BYTES,
    meta_bytes: MAX_META_BYTES,
    document_bytes: MAX_DOCUMENT_BYTES,
};

#[derive(Debug, thiserror::Error)]
pub enum FormatError {
    #[error("not a DesignCraft document: {0}")]
    NotOurs(String),
    #[error("document is from a newer DesignCraft (format {0}); please update")]
    TooNew(u32),
    #[error("{0}")]
    Io(String),
}

fn ext_for(mime: &str) -> &'static str {
    match mime {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/tiff" => "tif",
        "application/pdf" => "pdf",
        "image/svg+xml" => "svg",
        _ => "bin",
    }
}

/// Serialize a document to `.designcraft` bytes.
pub fn save(doc: &Document) -> Result<Vec<u8>, FormatError> {
    let io = |e: zip::result::ZipError| FormatError::Io(e.to_string());
    let meta = json!({"format": "designcraft", "version": VERSION, "generator": concat!("DesignCraft ", env!("CARGO_PKG_VERSION"))}).to_string();
    let json = serde_json::to_vec_pretty(doc).map_err(|e| FormatError::Io(e.to_string()))?;
    validate_save_parts(json.len(), meta.len(), doc.assets.values().filter(|a| !a.data.is_empty()).map(|a| a.data.len()), SAVE_LIMITS)?;
    let mut buf = Cursor::new(Vec::new());
    {
        let mut z = zip::ZipWriter::new(&mut buf);
        let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        let deflate = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        z.start_file("mimetype", stored).map_err(io)?;
        z.write_all(MIME.as_bytes()).map_err(|e| FormatError::Io(e.to_string()))?;
        z.start_file("meta.json", deflate).map_err(io)?;
        z.write_all(meta.as_bytes()).map_err(|e| FormatError::Io(e.to_string()))?;
        z.start_file("document.json", deflate).map_err(io)?;
        z.write_all(&json).map_err(|e| FormatError::Io(e.to_string()))?;
        for (id, a) in &doc.assets {
            if a.data.is_empty() {
                continue;
            }
            // Already-compressed images are stored as-is.
            let opts = if matches!(a.mime.as_str(), "image/png" | "image/jpeg" | "image/gif" | "image/webp") { stored } else { deflate };
            z.start_file(format!("assets/{}.{}", id.0, ext_for(&a.mime)), opts).map_err(io)?;
            z.write_all(&a.data).map_err(|e| FormatError::Io(e.to_string()))?;
        }
        z.finish().map_err(io)?;
    }
    let bytes = buf.into_inner();
    if bytes.len() > MAX_ARCHIVE_BYTES {
        return Err(FormatError::Io(format!("saved file exceeds the {MAX_ARCHIVE_BYTES}-byte size limit")));
    }
    designcraft_archive::preflight(&bytes, PREFLIGHT_LIMITS).map_err(|e| FormatError::Io(format!("saved ZIP is outside format limits: {e}")))?;
    Ok(bytes)
}

fn validate_save_parts(
    document_bytes: usize,
    meta_bytes: usize,
    asset_sizes: impl Iterator<Item = usize>,
    limits: SaveLimits,
) -> Result<(), FormatError> {
    if document_bytes > limits.document_bytes {
        return Err(FormatError::Io(format!("document JSON exceeds the {}-byte size limit", limits.document_bytes)));
    }
    if meta_bytes > limits.meta_bytes {
        return Err(FormatError::Io(format!("metadata exceeds the {}-byte size limit", limits.meta_bytes)));
    }
    let mut entries = 3usize;
    if entries > limits.entries {
        return Err(FormatError::Io(format!("saved document contains more than {} entries", limits.entries)));
    }
    let mut expanded = MIME
        .len()
        .checked_add(meta_bytes)
        .and_then(|n| n.checked_add(document_bytes))
        .ok_or_else(|| FormatError::Io("saved document size overflow".into()))?;
    for size in asset_sizes {
        entries = entries.checked_add(1).ok_or_else(|| FormatError::Io("saved document entry count overflow".into()))?;
        if entries > limits.entries {
            return Err(FormatError::Io(format!("saved document contains more than {} entries", limits.entries)));
        }
        let size_u64 = u64::try_from(size).map_err(|_| FormatError::Io("saved asset size does not fit the file format".into()))?;
        if size_u64 > limits.entry_bytes {
            return Err(FormatError::Io(format!("saved asset exceeds the {}-byte entry limit", limits.entry_bytes)));
        }
        expanded = expanded.checked_add(size).ok_or_else(|| FormatError::Io("saved document expanded size overflow".into()))?;
    }
    let expanded = u64::try_from(expanded).map_err(|_| FormatError::Io("saved document expanded size does not fit the file format".into()))?;
    if expanded > limits.expanded_bytes {
        return Err(FormatError::Io(format!("saved document expands beyond the {}-byte limit", limits.expanded_bytes)));
    }
    Ok(())
}

/// Read `.designcraft` bytes (zip, or the legacy single JSON file).
pub fn load(bytes: &[u8]) -> Result<Document, FormatError> {
    if bytes.starts_with(b"PK") {
        if bytes.len() > MAX_ARCHIVE_BYTES {
            return Err(FormatError::NotOurs(format!("file exceeds the {MAX_ARCHIVE_BYTES}-byte size limit")));
        }
        return load_zip(bytes);
    }
    if bytes.len() > MAX_DOCUMENT_BYTES {
        return Err(FormatError::NotOurs(format!("legacy document exceeds the {MAX_DOCUMENT_BYTES}-byte size limit")));
    }
    load_legacy_json(bytes)
}

fn validate_archive(z: &mut zip::ZipArchive<Cursor<&[u8]>>, limits: ArchiveLimits) -> Result<(), FormatError> {
    if z.len() > limits.entries {
        return Err(FormatError::NotOurs(format!("archive contains more than {} entries", limits.entries)));
    }
    let mut expanded = 0u64;
    for i in 0..z.len() {
        let f = z.by_index(i).map_err(|e| FormatError::NotOurs(e.to_string()))?;
        let size = f.size();
        if size > limits.entry_bytes {
            return Err(FormatError::NotOurs(format!("archive entry exceeds the {}-byte limit", limits.entry_bytes)));
        }
        expanded = expanded.checked_add(size).ok_or_else(|| FormatError::NotOurs("archive expanded size is too large".into()))?;
        if expanded > limits.expanded_bytes {
            return Err(FormatError::NotOurs(format!("archive expands beyond the {}-byte limit", limits.expanded_bytes)));
        }
    }
    Ok(())
}

fn read_entry(z: &mut zip::ZipArchive<Cursor<&[u8]>>, name: &str, max_bytes: usize) -> Result<Option<Vec<u8>>, FormatError> {
    let mut f = match z.by_name(name) {
        Ok(f) => f,
        Err(zip::result::ZipError::FileNotFound) => return Ok(None),
        Err(e) => return Err(FormatError::NotOurs(e.to_string())),
    };
    if f.size() > max_bytes as u64 {
        return Err(FormatError::NotOurs(format!("{name} exceeds the {max_bytes}-byte limit")));
    }
    let capacity = usize::try_from(f.size()).unwrap_or(max_bytes).min(max_bytes);
    let mut v = Vec::with_capacity(capacity);
    (&mut f).take(max_bytes.saturating_add(1) as u64).read_to_end(&mut v).map_err(|e| FormatError::NotOurs(format!("can't read {name}: {e}")))?;
    if v.len() > max_bytes {
        return Err(FormatError::NotOurs(format!("{name} exceeds the {max_bytes}-byte limit")));
    }
    Ok(Some(v))
}

fn load_zip(bytes: &[u8]) -> Result<Document, FormatError> {
    designcraft_archive::preflight(bytes, PREFLIGHT_LIMITS).map_err(|e| FormatError::NotOurs(e.to_string()))?;
    let mut z = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| FormatError::NotOurs(e.to_string()))?;
    validate_archive(&mut z, ARCHIVE_LIMITS)?;
    if let Some(meta) = read_entry(&mut z, "meta.json", MAX_META_BYTES)?
        && let Ok(m) = serde_json::from_slice::<Value>(&meta)
        && let Some(v) = m.get("version").and_then(Value::as_u64)
        && v as u32 > VERSION
    {
        return Err(FormatError::TooNew(v as u32));
    }
    let json = read_entry(&mut z, "document.json", MAX_DOCUMENT_BYTES)?.ok_or_else(|| FormatError::NotOurs("missing document.json".into()))?;
    let mut v: Value = serde_json::from_slice(&json).map_err(|e| FormatError::NotOurs(e.to_string()))?;
    upgrade(&mut v);
    let mut doc: Document = serde_json::from_value(v).map_err(|e| FormatError::NotOurs(e.to_string()))?;
    let names: HashSet<String> = z.file_names().filter(|name| name.starts_with("assets/")).map(str::to_string).collect();
    for name in names {
        let Some(stem) = name.strip_prefix("assets/") else { continue };
        let Some(id) = stem.split('.').next().and_then(|s| s.parse::<u64>().ok()) else { continue };
        if let (Some(data), Some(a)) = (read_entry(&mut z, &name, MAX_ENTRY_BYTES as usize)?, doc.assets.get_mut(&AssetId(id))) {
            Arc::make_mut(a).data = Arc::new(data);
        }
    }
    doc.check().map_err(|e| FormatError::NotOurs(e.to_string()))?;
    Ok(doc)
}

fn load_legacy_json(bytes: &[u8]) -> Result<Document, FormatError> {
    let mut v: Value = serde_json::from_slice(bytes).map_err(|e| FormatError::NotOurs(e.to_string()))?;
    upgrade(&mut v);
    let mut doc: Document = serde_json::from_value(v.clone()).map_err(|e| FormatError::NotOurs(e.to_string()))?;
    if let Some(data) = v.get("assetData").and_then(Value::as_object) {
        for (k, s) in data {
            if let (Ok(id), Some(s)) = (k.parse::<u64>(), s.as_str())
                && let Some(a) = doc.assets.get_mut(&AssetId(id))
            {
                Arc::make_mut(a).data = Arc::new(base64_decode(s));
            }
        }
    }
    doc.check().map_err(|e| FormatError::NotOurs(e.to_string()))?;
    Ok(doc)
}

/// Bring older documents' JSON up to the current model. Vertical Type was a text frame option
/// (`content.options.vertical`); it is the story's direction, which a story takes from its first
/// frame.
fn upgrade(doc: &mut Value) {
    let mut vertical = HashSet::new();
    vertical_frames(doc, &mut vertical, 0);
    if vertical.is_empty() {
        return;
    }
    let Some(stories) = doc.get_mut("stories").and_then(Value::as_object_mut) else { return };
    for st in stories.values_mut() {
        let first = st.get("frames").and_then(|f| f.get(0)).and_then(Value::as_u64);
        if let (Some(f), Some(st)) = (first, st.as_object_mut())
            && vertical.contains(&f)
            && !st.contains_key("vertical")
        {
            st.insert("vertical".into(), Value::Bool(true));
        }
    }
}

/// Ids of the text frames in `v` (any depth: groups, anchored objects) whose options say vertical.
fn vertical_frames(v: &Value, out: &mut HashSet<u64>, depth: usize) {
    // serde_json already refuses JSON nested deeper than 128 levels.
    if depth > 256 {
        return;
    }
    match v {
        Value::Object(m) => {
            if let (Some(id), Some(c)) = (m.get("id").and_then(Value::as_u64), m.get("content"))
                && c.get("type").and_then(Value::as_str) == Some("text")
                && c.get("options").and_then(|o| o.get("vertical")).and_then(Value::as_bool) == Some(true)
            {
                out.insert(id);
            }
            for c in m.values() {
                vertical_frames(c, out, depth + 1);
            }
        }
        Value::Array(a) => {
            for c in a {
                vertical_frames(c, out, depth + 1);
            }
        }
        _ => {}
    }
}

fn base64_decode(s: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut buf, mut bits) = (0u32, 0);
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

#[cfg(test)]
mod tests {
    use super::*;
    use designcraft_doc::build::NewDocument;
    use designcraft_doc::geom::Rect;
    use designcraft_doc::{Asset, ParaFormat, SpreadRef};

    #[test]
    fn roundtrip_with_assets() {
        let mut d = Document::new(&NewDocument { pages: 3, ..Default::default() });
        let lid = d.default_layer();
        d.add_text_frame(SpreadRef::Doc(0), Rect::new(10.0, 10.0, 200.0, 200.0), lid, "héllo\nworld", ParaFormat::default()).unwrap();
        let aid = AssetId(d.alloc());
        d.assets.insert(
            aid,
            Arc::new(Asset {
                page: 0,
                id: aid,
                name: "x.png".into(),
                mime: "image/png".into(),
                link: None,
                data: Arc::new(vec![1, 2, 3, 4]),
                pixels: Some((1, 1)),
            }),
        );
        let bytes = save(&d).unwrap();
        assert!(bytes.starts_with(b"PK"));
        let back = load(&bytes).unwrap();
        assert_eq!(back.page_count(), 3);
        assert_eq!(*back.assets[&aid].data, vec![1, 2, 3, 4]);
        assert_eq!(back.stories, d.stories);
    }

    /// The object of the item `id` in a document's JSON.
    fn item_json(v: &mut Value, id: u64) -> Option<&mut Value> {
        if v.get("id").and_then(Value::as_u64) == Some(id) && v.get("content").is_some() {
            return Some(v);
        }
        match v {
            Value::Object(m) => m.values_mut().find_map(|c| item_json(c, id)),
            Value::Array(a) => a.iter_mut().find_map(|c| item_json(c, id)),
            _ => None,
        }
    }

    #[test]
    fn frame_level_vertical_text_opens_as_a_vertical_story() {
        // Documents that set Vertical on the frame: the story takes its first frame's direction.
        let mut d = Document::new(&NewDocument::default());
        let lid = d.default_layer();
        let (a, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(72.0, 72.0, 200.0, 400.0), lid, "縦書き", ParaFormat::default()).unwrap();
        let (b, _) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(300.0, 72.0, 428.0, 400.0), lid, "", ParaFormat::default()).unwrap();
        d.thread(a, b).unwrap();
        let (c, other) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(450.0, 72.0, 550.0, 400.0), lid, "横", ParaFormat::default()).unwrap();
        let mut v = serde_json::to_value(&d).unwrap();
        item_json(&mut v, a.0).unwrap()["content"]["options"]["vertical"] = json!(true);
        item_json(&mut v, c.0).unwrap()["content"]["options"]["vertical"] = json!(false);
        let json = serde_json::to_vec(&v).unwrap();
        let mut zipped = Cursor::new(Vec::new());
        {
            let mut z = zip::ZipWriter::new(&mut zipped);
            z.start_file("document.json", zip::write::SimpleFileOptions::default()).unwrap();
            z.write_all(&json).unwrap();
            z.finish().unwrap();
        }
        for bytes in [json.clone(), zipped.into_inner()] {
            let back = load(&bytes).unwrap();
            assert!(back.stories[&sid].vertical);
            assert!(!back.stories[&other].vertical);
            // Saved again, the direction is the story's.
            let again = load(&save(&back).unwrap()).unwrap();
            assert!(again.stories[&sid].vertical && !again.stories[&other].vertical);
        }
    }

    #[test]
    fn documents_saved_before_glyph_fallback_keep_their_fallback_fonts() {
        let d = Document::new(&NewDocument::default());
        assert!(!d.settings.glyph_fallback);
        assert!(!load(&save(&d).unwrap()).unwrap().settings.glyph_fallback, "saved and read as off");
        let mut on = d.clone();
        on.settings.glyph_fallback = true;
        assert!(load(&save(&on).unwrap()).unwrap().settings.glyph_fallback);
        // Written before the setting existed: drawn from fallback fonts, as they were.
        let mut v = serde_json::to_value(&d).unwrap();
        v["settings"].as_object_mut().unwrap().remove("glyphFallback").unwrap();
        assert!(load(&serde_json::to_vec(&v).unwrap()).unwrap().settings.glyph_fallback);
    }

    #[test]
    fn rejects_garbage_and_newer_versions() {
        assert!(load(b"nope").is_err());
        let mut buf = Cursor::new(Vec::new());
        {
            let mut z = zip::ZipWriter::new(&mut buf);
            z.start_file("meta.json", zip::write::SimpleFileOptions::default()).unwrap();
            z.write_all(br#"{"version": 99}"#).unwrap();
            z.finish().unwrap();
        }
        assert!(matches!(load(&buf.into_inner()), Err(FormatError::TooNew(99))));
    }

    fn archive(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut buf = Cursor::new(Vec::new());
        {
            let mut z = zip::ZipWriter::new(&mut buf);
            for (name, body) in entries {
                z.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
                z.write_all(body).unwrap();
            }
            z.finish().unwrap();
        }
        buf.into_inner()
    }

    #[test]
    fn archive_resource_limits_cover_count_entry_and_total_sizes() {
        let bytes = archive(&[("one", b"123"), ("two", b"456")]);
        let limits = |entries, entry_bytes, expanded_bytes| ArchiveLimits { entries, entry_bytes, expanded_bytes };

        let mut z = zip::ZipArchive::new(Cursor::new(bytes.as_slice())).unwrap();
        assert!(validate_archive(&mut z, limits(1, 10, 10)).is_err());
        let mut z = zip::ZipArchive::new(Cursor::new(bytes.as_slice())).unwrap();
        assert!(validate_archive(&mut z, limits(2, 2, 10)).is_err());
        let mut z = zip::ZipArchive::new(Cursor::new(bytes.as_slice())).unwrap();
        assert!(validate_archive(&mut z, limits(2, 10, 5)).is_err());
        let mut z = zip::ZipArchive::new(Cursor::new(bytes.as_slice())).unwrap();
        assert!(validate_archive(&mut z, limits(2, 3, 6)).is_ok());
    }

    #[test]
    fn save_enforces_the_same_entry_and_expanded_limits_as_load() {
        let base = u64::try_from(MIME.len() + 8).unwrap();
        let limits = SaveLimits { entries: 4, entry_bytes: 3, expanded_bytes: base + 3, meta_bytes: 4, document_bytes: 4 };
        assert!(validate_save_parts(4, 4, [3].into_iter(), limits).is_ok());
        assert!(validate_save_parts(5, 4, std::iter::empty(), limits).is_err());
        assert!(validate_save_parts(4, 4, [4].into_iter(), limits).is_err());
        assert!(validate_save_parts(4, 4, [1, 1].into_iter(), limits).is_err());
        assert!(validate_save_parts(4, 4, [3, 3].into_iter(), SaveLimits { entries: 5, expanded_bytes: base + 5, ..limits }).is_err());
    }

    #[test]
    fn load_rejects_excessive_entry_count_in_the_zip_preflight() {
        let mut buf = Cursor::new(Vec::new());
        {
            let mut z = zip::ZipWriter::new(&mut buf);
            for i in 0..=MAX_ARCHIVE_ENTRIES {
                z.start_file(format!("entry-{i}"), zip::write::SimpleFileOptions::default()).unwrap();
            }
            z.finish().unwrap();
        }
        assert!(matches!(load(&buf.into_inner()), Err(FormatError::NotOurs(message)) if message.contains("entries")));
    }
}
