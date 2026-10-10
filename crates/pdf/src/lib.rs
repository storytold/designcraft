//! DesignCraft PDF export (print), written with `krilla`.
//!
//! [`export_pdf`] writes one PDF page per document page (or per spread) and mirrors the renderer's
//! walk (`designcraft-render`): layers back to front, parent items first (with page-number markers
//! resolved for each page), then the spread's own items. What goes into the file:
//!
//! - **Boxes:** MediaBox = trim + bleed (+ room for printer's marks), TrimBox = the page, BleedBox =
//!   page + document bleed. Content is clipped to the bleed box.
//! - **Vectors:** fills (solid and gradient swatches), strokes with alignment, dashes, caps, joins
//!   and miter limits, corner options, opacity and blend modes (transparency groups), the simple
//!   drop shadow.
//! - **Colour:** CMYK as DeviceCMYK, RGB as DeviceRGB, Gray as DeviceGray, spot swatches (and their
//!   tints) as `/Separation` with the swatch's own values as the alternate space, `[Registration]`
//!   as `/Separation /All`.
//!   Gradients with mixed output colour spaces are converted to RGB with an export warning;
//!   homogeneous gradients keep their existing colour space (subject to PDF/A conversion).
//! - **Images:** clipped to their frame; JPEG data is passed through, PNG/GIF/WebP are embedded
//!   losslessly (or re-encoded as JPEG with [`PdfOptions::compress_images`]).
//! - **Text as real text:** the composed glyph runs are emitted with embedded, subsetted fonts and a
//!   Unicode mapping taken from the story text (ligatures, page numbers and inserted hyphens get the
//!   right characters), so text is selectable and searchable.
//! - **Marks:** crop marks, bleed marks and a page-information line, drawn in `[Registration]`.
//! - **Standards:** PDF/A-2b through krilla's validator and PDF/X-4 output intent/identification
//!   with built-in conformance checks. `designcraft-cli validate-pdf` exposes those checks for CI.
//!
//! Not yet: tagged PDF, bookmarks, hyperlinks, overprint, layers as optional content.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod export;
mod links;
mod marks;
mod text;

mod forms;
mod pdfx;
mod transitions;
pub use export::{BookletKind, BookletOptions, booklet_pairs, export_booklet, export_pdf, export_pdf_with_report, merge_pdfs, sheet_spreads};
pub use pdfx::{check_pdfx4, cmyk_group_spaces, has_rgb_groups, make_pdfx4};
pub use transitions::{add_blend_space, add_catalog_entries, add_page_entries, add_transitions};

/// PDF standard to target.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Standard {
    /// Plain PDF 1.7.
    #[default]
    None,
    /// PDF/X-4 (PDF 1.6, trim/bleed boxes, output intent and identification).
    PdfX4,
    /// PDF/A-2b (archival; validated by krilla).
    PdfA2b,
}

impl Standard {
    /// `none`, `x4` / `pdfx4` / `PDF/X-4`, `a2b` / `pdfa2b` / `PDF/A-2b`.
    pub fn parse(s: &str) -> Option<Self> {
        let k: String = s.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_lowercase();
        match k.as_str() {
            "" | "none" => Some(Self::None),
            "x4" | "pdfx4" | "pdfx42008" | "pdfx42010" => Some(Self::PdfX4),
            "a2b" | "pdfa2b" => Some(Self::PdfA2b),
            _ => None,
        }
    }
}

/// Printer's marks (Export PDF › Marks and Bleeds).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Marks {
    pub crop: bool,
    pub bleed: bool,
    /// Document title, page name, date — in the slug area under the page.
    pub page_info: bool,
    /// Line weight in points (0.25 default).
    pub weight: f64,
    /// Distance of the marks from the trim edge in points (6 default). Marks never start inside
    /// the bleed.
    pub offset: f64,
}

impl Marks {
    pub const NONE: Marks = Marks { crop: false, bleed: false, page_info: false, weight: 0.25, offset: 6.0 };
    pub const ALL: Marks = Marks { crop: true, bleed: true, page_info: true, weight: 0.25, offset: 6.0 };
    pub fn any(&self) -> bool {
        self.crop || self.bleed || self.page_info
    }
}

impl Default for Marks {
    fn default() -> Self {
        Self::NONE
    }
}

/// Export options.
#[derive(Clone, Debug)]
pub struct PdfOptions {
    /// 0-based absolute page indices to export, in order; `None` = all pages.
    pub pages: Option<Vec<usize>>,
    /// One PDF page per spread (spreads containing any chosen page) instead of per page.
    pub spreads: bool,
    /// Include the document bleed (`DocSettings::bleed`) around each page.
    pub bleed: bool,
    pub marks: Marks,
    pub standard: Standard,
    /// Re-encode non-JPEG raster images as JPEG (quality 90) when they have no transparency.
    /// Off = lossless (JPEG data is always passed through unchanged).
    pub compress_images: bool,
    /// Flate-compress content streams.
    pub compress: bool,
    /// Title for the metadata; `None` = the document title.
    pub title: Option<String>,
    pub author: Option<String>,
    /// Creation date as Unix seconds (UTC); `None` = now (native) / omitted (wasm).
    pub created: Option<i64>,
    /// Tagged PDF: stories become paragraphs in reading order, graphics figures with their alt
    /// text, parent-page items and printer's marks artifacts.
    pub tagged: bool,
    /// Interactive PDF: placed video and sound are embedded and play in Screen annotations.
    pub media: bool,
}

impl Default for PdfOptions {
    fn default() -> Self {
        Self {
            pages: None,
            spreads: false,
            bleed: false,
            marks: Marks::NONE,
            standard: Standard::None,
            compress_images: false,
            compress: true,
            title: None,
            author: None,
            created: None,
            tagged: false,
            media: false,
        }
    }
}

/// Bytes plus non-fatal warnings (features approximated or dropped).
#[derive(Clone, Debug)]
pub struct ExportReport {
    pub bytes: Vec<u8>,
    pub pages: usize,
    pub warnings: Vec<String>,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum PdfError {
    #[error("nothing to export (no pages selected)")]
    NoPages,
    #[error("page {0} does not exist")]
    BadPage(usize),
    #[error("bad page range `{0}`")]
    BadRange(String),
    #[error("PDF writer error: {0}")]
    Write(String),
}

pub type Result<T> = std::result::Result<T, PdfError>;

/// Parse a 1-based page range like `"1-3, 5, 8-"` into 0-based indices (`count` pages in the
/// document). `""` / `"all"` = every page.
pub fn parse_page_range(s: &str, count: usize) -> Result<Vec<usize>> {
    let t = s.trim();
    if t.is_empty() || t.eq_ignore_ascii_case("all") {
        return Ok((0..count).collect());
    }
    let bad = || PdfError::BadRange(s.to_string());
    let num = |v: &str| v.trim().parse::<usize>().map_err(|_| bad());
    let mut out = Vec::new();
    for part in t.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let (a, b) = match part.split_once('-') {
            Some((a, b)) => {
                let a = if a.trim().is_empty() { 1 } else { num(a)? };
                let b = if b.trim().is_empty() { count } else { num(b)? };
                (a, b)
            }
            None => {
                let v = num(part)?;
                (v, v)
            }
        };
        if a == 0 || b == 0 || a > b {
            return Err(bad());
        }
        for p in a..=b {
            if p > count {
                return Err(PdfError::BadPage(p));
            }
            if !out.contains(&(p - 1)) {
                out.push(p - 1);
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
