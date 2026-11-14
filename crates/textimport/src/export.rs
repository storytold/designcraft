//! Story → text files (File › Export of a story): Text Only and Rich Text Format.
//!
//! Text Only keeps paragraphs (CRLF), tabs and forced line breaks; tables become tab-separated
//! rows; markers (footnote references, anchors, index marks) are dropped and page-number markers
//! become `#`. RTF also keeps fonts, size, bold/italic, underline, strikethrough,
//! superscript/subscript, alignment, indents and paragraph spacing.

use designcraft_doc::story::{FORCED_LINE_BREAK, NEXT_PAGE_NUMBER, PAGE_NUMBER, PREV_PAGE_NUMBER, TABLE_ANCHOR};
use designcraft_doc::{Align, Document, Position, Story};

/// Characters of a run as plain text (`None`: dropped).
fn plain_char(c: char) -> Option<&'static str> {
    match c {
        PAGE_NUMBER | NEXT_PAGE_NUMBER | PREV_PAGE_NUMBER => Some("#"),
        FORCED_LINE_BREAK => Some("\u{2028}"),
        '\u{E000}'..='\u{E1FF}' => None,
        _ => Some(""),
    }
}

/// Cells of table `id` as rows of tab-separated cell text (the cells' visible text, nested
/// tables and notes left out), `nl` between rows.
fn table_text(doc: &Document, story: &Story, id: u64, nl: &str) -> String {
    let Some(t) = story.tables.get(&id) else { return String::new() };
    (0..t.nrows())
        .map(|r| {
            (0..t.ncols())
                .map(|c| t.cell(r, c).map(|x| plain_text(doc, &x.text).replace("\r\n", " ").replace('\n', " ")).unwrap_or_default())
                .collect::<Vec<_>>()
                .join("\t")
        })
        .collect::<Vec<_>>()
        .join(nl)
}

/// Text Only: the story's text as it shows (hidden conditional text and tracked deletions left
/// out), CRLF between paragraphs.
pub fn plain_text(doc: &Document, story: &Story) -> String {
    let mut out = String::with_capacity(story.text.len() + 16);
    let spans = doc.visible_spans(story);
    for (i, r) in story.para_ranges().into_iter().enumerate() {
        if i > 0 {
            out.push_str("\r\n");
        }
        for c in Document::visible_text(story, &spans, r).chars() {
            match (c, plain_char(c)) {
                (TABLE_ANCHOR, _) => {
                    if let Some(id) = story.paras.get(i).and_then(|p| p.table) {
                        out.push_str(&table_text(doc, story, id, "\r\n"));
                    }
                }
                (_, Some("")) => out.push(c),
                (_, Some("\u{2028}")) => out.push('\n'),
                (_, Some(s)) => out.push_str(s),
                (_, None) => {}
            }
        }
    }
    out
}

fn twips(pt: f64) -> i32 {
    (pt * 20.0).round().clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32
}

/// Rich Text Format, with the story's resolved formatting.
pub fn rtf(doc: &Document, story: &Story) -> String {
    let styles = &doc.styles;
    let all_runs: Vec<_> = story.runs().collect();
    let mut neutral = craft_rtf::Document::default();
    for (i, r) in story.para_ranges().into_iter().enumerate() {
        let pf = story.paras.get(i).cloned().unwrap_or_default();
        let (pp, base) = styles.resolve_para(&pf);
        let mut para = craft_rtf::Para {
            left_indent: twips(pp.left_indent),
            right_indent: twips(pp.right_indent),
            first_line_indent: twips(pp.first_line_indent),
            space_before: twips(pp.space_before),
            space_after: twips(pp.space_after),
            align: Some(match pp.align {
                Align::Center => craft_rtf::Align::Center,
                Align::Right => craft_rtf::Align::Right,
                Align::Left => craft_rtf::Align::Left,
                _ => craft_rtf::Align::Justified,
            }),
            ..Default::default()
        };
        let first = all_runs.partition_point(|(rr, _)| rr.end <= r.start);
        for (rr, f) in all_runs.iter().skip(first).take_while(|(rr, _)| rr.start < r.end) {
            let (a, b) = (rr.start.max(r.start), rr.end.min(r.end));
            if a >= b {
                continue;
            }
            let cp = styles.resolve_char(&base, f);
            if doc.conditions_hide(&cp.conditions) || cp.change == designcraft_doc::ChangeMark::Deleted {
                continue;
            }
            let style = cp.font_style.to_ascii_lowercase();
            let half_points = (cp.size * 2.0).round();
            let fmt = craft_rtf::CharFmt {
                bold: style.contains("bold") || style.contains("black") || style.contains("heavy") || style.contains("semibold"),
                italic: style.contains("italic") || style.contains("oblique"),
                underline: cp.underline,
                strike: cp.strikethrough,
                vert_align: match cp.position {
                    Position::Superscript | Position::OtSuperscript => craft_rtf::VertAlign::Superscript,
                    Position::Subscript | Position::OtSubscript => craft_rtf::VertAlign::Subscript,
                    _ => craft_rtf::VertAlign::Baseline,
                },
                half_points: (half_points.is_finite() && (1.0..=25_000.0).contains(&half_points)).then_some(half_points as u32),
                color: None,
                font: Some(cp.font_family.clone()),
            };
            let mut text = String::new();
            for c in story.text[a..b].chars() {
                match c {
                    TABLE_ANCHOR => {
                        if let Some(id) = pf.table {
                            text.push_str(&table_text(doc, story, id, "\u{2028}"));
                        }
                    }
                    PAGE_NUMBER | NEXT_PAGE_NUMBER | PREV_PAGE_NUMBER => text.push('#'),
                    ''..='' => {}
                    c => text.push(c),
                }
            }
            if !text.is_empty() {
                para.runs.push(craft_rtf::Run { text, fmt });
            }
        }
        if para.runs.is_empty() {
            para.runs.push(craft_rtf::Run {
                text: String::new(),
                fmt: craft_rtf::CharFmt {
                    half_points: Some((base.size * 2.0).round().clamp(1.0, 25_000.0) as u32),
                    font: Some(base.font_family),
                    ..Default::default()
                },
            });
        }
        neutral.blocks.push(craft_rtf::Block::Para(para));
    }
    craft_rtf::write(&neutral, "Source Serif 4")
}

