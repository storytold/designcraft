use super::chapters::toc_entries;
use super::css::stylesheet_with;
use super::markup::{language_tag, xml_attr, xml_text};
use super::story_writer::StoryWriter;
use super::*;

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
        // Preserve traversal order while carrying each group's transform to spread space.
        // The iterator stack grows with nesting depth, without repeated document lookups.
        let mut stack = vec![(sp.items.iter(), Affine::IDENTITY)];
        while let Some((items, parent)) = stack.last_mut() {
            let Some(it) = items.next() else {
                stack.pop();
                continue;
            };
            let xf = *parent * it.xf;
            let children = it.children();
            if !children.is_empty() {
                stack.push((children.iter(), xf));
            }
            if it.hidden {
                continue;
            }
            let block = match &it.content {
                Content::Text(tf) => {
                    let Some(story) = doc.story(tf.story) else { continue };
                    // A story is placed where its first frame is.
                    if story.frames.first() != Some(&it.id) || seen_stories.contains(&tf.story) || story.text.trim().is_empty() {
                        continue;
                    }
                    seen_stories.push(tf.story);
                    Block::Story(tf.story)
                }
                Content::Graphic(_) => Block::Image(it.id),
                _ => continue,
            };
            // Transform the path once: rotating a local axis-aligned box again would
            // enlarge it and give a different top-left from the actual spread geometry.
            let b = it.path.transformed(xf).bounds().unwrap_or(Rect::ZERO);
            let page = first + sp.page_at_x(b.center().x).unwrap_or(0);
            let key = (page, (b.y0 * 10.0) as i64, (b.x0 * 10.0) as i64);
            keyed.push((key, block));
        }
    }
    let _ = SpreadRef::Doc(0);
    keyed.sort_by_key(|(k, _)| *k);
    keyed.into_iter().map(|(_, b)| b).collect()
}

/// Package path of an asset: `{dir}/{name}-{id}.{ext}`, the name from its original file name
/// (lowercase ASCII, so the package stays readable) and the id keeping two assets of the same
/// name apart.
pub(crate) fn asset_path(dir: &str, a: &designcraft_doc::Asset, mime: &str) -> String {
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

/// The document content as HTML, with the files it names.
pub(crate) struct Body<'a> {
    pub(crate) html: String,
    /// Images, video and sound by package name: (media type, bytes).
    pub(crate) images: BTreeMap<String, (&'a str, &'a [u8])>,
    /// Table of contents: (element id, label, level).
    pub(crate) toc: Vec<(String, String, u8)>,
    /// Asset names of the images written without alt text.
    no_alt: Vec<String>,
}

/// The content in reading order as HTML sections and figures; `img_src` names each image
/// (a package path, or a data URI for single-file HTML).
pub(crate) fn body_html<'a>(doc: &'a Document, epub: bool, split: Option<&EpubSplit>, img_src: &dyn Fn(&str, &str, &[u8]) -> String) -> Body<'a> {
    let order = reading_order(doc);
    let mut writer = StoryWriter::new(doc, epub, Some(img_src));
    if let Some(split) = split {
        writer.split = split.clone();
    }
    let mut body = String::new();
    let mut toc = toc_entries(doc, &order);
    for (k, b) in order.iter().enumerate() {
        match b {
            // The endnote frame's text is generated from the endnotes, which end the body as a
            // linked section instead.
            Block::Story(sid) if doc.endnote_story == Some(*sid) => {}
            Block::Story(sid) => {
                let anchor = format!("s{k}");
                let _ = write!(body, "<section id=\"{anchor}\">\n{}</section>\n", writer.story(*sid, epub.then_some(anchor.as_str())));
            }
            Block::Image(iid) => {
                let Some(it) = doc.item(*iid) else { continue };
                let Some((markup, media)) = writer.graphic(it) else { continue };
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
    Body { html: body, images: writer.images, toc, no_alt: writer.no_alt }
}

/// Placed and anchored images (not video or sound) without alt text, by asset name: they are exported as
/// decorative (`alt=""`), which an export can report.
pub fn images_without_alt(doc: &Document) -> Vec<String> {
    body_html(doc, true, None, &|name, _, _| name.to_string()).no_alt
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
    let title = opts.title.clone().unwrap_or_else(|| doc.metadata_title().to_string());
    let language = opts.language.as_deref().unwrap_or("en");
    let lang = language_tag(language);
    let body = body_html(doc, false, None, &|_, mime, data| format!("data:{mime};base64,{}", base64(data))).html;
    format!(
        "<!DOCTYPE html>\n<html lang=\"{lang}\">\n<head>\n<meta charset=\"utf-8\">\n<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<meta name=\"generator\" content=\"DesignCraft\">\n<title>{}</title>\n<style>\nbody {{ max-width: 40em; margin: 2em auto; padding: 0 1em; }}\nfigure {{ margin: 1em 0; }} figure img {{ max-width: 100%; height: auto; }}\n{}</style>\n</head>\n<body>\n{body}</body>\n</html>\n",
        xml_text(&title),
        stylesheet_with(doc, &opts.generic_families, &[])
    )
}

pub(crate) fn base64(data: &[u8]) -> String {
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
