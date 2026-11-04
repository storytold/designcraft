//! Reflowable EPUB 3 export (File → Export → EPUB (Reflowable)).
//!
//! Stories become XHTML in reading order (page, then top-to-bottom, left-to-right of their first
//! frame); paragraph styles become CSS classes (`p.<slug>`), character styles become `span`
//! classes, local overrides become inline styles. Placed graphics become `<figure>` elements at
//! their position in the reading order. Parent-page items (folios, running heads) are skipped.
//! Tables become `<table>` elements, footnotes linked `aside`s after the paragraph that refers to
//! them, and endnotes a linked section at the end.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::Write;

use designcraft_doc::geom::{Affine, Rect};
use designcraft_doc::{
    Align, AnchorPosition, AnchoredObject, CharAttrs, CharProps, Content, Document, ENDNOTE_REF, EpubSplit, FOOTNOTE_REF, Item, ItemId, OBJECT_MARK,
    ParaProps, SpreadRef, Story, StoryId, Styles, Table, story,
};

mod chapters;
mod css;
mod fonts;
mod html;
mod markup;
mod package;
mod story_writer;

#[cfg(test)]
use chapters::{CHAPTER_MARK, link_across_chapters, nav_list, split_chapters};
#[cfg(test)]
use css::char_css;
pub use css::stylesheet;
pub use fonts::{EmbeddedFont, stylesheet_fonts};
#[cfg(test)]
use fonts::{css_weight_style, generic_family};
#[cfg(test)]
use html::base64;
pub use html::{Block, HtmlOptions, export_html, images_without_alt, reading_order};
pub use markup::{EpubError, slug};
#[cfg(test)]
use package::iso8601;
pub use package::{FixedPage, contents_title, export_epub, export_fixed_epub};
pub use story_writer::story_html;

#[derive(Clone, Debug)]
pub struct EpubOptions {
    pub title: Option<String>,
    pub author: Option<String>,
    pub language: String,
    /// Stable identifier (urn:uuid:…); derived from the title when absent.
    pub identifier: Option<String>,
    /// Cover image (PNG or JPEG), shown first and marked as the book's cover.
    pub cover: Option<Vec<u8>>,
    /// Generic CSS family (`serif`, `sans-serif`, `monospace`, `cursive`, `fantasy`) per font
    /// family, from the fonts' classification; families not in it are guessed from their name.
    pub generic_families: BTreeMap<String, String>,
    /// Font files packaged with the book and declared with `@font-face`.
    pub fonts: Vec<EmbeddedFont>,
    /// `dcterms:modified` (Unix seconds, UTC); the document's last save, else its creation, when
    /// absent.
    pub modified: Option<i64>,
    /// Title of the navigation (table of contents); from `language` when absent.
    pub toc_title: Option<String>,
    /// Split Document: where content files start; the document's setting when absent.
    pub split: Option<EpubSplit>,
}

impl Default for EpubOptions {
    fn default() -> Self {
        EpubOptions {
            title: None,
            author: None,
            language: "en".into(),
            identifier: None,
            cover: None,
            generic_families: BTreeMap::new(),
            fonts: Vec::new(),
            modified: None,
            toc_title: None,
            split: None,
        }
    }
}

impl std::fmt::Display for EpubError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for EpubError {}

#[cfg(test)]
mod tests;
