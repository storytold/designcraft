//! The native `.designcraft` document format: the document model plus its shared ZIP bundle
//! envelope (`mimetype`, metadata, JSON, preview and embedded assets).
//!
//! The model stays in DesignCraft. `craft-document-bundle` owns package IO, bounded ZIP reads,
//! compression, preview extraction and legacy inline-asset decoding.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

use std::collections::{HashMap, HashSet};
use std::hash::BuildHasher;
use std::io::{Read, Seek};
use std::sync::Arc;

use designcraft_doc::{AssetId, Document, ParaAttrs, Story};
use serde_json::Value;

pub const MIME: &str = "application/vnd.designcraft+zip";
/// Latest reader capability. Version 2 pinned paragraph defaults, version 3 adds retained EPS
/// sources, and version 4 adds placed-graphic and PDF layer options.
pub const VERSION: u32 = 4;
pub const EXTENSION: &str = "designcraft";
pub const PREVIEW: &str = "preview.png";
pub const MAX_PREVIEW_BYTES: u64 = 4 << 20;

const BUNDLE: craft_document_bundle::Spec<'static> = craft_document_bundle::Spec {
    mimetype_entry: "mimetype",
    mimetype: MIME,
    metadata_entry: "meta.json",
    document_entry: "document.json",
    preview_entry: PREVIEW,
    assets_directory: "assets",
    format_name: "designcraft",
    legacy_assets_field: Some("assetData"),
};

#[derive(Debug, thiserror::Error)]
pub enum FormatError {
    #[error("not a DesignCraft document: {0}")]
    NotOurs(String),
    #[error("document is from a newer DesignCraft (format {0}); please update")]
    TooNew(u64),
    #[error("{0}")]
    Io(String),
    #[error("document cannot be saved: {0}")]
    Unsaveable(String),
}

impl From<craft_document_bundle::Error> for FormatError {
    fn from(error: craft_document_bundle::Error) -> Self {
        match error {
            craft_document_bundle::Error::TooNew(version) => FormatError::TooNew(version),
            craft_document_bundle::Error::NotBundle(message) => FormatError::NotOurs(message),
            craft_document_bundle::Error::Read(message) | craft_document_bundle::Error::Write(message) => FormatError::Io(message),
        }
    }
}

/// Serialize a document to `.designcraft` bytes.
pub fn save(doc: &Document) -> Result<Vec<u8>, FormatError> {
    save_with_preview(doc, None)
}

/// [`save`], plus a PNG preview of the first page.
pub fn save_with_preview(doc: &Document, preview: Option<&[u8]>) -> Result<Vec<u8>, FormatError> {
    if let Some(id) = doc.non_finite_item() {
        return Err(FormatError::Unsaveable(format!("object {} has a position or size that is not a finite number", id.0)));
    }
    let mut compact = doc.clone();
    compact.compact_assets();
    let version = required_version(&compact);
    let json = serde_json::to_vec_pretty(&compact).map_err(|e| FormatError::Io(e.to_string()))?;
    let owned_assets = compact.assets.values().map(|asset| (asset.id.0.to_string(), asset.mime.clone(), asset.data.clone())).collect::<Vec<_>>();
    let assets = owned_assets.iter().map(|(id, mime, data)| craft_document_bundle::Asset { id, mime, data }).collect::<Vec<_>>();
    craft_document_bundle::write(&json, version, concat!("DesignCraft ", env!("CARGO_PKG_VERSION")), preview, &assets, BUNDLE).map_err(Into::into)
}

fn required_version(doc: &Document) -> u32 {
    if doc.has_graphic_layer_options() || doc.has_explicit_all_on_pdf_layers() {
        return 4;
    }
    if doc.assets.values().any(|asset| craft_eps::is_eps(&asset.data)) { 3 } else { 2 }
}

/// Whether `bytes` are a native DesignCraft ZIP package.
pub fn is_designcraft(bytes: &[u8]) -> bool {
    craft_document_bundle::has_document(bytes, BUNDLE)
}

/// Whether `prefix` begins with this format's stored mimetype entry.
pub fn starts_designcraft(prefix: &[u8]) -> bool {
    craft_document_bundle::starts_with_mimetype(prefix, BUNDLE)
}

/// Read the first-page PNG preview from a seekable package file, with a fixed 4 MiB cap.
pub fn read_preview<R: Read + Seek>(reader: R) -> Option<Vec<u8>> {
    craft_document_bundle::read_preview_from(reader, PREVIEW)
}

