//! Reflowable EPUB 3 export (File → Export → EPUB (Reflowable)).
//!
//! Stories become XHTML in reading order (page, then top-to-bottom, left-to-right of their first
//! frame); paragraph styles become CSS classes (`p.<slug>`), character styles become `span`
//! classes, local overrides become inline styles. Placed graphics become `<figure>` elements at
//! their position in the reading order. Parent-page items (folios, running heads) are skipped.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::Write;

use designcraft_doc::{Align, CharAttrs, CharProps, Content, Document, Item, ItemId, ParaProps, SpreadRef, StoryId, Styles, story};

#[derive(Clone, Debug)]
pub struct EpubOptions {
    pub title: Option<String>,
    pub author: Option<String>,
    pub language: String,
    /// Stable identifier (urn:uuid:…); derived from the title when absent.
    pub identifier: Option<String>,
    /// Cover image (PNG), shown first and marked as the book's cover.
    pub cover: Option<Vec<u8>>,
}

/// A page of a fixed-layout EPUB: its image (PNG), size in points and text (kept for search,
/// read-aloud and accessibility).
#[derive(Clone, Debug)]
pub struct FixedPage {
    pub png: Vec<u8>,
    pub width: f64,
    pub height: f64,
    pub text: String,
}

/// A pre-paginated (fixed-layout) EPUB: one XHTML page per document page.
pub fn export_fixed_epub(doc: &Document, pages: &[FixedPage], opts: &EpubOptions) -> Result<Vec<u8>, EpubError> {
    let io = |e: std::io::Error| EpubError(e.to_string());
    let zerr = |e: zip::result::ZipError| EpubError(e.to_string());
    let title = opts.title.clone().unwrap_or_else(|| doc.title.clone());
    let lang = language_tag(&opts.language);
    let id = opts.identifier.clone().unwrap_or_else(|| format!("urn:designcraft:{}", slug(&title)));
    let mut manifest = String::from("<item id=\"nav\" href=\"nav.xhtml\" media-type=\"application/xhtml+xml\" properties=\"nav\"/>\n");
    let mut spine = String::new();
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    let mut nav_items = String::new();
    for (i, p) in pages.iter().enumerate() {
        let n = i + 1;
        let (w, h) = (p.width.round().max(1.0) as u32, p.height.round().max(1.0) as u32);
        let xhtml = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE html>\n<html xmlns=\"http://www.w3.org/1999/xhtml\" xml:lang=\"{lang}\" lang=\"{lang}\">\n<head><meta charset=\"utf-8\"/><meta name=\"viewport\" content=\"width={w}, height={h}\"/><title>{} — {n}</title>\n<style>body{{margin:0;width:{w}px;height:{h}px;position:relative}} img{{position:absolute;left:0;top:0;width:{w}px;height:{h}px}} .t{{position:absolute;left:0;top:0;width:{w}px;color:transparent;font-size:1px;overflow:hidden}}</style></head>\n<body><img src=\"pages/p{n}.png\" alt=\"\"/><div class=\"t\">{}</div></body>\n</html>\n",
            xml_text(&title),
            story_markup(&p.text).replace('\n', "<br/>")
        );
        let _ = writeln!(
            manifest,
            "<item id=\"page{n}\" href=\"page{n}.xhtml\" media-type=\"application/xhtml+xml\"/>\n<item id=\"pimg{n}\" href=\"pages/p{n}.png\" media-type=\"image/png\"{}/>",
            if n == 1 { " properties=\"cover-image\"" } else { "" }
        );
        let _ = writeln!(spine, "<itemref idref=\"page{n}\"/>");
        let _ = write!(nav_items, "<li><a href=\"page{n}.xhtml\">{n}</a></li>");
        files.push((format!("OEBPS/page{n}.xhtml"), xhtml.into_bytes()));
        files.push((format!("OEBPS/pages/p{n}.png"), p.png.clone()));
    }
    let (vw, vh) = pages.first().map_or((612, 792), |p| (p.width.round() as u32, p.height.round() as u32));
    let nav = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE html>\n<html xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:epub=\"http://www.idpf.org/2007/ops\" xml:lang=\"{lang}\">\n<head><meta charset=\"utf-8\"/><title>{}</title></head>\n<body><nav epub:type=\"page-list\" id=\"toc\"><h1>Pages</h1><ol>{nav_items}</ol></nav></body>\n</html>\n",
        xml_text(&title)
    );
    let author = opts.author.as_deref().map(|a| format!("<dc:creator>{}</dc:creator>", xml_text(a))).unwrap_or_default();
    let opf = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<package xmlns=\"http://www.idpf.org/2007/opf\" version=\"3.0\" unique-identifier=\"bookid\" xml:lang=\"{lang}\" prefix=\"rendition: http://www.idpf.org/vocab/rendition/#\">\n<metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\">\n<dc:identifier id=\"bookid\">{}</dc:identifier>\n<dc:title>{}</dc:title>\n<dc:language>{lang}</dc:language>{author}\n<meta property=\"dcterms:modified\">2026-01-01T00:00:00Z</meta>\n<meta property=\"rendition:layout\">pre-paginated</meta>\n<meta property=\"rendition:spread\">auto</meta>\n<meta name=\"original-resolution\" content=\"{vw}x{vh}\"/>\n<meta name=\"generator\" content=\"DesignCraft\"/>\n</metadata>\n<manifest>\n{manifest}</manifest>\n<spine>\n{spine}</spine>\n</package>\n",
        xml_text(&id),
        xml_text(&title)
    );
    let container = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<container version=\"1.0\" xmlns=\"urn:oasis:names:tc:opendocument:xmlns:container\"><rootfiles><rootfile full-path=\"OEBPS/content.opf\" media-type=\"application/oebps-package+xml\"/></rootfiles></container>\n";
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut z = zip::ZipWriter::new(&mut buf);
        let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        let deflate = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        z.start_file("mimetype", stored).map_err(zerr)?;
        z.write_all(b"application/epub+zip").map_err(io)?;
        for (name, data) in
            [("META-INF/container.xml", container.as_bytes()), ("OEBPS/content.opf", opf.as_bytes()), ("OEBPS/nav.xhtml", nav.as_bytes())]
        {
            z.start_file(name, deflate).map_err(zerr)?;
            z.write_all(data).map_err(io)?;
        }
        for (name, data) in &files {
            let o = if name.ends_with(".png") { stored } else { deflate };
            z.start_file(name.as_str(), o).map_err(zerr)?;
            z.write_all(data).map_err(io)?;
        }
        z.finish().map_err(zerr)?;
    }
    Ok(buf.into_inner())
}

