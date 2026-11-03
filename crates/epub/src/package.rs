use super::chapters::{chapter_file, link_across_chapters, nav_list, split_chapters};
use super::css::stylesheet_with;
use super::fonts::font_file;
use super::html::{Body, body_html};
use super::markup::{EpubError, language_tag, story_markup, xml_attr, xml_text};
use super::*;

/// An ISO 8601 UTC timestamp (`2026-10-13T09:30:00Z`) for Unix seconds `t`.
/// A time outside chrono's year range still converts: the calendar repeats every 400 years, so the
/// instant is moved into range by whole cycles and only the year is carried (matches the old
/// `civil_from_days`), rather than resetting to the epoch.
pub(crate) fn iso8601(t: i64) -> String {
    let (y, dt) = designcraft_doc::time::utc_parts(t, 0);
    // `{:04}` rather than `%Y`, which writes a `+` before years past 9999.
    format!("{y:04}-{}", dt.format("%m-%dT%H:%M:%SZ"))
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
    let title = opts.title.clone().unwrap_or_else(|| doc.metadata_title().to_string());
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
    let rendition = format!(
        "\n<meta property=\"rendition:layout\">pre-paginated</meta>\n<meta property=\"rendition:spread\">auto</meta>\n<meta name=\"original-resolution\" content=\"{vw}x{vh}\"/>"
    );
    let head = PackageHead { lang, id: &id, title: &title, author: opts.author.as_deref().or(doc.metadata_author()), modified: &modified };
    let opf = package_opf(&head, " prefix=\"rendition: http://www.idpf.org/vocab/rendition/#\"", &rendition, &manifest, &spine);
    let entries = files.iter().map(|(name, data)| (name.clone(), data.as_slice(), name.ends_with(".png")));
    write_package(&opf, &nav, entries)
}

/// The package metadata both EPUB flavours share.
struct PackageHead<'a> {
    lang: &'a str,
    id: &'a str,
    title: &'a str,
    author: Option<&'a str>,
    modified: &'a str,
}

/// The OPF package document. `prefix` goes on `<package>` (with its leading space), `meta` after
/// `dcterms:modified` (with its leading newline); `manifest` and `spine` are the item lines.
fn package_opf(h: &PackageHead, prefix: &str, meta: &str, manifest: &str, spine: &str) -> String {
    let PackageHead { lang, id, title, author, modified } = h;
    let author = author.map(|a| format!("<dc:creator>{}</dc:creator>", xml_text(a))).unwrap_or_default();
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<package xmlns=\"http://www.idpf.org/2007/opf\" version=\"3.0\" unique-identifier=\"bookid\" xml:lang=\"{lang}\"{prefix}>\n<metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\">\n<dc:identifier id=\"bookid\">{}</dc:identifier>\n<dc:title>{}</dc:title>\n<dc:language>{lang}</dc:language>{author}\n<meta property=\"dcterms:modified\">{modified}</meta>{meta}\n<meta name=\"generator\" content=\"DesignCraft\"/>\n</metadata>\n<manifest>\n{manifest}</manifest>\n<spine>\n{spine}</spine>\n</package>\n",
        xml_text(id),
        xml_text(title)
    )
}

/// Zips an EPUB: the stored `mimetype`, `container.xml`, the OPF and nav, then `entries` in order
/// as (path, bytes, stored uncompressed).
fn write_package<'a>(opf: &str, nav: &str, entries: impl IntoIterator<Item = (String, &'a [u8], bool)>) -> Result<Vec<u8>, EpubError> {
    let io = |e: std::io::Error| EpubError(e.to_string());
    let zerr = |e: zip::result::ZipError| EpubError(e.to_string());
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
        for (name, data, store) in entries {
            z.start_file(name, if store { stored } else { deflate }).map_err(zerr)?;
            z.write_all(data).map_err(io)?;
        }
        z.finish().map_err(zerr)?;
    }
    Ok(buf.into_inner())
}

/// Export the document as a reflowable EPUB 3.
pub fn export_epub(doc: &Document, opts: &EpubOptions) -> Result<Vec<u8>, EpubError> {
    let title = opts.title.clone().unwrap_or_else(|| doc.metadata_title().to_string());
    let id = identifier(doc, opts, &title);
    let modified = modified(doc, opts);
    let Body { html: body, images, toc, .. } = body_html(doc, true, opts.split.as_ref(), &|name, _, _| name.to_string());
    let lang = language_tag(&opts.language);
    // One file per chapter (see `starts_chapter`); one file in all when Split Document is off.
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
    let head = PackageHead { lang, id: &id, title: &title, author: opts.author.as_deref().or(doc.metadata_author()), modified: &modified };
    let opf = package_opf(&head, "", "", &manifest, &format!("{cover_spine}{spine}"));
    let css = stylesheet_with(doc, &opts.generic_families, &opts.fonts);
    let mut entries: Vec<(String, &[u8], bool)> = vec![("OEBPS/style.css".into(), css.as_bytes(), false)];
    entries.extend(chapters.iter().enumerate().map(|(k, c)| (format!("OEBPS/{}", chapter_file(k)), c.as_bytes(), false)));
    entries.extend(images.iter().map(|(name, (_, data))| (format!("OEBPS/{name}"), *data, true)));
    entries.extend(opts.fonts.iter().enumerate().map(|(k, f)| (format!("OEBPS/{}", font_file(k, f).0), f.data.as_slice(), false)));
    let cover_xhtml = opts.cover.as_ref().map(|c| {
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE html>\n<html xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:epub=\"http://www.idpf.org/2007/ops\" xml:lang=\"{lang}\">\n<head><meta charset=\"utf-8\"/><title>{}</title><style>body{{margin:0;text-align:center}} img{{max-width:100%;max-height:100vh}}</style></head>\n<body epub:type=\"cover\"><img src=\"{}\" alt=\"{}\"/></body>\n</html>\n",
            xml_text(&title),
            cover_file(c).0,
            xml_attr(&title)
        )
    });
    if let (Some(c), Some(xhtml)) = (&opts.cover, &cover_xhtml) {
        entries.push((format!("OEBPS/{}", cover_file(c).0), c.as_slice(), true));
        entries.push(("OEBPS/cover.xhtml".into(), xhtml.as_bytes(), false));
    }
    write_package(&opf, &nav, entries)
}
