//! InCopy Markup (ICML) → one story and what it uses.
//!
//! An ICML file is what InDesign writes for File › Export › InCopy Document and InCopy saves
//! (other tools, such as Pandoc, write it too): a single XML document, not a package, that holds
//! inline what an IDML package spreads over its parts. Two processing instructions mark it,
//! `<?aid style="50" type="snippet" …?>` and `<?aid SnippetType="InCopyInterchange"?>`, and its
//! `Document` root holds colours and swatches, fonts, the style groups, numbering lists,
//! conditions, cross-reference formats and text variables, then the `Story` and the hyperlinks
//! that point into it. The elements are those of the IDML File Format Specification, so the IDML
//! importer reads them; this module only finds the story and leaves the pages out.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use designcraft_doc::{Document, NO_PARA_STYLE, Spread, SpreadId, StoryId, story as st};

use super::Importer;
use crate::xml::{El, parse};
use crate::{IdmlError, Result};

/// Most bytes an ICML file may hold: the limit one part of an IDML package has.
pub const MAX_ICML_BYTES: usize = 512 << 20;

/// How far into the file the ICML processing instruction is looked for.
const SNIFF_BYTES: usize = 4096;

/// A story read from an ICML file, with the document that holds its resources.
#[derive(Debug)]
pub struct ImportedStory {
    /// Holds the story and what it uses: styles, swatches, numbering lists, conditions, stroke
    /// styles, placed graphics, and the stories of text frames anchored in it. It has no pages.
    pub document: Document,
    /// The story (in `document.stories`). It has no frames.
    pub story: StoryId,
    /// The paragraph styles the file defines, in its order, without the built-in ones
    /// (`[No Paragraph Style]`, `[Basic Paragraph]`): a document keeps its own definitions of those.
    pub paragraph_styles: Vec<String>,
    /// The character styles the file defines, in its order, without `[None]`.
    pub character_styles: Vec<String>,
    /// What the import could not bring in, one line each.
    pub warnings: Vec<String>,
}

/// Does `bytes` look like an ICML file (an XML document marked `SnippetType="InCopyInterchange"`)?
pub fn is_icml(bytes: &[u8]) -> bool {
    let head = bytes.get(..bytes.len().min(SNIFF_BYTES)).unwrap_or_default();
    let head = String::from_utf8_lossy(head);
    head.trim_start_matches('\u{FEFF}').trim_start().starts_with('<') && head.contains("SnippetType=\"InCopyInterchange\"")
}

/// Read an ICML file. `read_link` reads the files that placed graphics link to (this crate never
/// touches the file system; see [`crate::import_idml_with`]).
pub fn import_icml(bytes: &[u8], read_link: &dyn Fn(&str) -> Option<Vec<u8>>) -> Result<ImportedStory> {
    check_size(bytes.len())?;
    let root = parse(bytes).map_err(|msg| IdmlError::Part { part: "ICML".into(), msg })?;
    if root.local() != "Document" {
        return Err(IdmlError::NotIdml(format!("ICML root is <{}>, not <Document>", root.name)));
    }
    let mut warnings = Vec::new();
    // Everything is inline. A reference to a package part can't be followed: there is no package.
    let top: Vec<El> = root
        .elements()
        .filter(|c| {
            let part = c.name.starts_with("idPkg:");
            if part {
                warnings.push(format!("{}: an ICML file has no package parts; it was not read", c.get("src").unwrap_or(&c.name)));
            }
            !part
        })
        .cloned()
        .collect();
    if !top.iter().any(|e| e.local() == "Story") {
        return Err(IdmlError::Invalid("the ICML file holds no story".into()));
    }
    let previews = HashMap::new();
    let mut im = Importer::new(read_link, &previews);
    im.run(&root, &top)?;
    let paragraph_styles = defined(im.para_names.values(), im.styles.paragraph.iter().map(|s| &s.name), &[st::BASIC_PARAGRAPH, NO_PARA_STYLE]);
    let character_styles = defined(im.char_names.values(), im.styles.character.iter().map(|s| &s.name), &[st::NO_CHAR_STYLE]);
    let unlinked = top.iter().filter(|h| h.local() == "Hyperlink").filter(|h| h.get("Source").is_none_or(|s| !im.xref_srcs.contains_key(s))).count();
    if unlinked > 0 {
        warnings.push(format!("{unlinked} hyperlink(s) kept as text, without their destinations"));
    }
    if im.repaired_tab_lists > 0 {
        warnings.push(format!("{} tab list(s) repaired to fit the limits", im.repaired_tab_lists));
    }
    warnings.append(&mut im.warnings);
    // The importer finishes a document only with a spread: it gets an empty one, taken away again.
    let id = SpreadId(im.alloc());
    im.spreads.push(Spread { id, pages: Vec::new(), items: Vec::new(), parent: None, allow_shuffle: true });
    let mut document = im.finish(&root)?;
    document.spreads.clear();
    document.check().map_err(|e| IdmlError::Invalid(e.to_string()))?;
    let story = main_story(&mut document, &mut warnings)?;
    Ok(ImportedStory { document, story, paragraph_styles, character_styles, warnings })
}

/// Refuse a file longer than [`MAX_ICML_BYTES`] before reading it.
fn check_size(len: usize) -> Result<()> {
    if len > MAX_ICML_BYTES {
        return Err(IdmlError::NotIdml(format!("an ICML file of {len} bytes (at most {MAX_ICML_BYTES})")));
    }
    Ok(())
}

/// The names in `all` (style order) that the file defines (`file`), without `builtins`.
fn defined<'a>(file: impl Iterator<Item = &'a String>, all: impl Iterator<Item = &'a String>, builtins: &[&str]) -> Vec<String> {
    let file: HashSet<&String> = file.collect();
    all.filter(|n| file.contains(n) && !builtins.contains(&n.as_str())).cloned().collect()
}

/// The story to place: the first one no frame holds (the others are the stories of anchored text
/// frames). Stories neither placed nor anchored in it are dropped, with a warning.
fn main_story(d: &mut Document, warnings: &mut Vec<String>) -> Result<StoryId> {
    let sid =
        d.stories.values().find(|s| s.frames.is_empty()).map(|s| s.id).ok_or_else(|| IdmlError::Invalid("the ICML file holds no story".into()))?;
    // The stories the placed one reaches through its anchored text frames, at any depth
    // (`Document::check` has refused cycles and deep nesting).
    let mut owner: BTreeMap<StoryId, StoryId> = BTreeMap::new();
    for (o, item) in d.anchored_items() {
        if let Some(tf) = item.text_frame() {
            owner.insert(tf.story, o);
        }
    }
    let keep: BTreeSet<StoryId> = d
        .stories
        .keys()
        .copied()
        .filter(|s| {
            let mut cur = *s;
            for _ in 0..=owner.len() {
                if cur == sid {
                    return true;
                }
                match owner.get(&cur) {
                    Some(o) => cur = *o,
                    None => return false,
                }
            }
            false
        })
        .collect();
    let dropped = d.stories.len() - keep.len();
    if dropped > 0 {
        warnings.push(format!("{dropped} other stor{} in the file left out: an ICML file places one story", if dropped == 1 { "y" } else { "ies" }));
        d.stories.retain(|k, _| keep.contains(k));
    }
    // Only what the kept stories use stays.
    let used = d.used_assets();
    d.assets.retain(|k, _| used.contains(k));
    Ok(sid)
}

#[cfg(test)]
mod tests;
