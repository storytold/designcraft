//! Seekable recovery writes until the shared codec exposes a streaming writer.
//! Its Vec-only save cannot preserve cancellation while avoiding an archive-sized RAM buffer.
use super::{FormatError, MIME, PREVIEW};
use designcraft_doc::Document;
use serde_json::json;
use std::io::Write;

const LARGE_ENTRY: usize = (u32::MAX as usize).saturating_sub(64 << 20);
struct Limits {
    asset: u64,
    total: u64,
}
impl Limits {
    const DEFAULT: Self = Self { asset: 1 << 30, total: 4 << 30 };
}
fn ext_for(mime: &str) -> &'static str {
    match mime {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/tiff" => "tif",
        "application/pdf" => "pdf",
        "application/postscript" => "eps",
        "image/svg+xml" => "svg",
        _ => "bin",
    }
}

pub(super) fn write<W: Write + std::io::Seek>(out: &mut W, doc: &Document, preview: Option<&[u8]>, version: u32) -> Result<(), FormatError> {
    write_with_limits(out, doc, preview, version, LARGE_ENTRY, Limits::DEFAULT)
}

fn write_with_limits<W: Write + std::io::Seek>(
    out: &mut W,
    doc: &Document,
    preview: Option<&[u8]>,
    version: u32,
    large_entry: usize,
    limits: Limits,
) -> Result<(), FormatError> {
    let io = |e: zip::result::ZipError| FormatError::Io(e.to_string());
    {
        let mut z = zip::ZipWriter::new(out);
        let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        let deflate = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        z.start_file("mimetype", stored).map_err(io)?;
        z.write_all(MIME.as_bytes()).map_err(|e| FormatError::Io(e.to_string()))?;
        z.start_file("meta.json", deflate).map_err(io)?;
        let meta = json!({"format": "designcraft", "version": version, "generator": concat!("DesignCraft ", env!("CARGO_PKG_VERSION"))}).to_string();
        z.write_all(meta.as_bytes()).map_err(|e| FormatError::Io(e.to_string()))?;
        if let Some(png) = preview {
            z.start_file(PREVIEW, stored).map_err(io)?;
            z.write_all(png).map_err(|e| FormatError::Io(e.to_string()))?;
        }
        z.start_file("document.json", deflate).map_err(io)?;
        let json = serde_json::to_vec_pretty(doc).map_err(|e| FormatError::Io(e.to_string()))?;
        z.write_all(&json).map_err(|e| FormatError::Io(e.to_string()))?;
        // Opening caps what deflated entries expand to, one asset and all of them together, while a
        // stored entry opens at any size. An asset deflated past those caps would save but never
        // open again (#2283), so it is stored as is instead.
        let mut deflate_left = limits.total.saturating_sub(meta.len() as u64).saturating_sub(json.len() as u64);
        for (id, a) in &doc.assets {
            if a.data.is_empty() {
                continue;
            }
            let len = a.data.len() as u64;
            // Already-compressed images are stored as-is.
            let compressed = matches!(a.mime.as_str(), "image/png" | "image/jpeg" | "image/gif" | "image/webp");
            let opts = if compressed || len > limits.asset || len > deflate_left {
                stored
            } else {
                deflate_left = deflate_left.saturating_sub(len);
                deflate
            };
            let opts = opts.large_file(a.data.len() >= large_entry);
            z.start_file(format!("assets/{}.{}", id.0, ext_for(&a.mime)), opts).map_err(io)?;
            z.write_all(&a.data).map_err(|e| FormatError::Io(e.to_string()))?;
        }
        z.finish().map_err(io)?;
    }
    Ok(())
}
