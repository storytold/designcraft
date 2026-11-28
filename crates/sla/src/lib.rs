//! Scribus document (`.sla`) import: [`import_sla`].
//!
//! Implemented clean-room from inspecting `.sla` files (plain XML) and the PDFs Scribus exported
//! from them; no Scribus code was read. Two attribute schemas exist: Scribus 1.5 and 1.6 write
//! upper-case, partly German names (`PAGEOBJECT`, `XPOS`, `PCOLOR`, `ITEXT CH=…`, `LAYERS
//! SICHTBAR=…`), 1.7 renamed them (`PageObject`, `XPosition`, `FillColor`, `Content Chars=…`,
//! `Layers IsViewable=…`). Every lookup names the new attribute first, then the old one.
//!
//! ## Coordinates
//! Scribus places every page on one canvas (`PAGEXPOS`/`PAGEYPOS`) and every item at canvas
//! coordinates (`XPOS`/`YPOS`, its top-left before rotation), in points, y down. Pages that sit
//! side by side on one row become a DesignCraft spread; each item goes to the spread of the page
//! that owns it (`OwnPage`), or of the page it lies on. Item geometry is the `path` attribute in
//! the item's own coordinates (SVG path syntax), rotated by `ROT` degrees clockwise about its
//! top-left. Placed images are scaled from one image pixel to the point (`LOCALSCX`), whatever
//! resolution the file states.
//!
//! ## Text
//! A frame's `StoryText` holds runs (`ITEXT`/`Content`) and special characters (`tab`,
//! `breakline`, `nbspace`, `var name="pgno"`, …). A `para` element ends the paragraph before it
//! and carries that paragraph's style; `trail` carries the last paragraph's. Threaded frames
//! (`NEXTITEM`/`BACKITEM`, by `ItemID`) share the first frame's story.
//!
//! ## Supported subset
//! Page size, facing pages, bleed, margins, ruler guides, sections, layers, master pages (as
//! one-page parents), colours (CMYK, RGB, Lab, spot, registration), paragraph and character
//! styles with their parents, text frames (columns, insets, vertical alignment), threading,
//! character and paragraph overrides, tabs, drop caps, image frames (linked or embedded),
//! shapes (rectangles, ellipses, polygons, Bézier paths, lines, polylines, arcs, spirals),
//! groups, rotation, flipping, fill and stroke colours with shades, stroke weight, opacity.
//! Text shadows: DesignCraft has no character shadow, so a frame whose text is all shadowed
//! casts a hard drop shadow (Scribus' offset turned with the frame, the stroke colour), and its
//! fill and stroke move to a rectangle behind it so only the text casts it.
//!
//! ## Not imported (reported as warnings)
//! Tables, text on a path (imported as a plain text frame), inline objects, LaTeX/render
//! frames, symbols, notes, gradients and patterns (the first colour is used), image effects,
//! text shadows on part of a story.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod import;
mod text;
mod xml;

pub use import::{Imported, import_sla, import_sla_report};

#[derive(Debug, thiserror::Error)]
pub enum SlaError {
    #[error("not a Scribus document: {0}")]
    NotSla(String),
    #[error("invalid Scribus document: {0}")]
    Invalid(String),
}

pub type Result<T> = std::result::Result<T, SlaError>;

/// Largest `.sla` file imported (the largest seen in practice is a few MB; images are usually
/// linked, not embedded).
pub const MAX_BYTES: usize = 256 << 20;

/// The file extension Scribus documents use.
pub const EXTENSION: &str = "sla";

/// Is `bytes` a Scribus document? Its root element is `SCRIBUSUTF8NEW` (1.3.1 and later) or
/// `SCRIBUSUTF8`/`SCRIBUS` (older), after the XML declaration and any comments.
pub fn is_sla(bytes: &[u8]) -> bool {
    let head = bytes.get(..bytes.len().min(4096)).unwrap_or(bytes);
    let head = String::from_utf8_lossy(head);
    let mut rest = head.trim_start_matches('\u{feff}').trim_start();
    loop {
        if let Some(r) = rest.strip_prefix("<?") {
            let Some(end) = r.find("?>") else { return false };
            rest = r.get(end + 2..).unwrap_or("").trim_start();
        } else if let Some(r) = rest.strip_prefix("<!--") {
            let Some(end) = r.find("-->") else { return false };
            rest = r.get(end + 3..).unwrap_or("").trim_start();
        } else {
            break;
        }
    }
    ["<SCRIBUSUTF8NEW", "<SCRIBUSUTF8", "<SCRIBUS"]
        .iter()
        .any(|tag| rest.strip_prefix(tag).and_then(|r| r.chars().next()).is_some_and(|c| c.is_whitespace() || c == '>' || c == '/'))
}

#[cfg(test)]
mod tests;