#[cfg(test)]
mod tests {
    use designcraft_doc::build::NewDocument;
    use designcraft_doc::{CharAttrs, ParaAttrs, StoryId};

    use super::*;

    fn story() -> Story {
        let mut st = Story::with_text(StoryId(1), "Plain bold {x}\nCafé\u{2028}two\u{E00A}", Default::default());
        let b = st.text.find("bold").unwrap();
        st.format_chars(b..b + 4, |f| f.over = CharAttrs { font_style: Some("Bold".into()), ..Default::default() });
        st.format_paras(0..1, |p| p.para = ParaAttrs { align: Some(Align::Center), ..Default::default() });
        st
    }

    #[test]
    fn text_variable_markers_are_dropped() {
        let st = Story::with_text(StoryId(1), "Page \u{E100} of", Default::default());
        assert_eq!(plain_text(&Document::new(&NewDocument::default()), &st), "Page  of");
        let r = rtf(&Document::new(&NewDocument::default()), &st);
        assert!(!r.contains("\\u-7936") && r.contains("Page  of"), "{r}");
    }

    #[test]
    fn text_only() {
        assert_eq!(plain_text(&Document::new(&NewDocument::default()), &story()), "Plain bold {x}\r\nCafé\ntwo");
    }

    /// Text hidden by a condition set on the paragraph style, and text marked deleted, don't
    /// reach Text Only: not in the story and not in a table cell.
    #[test]
    fn text_only_leaves_out_hidden_and_deleted_text() {
        let mut doc = Document::new(&NewDocument::default());
        doc.conditions.push(designcraft_doc::Condition { name: "Secret".into(), color: [0, 0, 0], visible: false });
        doc.styles_mut().paragraph.push(designcraft_doc::ParagraphStyle {
            name: "Hidden".into(),
            based_on: None,
            next_style: None,
            para: Default::default(),
            chars: CharAttrs { conditions: Some(vec!["Secret".into()]), ..Default::default() },
            shortcut: String::new(),
        });
        let mut st = Story::with_text(StoryId(1), "shown\nhidden by style\nkept gone", Default::default());
        let h = st.text.find("hidden").unwrap();
        st.format_paras(h..h + 1, |p| p.style = "Hidden".into());
        let g = st.text.find(" gone").unwrap();
        st.format_chars(g..g + 5, |f| f.over = CharAttrs { change: Some(designcraft_doc::ChangeMark::Deleted), ..Default::default() });
        assert_eq!(plain_text(&doc, &st), "shown\r\n\r\nkept");
        let mut outer = Story::with_text(StoryId(2), &TABLE_ANCHOR.to_string(), Default::default());
        let mut t = designcraft_doc::Table::new(7, 1, 1, 0, 0, 100.0);
        t.cells[0].text = st.clone();
        outer.tables.insert(7, std::sync::Arc::new(t));
        outer.paras[0].table = Some(7);
        assert_eq!(plain_text(&doc, &outer), "shown  kept");
    }

    #[test]
    fn rtf_round_trips_through_the_importer() {
        let doc = Document::new(&NewDocument::default());
        let r = rtf(&doc, &story());
        assert!(r.starts_with("{\\rtf1") && r.contains("\\qc") && r.contains("\\{x\\}") && r.contains("Caf\\u233?"), "{r}");
        let back = crate::rtf::import(r.as_bytes()).unwrap().story;
        assert_eq!(back.text.trim_end_matches('\n'), "Plain bold {x}\nCafé\u{2028}two");
        assert_eq!(back.format_after(back.text.find("bold").unwrap()).over.font_style.as_deref(), Some("Bold"));
        assert_ne!(back.format_after(1).over.font_style.as_deref(), Some("Bold"));
    }
}
