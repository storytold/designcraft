//! The native `.designcraft` format: a zip archive holding
//!
//! - `mimetype` (stored first, uncompressed): `application/vnd.designcraft+zip`
//! - `document.json`: the [`Document`] (pretty JSON, documented by the serde model in `designcraft-doc`)
//! - `assets/<id>.<ext>`: embedded placed files, byte-for-byte: one per distinct file, and only
//!   the ones the document references (see [`Document::compact_assets`])
//! - `meta.json`: format version and generator
//!
//! Older single-file JSON documents (assets as base64 in `assetData`) still open. Documents
//! from older format versions are upgraded on load so they lay out as they did.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

use std::collections::{HashMap, HashSet};
use std::hash::BuildHasher;
use std::io::{Cursor, Read, Write};
use std::sync::Arc;

use designcraft_doc::{AssetId, Document, ParaAttrs, Story};
use serde_json::{Value, json};

pub const MIME: &str = "application/vnd.designcraft+zip";
pub const VERSION: u32 = 2;

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

/// Serialize a document to `.designcraft` bytes. The file holds each distinct placed file once
/// and leaves out assets nothing references; `doc` itself keeps them (undo can bring them back).
pub fn save(doc: &Document) -> Result<Vec<u8>, FormatError> {
    let mut compact = doc.clone();
    compact.compact_assets();
    let doc = &compact;
    let io = |e: zip::result::ZipError| FormatError::Io(e.to_string());
    let mut buf = Cursor::new(Vec::new());
    {
        let mut z = zip::ZipWriter::new(&mut buf);
        let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        let deflate = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        z.start_file("mimetype", stored).map_err(io)?;
        z.write_all(MIME.as_bytes()).map_err(|e| FormatError::Io(e.to_string()))?;
        z.start_file("meta.json", deflate).map_err(io)?;
        let meta = json!({"format": "designcraft", "version": VERSION, "generator": concat!("DesignCraft ", env!("CARGO_PKG_VERSION"))});
        z.write_all(meta.to_string().as_bytes()).map_err(|e| FormatError::Io(e.to_string()))?;
        z.start_file("document.json", deflate).map_err(io)?;
        let json = serde_json::to_vec_pretty(doc).map_err(|e| FormatError::Io(e.to_string()))?;
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
    Ok(buf.into_inner())
}

/// Read `.designcraft` bytes (zip, or the legacy single JSON file).
pub fn load(bytes: &[u8]) -> Result<Document, FormatError> {
    if bytes.starts_with(b"PK") {
        return load_zip(bytes);
    }
    load_legacy_json(bytes)
}

/// Most entries a `.designcraft` archive may hold.
const MAX_ENTRIES: usize = 100_000;
/// Most bytes `meta.json` or `document.json` may expand to.
const MAX_JSON_BYTES: u64 = 256 << 20;
/// Most bytes one embedded asset may expand to.
const MAX_ASSET_BYTES: u64 = 1 << 30;
/// Most bytes all the entries read from one archive may expand to together.
const MAX_TOTAL_BYTES: u64 = 4 << 30;

/// Read entry `name` of `z`, or `None` when it is missing or unreadable. Refuses an entry that
/// expands past `limit` bytes or past what is `left` of the archive's budget, without
/// decompressing more than that: the sizes the archive claims are not trusted.
fn read_entry(z: &mut zip::ZipArchive<Cursor<&[u8]>>, name: &str, limit: u64, left: &mut u64) -> Result<Option<Vec<u8>>, FormatError> {
    let too_big = || FormatError::Io(format!("{name} is larger than the {limit}-byte limit (or the archive is larger than {MAX_TOTAL_BYTES} bytes)"));
    let Ok(f) = z.by_name(name) else { return Ok(None) };
    let limit = limit.min(*left);
    if f.size() > limit {
        return Err(too_big());
    }
    let mut v = Vec::new();
    if f.take(limit.saturating_add(1)).read_to_end(&mut v).is_err() {
        return Ok(None);
    }
    let n = v.len() as u64;
    if n > limit {
        return Err(too_big());
    }
    *left = left.saturating_sub(n);
    Ok(Some(v))
}

fn load_zip(bytes: &[u8]) -> Result<Document, FormatError> {
    let mut z = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| FormatError::NotOurs(e.to_string()))?;
    if z.len() > MAX_ENTRIES {
        return Err(FormatError::NotOurs(format!("{} entries (at most {MAX_ENTRIES})", z.len())));
    }
    let mut left = MAX_TOTAL_BYTES;
    let version = read_entry(&mut z, "meta.json", MAX_JSON_BYTES, &mut left)?
        .and_then(|meta| serde_json::from_slice::<Value>(&meta).ok())
        .and_then(|m| m.get("version").and_then(Value::as_u64))
        .map_or(1, |v| u32::try_from(v).unwrap_or(u32::MAX));
    if version > VERSION {
        return Err(FormatError::TooNew(version));
    }
    let json = read_entry(&mut z, "document.json", MAX_JSON_BYTES, &mut left)?.ok_or_else(|| FormatError::NotOurs("missing document.json".into()))?;
    let mut v: Value = serde_json::from_slice(&json).map_err(|e| FormatError::NotOurs(e.to_string()))?;
    upgrade_json(&mut v);
    let mut doc: Document = serde_json::from_value(v).map_err(|e| FormatError::NotOurs(e.to_string()))?;
    let names: Vec<String> = z.file_names().map(str::to_string).collect();
    // Files saved with one copy per placement: identical copies share one buffer as they are
    // read, so memory holds each distinct file once.
    let fingerprint = std::collections::hash_map::RandomState::new();
    let mut loaded: HashMap<u64, Vec<Arc<Vec<u8>>>> = HashMap::new();
    for name in names {
        let Some(stem) = name.strip_prefix("assets/") else { continue };
        let Some(id) = stem.split('.').next().and_then(|s| s.parse::<u64>().ok()) else { continue };
        // Entries for assets the document doesn't have are never decompressed.
        if !doc.assets.contains_key(&AssetId(id)) {
            continue;
        }
        if let (Some(data), Some(a)) = (read_entry(&mut z, &name, MAX_ASSET_BYTES, &mut left)?, doc.assets.get_mut(&AssetId(id))) {
            let same = loaded.entry(fingerprint.hash_one(data.as_slice())).or_default();
            let data = match same.iter().find(|d| ***d == data) {
                Some(d) => d.clone(),
                None => {
                    let d = Arc::new(data);
                    same.push(d.clone());
                    d
                }
            };
            Arc::make_mut(a).data = data;
        }
    }
    upgrade(&mut doc, version);
    doc.merge_duplicate_assets();
    doc.check().map_err(|e| FormatError::NotOurs(e.to_string()))?;
    Ok(doc)
}

