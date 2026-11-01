//! RTF → story: text, paragraphs, line breaks, tabs, bold/italic/underline/strikethrough, size,
//! Unicode (`\uN`) and hex (`\'hh`) characters. Destinations that aren't body text (font and
//! colour tables, stylesheet, info, pictures, `\*` groups) are skipped.

use designcraft_doc::{CharAttrs, CharFormat, Story, StoryId};

use crate::{ImportError, Imported};

#[derive(Clone, Default)]
struct State {
    bold: bool,
    italic: bool,
    underline: bool,
    strike: bool,
    size: Option<f64>,
    /// Inside a destination whose text is ignored.
    skip: bool,
    /// Characters to skip after `\uN` (`\ucN`).
    uc: usize,
}

impl State {
    fn format(&self) -> CharFormat {
        let font_style = match (self.bold, self.italic) {
            (true, true) => Some("Bold Italic".to_string()),
            (true, false) => Some("Bold".into()),
            (false, true) => Some("Italic".into()),
            _ => None,
        };
        CharFormat {
            style: designcraft_doc::story::NO_CHAR_STYLE.into(),
            over: CharAttrs {
                font_style,
                underline: self.underline.then_some(true),
                strikethrough: self.strike.then_some(true),
                size: self.size,
                ..Default::default()
            },
        }
    }
}

const SKIP: &[&str] = &[
    "fonttbl",
    "colortbl",
    "stylesheet",
    "info",
    "pict",
    "header",
    "footer",
    "footnote",
    "listtable",
    "listoverridetable",
    "rsidtbl",
    "generator",
    "themedata",
    "colorschememapping",
    "latentstyles",
    "datastore",
    "xmlnstbl",
    "fldinst",
    "object",
];

struct ControlWord {
    word: String,
    parameter: Option<i64>,
    next_index: usize,
}

fn read_control_word(chars: &[char], index: usize) -> ControlWord {
    let mut word_end = index + 1;
    while word_end < chars.len() && chars[word_end].is_ascii_alphabetic() {
        word_end += 1;
    }
    let word = chars[index + 1..word_end].iter().collect();
    let mut next_index = word_end;
    // A '-' only starts the parameter when a digit follows; otherwise it is text.
    let negative = chars.get(next_index) == Some(&'-') && chars.get(next_index + 1).is_some_and(char::is_ascii_digit);
    let has_parameter = negative || chars.get(next_index).is_some_and(char::is_ascii_digit);
    if has_parameter {
        next_index += 1;
        while next_index < chars.len() && chars[next_index].is_ascii_digit() {
            next_index += 1;
        }
    }
    let parameter = chars[word_end..next_index].iter().collect::<String>().parse().ok();
    if chars.get(next_index) == Some(&' ') {
        next_index += 1;
    }
    ControlWord { word, parameter, next_index }
}

