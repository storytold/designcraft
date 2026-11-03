//! Scribus (`.sla`) interchange: [`import_scribus`] and [`export_scribus`].
//!
//! Implemented clean-room from the XML that Scribus 1.7.x writes for synthetic documents and from
//! opening the files we write in Scribus. A `.sla` file is one XML document whose root is
//! `<SCRIBUSUTF8NEW>` (or `<SCRIBUSUTF8>` in older files) holding a `<Document>` element with the
//! page setup, colours, styles, layers, master pages and pages, followed by the page items
//! (`<PageObject>`) and their text (`<StoryText>`).
//!
//! ## Coordinates
//! Scribus stores each page and item in one document-wide coordinate space in points, with the
//! origin at the top left and y down. DesignCraft spreads put the leftmost page at x = 0 and the
//! page tops at y = 0; an item's transform maps its local path to spread space. Import subtracts
//! the page origin and turns `Rotation` (clockwise degrees) into
//! `translate(XPosition, YPosition) · rotate(Rotation)`, the transform Scribus draws with.
//!
//! ## Supported subset
//! Page size, margins, bleed, facing pages, units, baseline grid, document information, process
//! and spot RGB/CMYK colours, character and paragraph styles, layers, master pages with their
//! items, spreads and pages, ruler guides, sections, text frames with columns/insets/vertical
//! justification and threaded stories, character and paragraph overrides, images with scale and
//! offset (linked files are read through a caller-supplied reader), rectangles, ellipses,
//! polygons, straight lines, groups, rotation, fill tint and stroke weight/style.
//!
//! ## Not imported (warnings in [`Imported::warnings`])
//! Tables, footnotes, embedded objects, text on a path, gradients and patterns on items, effects,
//! multi-state objects, named item attributes, page sets/presentation effects, and vector imports
//! other than the path stored on a frame. Unknown elements are ignored rather than preserved.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod export;
mod import;
#[cfg(test)]
mod tests;
mod xml;

pub use export::{ExportOptions, Exported, ExportedImage, export_scribus, export_scribus_with};
pub use import::{Imported, import_scribus, import_scribus_report, import_scribus_with};

/// The newest Scribus version whose file format this reader and writer target.
pub const VERSION: &str = "1.7.3";

#[derive(Debug, thiserror::Error)]
pub enum ScribusError {
    #[error("not a Scribus document: {0}")]
    NotScribus(String),
    #[error("invalid Scribus document: {0}")]
    Invalid(String),
}

pub type Result<T> = std::result::Result<T, ScribusError>;

/// Does `bytes` look like a Scribus document? Scribus writes UTF-8 XML whose root is
/// `<SCRIBUSUTF8NEW>`; older files use `<SCRIBUSUTF8>` or `<SCRIBUSUTF8OLD>`.
pub fn is_scribus(bytes: &[u8]) -> bool {
    let b = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    let head = &b[..b.len().min(4096)];
    let text = String::from_utf8_lossy(head);
    text.contains("<SCRIBUSUTF8")
}
