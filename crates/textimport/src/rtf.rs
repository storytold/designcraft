//! Adapter from craft-rtf's neutral document model to a DesignCraft story.

use designcraft_doc::{CharAttrs, CharFormat, Story, StoryId};

use crate::{ImportError, Imported};

fn char_format(fmt: &craft_rtf::CharFmt) -> CharFormat {
    let font_style = match (fmt.bold, fmt.italic) {
        (true, true) => "Bold Italic",
        (true, false) => "Bold",
        (false, true) => "Italic",
        (false, false) => "Regular",
    };
    let mut over = CharAttrs {
        font_style: (font_style != "Regular").then(|| font_style.to_string()),
        underline: fmt.underline.then_some(true),
        strikethrough: fmt.strike.then_some(true),
        size: fmt.half_points.and_then(|size| designcraft_doc::attrs::imported_type_size(f64::from(size) / 2.0)),
        ..Default::default()
    };
    if let Some(font) = &fmt.font {
        over.font_family = Some(font.clone());
    }
    CharFormat { style: designcraft_doc::story::NO_CHAR_STYLE.into(), over }
}

fn append_para(story: &mut Story, para: &craft_rtf::Para, separate: bool) {
    if separate {
        let at = story.len();
        story.insert_with(at, "\n", CharFormat::default());
    }
    if let Some(attrs) = story.paras.last_mut().map(|format| &mut format.para) {
        attrs.left_indent = Some(f64::from(para.left_indent) / 20.0);
        attrs.right_indent = Some(f64::from(para.right_indent) / 20.0);
        attrs.first_line_indent = Some(f64::from(para.first_line_indent) / 20.0);
        attrs.space_before = Some(f64::from(para.space_before) / 20.0);
        attrs.space_after = Some(f64::from(para.space_after) / 20.0);
        attrs.align = Some(match para.align {
            Some(craft_rtf::Align::Left) | None => designcraft_doc::attrs::Align::Left,
            Some(craft_rtf::Align::Center) => designcraft_doc::attrs::Align::Center,
            Some(craft_rtf::Align::Right) => designcraft_doc::attrs::Align::Right,
            Some(craft_rtf::Align::Justified) => designcraft_doc::attrs::Align::FullyJustified,
        });
    }
    for run in &para.runs {
        let mut text = String::with_capacity(run.text.len());
        for c in run.text.chars() {
            if c == '\u{FFFD}' || c == '\u{FFFF}' {
                continue;
            }
            text.push(match c {
                craft_rtf::LINE_BREAK => designcraft_doc::FORCED_LINE_BREAK,
                craft_rtf::PAGE_BREAK => designcraft_doc::PAGE_BREAK,
                craft_rtf::COLUMN_BREAK => designcraft_doc::COLUMN_BREAK,
                other => other,
            });
        }
        if !text.is_empty() {
            let at = story.len();
            story.insert_with(at, &text, char_format(&run.fmt));
        }
    }
}

