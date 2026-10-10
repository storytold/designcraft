//! Tagged Text: plain text with `<Tag:value>` markup for paragraph and character styles and local
//! formatting — the text interchange format of layout applications. Written as ASCII (other
//! characters as `<0xXXXX>`); read from ASCII, UTF-8 or UTF-16 files. Paragraph styles, character
//! styles, font family, font style and size are kept; other tags are skipped.

use std::fmt::Write as _;

use designcraft_doc::{CharFormat, Document, Story, StoryId, story::NO_CHAR_STYLE};

use crate::{ImportError, Imported, ImportedStyle};

fn escape(out: &mut String, c: char) {
    match c {
        '<' => out.push_str("\\<"),
        '>' => out.push_str("\\>"),
        '\\' => out.push_str("\\\\"),
        c if c.is_ascii() && !c.is_ascii_control() => out.push(c),
        '\t' => out.push('\t'),
        c => {
            let mut buf = [0u16; 2];
            for u in c.encode_utf16(&mut buf) {
                let _ = write!(out, "<0x{u:04X}>");
            }
        }
    }
}

/// `story` as Tagged Text.
pub fn tagged_text(doc: &Document, story: &Story) -> String {
    let mut out = String::from("<ASCII-WIN>\r\n<Version:7>\r\n");
    let ranges = story.para_ranges();
    for (i, r) in ranges.iter().enumerate() {
        if i > 0 {
            out.push_str("\r\n");
        }
        let pf = story.paras.get(i).cloned().unwrap_or_default();
        let _ = write!(out, "<ParaStyle:{}>", pf.style.replace(['<', '>', ':'], ""));
        for (rr, f) in story.runs() {
            let (a, b) = (rr.start.max(r.start), rr.end.min(r.end));
            if a >= b {
                continue;
            }
            if doc.conditions_hide(f.over.conditions.as_deref().unwrap_or(&[])) || f.over.change == Some(designcraft_doc::ChangeMark::Deleted) {
                continue;
            }
            let mut open = Vec::new();
            if f.style != NO_CHAR_STYLE {
                let _ = write!(out, "<CharStyle:{}>", f.style.replace(['<', '>', ':'], ""));
                open.push("CharStyle");
            }
            if let Some(v) = &f.over.font_family {
                let _ = write!(out, "<cFont:{v}>");
                open.push("cFont");
            }
            if let Some(v) = &f.over.font_style {
                let _ = write!(out, "<cTypeface:{v}>");
                open.push("cTypeface");
            }
            if let Some(v) = f.over.size {
                let _ = write!(out, "<cSize:{v:.6}>");
                open.push("cSize");
            }
            for c in story.text[a..b].chars() {
                escape(&mut out, c);
            }
            for t in open.into_iter().rev() {
                let _ = write!(out, "<{t}:>");
            }
        }
    }
    out
}

/// Text of a Tagged Text file (ASCII, UTF-8 or UTF-16 by byte-order mark).
fn decode(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]) {
        let le = bytes[0] == 0xFF;
        let units: Vec<u16> = bytes[2..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| if le { u16::from_le_bytes([c[0], c[1]]) } else { u16::from_be_bytes([c[0], c[1]]) })
            .collect();
        return String::from_utf16_lossy(&units);
    }
    String::from_utf8_lossy(bytes).trim_start_matches('\u{FEFF}').to_string()
}

/// Is this Tagged Text (by its header)?
pub fn is_tagged(bytes: &[u8]) -> bool {
    let head = decode(&bytes[..bytes.len().min(64)]);
    let h = head.trim_start();
    h.starts_with("<ASCII-") || h.starts_with("<UNICODE-") || h.starts_with("<ANSI-")
}

