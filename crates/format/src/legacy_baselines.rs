//! One extra reader capability: format-1 frames retain their historical first baseline.
//! Formats through 5 still use the shared codec's schema migrations and bounded bundle reader.
use std::io::{Cursor, Read, Seek, Write};

use designcraft_doc::Document;

use super::FormatError;

pub(super) const VERSION: u32 = 6;
const BUNDLE: craft_document_bundle::Spec<'static> = craft_document_bundle::Spec {
    mimetype_entry: "mimetype",
    mimetype: super::MIME,
    metadata_entry: "meta.json",
    document_entry: "document.json",
    preview_entry: super::PREVIEW,
    assets_directory: "assets",
    proxies_directory: "proxies",
    format_name: "designcraft",
    legacy_assets_field: Some("assetData"),
};

pub(super) fn contains(doc: &Document) -> bool {
    let mut found = false;
    for spread in doc.spreads.iter().chain(&doc.parents) {
        for item in &spread.items {
            item.walk(&mut |item| found |= item.text_frame().is_some_and(|frame| frame.options.legacy_first_baseline));
        }
    }
    found
}

pub(super) fn write<W: Write + Seek>(
    out: &mut W,
    doc: &Document,
    preview: Option<&[u8]>,
    assets: &[craft_designcraft_format::Asset<'_>],
    proxies: &[craft_designcraft_format::Asset<'_>],
) -> Result<(), FormatError> {
    let json = serde_json::to_vec_pretty(doc).map_err(|error| FormatError::Io(error.to_string()))?;
    craft_document_bundle::write_to(out, &json, VERSION, concat!("DesignCraft ", env!("CARGO_PKG_VERSION")), preview, assets, proxies, BUNDLE)
        .map_err(Into::into)
}

fn declared_version(bytes: &[u8]) -> Result<u64, FormatError> {
    if !bytes.starts_with(b"PK") {
        return Ok(1);
    }
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|error| FormatError::NotOurs(error.to_string()))?;
    let Ok(entry) = archive.by_name("meta.json") else { return Ok(1) };
    // A metadata probe must never decompress an unbounded entry; the shared reader then
    // validates the complete envelope, duplicate names and aggregate uncompressed size.
    const LIMIT: u64 = 1 << 20;
    if entry.size() > LIMIT {
        return Err(FormatError::NotOurs("native metadata exceeds its read limit".into()));
    }
    let mut data = Vec::new();
    entry.take(LIMIT + 1).read_to_end(&mut data).map_err(|error| FormatError::Io(error.to_string()))?;
    if data.len() as u64 > LIMIT {
        return Err(FormatError::NotOurs("native metadata exceeds its read limit".into()));
    }
    let value: serde_json::Value = serde_json::from_slice(&data).map_err(|error| FormatError::NotOurs(error.to_string()))?;
    Ok(value.get("version").and_then(serde_json::Value::as_u64).unwrap_or(1))
}

pub(super) fn load(bytes: &[u8]) -> Result<craft_designcraft_format::Loaded<Document>, FormatError> {
    let version = declared_version(bytes)?;
    let mut loaded = if version <= u64::from(craft_designcraft_format::VERSION) {
        craft_designcraft_format::load(bytes)?
    } else {
        let package = craft_document_bundle::read(bytes, VERSION, BUNDLE).map_err(FormatError::from)?;
        let document: Document = serde_json::from_value(package.document).map_err(|error| FormatError::NotOurs(error.to_string()))?;
        craft_designcraft_format::Loaded { document, assets: package.assets, proxies: package.proxies }
    };
    if version < 2 {
        let doc = &mut loaded.document;
        let mut ids = Vec::new();
        for spread in doc.spreads.iter().chain(&doc.parents) {
            for item in &spread.items {
                item.walk(&mut |item| {
                    if item.text_frame().is_some() {
                        ids.push(item.id);
                    }
                });
            }
        }
        for id in ids {
            if let Some(frame) = doc.item_mut(id).and_then(|item| item.text_frame_mut()) {
                frame.options.legacy_first_baseline = true;
            }
        }
    }
    Ok(loaded)
}
