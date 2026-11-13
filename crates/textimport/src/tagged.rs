//! DesignCraft model adapter for the shared Tagged Text codec.

use designcraft_doc::{CharFormat, Document, Story, StoryId, attrs::CharAttrs, story::NO_CHAR_STYLE};

use crate::{ImportError, Imported, ImportedStyle};

pub fn tagged_text(doc: &Document, story: &Story) -> String {
    let mut paragraphs = Vec::new();
    for (i, range) in story.para_ranges().iter().enumerate() {
        let mut paragraph =
            craft_idml::tagged::Paragraph { style: story.paras.get(i).map_or_else(String::new, |p| p.style.clone()), runs: Vec::new() };
        for (run_range, format) in story.runs() {
            let start = run_range.start.max(range.start);
            let end = run_range.end.min(range.end);
            if start >= end
                || doc.conditions_hide(format.over.conditions.as_deref().unwrap_or(&[]))
                || format.over.change == Some(designcraft_doc::ChangeMark::Deleted)
            {
                continue;
            }
            paragraph.runs.push(craft_idml::tagged::Run {
                text: story.text[start..end].to_string(),
                format: craft_idml::tagged::CharFormat {
                    style: (format.style != NO_CHAR_STYLE).then(|| format.style.clone()),
                    font: format.over.font_family.clone(),
                    typeface: format.over.font_style.clone(),
                    size: format.over.size,
                },
            });
        }
        paragraphs.push(paragraph);
    }
    craft_idml::tagged::write(&paragraphs)
}

pub(crate) fn decode(bytes: &[u8]) -> String {
    craft_idml::tagged::decode(bytes)
}

pub fn is_tagged(bytes: &[u8]) -> bool {
    craft_idml::tagged::is_tagged(bytes)
}

pub fn import(bytes: &[u8]) -> Result<Imported, ImportError> {
    let parsed = craft_idml::tagged::parse(bytes);
    let mut story = Story::new(StoryId(0));
    for (index, paragraph) in parsed.paragraphs.iter().enumerate() {
        if index > 0 {
            let at = story.len();
            story.insert(at, "\n");
        }
        for run in &paragraph.runs {
            let format = CharFormat {
                style: run.format.style.as_deref().filter(|s| !s.is_empty()).unwrap_or(NO_CHAR_STYLE).to_string(),
                over: CharAttrs {
                    font_family: run.format.font.clone(),
                    font_style: run.format.typeface.clone(),
                    size: run.format.size.and_then(designcraft_doc::attrs::imported_type_size),
                    ..Default::default()
                },
            };
            let at = story.len();
            story.insert_with(at, &run.text, format);
        }
        if let Some(p) = story.paras.last_mut() {
            p.style = paragraph.style.clone();
        }
    }
    let styles = |names: Vec<String>| names.into_iter().map(|name| ImportedStyle { name, ..Default::default() }).collect();
    let warnings = (parsed.skipped_tags > 0).then(|| format!("{} formatting tag(s) not kept", parsed.skipped_tags)).into_iter().collect();
    Ok(Imported { story, para_styles: styles(parsed.para_styles), char_styles: styles(parsed.char_styles), warnings })
}
