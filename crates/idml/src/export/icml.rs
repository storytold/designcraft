//! One story → InCopy Markup (ICML), as File › Export › InCopy Document writes it.
//!
//! The file is the IDML export's designmap with its parts written inline: the colours and
//! swatches (`Graphic.xml`), the fonts (`Fonts.xml`) and the style groups (`Styles.xml`) where the
//! designmap includes them, then the story, the stories of text frames anchored in it, and the
//! hyperlinks of its cross-references. What belongs to pages is left out: layers, sections,
//! spreads and the document preferences, but for the footnote options, whose separator the
//! footnote text starts with. See `import/icml.rs` for the format.

use designcraft_doc::StoryId;

use super::{DOM_VERSION, Document, El, Ex, ExportOptions, HashSet, document_with_pi};
use crate::xml::parse;
use crate::{IdmlError, Result};

/// The processing instructions that mark an ICML file.
fn icml_pi() -> String {
    format!(
        "<?aid style=\"50\" type=\"snippet\" readerVersion=\"6.0\" featureSet=\"513\" product=\"{DOM_VERSION}(1)\" ?>\n<?aid SnippetType=\"InCopyInterchange\"?>"
    )
}

/// Export story `story` of `doc` as an ICML file (UTF-8 XML). Images placed in the story are
/// embedded, so the file stands on its own.
pub fn export_icml(doc: &Document, story: StoryId) -> Result<Vec<u8>> {
    if !doc.stories.contains_key(&story) {
        return Err(IdmlError::Invalid(format!("no story {}", story.0)));
    }
    let opts = ExportOptions::default();
    let mut ex = Ex::new(doc, &opts);
    ex.icml(story)
}

/// `story` and the stories of the text frames anchored in it, at any depth, in that order.
fn with_anchored(d: &Document, story: StoryId) -> Vec<StoryId> {
    let mut out = vec![story];
    let mut seen: HashSet<StoryId> = out.iter().copied().collect();
    let mut i = 0;
    while let Some(&sid) = out.get(i) {
        i += 1;
        let Some(st) = d.stories.get(&sid) else { continue };
        let mut found = Vec::new();
        let mut stack: Vec<&designcraft_doc::Story> = vec![st];
        // Anchored objects sit in the story, its notes and its table cells.
        while let Some(s) = stack.pop() {
            stack.extend(s.notes.iter().chain(&s.endnotes).map(|n| &n.text));
            stack.extend(s.tables.values().flat_map(|t| t.cells.iter().map(|c| &c.text)));
            for o in &s.objects {
                o.item.walk(&mut |it| {
                    if let Some(tf) = it.text_frame() {
                        found.push(tf.story);
                    }
                });
            }
        }
        for f in found {
            if seen.insert(f) {
                out.push(f);
            }
        }
    }
    out
}

/// The children of the root element of `part` (its name, for the error), as the IDML export
/// wrote it. A part that doesn't read back is an error: the file would be missing its contents.
fn part_children(part: &str, bytes: &[u8]) -> Result<Vec<El>> {
    let root = parse(bytes).map_err(|msg| IdmlError::Part { part: part.into(), msg })?;
    Ok(root.elements().cloned().collect())
}

impl Ex<'_> {
    /// The ICML file of `story` (see the module comment for what it holds and in what order).
    fn icml(&mut self, story: StoryId) -> Result<Vec<u8>> {
        let d = self.d;
        // Stories first: they register the fonts and the cross-references the rest lists.
        let stories: Vec<El> = with_anchored(d, story).into_iter().filter_map(|sid| d.stories.get(&sid)).map(|s| self.story_el(s)).collect();
        let styles = part_children("Resources/Styles.xml", &self.styles_part())?;
        let graphic = part_children("Resources/Graphic.xml", &self.graphic_part())?;
        let fonts = part_children("Resources/Fonts.xml", &self.fonts_part())?;
        let map = part_children("designmap.xml", &self.designmap(&[], &[], &[]))?;
        let mut root = El::new("Document").attr("DOMVersion", DOM_VERSION).attr("Self", "d");
        let mut stories = Some(stories);
        for el in map {
            match el.name.as_str() {
                "idPkg:Graphic" => graphic.iter().cloned().for_each(|e| root.push(e)),
                "idPkg:Fonts" => fonts.iter().cloned().for_each(|e| root.push(e)),
                "idPkg:Styles" => {
                    styles.iter().cloned().for_each(|e| root.push(e));
                    root.push(self.footnote_option_el());
                }
                // Pages and what belongs to them.
                "Layer" | "Section" => {}
                n if n.starts_with("idPkg:") => {}
                // The stories go where the designmap lists them: after the formats their
                // cross-references apply, before the hyperlinks that point into them.
                "Hyperlink" | "ColorGroup" => {
                    stories.take().into_iter().flatten().for_each(|s| root.push(s));
                    root.push(el);
                }
                _ => root.push(el),
            }
        }
        stories.into_iter().flatten().for_each(|s| root.push(s));
        Ok(document_with_pi(&root, &icml_pi()))
    }
}

#[cfg(test)]
mod tests {
    use super::part_children;
    use crate::IdmlError;

    /// A part the export wrote that doesn't read back (here XML nested past the parser's depth
    /// limit) is an error naming the part, not an empty list that would leave what it holds out
    /// of the file without notice.
    #[test]
    fn a_part_that_does_not_read_back_is_an_error() {
        let n = crate::xml::MAX_DEPTH + 1;
        let deep = format!("{}{}", "<a>".repeat(n), "</a>".repeat(n));
        let err = part_children("Resources/Styles.xml", deep.as_bytes()).unwrap_err();
        assert!(matches!(&err, IdmlError::Part { part, .. } if part == "Resources/Styles.xml"), "{err:?}");
        assert_eq!(part_children("Resources/Styles.xml", b"<Styles><A/><B/></Styles>").unwrap().len(), 2);
    }
}
