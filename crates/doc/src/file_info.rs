//! File › File Info: the document's title, author, description, keywords and copyright, saved
//! with the document. PDF export writes the title, author, description and keywords into the
//! file's metadata, and EPUB export writes the title and author.

use serde::{Deserialize, Serialize};

use crate::Document;

/// File › File Info.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FileInfo {
    /// Document Title. Empty: exports use the document's name.
    pub document_title: String,
    pub author: String,
    pub description: String,
    /// Keywords in the order they were typed, trimmed, without empty or repeated ones.
    pub keywords: Vec<String>,
    pub copyright_notice: String,
    pub copyright_info_url: String,
}

impl FileInfo {
    pub fn is_empty(&self) -> bool {
        *self == FileInfo::default()
    }
}

/// Keywords typed as one line: separated by semicolons or commas, trimmed, empty and repeated
/// ones dropped.
pub fn parse_keywords(text: &str) -> Vec<String> {
    clean_keywords(text.split([';', ',']))
}

/// Keywords typed as one line, read as [`parse_keywords`] reads them, except that a keyword of
/// `kept` that holds a semicolon or comma stays one keyword wherever it still stands whole
/// between separators.
pub fn parse_keywords_keeping(text: &str, kept: &[String]) -> Vec<String> {
    let mut whole: Vec<&str> = kept.iter().map(|k| k.trim()).filter(|k| k.contains([';', ','])).collect();
    whole.sort_by_key(|k| std::cmp::Reverse(k.len()));
    let separator = |c: char| c == ';' || c == ',';
    let mut words = Vec::new();
    let mut rest = text;
    loop {
        rest = rest.trim_start_matches(|c: char| separator(c) || c.is_whitespace());
        if rest.is_empty() {
            break;
        }
        let kept_here = whole.iter().find(|k| rest.strip_prefix(**k).is_some_and(|after| after.trim_start().chars().next().is_none_or(separator)));
        let end = kept_here.map_or_else(|| rest.find(separator).unwrap_or(rest.len()), |k| k.len());
        let (word, after) = rest.split_at_checked(end).unwrap_or((rest, ""));
        words.push(word);
        rest = after;
    }
    clean_keywords(words)
}

/// Trimmed keywords without empty or repeated ones, in their first order.
pub fn clean_keywords<'a>(words: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    words.into_iter().map(str::trim).filter(|w| !w.is_empty() && seen.insert(*w)).map(str::to_string).collect()
}

impl Document {
    /// The title exports write into a file's metadata: File Info's Document Title, or the
    /// document's name when that is empty.
    pub fn metadata_title(&self) -> &str {
        let t = self.file_info.document_title.trim();
        if t.is_empty() { &self.title } else { t }
    }

    /// File Info's Author, if any.
    pub fn metadata_author(&self) -> Option<&str> {
        let a = self.file_info.author.trim();
        (!a.is_empty()).then_some(a)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::NewDocument;

    #[test]
    fn keywords_split_at_semicolons_and_commas() {
        assert_eq!(parse_keywords(" spring; catalog,  shoes ;;spring , "), ["spring", "catalog", "shoes"]);
        assert!(parse_keywords(" ; , ").is_empty());
    }

    #[test]
    fn keywords_with_separators_stay_whole_when_another_one_changes() {
        let kept = ["Washington, DC".to_string(), "map".into(), "a; b".into()];
        assert_eq!(parse_keywords_keeping("Washington, DC; atlas", &kept), ["Washington, DC", "atlas"]);
        assert_eq!(parse_keywords_keeping("atlas, a; b ;Washington, DC", &kept), ["atlas", "a; b", "Washington, DC"]);
        // Edited, a keyword is read like any other text.
        assert_eq!(parse_keywords_keeping("Washington, D.C.; map", &kept), ["Washington", "D.C.", "map"]);
        assert_eq!(parse_keywords_keeping("Washington, DCX", &kept), ["Washington", "DCX"]);
        assert_eq!(parse_keywords_keeping(" spring; catalog,  shoes ;;spring , ", &[]), ["spring", "catalog", "shoes"]);
    }

    #[test]
    fn the_metadata_title_falls_back_to_the_document_name() {
        let mut d = Document::new(&NewDocument { title: "Flyer".into(), ..Default::default() });
        assert_eq!((d.metadata_title(), d.metadata_author()), ("Flyer", None));
        d.file_info.document_title = "  Spring Catalog ".into();
        d.file_info.author = " Ana ".into();
        assert_eq!((d.metadata_title(), d.metadata_author()), ("Spring Catalog", Some("Ana")));
        d.file_info.document_title = "   ".into();
        assert_eq!(d.metadata_title(), "Flyer");
    }

    #[test]
    fn a_document_without_file_info_saves_as_before() {
        let mut d = Document::new(&NewDocument::default());
        let v = serde_json::to_value(&d).unwrap();
        assert!(v.get("fileInfo").is_none());
        d.file_info.keywords = vec!["a".into()];
        d.file_info.copyright_notice = "© 2026 Ana".into();
        let json = serde_json::to_string(&d).unwrap();
        let back: Document = serde_json::from_str(&json).unwrap();
        assert_eq!(back.file_info, d.file_info);
    }
}