pub fn import(bytes: &[u8]) -> Result<Imported, ImportError> {
    let doc = craft_rtf::read(bytes).map_err(|_| ImportError::Corrupt("not an RTF file".into()))?;
    let mut story = Story::new(StoryId(0));
    let mut paragraph_count = 0;
    for block in &doc.blocks {
        let paras: Vec<&craft_rtf::Para> = match block {
            craft_rtf::Block::Para(para) => vec![para],
            craft_rtf::Block::Table(table) => table.rows.iter().flatten().flat_map(|cell| &cell.paras).collect(),
        };
        for para in paras {
            append_para(&mut story, para, paragraph_count > 0);
            paragraph_count += 1;
        }
    }
    story.end_paragraphs_at_breaks();
    let warnings = if doc.pictures_left_out > 0 { vec!["Pictures in the RTF file were not imported.".into()] } else { vec![] };
    Ok(Imported { story, para_styles: vec![], char_styles: vec![], warnings })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_and_footer_variants_are_skipped() {
        for w in ["header", "headerl", "headerr", "headerf", "footer", "footerl", "footerr", "footerf"] {
            let rtf = format!(r"{{\rtf1\ansi {{\{w} Page head\par}}Body text\par}}");
            let text = import(rtf.as_bytes()).unwrap().story.text;
            assert_eq!(text.trim_end(), "Body text", "{w}");
        }
    }

    #[test]
    fn out_of_range_unicode_escape_is_dropped() {
        assert_eq!(import(br"{\rtf1 a\u4294967361?b}").unwrap().story.text, "ab");
        assert_eq!(import(br"{\rtf1 a\u-4294967231?b}").unwrap().story.text, "ab");
    }

    /// A short or invalid `\'hh` escape consumes only its valid hex digits.
    #[test]
    fn short_hex_escape_keeps_closing_brace() {
        let i = import(br"{\rtf1 {\b A\'4}B}").unwrap();
        assert_eq!(i.story.text, "AB");
        assert_eq!(i.story.format_after(1).over.font_style, None);
        assert_eq!(import(br"{\rtf1 a\'+fb}").unwrap().story.text, "a+fb");
    }

    #[test]
    fn imports_rtf_text_and_formatting() {
        let rtf = br"{\rtf1\ansi{\fonttbl{\f0 Times;}}{\colortbl;\red0\green0\blue0;}{\*\generator Test;}
\f0\fs24 Hello {\b bold} and {\i italic\i0  plain}\par
Caf\'e9 \u8364? price\tab tab\line next\par
}";
        let i = import(rtf).unwrap();
        let st = &i.story;
        st.check().unwrap();
        assert_eq!(st.text, "Hello bold and italic plain\nCafé € price\ttab\u{2028}next");
        let at = st.text.find("bold").unwrap();
        assert_eq!(st.format_after(at).over.font_style.as_deref(), Some("Bold"));
        assert_eq!(st.format_after(st.text.find("italic").unwrap()).over.font_style.as_deref(), Some("Italic"));
        assert_eq!(st.format_after(st.text.find("plain").unwrap()).over.font_style, None);
        assert_eq!(st.format_after(0).over.size, Some(12.0));
        assert!(!st.text.contains("Times"), "font table skipped");
    }

    /// `\fs` sizes outside the engine's 0.1–1296 pt character size range are not stored.
    #[test]
    fn font_size_out_of_range_is_not_stored() {
        let i = import(br"{\rtf1\ansi\fs0 A\fs-8 B\fs999999999 C\fs24 D}").unwrap();
        let st = &i.story;
        for ch in ["A", "B", "C"] {
            let size = st.format_after(st.text.find(ch).unwrap()).over.size;
            assert!(size.is_none_or(|s| (0.1..=1296.0).contains(&s)), "{ch}: {size:?}");
        }
        assert_eq!(st.format_after(st.text.find('D').unwrap()).over.size, Some(12.0));
    }

    /// A '-' right after a control word is text unless a digit follows it (then it is a negative
    /// parameter). It used to be swallowed.
    #[test]
    fn hyphen_after_control_word_without_digits_is_text() {
        let i = import(br"{\rtf1\ansi Rock\emdash-roll, x\tab-y, a\li-720 b}").unwrap();
        assert_eq!(i.story.text, "Rock\u{2014}-roll, x\t-y, ab");
    }

    /// A control word or control symbol is one character of the `\uN` fallback, so it is skipped
    /// like a plain character and does not eat the text after it.
    #[test]
    fn control_fallback_after_unicode_is_skipped() {
        let i = import(br"{\rtf1 \u8220\ldblquote Hi\u8221\rdblquote  x\u8212\emdash y}").unwrap();
        assert_eq!(i.story.text, "\u{201C}Hi\u{201D} x\u{2014}y");
        let i = import(br"{\rtf1 \u8220\{Hi}").unwrap();
        assert_eq!(i.story.text, "\u{201C}Hi");
        let i = import(br"{\rtf1 \u8220{\b Hi}}").unwrap();
        assert_eq!(i.story.text, "\u{201C}Hi");
    }

    /// Characters past U+FFFF are written as two `\uN` words, a UTF-16 surrogate pair (the form
    /// our own exporter and Word write). Each half was converted alone and dropped.
    #[test]
    fn surrogate_pairs_become_one_character() {
        let i = import(br"{\rtf1\ansi\uc1 A\u-10179?\u-8704?B \u-10174?\u-8265?\u30000?}").unwrap();
        assert_eq!(i.story.text, "A\u{1F600}B \u{20BB7}\u{7530}");
        // What the exporter writes comes back.
        let mut st = Story::new(StoryId(1));
        st.insert(0, "x \u{1F600} \u{20BB7}\u{7530} y");
        let doc = designcraft_doc::Document::new(&designcraft_doc::build::NewDocument::default());
        assert_eq!(import(crate::export::rtf(&doc, &st).as_bytes()).unwrap().story.text, st.text);
        // Half a pair is not a character: it is still left out.
        assert_eq!(import(br"{\rtf1 A\u-10179?B\u-8704?C}").unwrap().story.text, "ABC");
    }

    /// A `\uN` at the end of a group has no fallback character inside it. The skip count must
    /// end with the group, not eat the text after it.
    #[test]
    fn unicode_fallback_skip_ends_with_group() {
        assert_eq!(import(br"{\rtf1 {\b \u8364} AB}").unwrap().story.text, "\u{20AC} AB");
        assert_eq!(import(br"{\rtf1 {\uc2\b \u8364} ABC}").unwrap().story.text, "\u{20AC} ABC");
    }
}