impl Default for EpubOptions {
    fn default() -> Self {
        EpubOptions { title: None, author: None, language: "en".into(), identifier: None, cover: None }
    }
}

#[derive(Debug)]
pub struct EpubError(pub String);

impl std::fmt::Display for EpubError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for EpubError {}

fn valid_xml_char(c: char) -> bool {
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

fn xml_text(s: &str) -> String {
    escape_markup(s, false, false)
}

fn xml_attr(s: &str) -> String {
    escape_markup(s, true, false)
}

/// Story content permits the document's discretionary line-separator marker to become markup.
/// Other contexts always serialize it as ordinary text.
fn story_markup(s: &str) -> String {
    escape_markup(s, false, true)
}

/// A conservative BCP 47 subset suitable for both HTML and EPUB language attributes.
fn language_tag(s: &str) -> &str {
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
fn safe_mime(mime: &str) -> &str {
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
fn css_string(s: &str) -> String {
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

/// Class names for `names`, in order: [`slug`]s made unique with a `-2`, `-3`… suffix, so styles
/// whose names slug alike ("Body Text" / "Body-Text", or non-ASCII names) keep their own rules.
fn class_names<'a>(names: impl Iterator<Item = &'a str>) -> BTreeMap<&'a str, String> {
    let mut out = BTreeMap::new();
    let mut used = std::collections::BTreeSet::new();
    for name in names {
        let base = slug(name);
        let mut class = base.clone();
        let mut n = 1;
        while !used.insert(class.clone()) {
            n += 1;
            class = format!("{base}-{n}");
        }
        out.insert(name, class);
    }
    out
}

fn para_classes(st: &Styles) -> BTreeMap<&str, String> {
    class_names(st.paragraph.iter().map(|s| s.name.as_str()))
}

fn char_classes(st: &Styles) -> BTreeMap<&str, String> {
    class_names(st.character.iter().map(|s| s.name.as_str()))
}

fn hex(doc: &Document, swatch: &str, tint: f32) -> Option<String> {
    let c = doc.resolve_color(swatch, tint)?;
    let [r, g, b, _] = c.to_rgba8(1.0);
    Some(format!("#{r:02x}{g:02x}{b:02x}"))
}

fn char_css(doc: &Document, c: &CharProps, base_size: f64, out: &mut String) {
    let _ = write!(out, "font-family: \"{}\", serif; ", css_string(&c.font_family));
    let st = c.font_style.to_lowercase();
    if st.contains("bold") {
        out.push_str("font-weight: bold; ");
    } else if st.contains("semibold") {
        out.push_str("font-weight: 600; ");
    }
    if st.contains("italic") {
        out.push_str("font-style: italic; ");
    }
    let _ = write!(out, "font-size: {:.3}em; ", c.size / base_size);
    if c.tracking != 0.0 {
        let _ = write!(out, "letter-spacing: {:.3}em; ", c.tracking / 1000.0);
    }
    match c.capitalization {
        designcraft_doc::Capitalization::AllCaps => out.push_str("text-transform: uppercase; "),
        designcraft_doc::Capitalization::SmallCaps | designcraft_doc::Capitalization::OpenTypeAllSmallCaps => {
            out.push_str("font-variant: small-caps; ")
        }
        _ => {}
    }
    if c.underline {
        out.push_str("text-decoration: underline; ");
    }
    if c.strikethrough {
        out.push_str("text-decoration: line-through; ");
    }
    if let Some(h) = hex(doc, &c.fill, c.fill_tint) {
        let _ = write!(out, "color: {h}; ");
    }
}

fn para_css(p: &ParaProps, base_size: f64, line_px: f64, out: &mut String) {
    let align = match p.align {
        Align::Left | Align::TowardsSpine => "left",
        Align::Center => "center",
        Align::Right | Align::AwayFromSpine => "right",
        _ => "justify",
    };
    let _ = write!(out, "text-align: {align}; ");
    let em = |v: f64| v / base_size;
    let _ = write!(
        out,
        "margin: {:.3}em {:.3}em {:.3}em {:.3}em; text-indent: {:.3}em; line-height: {:.3}; ",
        em(p.space_before),
        em(p.right_indent),
        em(p.space_after),
        em(p.left_indent),
        em(p.first_line_indent),
        line_px
    );
    out.push_str(if p.hyphenate { "hyphens: auto; -webkit-hyphens: auto; " } else { "hyphens: manual; " });
}

/// The stylesheet for all paragraph and character styles.
pub fn stylesheet(doc: &Document) -> String {
    let st: &Styles = &doc.styles;
    let (_, basic) = st.resolve_para_style(story::BASIC_PARAGRAPH);
    let base = basic.size.max(1.0);
    let mut css = String::from(
        "body { margin: 0 5%; font-size: 1em; }\nfigure { margin: 1em 0; text-align: center; }\nfigure img { max-width: 100%; }\nh1, h2, h3 { font-size: inherit; margin: 0; }\n",
    );
    let (pc, cc) = (para_classes(st), char_classes(st));
    for ps in &st.paragraph {
        if ps.name == designcraft_doc::NO_PARA_STYLE {
            continue;
        }
        let (pp, cp) = st.resolve_para_style(&ps.name);
        let lead = match cp.leading {
            designcraft_doc::Leading::Auto => pp.auto_leading,
            designcraft_doc::Leading::Points(v) => v / cp.size.max(1.0),
        };
        let mut rule = String::new();
        para_css(&pp, base, lead, &mut rule);
        char_css(doc, &cp, base, &mut rule);
        let _ = writeln!(css, "p.{} {{ {rule}}}", pc[ps.name.as_str()]);
    }
    for cs in &st.character {
        if cs.name == story::NO_CHAR_STYLE {
            continue;
        }
        let mut rule = String::new();
        let c = &cs.chars;
        if let Some(f) = &c.font_style {
            let f = f.to_lowercase();
            if f.contains("bold") {
                rule.push_str("font-weight: bold; ");
            }
            if f.contains("italic") {
                rule.push_str("font-style: italic; ");
            }
        }
        if let Some(fill) = &c.fill
            && let Some(h) = hex(doc, fill, c.fill_tint.unwrap_or(1.0))
        {
            let _ = write!(rule, "color: {h}; ");
        }
        if let Some(sz) = c.size {
            let _ = write!(rule, "font-size: {:.3}em; ", sz / base);
        }
        let _ = writeln!(css, "span.{} {{ {rule}}}", cc[cs.name.as_str()]);
    }
    css
}

fn override_css(doc: &Document, o: &CharAttrs) -> String {
    let mut s = String::new();
    if let Some(f) = &o.font_style {
        let f = f.to_lowercase();
        if f.contains("bold") {
            s.push_str("font-weight: bold; ");
        }
        if f.contains("italic") {
            s.push_str("font-style: italic; ");
        }
    }
    if let Some(fill) = &o.fill
        && let Some(h) = hex(doc, fill, o.fill_tint.unwrap_or(1.0))
    {
        let _ = write!(s, "color: {h}; ");
    }
    if o.underline == Some(true) {
        s.push_str("text-decoration: underline; ");
    }
    if o.capitalization == Some(designcraft_doc::Capitalization::AllCaps) {
        s.push_str("text-transform: uppercase; ");
    }
    s.trim_end().to_string()
}

/// Elements Export Tagging may choose (paragraph or character level).
fn is_tag(t: &str, character: bool) -> bool {
    if character {
        matches!(t, "span" | "em" | "strong" | "i" | "b" | "code" | "sup" | "sub" | "small" | "mark" | "cite" | "q")
    } else {
        matches!(t, "p" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "blockquote" | "pre" | "li" | "figcaption" | "aside" | "div")
    }
}

/// One story as XHTML paragraphs.
pub fn story_html(doc: &Document, sid: StoryId) -> String {
    let Some(st) = doc.story(sid) else { return String::new() };
    let (pc, cc) = (para_classes(&doc.styles), char_classes(&doc.styles));
    let mut out = String::new();
    for (pi, r) in st.para_ranges().iter().enumerate() {
        let pf = &st.paras[pi];
        // Export Tagging: the style's element and class.
        let et = doc.styles.export_tag(&pf.style, false);
        let tag = et.map(|e| e.tag.as_str()).filter(|t| is_tag(t, false)).unwrap_or("p");
        let class =
            et.map(|e| e.class.clone()).filter(|c| !c.is_empty()).or_else(|| pc.get(pf.style.as_str()).cloned()).unwrap_or_else(|| slug(&pf.style));
        let text_empty = st.text[r.clone()].trim().is_empty();
        let rtl = doc.styles.resolve_para(pf).0.direction == designcraft_doc::TextDirection::RightToLeft;
        let _ = write!(out, "<{tag} class=\"{}\"{}>", xml_attr(&class), if rtl { " dir=\"rtl\"" } else { "" });
        if text_empty {
            out.push_str("&#160;");
        }
        for (rr, f) in st.runs() {
            let a = rr.start.max(r.start);
            let b = rr.end.min(r.end);
            if a >= b {
                continue;
            }
            // Hidden conditional text isn't exported.
            if f.over.conditions.as_deref().is_some_and(|c| doc.conditions_hide(c)) || f.over.change == Some(designcraft_doc::ChangeMark::Deleted) {
                continue;
            }
            let raw = &st.text[a..b];
            let t = if raw.chars().any(|c| designcraft_doc::vars::var_index(c).is_some()) {
                let sub: String = raw
                    .chars()
                    .map(|c| match designcraft_doc::vars::var_index(c) {
                        Some(i) => doc.variable_value(i, None).unwrap_or_default(),
                        None => c.to_string(),
                    })
                    .collect();
                story_markup(&sub)
            } else {
                story_markup(raw)
            };
            let et = (f.style != story::NO_CHAR_STYLE).then(|| doc.styles.export_tag(&f.style, true)).flatten();
            let ctag = et.map(|e| e.tag.as_str()).filter(|t| is_tag(t, true)).unwrap_or("span");
            let cls = if f.style != story::NO_CHAR_STYLE {
                let class = et
                    .map(|e| e.class.clone())
                    .filter(|c| !c.is_empty())
                    .or_else(|| cc.get(f.style.as_str()).cloned())
                    .unwrap_or_else(|| slug(&f.style));
                format!(" class=\"{}\"", xml_attr(&class))
            } else {
                String::new()
            };
            let style = override_css(doc, &f.over);
            let style_attr = if style.is_empty() { String::new() } else { format!(" style=\"{}\"", xml_attr(&style)) };
            if cls.is_empty() && style_attr.is_empty() {
                out.push_str(&t);
            } else {
                let _ = write!(out, "<{ctag}{cls}{style_attr}>{t}</{ctag}>");
            }
        }
        let _ = writeln!(out, "</{tag}>");
    }
    out
}

/// Reading order of document content: (page, y, x) of each story's first frame and of graphics.
#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    Story(StoryId),
    Image(ItemId),
}

pub fn reading_order(doc: &Document) -> Vec<Block> {
    // Articles, when there are any to export, decide the order (and what's included).
    if doc.articles.iter().any(|a| a.export) {
        let mut out = Vec::new();
        let mut seen = Vec::new();
        for a in doc.articles.iter().filter(|a| a.export) {
            for id in &a.items {
                let Some(top) = doc.item(*id) else { continue };
                let mut push = |it: &Item| match &it.content {
                    Content::Text(tf) if !seen.contains(&tf.story) => {
                        seen.push(tf.story);
                        out.push(Block::Story(tf.story));
                    }
                    Content::Graphic(_) => out.push(Block::Image(it.id)),
                    _ => {}
                };
                top.walk(&mut |i| push(i));
            }
        }
        return out;
    }
    let mut keyed: Vec<((usize, i64, i64), Block)> = Vec::new();
    let mut seen_stories = Vec::new();
    for (si, sp) in doc.spreads.iter().enumerate() {
        let first = doc.first_page_of_spread(si);
        let mut items: Vec<&Item> = Vec::new();
        for top in &sp.items {
            top.walk(&mut |i| items.push(i));
        }
        for it in items {
            if it.hidden {
                continue;
            }
            let b = it.bounds();
            let page = first + sp.page_at_x(b.center().x).unwrap_or(0);
            let key = (page, (b.y0 * 10.0) as i64, (b.x0 * 10.0) as i64);
            match &it.content {
                Content::Text(tf) => {
                    let Some(story) = doc.story(tf.story) else { continue };
                    // A story is placed where its first frame is.
                    if story.frames.first() == Some(&it.id) && !seen_stories.contains(&tf.story) && !story.text.trim().is_empty() {
                        seen_stories.push(tf.story);
                        keyed.push((key, Block::Story(tf.story)));
                    }
                }
                Content::Graphic(_) => keyed.push((key, Block::Image(it.id))),
                _ => {}
            }
        }
    }
    let _ = SpreadRef::Doc(0);
    keyed.sort_by_key(|(k, _)| *k);
    keyed.into_iter().map(|(_, b)| b).collect()
}

fn ext(mime: &str) -> &'static str {
    match mime {
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/svg+xml" => "svg",
        "video/mp4" => "mp4",
        "video/quicktime" => "mov",
        "video/webm" => "webm",
        "video/ogg" => "ogv",
        "audio/mpeg" => "mp3",
        "audio/mp4" => "m4a",
        "audio/wav" => "wav",
        "audio/ogg" => "ogg",
        "audio/aac" => "aac",
        _ => "png",
    }
}

