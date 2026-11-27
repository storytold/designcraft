use std::fmt::Write as _;

#[derive(Debug)]
pub struct EpubError(pub String);

pub(crate) fn valid_xml_char(c: char) -> bool {
    matches!(c, '\u{9}' | '\u{A}' | '\u{D}' | '\u{20}'..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}' | '\u{10000}'..='\u{10FFFF}')
}

fn escape_markup(s: &str, attr: bool, story_breaks: bool) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' if attr => o.push_str("&apos;"),
            '\u{E000}'..='\u{E1FF}' => {}
            '\u{2028}' if story_breaks => o.push_str("<br/>"),
            '\u{AD}' => o.push_str("&#173;"),
            c if valid_xml_char(c) => o.push(c),
            _ => {}
        }
    }
    o
}

pub(crate) fn xml_text(s: &str) -> String {
    escape_markup(s, false, false)
}

pub(crate) fn xml_attr(s: &str) -> String {
    escape_markup(s, true, false)
}

/// Story content permits the document's discretionary line-separator marker to become markup.
/// Other contexts always serialize it as ordinary text.
pub(crate) fn story_markup(s: &str) -> String {
    escape_markup(s, false, true)
}

/// A conservative BCP 47 subset suitable for both HTML and EPUB language attributes.
pub(crate) fn language_tag(s: &str) -> &str {
    if s.len() > 63 {
        return "und";
    }
    let mut parts = s.split('-');
    let Some(first) = parts.next() else { return "und" };
    if !(2..=8).contains(&first.len()) || !first.bytes().all(|b| b.is_ascii_alphabetic()) {
        return "und";
    }
    if parts.any(|p| p.is_empty() || p.len() > 8 || !p.bytes().all(|b| b.is_ascii_alphanumeric())) {
        return "und";
    }
    s
}

/// Return a MIME type that is safe to place in an attribute and a data URL. Parameters are omitted
/// because exported assets are always represented as their original binary bytes.
pub(crate) fn safe_mime(mime: &str) -> &str {
    fn token(s: &str) -> bool {
        !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'+' | b'-'))
    }

    let mime = mime.split_once(';').map_or(mime, |(essence, _)| essence).trim();
    if mime.len() <= 127
        && let Some((kind, subtype)) = mime.split_once('/')
        && !subtype.contains('/')
        && token(kind)
        && token(subtype)
    {
        mime
    } else {
        "image/png"
    }
}

/// Quote arbitrary document metadata as one CSS string token. Escaping `<` is important because
/// HTML parses the contents of a `style` element before CSS parsing sees it.
pub(crate) fn css_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(c, '"' | '\\' | '<' | '>' | '&') || c.is_control() {
            let _ = write!(out, "\\{:x} ", c as u32);
        } else {
            out.push(c);
        }
    }
    out
}

/// CSS class name for a style name.
pub fn slug(name: &str) -> String {
    let s: String = name.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect();
    let s = s.trim_matches('-').to_string();
    if s.is_empty() {
        "basic".into()
    } else if s.starts_with(|c: char| c.is_ascii_digit()) {
        format!("s-{s}")
    } else {
        s
    }
}
