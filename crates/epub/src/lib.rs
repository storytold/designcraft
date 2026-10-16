//! Reflowable EPUB 3 export (File → Export → EPUB (Reflowable)).
//!
//! Stories become XHTML in reading order (page, then top-to-bottom, left-to-right of their first
//! frame); paragraph styles become CSS classes (`p.<slug>`), character styles become `span`
//! classes, local overrides become inline styles. Placed graphics become `<figure>` elements at
//! their position in the reading order. Parent-page items (folios, running heads) are skipped.
//! Tables become `<table>` elements, footnotes linked `aside`s after the paragraph that refers to
//! them, and endnotes a linked section at the end.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::Write;

use designcraft_doc::{
    Align, CharAttrs, CharProps, Content, Document, ENDNOTE_REF, FOOTNOTE_REF, Item, ItemId, ParaProps, SpreadRef, Story, StoryId, Styles, Table,
    story,
};

#[derive(Clone, Debug)]
pub struct EpubOptions {
    pub title: Option<String>,
    pub author: Option<String>,
    pub language: String,
    /// Stable identifier (urn:uuid:…); derived from the title when absent.
    pub identifier: Option<String>,
    /// Cover image (PNG or JPEG), shown first and marked as the book's cover.
    pub cover: Option<Vec<u8>>,
    /// Title of the navigation (table of contents); from `language` when absent.
    pub toc_title: Option<String>,
    /// Generic CSS family (`serif`, `sans-serif`, `monospace`, `cursive`, `fantasy`) per font
    /// family, from the fonts' classification; families not in it are guessed from their name.
    pub generic_families: BTreeMap<String, String>,
    /// Font files packaged with the book and declared with `@font-face`.
    pub fonts: Vec<EmbeddedFont>,
    /// `dcterms:modified` (Unix seconds, UTC); the document's last save, else its creation, when
    /// absent.
    pub modified: Option<i64>,
}

/// A font file packaged with an EPUB: the face of `family` in `style` (`Bold Italic`…).
#[derive(Clone, Debug)]
pub struct EmbeddedFont {
    pub family: String,
    pub style: String,
    pub data: Vec<u8>,
}

/// The `(family, style)` pairs the stylesheet names, one each, in style order: the fonts an
/// export can embed.
pub fn stylesheet_fonts(doc: &Document) -> Vec<(String, String)> {
    let st: &Styles = &doc.styles;
    let mut out: Vec<(String, String)> = Vec::new();
    for ps in &st.paragraph {
        if ps.name == designcraft_doc::NO_PARA_STYLE {
            continue;
        }
        let (_, cp) = st.resolve_para_style(&ps.name);
        let f = (cp.font_family.clone(), cp.font_style.clone());
        if !f.0.is_empty() && !out.contains(&f) {
            out.push(f);
        }
    }
    out
}

/// The CSS generic family for `family`: the known one, else a guess from the name
/// (`… Mono`/`Courier`/`… Code` → monospace, `… Sans`/`Gothic`/`Grotesk`/`Helvetica`/`Arial`/
/// `Verdana` → sans-serif), else serif.
fn generic_family<'a>(family: &str, known: &'a BTreeMap<String, String>) -> &'a str {
    const GENERIC: [&str; 6] = ["serif", "sans-serif", "monospace", "cursive", "fantasy", "system-ui"];
    if let Some(g) = known.get(family).and_then(|g| GENERIC.iter().find(|x| **x == g.as_str())) {
        return g;
    }
    let f = family.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| f.contains(w));
    if has(&["mono", "courier", " code", "consol"]) {
        "monospace"
    } else if has(&["sans", "gothic", "grotesk", "grotesque", "helvetica", "arial", "verdana"]) {
        "sans-serif"
    } else {
        "serif"
    }
}

/// CSS weight and style of a style name (`Semibold Italic` → 600, italic).
fn css_weight_style(style: &str) -> (u16, &'static str) {
    let s = style.to_lowercase().replace([' ', '-'], "");
    let weight = [
        ("thin", 100),
        ("extralight", 200),
        ("ultralight", 200),
        ("light", 300),
        ("medium", 500),
        ("semibold", 600),
        ("demibold", 600),
        ("extrabold", 800),
        ("ultrabold", 800),
        ("bold", 700),
        ("black", 900),
        ("heavy", 900),
    ]
    .iter()
    .find(|(n, _)| s.contains(n))
    .map_or(400, |(_, w)| *w);
    (weight, if s.contains("italic") || s.contains("oblique") { "italic" } else { "normal" })
}

/// Package path and media type of an embedded font (`fonts/{k}-{family}-{style}.{ext}`).
fn font_file(k: usize, f: &EmbeddedFont) -> (String, &'static str) {
    let (ext, mime) = match f.data.get(..4) {
        Some(b"OTTO") => ("otf", "font/otf"),
        Some(b"wOFF") => ("woff", "font/woff"),
        Some(b"wOF2") => ("woff2", "font/woff2"),
        _ => ("ttf", "font/ttf"),
    };
    let name: String =
        format!("{}-{}", f.family, f.style).chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect();
    let name = name.split('-').filter(|p| !p.is_empty()).collect::<Vec<_>>().join("-");
    (format!("fonts/{k}-{name}.{ext}"), mime)
}

/// The table of contents' title in `language` (a BCP 47 tag or a language name's code), English
/// for languages without one here.
pub fn contents_title(language: &str) -> &'static str {
    let code = language.split(['-', '_']).next().unwrap_or("").to_ascii_lowercase();
    match code.as_str() {
        "nl" => "Inhoud",
        "de" => "Inhalt",
        "fr" => "Table des matières",
        "es" | "pt" => "Índice",
        "it" => "Indice",
        "ja" => "目次",
        "zh" => "目录",
        _ => "Contents",
    }
}

/// An ISO 8601 UTC timestamp (`2026-10-13T09:30:00Z`) for Unix seconds `t`.
fn iso8601(t: i64) -> String {
    let (days, secs) = (t.div_euclid(86_400), t.rem_euclid(86_400));
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z", secs / 3600, secs % 3600 / 60, secs % 60)
}

/// The package's `dcterms:modified`: the given time, else the document's last save or creation.
fn modified(doc: &Document, opts: &EpubOptions) -> String {
    let t = opts.modified.or((doc.modified > 0).then_some(doc.modified)).or((doc.created > 0).then_some(doc.created));
    t.map_or_else(|| "2026-01-01T00:00:00Z".to_string(), iso8601)
}

/// The book's identifier: the given one, else a `urn:uuid` derived from the title and the
/// document's creation time, so a book keeps its identifier from export to export and two books
/// with the same title don't share one.
fn identifier(doc: &Document, opts: &EpubOptions, title: &str) -> String {
    if let Some(id) = opts.identifier.clone().filter(|i| !i.trim().is_empty()) {
        return id;
    }
    // Two FNV-1a 64-bit hashes (different offsets) as 128 bits, shaped as a version-5-style UUID.
    let seed = format!("{title}\n{}", doc.created);
    let fnv = |offset: u64| seed.bytes().fold(offset, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3));
    let (a, b) = (fnv(0xcbf2_9ce4_8422_2325), fnv(0x84222325_cbf29ce4));
    let hi = (a & 0xffff_ffff_ffff_0fff) | 0x5000;
    let lo = (b & 0x3fff_ffff_ffff_ffff) | 0x8000_0000_0000_0000;
    format!("urn:uuid:{:08x}-{:04x}-{:04x}-{:04x}-{:012x}", hi >> 32, (hi >> 16) & 0xffff, hi & 0xffff, lo >> 48, lo & 0xffff_ffff_ffff)
}