/// The content in reading order as HTML sections and figures; `img_src` names each image
/// (a package path, or a data URI for single-file HTML). Returns (body, images by name, table of
/// contents).
#[allow(clippy::type_complexity)]
fn body_html<'a>(
    doc: &'a Document,
    img_src: &dyn Fn(&str, &str, &[u8]) -> String,
) -> (String, BTreeMap<String, (&'a str, &'a [u8])>, Vec<(String, String)>) {
    let order = reading_order(doc);
    let mut body = String::new();
    let mut images: BTreeMap<String, (&str, &[u8])> = BTreeMap::new();
    let mut toc: Vec<(String, String)> = Vec::new();
    for (k, b) in order.iter().enumerate() {
        match b {
            Block::Story(sid) => {
                let anchor = format!("s{k}");
                // First paragraph text as the TOC label.
                if let Some(st) = doc.story(*sid) {
                    let first: String =
                        st.text.split('\n').next().unwrap_or("").chars().filter(|c| !('\u{E000}'..='\u{E0FF}').contains(c)).take(60).collect();
                    if !first.trim().is_empty() {
                        toc.push((anchor.clone(), first));
                    }
                }
                let _ = write!(body, "<section id=\"{anchor}\">\n{}</section>\n", story_html(doc, *sid));
            }
            Block::Image(iid) => {
                let Some(Content::Graphic(g)) = doc.item(*iid).map(|i| &i.content) else { continue };
                let Some(a) = doc.assets.get(&g.asset) else { continue };
                if a.data.is_empty() {
                    continue;
                }
                let mime = safe_mime(&a.mime);
                // Video and sound: an HTML5 player (with the poster, when there is one).
                if let Some(kind) = designcraft_doc::media_kind(mime) {
                    let name = format!("media/{}.{}", g.asset.0, ext(mime));
                    images.insert(name.clone(), (mime, a.data.as_slice()));
                    let m = doc.item(*iid).and_then(|i| i.media.clone()).unwrap_or_default();
                    let mut attrs = String::new();
                    for (on, attr) in
                        [(m.controls, " controls=\"controls\""), (m.play_on_page_load, " autoplay=\"autoplay\""), (m.looping, " loop=\"loop\"")]
                    {
                        if on {
                            attrs.push_str(attr);
                        }
                    }
                    if let Some(pa) = m.poster.and_then(|p| doc.assets.get(&p)) {
                        let poster_mime = safe_mime(&pa.mime);
                        let pn = format!("images/{}.{}", pa.id.0, ext(poster_mime));
                        images.insert(pn.clone(), (poster_mime, pa.data.as_slice()));
                        if kind == "video" {
                            attrs.push_str(&format!(" poster=\"{}\"", xml_attr(&img_src(&pn, poster_mime, &pa.data))));
                        }
                    }
                    let _ = writeln!(
                        body,
                        "<figure><{kind} src=\"{}\"{attrs}>{}</{kind}></figure>",
                        xml_attr(&img_src(&name, mime, &a.data)),
                        xml_text(&a.name)
                    );
                    continue;
                }
                let name = format!("images/{}.{}", g.asset.0, ext(mime));
                images.insert(name.clone(), (mime, a.data.as_slice()));
                // Alt text from Object Export Options, else the file name.
                let alt = doc.item(*iid).map(|i| i.alt_text.as_str()).filter(|t| !t.is_empty()).unwrap_or(&a.name);
                // Object Export Options: alignment and a page break before.
                let eo = doc.item(*iid).map(|i| i.export_options.clone()).unwrap_or_default();
                let mut css = String::new();
                if matches!(eo.align.as_str(), "left" | "center" | "right") {
                    css.push_str(&format!("text-align:{};", eo.align));
                }
                if eo.page_break_before {
                    css.push_str("page-break-before:always;break-before:page;");
                }
                let style = if css.is_empty() { String::new() } else { format!(" style=\"{}\"", xml_attr(&css)) };
                let _ =
                    writeln!(body, "<figure{style}><img src=\"{}\" alt=\"{}\"/></figure>", xml_attr(&img_src(&name, mime, &a.data)), xml_attr(alt));
            }
        }
    }
    (body, images, toc)
}

