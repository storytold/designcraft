//! Text import (File › Place of text files): plain text, Word documents (`.docx`) and RTF become
//! a frameless [`Story`] with paragraph and character styles, local bold/italic/underline/size,
//! footnotes, tables and bullets/numbering (Word), ready to flow into text frames.
//!
//! Word styles are imported by name (their font, size, bold/italic and space before/after become
//! the style definition, like InDesign's "Use Word styles" import option). RTF keeps bold,
//! italic, underline, size and paragraph breaks.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod docx;
pub mod export;
mod markdown;
mod rtf;
pub mod tagged;
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
    matches!(
        craft_file_formats::detect(name, &[]),
        Some(
            craft_file_formats::Format::PlainText
                | craft_file_formats::Format::Docx
                | craft_file_formats::Format::Rtf
                | craft_file_formats::Format::Xlsx
        )
    )
}

/// Import `bytes` (format from the file name, else sniffed).
pub fn import(name: &str, bytes: &[u8]) -> Result<Imported, ImportError> {
    if markdown::is_markdown(name) {
        return markdown::import(bytes);
    }
    match craft_file_formats::detect(name, bytes) {
        Some(craft_file_formats::Format::Xlsx) => return xlsx::import(bytes),
        Some(craft_file_formats::Format::Docx) => return docx::import(bytes),
        Some(craft_file_formats::Format::Rtf) => return rtf::import(bytes),
        Some(craft_file_formats::Format::TaggedText) => return tagged::import(bytes),
        Some(craft_file_formats::Format::LegacyDoc) => {
            return Err(ImportError::Unsupported("legacy Word .doc (save it as .docx)".into()));
        }
        _ => {}
    }
    Ok(plain(&tagged::decode(bytes)))
}

/// Plain text: line breaks become paragraphs (CRLF / CR normalised). The private-use characters
/// (U+E000–U+E1FF) that stories use as markers are dropped, since a stray one would be read as a
/// marker without its payload.
pub fn plain(text: &str) -> Imported {
    let t = text.trim_start_matches('\u{FEFF}').replace("\r\n", "\n").replace('\r', "\n");
    let t: String = t.chars().filter(|c| !('\u{E000}'..='\u{E1FF}').contains(c)).collect();
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

    #[test]
    fn plain_text_drops_story_markers() {
        let i = import("a.txt", "a\u{E009}b\u{E000}c\u{E002}d".as_bytes()).unwrap();
        assert_eq!(i.story.text, "abcd");
    }

    #[test]
    fn markdown_headings_emphasis_tables_and_quotes() {
        let md = "# Capítulo\n\nTexto com **negrito** e *itálico*.\n\n> Citação.\n\n| A | B |\n|---|---|\n| 1 | 2 |\n";
        let i = import("cap.md", md.as_bytes()).unwrap();
        assert!(!i.story.text.contains('#') && !i.story.text.contains("**") && !i.story.text.contains('|'), "{}", i.story.text);
        assert!(i.story.text.starts_with("Capítulo"));
        let names: Vec<&str> = i.para_styles.iter().map(|s| s.name.as_str()).collect();
        assert!(names.iter().any(|n| n.contains("Heading") || n.contains('1')), "{names:?}");
        let at = i.story.text.find("negrito").unwrap();
        assert_eq!(i.story.format_after(at).over.font_style.as_deref(), Some("Bold"));
        assert!(i.story.text.contains("Citação"));
        assert_eq!(i.story.tables.len(), 1, "the pipe table is a table, not text");
        let table = i.story.tables.values().next().unwrap();
        assert_eq!(table.cell(0, 1).unwrap().text.text.trim(), "B");
        assert_eq!(table.cell(1, 0).unwrap().text.text.trim(), "1");
        assert_eq!(import("cap.MARKDOWN", b"# T").unwrap().story.text.trim(), "T");
    }

    #[test]
    fn plain_text_utf16() {
        let mut le = vec![0xFF, 0xFE];
        le.extend("Hi there".encode_utf16().flat_map(u16::to_le_bytes));
        assert_eq!(import("a.txt", &le).unwrap().story.text, "Hi there");
        let mut be = vec![0xFE, 0xFF];
        be.extend("Hi there".encode_utf16().flat_map(u16::to_be_bytes));
        assert_eq!(import("a.txt", &be).unwrap().story.text, "Hi there");
    }
}
