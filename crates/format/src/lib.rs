//! Thin DesignCraft model adapter for the shared native document format.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::hash::BuildHasher;
use std::sync::Arc;

use designcraft_doc::{AssetId, Document};
#[cfg(test)]
use serde_json::Value;
#[cfg(test)]
use std::collections::HashSet;

pub use craft_designcraft_format::{
    EXTENSION, Error as FormatError, MAX_PREVIEW_BYTES, MIME, PREVIEW, VERSION, is_designcraft, read_preview, starts_designcraft,
};

#[cfg(test)]
mod tests;

/// Serialize a DesignCraft document to `.designcraft` bytes.
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
    let owned_assets = compact.assets.values().map(|asset| (asset.id.0.to_string(), asset.mime.clone(), asset.data.clone())).collect::<Vec<_>>();
    let assets = owned_assets.iter().map(|(id, mime, data)| craft_designcraft_format::Asset { id, mime, data }).collect::<Vec<_>>();
    craft_designcraft_format::save(&compact, concat!("DesignCraft ", env!("CARGO_PKG_VERSION")), preview, &assets)
}

/// [`save_with_preview`] into `out`, in chunks, so a writer that stops on cancellation stops the
/// write at its next chunk.
pub fn write<W: std::io::Write>(out: &mut W, doc: &Document, preview: Option<&[u8]>) -> Result<(), FormatError> {
    let bytes = save_with_preview(doc, preview)?;
    for chunk in bytes.chunks(1 << 20) {
        out.write_all(chunk).map_err(|e| FormatError::Io(e.to_string()))?;
    }
    Ok(())
}

/// Required native format version for a document. Kept for app-side compatibility; inference is
/// performed by the shared codec over the serialized neutral document schema.
#[cfg(test)]
fn required_version(doc: &Document) -> u32 {
    let Ok(value) = serde_json::to_value(doc) else { return VERSION };
    let owned_assets = doc.assets.values().map(|asset| (asset.id.0.to_string(), asset.mime.clone(), asset.data.clone())).collect::<Vec<_>>();
    let assets = owned_assets.iter().map(|(id, mime, data)| craft_designcraft_format::Asset { id, mime, data }).collect::<Vec<_>>();
    craft_designcraft_format::required_version(&value, &assets)
}

/// Read `.designcraft` bytes (ZIP, or legacy single JSON).
pub fn load(bytes: &[u8]) -> Result<Document, FormatError> {
    let loaded = craft_designcraft_format::load::<Document>(bytes)?;
    let mut doc = loaded.document;
    let fingerprint = std::collections::hash_map::RandomState::new();
    let mut dedup: HashMap<u64, Vec<Arc<Vec<u8>>>> = HashMap::new();
    for (raw_id, bytes) in loaded.assets {
        let Ok(id) = raw_id.parse::<u64>() else { continue };
        let asset_id = AssetId(id);
        let Some(asset) = doc.assets.get_mut(&asset_id) else { continue };
        let same = dedup.entry(fingerprint.hash_one(bytes.as_slice())).or_default();
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
    doc.merge_duplicate_assets();
    doc.mark_differing_margins_local();
    doc.renumber_duplicate_items();
    doc.check().map_err(|error| FormatError::NotOurs(error.to_string()))?;
    Ok(doc)
}
