//! Story → text files (File › Export of a story): Text Only and Rich Text Format.
//!
//! Text Only keeps paragraphs (CRLF), tabs and forced line breaks; tables become tab-separated
//! rows; markers (footnote references, anchors, index marks) are dropped and page-number markers
//! become `#`. RTF also keeps fonts, size, bold/italic, underline, strikethrough,
//! superscript/subscript, alignment, indents and paragraph spacing.

use std::fmt::Write;

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

fn table_text(story: &Story, id: u64, nl: &str) -> String {
    let Some(t) = story.tables.get(&id) else { return String::new() };
    (0..t.nrows())
        .map(|r| (0..t.ncols()).map(|c| t.cell(r, c).map(|x| x.text.text.replace('\n', " ")).unwrap_or_default()).collect::<Vec<_>>().join("\t"))
        .collect::<Vec<_>>()
        .join(nl)
}

/// Text Only: the story's text, CRLF between paragraphs.
pub fn plain_text(story: &Story) -> String {
    // Tracked deletions aren't part of the text.
    let deleted: Vec<std::ops::Range<usize>> =
        story.runs().filter(|(_, f)| f.over.change == Some(designcraft_doc::ChangeMark::Deleted)).map(|(r, _)| r).collect();
    let mut out = String::with_capacity(story.text.len() + 16);
    for (i, r) in story.para_ranges().into_iter().enumerate() {
        if i > 0 {
            out.push_str("\r\n");
        }
        let start = r.start;
        for (ci, c) in story.text[r].char_indices() {
            if deleted.iter().any(|d| d.contains(&(start + ci))) {
                continue;
            }
            match (c, plain_char(c)) {
                (TABLE_ANCHOR, _) => {
                    if let Some(id) = story.paras.get(i).and_then(|p| p.table) {
                        out.push_str(&table_text(story, id, "\r\n"));
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

fn rtf_escape(out: &mut String, c: char) {
    match c {
        '\\' | '{' | '}' => {
            out.push('\\');
            out.push(c);
        }
        '\t' => out.push_str("\\tab "),
        FORCED_LINE_BREAK => out.push_str("\\line "),
        c if (c as u32) < 0x80 => out.push(c),
        c => {
            // \uN with a `?` fallback; N is a signed 16-bit value, astral characters as surrogates.
            let mut buf = [0u16; 2];
            for u in c.encode_utf16(&mut buf) {
                let _ = write!(out, "\\u{}?", *u as i16);
            }
        }
    }
}

fn twips(pt: f64) -> i64 {
    (pt * 20.0).round() as i64
}

/// Rich Text Format, with the story's resolved formatting.
pub fn rtf(doc: &Document, story: &Story) -> String {
    let styles = &doc.styles;
    // Resolve every run first so the font table can be written up front.
    let ranges = story.para_ranges();
    let mut paras = Vec::with_capacity(ranges.len());
    let mut fonts: Vec<String> = Vec::new();
    for (i, r) in ranges.iter().enumerate() {
        let pf = story.paras.get(i).cloned().unwrap_or_default();
        let (pp, base) = styles.resolve_para(&pf);
        let mut runs = Vec::new();
        for (rr, f) in story.runs() {
            let (a, b) = (rr.start.max(r.start), rr.end.min(r.end));
            if a >= b {
                continue;
            }
            let cp = styles.resolve_char(&base, f);
            // Hidden conditional text isn't exported.
            if doc.conditions_hide(&cp.conditions) || cp.change == designcraft_doc::ChangeMark::Deleted {
                continue;
            }
            if !fonts.contains(&cp.font_family) {
                fonts.push(cp.font_family.clone());
            }
            runs.push((a..b, cp));
        }
        if runs.is_empty() && !fonts.contains(&base.font_family) {
            fonts.push(base.font_family.clone());
        }
        paras.push((pp, base, pf.table, runs));
    }
    let mut out = String::from("{\\rtf1\\ansi\\ansicpg1252\\uc1\\deff0\n{\\fonttbl");
    for (i, f) in fonts.iter().enumerate() {
        let _ = write!(out, "{{\\f{i}\\fnil ");
        for c in f.chars() {
            rtf_escape(&mut out, c);
        }
        out.push_str(";}");
    }
    out.push_str("}\n");
    for (pp, base, table, runs) in &paras {
        let q = match pp.align {
            Align::Center => "\\qc",
            Align::Right => "\\qr",
            Align::Left => "\\ql",
            _ => "\\qj",
        };
        let _ = write!(
            out,
            "\\pard{q}\\li{}\\ri{}\\fi{}\\sb{}\\sa{}",
            twips(pp.left_indent),
            twips(pp.right_indent),
            twips(pp.first_line_indent),
            twips(pp.space_before),
            twips(pp.space_after)
        );
        if runs.is_empty() {
            let f = fonts.iter().position(|x| *x == base.font_family).unwrap_or(0);
            let _ = write!(out, "\\plain\\f{f}\\fs{} ", (base.size * 2.0).round() as i64);
        }
        for (r, cp) in runs {
            let f = fonts.iter().position(|x| *x == cp.font_family).unwrap_or(0);
            let st = cp.font_style.to_ascii_lowercase();
            let _ = write!(out, "{{\\plain\\f{f}\\fs{}", (cp.size * 2.0).round() as i64);
            if st.contains("bold") || st.contains("black") || st.contains("heavy") || st.contains("semibold") {
                out.push_str("\\b");
            }
            if st.contains("italic") || st.contains("oblique") {
                out.push_str("\\i");
            }
            if cp.underline {
                out.push_str("\\ul");
            }
            if cp.strikethrough {
                out.push_str("\\strike");
            }
            match cp.position {
                Position::Superscript | Position::OtSuperscript => out.push_str("\\super"),
                Position::Subscript | Position::OtSubscript => out.push_str("\\sub"),
                _ => {}
            }
            out.push(' ');
            for c in story.text[r.clone()].chars() {
                match c {
                    TABLE_ANCHOR => {
                        if let Some(id) = table {
                            for ch in table_text(story, *id, "\u{2028}").chars() {
                                rtf_escape(&mut out, ch);
                            }
                        }
                    }
                    PAGE_NUMBER | NEXT_PAGE_NUMBER | PREV_PAGE_NUMBER => out.push('#'),
                    '\u{E000}'..='\u{E1FF}' => {}
                    c => rtf_escape(&mut out, c),
                }
            }
            out.push('}');
        }
        out.push_str("\\par\n");
    }
    out.push('}');
    out
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
        assert_eq!(plain_text(&st), "Page  of");
        let r = rtf(&Document::new(&NewDocument::default()), &st);
        assert!(!r.contains("\\u-7936") && r.contains("Page  of"), "{r}");
    }

    #[test]
    fn text_only() {
        assert_eq!(plain_text(&story()), "Plain bold {x}\r\nCafé\ntwo");
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