/// Options for [`export_html`].
#[derive(Clone, Debug, Default)]
pub struct HtmlOptions {
    pub title: Option<String>,
    pub language: Option<String>,
}

/// File › Export › HTML: one self-contained page (styles inline, images as data URIs) with the
/// document's stories and graphics in reading order.
pub fn export_html(doc: &Document, opts: &HtmlOptions) -> String {
    let title = opts.title.clone().unwrap_or_else(|| doc.title.clone());
    let language = opts.language.as_deref().unwrap_or("en");
    let lang = language_tag(language);
    let (body, _, _) = body_html(doc, &|_, mime, data| format!("data:{mime};base64,{}", base64(data)));
    format!(
        "<!DOCTYPE html>\n<html lang=\"{lang}\">\n<head>\n<meta charset=\"utf-8\">\n<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<meta name=\"generator\" content=\"DesignCraft\">\n<title>{}</title>\n<style>\nbody {{ max-width: 40em; margin: 2em auto; padding: 0 1em; }}\nfigure {{ margin: 1em 0; }} figure img {{ max-width: 100%; height: auto; }}\n{}</style>\n</head>\n<body>\n{body}</body>\n</html>\n",
        xml_text(&title),
        stylesheet(doc)
    )
}

fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for (i, shift) in [18, 12, 6, 0].into_iter().enumerate() {
            out.push(if i <= c.len() { T[(n >> shift & 63) as usize] as char } else { '=' });
        }
    }
    out
}

