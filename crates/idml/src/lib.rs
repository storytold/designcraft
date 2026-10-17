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
//! (page size, facing pages, bleed/slug, margins and columns, units, grids), layers, sections,
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

pub use export::{ExportOptions, export_idml, export_idml_with};
pub use import::{Imported, import_idml, import_idml_report, import_idml_report_with_previews, import_idml_with};

/// The IDML package mimetype (content of the first, stored `mimetype` entry).
pub const MIMETYPE: &str = "application/vnd.adobe.indesign-idml-package";
/// The DOM version written to every part.
pub const DOM_VERSION: &str = "16.0";

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

/// Does `bytes` look like an IDML package (a ZIP whose first, stored entry is the IDML `mimetype`)?
pub fn is_idml(bytes: &[u8]) -> bool {
    bytes.starts_with(b"PK\x03\x04") && bytes.get(30..38) == Some(b"mimetype") && {
        let extra = bytes.get(28..30).map_or(0, |e| u16::from_le_bytes([e[0], e[1]]) as usize);
        bytes.get(38 + extra..).is_some_and(|rest| rest.starts_with(MIMETYPE.as_bytes()))
    }
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Base64 with line breaks every 76 characters (as embedded image `Contents` are written).
pub(crate) fn base64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4 + data.len() / 57);
    for (i, c) in data.chunks(3).enumerate() {
        if i > 0 && i % 19 == 0 {
            out.push('\n');
        }
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        out.push(B64[(n >> 18) as usize & 63] as char);
        out.push(B64[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { B64[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { B64[n as usize & 63] as char } else { '=' });
    }
    out
}

/// Lenient base64 decoding (whitespace and unknown characters are skipped).
pub(crate) fn base64_decode(s: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let mut buf = 0u32;
    let mut bits = 0;
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

/// Re-wrap the raw preview raster InDesign embeds for an EPS placement (`EPSImage` + five
/// little-endian words: row bytes, width, height, bits per channel, channels + top-down 8-bit
/// pixels) as a 24-bit BMP, which the image decoders read. `None` for anything else, a
/// truncated raster or a size that would not fit a BMP.
pub(crate) fn eps_preview_to_bmp(b: &[u8]) -> Option<Vec<u8>> {
    let rest = b.strip_prefix(b"EPSImage")?;
    let word = |i: usize| rest.get(i * 4..i * 4 + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]) as usize);
    let (w, h, bits, ch) = (word(1)?, word(2)?, word(3)?, word(4)?);
    let px = rest.get(20..)?;
    if bits != 8 || !matches!(ch, 1 | 3 | 4) || w == 0 || h == 0 || w > 1 << 15 || h > 1 << 15 || px.len() != w.checked_mul(h)?.checked_mul(ch)? {
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
            let rgb = match ch {
                1 => [p[0]; 3],
                3 => [p[0], p[1], p[2]],
                _ => {
                    let k = 255 - p[3] as u32;
                    [0, 1, 2].map(|i| ((255 - p[i] as u32) * k / 255) as u8)
                }
            };
            out.extend_from_slice(&[rgb[2], rgb[1], rgb[0]]);
        }
        out.resize(out.len() + stride - w * 3, 0);
    }
    Some(out)
}

/// One line of text InDesign keeps for an `<EPSText>` (the live text of a placed EPS), in the
/// EPS's own coordinates.
#[derive(Debug, PartialEq)]
pub(crate) struct EpsText {
    /// PostScript name, e.g. `YuMincho-Regular`.
    pub font: String,
    pub size: f64,
    /// Left end of the baseline.
    pub x: f64,
    pub y: f64,
    pub text: String,
}

