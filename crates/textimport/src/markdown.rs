//! Markdown (`.md`, `.markdown`): read by craft-markdown, the importer WordCraft uses, into a
//! craft-wordmodel document, then placed the same way as a Word file. Headings, lists, block
//! quotes, emphasis, code and GitHub tables become paragraph styles (Heading1…, ListParagraph,
//! Quote, …) that the place dialog's style map can point at the layout's own styles.

use crate::{ImportError, Imported, docx};

/// True for the Markdown extensions (which the format detector otherwise reads as plain text).
pub fn is_markdown(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".md") || lower.ends_with(".markdown")
}

pub fn import(bytes: &[u8]) -> Result<Imported, ImportError> {
    let text = crate::tagged::decode(bytes);
    let doc = craft_markdown::import(&text);
    Ok(docx::import_document(&doc))
}