/// Bring a document saved in format `from` up to [`VERSION`].
fn upgrade(doc: &mut Document, from: u32) {
    if from < 2 {
        // Format 1 laid out an unset Keep Lines Together mode as all lines in paragraph, and drop
        // caps without Align Left Edge.
        pin_para_default(doc, |a| &mut a.keep_all_lines, true);
        pin_para_default(doc, |a| &mut a.drop_cap_align_left, false);
    }
}

/// Pin a paragraph attribute to the value an older format resolved it to where nothing set it:
/// at the start of each paragraph style chain that leaves it unset, and on paragraphs whose style
/// is missing.
fn pin_para_default<T: Clone>(doc: &mut Document, field: fn(&mut ParaAttrs) -> &mut Option<T>, old: T) {
    let unset = |a: &ParaAttrs| field(&mut a.clone()).is_none();
    let roots: std::collections::HashSet<String> = doc
        .styles
        .paragraph
        .iter()
        .filter_map(|s| {
            let chain = doc.styles.para_chain(&s.name);
            if chain.iter().all(|c| unset(&c.para)) { chain.first().map(|c| c.name.clone()) } else { None }
        })
        .collect();
    if !roots.is_empty() {
        for s in doc.styles_mut().paragraph.iter_mut().filter(|s| roots.contains(&s.name)) {
            *field(&mut s.para) = Some(old.clone());
        }
    }
    let styles = doc.styles.clone();
    let mut pin = |st: &mut Story| {
        for p in st.paras.iter_mut().filter(|p| styles.para(&p.style).is_none()) {
            let v = field(&mut p.para);
            if v.is_none() {
                *v = Some(old.clone());
            }
        }
    };
    for st in doc.stories.values_mut() {
        let st = Arc::make_mut(st);
        st.for_each_text_mut(&mut pin);
        for n in &mut st.endnotes {
            Arc::make_mut(n).text.for_each_text_mut(&mut pin);
        }
    }
}

