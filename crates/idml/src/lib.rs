//! IDML (InDesign Markup Language) interchange: [`export_idml`] and [`import_idml`].
//!
//! Implemented clean-room from the public IDML File Format Specification and from inspecting
//! IDML packages exported from synthetic documents. An IDML package is a ZIP whose first entry is
//! an uncompressed `mimetype`, followed by `designmap.xml` (the root `Document` element that
//! includes the other parts) and the `Resources/`, `MasterSpreads/`, `Spreads/` and `Stories/`
//! parts.
//!
//! ## Coordinates
//! DesignCraft spreads have their origin at the top-left of the first page (y down). IDML spread
//! coordinates have their origin at the binding spine (facing pages) or the spread centre
//! (single-sided documents), vertically centred on the pages; pages are placed by their
//! `ItemTransform` and every page item's geometry is `PathGeometry` in inner coordinates mapped by
//! `ItemTransform` (inner → parent). Export maps `xf_idml = T(-origin) · xf`; import computes the
//! page rectangles from the page transforms and maps back.
//!
//! ## Supported subset
//! Swatches (process/spot CMYK, RGB and Lab colours, tints, linear/radial gradients), paragraph,
//! character and object styles (with `BasedOn` and style groups), fonts, document preferences
//! (page size, facing pages, bleed/slug, margins and columns including dragged column positions,
//! units, grids), ruler guides, layers, sections,
//! parent (master) spreads and applied parents, spreads and pages, page items (`TextFrame`,
//! `Rectangle`, `Oval`, `Polygon`, `GraphicLine`, `Group`) with fill/stroke, corner options, text
//! frame options (with column rules), text wrap, transparency (opacity, blend mode, drop shadow,
//! feather, gradient feather, directional feather), images (embedded `Contents` or linked), stories with paragraph/character style ranges and local overrides, special
//! characters and breaks, text threading, ruler and liquid guides, and stored page liquid rules.
//!
//! ## TODO (ignored on import / not written on export)
//! - Tables, footnotes, anchored/inline objects, notes, tracked changes, hyperlinks, index and
//!   cross-reference markers, text variables, conditions, XML structure (`XML/*` parts).
//! - Page-item overrides of parent items (only the page's `OverrideList` is read),
//!   parent-of-parent page mapping beyond `AppliedMaster`, alternate layouts, liquid object constraints.
//! - Nested/GREP/line styles, bullets & numbering details (bullet glyph, number format), OpenType
//!   feature flags, paragraph borders, grid alignment, custom dashed stroke styles; column rule
//!   stroke type and overprint.
//! - Mixed inks, gradient stop opacity, gradient feather stops between the first and last, its
//!   midpoints and highlight, directional feather angle, noise, choke and shape mode, other effects
//!   than drop shadow and the feathers, stroke/fill/content transparency, EPS/PDF/AI placed graphics (imported as images when the data is available),
//!   clipping paths, compound-path fill rules.
//! - Placed PDFs: the page (`PageNumber`) and crop choice (`PDFCrop`) round-trip, and
//!   `GraphicBounds` span the chosen box. Where that box sits on the page needs the PDF parsed,
//!   which this crate doesn't do: the engine finds it after import (`file.openIdml`), and when the
//!   PDF's data isn't available the page's crop box fills `GraphicBounds`. The content crops
//!   (`CropContentVisibleLayers`, `CropContentAllLayers`) are the bounds of what the page draws:
//!   its paths, text and images, each cut to the clip in force.
//! - Unknown elements are ignored; nothing is preserved opaquely for round-trip yet.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod arabic;
mod cjk;
mod export;
mod import;
mod names;
mod ruby;
mod xml;

#[cfg(test)]
mod tests;

pub use craft_idml::is_idml;
pub use craft_idml::value::{base64_decode, base64_encode};
pub use export::{ExportOptions, export_idml, export_idml_report, export_idml_with};
pub use import::{Imported, import_idml, import_idml_report, import_idml_report_with_previews, import_idml_with};

/// The IDML package mimetype (content of the first, stored `mimetype` entry).
pub const MIMETYPE: &str = craft_idml::MIMETYPE;
/// The DOM version written to every part.
pub const DOM_VERSION: &str = craft_idml::DOM_VERSION;

#[derive(Debug, thiserror::Error)]
pub enum IdmlError {
    #[error("not an IDML package: {0}")]
    NotIdml(String),
    #[error("IDML part {part}: {msg}")]
    Part { part: String, msg: String },
    #[error("invalid IDML document: {0}")]
    Invalid(String),
}

pub type Result<T> = std::result::Result<T, IdmlError>;

/// Re-wrap the raw preview raster InDesign embeds for an EPS placement (`EPSImage` + five
/// little-endian words: row bytes, width, height, bits per channel, channels + top-down 8-bit
/// pixels) as a 24-bit BMP, which the image decoders read. Gray (1 channel) and RGB (3) only: the
/// channel order of a 4-channel raster was not checked against InDesign. `None` for anything else,
/// a truncated raster or one over the pixel budget of the EPS previews (`crates/images/src/eps.rs`).
/// The budget bounds the size of the BMP (a gray raster becomes one three times its input), so a
/// header cannot ask for an unbounded one.
pub(crate) fn eps_preview_to_bmp(b: &[u8]) -> Option<Vec<u8>> {
    /// 256 MiB of RGBA, as the EPS previews in `designcraft-images`.
    const MAX_PIXELS: usize = 256 * 1024 * 1024 / 4;
    let rest = b.strip_prefix(b"EPSImage")?;
    let word = |i: usize| rest.get(i * 4..i * 4 + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]) as usize);
    let (w, h, bits, ch) = (word(1)?, word(2)?, word(3)?, word(4)?);
    let px = rest.get(20..)?;
    if bits != 8
        || !matches!(ch, 1 | 3)
        || w == 0
        || h == 0
        || w > 1 << 15
        || h > 1 << 15
        || w.checked_mul(h)? > MAX_PIXELS
        || px.len() != w.checked_mul(h)?.checked_mul(ch)?
    {
        return None;
    }
    let stride = (w * 3).div_ceil(4) * 4;
    let size = 54 + stride * h;
    let mut out = Vec::with_capacity(size);
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&u32::try_from(size).ok()?.to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&54u32.to_le_bytes());
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&i32::try_from(w).ok()?.to_le_bytes());
    out.extend_from_slice(&i32::try_from(h).ok()?.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&24u16.to_le_bytes());
    out.extend_from_slice(&[0; 24]);
    for row in px.chunks_exact(w * ch).rev() {
        for p in row.chunks_exact(ch) {
            let rgb = if ch == 1 { [p[0]; 3] } else { [p[0], p[1], p[2]] };
            out.extend_from_slice(&[rgb[2], rgb[1], rgb[0]]);
        }
        out.resize(out.len() + stride - w * 3, 0);
    }
    Some(out)
}

/// MIME and size metadata for a placed image, delegated to the shared format registry and image
/// boundary. The application keeps only its EPS proxy pixel-size policy.
pub(crate) fn sniff_image(b: &[u8]) -> (Option<&'static str>, Option<(u32, u32)>) {
    let Some(format) = craft_file_formats::detect("", b) else { return (None, None) };
    let mime = format.mime_type();
    let pixels = if format == craft_file_formats::Format::Eps {
        designcraft_images::eps_pixel_size(b)
    } else if mime.is_some_and(|mime| mime.starts_with("image/")) {
        designcraft_images::pixel_size(b)
    } else {
        None
    };
    (mime, pixels)
}
