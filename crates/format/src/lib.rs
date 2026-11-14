//! Thin DesignCraft model adapter for the shared native document format.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::hash::BuildHasher;
use std::sync::Arc;

use designcraft_doc::{AssetData, AssetId, Document, LinkedSource, Storage};
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
    let mut out = std::io::Cursor::new(Vec::new());
    write(&mut out, doc, preview)?;
    Ok(out.into_inner())
}

/// [`save_with_preview`] into `out`, so a writer that stops on cancellation stops the write at
/// its next chunk. Linked assets are stored as their record and proxy only.
pub fn write<W: std::io::Write + std::io::Seek>(out: &mut W, doc: &Document, preview: Option<&[u8]>) -> Result<(), FormatError> {
    if let Some(id) = doc.non_finite_item() {
        return Err(FormatError::Unsaveable(format!("object {} has a position or size that is not a finite number", id.0)));
    }
    let mut compact = doc.clone();
    compact.compact_assets();
    let mut owned_assets = Vec::new();
    let mut owned_proxies = Vec::new();
    for asset in compact.assets.values() {
        let id = asset.id.0.to_string();
        if let Some(proxy) = &asset.proxy
            && let Some(bytes) = proxy.data.embedded_bytes()
        {
            owned_proxies.push((id.clone(), proxy.mime.clone(), bytes.clone()));
        }
        if asset.storage == Storage::Linked {
            continue;
        }
        let data = asset.data.load().map_err(|e| FormatError::Unsaveable(format!("image {}: {e}", asset.name)))?;
        owned_assets.push((id, asset.mime.clone(), data));
    }
    let assets = owned_assets.iter().map(|(id, mime, data)| craft_designcraft_format::Asset { id, mime, data }).collect::<Vec<_>>();
    let proxies = owned_proxies.iter().map(|(id, mime, data)| craft_designcraft_format::Asset { id, mime, data }).collect::<Vec<_>>();
    craft_designcraft_format::write_to(out, &compact, concat!("DesignCraft ", env!("CARGO_PKG_VERSION")), preview, &assets, &proxies)
}

/// Required native format version for a document. Kept for app-side compatibility; inference is
/// performed by the shared codec over the serialized neutral document schema.
#[cfg(test)]
fn required_version(doc: &Document) -> u32 {
    let Ok(value) = serde_json::to_value(doc) else { return VERSION };
    let owned_assets =
        doc.assets.values().map(|asset| (asset.id.0.to_string(), asset.mime.clone(), asset.data.load().unwrap_or_default())).collect::<Vec<_>>();
    let assets = owned_assets.iter().map(|(id, mime, data)| craft_designcraft_format::Asset { id, mime, data }).collect::<Vec<_>>();
    craft_designcraft_format::required_version(&value, &assets)
}

/// Read `.designcraft` bytes (ZIP, or legacy single JSON).
pub fn load(bytes: &[u8]) -> Result<Document, FormatError> {
    let loaded = craft_designcraft_format::load::<Document>(bytes)?;
    let mut doc = loaded.document;
    let fingerprint = std::collections::hash_map::RandomState::new();
    let mut dedup: HashMap<u64, Vec<AssetData>> = HashMap::new();
    for (raw_id, bytes) in loaded.assets {
        let Ok(id) = raw_id.parse::<u64>() else { continue };
        let asset_id = AssetId(id);
        let Some(asset) = doc.assets.get_mut(&asset_id) else { continue };
        let same = dedup.entry(fingerprint.hash_one(bytes.as_slice())).or_default();
        let data = match same.iter().find(|data| data.embedded_bytes().is_some_and(|b| **b == bytes)) {
            Some(data) => data.clone(),
            None => {
                let data = AssetData::embedded(bytes);
                same.push(data.clone());
                data
            }
        };
        Arc::make_mut(asset).data = data;
    }
    for (raw_id, bytes) in loaded.proxies {
        let Ok(id) = raw_id.parse::<u64>() else { continue };
        let Some(asset) = doc.assets.get_mut(&AssetId(id)) else { continue };
        if asset.proxy.is_some() {
            Arc::make_mut(asset).proxy.iter_mut().for_each(|proxy| proxy.data = AssetData::embedded(bytes.clone()));
        }
    }
    for asset in doc.assets.values_mut() {
        // The document's own path is unknown here; open re-resolves relative links against it.
        if asset.storage == Storage::Linked
            && let Some(source) = LinkedSource::of(asset, None)
        {
            Arc::make_mut(asset).data = AssetData::linked(source);
        }
    }
    doc.merge_duplicate_assets();
    doc.mark_differing_margins_local();
    doc.renumber_duplicate_items();
    doc.check().map_err(|error| FormatError::NotOurs(error.to_string()))?;
    Ok(doc)
}
