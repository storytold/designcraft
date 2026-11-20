//! Apple Pages import (File › Open): documents saved by Pages 5.1 (2013) and later.
//!
//! A `.pages` file is a ZIP of `Index/*.iwa` archives (Snappy-compressed protocol buffer
//! objects) and the placed files in `Data/`. Apple publishes no schema; the field numbers used
//! here were observed in documents and checked against the field numbers that community readers
//! document (libetonyek, cupertino-files). No code from them is used.
//!
//! What comes over: page size and margins, every page's placed objects (shapes, text boxes,
//! images with their masks, groups) with their position, size, rotation, fill, stroke and
//! opacity, and text with paragraph and character formatting (font, size, colour, bold, italic,
//! underline, strikethrough, capitals, baseline, tracking, alignment, indents, spacing, line
//! spacing, language). Formatting is applied as local overrides; Pages' named styles are not
//! created as DesignCraft styles. Tables, charts, shadows, gradient fills, inline objects,
//! footnotes and the flow of word-processing body text across pages are not read yet and are
//! named in the warnings.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod import;
mod package;
mod proto;
mod snappy;
mod style;
mod text;

#[cfg(test)]
mod tests;

use designcraft_doc::Document;

#[derive(Debug, thiserror::Error)]
pub enum PagesError {
    #[error("not a Pages document: {0}")]
    NotPages(String),
    #[error("unsupported Pages document: {0}")]
    Unsupported(String),
    #[error("invalid Pages document: {0}")]
    Invalid(String),
}

pub type Result<T> = std::result::Result<T, PagesError>;

/// Family and style of the installed font with a PostScript name, if there is one.
pub type FontLookup<'a> = &'a dyn Fn(&str) -> Option<(String, String)>;

/// An imported document and what didn't come over.
#[derive(Debug)]
pub struct Import {
    pub document: Document,
    pub warnings: Vec<String>,
}

/// Is `bytes` a Pages package (a ZIP with `Index/Document.iwa`)?
pub fn is_pages(bytes: &[u8]) -> bool {
    bytes.starts_with(b"PK")
        && zip::ZipArchive::new(std::io::Cursor::new(bytes)).is_ok_and(|z| z.file_names().any(|n| n == "Index/Document.iwa" || n == "Index.zip"))
}

/// Import a Pages document. `fonts` names installed fonts by PostScript name; fonts it doesn't
/// know are named from the PostScript name itself.
pub fn import(bytes: &[u8], fonts: FontLookup) -> Result<Import> {
    let pkg = package::Package::read(bytes)?;
    import::import(&pkg, fonts)
}

/// Whether a file at `name` (a path inside the package, `/`-separated) is one
/// [`import_files`] reads: pass only those.
pub fn wanted_file(name: &str) -> bool {
    package::wanted(name)
}

/// Import a document saved as a package folder, from its files named by their path inside the
/// package (`Index/Document.iwa`, `Data/photo.jpg`).
pub fn import_files(files: Vec<(String, Vec<u8>)>, fonts: FontLookup) -> Result<Import> {
    let pkg = package::Package::from_files(files)?;
    import::import(&pkg, fonts)
}