/// Read Tagged Text.
pub fn import(bytes: &[u8]) -> Result<Imported, ImportError> {
    let text = decode(bytes);
    let chars: Vec<char> = text.chars().collect();
    // Paragraphs: (style, runs of (text, format)).
    let mut paras: Vec<(String, Vec<(String, CharFormat)>)> = vec![(String::new(), Vec::new())];
    let mut fmt = CharFormat::default();
    let mut para_styles: Vec<String> = Vec::new();
    let mut char_styles: Vec<String> = Vec::new();
    let mut warnings = Vec::new();
    let push = |paras: &mut Vec<(String, Vec<(String, CharFormat)>)>, c: char, fmt: &CharFormat| {
        let Some((_, runs)) = paras.last_mut() else { return };
        match runs.last_mut() {
            Some((t, f)) if f == fmt => t.push(c),
            _ => runs.push((c.to_string(), fmt.clone())),
        }
    };
    let mut units = crate::Utf16Units::default();
    let mut i = 0;
    let mut skipped = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '\\' if i + 1 < chars.len() => {
                units.reset();
                push(&mut paras, chars[i + 1], &fmt);
                i += 2;
            }
            '<' => {
                // A tag: up to the matching '>' (nested in definitions).
                let mut depth = 0;
                let mut j = i;
                while j < chars.len() {
                    match chars[j] {
                        '\\' => j += 1,
                        '<' => depth += 1,
                        '>' => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                    j += 1;
                }
                let tag: String = chars[i + 1..j.min(chars.len())].iter().collect();
                i = j + 1;
                let (name, value) = tag.split_once(':').map_or((tag.as_str(), ""), |(a, b)| (a, b));
                match name {
                    n if n.starts_with("0x") => {
                        if let Ok(u) = u32::from_str_radix(&n[2..], 16)
                            && let Some(ch) = units.push(u)
                        {
                            push(&mut paras, ch, &fmt);
                        }
                    }
                    "ParaStyle" => {
                        let v = value.trim().to_string();
                        if !v.is_empty() && !para_styles.contains(&v) {
                            para_styles.push(v.clone());
                        }
                        if let Some(p) = paras.last_mut() {
                            p.0 = v;
                        }
                    }
                    "CharStyle" => {
                        let v = value.trim();
                        fmt.style = if v.is_empty() { NO_CHAR_STYLE.into() } else { v.to_string() };
                        if !v.is_empty() && !char_styles.contains(&v.to_string()) {
                            char_styles.push(v.to_string());
                        }
                    }
                    "cFont" => fmt.over.font_family = (!value.is_empty()).then(|| value.to_string()),
                    "cTypeface" => fmt.over.font_style = (!value.is_empty()).then(|| value.to_string()),
                    "cSize" => fmt.over.size = value.trim().parse().ok(),
                    "DefineParaStyle" | "DefineCharStyle" => {
                        let nm = value.split('=').next().unwrap_or("").trim().to_string();
                        let list = if name == "DefineParaStyle" { &mut para_styles } else { &mut char_styles };
                        if !nm.is_empty() && !list.contains(&nm) {
                            list.push(nm);
                        }
                    }
                    n if n.starts_with("ASCII-") || n.starts_with("UNICODE-") || n.starts_with("ANSI-") || n == "Version" => {}
                    _ => skipped += 1,
                }
            }
            '\r' | '\n' => {
                // A paragraph break (CRLF counts once).
                if c == '\r' && chars.get(i + 1) == Some(&'\n') {
                    i += 1;
                }
                let style = paras.last().map(|p| p.0.clone()).unwrap_or_default();
                // Header lines before any text don't start paragraphs.
                if paras.len() == 1 && paras[0].1.is_empty() {
                    i += 1;
                    continue;
                }
                paras.push((style, Vec::new()));
                i += 1;
            }
            c => {
                units.reset();
                push(&mut paras, c, &fmt);
                i += 1;
            }
        }
    }
    if skipped > 0 {
        warnings.push(format!("{skipped} formatting tag(s) not kept"));
    }
    // Build the story.
    let mut st = Story::new(StoryId(0));
    for (k, (style, runs)) in paras.iter().enumerate() {
        if k > 0 {
            let n = st.len();
            st.insert(n, "\n");
        }
        for (t, f) in runs {
            let n = st.len();
            st.insert_with(n, t, f.clone());
        }
        if !style.is_empty()
            && let Some(p) = st.paras.last_mut()
        {
            p.style = style.clone();
        }
    }
    let mk = |n: &String| ImportedStyle { name: n.clone(), ..Default::default() };
    Ok(Imported { story: st, para_styles: para_styles.iter().map(mk).collect(), char_styles: char_styles.iter().map(mk).collect(), warnings })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Characters past U+FFFF are written as two tags, a UTF-16 surrogate pair. Each half was
    /// converted alone and dropped.
    #[test]
    fn surrogate_pairs_become_one_character() {
        let i = import("<ASCII-WIN>\r\n<Version:7>\r\n<ParaStyle:Body>A<0xD83D><0xDE00>B <0xD842><0xDFB7><0x7530>".as_bytes()).unwrap();
        assert_eq!(i.story.text, "A\u{1F600}B \u{20BB7}\u{7530}");
        // What the exporter writes comes back.
        let doc = Document::new(&designcraft_doc::build::NewDocument::default());
        let back = import(tagged_text(&doc, &i.story).as_bytes()).unwrap();
        assert_eq!(back.story.text, i.story.text);
        // Half a pair is not a character: it is still left out.
        assert_eq!(import("<ASCII-WIN>\r\n<ParaStyle:Body>A<0xD83D>B<0xDE00>C".as_bytes()).unwrap().story.text, "ABC");
    }

    #[test]
    fn tagged_text_round_trips() {
        let src = "<ASCII-WIN>\r\n<Version:7>\r\n<ParaStyle:Head>Caf<0x00E9> \\<1\\>\r\n<ParaStyle:Body>A <CharStyle:Em>word<CharStyle:> and <cSize:18.000000>big<cSize:> text";
        assert!(is_tagged(src.as_bytes()));
        let i = import(src.as_bytes()).unwrap();
        assert_eq!(i.story.text, "Café <1>\nA word and big text");
        assert_eq!(i.story.paras[0].style, "Head");
        assert_eq!(i.story.paras[1].style, "Body");
        assert_eq!(i.story.char_format_at(i.story.text.find("word").unwrap() + 1).style, "Em");
        assert_eq!(i.story.char_format_at(i.story.text.find("big").unwrap() + 1).over.size, Some(18.0));
        assert_eq!(i.para_styles.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["Head", "Body"]);
        // Export and back.
        let doc = Document::new(&designcraft_doc::build::NewDocument::default());
        let out = tagged_text(&doc, &i.story);
        assert!(out.starts_with("<ASCII-WIN>") && out.contains("Caf<0x00E9>") && out.contains("<CharStyle:Em>word<CharStyle:>"), "{out}");
        let back = import(out.as_bytes()).unwrap();
        assert_eq!(back.story.text, i.story.text);
        assert_eq!(back.story.paras[1].style, "Body");
        // UTF-16 with a byte-order mark.
        let mut u16 = vec![0xFF, 0xFE];
        for u in "<UNICODE-WIN>\r\n<ParaStyle:X>Hi".encode_utf16() {
            u16.extend(u.to_le_bytes());
        }
        assert_eq!(import(&u16).unwrap().story.text, "Hi");
    }
}