/// Read `.designcraft` bytes (ZIP, or the legacy single JSON file).
pub fn load(bytes: &[u8]) -> Result<Document, FormatError> {
    let package = craft_document_bundle::read(bytes, VERSION, BUNDLE)?;
    let mut value = package.document;
    upgrade_json(&mut value);
    let mut doc: Document = serde_json::from_value(value).map_err(|e| FormatError::NotOurs(e.to_string()))?;
    let fingerprint = std::collections::hash_map::RandomState::new();
    let mut loaded: HashMap<u64, Vec<Arc<Vec<u8>>>> = HashMap::new();
    for (raw_id, bytes) in package.assets {
        let Ok(id) = raw_id.parse::<u64>() else { continue };
        let asset_id = AssetId(id);
        let Some(asset) = doc.assets.get_mut(&asset_id) else { continue };
        let same = loaded.entry(fingerprint.hash_one(bytes.as_slice())).or_default();
        let data = match same.iter().find(|data| ***data == bytes) {
            Some(data) => data.clone(),
            None => {
                let data = Arc::new(bytes);
                same.push(data.clone());
                data
            }
        };
        Arc::make_mut(asset).data = data;
    }
    upgrade(&mut doc, u32::try_from(package.version).unwrap_or(u32::MAX));
    doc.merge_duplicate_assets();
    doc.mark_differing_margins_local();
    doc.renumber_duplicate_items();
    doc.check().map_err(|e| FormatError::NotOurs(e.to_string()))?;
    Ok(doc)
}

/// Bring a document saved in format `from` up to [`VERSION`].
fn upgrade(doc: &mut Document, from: u32) {
    if from < 2 {
        pin_para_default(doc, |attrs| &mut attrs.keep_all_lines, true);
        pin_para_default(doc, |attrs| &mut attrs.drop_cap_align_left, false);
    }
}

/// Pin an unset paragraph attribute to its old resolved value where an old format requires it.
fn pin_para_default<T: Clone>(doc: &mut Document, field: fn(&mut ParaAttrs) -> &mut Option<T>, old: T) {
    let unset = |a: &ParaAttrs| field(&mut a.clone()).is_none();
    // Every style is looked up by name: through one index, not a search of all styles each time.
    let index = doc.styles.para_index();
    let roots: std::collections::HashSet<String> = doc
        .styles
        .paragraph
        .iter()
        .filter_map(|s| {
            let chain = designcraft_doc::Styles::para_chain_by(&s.name, |n| index.get(n).copied());
            if chain.iter().all(|c| unset(&c.para)) { chain.first().map(|c| c.name.clone()) } else { None }
        })
        .collect();
    if !roots.is_empty() {
        for style in doc.styles_mut().paragraph.iter_mut().filter(|style| roots.contains(&style.name)) {
            *field(&mut style.para) = Some(old.clone());
        }
    }
    // Borrowed names: one entry per style already in memory, so no copy and no extra bound needed.
    let names: std::collections::HashSet<&str> = doc.styles.paragraph.iter().map(|s| s.name.as_str()).collect();
    let mut pin = |st: &mut Story| {
        for p in st.paras.iter_mut().filter(|p| !names.contains(p.style.as_str())) {
            let v = field(&mut p.para);
            if v.is_none() {
                *v = Some(old.clone());
            }
        }
    };
    for story in doc.stories.values_mut() {
        let story = Arc::make_mut(story);
        story.for_each_text_mut(&mut pin);
        for note in &mut story.endnotes {
            Arc::make_mut(note).text.for_each_text_mut(&mut pin);
        }
    }
}

/// Upgrade the old vertical-text representation before deserializing the current model.
fn upgrade_json(document: &mut Value) {
    let mut vertical = HashSet::new();
    vertical_frames(document, &mut vertical, 0);
    if vertical.is_empty() {
        return;
    }
    let Some(stories) = document.get_mut("stories").and_then(Value::as_object_mut) else { return };
    for story in stories.values_mut() {
        let first = story.get("frames").and_then(|frames| frames.get(0)).and_then(Value::as_u64);
        if let (Some(frame), Some(story)) = (first, story.as_object_mut())
            && vertical.contains(&frame)
            && !story.contains_key("vertical")
        {
            story.insert("vertical".into(), Value::Bool(true));
        }
    }
}

fn vertical_frames(value: &Value, out: &mut HashSet<u64>, depth: usize) {
    if depth > 256 {
        return;
    }
    match value {
        Value::Object(map) => {
            if let (Some(id), Some(content)) = (map.get("id").and_then(Value::as_u64), map.get("content"))
                && content.get("type").and_then(Value::as_str) == Some("text")
                && content.get("options").and_then(|options| options.get("vertical")).and_then(Value::as_bool) == Some(true)
            {
                out.insert(id);
            }
            for child in map.values() {
                vertical_frames(child, out, depth + 1);
            }
        }
        Value::Array(children) => {
            for child in children {
                vertical_frames(child, out, depth + 1);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests;