pub fn import(bytes: &[u8]) -> Result<Imported, ImportError> {
    let src = String::from_utf8_lossy(bytes);
    if !src.trim_start().starts_with("{\\rtf") {
        return Err(ImportError::Corrupt("not an RTF file".into()));
    }
    let mut st = Story::new(StoryId(0));
    let mut stack: Vec<State> = vec![State { uc: 1, ..Default::default() }];
    let mut pending = 0usize; // characters still to skip after \uN
    let mut units = crate::Utf16Units::default();
    let mut buf = String::new();
    let mut fmt = stack[0].format();
    let mut warnings = Vec::new();
    let flush = |st: &mut Story, buf: &mut String, fmt: &CharFormat| {
        if !buf.is_empty() {
            let len = st.len();
            st.insert_with(len, buf, fmt.clone());
            buf.clear();
        }
    };
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0;
    // The first `{` opens the document group.
    let mut first = true;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '{' => {
                flush(&mut st, &mut buf, &fmt);
                let top = stack.last().cloned().unwrap_or_default();
                if !first {
                    stack.push(top);
                }
                first = false;
                // A group is not a character: the `\uN` fallback skip ends here too.
                pending = 0;
                // `{\*\dest …}`: an ignorable destination.
                if chars.get(i + 1) == Some(&'\\')
                    && chars.get(i + 2) == Some(&'*')
                    && let Some(s) = stack.last_mut()
                {
                    s.skip = true;
                }
                i += 1;
            }
            '}' => {
                flush(&mut st, &mut buf, &fmt);
                if stack.len() > 1 {
                    stack.pop();
                }
                // The `\uN` fallback skip ends with the group.
                pending = 0;
                fmt = stack.last().map(State::format).unwrap_or_default();
                i += 1;
            }
            '\\' => {
                let next = chars.get(i + 1).copied().unwrap_or(' ');
                if next == '\'' {
                    // \'hh: a Windows-1252 byte.
                    // Only valid hex digits are consumed; anything else is left for the main loop.
                    let hex: String = chars.iter().skip(i + 2).take(2).take_while(|c| c.is_ascii_hexdigit()).collect();
                    i += 2 + hex.len();
                    if pending > 0 {
                        pending -= 1;
                        continue;
                    }
                    if hex.len() == 2
                        && let Ok(b) = u8::from_str_radix(&hex, 16)
                        && !stack.last().is_some_and(|s| s.skip)
                    {
                        units.reset();
                        buf.push(cp1252(b));
                    }
                    continue;
                }
                if !next.is_ascii_alphabetic() {
                    // Control symbols: \\ \{ \} \~ (nbsp) \- (soft hyphen) \_ (nb hyphen).
                    i += 2;
                    if pending > 0 {
                        pending -= 1;
                        continue;
                    }
                    let skip = stack.last().is_some_and(|s| s.skip);
                    if !skip {
                        match next {
                            '\\' | '{' | '}' => buf.push(next),
                            '~' => buf.push('\u{A0}'),
                            '-' => buf.push('\u{AD}'),
                            '_' => buf.push('\u{2011}'),
                            '\n' | '\r' => {
                                flush(&mut st, &mut buf, &fmt);
                                let len = st.len();
                                st.insert(len, "\n");
                            }
                            _ => {}
                        }
                    }
                    continue;
                }
                // A control word with an optional numeric parameter and one delimiting space.
                let ControlWord { word, parameter: param, next_index } = read_control_word(&chars, i);
                i = next_index;
                // A control word is one character of the `\uN` fallback (a following `\u` is not).
                if pending > 0 && word != "u" {
                    pending -= 1;
                    continue;
                }
                let skip = stack.last().is_some_and(|s| s.skip);
                if SKIP.contains(&word.as_str()) {
                    flush(&mut st, &mut buf, &fmt);
                    if let Some(s) = stack.last_mut() {
                        s.skip = true;
                    }
                    if word == "pict" && !warnings.iter().any(|w: &String| w.starts_with("Pictures")) {
                        warnings.push("Pictures in the RTF file were not imported.".into());
                    }
                    continue;
                }
                if skip {
                    continue;
                }
                let on = param != Some(0);
                let mut restyle = |f: &dyn Fn(&mut State)| {
                    flush(&mut st, &mut buf, &fmt);
                    if let Some(s) = stack.last_mut() {
                        f(s);
                        fmt = s.format();
                    }
                };
                match word.as_str() {
                    "par" | "sect" => {
                        flush(&mut st, &mut buf, &fmt);
                        let len = st.len();
                        st.insert_with(len, "\n", fmt.clone());
                    }
                    "line" => buf.push(designcraft_doc::FORCED_LINE_BREAK),
                    "tab" => buf.push('\t'),
                    "page" => buf.push(designcraft_doc::PAGE_BREAK),
                    "column" => buf.push(designcraft_doc::COLUMN_BREAK),
                    "emdash" => buf.push('\u{2014}'),
                    "endash" => buf.push('\u{2013}'),
                    "bullet" => buf.push('\u{2022}'),
                    "lquote" => buf.push('\u{2018}'),
                    "rquote" => buf.push('\u{2019}'),
                    "ldblquote" => buf.push('\u{201C}'),
                    "rdblquote" => buf.push('\u{201D}'),
                    "b" => restyle(&|s| s.bold = on),
                    "i" => restyle(&|s| s.italic = on),
                    "ul" => restyle(&|s| s.underline = on),
                    "ulnone" => restyle(&|s| s.underline = false),
                    "strike" => restyle(&|s| s.strike = on),
                    // Half-points; sizes outside the engine's 0.1–1296 pt range fall back to the default.
                    "fs" => restyle(&|s| {
                        s.size = param.map(|p| p as f64 / 2.0).and_then(designcraft_doc::attrs::imported_type_size);
                    }),
                    "plain" => restyle(&|s| {
                        s.bold = false;
                        s.italic = false;
                        s.underline = false;
                        s.strike = false;
                        s.size = None;
                    }),
                    "uc" => {
                        if let (Some(s), Some(n)) = (stack.last_mut(), param) {
                            s.uc = n.max(0) as usize;
                        }
                    }
                    "u" => {
                        if let Some(n) = param {
                            // RTF allows -32768..=65535; anything else is dropped.
                            if (-32768..=65535).contains(&n) {
                                let unit = if n < 0 { (n + 65536) as u32 } else { n as u32 };
                                if let Some(ch) = units.push(unit) {
                                    buf.push(ch);
                                }
                            }
                            pending = stack.last().map_or(1, |s| s.uc);
                        }
                    }
                    _ => {}
                }
            }
            '\r' | '\n' => i += 1,
            _ => {
                i += 1;
                if pending > 0 {
                    pending -= 1;
                    continue;
                }
                if !stack.last().is_some_and(|s| s.skip) {
                    units.reset();
                    buf.push(c);
                }
            }
        }
    }
    flush(&mut st, &mut buf, &fmt);
    // A trailing \par leaves an empty last paragraph.
    if st.text.ends_with('\n') {
        let n = st.len();
        st.delete(n - 1..n);
    }
    // A page or column break ends its paragraph.
    st.end_paragraphs_at_breaks();
    Ok(Imported { story: st, para_styles: vec![], char_styles: vec![], warnings })
}

/// Windows-1252 byte → char.
fn cp1252(b: u8) -> char {
    const HI: [u16; 32] = [
        0x20AC, 0x81, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, 0x02C6, 0x2030, 0x0160, 0x2039, 0x0152, 0x8D, 0x017D, 0x8F, 0x90, 0x2018,
        0x2019, 0x201C, 0x201D, 0x2022, 0x2013, 0x2014, 0x02DC, 0x2122, 0x0161, 0x203A, 0x0153, 0x9D, 0x017E, 0x0178,
    ];
    if (0x80..0xA0).contains(&b) { char::from_u32(HI[(b - 0x80) as usize] as u32).unwrap_or('?') } else { b as char }
}

#[cfg(test)]
mod tests {
    use super::*;

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