fn load_legacy_json(bytes: &[u8]) -> Result<Document, FormatError> {
    let mut v: Value = serde_json::from_slice(bytes).map_err(|e| FormatError::NotOurs(e.to_string()))?;
    upgrade_json(&mut v);
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
    upgrade(&mut doc, 1);
    doc.merge_duplicate_assets();
    doc.check().map_err(|e| FormatError::NotOurs(e.to_string()))?;
    Ok(doc)
}

/// Bring older documents' JSON up to the current model. Vertical Type was a text frame option
/// (`content.options.vertical`); it is the story's direction, which a story takes from its first
/// frame.
fn upgrade_json(doc: &mut Value) {
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

    fn png_asset(d: &mut Document, data: &[u8]) -> AssetId {
        let aid = AssetId(d.alloc());
        let a = Asset {
            page: 0,
            id: aid,
            name: "x.png".into(),
            mime: "image/png".into(),
            link: None,
            data: Arc::new(data.to_vec()),
            pixels: Some((1, 1)),
        };
        d.assets.insert(aid, Arc::new(a));
        aid
    }

    fn place(d: &mut Document, asset: AssetId, crop: f64) -> designcraft_doc::ItemId {
        use designcraft_doc::geom::{Affine, shapes};
        use designcraft_doc::{Content, Graphic, Item, ItemId, Shape};
        let id = ItemId(d.alloc());
        let mut it = Item::new(id, d.default_layer(), Shape::Rectangle, shapes::rectangle(Rect::new(0.0, 0.0, 50.0, 50.0)));
        it.content = Content::Graphic(Graphic {
            asset,
            size: (1.0, 1.0),
            xf: Affine::translate((crop, crop)),
            auto_fit: Default::default(),
            fit_align: 4,
            crop: [crop; 4],
        });
        d.insert_item(SpreadRef::Doc(0), it, None).unwrap();
        id
    }

    fn asset_entries(bytes: &[u8]) -> usize {
        let z = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        z.file_names().filter(|n| n.starts_with("assets/")).count()
    }

    /// Zip `d` the way files were saved with one asset per placement (nothing merged or pruned).
    fn save_uncompacted(d: &Document) -> Vec<u8> {
        let mut buf = Cursor::new(Vec::new());
        let mut z = zip::ZipWriter::new(&mut buf);
        let opts = zip::write::SimpleFileOptions::default();
        z.start_file("mimetype", opts).unwrap();
        z.write_all(MIME.as_bytes()).unwrap();
        z.start_file("document.json", opts).unwrap();
        z.write_all(&serde_json::to_vec(d).unwrap()).unwrap();
        for (id, a) in &d.assets {
            z.start_file(format!("assets/{}.png", id.0), opts).unwrap();
            z.write_all(&a.data).unwrap();
        }
        z.finish().unwrap();
        buf.into_inner()
    }

    #[test]
    fn roundtrip_with_assets() {
        let mut d = Document::new(&NewDocument { pages: 3, ..Default::default() });
        let lid = d.default_layer();
        d.add_text_frame(SpreadRef::Doc(0), Rect::new(10.0, 10.0, 200.0, 200.0), lid, "héllo\nworld", ParaFormat::default()).unwrap();
        let aid = png_asset(&mut d, &[1, 2, 3, 4]);
        place(&mut d, aid, 0.0);
        let bytes = save(&d).unwrap();
        assert!(bytes.starts_with(b"PK"));
        let back = load(&bytes).unwrap();
        assert_eq!(back.page_count(), 3);
        assert_eq!(*back.assets[&aid].data, vec![1, 2, 3, 4]);
        assert_eq!(back.stories, d.stories);
    }

    /// `doc` as format 1 wrote it.
    fn save_v1(doc: &Document) -> Vec<u8> {
        let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
        z.start_file("meta.json", zip::write::SimpleFileOptions::default()).unwrap();
        z.write_all(br#"{"format": "designcraft", "version": 1}"#).unwrap();
        z.start_file("document.json", zip::write::SimpleFileOptions::default()).unwrap();
        z.write_all(&serde_json::to_vec(doc).unwrap()).unwrap();
        z.finish().unwrap().into_inner()
    }

    fn para_style(name: &str, based_on: Option<&str>, keep_all_lines: Option<bool>) -> designcraft_doc::ParagraphStyle {
        designcraft_doc::ParagraphStyle {
            name: name.into(),
            based_on: based_on.map(str::to_string),
            next_style: None,
            para: designcraft_doc::ParaAttrs { keep_lines_together: Some(true), keep_all_lines, ..Default::default() },
            chars: Default::default(),
            shortcut: String::new(),
        }
    }

    /// Format 1 resolved an unset Keep Lines Together mode as all lines: those documents still
    /// do after loading, wherever the mode comes from. New documents keep lines at start/end.
    #[test]
    fn format_1_keep_lines_without_mode_stays_all_lines() {
        let mut d = Document::new(&NewDocument::default());
        let st = d.styles_mut();
        st.paragraph.push(para_style("Root", None, None));
        st.paragraph.push(para_style("Child", Some("Root"), None));
        st.paragraph.push(para_style("Explicit", None, Some(false)));
        st.paragraph.push(para_style("Under explicit", Some("Explicit"), None));
        st.paragraph.push(para_style("Loop A", Some("Loop B"), None));
        st.paragraph.push(para_style("Loop B", Some("Loop A"), None));
        let lid = d.default_layer();
        let names = [designcraft_doc::story::BASIC_PARAGRAPH, "Root", "Child", "Explicit", "Under explicit", "Loop A", "Loop B", "Missing"];
        let text = names.join("\n");
        let (_, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(10.0, 10.0, 200.0, 200.0), lid, &text, ParaFormat::default()).unwrap();
        {
            let s = d.story_mut(sid).unwrap();
            for (p, name) in s.paras.iter_mut().zip(names) {
                p.style = name.into();
                p.para.keep_lines_together = Some(true);
            }
            s.insert_note(1, "Note", ParaFormat { style: "Missing".into(), ..Default::default() });
        }
        let back = load(&save_v1(&d)).unwrap();
        let resolve = |f: &ParaFormat| back.styles.resolve_para(f).0;
        let story = back.story(sid).unwrap();
        for (p, name) in story.paras.iter().zip(names) {
            let pp = resolve(p);
            assert!(pp.keep_lines_together);
            assert_eq!(pp.keep_all_lines, !name.contains("xplicit"), "{name}");
        }
        assert!(resolve(&story.notes[0].text.paras[0]).keep_all_lines, "footnote with a missing style");
        assert!(back.styles.resolve_para_style("Loop A").0.keep_all_lines && back.styles.resolve_para_style("Loop B").0.keep_all_lines);
        // Pinned where the chains start; explicit and inheriting styles are left as they were.
        assert_eq!(back.styles.para("Root").unwrap().para.keep_all_lines, Some(true));
        assert_eq!(back.styles.para("Child").unwrap().para.keep_all_lines, None);
        assert_eq!(back.styles.para("Under explicit").unwrap().para.keep_all_lines, None);
        // The current format keeps the new default: at start/end of paragraph.
        let now = load(&save(&d).unwrap()).unwrap();
        let p = &now.story(sid).unwrap().paras[0];
        assert!(!now.styles.resolve_para(p).0.keep_all_lines);
        assert!(!designcraft_doc::ParaProps::default().keep_all_lines);
    }

    /// Format 1 wrote paragraph rules, text frame options and numbered lists in full, so their
    /// old defaults (black rules, auto-size from the top centre, lists continuing across stories)
    /// load as saved. New documents take InDesign's.
    #[test]
    fn format_1_rules_frames_and_lists_load_as_saved() {
        let mut d = Document::new(&NewDocument::default());
        let lid = d.default_layer();
        let old_rule = designcraft_doc::Rule { on: true, color: designcraft_doc::color::swatch::BLACK.into(), ..Default::default() };
        let (fid, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(10.0, 10.0, 200.0, 200.0), lid, "Ruled", ParaFormat::default()).unwrap();
        d.story_mut(sid).unwrap().paras[0].para.rule_above = Some(old_rule.clone());
        d.item_mut(fid).unwrap().text_frame_mut().unwrap().options.auto_size_ref = 1;
        d.settings.lists.push(designcraft_doc::NumberedList { name: "Steps".into(), continue_across_stories: true });
        let back = load(&save_v1(&d)).unwrap();
        assert_eq!(back.story(sid).unwrap().paras[0].para.rule_above, Some(old_rule));
        assert_eq!(back.item(fid).unwrap().text_frame().unwrap().options.auto_size_ref, 1);
        assert!(back.settings.lists[0].continue_across_stories);
        assert_eq!(designcraft_doc::Rule::default().color, designcraft_doc::TEXT_COLOR);
        assert_eq!(designcraft_doc::TextFrameOptions::default().auto_size_ref, 4);
        assert!(!designcraft_doc::NumberedList::default().continue_across_stories);
    }

    /// Format 1 set drop caps without Align Left Edge: those documents still do where nothing set
    /// it. New documents align the drop cap's left edge.
    #[test]
    fn format_1_drop_caps_stay_unaligned() {
        let mut d = Document::new(&NewDocument::default());
        let st = d.styles_mut();
        st.paragraph.push(para_style("Opener", None, None));
        st.paragraph.push(designcraft_doc::ParagraphStyle {
            para: designcraft_doc::ParaAttrs { drop_cap_align_left: Some(true), ..Default::default() },
            ..para_style("Aligned", None, None)
        });
        let lid = d.default_layer();
        let (_, sid) =
            d.add_text_frame(SpreadRef::Doc(0), Rect::new(10.0, 10.0, 200.0, 200.0), lid, "One\nTwo\nThree", ParaFormat::default()).unwrap();
        {
            let s = d.story_mut(sid).unwrap();
            for (p, name) in s.paras.iter_mut().zip(["Opener", "Aligned", "Missing"]) {
                p.style = name.into();
                p.para.drop_cap_lines = Some(2);
                p.para.drop_cap_chars = Some(1);
            }
        }
        let back = load(&save_v1(&d)).unwrap();
        let aligned: Vec<bool> = back.story(sid).unwrap().paras.iter().map(|p| back.styles.resolve_para(p).0.drop_cap_align_left).collect();
        assert_eq!(aligned, [false, true, false]);
        let now = load(&save(&d).unwrap()).unwrap();
        assert!(now.styles.resolve_para(&now.story(sid).unwrap().paras[0]).0.drop_cap_align_left);
    }

    #[test]
    fn saves_hold_each_used_file_once() {
        let mut d = Document::new(&NewDocument::default());
        let first = png_asset(&mut d, &[7; 64]);
        let copy = png_asset(&mut d, &[7; 64]);
        let replaced = png_asset(&mut d, &[9; 64]);
        let a = place(&mut d, first, 1.0);
        let b = place(&mut d, copy, 2.0);
        assert_eq!(d.assets.len(), 3);

        // Duplicates merge into the first asset, the unreferenced one is left out, and every
        // placement keeps its own crop and transform.
        let bytes = save(&d).unwrap();
        assert_eq!(asset_entries(&bytes), 1);
        assert_eq!(d.assets.len(), 3, "the open document keeps its assets");
        let back = load(&bytes).unwrap();
        assert_eq!(back.assets.keys().copied().collect::<Vec<_>>(), vec![first]);
        assert!(!back.assets.contains_key(&replaced));
        for (id, crop) in [(a, 1.0), (b, 2.0)] {
            let g = back.item(id).and_then(|i| i.graphic()).unwrap();
            assert_eq!((g.asset, g.crop, g.xf), (first, [crop; 4], d.item(id).unwrap().graphic().unwrap().xf));
        }
    }

    #[test]
    fn files_with_identical_bytes_at_different_paths_keep_their_paths() {
        let mut d = Document::new(&NewDocument::default());
        let ids: Vec<AssetId> = (0..3).map(|_| png_asset(&mut d, &[3; 32])).collect();
        for (id, link) in ids.iter().zip(["/p/a.png", "/q/b.png"]) {
            if let Some(a) = d.assets.get_mut(id) {
                Arc::make_mut(a).link = Some(link.into());
            }
        }
        let items: Vec<_> = ids.iter().enumerate().map(|(k, id)| place(&mut d, *id, k as f64)).collect();
        for bytes in [save(&d).unwrap(), save_uncompacted(&d)] {
            let back = load(&bytes).unwrap();
            assert_eq!(back.assets.len(), 3);
            let links: Vec<Option<&str>> = items
                .iter()
                .map(|i| back.item(*i).and_then(|i| i.graphic()).and_then(|g| back.assets.get(&g.asset)).unwrap().link.as_deref())
                .collect();
            assert_eq!(links, [Some("/p/a.png"), Some("/q/b.png"), None]);
        }
    }

    #[test]
    fn equal_size_archive_assets_preserve_content_and_share_only_identical_buffers() {
        let mut d = Document::new(&NewDocument::default());
        let mut pairs = Vec::new();
        for n in 0..128u64 {
            let mut bytes = vec![42; 1024];
            bytes[1016..].copy_from_slice(&n.to_le_bytes());
            let a = png_asset(&mut d, &bytes);
            let b = png_asset(&mut d, &bytes);
            // Distinct links must remain independently relinkable even when their bytes match.
            Arc::make_mut(d.assets.get_mut(&a).unwrap()).link = Some(format!("/a/{n}.png"));
            Arc::make_mut(d.assets.get_mut(&b).unwrap()).link = Some(format!("/b/{n}.png"));
            place(&mut d, a, 1.0);
            place(&mut d, b, 2.0);
            pairs.push((a, b));
        }
        let back = load(&save_uncompacted(&d)).unwrap();
        assert_eq!(back.assets.len(), 256);
        for (n, (a, b)) in pairs.into_iter().enumerate() {
            assert_eq!(back.assets[&a].data[1016..], (n as u64).to_le_bytes());
            assert!(Arc::ptr_eq(&back.assets[&a].data, &back.assets[&b].data));
            assert_eq!(back.assets[&a].link, d.assets[&a].link);
            assert_eq!(back.assets[&b].link, d.assets[&b].link);
        }
        assert_eq!(back.spreads, d.spreads);
    }

    #[test]
    fn older_files_with_one_asset_per_placement_open_with_one() {
        let mut d = Document::new(&NewDocument::default());
        let ids: Vec<AssetId> = (0..3).map(|_| png_asset(&mut d, &[5; 32])).collect();
        for (k, id) in ids.iter().enumerate() {
            place(&mut d, *id, k as f64);
        }
        let back = load(&save_uncompacted(&d)).unwrap();
        assert_eq!(back.assets.len(), 1);
        assert_eq!(asset_entries(&save(&back).unwrap()), 1);
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

    /// Write `len` bytes of `fill` to the current entry of `z`.
    fn write_fill(z: &mut zip::ZipWriter<Cursor<Vec<u8>>>, len: usize, fill: u8) {
        let chunk = vec![fill; 1 << 20];
        let mut left = len;
        while left > 0 {
            let n = left.min(chunk.len());
            z.write_all(&chunk[..n]).unwrap();
            left -= n;
        }
    }

    /// Make the archive claim that entry `name` expands to `size` bytes, in both its local header
    /// and its central directory record (the archive has no ZIP64 fields or data descriptors).
    fn claim_size(bytes: &mut [u8], name: &str, size: u32) {
        let mut patched = 0;
        for i in 0..bytes.len().saturating_sub(46) {
            let (size_at, len_at, name_at) = match &bytes[i..i + 4] {
                b"PK\x03\x04" => (22, 26, 30),
                b"PK\x01\x02" => (24, 28, 46),
                _ => continue,
            };
            let len = u16::from_le_bytes([bytes[i + len_at], bytes[i + len_at + 1]]) as usize;
            if bytes.get(i + name_at..i + name_at + len) == Some(name.as_bytes()) {
                bytes[i + size_at..i + size_at + 4].copy_from_slice(&size.to_le_bytes());
                patched += 1;
            }
        }
        assert_eq!(patched, 2, "{name} not found in both headers");
    }

    /// A small archive whose `document.json` expands past the 256 MiB limit while claiming to
    /// hold one byte is refused once the limit is reached, with an error that says so, instead of
    /// being read whole into memory.
    #[test]
    fn oversized_document_json_is_refused() {
        let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
        z.start_file("document.json", zip::write::SimpleFileOptions::default()).unwrap();
        write_fill(&mut z, (256 << 20) + 1, b' ');
        let mut bytes = z.finish().unwrap().into_inner();
        assert!(bytes.len() < 4 << 20, "{}", bytes.len());
        claim_size(&mut bytes, "document.json", 1);
        let Err(err) = load(&bytes) else { panic!("an oversized document.json loaded") };
        assert!(err.to_string().contains("limit"), "{err}");
    }

    /// An embedded file that expands past the 1 GiB limit (and says so) is refused too.
    #[test]
    fn oversized_asset_is_refused() {
        let mut d = Document::new(&NewDocument::default());
        let aid = png_asset(&mut d, &[1]);
        place(&mut d, aid, 0.0);
        let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
        z.start_file("document.json", zip::write::SimpleFileOptions::default()).unwrap();
        z.write_all(&serde_json::to_vec(&d).unwrap()).unwrap();
        z.start_file(format!("assets/{}.png", aid.0), zip::write::SimpleFileOptions::default()).unwrap();
        write_fill(&mut z, (1 << 30) + 1, 0);
        let Err(err) = load(&z.finish().unwrap().into_inner()) else { panic!("an oversized asset loaded") };
        assert!(err.to_string().contains("limit"), "{err}");
    }

    /// An archive with more entries than any document needs is refused before any is read.
    #[test]
    fn too_many_entries_are_refused() {
        let d = Document::new(&NewDocument::default());
        let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        z.start_file("document.json", stored).unwrap();
        z.write_all(&serde_json::to_vec(&d).unwrap()).unwrap();
        for i in 0..100_000 {
            z.start_file(format!("junk/{i}"), stored).unwrap();
        }
        let Err(err) = load(&z.finish().unwrap().into_inner()) else { panic!("an archive with 100 001 entries loaded") };
        assert!(err.to_string().contains("entries"), "{err}");
    }
}