/// Package name and media type of a cover image: JPEG by its signature, else PNG.
fn cover_file(data: &[u8]) -> (&'static str, &'static str) {
    if data.starts_with(&[0xFF, 0xD8, 0xFF]) { ("images/cover.jpg", "image/jpeg") } else { ("images/cover.png", "image/png") }
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
    let id = identifier(doc, opts, &title);
    let modified = modified(doc, opts);
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
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<package xmlns=\"http://www.idpf.org/2007/opf\" version=\"3.0\" unique-identifier=\"bookid\" xml:lang=\"{lang}\" prefix=\"rendition: http://www.idpf.org/vocab/rendition/#\">\n<metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\">\n<dc:identifier id=\"bookid\">{}</dc:identifier>\n<dc:title>{}</dc:title>\n<dc:language>{lang}</dc:language>{author}\n<meta property=\"dcterms:modified\">{modified}</meta>\n<meta property=\"rendition:layout\">pre-paginated</meta>\n<meta property=\"rendition:spread\">auto</meta>\n<meta name=\"original-resolution\" content=\"{vw}x{vh}\"/>\n<meta name=\"generator\" content=\"DesignCraft\"/>\n</metadata>\n<manifest>\n{manifest}</manifest>\n<spine>\n{spine}</spine>\n</package>\n",
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
        EpubOptions {
            title: None,
            author: None,
            language: "en".into(),
            identifier: None,
            cover: None,
            toc_title: None,
            generic_families: BTreeMap::new(),
            fonts: Vec::new(),
            modified: None,
        }
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

fn char_css(doc: &Document, c: &CharProps, base_size: f64, generics: &BTreeMap<String, String>, out: &mut String) {
    let _ = write!(out, "font-family: \"{}\", {}; ", css_string(&c.font_family), generic_family(&c.font_family, generics));
    // The face's own weight, so an embedded Semibold or Light face is the one chosen.
    let (weight, style) = css_weight_style(&c.font_style);
    let _ = write!(out, "font-weight: {weight}; font-style: {style}; ");
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
    stylesheet_with(doc, &BTreeMap::new(), &[])
}

/// [`stylesheet`] with the generic families to fall back on and `@font-face` rules for the
/// embedded fonts (packaged as [`font_file`] names them).
fn stylesheet_with(doc: &Document, generics: &BTreeMap<String, String>, fonts: &[EmbeddedFont]) -> String {
    let st: &Styles = &doc.styles;
    let mut faces = String::new();
    for (k, f) in fonts.iter().enumerate() {
        let (path, _) = font_file(k, f);
        let (weight, style) = css_weight_style(&f.style);
        let _ = writeln!(
            faces,
            "@font-face {{ font-family: \"{}\"; src: url(\"{path}\"); font-weight: {weight}; font-style: {style}; }}",
            css_string(&f.family)
        );
    }
    let (_, basic) = st.resolve_para_style(story::BASIC_PARAGRAPH);
    let base = basic.size.max(1.0);
    let mut css = faces;
    css.push_str(
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
        char_css(doc, &cp, base, generics, &mut rule);
        let (tag, class) = export_mapping(st, &ps.name, false, &pc);
        if let Some(selector) = class_selector(tag, &class) {
            let _ = writeln!(css, "{selector} {{ {rule}}}");
        }
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
        let (tag, class) = export_mapping(st, &cs.name, true, &cc);
        if let Some(selector) = class_selector(tag, &class) {
            let _ = writeln!(css, "{selector} {{ {rule}}}");
        }
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

/// Resolve Export Tagging once for both markup and its stylesheet. Keep the generated,
/// collision-free class when no custom class was supplied, and the existing tag fallback.
fn export_mapping<'a>(st: &'a Styles, name: &str, character: bool, classes: &BTreeMap<&str, String>) -> (&'a str, String) {
    let et = st.export_tag(name, character);
    let tag = et.map(|e| e.tag.as_str()).filter(|t| is_tag(t, character)).unwrap_or(if character { "span" } else { "p" });
    let class = et.map(|e| e.class.clone()).filter(|c| !c.is_empty()).or_else(|| classes.get(name).cloned()).unwrap_or_else(|| slug(name));
    // Match the characters xml_attr actually emits, before escaping either output context.
    let class = class.chars().filter(|c| valid_xml_char(*c) && !('\u{E000}'..='\u{E1FF}').contains(c)).collect();
    (tag, class)
}

/// Each class token must match on the same element. Escape CSS identifier punctuation and
/// leading digits without changing the class written into XHTML.
fn class_selector(tag: &str, class: &str) -> Option<String> {
    let mut tokens = class.split_ascii_whitespace().peekable();
    // A nonempty but whitespace-only custom class has no token to select. Never emit a bare
    // element selector, which would apply this style to unrelated paragraphs or character runs.
    tokens.peek()?;
    let mut selector = tag.to_string();
    for token in tokens {
        selector.push('.');
        for (i, c) in token.chars().enumerate() {
            let leading_digit = c.is_ascii_digit() && (i == 0 || i == 1 && token.starts_with('-'));
            if !leading_digit && token != "-" && (c.is_ascii_alphanumeric() || matches!(c, '-' | '_') || !c.is_ascii()) {
                selector.push(c);
            } else {
                let _ = write!(selector, "\\{:x} ", c as u32);
            }
        }
    }
    Some(selector)
}

/// Nesting limit for stories inside stories (table cells, footnotes, tables in cells), so a deeply
/// nested or cyclic document can't recurse without bound.
const MAX_DEPTH: usize = 8;

/// Writes stories with their tables and notes, numbering notes across the stories of one export.
struct StoryWriter<'a> {
    doc: &'a Document,
    pc: BTreeMap<&'a str, String>,
    cc: BTreeMap<&'a str, String>,
    /// EPUB structural semantics (`epub:type`); plain HTML doesn't declare the namespace.
    epub: bool,
    /// Notes written so far, for unique element ids.
    footnote_ids: u32,
    endnote_ids: u32,
    /// Number shown on the next footnote and endnote of the current story.
    next_footnote: u32,
    next_endnote: u32,
    /// Endnotes met so far: (number, list item markup).
    endnotes: Vec<(u32, String)>,
}

impl<'a> StoryWriter<'a> {
    fn new(doc: &'a Document, epub: bool) -> Self {
        StoryWriter {
            doc,
            pc: para_classes(&doc.styles),
            cc: char_classes(&doc.styles),
            epub,
            footnote_ids: 0,
            endnote_ids: 0,
            next_footnote: 1,
            next_endnote: 1,
            endnotes: Vec::new(),
        }
    }

    fn epub_type(&self, t: &str) -> String {
        if self.epub { format!(" epub:type=\"{t}\"") } else { String::new() }
    }

    /// One document story: paragraphs, tables, and each footnote after the paragraph that refers
    /// to it.
    /// With `ids`, every heading (`h1`–`h6`) gets an `id`, `{prefix}-p{paragraph index}` (the
    /// targets of the table of contents), and each `h1` starts a chapter. Each anchored object is
    /// written where it is anchored as the markup `object` returns for it.
    fn story(&mut self, sid: StoryId, ids: Option<&str>, object: &mut dyn FnMut(&designcraft_doc::AnchoredObject) -> String) -> String {
        let Some(st) = self.doc.story(sid) else { return String::new() };
        self.next_footnote = self.doc.footnote_start(sid);
        self.next_endnote = self.doc.endnote_start(sid);
        let mut out = String::new();
        self.paragraphs(st, 0, "", ids, object, &mut out);
        out
    }

    /// `st`'s paragraphs. `lead` (a note's number) starts the first paragraph. Consecutive `li`
    /// paragraphs share one list (`ol` for a numbered style, `ul` for a bulleted one, an unmarked
    /// `ul` when the style has no list type of its own, e.g. typed bullets), and consecutive `pre`
    /// paragraphs of one style one block, their lines separated by newlines.
    fn paragraphs(
        &mut self,
        st: &Story,
        depth: usize,
        lead: &str,
        ids: Option<&str>,
        object: &mut dyn FnMut(&designcraft_doc::AnchoredObject) -> String,
        out: &mut String,
    ) {
        let doc = self.doc;
        let mut lead = lead;
        let mut list: Option<&str> = None;
        let mut pre: Option<(String, bool)> = None;
        // Closes an open `pre` block and list, before anything that can't go inside them.
        fn close(out: &mut String, list: &mut Option<&str>, pre: &mut Option<(String, bool)>) {
            if pre.take().is_some() {
                out.push_str("</pre>\n");
            }
            if let Some(l) = list.take() {
                let _ = writeln!(out, "</{}>", l.split(' ').next().unwrap_or(l));
            }
        }
        for (pi, (r, pf)) in st.para_ranges().into_iter().zip(&st.paras).enumerate() {
            let Some(text) = st.text.get(r.clone()) else { continue };
            let table = pf.table.and_then(|id| st.tables.get(&id)).filter(|_| text.contains(story::TABLE_ANCHOR));
            let only_table = table.is_some() && text.chars().all(|c| c == story::TABLE_ANCHOR || c.is_whitespace());
            if !only_table || !lead.is_empty() {
                // Export Tagging: the style's element and class.
                let (tag, class) = export_mapping(&doc.styles, &pf.style, false, &self.pc);
                let props = doc.styles.resolve_para(pf).0;
                let rtl = props.direction == designcraft_doc::TextDirection::RightToLeft;
                let continues_pre = tag == "pre" && lead.is_empty() && pre.as_ref() == Some(&(class.clone(), rtl));
                if !continues_pre && pre.take().is_some() {
                    out.push_str("</pre>\n");
                }
                let list_tag = (tag == "li").then_some(match props.list_type {
                    designcraft_doc::ListType::Numbers => "ol",
                    designcraft_doc::ListType::Bullets => "ul",
                    designcraft_doc::ListType::None => "ul style=\"list-style:none\"",
                });
                if list != list_tag {
                    if let Some(l) = list {
                        let _ = writeln!(out, "</{}>", l.split(' ').next().unwrap_or(l));
                    }
                    if let Some(l) = list_tag {
                        let _ = writeln!(out, "<{l}>");
                    }
                    list = list_tag;
                }
                if continues_pre {
                    out.push('\n');
                } else {
                    let id = match ids {
                        Some(prefix) if heading_level(tag).is_some() => format!(" id=\"{prefix}-p{pi}\""),
                        _ => String::new(),
                    };
                    if ids.is_some() && heading_level(tag) == Some(1) {
                        out.push_str(CHAPTER_MARK);
                    }
                    let _ = write!(out, "<{tag}{id} class=\"{}\"{}>", xml_attr(&class), if rtl { " dir=\"rtl\"" } else { "" });
                }
                out.push_str(lead);
                if lead.is_empty() && text.trim().is_empty() {
                    out.push_str("&#160;");
                }
                lead = "";
                let mut after = String::new();
                if !only_table {
                    self.runs(st, r, depth, object, out, &mut after);
                }
                if tag == "pre" {
                    // Closed when the next paragraph isn't a line of the same block.
                    pre = Some((class, rtl));
                } else {
                    let _ = writeln!(out, "</{tag}>");
                }
                if !after.is_empty() {
                    close(out, &mut list, &mut pre);
                    out.push_str(&after);
                }
            }
            if let Some(t) = table
                && depth < MAX_DEPTH
            {
                close(out, &mut list, &mut pre);
                self.table(t, depth + 1, object, out);
            }
        }
        close(out, &mut list, &mut pre);
    }

    /// The runs of paragraph `para` of `st`; footnotes go to `after`.
    fn runs(
        &mut self,
        st: &Story,
        para: std::ops::Range<usize>,
        depth: usize,
        object: &mut dyn FnMut(&designcraft_doc::AnchoredObject) -> String,
        out: &mut String,
        after: &mut String,
    ) {
        let doc = self.doc;
        for (rr, f) in st.runs() {
            let a = rr.start.max(para.start);
            let b = rr.end.min(para.end);
            if a >= b {
                continue;
            }
            // Hidden conditional text isn't exported.
            if f.over.conditions.as_deref().is_some_and(|c| doc.conditions_hide(c)) || f.over.change == Some(designcraft_doc::ChangeMark::Deleted) {
                continue;
            }
            let Some(raw) = st.text.get(a..b) else { continue };
            let mut t = String::new();
            let mut seg_start = 0;
            // Each anchored object's markup in place of its mark.
            let mut mark = st.text.get(..a).map_or(0, |t| t.matches(designcraft_doc::OBJECT_MARK).count());
            for (i, c) in raw.char_indices() {
                if c != FOOTNOTE_REF && c != ENDNOTE_REF && c != designcraft_doc::OBJECT_MARK {
                    continue;
                }
                t.push_str(&run_text(doc, raw.get(seg_start..i).unwrap_or_default()));
                seg_start = i + c.len_utf8();
                if c == designcraft_doc::OBJECT_MARK {
                    if let Some(o) = st.objects.get(mark) {
                        t.push_str(&object(o));
                    }
                    mark += 1;
                } else if depth < MAX_DEPTH {
                    if c == FOOTNOTE_REF {
                        self.footnote(st, a + i, depth, object, &mut t, after);
                    } else {
                        self.endnote(st, a + i, depth, object, &mut t);
                    }
                }
            }
            t.push_str(&run_text(doc, raw.get(seg_start..).unwrap_or_default()));
            let (ctag, cls) = if f.style != story::NO_CHAR_STYLE {
                let (tag, class) = export_mapping(&doc.styles, &f.style, true, &self.cc);
                (tag, format!(" class=\"{}\"", xml_attr(&class)))
            } else {
                ("span", String::new())
            };
            let style = override_css(doc, &f.over);
            let style_attr = if style.is_empty() { String::new() } else { format!(" style=\"{}\"", xml_attr(&style)) };
            if cls.is_empty() && style_attr.is_empty() {
                out.push_str(&t);
            } else {
                let _ = write!(out, "<{ctag}{cls}{style_attr}>{t}</{ctag}>");
            }
        }
    }

    /// The footnote whose reference is at byte `pos` of `st`: a note reference in `out`, the note
    /// itself in `after`.
    fn footnote(
        &mut self,
        st: &Story,
        pos: usize,
        depth: usize,
        object: &mut dyn FnMut(&designcraft_doc::AnchoredObject) -> String,
        out: &mut String,
        after: &mut String,
    ) {
        let Some(note) = st.notes.get(st.notes_before(pos)) else { return };
        let doc = self.doc;
        let o = &doc.footnote_options;
        let n = self.next_footnote;
        self.next_footnote = n.saturating_add(1);
        self.footnote_ids = self.footnote_ids.saturating_add(1);
        let id = self.footnote_ids;
        let _ = write!(
            out,
            "<sup><a{} role=\"doc-noteref\" id=\"fnref-{id}\" href=\"#fn-{id}\">{}</a></sup>",
            self.epub_type("noteref"),
            xml_text(&o.label(n, false))
        );
        let lead = format!("<a href=\"#fnref-{id}\">{}</a> ", xml_text(&o.label(n, true)));
        let mut body = String::new();
        self.paragraphs(&note.text, depth + 1, &lead, None, object, &mut body);
        let _ = write!(after, "<aside{} role=\"doc-footnote\" id=\"fn-{id}\">\n{body}</aside>\n", self.epub_type("footnote"));
    }

    /// The endnote whose reference is at byte `pos` of `st`: a note reference in `out`; the note
    /// joins the endnotes section.
    fn endnote(
        &mut self,
        st: &Story,
        pos: usize,
        depth: usize,
        object: &mut dyn FnMut(&designcraft_doc::AnchoredObject) -> String,
        out: &mut String,
    ) {
        let Some(note) = st.endnotes.get(st.endnotes_before(pos)) else { return };
        let n = self.next_endnote;
        self.next_endnote = n.saturating_add(1);
        self.endnote_ids = self.endnote_ids.saturating_add(1);
        let id = self.endnote_ids;
        let label = xml_text(&self.doc.endnote_options.label(n));
        let _ = write!(out, "<sup><a{} role=\"doc-noteref\" id=\"enref-{id}\" href=\"#en-{id}\">{label}</a></sup>", self.epub_type("noteref"));
        let mut body = String::new();
        self.paragraphs(&note.text, depth + 1, &format!("<a href=\"#enref-{id}\">{label}</a> "), None, object, &mut body);
        let item = format!("<li{} id=\"en-{id}\">\n{body}</li>\n", self.epub_type("endnote"));
        self.endnotes.push((n, item));
    }

    /// The endnotes met so far, in number order, as one section (empty when there are none).
    fn endnotes_section(&mut self) -> String {
        if self.endnotes.is_empty() {
            return String::new();
        }
        self.endnotes.sort_by_key(|(n, _)| *n);
        let doc = self.doc;
        let o = &doc.endnote_options;
        let mut out = format!("<section{} role=\"doc-endnotes\" id=\"endnotes\">\n", self.epub_type("endnotes"));
        if !o.heading.trim().is_empty() {
            let (tag, class) = export_mapping(&doc.styles, &o.heading_style, false, &self.pc);
            let _ = writeln!(out, "<{tag} class=\"{}\">{}</{tag}>", xml_attr(&class), xml_text(&o.heading));
        }
        out.push_str("<ol style=\"list-style: none; padding-left: 0\">\n");
        for (_, item) in &self.endnotes {
            out.push_str(item);
        }
        out.push_str("</ol>\n</section>\n");
        out
    }

    /// A table: header rows as `thead`, footer rows as `tfoot`, merged cells as row and column
    /// spans (kept within their row group).
    fn table(&mut self, t: &Table, depth: usize, object: &mut dyn FnMut(&designcraft_doc::AnchoredObject) -> String, out: &mut String) {
        let (nr, nc) = (t.nrows(), t.ncols());
        if nr.checked_mul(nc) != Some(t.cells.len()) {
            return;
        }
        let (h, f) = (t.header_rows(), t.footer_rows());
        let body_end = nr.saturating_sub(f).max(h);
        let mut covered = vec![false; t.cells.len()];
        out.push_str("<table>\n");
        for (rows, group, cell_tag) in [(0..h, "thead", "th"), (h..body_end, "tbody", "td"), (body_end..nr, "tfoot", "td")] {
            if rows.is_empty() {
                continue;
            }
            let _ = writeln!(out, "<{group}>");
            for r in rows.clone() {
                out.push_str("<tr>");
                for c in 0..nc {
                    let i = r * nc + c;
                    if covered.get(i).copied().unwrap_or(true) {
                        continue;
                    }
                    let Some(cell) = t.cells.get(i) else { continue };
                    let rs = (cell.row_span.max(1) as usize).min(rows.end - r);
                    let cs = (cell.col_span.max(1) as usize).min(nc - c);
                    for rr in r..r + rs {
                        for cc in c..c + cs {
                            if let Some(slot) = covered.get_mut(rr * nc + cc) {
                                *slot = true;
                            }
                        }
                    }
                    let _ = write!(out, "<{cell_tag}");
                    if rs > 1 {
                        let _ = write!(out, " rowspan=\"{rs}\"");
                    }
                    if cs > 1 {
                        let _ = write!(out, " colspan=\"{cs}\"");
                    }
                    out.push('>');
                    self.paragraphs(&cell.text, depth, "", None, object, out);
                    let _ = writeln!(out, "</{cell_tag}>");
                }
                out.push_str("</tr>\n");
            }
            let _ = writeln!(out, "</{group}>");
        }
        out.push_str("</table>\n");
    }
}

/// Run text with variables replaced by their values.
fn run_text(doc: &Document, raw: &str) -> String {
    if raw.chars().any(|c| designcraft_doc::vars::var_index(c).is_some()) {
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
    }
}

/// One story as XHTML: paragraphs, tables, and each footnote after the paragraph that refers to
/// it (anchored objects left out). Endnote references link to the endnotes section that the
/// full export ends with.
pub fn story_html(doc: &Document, sid: StoryId) -> String {
    StoryWriter::new(doc, true).story(sid, None, &mut |_| String::new())
}

/// Where the EPUB body splits into chapter files: before each `h1`.
const CHAPTER_MARK: &str = "<!--dc:chapter-->";

/// The body split at its [`CHAPTER_MARK`]s into chapter bodies, each with its sections closed
/// and the one a split goes through reopened in the next; a part with nothing but the start of a
/// section (a story that opens with its `h1`) moves that start into the next part.
fn split_chapters(body: &str) -> Vec<String> {
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
fn link_across_chapters(chapter: &str, own: &str, files: &BTreeMap<String, String>) -> String {
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
fn chapter_file(k: usize) -> String {
    if k == 0 { "content.xhtml".to_string() } else { format!("content{}.xhtml", k + 1) }
}

/// The level of a heading element (`h1` → 1 … `h6` → 6).
fn heading_level(tag: &str) -> Option<u8> {
    tag.strip_prefix('h').and_then(|n| n.parse::<u8>().ok()).filter(|n| (1..=6).contains(n))
}

/// Table of contents entries (target, label, level): the `h1`–`h3` paragraphs of the stories in
/// reading order, or, in a document without headings, each story's first paragraph (level 1).
fn toc_entries(doc: &Document, order: &[Block]) -> Vec<(String, String, u8)> {
    let pc = para_classes(&doc.styles);
    let label = |t: &str| -> String { t.chars().filter(|c| !('\u{E000}'..='\u{E0FF}').contains(c)).take(80).collect::<String>().trim().to_string() };
    let mut headings = Vec::new();
    let mut firsts = Vec::new();
    for (k, b) in order.iter().enumerate() {
        let Block::Story(sid) = b else { continue };
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
fn nav_list(entries: &[(String, String, u8)], files: &BTreeMap<String, String>) -> String {
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

/// Package path of an asset: `{dir}/{name}-{id}.{ext}`, the name from its original file name
/// (lowercase ASCII, so the package stays readable) and the id keeping two assets of the same
/// name apart.
fn asset_path(dir: &str, a: &designcraft_doc::Asset, mime: &str) -> String {
    let stem = a.name.rsplit(['/', '\\']).next().unwrap_or("");
    let stem = stem.rsplit_once('.').map_or(stem, |(s, _)| s);
    let name: String = stem.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect();
    let name: String = name.split('-').filter(|p| !p.is_empty()).collect::<Vec<_>>().join("-").chars().take(40).collect();
    if name.is_empty() { format!("{dir}/{}.{}", a.id.0, ext(mime)) } else { format!("{dir}/{name}-{}.{}", a.id.0, ext(mime)) }
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

/// The markup for one graphic item: an `<img>`, or an HTML5 player for video and sound (with
/// `true`), its file registered in `images` under the name `img_src` is given. `None` for an
/// item that isn't a graphic or whose asset has no data.
fn graphic_html<'a>(
    doc: &'a Document,
    it: &Item,
    images: &mut BTreeMap<String, (&'a str, &'a [u8])>,
    img_src: &dyn Fn(&str, &str, &[u8]) -> String,
) -> Option<(String, bool)> {
    let Content::Graphic(g) = &it.content else { return None };
    let a = doc.assets.get(&g.asset)?;
    if a.data.is_empty() {
        return None;
    }
    let mime = safe_mime(&a.mime);
    // Video and sound: an HTML5 player (with the poster, when there is one).
    if let Some(kind) = designcraft_doc::media_kind(mime) {
        let name = asset_path("media", a, mime);
        images.insert(name.clone(), (mime, a.data.as_slice()));
        let m = it.media.clone().unwrap_or_default();
        let mut attrs = String::new();
        for (on, attr) in [(m.controls, " controls=\"controls\""), (m.play_on_page_load, " autoplay=\"autoplay\""), (m.looping, " loop=\"loop\"")] {
            if on {
                attrs.push_str(attr);
            }
        }
        if let Some(pa) = m.poster.and_then(|p| doc.assets.get(&p)) {
            let poster_mime = safe_mime(&pa.mime);
            let pn = asset_path("images", pa, poster_mime);
            images.insert(pn.clone(), (poster_mime, pa.data.as_slice()));
            if kind == "video" {
                attrs.push_str(&format!(" poster=\"{}\"", xml_attr(&img_src(&pn, poster_mime, &pa.data))));
            }
        }
        let markup = format!("<{kind} src=\"{}\"{attrs}>{}</{kind}>", xml_attr(&img_src(&name, mime, &a.data)), xml_text(&a.name));
        return Some((markup, true));
    }
    let name = asset_path("images", a, mime);
    images.insert(name.clone(), (mime, a.data.as_slice()));
    // Alt text from Object Export Options; without one the image counts as decorative
    // (`alt=""`): a file name read aloud helps nobody.
    let alt = it.alt_text.as_str();
    Some((format!("<img src=\"{}\" alt=\"{}\"/>", xml_attr(&img_src(&name, mime, &a.data)), xml_attr(alt)), false))
}

/// The content in reading order as HTML sections and figures; `img_src` names each image
/// (a package path, or a data URI for single-file HTML). Returns (body, images by name, table of
/// contents).
#[allow(clippy::type_complexity)]
fn body_html<'a>(
    doc: &'a Document,
    img_src: &dyn Fn(&str, &str, &[u8]) -> String,
    epub: bool,
) -> (String, BTreeMap<String, (&'a str, &'a [u8])>, Vec<(String, String, u8)>) {
    let order = reading_order(doc);
    let mut writer = StoryWriter::new(doc, epub);
    let mut body = String::new();
    let mut images: BTreeMap<String, (&str, &[u8])> = BTreeMap::new();
    let mut toc = toc_entries(doc, &order);
    for (k, b) in order.iter().enumerate() {
        match b {
            // The endnote frame's text is generated from the endnotes, which end the body as a
            // linked section instead.
            Block::Story(sid) if doc.endnote_story == Some(*sid) => {}
            Block::Story(sid) => {
                let anchor = format!("s{k}");
                // Anchored objects go where they are anchored: their graphics in the text, an
                // above-line object as a block of its own.
                let mut object = |o: &designcraft_doc::AnchoredObject| {
                    let mut out = String::new();
                    o.item.walk(&mut |it| {
                        if let Some((markup, _)) = graphic_html(doc, it, &mut images, img_src) {
                            out.push_str(&markup);
                        }
                    });
                    match &o.position {
                        designcraft_doc::AnchorPosition::AboveLine { align, .. } if !out.is_empty() => {
                            let align = match align {
                                designcraft_doc::anchored::AnchorAlign::Left => "left",
                                designcraft_doc::anchored::AnchorAlign::Center => "center",
                                designcraft_doc::anchored::AnchorAlign::Right => "right",
                            };
                            format!("<span class=\"anchored\" style=\"display:block;text-align:{align}\">{out}</span>")
                        }
                        _ => out,
                    }
                };
                let _ = write!(body, "<section id=\"{anchor}\">\n{}</section>\n", writer.story(*sid, epub.then_some(anchor.as_str()), &mut object));
            }
            Block::Image(iid) => {
                let Some(it) = doc.item(*iid) else { continue };
                let Some((markup, media)) = graphic_html(doc, it, &mut images, img_src) else { continue };
                if media {
                    let _ = writeln!(body, "<figure>{markup}</figure>");
                    continue;
                }
                // Object Export Options: alignment and a page break before.
                let eo = &it.export_options;
                let mut css = String::new();
                if matches!(eo.align.as_str(), "left" | "center" | "right") {
                    css.push_str(&format!("text-align:{};", eo.align));
                }
                if eo.page_break_before {
                    css.push_str("page-break-before:always;break-before:page;");
                }
                let style = if css.is_empty() { String::new() } else { format!(" style=\"{}\"", xml_attr(&css)) };
                let _ = writeln!(body, "<figure{style}>{markup}</figure>");
            }
        }
    }
    let endnotes = writer.endnotes_section();
    if !endnotes.is_empty() {
        let heading = &doc.endnote_options.heading;
        if !heading.trim().is_empty() {
            toc.push(("endnotes".into(), heading.clone(), 1));
        }
        body.push_str(&endnotes);
    }
    (body, images, toc)
}

/// Placed images (not video or sound) without alt text, by asset name: they are exported as
/// decorative (`alt=""`), which an export can report.
pub fn images_without_alt(doc: &Document) -> Vec<String> {
    let mut out = Vec::new();
    for b in reading_order(doc) {
        let Block::Image(iid) = b else { continue };
        let Some(it) = doc.item(iid) else { continue };
        let Content::Graphic(g) = &it.content else { continue };
        let Some(a) = doc.assets.get(&g.asset) else { continue };
        if !a.data.is_empty() && designcraft_doc::media_kind(safe_mime(&a.mime)).is_none() && it.alt_text.trim().is_empty() {
            out.push(a.name.clone());
        }
    }
    out
}

/// Options for [`export_html`].
#[derive(Clone, Debug, Default)]
pub struct HtmlOptions {
    pub title: Option<String>,
    pub language: Option<String>,
    /// Generic CSS family per font family (see [`EpubOptions::generic_families`]).
    pub generic_families: BTreeMap<String, String>,
}

/// File › Export › HTML: one self-contained page (styles inline, images as data URIs) with the
/// document's stories and graphics in reading order.
pub fn export_html(doc: &Document, opts: &HtmlOptions) -> String {
    let title = opts.title.clone().unwrap_or_else(|| doc.title.clone());
    let language = opts.language.as_deref().unwrap_or("en");
    let lang = language_tag(language);
    let (body, _, _) = body_html(doc, &|_, mime, data| format!("data:{mime};base64,{}", base64(data)), false);
    format!(
        "<!DOCTYPE html>\n<html lang=\"{lang}\">\n<head>\n<meta charset=\"utf-8\">\n<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<meta name=\"generator\" content=\"DesignCraft\">\n<title>{}</title>\n<style>\nbody {{ max-width: 40em; margin: 2em auto; padding: 0 1em; }}\nfigure {{ margin: 1em 0; }} figure img {{ max-width: 100%; height: auto; }}\n{}</style>\n</head>\n<body>\n{body}</body>\n</html>\n",
        xml_text(&title),
        stylesheet_with(doc, &opts.generic_families, &[])
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
    let id = identifier(doc, opts, &title);
    let modified = modified(doc, opts);
    let (body, images, toc) = body_html(doc, &|name, _, _| name.to_string(), true);
    let lang = language_tag(&opts.language);
    // One file per chapter (split before each h1), so e-readers needn't load the whole book.
    let chapters: Vec<String> = split_chapters(&body)
        .into_iter()
        .map(|part| {
            format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE html>\n<html xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:epub=\"http://www.idpf.org/2007/ops\" xml:lang=\"{lang}\" lang=\"{lang}\">\n<head><meta charset=\"utf-8\"/><title>{}</title><link rel=\"stylesheet\" type=\"text/css\" href=\"style.css\"/></head>\n<body>\n{part}</body>\n</html>\n",
                xml_text(&title)
            )
        })
        .collect();
    // The file each navigation target (an element id) ended up in.
    let mut files: BTreeMap<String, String> = BTreeMap::new();
    for (k, c) in chapters.iter().enumerate() {
        for rest in c.split(" id=\"").skip(1) {
            if let Some(id) = rest.split('"').next() {
                files.entry(id.to_string()).or_insert_with(|| chapter_file(k));
            }
        }
    }
    // Note links whose target ended up in another file (endnotes end the book) point at it.
    let chapters: Vec<String> = chapters.iter().enumerate().map(|(k, c)| link_across_chapters(c, &chapter_file(k), &files)).collect();
    let mut nav_items = nav_list(&toc, &files);
    if nav_items.is_empty() {
        nav_items = format!("<li><a href=\"content.xhtml\">{}</a></li>", xml_text(&title));
    }
    let nav = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE html>\n<html xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:epub=\"http://www.idpf.org/2007/ops\" xml:lang=\"{lang}\">\n<head><meta charset=\"utf-8\"/><title>{}</title></head>\n<body><nav epub:type=\"toc\" id=\"toc\"><h1>{}</h1><ol>{nav_items}</ol></nav></body>\n</html>\n",
        xml_text(&title),
        xml_text(opts.toc_title.as_deref().filter(|t| !t.trim().is_empty()).unwrap_or(contents_title(&opts.language)))
    );
    let mut manifest = String::from(
        "<item id=\"nav\" href=\"nav.xhtml\" media-type=\"application/xhtml+xml\" properties=\"nav\"/>\n<item id=\"css\" href=\"style.css\" media-type=\"text/css\"/>\n",
    );
    let mut spine = String::new();
    for k in 0..chapters.len() {
        let item = if k == 0 { "content".to_string() } else { format!("content{}", k + 1) };
        let _ = writeln!(manifest, "<item id=\"{item}\" href=\"{}\" media-type=\"application/xhtml+xml\"/>", chapter_file(k));
        let _ = writeln!(spine, "<itemref idref=\"{item}\"/>");
    }
    for (k, (name, (mime, _))) in images.iter().enumerate() {
        let _ = writeln!(manifest, "<item id=\"img{k}\" href=\"{}\" media-type=\"{}\"/>", xml_attr(name), xml_attr(mime));
    }
    for (k, f) in opts.fonts.iter().enumerate() {
        let (name, mime) = font_file(k, f);
        let _ = writeln!(manifest, "<item id=\"font{k}\" href=\"{}\" media-type=\"{mime}\"/>", xml_attr(&name));
    }
    // Cover: an image page first in reading order.
    if let Some(c) = &opts.cover {
        let (name, mime) = cover_file(c);
        let _ = writeln!(
            manifest,
            "<item id=\"cover-img\" href=\"{name}\" media-type=\"{mime}\" properties=\"cover-image\"/>\n<item id=\"cover\" href=\"cover.xhtml\" media-type=\"application/xhtml+xml\"/>"
        );
    }
    let cover_spine = if opts.cover.is_some() { "<itemref idref=\"cover\"/>\n" } else { "" };
    let author = opts.author.as_deref().map(|a| format!("<dc:creator>{}</dc:creator>", xml_text(a))).unwrap_or_default();
    let opf = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<package xmlns=\"http://www.idpf.org/2007/opf\" version=\"3.0\" unique-identifier=\"bookid\" xml:lang=\"{lang}\">\n<metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\">\n<dc:identifier id=\"bookid\">{}</dc:identifier>\n<dc:title>{}</dc:title>\n<dc:language>{lang}</dc:language>{author}\n<meta property=\"dcterms:modified\">{modified}</meta>\n<meta name=\"generator\" content=\"DesignCraft\"/>\n</metadata>\n<manifest>\n{manifest}</manifest>\n<spine>\n{cover_spine}{spine}</spine>\n</package>\n",
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
            ("OEBPS/style.css", stylesheet_with(doc, &opts.generic_families, &opts.fonts).as_bytes()),
        ] {
            z.start_file(name, deflate).map_err(zerr)?;
            z.write_all(data).map_err(io)?;
        }
        for (k, c) in chapters.iter().enumerate() {
            z.start_file(format!("OEBPS/{}", chapter_file(k)), deflate).map_err(zerr)?;
            z.write_all(c.as_bytes()).map_err(io)?;
        }
        for (name, (_, data)) in &images {
            z.start_file(format!("OEBPS/{name}"), stored).map_err(zerr)?;
            z.write_all(data).map_err(io)?;
        }
        for (k, f) in opts.fonts.iter().enumerate() {
            z.start_file(format!("OEBPS/{}", font_file(k, f).0), deflate).map_err(zerr)?;
            z.write_all(&f.data).map_err(io)?;
        }
        if let Some(c) = &opts.cover {
            z.start_file(format!("OEBPS/{}", cover_file(c).0), stored).map_err(zerr)?;
            z.write_all(c).map_err(io)?;
            let xhtml = format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE html>\n<html xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:epub=\"http://www.idpf.org/2007/ops\" xml:lang=\"{lang}\">\n<head><meta charset=\"utf-8\"/><title>{}</title><style>body{{margin:0;text-align:center}} img{{max-width:100%;max-height:100vh}}</style></head>\n<body epub:type=\"cover\"><img src=\"{}\" alt=\"{}\"/></body>\n</html>\n",
                xml_text(&title),
                cover_file(c).0,
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
    use designcraft_doc::{Asset, AssetId, Content, Graphic, Item, ItemId, ParaFormat, Shape, SpreadRef, Story, StoryId, Table};
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

    /// #766: anchored graphics are exported where they are anchored (inline in the text, an
    /// above-line object as a block of its own), and their images go into the package.
    #[test]
    fn anchored_objects_are_exported_in_place() {
        let mut d = Document::new(&NewDocument::default());
        let lid = d.default_layer();
        let (_, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 36.0, 300.0, 200.0), lid, "AB\nCD", ParaFormat::default()).unwrap();
        let aid = AssetId(d.alloc());
        d.assets.insert(
            aid,
            Arc::new(Asset {
                page: 0,
                id: aid,
                name: "dot.png".into(),
                mime: "image/png".into(),
                link: None,
                data: Arc::new(vec![7]),
                pixels: Some((1, 1)),
            }),
        );
        let graphic = |d: &mut Document, alt: &str| {
            let mut item = Item::new(ItemId(d.alloc()), lid, Shape::Rectangle, shapes::rectangle(Rect::new(0.0, 0.0, 10.0, 10.0)));
            item.alt_text = alt.into();
            item.content = Content::Graphic(Graphic {
                asset: aid,
                size: (1.0, 1.0),
                xf: Affine::IDENTITY,
                auto_fit: Default::default(),
                fit_align: 4,
                crop: [0.0; 4],
            });
            item
        };
        let inline = designcraft_doc::AnchoredObject::new(graphic(&mut d, "inline"), designcraft_doc::AnchorPosition::Inline { y_offset: 0.0 });
        let above = designcraft_doc::AnchoredObject::new(
            graphic(&mut d, "above"),
            designcraft_doc::AnchorPosition::AboveLine { align: designcraft_doc::anchored::AnchorAlign::Center, space_before: 0.0, space_after: 0.0 },
        );
        let st = d.story_mut(sid).unwrap();
        st.insert_object(1, inline);
        // Byte offsets: "A" (1), the mark (3), "B\nC" (3): the second object goes between C and D.
        st.insert_object(7, above);
        let bytes = export_epub(&d, &EpubOptions::default()).unwrap();
        let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let mut html = String::new();
        z.by_name("OEBPS/content.xhtml").unwrap().read_to_string(&mut html).unwrap();
        let img = |alt: &str| format!("<img src=\"images/dot-{}.png\" alt=\"{alt}\"/>", aid.0);
        assert!(html.contains(&format!("A{}B</p>", img("inline"))), "{html}");
        assert!(html.contains(&format!("C<span class=\"anchored\" style=\"display:block;text-align:center\">{}</span>D", img("above"))), "{html}");
        assert!(z.by_name(&format!("OEBPS/images/dot-{}.png", aid.0)).is_ok(), "the image is in the package");
        let page = export_html(&d, &HtmlOptions::default());
        assert!(page.contains("alt=\"inline\"") && page.contains("alt=\"above\""), "{page}");
        // story_html on its own still leaves objects out.
        assert!(!story_html(&d, sid).contains("<img"));
    }

    fn st_typed(d: &mut Document, sid: StoryId) {
        if let Some(st) = d.story_mut(sid) {
            st.paras[8].style = "Typed".into();
        }
    }

    /// #767: consecutive `li` paragraphs share one list (`ol` for a numbered style), and
    /// consecutive `pre` paragraphs of one style one block with their lines on separate lines.
    #[test]
    fn lists_and_preformatted_blocks_are_grouped() {
        let mut d = Document::new(&NewDocument::default());
        let lid = d.default_layer();
        for (name, tag, list) in [
            ("Bullet", "li", Some(designcraft_doc::ListType::Bullets)),
            ("Number", "li", Some(designcraft_doc::ListType::Numbers)),
            ("Typed", "li", None),
            ("Code", "pre", None),
        ] {
            let para = designcraft_doc::ParaAttrs { list_type: list, ..Default::default() };
            d.styles_mut().paragraph.push(designcraft_doc::ParagraphStyle {
                name: name.into(),
                based_on: None,
                next_style: None,
                para,
                chars: CharAttrs::default(),
                shortcut: String::new(),
            });
            d.styles_mut().export_tags.insert(format!("p:{name}"), designcraft_doc::ExportTag { tag: tag.into(), class: String::new() });
        }
        let text = "Intro\nfirst\nsecond\none\ntwo\nx = 1\ny = 2\nEnd\n• typed";
        let (_, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 36.0, 300.0, 300.0), lid, text, ParaFormat::default()).unwrap();
        let st = d.story_mut(sid).unwrap();
        for (i, style) in ["Bullet", "Bullet", "Number", "Number", "Code", "Code"].into_iter().enumerate() {
            st.paras[i + 1].style = style.into();
        }
        let html = story_html(&d, sid);
        let li = |c: &str, t: &str| format!("<li class=\"{c}\">{t}</li>");
        assert!(
            html.contains(&format!(
                "<ul>\n{}\n{}\n</ul>\n<ol>\n{}\n{}\n</ol>\n",
                li("bullet", "first"),
                li("bullet", "second"),
                li("number", "one"),
                li("number", "two")
            )),
            "{html}"
        );
        assert!(html.contains("<pre class=\"code\">x = 1\ny = 2</pre>\n<p class=\"basic-paragraph\">End</p>"), "{html}");
        assert_eq!(html.matches("<pre").count(), 1, "{html}");
        // A style without a list type of its own (typed bullets) gets no second marker.
        st_typed(&mut d, sid);
        let html = story_html(&d, sid);
        assert!(html.contains("<ul style=\"list-style:none\">\n<li class=\"typed\">• typed</li>\n</ul>\n"), "{html}");
    }

    /// #769, #770: the navigation lists the h1–h3 headings (nested by level) and links to them,
    /// with its title in the book's language; without headings, each story's first paragraph.
    #[test]
    fn navigation_follows_the_headings() {
        let mut d = Document::new(&NewDocument { pages: 2, ..Default::default() });
        let lid = d.default_layer();
        for (name, tag) in [("Chapter", "h1"), ("Section", "h2")] {
            d.styles_mut().paragraph.push(designcraft_doc::ParagraphStyle {
                name: name.into(),
                based_on: None,
                next_style: None,
                para: Default::default(),
                chars: CharAttrs::default(),
                shortcut: String::new(),
            });
            d.styles_mut().export_tags.insert(format!("p:{name}"), designcraft_doc::ExportTag { tag: tag.into(), class: String::new() });
        }
        let text = "One\nBody\nOne point one\nMore\nTwo\nText";
        let (_, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 36.0, 300.0, 300.0), lid, text, ParaFormat::default()).unwrap();
        let st = d.story_mut(sid).unwrap();
        for (i, style) in [(0, "Chapter"), (2, "Section"), (4, "Chapter")] {
            st.paras[i].style = style.into();
        }
        // A story that starts with body text adds nothing to the table of contents.
        d.add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 400.0, 300.0, 500.0), lid, "Text after the figure.", ParaFormat::default()).unwrap();
        let read = |d: &Document, opts: &EpubOptions, path: &str| {
            let mut z = zip::ZipArchive::new(std::io::Cursor::new(export_epub(d, opts).unwrap())).unwrap();
            let mut s = String::new();
            z.by_name(path).unwrap().read_to_string(&mut s).unwrap();
            s
        };
        let nl = EpubOptions { language: "nl".into(), ..Default::default() };
        let nav = read(&d, &nl, "OEBPS/nav.xhtml");
        let link = |f: &str, t: &str, l: &str| format!("<a href=\"{f}#{t}\">{l}</a>");
        // Each h1 starts a chapter file (#768).
        let want = format!(
            "<h1>Inhoud</h1><ol><li>{}<ol><li>{}</li></ol></li><li>{}</li></ol>",
            link("content.xhtml", "s0-p0", "One"),
            link("content.xhtml", "s0-p2", "One point one"),
            link("content2.xhtml", "s0-p4", "Two")
        );
        assert!(nav.contains(&want), "{nav}");
        assert!(!nav.contains("Text after the figure"), "{nav}");
        let content = read(&d, &nl, "OEBPS/content.xhtml");
        assert!(content.contains("<h1 id=\"s0-p0\" class=\"chapter\">One</h1>") && content.contains("<h2 id=\"s0-p2\""), "{content}");
        let custom = EpubOptions { toc_title: Some("In dit boek".into()), ..Default::default() };
        assert!(read(&d, &custom, "OEBPS/nav.xhtml").contains("<h1>In dit boek</h1>"));
        // Without headings: one entry per story, its first paragraph.
        let mut plain = Document::new(&NewDocument::default());
        let lid = plain.default_layer();
        plain.add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 36.0, 300.0, 100.0), lid, "Opening\nrest", ParaFormat::default()).unwrap();
        let nav = read(&plain, &EpubOptions::default(), "OEBPS/nav.xhtml");
        assert!(nav.contains(&format!("<h1>Contents</h1><ol><li>{}</li></ol>", link("content.xhtml", "s0", "Opening"))), "{nav}");
        // A heading shallower than the one before it but deeper than its parent stays under the
        // parent: h1 A, h3 B, h2 C nest B and C under A.
        let e = |t: &str, l: u8| (t.to_string(), t.to_string(), l);
        let l = |t: &str| link("content.xhtml", t, t);
        assert_eq!(
            nav_list(&[e("A", 1), e("B", 3), e("C", 2), e("D", 1)], &BTreeMap::new()),
            format!("<li>{}<ol><li>{}</li><li>{}</li></ol></li><li>{}</li>", l("A"), l("B"), l("C"), l("D"))
        );
        assert_eq!(
            nav_list(&[e("A", 1), e("B", 2), e("C", 3), e("D", 1)], &BTreeMap::new()),
            format!("<li>{}<ol><li>{}<ol><li>{}</li></ol></li></ol></li><li>{}</li>", l("A"), l("B"), l("C"), l("D"))
        );
    }

    /// #768: the book is split into one file per chapter, before each h1, with well-formed
    /// sections; the spine and manifest list the files in order.
    #[test]
    fn chapters_are_split_at_h1() {
        let mut d = Document::new(&NewDocument { pages: 3, ..Default::default() });
        let lid = d.default_layer();
        d.styles_mut().paragraph.push(designcraft_doc::ParagraphStyle {
            name: "Chapter".into(),
            based_on: None,
            next_style: None,
            para: Default::default(),
            chars: CharAttrs::default(),
            shortcut: String::new(),
        });
        d.styles_mut().export_tags.insert("p:Chapter".into(), designcraft_doc::ExportTag { tag: "h1".into(), class: String::new() });
        // A title page without a heading, then a story that opens with a chapter and has a
        // second one in the middle.
        d.add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 36.0, 300.0, 100.0), lid, "Title page", ParaFormat::default()).unwrap();
        let (_, sid) = d
            .add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 200.0, 300.0, 400.0), lid, "One\nFirst text\nTwo\nSecond text", ParaFormat::default())
            .unwrap();
        let st = d.story_mut(sid).unwrap();
        st.paras[0].style = "Chapter".into();
        st.paras[2].style = "Chapter".into();
        let mut z = zip::ZipArchive::new(std::io::Cursor::new(export_epub(&d, &EpubOptions::default()).unwrap())).unwrap();
        let mut read = |path: &str| {
            let mut s = String::new();
            z.by_name(path).unwrap().read_to_string(&mut s).unwrap();
            s
        };
        let (c1, c2, c3) = (read("OEBPS/content.xhtml"), read("OEBPS/content2.xhtml"), read("OEBPS/content3.xhtml"));
        assert!(c1.contains("Title page") && !c1.contains("One"), "{c1}");
        assert!(c2.contains("<section id=\"s1\">\n<h1 id=\"s1-p0\"") && c2.contains("First text") && !c2.contains("Two"), "{c2}");
        assert!(c3.contains("<section>\n<h1 id=\"s1-p2\""), "{c3}");
        for c in [&c1, &c2, &c3] {
            assert_eq!(c.matches("<section").count(), c.matches("</section>").count(), "{c}");
            assert!(!c.contains(CHAPTER_MARK));
        }
        let opf = read("OEBPS/content.opf");
        assert!(opf.contains("<itemref idref=\"content\"/>\n<itemref idref=\"content2\"/>\n<itemref idref=\"content3\"/>\n</spine>"), "{opf}");
        assert!(opf.contains("<item id=\"content3\" href=\"content3.xhtml\""), "{opf}");
        let nav = read("OEBPS/nav.xhtml");
        assert!(nav.contains("href=\"content2.xhtml#s1-p0\"") && nav.contains("href=\"content3.xhtml#s1-p2\""), "{nav}");
        assert!(z.by_name("OEBPS/content4.xhtml").is_err());
        // HTML stays one page, without the marks.
        let html = export_html(&d, &HtmlOptions::default());
        assert!(!html.contains(CHAPTER_MARK) && html.contains("Second text"));
    }

    /// Note links stay working across chapter files: a link whose target ended up in another
    /// file (endnotes end the book) names that file; one in its own file stays as it is.
    #[test]
    fn note_links_point_at_the_chapter_file_of_their_target() {
        let files: BTreeMap<String, String> =
            [("enref-1", "content.xhtml"), ("fn-1", "content.xhtml"), ("fnref-1", "content.xhtml"), ("en-1", "content2.xhtml")]
                .into_iter()
                .map(|(id, f)| (id.to_string(), f.to_string()))
                .collect();
        let first = "<p>First<a id=\"enref-1\" href=\"#en-1\">1</a><a id=\"fnref-1\" href=\"#fn-1\">*</a></p><aside id=\"fn-1\"><a href=\"#fnref-1\">*</a></aside><a href=\"#gone\">x</a>";
        assert_eq!(
            link_across_chapters(first, "content.xhtml", &files),
            "<p>First<a id=\"enref-1\" href=\"content2.xhtml#en-1\">1</a><a id=\"fnref-1\" href=\"#fn-1\">*</a></p><aside id=\"fn-1\"><a href=\"#fnref-1\">*</a></aside><a href=\"#gone\">x</a>"
        );
        let last = "<li id=\"en-1\"><a href=\"#enref-1\">1</a> End note</li>";
        assert_eq!(link_across_chapters(last, "content2.xhtml", &files), "<li id=\"en-1\"><a href=\"content.xhtml#enref-1\">1</a> End note</li>");
    }

    #[test]
    fn split_chapters_keeps_sections_well_formed() {
        let m = CHAPTER_MARK;
        assert_eq!(split_chapters("<section id=\"s0\">\n<p>a</p>\n</section>\n"), vec!["<section id=\"s0\">\n<p>a</p>\n</section>\n"]);
        assert_eq!(
            split_chapters(&format!("<section id=\"s0\">\n{m}<h1>A</h1>\n</section>\n")),
            vec!["<section id=\"s0\">\n<h1>A</h1>\n</section>\n"]
        );
        assert_eq!(
            split_chapters(&format!("<section id=\"s0\">\n<p>a</p>\n{m}<h1>B</h1>\n</section>\n")),
            vec!["<section id=\"s0\">\n<p>a</p>\n</section>\n", "<section>\n<h1>B</h1>\n</section>\n"]
        );
        assert_eq!(split_chapters(""), vec![String::new()]);
    }

    #[test]
    fn iso8601_timestamps() {
        for (t, want) in [
            (0, "1970-01-01T00:00:00Z"),
            (951_782_400, "2000-02-29T00:00:00Z"),
            (1_791_891_000, "2026-10-13T11:30:00Z"),
            (-86_400, "1969-12-31T00:00:00Z"),
        ] {
            assert_eq!(iso8601(t), want);
        }
    }

    /// #771, #773: dcterms:modified is a real time, the default identifier is a stable urn:uuid
    /// that differs between documents with the same title, and a JPEG cover keeps its type.
    #[test]
    fn package_metadata_and_cover_image() {
        let opf = |d: &Document, opts: &EpubOptions| {
            let mut z = zip::ZipArchive::new(std::io::Cursor::new(export_epub(d, opts).unwrap())).unwrap();
            let mut s = String::new();
            z.by_name("OEBPS/content.opf").unwrap().read_to_string(&mut s).unwrap();
            let names: Vec<String> = z.file_names().map(str::to_string).collect();
            (s, names)
        };
        let id_of = |opf: &str| opf.split("<dc:identifier id=\"bookid\">").nth(1).and_then(|r| r.split('<').next()).unwrap_or_default().to_string();
        let mut a = Document::new(&NewDocument::default());
        a.title = "Same".into();
        a.created = 1_700_000_000;
        let mut b = a.clone();
        b.created = 1_700_000_001;
        let (oa, _) = opf(&a, &EpubOptions::default());
        let (oa2, _) = opf(&a, &EpubOptions::default());
        let (ob, _) = opf(&b, &EpubOptions::default());
        assert!(id_of(&oa).starts_with("urn:uuid:") && id_of(&oa).len() == 45, "{}", id_of(&oa));
        assert_eq!(id_of(&oa), id_of(&oa2), "stable from export to export");
        assert_ne!(id_of(&oa), id_of(&ob), "not shared by two books with the same title");
        // Modified: the given time, else the document's save (or creation) time.
        assert!(oa.contains("<meta property=\"dcterms:modified\">2023-11-14T22:13:20Z</meta>"), "{oa}");
        let at = EpubOptions { modified: Some(1_791_891_000), ..Default::default() };
        assert!(opf(&a, &at).0.contains("2026-10-13T11:30:00Z"));
        // A JPEG cover is packaged as such.
        let jpeg = EpubOptions { cover: Some(vec![0xFF, 0xD8, 0xFF, 0xE0, 1, 2]), ..Default::default() };
        let (o, names) = opf(&a, &jpeg);
        assert!(o.contains("href=\"images/cover.jpg\" media-type=\"image/jpeg\" properties=\"cover-image\""), "{o}");
        assert!(names.contains(&"OEBPS/images/cover.jpg".to_string()), "{names:?}");
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
        let html = export_html(&d, &HtmlOptions { title: Some("T".into()), language: None, ..Default::default() });
        assert!(html.starts_with("<!DOCTYPE html>") && html.contains("<title>T</title>") && html.contains("Hello &lt;web&gt;"));
        assert!(html.contains("p.basic-paragraph"), "styles inline");
        assert_eq!(base64(b"Man"), "TWFu");
        assert_eq!(base64(b"Ma"), "TWE=");
        assert_eq!(base64(b"M"), "TQ==");
    }

    /// #774: an image without alt text is decorative (`alt=""`) and reported, and package names
    /// come from the original file names.
    #[test]
    fn images_get_readable_names_and_no_file_name_alt() {
        let mut d = Document::new(&NewDocument::default());
        let lid = d.default_layer();
        let place = |d: &mut Document, name: &str, mime: &str, alt: &str, y: f64| {
            let aid = AssetId(d.alloc());
            d.assets.insert(
                aid,
                Arc::new(Asset { page: 0, id: aid, name: name.into(), mime: mime.into(), link: None, data: Arc::new(vec![1]), pixels: Some((1, 1)) }),
            );
            let mut item = Item::new(ItemId(d.alloc()), lid, Shape::Rectangle, shapes::rectangle(Rect::new(36.0, y, 72.0, y + 36.0)));
            item.alt_text = alt.into();
            item.content = Content::Graphic(Graphic {
                asset: aid,
                size: (1.0, 1.0),
                xf: Affine::IDENTITY,
                auto_fit: Default::default(),
                fit_align: 4,
                crop: [0.0; 4],
            });
            d.insert_item(SpreadRef::Doc(0), item, None).unwrap();
            aid.0
        };
        let a = place(&mut d, "/tmp/Anim Frame (1).GIF", "image/gif", "", 36.0);
        let b = place(&mut d, "photo.jpg", "image/jpeg", "A heron", 120.0);
        let c = place(&mut d, "???.png", "image/png", "Mark", 200.0);
        assert_eq!(images_without_alt(&d), vec!["/tmp/Anim Frame (1).GIF".to_string()]);
        let mut z = zip::ZipArchive::new(std::io::Cursor::new(export_epub(&d, &EpubOptions::default()).unwrap())).unwrap();
        let mut x = String::new();
        z.by_name("OEBPS/content.xhtml").unwrap().read_to_string(&mut x).unwrap();
        assert!(x.contains(&format!("<img src=\"images/anim-frame-1-{a}.gif\" alt=\"\"/>")), "{x}");
        assert!(x.contains(&format!("<img src=\"images/photo-{b}.jpg\" alt=\"A heron\"/>")), "{x}");
        assert!(x.contains(&format!("<img src=\"images/{c}.png\" alt=\"Mark\"/>")), "{x}");
        assert!(z.by_name(&format!("OEBPS/images/anim-frame-1-{a}.gif")).is_ok());
    }

    /// #775: the generic fallback follows the font, and embedded fonts are packaged and declared.
    #[test]
    fn generic_fallbacks_and_embedded_fonts() {
        let none = BTreeMap::new();
        for (family, want) in [
            ("Source Serif 4", "serif"),
            ("Source Sans 3", "sans-serif"),
            ("Microsoft Sans Serif", "sans-serif"),
            ("JetBrains Mono", "monospace"),
            ("Courier New", "monospace"),
            ("Franklin Gothic", "sans-serif"),
            ("Garamond", "serif"),
        ] {
            assert_eq!(generic_family(family, &none), want, "{family}");
        }
        let known: BTreeMap<String, String> =
            [("Montserrat".to_string(), "sans-serif".to_string()), ("Odd".to_string(), "nonsense".to_string())].into();
        assert_eq!(generic_family("Montserrat", &known), "sans-serif");
        assert_eq!(generic_family("Odd", &known), "serif", "only CSS generic families are used");
        assert_eq!(css_weight_style("Semibold Italic"), (600, "italic"));
        assert_eq!(css_weight_style("Bold"), (700, "normal"));
        assert_eq!(css_weight_style("Extra Bold"), (800, "normal"));
        assert_eq!(css_weight_style("Regular"), (400, "normal"));
        assert_eq!(css_weight_style("Light"), (300, "normal"));
        assert_eq!(css_weight_style("Thin Oblique"), (100, "italic"));
        assert_eq!(css_weight_style("Black"), (900, "normal"));

        let mut d = Document::new(&NewDocument::default());
        let lid = d.default_layer();
        d.add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 36.0, 300.0, 100.0), lid, "Hello", ParaFormat::default()).unwrap();
        if let Some(basic) = d.styles_mut().paragraph.iter_mut().find(|s| s.name == story::BASIC_PARAGRAPH) {
            basic.chars.font_family = Some("Montserrat".into());
            basic.chars.font_style = Some("Bold Italic".into());
        }
        assert_eq!(stylesheet_fonts(&d), vec![("Montserrat".to_string(), "Bold Italic".to_string())]);
        let opts = EpubOptions {
            generic_families: known,
            fonts: vec![EmbeddedFont { family: "Montserrat".into(), style: "Bold Italic".into(), data: b"OTTO font".to_vec() }],
            ..Default::default()
        };
        let mut z = zip::ZipArchive::new(std::io::Cursor::new(export_epub(&d, &opts).unwrap())).unwrap();
        let mut read = |path: &str| {
            let mut s = String::new();
            z.by_name(path).unwrap().read_to_string(&mut s).unwrap();
            s
        };
        let css = read("OEBPS/style.css");
        assert!(
            css.starts_with(
                "@font-face { font-family: \"Montserrat\"; src: url(\"fonts/0-montserrat-bold-italic.otf\"); font-weight: 700; font-style: italic; }"
            ),
            "{css}"
        );
        assert!(css.contains("font-family: \"Montserrat\", sans-serif;"), "{css}");
        let opf = read("OEBPS/content.opf");
        assert!(opf.contains("<item id=\"font0\" href=\"fonts/0-montserrat-bold-italic.otf\" media-type=\"font/otf\"/>"), "{opf}");
        assert_eq!(read("OEBPS/fonts/0-montserrat-bold-italic.otf"), "OTTO font");
        // HTML takes the generic families too, and embeds nothing.
        let html = export_html(&d, &HtmlOptions { generic_families: opts.generic_families.clone(), ..Default::default() });
        assert!(html.contains("\"Montserrat\", sans-serif;") && !html.contains("@font-face"), "{html}");
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
                page: 0,
                id: aid,
                name: "Cover & Notes".into(),
                mime: "image/png; profile=screen".into(),
                link: None,
                data: Arc::new(vec![1]),
                pixels: Some((1, 1)),
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
        let html = export_html(&d, &HtmlOptions { title: None, language: Some("English (US)".into()), ..Default::default() });
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
                ..Default::default()
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

    /// "Body¹ text", a 2×2 table, then "Closing" with an endnote.
    fn tables_and_notes() -> Document {
        let mut d = Document::new(&NewDocument::default());
        let lid = d.default_layer();
        let (_, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 36.0, 300.0, 400.0), lid, "Body text", ParaFormat::default()).unwrap();
        let st = d.story_mut(sid).unwrap();
        st.insert_note(4, "Foot note text", ParaFormat::default());
        let mut t = Table::new(1, 2, 2, 0, 0, 200.0);
        for (r, c, s) in [(0, 0, "Cell A1"), (0, 1, "Cell B1"), (1, 0, "Cell A2"), (1, 1, "Cell B2")] {
            t.cell_mut(r, c).unwrap().text = Story::with_text(StoryId(0), s, ParaFormat::default());
        }
        let end = st.len();
        st.insert_table(end, t);
        let end = st.len();
        st.insert(end, "Closing");
        let end = st.len();
        st.insert_endnote(end, "End note text", ParaFormat::default());
        d
    }

    #[test]
    fn tables_footnotes_and_endnotes_are_exported() {
        let d = tables_and_notes();
        let bytes = export_epub(&d, &EpubOptions::default()).unwrap();
        let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let mut html = String::new();
        z.by_name("OEBPS/content.xhtml").unwrap().read_to_string(&mut html).unwrap();
        let (t0, t1) = (html.find("<table>").unwrap(), html.find("</table>").unwrap());
        for cell in ["Cell A1", "Cell B1", "Cell A2", "Cell B2"] {
            let at = html.find(&format!("<td><p class=\"basic-paragraph\">{cell}</p>\n</td>")).unwrap_or_else(|| panic!("{cell} in a cell:\n{html}"));
            assert!(t0 < at && at < t1);
        }
        assert_eq!(html.matches("<tr>").count(), 2);

        let noteref = "Body<sup><a epub:type=\"noteref\" role=\"doc-noteref\" id=\"fnref-1\" href=\"#fn-1\">1</a></sup> text";
        assert!(html.contains(noteref), "footnote reference:\n{html}");
        let aside = html.find("<aside epub:type=\"footnote\" role=\"doc-footnote\" id=\"fn-1\">").unwrap();
        let aside_end = aside + html[aside..].find("</aside>").unwrap();
        assert!(html[aside..aside_end].contains("<a href=\"#fnref-1\">1</a> Foot note text"));
        assert!(aside < t0, "the footnote follows its paragraph");

        assert!(html.contains("Closing<sup><a epub:type=\"noteref\" role=\"doc-noteref\" id=\"enref-1\" href=\"#en-1\">1</a></sup>"));
        let section = html.find("<section epub:type=\"endnotes\" role=\"doc-endnotes\" id=\"endnotes\">").unwrap();
        assert!(section > html.find("Closing").unwrap());
        assert!(html[section..].contains("<li epub:type=\"endnote\" id=\"en-1\">"));
        assert!(html[section..].contains("<a href=\"#enref-1\">1</a> End note text"));
        let mut nav = String::new();
        z.by_name("OEBPS/nav.xhtml").unwrap().read_to_string(&mut nav).unwrap();
        assert!(nav.contains("href=\"content.xhtml#endnotes\""));

        let page = export_html(&d, &HtmlOptions::default());
        assert!(page.contains("<aside role=\"doc-footnote\" id=\"fn-1\">") && page.contains("Cell B2") && page.contains("End note text"));
        assert!(!page.contains("epub:type"), "plain HTML has no EPUB namespace");
    }

    #[test]
    fn table_rows_groups_and_merged_cells() {
        let mut d = Document::new(&NewDocument::default());
        let lid = d.default_layer();
        let (_, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 36.0, 300.0, 400.0), lid, "", ParaFormat::default()).unwrap();
        // One header row, two body rows, one footer row; the header spans both columns and the
        // first body cell spans both body rows and runs into the footer.
        let mut t = Table::new(1, 2, 2, 1, 1, 200.0);
        for (r, c, s) in [(0, 0, "Head"), (1, 0, "Tall"), (1, 1, "B1"), (2, 1, "B2"), (3, 0, "F1"), (3, 1, "F2")] {
            t.cell_mut(r, c).unwrap().text = Story::with_text(StoryId(0), s, ParaFormat::default());
        }
        t.cell_mut(0, 0).unwrap().col_span = 2;
        t.cell_mut(1, 0).unwrap().row_span = 3;
        d.story_mut(sid).unwrap().insert_table(0, t);
        let html = story_html(&d, sid);
        let p = |s: &str| format!("<p class=\"basic-paragraph\">{s}</p>\n");
        let expected = format!(
            "<table>\n<thead>\n<tr><th colspan=\"2\">{}</th>\n</tr>\n</thead>\n<tbody>\n<tr><td rowspan=\"2\">{}</td>\n<td>{}</td>\n</tr>\n<tr><td>{}</td>\n</tr>\n</tbody>\n<tfoot>\n<tr><td>{}</td>\n<td>{}</td>\n</tr>\n</tfoot>\n</table>\n",
            p("Head"),
            p("Tall"),
            p("B1"),
            p("B2"),
            p("F1"),
            p("F2")
        );
        assert!(html.contains(&expected), "{html}");
    }
}
