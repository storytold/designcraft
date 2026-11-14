use std::borrow::Cow;

use super::chapters::{chapter_file, link_across_chapters, split_chapters};
use super::css::stylesheet_with;
use super::fonts::font_file;
use super::html::{Body, body_html};
use super::markup::{EpubError, language_tag, story_markup, xml_attr, xml_text};
use super::{Document, EpubOptions};

/// The table of contents' title in `language` (a BCP 47 tag or a language name's code), English
/// for languages without one here.
pub fn contents_title(language: &str) -> &'static str {
    craft_epub::contents_title(language)
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

fn capped_title(title: String) -> String {
    match title.char_indices().nth(1024) {
        Some((at, _)) => title.get(..at).unwrap_or(&title).to_owned(),
        None => title,
    }
}

fn identifier(doc: &Document, opts: &EpubOptions, title: &str) -> String {
    opts.identifier.clone().filter(|id| !id.trim().is_empty()).unwrap_or_else(|| craft_epub::urn_uuid(&format!("{title}\n{}", doc.created)))
}

fn modified(doc: &Document, opts: &EpubOptions) -> String {
    let timestamp = opts.modified.or((doc.modified > 0).then_some(doc.modified)).or((doc.created > 0).then_some(doc.created));
    timestamp.map_or_else(
        || "2026-01-01T00:00:00Z".to_owned(),
        |time| {
            let (year, date) = designcraft_doc::time::utc_parts(time, 0);
            format!("{year:04}-{}", date.format("%m-%dT%H:%M:%SZ"))
        },
    )
}

fn metadata(doc: &Document, opts: &EpubOptions, title: &str, language: &str) -> craft_epub::Metadata {
    craft_epub::Metadata {
        identifier: identifier(doc, opts, title),
        title: title.to_owned(),
        language: language.to_owned(),
        creators: opts.author.as_deref().or(doc.metadata_author()).into_iter().map(str::to_owned).collect(),
        modified: modified(doc, opts),
        generator: Some("DesignCraft".into()),
        ..Default::default()
    }
}

/// A pre-paginated (fixed-layout) EPUB: one XHTML page per document page.
pub fn export_fixed_epub(doc: &Document, pages: &[FixedPage], opts: &EpubOptions) -> Result<Vec<u8>, EpubError> {
    let title = capped_title(opts.title.clone().unwrap_or_else(|| doc.metadata_title().to_string()));
    let lang = language_tag(&opts.language);
    let mut resources = Vec::with_capacity(pages.len().saturating_mul(2));
    let mut spine = Vec::with_capacity(pages.len());
    let mut navigation = Vec::with_capacity(pages.len());
    for (index, page) in pages.iter().enumerate() {
        let number = index + 1;
        let width = page.width.round().max(1.0) as u32;
        let height = page.height.round().max(1.0) as u32;
        let xhtml = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE html>\n<html xmlns=\"http://www.w3.org/1999/xhtml\" xml:lang=\"{lang}\" lang=\"{lang}\">\n<head><meta charset=\"utf-8\"/><meta name=\"viewport\" content=\"width={width}, height={height}\"/><title>{} — {number}</title>\n<style>body{{margin:0;width:{width}px;height:{height}px;position:relative}} img{{position:absolute;left:0;top:0;width:{width}px;height:{height}px}} .t{{position:absolute;left:0;top:0;width:{width}px;color:transparent;font-size:1px;overflow:hidden}}</style></head>\n<body><img src=\"pages/p{number}.png\" alt=\"\"/><div class=\"t\">{}</div></body>\n</html>\n",
            xml_text(&title),
            story_markup(&page.text).replace('\n', "<br/>")
        );
        resources.push(craft_epub::Resource::new(
            format!("page{number}"),
            format!("page{number}.xhtml"),
            "application/xhtml+xml",
            xhtml.into_bytes(),
        ));
        let image =
            craft_epub::Resource::new(format!("pimg{number}"), format!("pages/p{number}.png"), "image/png", Cow::Borrowed(page.png.as_slice()));
        resources.push(if number == 1 { image.with_properties("cover-image").stored() } else { image.stored() });
        spine.push(format!("page{number}"));
        navigation.push(craft_epub::NavItem { href: format!("page{number}.xhtml"), label: number.to_string(), level: 0 });
    }
    let (width, height) = pages.first().map_or((612, 792), |page| (page.width.round() as u32, page.height.round() as u32));
    let book = craft_epub::Book {
        metadata: metadata(doc, opts, &title, lang),
        layout: craft_epub::Layout::PrePaginated { width, height },
        resources,
        spine,
        nav: craft_epub::Nav { kind: craft_epub::NavKind::PageList, heading: "Pages".into(), items: navigation },
        ncx: false,
    };
    craft_epub::write(&book).map_err(|error| EpubError(error.to_string()))
}

