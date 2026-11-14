//! App-only format-5 capabilities, alongside the shared codec's format-1 through -4 migrations.
use std::io::{Cursor, Read};

use designcraft_doc::Document;
use serde_json::Value;

use super::{FormatError, VERSION};

const SPEC: craft_document_bundle::Spec<'static> = craft_document_bundle::Spec {
    mimetype_entry: "mimetype",
    mimetype: super::MIME,
    metadata_entry: "meta.json",
    document_entry: "document.json",
    preview_entry: super::PREVIEW,
    assets_directory: "assets",
    format_name: "designcraft",
    legacy_assets_field: Some("assetData"),
};

pub(super) fn load(bytes: &[u8]) -> Result<(craft_designcraft_format::Loaded<Document>, u64), FormatError> {
    let version = declared_version(bytes)?;
    let loaded = if version <= u64::from(craft_designcraft_format::VERSION) {
        // Preserve shared schema upgrades and bounded asset loading for existing formats.
        craft_designcraft_format::load(bytes)?
    } else {
        // Format 5 already stores modern fields. Use the same shared bounded ZIP reader,
        // accepting only the extra reader capability implemented by this PR.
        let package = craft_document_bundle::read(bytes, VERSION, SPEC).map_err(FormatError::from)?;
        let document = serde_json::from_value(package.document).map_err(|error| FormatError::NotOurs(error.to_string()))?;
        craft_designcraft_format::Loaded { document, assets: package.assets }
    };
    Ok((loaded, version))
}

fn declared_version(bytes: &[u8]) -> Result<u64, FormatError> {
    if !bytes.starts_with(b"PK") {
        return Ok(1);
    }
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|error| FormatError::NotOurs(error.to_string()))?;
    let Ok(entry) = archive.by_name("meta.json") else { return Ok(1) };
    // This preliminary metadata probe must not decompress assets or trust a declared size.
    // The shared reader subsequently enforces duplicate names and aggregate archive limits.
    const MAX_METADATA_BYTES: u64 = 256 << 20;
    if entry.size() > MAX_METADATA_BYTES {
        return Err(FormatError::Io("native metadata exceeds its read limit".into()));
    }
    let mut data = Vec::new();
    entry.take(MAX_METADATA_BYTES + 1).read_to_end(&mut data).map_err(|error| FormatError::Io(error.to_string()))?;
    if data.len() as u64 > MAX_METADATA_BYTES {
        return Err(FormatError::Io("native metadata exceeds its read limit".into()));
    }
    Ok(serde_json::from_slice::<Value>(&data).ok().and_then(|value| value.get("version").and_then(Value::as_u64)).unwrap_or(1))
}

pub(super) fn pin_baselines(doc: &mut Document) {
    let mut frames = Vec::new();
    for spread in doc.spreads.iter().chain(&doc.parents) {
        for item in &spread.items {
            item.walk(&mut |item| {
                if item.text_frame().is_some() {
                    frames.push(item.id);
                }
            });
        }
    }
    for id in frames {
        if let Some(frame) = doc.item_mut(id).and_then(|item| item.text_frame_mut()) {
            frame.options.legacy_first_baseline = true;
        }
    }
}

pub(super) fn legacy_baselines(doc: &Document) -> bool {
    let mut found = false;
    for spread in doc.spreads.iter().chain(&doc.parents) {
        for item in &spread.items {
            item.walk(&mut |item| {
                found |= item.text_frame().is_some_and(|frame| frame.options.legacy_first_baseline);
            });
        }
    }
    found
}
