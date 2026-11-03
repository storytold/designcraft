use super::css::{export_mapping, para_classes};
use super::html::Block;
use super::markup::xml_text;
use super::*;

/// Whether an EPUB chapter file starts before a paragraph of `style` (Split Document): never
/// when it's off, before each paragraph of the one style it names, or before each paragraph of
/// a style whose Export Tagging has Split Document on.
pub(crate) fn starts_chapter(split: &EpubSplit, st: &Styles, style: &str) -> bool {
    match split {
        EpubSplit::Off => false,
        EpubSplit::Style(name) => name == style,
        EpubSplit::ExportTags => st.export_tag(style, false).is_some_and(|e| e.split),
    }
}

/// Where the EPUB body splits into chapter files (see [`starts_chapter`]).
pub(crate) const CHAPTER_MARK: &str = "<!--dc:chapter-->";

/// The body split at its [`CHAPTER_MARK`]s into chapter bodies, each with its sections closed
/// and the one a split goes through reopened in the next; a part with nothing but the start of a
/// section (a story that opens with its `h1`) moves that start into the next part.
pub(crate) fn split_chapters(body: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut carry: Option<String> = None;
    for piece in body.split(CHAPTER_MARK) {
        let mut part = carry.take().unwrap_or_default();
        part.push_str(piece);
        if let Some(open) = part.rfind("<section").filter(|i| !part[*i..].contains("</section>")) {
            let tag_end = part[open..].find('>').map_or(part.len(), |e| open + e + 1);
            if part[tag_end..].trim().is_empty() {
                carry = Some(part[open..].to_string());
                part.truncate(open);
            } else {
                part.push_str("</section>\n");
                carry = Some("<section>\n".to_string());
            }
        }
        if !part.trim().is_empty() {
            out.push(part);
        }
    }
    if let Some(rest) = carry.filter(|c| !c.trim().is_empty())
        && let Some(last) = out.last_mut()
    {
        // Only an opening tag can be left over; close it where it stands.
        last.push_str(&rest);
        last.push_str("</section>\n");
    }
    if out.is_empty() {
        out.push(String::new());
    }
    out
}

/// `chapter` (the file `own`) with each in-document link (`href="#id"`) to an element in another
/// chapter file pointed at that file.
pub(crate) fn link_across_chapters(chapter: &str, own: &str, files: &BTreeMap<String, String>) -> String {
    const HREF: &str = "href=\"#";
    let mut out = String::with_capacity(chapter.len());
    let mut rest = chapter;
    while let Some(i) = rest.find(HREF) {
        out.push_str(rest.get(..i).unwrap_or_default());
        let tail = rest.get(i + HREF.len()..).unwrap_or_default();
        let id = tail.split('"').next().unwrap_or_default();
        match files.get(id) {
            Some(file) if file != own => {
                let _ = write!(out, "href=\"{file}#");
            }
            _ => out.push_str(HREF),
        }
        rest = tail;
    }
    out.push_str(rest);
    out
}

/// The package file of chapter `k` (0-based): `content.xhtml`, then `content2.xhtml`, ….
pub(crate) fn chapter_file(k: usize) -> String {
    if k == 0 { "content.xhtml".to_string() } else { format!("content{}.xhtml", k + 1) }
}

/// The level of a heading element (`h1` → 1 … `h6` → 6).
pub(crate) fn heading_level(tag: &str) -> Option<u8> {
    tag.strip_prefix('h').and_then(|n| n.parse::<u8>().ok()).filter(|n| (1..=6).contains(n))
}

/// Table of contents entries (target, label, level): the `h1`–`h3` paragraphs of the stories in
/// reading order, or, in a document without headings, each story's first paragraph (level 1).
pub(crate) fn toc_entries(doc: &Document, order: &[Block]) -> Vec<(String, String, u8)> {
    let pc = para_classes(&doc.styles);
    let label = |t: &str| -> String { t.chars().filter(|c| !('\u{E000}'..='\u{E0FF}').contains(c)).take(80).collect::<String>().trim().to_string() };
    let mut headings = Vec::new();
    let mut firsts = Vec::new();
    for (k, b) in order.iter().enumerate() {
        let Block::Story(sid) = b else { continue };
        // The endnote story isn't written (the endnotes section stands in for it).
        if doc.endnote_story == Some(*sid) {
            continue;
        }
        let Some(st) = doc.story(*sid) else { continue };
        let first = label(st.text.split('\n').next().unwrap_or(""));
        if !first.is_empty() {
            firsts.push((format!("s{k}"), first, 1));
        }
        for (pi, r) in st.para_ranges().iter().enumerate() {
            let Some(pf) = st.paras.get(pi) else { continue };
            let (tag, _) = export_mapping(&doc.styles, &pf.style, false, &pc);
            let Some(level) = heading_level(tag).filter(|l| *l <= 3) else { continue };
            let text = label(st.text.get(r.clone()).unwrap_or(""));
            if !text.is_empty() {
                headings.push((format!("s{k}-p{pi}"), text, level));
            }
        }
    }
    if headings.is_empty() { firsts } else { headings }
}

/// The table of contents as nested `li`/`ol` items (the content of the navigation's outer
/// `ol`): a deeper level goes into the previous item, skipped levels count as one.
pub(crate) fn nav_list(entries: &[(String, String, u8)], files: &BTreeMap<String, String>) -> String {
    let mut out = String::new();
    let mut open: Vec<u8> = Vec::new();
    for (target, label, level) in entries {
        let file = files.get(target).map_or("content.xhtml", String::as_str);
        let item = format!("<li><a href=\"{file}#{target}\">{}</a>", xml_text(label));
        match open.last().copied() {
            None => open.push(*level),
            Some(top) if *level > top => {
                out.push_str("<ol>");
                open.push(*level);
            }
            Some(_) => {
                // Close the levels whose parent is as deep as this entry or deeper.
                while open.len() > 1 && open.len().checked_sub(2).and_then(|i| open.get(i)).is_some_and(|parent| *parent >= *level) {
                    open.pop();
                    out.push_str("</li></ol>");
                }
                out.push_str("</li>");
                if let Some(t) = open.last_mut() {
                    *t = (*t).min(*level);
                }
            }
        }
        out.push_str(&item);
    }
    for i in (0..open.len()).rev() {
        out.push_str("</li>");
        if i > 0 {
            out.push_str("</ol>");
        }
    }
    out
}
