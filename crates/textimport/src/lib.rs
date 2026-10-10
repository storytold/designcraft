//! Text import (File › Place of text files): plain text, Word documents (`.docx`) and RTF become
//! a frameless [`Story`] with paragraph and character styles, local bold/italic/underline/size,
//! footnotes (Word) and tables (Word), ready to flow into text frames.
//!
//! Word styles are imported by name (their font, size, bold/italic and space before/after become
//! the style definition, like InDesign's "Use Word styles" import option). RTF keeps bold,
//! italic, underline, size and paragraph breaks.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod docx;
pub mod export;
mod rtf;
pub mod tagged;
mod xls;
mod xlsx;

/// Data-merge row cap (header not included). More than this is an error.
pub const DATA_MERGE_MAX_ROWS: usize = 100_000;
/// Data-merge column cap. More than this is an error.
pub const DATA_MERGE_MAX_COLS: usize = 200;

/// Worksheet rows for data merge. Empty cells inside the header width are kept.
/// `sheet` names a worksheet; `None` is the first sheet in workbook order.
/// Place keeps [`xlsx::import`], which drops empty cells.
pub fn xlsx_records(bytes: &[u8], sheet: Option<&str>) -> Result<Vec<Vec<String>>, ImportError> {
    xlsx::records(bytes, sheet)
}

/// Data-merge worksheet rows, with formula values and date text. Warnings are per cell.
/// [`xlsx_records`] and [`import`] stay on the cached value only.
pub fn xlsx_merge_records(bytes: &[u8], sheet: Option<&str>) -> Result<(Vec<Vec<String>>, Vec<String>), ImportError> {
    xlsx::merge_records(bytes, sheet)
}

/// Data-merge rows from a BIFF `.xls` workbook. `.xlsm` is not read here.
pub fn xls_records(bytes: &[u8]) -> Result<Vec<Vec<String>>, ImportError> {
    xls::records(bytes)
}

/// A small `.xls` workbook for tests. Data merge reads it with [`xls_records`].
pub use xls::{XlsCell, xls_fixture};

use designcraft_doc::{CharAttrs, ParaAttrs, Story};

/// A paragraph or character style found in the source.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ImportedStyle {
    pub name: String,
    pub based_on: Option<String>,
    pub para: ParaAttrs,
    pub chars: CharAttrs,
}

/// The imported text.
#[derive(Clone, Debug)]
pub struct Imported {
    pub story: Story,
    pub para_styles: Vec<ImportedStyle>,
    pub char_styles: Vec<ImportedStyle>,
    /// Anything the import couldn't keep (images, fields, …).
    pub warnings: Vec<String>,
}

#[derive(Debug, PartialEq)]
pub enum ImportError {
    Unsupported(String),
    Corrupt(String),
}

impl std::fmt::Display for ImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImportError::Unsupported(s) => write!(f, "unsupported text file: {s}"),
            ImportError::Corrupt(s) => write!(f, "can't read the file: {s}"),
        }
    }
}

/// Text files this crate reads, by extension.
pub fn is_text_file(name: &str) -> bool {
    let l = name.to_ascii_lowercase();
    l.ends_with(".txt") || l.ends_with(".text") || l.ends_with(".md") || l.ends_with(".docx") || l.ends_with(".rtf") || l.ends_with(".xlsx")
}

/// Import `bytes` (format from the file name, else sniffed).
pub fn import(name: &str, bytes: &[u8]) -> Result<Imported, ImportError> {
    let l = name.to_ascii_lowercase();
    if l.ends_with(".xlsx") {
        return xlsx::import(bytes);
    }
    if l.ends_with(".docx") || bytes.starts_with(b"PK\x03\x04") {
        return docx::import(bytes);
    }
    if l.ends_with(".rtf") || bytes.starts_with(b"{\\rtf") {
        return rtf::import(bytes);
    }
    if tagged::is_tagged(bytes) {
        return tagged::import(bytes);
    }
    if l.ends_with(".doc") {
        return Err(ImportError::Unsupported("legacy Word .doc (save it as .docx)".into()));
    }
    Ok(plain(&String::from_utf8_lossy(bytes)))
}

/// Plain text: line breaks become paragraphs (CRLF / CR normalised).
pub fn plain(text: &str) -> Imported {
    let t = text.trim_start_matches('\u{FEFF}').replace("\r\n", "\n").replace('\r', "\n");
    let story = Story::with_text(designcraft_doc::StoryId(0), &t, Default::default());
    Imported { story, para_styles: vec![], char_styles: vec![], warnings: vec![] }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_paragraphs() {
        let i = import("a.txt", b"\xEF\xBB\xBFOne\r\nTwo\rThree").unwrap();
        assert_eq!(i.story.text, "One\nTwo\nThree");
        assert_eq!(i.story.paras.len(), 3);
        assert!(is_text_file("Report.DOCX") && !is_text_file("a.png"));
        assert!(matches!(import("x.doc", b"\xD0\xCF"), Err(ImportError::Unsupported(_))));
    }
}
