// Copyright (c) 2026 srt8hellcat2-ui. SPDX-License-Identifier: MIT OR Apache-2.0
//! Read-only composition diagnostics, separate from import-loss warnings.

use designcraft_compose::compose_story;
use designcraft_doc::{Document, ItemId, SpreadRef};

fn location(doc: &Document, frame: ItemId) -> String {
    let Some(loc) = doc.find(frame) else { return "Page unknown".into() };
    match loc.spread {
        SpreadRef::Doc(si) => {
            let page = doc.spreads.get(si).and_then(|spread| {
                let item = doc.item_at(&loc)?;
                let center = doc.parent_xf(&loc).transform_rect_bbox(item.bounds()).center();
                spread.page_at_x(center.x)
            });
            page.map_or_else(|| "Page unknown".into(), |pi| format!("Page {}", doc.page_name(doc.first_page_of_spread(si) + pi)))
        }
        SpreadRef::Parent(pi) => doc
            .parents
            .get(pi)
            .and_then(|sp| sp.parent.as_ref())
            .map_or_else(|| "Parent page".into(), |parent| format!("Parent page {}", parent.prefix)),
    }
}

fn beginning(text: &str) -> String {
    text.split_whitespace()
        .take(8)
        .flat_map(|word| word.chars().chain(std::iter::once(' ')))
        .filter(|c| !c.is_control())
        .take(80)
        .collect::<String>()
        .trim_end()
        .to_owned()
}

pub(crate) fn notes(doc: &Document) -> Vec<String> {
    let mut notes = Vec::new();
    // Each story is visited once, independently of its number of threaded frames.
    // Unframed cell/auxiliary stories are laid out by their containing story.
    for (&sid, story) in &doc.stories {
        if story.frames.is_empty() || story.text.is_empty() {
            continue;
        }
        let composed = compose_story(doc, sid, &Default::default());
        if !composed.is_overset() {
            continue;
        }
        let frame = composed.frames.last().map(|f| f.frame).or_else(|| story.frames.last().copied());
        let page = frame.map_or_else(|| "Page unknown".into(), |frame| location(doc, frame));
        notes.push(format!(
            "{page}, text beginning \"{}\": Some text in this frame does not fit and is hidden (it is still in the file). Publisher may have used a different font, so check this frame against your original.",
            beginning(&story.text)
        ));
    }
    notes
}

#[cfg(test)]
mod tests {
    use super::*;
    use designcraft_doc::{ParaFormat, build::NewDocument};
    use designcraft_geom::Rect;

    fn doc() -> Document {
        Document::new(&NewDocument { pages: 2, facing_pages: false, ..Default::default() })
    }

    #[test]
    fn zero_height_frame_reports_hidden_text_without_changing_the_document() {
        let mut d = doc();
        let (_, sid) = d
            .add_text_frame(SpreadRef::Doc(0), Rect::new(10.0, 10.0, 210.0, 10.0), d.default_layer(), "Keep all this text", ParaFormat::default())
            .unwrap();
        let before = serde_json::to_value(&d).unwrap();
        let messages = notes(&d);
        assert_eq!(
            messages,
            [
                "Page 1, text beginning \"Keep all this text\": Some text in this frame does not fit and is hidden (it is still in the file). Publisher may have used a different font, so check this frame against your original."
            ]
        );
        assert_eq!(d.story(sid).unwrap().text, "Keep all this text");
        assert_eq!(serde_json::to_value(&d).unwrap(), before);
    }

    #[test]
    fn short_text_in_a_large_frame_has_no_note() {
        let mut d = doc();
        d.add_text_frame(SpreadRef::Doc(0), Rect::new(10.0, 10.0, 210.0, 110.0), d.default_layer(), "Hi", ParaFormat::default()).unwrap();
        assert!(notes(&d).is_empty());
    }

    #[test]
    fn threaded_overflow_is_counted_once_at_the_last_frame_page() {
        let mut d = doc();
        let rect = Rect::new(10.0, 10.0, 210.0, 10.0);
        let (a, _) = d.add_text_frame(SpreadRef::Doc(0), rect, d.default_layer(), "Threaded text", ParaFormat::default()).unwrap();
        let (b, _) = d.add_text_frame(SpreadRef::Doc(1), rect, d.default_layer(), "", ParaFormat::default()).unwrap();
        d.thread(a, b).unwrap();
        let messages = notes(&d);
        assert_eq!(messages.len(), 1);
        assert!(messages[0].starts_with("Page 2, text beginning \"Threaded text\""));
    }

    #[test]
    fn preview_is_bounded_unicode_and_has_no_line_breaks() {
        assert_eq!(beginning("  first\nsecond\tthird  "), "first second third");
        assert_eq!(beginning(&"é".repeat(1000)).chars().count(), 80);
        assert_eq!(beginning("one two three four five six seven eight nine"), "one two three four five six seven eight");
    }
}