/// Export the document as a reflowable EPUB 3.
pub fn export_epub(doc: &Document, opts: &EpubOptions) -> Result<Vec<u8>, EpubError> {
    let io = |e: std::io::Error| EpubError(e.to_string());
    let zerr = |e: zip::result::ZipError| EpubError(e.to_string());
    let title = opts.title.clone().unwrap_or_else(|| doc.title.clone());
    let id = opts.identifier.clone().unwrap_or_else(|| format!("urn:designcraft:{}", slug(&title)));
    let (body, images, toc) = body_html(doc, &|name, _, _| name.to_string());
    let lang = language_tag(&opts.language);
    let chapter = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE html>\n<html xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:epub=\"http://www.idpf.org/2007/ops\" xml:lang=\"{lang}\" lang=\"{lang}\">\n<head><meta charset=\"utf-8\"/><title>{}</title><link rel=\"stylesheet\" type=\"text/css\" href=\"style.css\"/></head>\n<body>\n{body}</body>\n</html>\n",
        xml_text(&title)
    );
    let mut nav_items = String::new();
    for (a, label) in &toc {
        let _ = write!(nav_items, "<li><a href=\"content.xhtml#{a}\">{}</a></li>", xml_text(label));
    }
    if nav_items.is_empty() {
        nav_items = format!("<li><a href=\"content.xhtml\">{}</a></li>", xml_text(&title));
    }
    let nav = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE html>\n<html xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:epub=\"http://www.idpf.org/2007/ops\" xml:lang=\"{lang}\">\n<head><meta charset=\"utf-8\"/><title>{}</title></head>\n<body><nav epub:type=\"toc\" id=\"toc\"><h1>Contents</h1><ol>{nav_items}</ol></nav></body>\n</html>\n",
        xml_text(&title)
    );
    let mut manifest = String::from(
        "<item id=\"nav\" href=\"nav.xhtml\" media-type=\"application/xhtml+xml\" properties=\"nav\"/>\n<item id=\"css\" href=\"style.css\" media-type=\"text/css\"/>\n<item id=\"content\" href=\"content.xhtml\" media-type=\"application/xhtml+xml\"/>\n",
    );
    for (k, (name, (mime, _))) in images.iter().enumerate() {
        let _ = writeln!(manifest, "<item id=\"img{k}\" href=\"{}\" media-type=\"{}\"/>", xml_attr(name), xml_attr(mime));
    }
    // Cover: an image page first in reading order.
    if opts.cover.is_some() {
        manifest.push_str("<item id=\"cover-img\" href=\"images/cover.png\" media-type=\"image/png\" properties=\"cover-image\"/>\n<item id=\"cover\" href=\"cover.xhtml\" media-type=\"application/xhtml+xml\"/>\n");
    }
    let cover_spine = if opts.cover.is_some() { "<itemref idref=\"cover\"/>\n" } else { "" };
    let author = opts.author.as_deref().map(|a| format!("<dc:creator>{}</dc:creator>", xml_text(a))).unwrap_or_default();
    let opf = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<package xmlns=\"http://www.idpf.org/2007/opf\" version=\"3.0\" unique-identifier=\"bookid\" xml:lang=\"{lang}\">\n<metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\">\n<dc:identifier id=\"bookid\">{}</dc:identifier>\n<dc:title>{}</dc:title>\n<dc:language>{lang}</dc:language>{author}\n<meta property=\"dcterms:modified\">2026-01-01T00:00:00Z</meta>\n<meta name=\"generator\" content=\"DesignCraft\"/>\n</metadata>\n<manifest>\n{manifest}</manifest>\n<spine>\n{cover_spine}<itemref idref=\"content\"/>\n</spine>\n</package>\n",
        xml_text(&id),
        xml_text(&title)
    );
    let container = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<container version=\"1.0\" xmlns=\"urn:oasis:names:tc:opendocument:xmlns:container\"><rootfiles><rootfile full-path=\"OEBPS/content.opf\" media-type=\"application/oebps-package+xml\"/></rootfiles></container>\n";
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut z = zip::ZipWriter::new(&mut buf);
        let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        let deflate = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        z.start_file("mimetype", stored).map_err(zerr)?;
        z.write_all(b"application/epub+zip").map_err(io)?;
        for (name, data) in [
            ("META-INF/container.xml", container.as_bytes()),
            ("OEBPS/content.opf", opf.as_bytes()),
            ("OEBPS/nav.xhtml", nav.as_bytes()),
            ("OEBPS/style.css", stylesheet(doc).as_bytes()),
            ("OEBPS/content.xhtml", chapter.as_bytes()),
        ] {
            z.start_file(name, deflate).map_err(zerr)?;
            z.write_all(data).map_err(io)?;
        }
        for (name, (_, data)) in &images {
            z.start_file(format!("OEBPS/{name}"), stored).map_err(zerr)?;
            z.write_all(data).map_err(io)?;
        }
        if let Some(c) = &opts.cover {
            z.start_file("OEBPS/images/cover.png", stored).map_err(zerr)?;
            z.write_all(c).map_err(io)?;
            let xhtml = format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE html>\n<html xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:epub=\"http://www.idpf.org/2007/ops\" xml:lang=\"{lang}\">\n<head><meta charset=\"utf-8\"/><title>{}</title><style>body{{margin:0;text-align:center}} img{{max-width:100%;max-height:100vh}}</style></head>\n<body epub:type=\"cover\"><img src=\"images/cover.png\" alt=\"{}\"/></body>\n</html>\n",
                xml_text(&title),
                xml_attr(&title)
            );
            z.start_file("OEBPS/cover.xhtml", deflate).map_err(zerr)?;
            z.write_all(xhtml.as_bytes()).map_err(io)?;
        }
        z.finish().map_err(zerr)?;
    }
    Ok(buf.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use designcraft_doc::build::NewDocument;
    use designcraft_doc::geom::{Affine, Rect, shapes};
    use designcraft_doc::{Asset, AssetId, Content, Graphic, Item, ItemId, ParaFormat, Shape, SpreadRef};
    use std::io::Read;
    use std::sync::Arc;

    #[test]
    fn exports_valid_package_in_reading_order() {
        let mut d = Document::new(&NewDocument { pages: 2, ..Default::default() });
        let lid = d.default_layer();
        d.add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 400.0, 300.0, 500.0), lid, "Second <b> & co", ParaFormat::default()).unwrap();
        d.add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 36.0, 300.0, 100.0), lid, "First\nline two", ParaFormat::default()).unwrap();
        let bytes = export_epub(&d, &EpubOptions::default()).unwrap();
        let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        assert_eq!(z.by_index(0).unwrap().name(), "mimetype");
        let mut html = String::new();
        z.by_name("OEBPS/content.xhtml").unwrap().read_to_string(&mut html).unwrap();
        let a = html.find("First").unwrap();
        let b = html.find("Second &lt;b&gt; &amp; co").unwrap();
        assert!(a < b, "reading order");
        assert!(html.contains("<p class=\"basic-paragraph\">"));
        let mut css = String::new();
        z.by_name("OEBPS/style.css").unwrap().read_to_string(&mut css).unwrap();
        assert!(css.contains("p.basic-paragraph"));
        assert!(z.by_name("OEBPS/nav.xhtml").is_ok());
    }

    #[test]
    fn styles_whose_names_slug_alike_keep_their_own_classes() {
        let mut d = Document::new(&NewDocument::default());
        let lid = d.default_layer();
        for (name, size) in [("見出し", 30.0), ("本文", 10.0), ("Body Text", 12.0), ("Body-Text", 20.0)] {
            d.styles_mut().paragraph.push(designcraft_doc::ParagraphStyle {
                name: name.into(),
                based_on: None,
                next_style: None,
                para: Default::default(),
                chars: CharAttrs { size: Some(size), ..Default::default() },
                shortcut: String::new(),
            });
        }
        for (k, name) in ["見出し", "本文", "Body Text", "Body-Text"].into_iter().enumerate() {
            let y = 36.0 + 100.0 * k as f64;
            let pf = ParaFormat { style: name.into(), ..Default::default() };
            d.add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, y, 300.0, y + 50.0), lid, &format!("P{k}"), pf).unwrap();
        }
        let bytes = export_epub(&d, &EpubOptions::default()).unwrap();
        let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let mut html = String::new();
        z.by_name("OEBPS/content.xhtml").unwrap().read_to_string(&mut html).unwrap();
        let mut css = String::new();
        z.by_name("OEBPS/style.css").unwrap().read_to_string(&mut css).unwrap();
        let class_of = |text: &str| {
            let end = html.find(&format!("\">{text}</p>")).unwrap();
            let start = html[..end].rfind("class=\"").unwrap() + 7;
            html[start..end].to_string()
        };
        let classes: Vec<String> = (0..4).map(|k| class_of(&format!("P{k}"))).collect();
        for (k, c) in classes.iter().enumerate() {
            assert!(!classes[..k].contains(c), "class `{c}` shared: {classes:?}");
            assert_eq!(css.matches(&format!("p.{c} {{")).count(), 1, "one rule for `{c}`:\n{css}");
        }
    }

    #[test]
    fn slugs() {
        assert_eq!(slug("[Basic Paragraph]"), "basic-paragraph");
        assert_eq!(slug("Body First"), "body-first");
        assert_eq!(slug("1 Head"), "s-1-head");
    }

    #[test]
    fn html_is_one_self_contained_page() {
        let mut d = Document::new(&NewDocument::default());
        let lid = d.default_layer();
        d.add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 36.0, 300.0, 100.0), lid, "Hello <web>", ParaFormat::default()).unwrap();
        let html = export_html(&d, &HtmlOptions { title: Some("T".into()), language: None });
        assert!(html.starts_with("<!DOCTYPE html>") && html.contains("<title>T</title>") && html.contains("Hello &lt;web&gt;"));
        assert!(html.contains("p.basic-paragraph"), "styles inline");
        assert_eq!(base64(b"Man"), "TWFu");
        assert_eq!(base64(b"Ma"), "TWE=");
        assert_eq!(base64(b"M"), "TQ==");
    }

    #[test]
    fn metadata_is_serialized_for_its_output_context() {
        let mut d = Document::new(&NewDocument::default());
        d.title = "Field Notes & <Edition>".into();
        let lid = d.default_layer();
        let (_, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 36.0, 300.0, 100.0), lid, "A&B <C> \"D\"", ParaFormat::default()).unwrap();
        d.styles_mut()
            .export_tags
            .insert(format!("p:{}", story::BASIC_PARAGRAPH), designcraft_doc::ExportTag { tag: "h2".into(), class: "Primary \"Copy\"".into() });
        if let Some(basic) = d.styles_mut().paragraph.iter_mut().find(|s| s.name == story::BASIC_PARAGRAPH) {
            basic.chars.font_family = Some("Quoted \"Serif\" <Alt>".into());
        }

        let aid = AssetId(d.alloc());
        d.assets.insert(
            aid,
            Arc::new(Asset {
                id: aid,
                name: "Cover & Notes".into(),
                mime: "image/png; profile=screen".into(),
                data: Arc::new(vec![1]),
                pixels: Some((1, 1)),
                ..Default::default()
            }),
        );
        let mut item = Item::new(ItemId(d.alloc()), lid, Shape::Rectangle, shapes::rectangle(Rect::new(36.0, 120.0, 72.0, 156.0)));
        item.alt_text = "Cover \"A\" & B".into();
        item.content = Content::Graphic(Graphic {
            asset: aid,
            size: (1.0, 1.0),
            xf: Affine::IDENTITY,
            auto_fit: Default::default(),
            fit_align: 4,
            crop: [0.0; 4],
        });
        d.insert_item(SpreadRef::Doc(0), item, None).unwrap();

        let story = story_html(&d, sid);
        assert!(story.contains("<h2 class=\"Primary &quot;Copy&quot;\">A&amp;B &lt;C&gt; &quot;D&quot;</h2>"));
        let html = export_html(&d, &HtmlOptions { title: None, language: Some("English (US)".into()) });
        assert!(html.contains("<html lang=\"und\">"));
        assert!(html.contains("<title>Field Notes &amp; &lt;Edition&gt;</title>"));
        assert!(html.contains("alt=\"Cover &quot;A&quot; &amp; B\""));
        assert!(html.contains("src=\"data:image/png;base64,AQ==\""));
        assert!(!html.contains("image/png; profile=screen"));
        assert!(!stylesheet(&d).contains("<Alt>"));
        assert!(stylesheet(&d).contains("\\3c Alt\\3e "));

        let bytes = export_epub(
            &d,
            &EpubOptions {
                title: None,
                author: Some("A & B".into()),
                language: "English (US)".into(),
                identifier: Some("notes&edition".into()),
                cover: None,
            },
        )
        .unwrap();
        let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let mut opf = String::new();
        z.by_name("OEBPS/content.opf").unwrap().read_to_string(&mut opf).unwrap();
        assert!(opf.contains("xml:lang=\"und\""));
        assert!(opf.contains("<dc:identifier id=\"bookid\">notes&amp;edition</dc:identifier>"));
        assert!(opf.contains("<dc:creator>A &amp; B</dc:creator>"));
        assert!(opf.contains("media-type=\"image/png\""));
    }
}