/// Read `EPSTextData`: a few header bytes, the font name, five big-endian doubles (size, two
/// reserved, size again, x, y), then the text: its length in characters, then runs of
/// `0x40, n` + n bytes (8-bit characters) or `0x80, n` + n big-endian UTF-16 units, until the
/// length is reached (`野地合板＠12` is a wide run of five characters and a narrow one of two).
pub(crate) fn parse_eps_text(b: &[u8]) -> Option<EpsText> {
    let string = |at: usize| -> Option<(String, usize)> {
        let (total, mut at) = (*b.get(at)? as usize, at + 1);
        let mut out = String::new();
        let mut count = 0;
        while count < total {
            let (&kind, &len) = (b.get(at)?, b.get(at + 1)?);
            let (len, start) = (len as usize, at + 2);
            match kind {
                0x40 => {
                    out.extend(b.get(start..start + len)?.iter().map(|&c| c as char));
                    at = start + len;
                }
                0x80 => {
                    let units: Vec<u16> = b.get(start..start + len * 2)?.as_chunks::<2>().0.iter().map(|p| u16::from_be_bytes(*p)).collect();
                    out.push_str(&String::from_utf16(&units).ok()?);
                    at = start + len * 2;
                }
                _ => return None,
            }
            if len == 0 {
                return None;
            }
            count += len;
        }
        Some((out, at))
    };
    let (font, o) = string(8)?;
    let double = |i: usize| b.get(o + i..o + i + 8).map(|s| f64::from_be_bytes([s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]]));
    let (size, x, y) = (double(0)?, double(32)?, double(40)?);
    // The text record follows after the doubles and some zero padding.
    let at = b.get(o + 48..)?.windows(4).position(|w| w == [2, 1, 1, 0])? + o + 48 + 4;
    let (text, _) = string(at)?;
    (size.is_finite() && size > 0.0 && x.is_finite() && y.is_finite() && !text.is_empty()).then_some(EpsText { font, size, x, y, text })
}

/// Sniff an image's MIME type and pixel size (PNG, JPEG, GIF, WebP, TIFF, BMP, PDF, EPS).
pub(crate) fn sniff_image(b: &[u8]) -> (Option<&'static str>, Option<(u32, u32)>) {
    if designcraft_images::is_eps(b) {
        return (Some("application/postscript"), designcraft_images::eps_pixel_size(b));
    }
    let be32 = |i: usize| b.get(i..i + 4).map(|s| u32::from_be_bytes([s[0], s[1], s[2], s[3]]));
    if b.starts_with(b"\x89PNG\r\n\x1a\n") {
        return (Some("image/png"), be32(16).zip(be32(20)));
    }
    if b.starts_with(&[0xFF, 0xD8]) {
        let mut i = 2;
        while i + 9 < b.len() {
            if b[i] != 0xFF {
                i += 1;
                continue;
            }
            let m = b[i + 1];
            let len = u16::from_be_bytes([b[i + 2], b[i + 3]]) as usize;
            if matches!(m, 0xC0..=0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF) {
                let h = u16::from_be_bytes([b[i + 5], b[i + 6]]) as u32;
                let w = u16::from_be_bytes([b[i + 7], b[i + 8]]) as u32;
                return (Some("image/jpeg"), Some((w, h)));
            }
            i += 2 + len;
        }
        return (Some("image/jpeg"), None);
    }
    if b.starts_with(b"GIF8") && b.len() >= 10 {
        return (Some("image/gif"), Some((u16::from_le_bytes([b[6], b[7]]) as u32, u16::from_le_bytes([b[8], b[9]]) as u32)));
    }
    if b.starts_with(b"RIFF") && b.get(8..12) == Some(b"WEBP") {
        return (Some("image/webp"), None);
    }
    if b.starts_with(b"II*\0") || b.starts_with(b"MM\0*") {
        return (Some("image/tiff"), None);
    }
    if b.starts_with(b"%PDF") {
        return (Some("application/pdf"), None);
    }
    if b.starts_with(b"BM") && b.len() >= 26 {
        let le = |i: usize| i32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
        return (Some("image/bmp"), Some((le(18).unsigned_abs(), le(22).unsigned_abs())));
    }
    (None, None)
}