/// Export the document as a reflowable EPUB 3.
pub fn export_epub(doc: &Document, opts: &EpubOptions) -> Result<Vec<u8>, EpubError> {
    let title = capped_title(opts.title.clone().unwrap_or_else(|| doc.metadata_title().to_string()));
    let lang = language_tag(&opts.language);
    let Body { html: body, images, toc, .. } = body_html(doc, true, opts.split.as_ref(), &|name, _, _| name.to_string());
    let chapters: Vec<String> = split_chapters(&body)
        .into_iter()
        .map(|part| {
            format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE html>\n<html xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:epub=\"http://www.idpf.org/2007/ops\" xml:lang=\"{lang}\" lang=\"{lang}\">\n<head><meta charset=\"utf-8\"/><title>{}</title><link rel=\"stylesheet\" type=\"text/css\" href=\"style.css\"/></head>\n<body>\n{part}</body>\n</html>\n",
                xml_text(&title)
            )
        })
        .collect();
    let mut files = std::collections::BTreeMap::<String, String>::new();
    for (index, chapter) in chapters.iter().enumerate() {
        for rest in chapter.split(" id=\"").skip(1) {
            if let Some(id) = rest.split('"').next() {
                files.entry(id.to_owned()).or_insert_with(|| chapter_file(index));
            }
        }
    }
    let chapters: Vec<String> =
        chapters.iter().enumerate().map(|(index, chapter)| link_across_chapters(chapter, &chapter_file(index), &files)).collect();
    let nav_items = if toc.is_empty() {
        vec![craft_epub::NavItem { href: "content.xhtml".into(), label: title.clone(), level: 0 }]
    } else {
        toc.iter()
            .map(|(target, label, level)| {
                let file = files.get(target).map_or("content.xhtml", String::as_str);
                craft_epub::NavItem { href: format!("{file}#{target}"), label: label.clone(), level: *level }
            })
            .collect()
    };
    let heading = opts.toc_title.as_deref().filter(|value| !value.trim().is_empty()).unwrap_or(contents_title(&opts.language));
    let mut resources = Vec::new();
    let css = stylesheet_with(doc, &opts.generic_families, &opts.fonts);
    resources.push(craft_epub::Resource::new("css", "style.css", "text/css", css.into_bytes()));
    let mut spine = Vec::new();
    for (index, chapter) in chapters.into_iter().enumerate() {
        let id = if index == 0 { "content".to_owned() } else { format!("content{}", index + 1) };
        resources.push(craft_epub::Resource::new(id.clone(), chapter_file(index), "application/xhtml+xml", chapter.into_bytes()));
        spine.push(id);
    }
    for (index, (path, (mime, data))) in images.iter().enumerate() {
        resources.push(craft_epub::Resource::new(format!("img{index}"), path.clone(), *mime, Cow::Borrowed(*data)).stored());
    }
    for (index, font) in opts.fonts.iter().enumerate() {
        let (path, mime) = font_file(index, font);
        resources.push(craft_epub::Resource::new(format!("font{index}"), path, mime, Cow::Borrowed(font.data.as_slice())));
    }
    if let Some(cover) = &opts.cover {
        let (path, mime) = cover_file(cover);
        resources.push(craft_epub::Resource::new("cover-img", path, mime, Cow::Borrowed(cover.as_slice())).with_properties("cover-image").stored());
        let xhtml = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE html>\n<html xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:epub=\"http://www.idpf.org/2007/ops\" xml:lang=\"{lang}\">\n<head><meta charset=\"utf-8\"/><title>{}</title><style>body{{margin:0;text-align:center}} img{{max-width:100%;max-height:100vh}}</style></head>\n<body epub:type=\"cover\"><img src=\"{}\" alt=\"{}\"/></body>\n</html>\n",
            xml_text(&title),
            path,
            xml_attr(&title)
        );
        resources.push(craft_epub::Resource::new("cover", "cover.xhtml", "application/xhtml+xml", xhtml.into_bytes()));
        spine.insert(0, "cover".into());
    }
    let book = craft_epub::Book {
        metadata: metadata(doc, opts, &title, lang),
        layout: craft_epub::Layout::Reflowable,
        resources,
        spine,
        nav: craft_epub::Nav { kind: craft_epub::NavKind::Toc, heading: heading.to_owned(), items: nav_items },
        ncx: false,
    };
    craft_epub::write(&book).map_err(|error| EpubError(error.to_string()))
}

fn cover_file(data: &[u8]) -> (&'static str, &'static str) {
    if data.starts_with(&[0xFF, 0xD8, 0xFF]) { ("images/cover.jpg", "image/jpeg") } else { ("images/cover.png", "image/png") }
}
