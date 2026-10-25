//! File › Export PDF (Print).

use designcraft_pdf::{Marks, PdfOptions, Standard};
use serde_json::{Value, json};
use std::{borrow::Cow, sync::Arc};

use super::{CommandSpec, bad, cmd, has_doc, str_param};
use crate::{EngineError, Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(noundo "file.exportPdf", "Export PDF…", ["File"], None,
        "{path?, pages?: \"1-3,5\" | [1,3] (1-based positions; default all), flatten?: high|medium|low|ppi (Transparency Flattener: spreads with transparency are rasterised), spreads?: bool, fullScreen?: bool, bookmarksPanel?: bool, pageLayout?: single|continuous|twoUp|twoUpCover|twoUpContinuous, view?: fitPage|fitWidth|actual, advanceSeconds?: number (interactive PDF), bleed?: bool (document bleed), marks?: bool | {crop?, bleed?, pageInfo?, weight?, offset?}, standard?: \"none\"|\"x4\"|\"a2b\", compressImages?: bool, tagged?: bool (structure tree: stories as paragraphs, figures with alt text), media?: bool (interactive: embed placed video and sound), title?, author?, created?: int (creation date in Unix seconds; default now, for reproducible files)} → {path, bytes, pages, warnings} (no path: {base64, …})",
        has_doc, export_pdf),
        cmd!(noundo "file.exportEpub", "Export EPUB (Reflowable)…", ["File"], None,
        "{path?, title?, author?, language?: \"en\", identifier?: str (dc:identifier; a stable urn:uuid by default), coverImage?: path (a PNG or JPEG as the cover), tocTitle?: str (the table of contents' title; from the language by default), cover?: bool (the first page as the cover image), embedFonts?: bool (package the stylesheet's fonts whose licence allows it, whole files), split?: \"off\"|\"exportTags\"|{style: name} (Split Document for this export, not stored; the document's `document.epubSplit` by default), fixedLayout?: bool (pre-paginated: each page as an image with its text)} → {path, bytes, warnings} (no path: {base64, bytes, warnings}); warnings name images without alt text, exported as decorative, and fonts that could not be embedded",
        has_doc, export_epub),
        cmd!(
            "document.epubSplit",
            "EPUB Split Document",
            [],
            None,
            "{split?: \"off\"|\"exportTags\"|{style: name}} — EPUB Export Options › Split Document, stored with the document: one content file (off, the default), a file before each paragraph whose style has Export Tagging › Split Document on (exportTags), or before each paragraph of one style → {split}",
            has_doc,
            |s, p| {
                const ID: &str = "document.epubSplit";
                let cur = s.doc()?.doc.settings.epub_split.clone();
                let Some(v) = p.get("split") else { return Ok(json!({"split": cur})) };
                let split = epub_split_param(ID, &s.doc()?.doc, v)?;
                if split != cur {
                    let stored = split.clone();
                    s.edit(move |d, _| {
                        d.settings.epub_split = stored;
                        Ok(Value::Null)
                    })?;
                }
                Ok(json!({"split": split}))
            }
        ),
        cmd!(noundo "file.exportFixedEpub", "Export EPUB (Fixed Layout)…", ["File"], None,
        "{path?, title?, author?, language?} — pre-paginated EPUB → {path, bytes} (no path: {base64, bytes})",
        has_doc, |s, p| {
            export_epub(s, &super::with_param(p, "fixedLayout", serde_json::json!(true)))
        }),
        cmd!(noundo "file.printBooklet", "Print Booklet…", ["File"], None,
        "{path?, type?: saddleStitch|twoUpConsecutive, spaceBetween? (pt)} — printer spreads as PDF (pages imposed in booklet order) → {path, bytes, sheets} (no path: {base64, …})",
        has_doc, print_booklet),
        cmd!(noundo "file.exportHtml", "Export HTML…", ["File"], None,
        "{path?, title?, language?} — one self-contained page (styles inline, images embedded), stories and graphics in reading order → {path, bytes, warnings} (no path: {text, bytes, warnings})",
        has_doc, export_html),
        cmd!(noundo "file.exportPptx", "Export PowerPoint…", ["File"], None,
        "{path?, pages?: \"1-3,5\" | [1,3] (1-based positions; default all), lineBreaks?: keep|reflow (keep: DesignCraft's lines, spacing and leading; reflow: one box per frame, PowerPoint sets the lines), rasterize?: bool (default true: objects PowerPoint can't show — placed PDF/EPS, type on a path, some effects — go in as 300 ppi images), embedFonts?: bool (default true: the TrueType fonts whose licences allow it), title?, author?} — every page as a slide of editable shapes, pictures, text boxes and tables → {path, bytes, slides, warnings} (no path: {base64, …})",
        has_doc, export_pptx),
        cmd!(noundo "file.exportText", "Export Text…", ["File"], None,
        "{path?, format?: \"txt\"|\"rtf\"|\"tagged\" (Tagged Text; default from the path, else txt), story?: unsigned integer, frame?: unsigned integer (text frame)} — both ids are validated; story wins; null means absent (default: the story being edited or of the selected frame) → {path, bytes} (no path: {text, bytes})",
        has_story_target, export_text),
    ]
}

fn print_booklet(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "file.printBooklet";
    let kind = match str_param(p, "type").unwrap_or("saddleStitch") {
        "saddleStitch" => designcraft_pdf::BookletKind::SaddleStitch,
        "twoUpConsecutive" | "twoUp" => designcraft_pdf::BookletKind::TwoUpConsecutive,
        t => {
            return Err(bad(ID, format!("unknown type `{t}` (saddleStitch, twoUpConsecutive)")));
        }
    };
    let opts = designcraft_pdf::BookletOptions { kind, space_between: p.get("spaceBetween").and_then(Value::as_f64).unwrap_or(0.0), title: None };
    let st = s.doc()?;
    let r = designcraft_pdf::export_booklet(&super::interchange::with_pdf_boxes(&st.doc), &s.cache, &opts)
        .map_err(|e| EngineError::Other(e.to_string()))?;
    match str_param(p, "path") {
        Some(path) => {
            #[cfg(not(target_arch = "wasm32"))]
            crate::io::write_file(path, &r.bytes).map_err(EngineError::Other)?;
            Ok(json!({"path": path, "bytes": r.bytes.len(), "sheets": r.pages, "warnings": r.warnings}))
        }
        None => Ok(json!({"base64": super::file::base64_encode(&r.bytes), "bytes": r.bytes.len(), "sheets": r.pages, "warnings": r.warnings})),
    }
}

/// HTML and reflowable EPUB embed the EPS preview; keep the session's source bytes intact.
fn eps_previews(d: &designcraft_doc::Document) -> (Cow<'_, designcraft_doc::Document>, Vec<String>) {
    // Match the exporter's reading order (including Article exclusions), not every asset
    // in the document: unused or hidden graphics must not prevent an export.
    let mut used = std::collections::BTreeSet::new();
    for block in designcraft_epub::reading_order(d) {
        let designcraft_epub::Block::Image(id) = block else {
            continue;
        };
        let Some(it) = d.item(id) else { continue };
        let designcraft_doc::Content::Graphic(g) = &it.content else {
            continue;
        };
        used.insert(g.asset);
        if d.assets.get(&g.asset).is_some_and(|a| designcraft_doc::media_kind(&a.mime).is_some())
            && let Some(poster) = it.media.as_ref().and_then(|m| m.poster)
        {
            used.insert(poster);
        }
    }
    let mut out = Cow::Borrowed(d);
    let mut warnings = Vec::new();
    for id in used {
        let Some(a) = d.assets.get(&id).filter(|a| designcraft_images::is_eps(&a.data)) else {
            continue;
        };
        let png = designcraft_images::eps_png(&a.data);
        if png.is_none() {
            warnings.push(format!("{}: unusable EPS preview; graphic omitted", a.name));
        }
        if let Some(a) = out.to_mut().assets.get_mut(&id) {
            let a = Arc::make_mut(a);
            a.mime = "image/png".into();
            a.data = Arc::new(png.unwrap_or_default());
        }
    }
    (out, warnings)
}

fn export_html(s: &mut Session, p: &Value) -> Result<Value> {
    let st = s.doc()?;
    let opts = designcraft_epub::HtmlOptions {
        title: str_param(p, "title").map(str::to_string),
        language: str_param(p, "language").map(str::to_string),
        generic_families: generic_families(&st.doc),
    };
    let (doc, warnings) = eps_previews(&st.doc);
    let text = designcraft_epub::export_html(&doc, &opts);
    match str_param(p, "path") {
        Some(path) => {
            #[cfg(not(target_arch = "wasm32"))]
            crate::io::write_file(path, text.as_bytes()).map_err(EngineError::Other)?;
            Ok(json!({"path": path, "bytes": text.len(), "warnings": warnings}))
        }
        None => Ok(json!({"bytes": text.len(), "text": text, "warnings": warnings})),
    }
}

fn export_pptx(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "file.exportPptx";
    let st = s.doc()?;
    let line_breaks = match str_param(p, "lineBreaks") {
        None | Some("keep") => designcraft_pptx::LineBreaks::Keep,
        Some("reflow") => designcraft_pptx::LineBreaks::Reflow,
        Some(x) => return Err(bad(ID, format!("unknown lineBreaks `{x}` (keep, reflow)"))),
    };
    let opts = designcraft_pptx::PptxOptions {
        pages: pages_param(ID, p, st.doc.page_count())?,
        line_breaks,
        title: str_param(p, "title").map(str::to_string),
        author: str_param(p, "author").map(str::to_string),
        created: None,
        embed_fonts: p.get("embedFonts").and_then(Value::as_bool).unwrap_or(true),
    };
    // Object Export Options › Rasterize, and what PowerPoint has no objects for (a threaded frame
    // stays text: a picture of it would drop the rest of its story).
    let auto = p.get("rasterize").and_then(Value::as_bool).unwrap_or(true);
    let (raster, rasterized) = rasterize_where(&st.doc, &s.cache, 300.0, true, |it| {
        !threaded(&st.doc, it) && (it.export_options.rasterize || (auto && designcraft_pptx::needs_raster(&st.doc, it)))
    });
    let doc: &designcraft_doc::Document = raster.as_ref().unwrap_or(&st.doc);
    let mut r = designcraft_pptx::export_pptx(doc, &s.cache, &opts).map_err(|e| EngineError::Other(e.to_string()))?;
    if rasterized > 0 {
        r.warnings.push(format!("{rasterized} object(s) went in as images"));
    }
    match str_param(p, "path") {
        Some(path) => {
            #[cfg(not(target_arch = "wasm32"))]
            crate::io::write_file(path, &r.bytes).map_err(EngineError::Other)?;
            Ok(json!({"path": path, "bytes": r.bytes.len(), "slides": r.slides, "warnings": r.warnings}))
        }
        None => Ok(json!({"base64": super::file::base64_encode(&r.bytes), "bytes": r.bytes.len(), "slides": r.slides, "warnings": r.warnings})),
    }
}

fn has_story_target(s: &Session) -> std::result::Result<(), String> {
    super::text::story_of(s, &Value::Null).map(|_| ()).ok_or_else(|| "edit text or select a text frame".into())
}

fn export_text(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "file.exportText";
    let sid = super::text::validated_story_of(s, p, ID)?.ok_or_else(|| bad(ID, "edit text or select a text frame"))?;
    let doc = &s.doc()?.doc;
    let story = doc.stories.get(&sid).ok_or_else(|| bad(ID, format!("no story {}", sid.0)))?;
    let path = str_param(p, "path");
    let format = match str_param(p, "format") {
        Some(f @ ("rtf" | "tagged")) => f,
        Some("txt") | Some("text") => "txt",
        Some(f) => return Err(bad(ID, format!("unknown format `{f}` (txt, rtf, tagged)"))),
        None if path.is_some_and(|x| x.to_ascii_lowercase().ends_with(".rtf")) => "rtf",
        None => "txt",
    };
    let text = match format {
        "rtf" => designcraft_textimport::export::rtf(doc, story),
        "tagged" => designcraft_textimport::tagged::tagged_text(doc, story),
        _ => designcraft_textimport::export::plain_text(story),
    };
    match path {
        Some(path) => {
            #[cfg(not(target_arch = "wasm32"))]
            crate::io::write_file(path, text.as_bytes()).map_err(EngineError::Other)?;
            Ok(json!({"path": path, "bytes": text.len()}))
        }
        None => Ok(json!({"bytes": text.len(), "text": text})),
    }
}

/// Text of the frames on page `abs`, in line order (for fixed-layout pages).
fn page_text(d: &designcraft_doc::Document, cache: &designcraft_compose::Cache, abs: usize) -> String {
    let mut out = String::new();
    for st in d.stories.values() {
        let cs = cache.get(d, st.id, None);
        for ft in &cs.frames {
            if d.page_of_item(ft.frame) != Some(abs) {
                continue;
            }
            for l in &ft.lines {
                let r = l.range.start.min(st.text.len())..l.range.end.min(st.text.len());
                let t: String = st.text[r].chars().filter(|c| !('\u{E000}'..='\u{F8FF}').contains(c)).collect();
                if !t.trim().is_empty() {
                    out.push_str(t.trim_end_matches('\n'));
                    out.push('\n');
                }
            }
        }
    }
    out
}

/// The CSS generic family of each font family the EPUB/HTML stylesheet names, from the fonts'
/// classification (PANOSE, OS/2 class, name), for the `font-family` fallbacks.
fn generic_families(d: &designcraft_doc::Document) -> std::collections::BTreeMap<String, String> {
    use designcraft_fonts::FontClass;
    let db = designcraft_fonts::FontDb::global().scoped(d.font_scope);
    let mut out = std::collections::BTreeMap::new();
    for (family, _) in designcraft_epub::stylesheet_fonts(d) {
        if out.contains_key(&family) {
            continue;
        }
        let generic = match db.family_kind(&family).map(|(k, _)| k.class) {
            Some(FontClass::Serif | FontClass::SlabSerif) => "serif",
            Some(FontClass::SansSerif) => "sans-serif",
            Some(FontClass::Monospaced) => "monospace",
            Some(FontClass::Script) => "cursive",
            Some(FontClass::Decorative) => "fantasy",
            Some(FontClass::Symbol | FontClass::Other) | None => continue,
        };
        out.insert(family, generic.to_string());
    }
    out
}

/// The faces the EPUB stylesheet names, to package with it; `warnings` gets the ones left out
/// (not available, a licence that forbids embedding, or part of a font collection).
fn embedded_fonts(d: &designcraft_doc::Document, warnings: &mut Vec<String>) -> Vec<designcraft_epub::EmbeddedFont> {
    let db = designcraft_fonts::FontDb::global().scoped(d.font_scope);
    let mut out = Vec::new();
    for (family, style) in designcraft_epub::stylesheet_fonts(d) {
        let face = db.face(&family, &style);
        if !face.family.eq_ignore_ascii_case(&family) {
            warnings.push(format!("font `{family} {style}` is not available; not embedded"));
        } else if face.restricted_licence() || face.bitmap_only() {
            warnings.push(format!("font `{family} {style}`: its licence doesn't allow embedding; not embedded"));
        } else if face.data().starts_with(b"ttcf") {
            warnings.push(format!("font `{family} {style}` is part of a font collection; not embedded"));
        } else {
            out.push(designcraft_epub::EmbeddedFont { family, style: face.style.clone(), data: face.data().to_vec() });
        }
    }
    out
}

/// A Split Document setting (`"off"`, `"exportTags"` or `{"style": name}`), naming a paragraph
/// style the document has.
pub(crate) fn epub_split_param(id: &str, doc: &designcraft_doc::Document, v: &Value) -> Result<designcraft_doc::EpubSplit> {
    let split: designcraft_doc::EpubSplit =
        serde_json::from_value(v.clone()).map_err(|_| bad(id, "`split`: \"off\", \"exportTags\" or {\"style\": name}"))?;
    if let designcraft_doc::EpubSplit::Style(name) = &split
        && doc.styles.para(name).is_none()
    {
        return Err(bad(id, format!("no paragraph style `{name}`")));
    }
    Ok(split)
}

fn export_epub(s: &mut Session, p: &Value) -> Result<Value> {
    let st = s.doc()?;
    let split = p.get("split").map(|v| epub_split_param("file.exportEpub", &st.doc, v)).transpose()?;
    // Object Export Options › Rasterize: those objects go in as images.
    let (raster, _) = rasterize_where(&st.doc, &s.cache, 144.0, false, |it| it.export_options.rasterize && !threaded(&st.doc, it));
    let epub_doc: &designcraft_doc::Document = raster.as_ref().unwrap_or(&st.doc);
    let fixed = p.get("fixedLayout").and_then(Value::as_bool).unwrap_or(false);
    let want_cover = p.get("cover").and_then(Value::as_bool).unwrap_or(false);
    let mut rr = designcraft_render::Renderer::new();
    rr.threads = designcraft_render::default_threads();
    let ropts = designcraft_render::RenderOptions { printing_only: true, ..Default::default() };
    let mut warnings: Vec<String> = Vec::new();
    // A cover image file (PNG or JPEG), else the first page rendered when `cover` asks for it.
    let cover = match p.get("coverImage").and_then(Value::as_str).filter(|_| !fixed) {
        Some(path) => Some(super::datamerge::read_path_capped(path, super::datamerge::MAX_IMAGE_FILE_BYTES).map_err(crate::EngineError::Other)?),
        None => (want_cover && !fixed).then(|| rr.render_page(&st.doc, &s.cache, 0, 2.0, false, &ropts).map(|img| img.to_png())).flatten(),
    };
    if let Some(c) = &cover
        && !(c.starts_with(b"\x89PNG") || c.starts_with(&[0xFF, 0xD8, 0xFF]))
    {
        return Err(crate::EngineError::Other("the cover image must be a PNG or JPEG file".into()));
    }
    let opts = designcraft_epub::EpubOptions {
        title: p.get("title").and_then(Value::as_str).map(str::to_string),
        author: p.get("author").and_then(Value::as_str).map(str::to_string),
        language: p.get("language").and_then(Value::as_str).unwrap_or("en").to_string(),
        identifier: p.get("identifier").and_then(Value::as_str).map(str::to_string),
        cover,
        toc_title: p.get("tocTitle").and_then(Value::as_str).map(str::to_string),
        split,
        generic_families: generic_families(&st.doc),
        fonts: if p.get("embedFonts").and_then(Value::as_bool).unwrap_or(false) && !fixed {
            embedded_fonts(&st.doc, &mut warnings)
        } else {
            Vec::new()
        },
        // The export time; on the web the document's own save time stands in.
        #[cfg(not(target_arch = "wasm32"))]
        modified: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok().and_then(|d| i64::try_from(d.as_secs()).ok()),
        #[cfg(target_arch = "wasm32")]
        modified: None,
    };
    if fixed {
        let d = &st.doc;
        let mut pages = Vec::new();
        for i in 0..d.page_count() {
            let Some((si, pi)) = d.page_loc(i) else {
                continue;
            };
            let b = d.spreads[si].pages[pi].bounds();
            let Some(img) = rr.render_page(d, &s.cache, i, 2.0, false, &ropts) else {
                continue;
            };
            pages.push(designcraft_epub::FixedPage { png: img.to_png(), width: b.width(), height: b.height(), text: page_text(d, &s.cache, i) });
        }
        let bytes = designcraft_epub::export_fixed_epub(d, &pages, &opts).map_err(|e| crate::EngineError::Other(e.to_string()))?;
        return match p.get("path").and_then(Value::as_str) {
            Some(path) => {
                write_file(path, &bytes)?;
                Ok(serde_json::json!({"path": path, "bytes": bytes.len(), "pages": pages.len()}))
            }
            None => Ok(serde_json::json!({"base64": super::base64_encode(&bytes), "bytes": bytes.len(), "pages": pages.len()})),
        };
    }
    let (epub_doc, eps_warnings) = eps_previews(epub_doc);
    warnings.extend(eps_warnings);
    let bytes = designcraft_epub::export_epub(&epub_doc, &opts).map_err(|e| crate::EngineError::Other(e.to_string()))?;
    warnings.extend(
        designcraft_epub::images_without_alt(&epub_doc)
            .into_iter()
            .map(|name| format!("{name}: no alt text (Object Export Options); exported as decorative (alt=\"\")")),
    );
    match p.get("path").and_then(Value::as_str) {
        Some(path) => {
            #[cfg(not(target_arch = "wasm32"))]
            crate::io::write_file(path, &bytes).map_err(crate::EngineError::Other)?;
            Ok(serde_json::json!({"path": path, "bytes": bytes.len(), "warnings": warnings}))
        }
        None => Ok(serde_json::json!({"base64": super::file::base64_encode(&bytes), "bytes": bytes.len(), "warnings": warnings})),
    }
}

/// The `pages` parameter: a range string ("1-3,5"), a list or a number of 1-based positions.
fn pages_param(id: &str, p: &Value, page_count: usize) -> Result<Option<Vec<usize>>> {
    Ok(match p.get("pages") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(designcraft_pdf::parse_page_range(s, page_count).map_err(|e| bad(id, e.to_string()))?),
        Some(Value::Array(a)) => {
            let mut v = Vec::new();
            for x in a {
                let n = x
                    .as_u64()
                    .and_then(|n| usize::try_from(n).ok())
                    .filter(|n| *n >= 1 && *n <= page_count)
                    .ok_or_else(|| bad(id, format!("bad page {x}")))?;
                // A page named twice is one page, as `"2,2"` already is: the list says which
                // pages to export, not how many times. `[2, 2]` exported two copies of page 2
                // while `"2,2"` exported one.
                if !v.contains(&(n - 1)) {
                    v.push(n - 1);
                }
            }
            Some(v)
        }
        Some(Value::Number(n)) => {
            let n = n
                .as_u64()
                .and_then(|n| usize::try_from(n).ok())
                .filter(|n| *n >= 1 && *n <= page_count)
                .ok_or_else(|| bad(id, format!("bad page {n}")))?;
            Some(vec![n - 1])
        }
        Some(v) => return Err(bad(id, format!("bad `pages`: {v}"))),
    })
}

pub(crate) fn options(p: &Value, page_count: usize) -> Result<PdfOptions> {
    const ID: &str = "file.exportPdf";

    let pages = pages_param(ID, p, page_count)?;
    let spreads = p.get("spreads").and_then(Value::as_bool).unwrap_or(false);
    let bleed = p.get("bleed").and_then(Value::as_bool).unwrap_or(false);
    let marks = match p.get("marks") {
        Some(Value::Bool(true)) => Marks::ALL,
        Some(Value::Object(m)) => {
            let b = |k: &str| m.get(k).and_then(Value::as_bool).unwrap_or(false);
            Marks {
                crop: b("crop"),
                bleed: b("bleed"),
                page_info: b("pageInfo"),
                weight: m.get("weight").and_then(Value::as_f64).unwrap_or(0.25),
                offset: m.get("offset").and_then(Value::as_f64).unwrap_or(6.0),
            }
        }
        _ => Marks::NONE,
    };
    let standard = match str_param(p, "standard") {
        Some(s) => Standard::parse(s).ok_or_else(|| bad(ID, format!("unknown standard `{s}` (none, x4, a2b)")))?,
        None => Standard::None,
    };
    let created = match p.get("created") {
        None | Some(Value::Null) => None,
        Some(v) => Some(v.as_i64().ok_or_else(|| bad(ID, "`created`: Unix seconds, an integer"))?),
    };
    Ok(PdfOptions {
        pages,
        spreads,
        bleed,
        marks,
        standard,
        compress_images: p.get("compressImages").and_then(Value::as_bool).unwrap_or(false),
        title: str_param(p, "title").map(str::to_string),
        author: str_param(p, "author").map(str::to_string),
        tagged: p.get("tagged").and_then(Value::as_bool).unwrap_or(false),
        media: p.get("media").and_then(Value::as_bool).unwrap_or(false),
        created,
        ..PdfOptions::default()
    })
}

/// Does this object (or anything in it) need the transparency flattener?
fn transparent(it: &designcraft_doc::Item) -> bool {
    it.involves_transparency()
}

/// Transparency Flattener: each object involving transparency becomes an opaque image of its
/// area as it looks over what's beneath it (so the rest stays vector); spreads whose parent items
/// involve transparency are rasterised whole. Works on a copy of the document made for export.
/// Returns the copy and the number of rasterised regions.
fn flatten(d: &designcraft_doc::Document, cache: &designcraft_compose::Cache, ppi: f64) -> (designcraft_doc::Document, usize) {
    use designcraft_doc::{Asset, AssetId, Content, Graphic, Item, ItemId, Shape, SpreadRef};
    let mut out = d.clone();
    let mut n = 0;
    let mut rr = designcraft_render::Renderer::new();
    rr.threads = designcraft_render::default_threads();
    let k = ppi / 72.0;
    let parent_transparent = |pid: Option<designcraft_doc::SpreadId>| {
        pid.and_then(|id| d.parent_index(id))
            .is_some_and(|pi| d.parent_chain(pi).iter().filter_map(|&i| d.parents.get(i)).any(|p| p.items.iter().any(|it| transparent(it))))
    };
    // An opaque image item showing `img` over `r`.
    let image_item =
        |out: &mut designcraft_doc::Document, r: designcraft_geom::Rect, img: designcraft_render::Rendered, layer: designcraft_doc::LayerId| {
            let (w, h) = (img.width, img.height);
            let aid = AssetId(out.alloc());
            out.assets.insert(
                aid,
                std::sync::Arc::new(Asset {
                    id: aid,
                    name: format!("flattened-{}.png", aid.0),
                    mime: "image/png".into(),
                    data: std::sync::Arc::new(img.to_png()),
                    pixels: Some((w, h)),
                    ..Default::default()
                }),
            );
            let mut it = Item::new(ItemId(out.alloc()), layer, Shape::Rectangle, designcraft_geom::shapes::rectangle(r));
            it.content = Content::Graphic(Graphic {
                layer_options: None,
                asset: aid,
                size: (w as f64, h as f64),
                xf: designcraft_geom::Affine::translate((r.x0, r.y0))
                    * designcraft_geom::Affine::scale_non_uniform(r.width() / w as f64, r.height() / h as f64),
                clip: None,
                auto_fit: Default::default(),
                fit_align: 4,
                crop: [0.0; 4],
                wrap: Default::default(),
                fill: Default::default(),
            });
            it.stroke.weight = 0.0;
            it
        };
    for (si, sp) in d.spreads.iter().enumerate() {
        let whole = sp.pages.iter().any(|pg| pg.show_parent_items && parent_transparent(pg.parent));
        if whole {
            // Parent items involve transparency: every page of the spread as one image.
            let first = d.first_page_of_spread(si);
            let lid = out.default_layer();
            let mut images = Vec::new();
            for (pi, pg) in sp.pages.iter().enumerate() {
                if let Some(img) = rr.render_page(d, cache, first + pi, k, false, &Default::default()) {
                    images.push((pg.bounds(), img));
                }
            }
            let osp = std::sync::Arc::make_mut(&mut out.spreads[si]);
            osp.items.clear();
            for pg in &mut osp.pages {
                pg.show_parent_items = false;
            }
            for (b, img) in images {
                let it = image_item(&mut out, b, img, lid);
                let _ = out.insert_item(SpreadRef::Doc(si), it, None);
                n += 1;
            }
            continue;
        }
        // Region by region, bottom to top.
        for (idx, it) in sp.items.iter().enumerate() {
            if it.hidden || !transparent(it) {
                continue;
            }
            let reach = it.bounds().inflate(
                designcraft_render::effect_outset(it) + it.stroke.extent() + 1.0,
                designcraft_render::effect_outset(it) + it.stroke.extent() + 1.0,
            );
            // Only the part on the spread's pages and pasteboard that prints.
            let (w, h) = ((reach.width() * k).ceil().max(1.0) as u32, (reach.height() * k).ceil().max(1.0) as u32);
            if w as u64 * h as u64 > 80_000_000 {
                continue;
            }
            // The stack up to and including this object.
            let mut below = d.clone();
            std::sync::Arc::make_mut(&mut below.spreads[si]).items.truncate(idx + 1);
            let view = designcraft_geom::Affine::scale(k) * designcraft_geom::Affine::translate((-reach.x0, -reach.y0));
            let opts = designcraft_render::RenderOptions { printing_only: true, ..Default::default() };
            let img = rr.render(
                &below,
                cache,
                &[designcraft_render::Placed { spread: SpreadRef::Doc(si), xf: designcraft_geom::Affine::translate(designcraft_geom::Vec2::ZERO) }],
                w,
                h,
                view,
                &opts,
            );
            let new = image_item(&mut out, reach, img, it.layer);
            let osp = std::sync::Arc::make_mut(&mut out.spreads[si]);
            let Some(pos) = osp.items.iter().position(|x| x.id == it.id) else {
                continue;
            };
            if threaded(d, it) {
                // Keep the frame (its story flows on) under the image.
                osp.items.insert(pos + 1, std::sync::Arc::new(new));
            } else {
                osp.items[pos] = std::sync::Arc::new(new);
            }
            n += 1;
        }
    }
    (out, n)
}

/// A text frame in a story that runs through other frames (replacing it would reflow the story).
fn threaded(d: &designcraft_doc::Document, it: &designcraft_doc::Item) -> bool {
    it.text_frame().and_then(|t| d.story(t.story)).is_some_and(|st| st.frames.len() > 1)
}

/// Soft effects other than the gradient feather (which PDF export draws itself).
fn raster_effects(it: &designcraft_doc::Item) -> bool {
    let e = &it.effects;
    // Knockout groups too: PDF output here has no knockout transparency groups.
    it.knockout
        || e.drop_shadow.on
        || e.feather > 0.0
        || e.inner_shadow.on
        || e.outer_glow.on
        || e.inner_glow.on
        || e.bevel.on
        || e.satin.on
        || e.directional_feather.on
}

/// A copy of `d` where spread-level objects with soft effects are 300 ppi images of their whole
/// appearance (transparent around them, so they composite over what's behind).
fn rasterize_effects(d: &designcraft_doc::Document, cache: &designcraft_compose::Cache) -> (Option<designcraft_doc::Document>, usize) {
    rasterize_where(d, cache, 300.0, true, |it| raster_effects(it) && !threaded(d, it))
}

/// A copy of `d` where the spread-level objects `pred` picks are images of their appearance at
/// `ppi` (transparent around them). With `keep_wrap`, each object also stays behind as a
/// nonprinting [`wrap_stand_in`], so text wraps around it as before.
fn rasterize_where(
    d: &designcraft_doc::Document,
    cache: &designcraft_compose::Cache,
    ppi: f64,
    keep_wrap: bool,
    pred: impl Fn(&designcraft_doc::Item) -> bool,
) -> (Option<designcraft_doc::Document>, usize) {
    use designcraft_doc::{Asset, AssetId, Content, Graphic, Item, ItemId, Shape, SpreadRef};
    let todo: Vec<(usize, ItemId)> =
        d.spreads.iter().enumerate().flat_map(|(si, sp)| sp.items.iter().filter(|it| !it.hidden && pred(it)).map(move |it| (si, it.id))).collect();
    if todo.is_empty() {
        return (None, 0);
    }
    let mut out = d.clone();
    let mut rr = designcraft_render::Renderer::new();
    rr.threads = designcraft_render::default_threads();
    let k = ppi / 72.0;
    let mut n = 0;
    for (si, id) in todo {
        let Some(it) = d.item(id) else { continue };
        let reach = it.bounds().inflate(
            designcraft_render::effect_outset(it) + it.stroke.extent() + 2.0,
            designcraft_render::effect_outset(it) + it.stroke.extent() + 2.0,
        );
        // Just this object on its spread, no paper.
        let mut solo = d.clone();
        let sp = std::sync::Arc::make_mut(&mut solo.spreads[si]);
        sp.items.retain(|x| x.id == id);
        for pg in &mut sp.pages {
            pg.show_parent_items = false;
        }
        let (w, h) = ((reach.width() * k).ceil().max(1.0) as u32, (reach.height() * k).ceil().max(1.0) as u32);
        if w as u64 * h as u64 > 60_000_000 {
            continue;
        }
        let view = designcraft_geom::Affine::scale(k) * designcraft_geom::Affine::translate((-reach.x0, -reach.y0));
        let opts = designcraft_render::RenderOptions { paper: false, background: None, printing_only: true, ..Default::default() };
        let img = rr.render(
            &solo,
            cache,
            &[designcraft_render::Placed { spread: SpreadRef::Doc(si), xf: designcraft_geom::Affine::translate(designcraft_geom::Vec2::ZERO) }],
            w,
            h,
            view,
            &opts,
        );
        let aid = AssetId(out.alloc());
        out.assets.insert(
            aid,
            std::sync::Arc::new(Asset {
                id: aid,
                name: format!("effects-{}.png", id.0),
                mime: "image/png".into(),
                data: std::sync::Arc::new(img.to_png()),
                pixels: Some((w, h)),
                ..Default::default()
            }),
        );
        let mut img_item = Item::new(ItemId(out.alloc()), it.layer, Shape::Rectangle, designcraft_geom::shapes::rectangle(reach));
        img_item.content = Content::Graphic(Graphic {
            layer_options: None,
            asset: aid,
            size: (w as f64, h as f64),
            xf: designcraft_geom::Affine::translate((reach.x0, reach.y0)) * designcraft_geom::Affine::scale(1.0 / k),
            clip: None,
            auto_fit: Default::default(),
            fit_align: 4,
            crop: [0.0; 4],
            wrap: Default::default(),
            fill: Default::default(),
        });
        img_item.stroke.weight = 0.0;
        img_item.alt_text = it.alt_text.clone();
        img_item.export_options = it.export_options.clone();
        // Same place in the stacking order.
        let osp = std::sync::Arc::make_mut(&mut out.spreads[si]);
        if let Some(pos) = osp.items.iter().position(|x| x.id == id) {
            osp.items[pos] = std::sync::Arc::new(img_item);
            if keep_wrap && has_wrap(it, 0) {
                osp.items.insert(pos, std::sync::Arc::new(wrap_stand_in(it, 0)));
            }
            n += 1;
        }
    }
    (Some(out), n)
}

/// Whether `it`, its placed graphic or an item in it wraps text.
fn has_wrap(it: &designcraft_doc::Item, depth: usize) -> bool {
    use designcraft_doc::{Content, WrapMode};
    if depth > MAX_STAND_IN_DEPTH {
        return false;
    }
    it.wrap.mode != WrapMode::None
        || match &it.content {
            Content::Graphic(g) => g.wrap.mode != WrapMode::None,
            Content::Group { items } => items.iter().any(|c| has_wrap(c, depth + 1)),
            _ => false,
        }
}

/// Nesting depth past which groups inside a rasterised object keep no wrap stand-ins.
const MAX_STAND_IN_DEPTH: usize = 64;

/// A nonprinting copy of `it` that keeps its text wrap (and its graphic's and its items') and
/// nothing else that reaches the PDF: no text, form field, button or media. The image of the
/// object stands in for all of that, as it does without the copy.
fn wrap_stand_in(it: &designcraft_doc::Item, depth: usize) -> designcraft_doc::Item {
    use designcraft_doc::Content;
    let mut c = it.clone();
    c.nonprinting = true;
    c.form_field = None;
    c.button = None;
    c.media = None;
    c.content = match &it.content {
        Content::Text(_) => Content::Unassigned,
        Content::Group { items } if depth < MAX_STAND_IN_DEPTH => {
            Content::Group { items: items.iter().map(|i| std::sync::Arc::new(wrap_stand_in(i, depth + 1))).collect() }
        }
        Content::Group { .. } => Content::Group { items: Vec::new() },
        other => other.clone(),
    };
    c
}

/// A copy of `d` where placed PDFs with Object Layer Options hidden layers are 300 ppi images.
///
/// Parent-page and anchored placements count too, including anchors inside table-cell,
/// footnote and endnote text. Only the export copy is rewritten.
fn rasterize_layered(d: &designcraft_doc::Document) -> Result<(Option<designcraft_doc::Document>, usize)> {
    let choices: Vec<_> = d
        .placed_items()
        .into_iter()
        .filter(|it| it.has_pdf_layer_override())
        .filter_map(|it| {
            let g = it.graphic()?;
            let a = d.assets.get(&g.asset).filter(|a| designcraft_render::is_pdf(&a.data))?;
            Some((it.id, g.clone(), a.clone(), it.pdf_hidden_layers.clone()))
        })
        .collect();
    if choices.is_empty() {
        return Ok((None, 0));
    }
    let mut out = d.clone();
    let mut images = std::collections::HashMap::new();
    let mut n = 0;
    for (id, g, a, hidden) in choices {
        // The whole page at 300 ppi (the graphic may show only a box of it).
        let shown = designcraft_render::shown_box(&a);
        let [l, t, r, b] = shown.unwrap_or([0.0, 0.0, 1.0, 1.0]);
        let page_side = (g.size.0 / (r - l)).max(g.size.1 / (b - t));
        let side = (page_side * 300.0 / 72.0).clamp(64.0, 8000.0) as u32;
        let png = designcraft_render::pdf_page_png_options_checked(&a.data, a.page as usize, side, Some(&hidden), shown)
            .map_err(|e| EngineError::Other(format!("{}: {e}", a.name)))?;
        let px = designcraft_render::image_size(&png);
        let aid = designcraft_doc::AssetId(out.alloc());
        out.assets.insert(
            aid,
            std::sync::Arc::new(designcraft_doc::Asset {
                id: aid,
                name: format!("{}.png", a.name),
                mime: "image/png".into(),
                data: std::sync::Arc::new(png),
                pixels: px,
                ..Default::default()
            }),
        );
        images.insert(id, aid);
        n += 1;
    }
    out.update_placed_items(
        |it| images.contains_key(&it.id),
        |it| {
            if let designcraft_doc::Content::Graphic(gg) = &mut it.content
                && let Some(aid) = images.get(&it.id)
            {
                gg.asset = *aid;
            }
        },
    );
    Ok((Some(out), n))
}

/// Object number of the first page of a PDF (its page tree's first kid).
fn first_page_ref(pdf: &[u8]) -> Option<usize> {
    let s = String::from_utf8_lossy(pdf);
    let i = s.find("/Kids[")? + 6;
    s[i..].split_whitespace().next()?.parse().ok()
}

fn export_pdf(s: &mut Session, p: &Value) -> Result<Value> {
    let st = s.doc()?;
    let opts = options(p, st.doc.page_count())?;
    // Transparency Flattener presets: High / Medium / Low Resolution, or a ppi.
    let ppi = match p.get("flatten") {
        None | Some(Value::Null) | Some(Value::Bool(false)) => None,
        Some(Value::String(s)) if s == "high" => Some(300.0),
        Some(Value::String(s)) if s == "medium" => Some(150.0),
        Some(Value::String(s)) if s == "low" => Some(72.0),
        Some(Value::Bool(true)) => Some(300.0),
        Some(v) => Some(
            v.as_f64().filter(|v| (36.0..=1200.0).contains(v)).ok_or_else(|| bad("file.exportPdf", "`flatten`: high|medium|low or 36–1200 ppi"))?,
        ),
    };
    let flattened = ppi.map(|ppi| flatten(&st.doc, &s.cache, ppi));
    let base: &designcraft_doc::Document = flattened.as_ref().map_or(&st.doc, |f| &f.0);
    let (layered, layer_count) = if opts.standard == Standard::PdfA2b { rasterize_layered(base)? } else { (None, 0) };
    let base2: &designcraft_doc::Document = layered.as_ref().unwrap_or(base);
    let (fx_doc, fx_count) = rasterize_effects(base2, &s.cache);
    let doc: &designcraft_doc::Document = fx_doc.as_ref().unwrap_or(base2);
    let boxed = super::interchange::with_pdf_boxes(doc);
    let doc: &designcraft_doc::Document = &boxed;
    let r = designcraft_pdf::export_pdf_with_report(doc, &s.cache, &opts).map_err(|e| EngineError::Other(e.to_string()))?;
    let mut r = r;
    // Flattening and effect rasterization replace placements, so inspect the source too.
    if st.doc.has_graphic_layer_visibility_overrides() && !r.warnings.iter().any(|w| w == designcraft_doc::GRAPHIC_LAYER_RENDERING_WARNING) {
        r.warnings.push(designcraft_doc::GRAPHIC_LAYER_RENDERING_WARNING.into());
    }
    // Transparency Blend Space: the page group of pages with transparency.
    let spreads = designcraft_pdf::sheet_spreads(doc, &opts);
    let blended: Vec<bool> = spreads.iter().map(|si| doc.spreads.get(*si).is_some_and(|sp| sp.items.iter().any(|it| transparent(it)))).collect();
    if blended.iter().any(|b| *b) {
        match designcraft_pdf::add_blend_space(&r.bytes, &blended, doc.settings.blend_space == designcraft_doc::BlendSpace::Cmyk) {
            Some(b) => r.bytes = b,
            None => r.warnings.push("the transparency blend space couldn't be set".into()),
        }
    }
    // Interactive PDF view options.
    let mut cat = String::new();
    if p.get("fullScreen").and_then(Value::as_bool) == Some(true) {
        cat.push_str("/PageMode/FullScreen");
    } else if p.get("bookmarksPanel").and_then(Value::as_bool) == Some(true) {
        cat.push_str("/PageMode/UseOutlines");
    }
    match str_param(p, "pageLayout") {
        Some("single") => cat.push_str("/PageLayout/SinglePage"),
        Some("continuous") => cat.push_str("/PageLayout/OneColumn"),
        Some("twoUp") => cat.push_str("/PageLayout/TwoPageLeft"),
        Some("twoUpCover") => cat.push_str("/PageLayout/TwoPageRight"),
        Some("twoUpContinuous") => cat.push_str("/PageLayout/TwoColumnRight"),
        Some(o) => return Err(bad("file.exportPdf", format!("unknown pageLayout `{o}`"))),
        None => {}
    }
    if let Some(v) = str_param(p, "view") {
        // The first page opens with this view (pages are objects in order, so refer by index 0).
        let dest = match v {
            "fitPage" => "/Fit",
            "fitWidth" => "/FitH null",
            "actual" => "/XYZ null null 1",
            o => {
                return Err(bad("file.exportPdf", format!("unknown view `{o}` (fitPage, fitWidth, actual)")));
            }
        };
        if let Some(first) = first_page_ref(&r.bytes) {
            cat.push_str(&format!("/OpenAction[{first} 0 R{dest}]"));
        }
    }
    if !cat.is_empty() {
        match designcraft_pdf::add_catalog_entries(&r.bytes, &cat) {
            Some(b) => r.bytes = b,
            None => r.warnings.push("the PDF view options couldn't be set".into()),
        }
    }
    // Flip pages every N seconds (presentation).
    if let Some(secs) = p.get("advanceSeconds").and_then(Value::as_f64).filter(|s| *s > 0.0) {
        let n = designcraft_pdf::sheet_spreads(doc, &opts).len();
        let dur: Vec<Option<String>> = (0..n).map(|_| Some(format!("/Dur {secs:.2}"))).collect();
        match designcraft_pdf::add_page_entries(&r.bytes, &dur) {
            Some(b) => r.bytes = b,
            None => r.warnings.push("page timings couldn't be set".into()),
        }
    }
    // Page transitions (each spread's first page holds them).
    let trans: Vec<Option<designcraft_doc::PageTransition>> =
        spreads.into_iter().map(|si| doc.spreads.get(si).and_then(|sp| sp.pages.first()).and_then(|pg| pg.transition)).collect();
    if trans.iter().any(Option::is_some) {
        match designcraft_pdf::add_transitions(&r.bytes, &trans) {
            Some(b) => r.bytes = b,
            None => r.warnings.push("page transitions couldn't be added to this PDF".into()),
        }
    }
    if fx_count > 0 {
        r.warnings.push(format!("{fx_count} object(s) with soft effects (shadows, glows, feathers, bevels) or knockout groups exported as images"));
    }
    if layer_count > 0 {
        r.warnings.push(format!("{layer_count} placed PDF(s) with hidden layers exported as images"));
    }
    if let Some((_, n)) = &flattened
        && *n > 0
    {
        r.warnings.push(format!("transparency flattened: {n} region(s) rasterised"));
    }
    match str_param(p, "path") {
        Some(path) => {
            write_file(path, &r.bytes)?;
            Ok(json!({"path": path, "bytes": r.bytes.len(), "pages": r.pages, "warnings": r.warnings}))
        }
        None => Ok(json!({"base64": super::base64_encode(&r.bytes), "bytes": r.bytes.len(), "pages": r.pages, "warnings": r.warnings})),
    }
}

#[cfg(test)]
mod placed_layer_tests {
    use std::sync::Arc;

    use designcraft_doc::{AnchoredObject, Document, Item, ItemId, SpreadRef, StoryId, Table};
    use designcraft_geom::Rect;
    use serde_json::json;

    use crate::Session;

    fn two_layer_pdf() -> Vec<u8> {
        let content = "/OC /a BDC 0 0 1 rg 0 0 50 100 re f EMC /OC /b BDC 1 0 0 rg 50 0 50 100 re f EMC";
        let objs = [
            "<</Type/Catalog/Lang(en-GB)/Pages 2 0 R/OCProperties<</OCGs[5 0 R 6 0 R]/D<</Order[5 0 R 6 0 R]/Name(Default)/Locked[5 0 R]>>>>>>"
                .to_string(),
            "<</Type/Pages/Kids[3 0 R]/Count 1>>".to_string(),
            "<</Type/Page/Parent 2 0 R/MediaBox[0 0 100 100]/Resources<</Properties<</a 5 0 R/b 6 0 R>>>>/Contents 4 0 R>>".to_string(),
            format!("<</Length {}>>\nstream\n{content}\nendstream", content.len()),
            "<</Type/OCG/Name(Blue)>>".to_string(),
            "<</Type/OCG/Name(Red)>>".to_string(),
        ];
        let mut out = b"%PDF-1.7\n".to_vec();
        let mut offs = Vec::new();
        for (i, o) in objs.iter().enumerate() {
            offs.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n{}\nendobj\n", i + 1, o).as_bytes());
        }
        let x = out.len();
        let mut t = format!("xref\n0 {}\n0000000000 65535 f\r\n", objs.len() + 1);
        for o in offs {
            t.push_str(&format!("{o:010} 00000 n\r\n"));
        }
        t.push_str(&format!("trailer\n<</Size {}/Root 1 0 R>>\nstartxref\n{x}\n%%EOF\n", objs.len() + 1));
        out.extend_from_slice(t.as_bytes());
        out
    }

    fn placement<'a>(d: &'a Document, location: &str, id: ItemId, sid: StoryId) -> &'a Item {
        match location {
            "cell" => &d.story(sid).unwrap().tables.values().next().unwrap().cells[0].text.objects[0].item,
            "footnote" => &d.story(sid).unwrap().notes[0].text.objects[0].item,
            "endnote" => &d.story(sid).unwrap().endnotes[0].text.objects[0].item,
            "anchor" => &d.story(sid).unwrap().objects[0].item,
            _ => d.item(id).unwrap(),
        }
    }

    #[test]
    fn stale_layer_fields_on_non_pdf_placements_do_not_block_pdf_a() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let result = s
            .execute(
                "file.place",
                &json!({
                    "base64": super::super::file::base64_encode(&two_layer_pdf()), "name": "layers.pdf",
                }),
            )
            .unwrap();
        let id = ItemId(result["id"].as_u64().unwrap());
        s.execute("object.layerOptions", &json!({"ids": [id.0], "hidden": ["Blue"]})).unwrap();
        // Relinking to an ordinary image can leave legacy fields on the placement.
        // They are not an actionable placed-PDF override and must not fail closed.
        let d = Arc::make_mut(&mut s.doc_mut().unwrap().doc);
        let aid = d.item(id).unwrap().graphic().unwrap().asset;
        let asset = Arc::make_mut(d.assets.get_mut(&aid).unwrap());
        asset.mime = "image/png".into();
        asset.data = Arc::new(designcraft_render::Rendered { width: 1, height: 1, pixels: vec![0, 0, 0, 0] }.to_png());
        let (copy, count) = super::rasterize_layered(d).unwrap();
        assert_eq!(count, 0);
        assert!(copy.is_none());
        s.execute("file.exportPdf", &json!({"standard": "a2b"})).unwrap();
    }

    fn check_placement(location: &str) {
        for all_on in [false, true] {
            check_choice(location, all_on);
        }
    }

    fn check_choice(location: &str, all_on: bool) {
        let mut s = Session::new();
        s.execute("file.new", &json!({"width": 100, "height": 100, "pages": 1})).unwrap();
        let placed = s
            .execute(
                "file.place",
                &json!({
                    "base64": super::super::file::base64_encode(&two_layer_pdf()), "name": "layers.pdf",
                    "x": 0, "y": 0, "width": 100,
                    "spread": {"kind": if location == "parent" { "parent" } else { "doc" }, "index": 0}
                }),
            )
            .unwrap();
        let id = ItemId(placed["id"].as_u64().unwrap());
        let hidden = if all_on { vec![] } else { vec!["Blue"] };
        s.execute("object.layerOptions", &json!({"ids": [id.0], "hidden": hidden, "useDefaults": false})).unwrap();
        let sid = {
            let d = Arc::make_mut(&mut s.doc_mut().unwrap().doc);
            let layer = d.default_layer();
            let (_, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(0.0, 0.0, 80.0, 80.0), layer, "Body", Default::default()).unwrap();
            if !matches!(location, "doc" | "parent") {
                let item = (**d.spreads[0].items.iter().find(|it| it.id == id).unwrap()).clone();
                Arc::make_mut(&mut d.spreads[0]).items.retain(|it| it.id != id);
                let object = AnchoredObject { item, position: Default::default() };
                let story = d.story_mut(sid).unwrap();
                match location {
                    "cell" => {
                        let mut table = Table::new(1, 1, 1, 0, 0, 20.0);
                        table.cells[0].text.insert_object(0, object);
                        story.insert_table(0, table);
                    }
                    "footnote" => {
                        story.insert_note(0, "Note", Default::default()).unwrap();
                        Arc::make_mut(&mut story.notes[0]).text.insert_object(0, object);
                    }
                    "endnote" => {
                        story.insert_endnote(0, "Endnote", Default::default()).unwrap();
                        Arc::make_mut(&mut story.endnotes[0]).text.insert_object(0, object);
                    }
                    "anchor" => story.insert_object(0, object),
                    _ => assert!(matches!(location, "doc" | "parent")),
                }
            }
            sid
        };
        // Exercise the real export command for each location, not just the traversal helper.
        let result = s.execute("file.exportPdf", &json!({"standard": "a2b"})).unwrap();
        assert!(
            result["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap().contains("1 placed PDF(s)")),
            "{location}: {}",
            result["warnings"]
        );
        // Check the rewritten placement, including its pixels. Counting a placement without
        // updating the nested object would still export the file's default layer visibility.
        let source = &s.doc().unwrap().doc;
        let (copy, count) = super::rasterize_layered(source).unwrap();
        assert_eq!(count, 1, "{location}");
        let copy = copy.unwrap();
        let item = placement(&copy, location, id, sid);
        let asset = &copy.assets[&item.graphic().unwrap().asset];
        assert_eq!(asset.mime, "image/png", "{location}");
        let image = designcraft_render::images::decoded(&asset.data, 0);
        let image = image.unwrap();
        let pixel = image.data()[usize::from(image.height()) / 2 * usize::from(image.width()) + usize::from(image.width()) / 4];
        assert_eq!(pixel.a, if all_on { 255 } else { 0 }, "Blue visibility at {location}, all_on={all_on}");
        // Export preparation must not change the open document's placement or source asset.
        let original = placement(source, location, id, sid);
        assert_eq!(original.pdf_hidden_layers, hidden);
        assert!(original.pdf_layer_override);
        assert_eq!(source.assets[&original.graphic().unwrap().asset].mime, "application/pdf");
    }

    #[test]
    fn layered_pdf_a_document_placement() {
        check_placement("doc");
    }
    #[test]
    fn layered_pdf_a_parent_placement() {
        check_placement("parent");
    }
    #[test]
    fn layered_pdf_a_story_anchor() {
        check_placement("anchor");
    }
    #[test]
    fn layered_pdf_a_table_cell_anchor() {
        check_placement("cell");
    }
    #[test]
    fn layered_pdf_a_footnote_anchor() {
        check_placement("footnote");
    }
    #[test]
    fn layered_pdf_a_endnote_anchor() {
        check_placement("endnote");
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn write_file(path: &str, bytes: &[u8]) -> Result<()> {
    crate::io::write_file(path, bytes).map_err(EngineError::Other)
}
#[cfg(target_arch = "wasm32")]
fn write_file(path: &str, _bytes: &[u8]) -> Result<()> {
    Err(EngineError::Other(format!("{path}: no file system on the web; omit `path` to get the bytes")))
}

#[cfg(test)]
mod text_tests {
    use serde_json::json;

    use crate::Session;

    fn unzip(b64: &str) -> zip::ZipArchive<std::io::Cursor<Vec<u8>>> {
        zip::ZipArchive::new(std::io::Cursor::new(super::super::file::base64_decode(b64))).unwrap()
    }

    fn read(z: &mut zip::ZipArchive<std::io::Cursor<Vec<u8>>>, name: &str) -> String {
        let mut s = String::new();
        std::io::Read::read_to_string(&mut z.by_name(name).unwrap(), &mut s).unwrap();
        s
    }

    /// Split Document is stored with the document as one undo step, follows a renamed style, and
    /// an export can override it without storing its own.
    #[test]
    fn epub_split_document_setting() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 400, 300], "content": "text", "text": "One\nText\nTwo\nMore"})).unwrap();
        let sid = r["story"].as_u64().unwrap();
        s.execute("style.paragraph.create", &json!({"name": "Chapter"})).unwrap();
        for at in [0, 9] {
            s.execute("text.select", &json!({"story": sid, "anchor": at, "focus": at})).unwrap();
            s.execute("style.paragraph.apply", &json!({"name": "Chapter"})).unwrap();
        }
        let files = |s: &mut Session, p: serde_json::Value| {
            let mut z = unzip(s.execute("file.exportEpub", &p).unwrap()["base64"].as_str().unwrap());
            (1..).take_while(|n| z.by_name(&if *n == 1 { "OEBPS/content.xhtml".into() } else { format!("OEBPS/content{n}.xhtml") }).is_ok()).count()
        };
        assert_eq!(s.execute("document.epubSplit", &json!({})).unwrap()["split"], "off");
        assert_eq!(files(&mut s, json!({})), 1, "one content file by default");
        assert!(s.execute("document.epubSplit", &json!({"split": {"style": "Nope"}})).is_err());
        assert!(s.execute("document.epubSplit", &json!({"split": "h1"})).is_err());
        assert!(s.execute("file.exportEpub", &json!({"split": {"style": "Nope"}})).is_err());
        assert_eq!(s.execute("document.epubSplit", &json!({"split": {"style": "Chapter"}})).unwrap()["split"], json!({"style": "Chapter"}));
        assert_eq!(files(&mut s, json!({})), 2);
        assert_eq!(files(&mut s, json!({"split": "off"})), 1);
        assert_eq!(s.doc().unwrap().doc.settings.epub_split, designcraft_doc::EpubSplit::Style("Chapter".into()), "the override isn't stored");
        s.execute("style.paragraph.edit", &json!({"name": "Chapter", "rename": "Part"})).unwrap();
        assert_eq!(s.execute("document.epubSplit", &json!({})).unwrap()["split"], json!({"style": "Part"}));
        assert_eq!(files(&mut s, json!({})), 2);
        s.execute("edit.undo", &json!({})).unwrap();
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.execute("document.epubSplit", &json!({})).unwrap()["split"], "off");
    }

    #[test]
    fn object_export_options() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        // A plain rectangle rasterised for EPUB, centred, after a page break.
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 200, 200], "shape": "ellipse"})).unwrap();
        s.execute("object.fill", &json!({"swatch": "[Black]", "ids": [r["id"]]})).unwrap();
        s.execute(
            "object.exportOptions",
            &json!({"ids": [r["id"]], "rasterize": true, "align": "center", "pageBreakBefore": true, "altText": "A dot"}),
        )
        .unwrap();
        let e = s.execute("file.exportEpub", &json!({})).unwrap();
        let mut z = unzip(e["base64"].as_str().unwrap());
        let x = read(&mut z, "OEBPS/content.xhtml");
        assert!(
            x.contains("<figure style=\"text-align:center;page-break-before:always;break-before:page;\"><img") && x.contains("alt=\"A dot\""),
            "{x}"
        );
        // Tagged PDF: an artifact object adds no figure.
        let pdf = |s: &mut Session| {
            String::from_utf8_lossy(&super::super::file::base64_decode(
                s.execute("file.exportPdf", &json!({"tagged": true})).unwrap()["base64"].as_str().unwrap(),
            ))
            .matches("/S/Figure")
            .count()
        };
        let figures = pdf(&mut s);
        s.execute("object.exportOptions", &json!({"ids": [r["id"]], "artifact": true})).unwrap();
        assert!(pdf(&mut s) < figures);
    }

    #[test]
    fn epub_cover_and_fixed_layout() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 2})).unwrap();
        s.execute("frame.create", &json!({"rect": [72, 72, 400, 200], "content": "text", "text": "Page one words", "caret": false})).unwrap();
        let r = s.execute("file.exportEpub", &json!({"cover": true})).unwrap();
        assert_eq!(r["warnings"], json!([]), "no images, nothing to report");
        let mut z = unzip(r["base64"].as_str().unwrap());
        let opf = read(&mut z, "OEBPS/content.opf");
        assert!(opf.contains("cover-image") && opf.contains("<itemref idref=\"cover\"/>"));
        assert!(z.by_name("OEBPS/images/cover.png").is_ok());
        let r = s.execute("file.exportFixedEpub", &json!({})).unwrap();
        assert_eq!(r["pages"], 2);
        let mut z = unzip(r["base64"].as_str().unwrap());
        let opf = read(&mut z, "OEBPS/content.opf");
        assert!(opf.contains("rendition:layout\">pre-paginated") && opf.contains("page2.xhtml"));
        let p1 = read(&mut z, "OEBPS/page1.xhtml");
        assert!(p1.contains("width=612, height=792") && p1.contains("Page one words"), "{p1}");
    }

    /// #775: the fallback follows the font's class, and embedFonts packages the stylesheet's fonts
    /// (and reports those it can't).
    #[test]
    fn epub_font_fallbacks_and_embedding() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 1})).unwrap();
        s.execute("style.paragraph.create", &json!({"name": "Sans", "chars": {"fontFamily": "Source Sans 3", "fontStyle": "Regular"}})).unwrap();
        s.execute("style.paragraph.create", &json!({"name": "Gone", "chars": {"fontFamily": "No Such Font Family", "fontStyle": "Regular"}}))
            .unwrap();
        s.execute("frame.create", &json!({"rect": [72, 72, 400, 200], "content": "text", "text": "Words", "caret": false})).unwrap();
        let r = s.execute("file.exportEpub", &json!({})).unwrap();
        let mut z = unzip(r["base64"].as_str().unwrap());
        let css = read(&mut z, "OEBPS/style.css");
        assert!(css.contains("font-family: \"Source Sans 3\", sans-serif;"), "{css}");
        assert!(!css.contains("@font-face") && r["warnings"] == json!([]), "nothing embedded unless asked");
        let r = s.execute("file.exportEpub", &json!({"embedFonts": true})).unwrap();
        let mut z = unzip(r["base64"].as_str().unwrap());
        let css = read(&mut z, "OEBPS/style.css");
        assert!(css.contains("@font-face { font-family: \"Source Sans 3\"; src: url(\"fonts/"), "{css}");
        let opf = read(&mut z, "OEBPS/content.opf");
        let font = opf.split("href=\"fonts/").nth(1).and_then(|r| r.split('"').next()).map(|n| format!("OEBPS/fonts/{n}"));
        assert!(font.as_ref().is_some_and(|f| z.by_name(f).is_ok_and(|e| e.size() > 1000)), "{opf}");
        let warnings = r["warnings"].as_array().unwrap();
        assert!(warnings.iter().any(|w| w.as_str().unwrap().contains("No Such Font Family")), "{warnings:?}");
        let html = s.execute("file.exportHtml", &json!({})).unwrap()["text"].as_str().unwrap().to_string();
        assert!(html.contains("\"Source Sans 3\", sans-serif;"));
    }

    /// A font whose licence allows embedding bitmaps only (fsType bit 9) is not packaged whole,
    /// and a Semibold style asks for the Semibold face's weight, not Bold's.
    #[test]
    fn epub_embedding_follows_bitmap_only_licences_and_weights() {
        use designcraft_fonts::testing::{font_with, with_fs_type};
        const BITMAP: &str = "DocFont Epub Bitmap Only";
        for fs_type in [0x0200, 0x0208] {
            designcraft_fonts::FontDb::global()
                .add_font(with_fs_type(font_with(&format!("{BITMAP} {fs_type:04x}"), &['W']).unwrap(), fs_type).unwrap());
        }
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 1})).unwrap();
        for fs_type in [0x0200, 0x0208] {
            let family = format!("{BITMAP} {fs_type:04x}");
            s.execute("style.paragraph.create", &json!({"name": family, "chars": {"fontFamily": family, "fontStyle": "Regular"}})).unwrap();
        }
        s.execute("style.paragraph.create", &json!({"name": "Semi", "chars": {"fontFamily": "Source Sans 3", "fontStyle": "Semibold"}})).unwrap();
        s.execute("frame.create", &json!({"rect": [72, 72, 400, 200], "content": "text", "text": "Words", "caret": false})).unwrap();
        let r = s.execute("file.exportEpub", &json!({"embedFonts": true})).unwrap();
        let mut z = unzip(r["base64"].as_str().unwrap());
        let css = read(&mut z, "OEBPS/style.css");
        assert!(!css.contains(&format!("@font-face {{ font-family: \"{BITMAP}")), "{css}");
        let warnings = r["warnings"].to_string();
        for fs_type in [0x0200, 0x0208] {
            assert!(warnings.contains(&format!("{BITMAP} {fs_type:04x} Regular`: its licence doesn't allow embedding")), "{warnings}");
        }
        let semi = css.lines().find(|l| l.contains(".semi {")).unwrap_or_default();
        assert!(semi.contains("font-weight: 600;"), "{css}");
    }

    /// #771, #773: a cover image file and an identifier through the command; the package's
    /// modified time is the export's.
    #[test]
    fn epub_cover_image_file_and_identifier() {
        let dir = std::env::temp_dir().join(format!("dc-epub-cover-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let jpg = dir.join("cover.jpg");
        std::fs::write(&jpg, [0xFF, 0xD8, 0xFF, 0xE0, 0, 16]).unwrap();
        let bad = dir.join("cover.txt");
        std::fs::write(&bad, b"not an image").unwrap();
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 1})).unwrap();
        s.execute("frame.create", &json!({"rect": [72, 72, 400, 200], "content": "text", "text": "Words", "caret": false})).unwrap();
        let r = s.execute("file.exportEpub", &json!({"coverImage": jpg.to_str().unwrap(), "identifier": "urn:isbn:9780000000002"})).unwrap();
        let mut z = unzip(r["base64"].as_str().unwrap());
        let opf = read(&mut z, "OEBPS/content.opf");
        assert!(opf.contains("<dc:identifier id=\"bookid\">urn:isbn:9780000000002</dc:identifier>"), "{opf}");
        assert!(opf.contains("href=\"images/cover.jpg\" media-type=\"image/jpeg\""), "{opf}");
        assert!(!opf.contains("2026-01-01T00:00:00Z"), "the export time, not a fixed date: {opf}");
        assert!(z.by_name("OEBPS/images/cover.jpg").is_ok());
        assert!(s.execute("file.exportEpub", &json!({"coverImage": bad.to_str().unwrap()})).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn flattener_rasterises_spreads_with_transparency() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 3})).unwrap();
        let id = s.execute("frame.create", &json!({"rect": [100, 100, 300, 300]})).unwrap()["id"].clone();
        s.execute("object.fill", &json!({"swatch": "[Black]", "ids": [id]})).unwrap();
        // Opaque: nothing to flatten.
        let r = s.execute("file.exportPdf", &json!({"flatten": "low"})).unwrap();
        assert!(r["warnings"].as_array().unwrap().iter().all(|w| !w.as_str().unwrap().contains("flattened")));
        s.execute("object.opacity", &json!({"ids": [id], "opacity": 0.5})).unwrap();
        // Text away from the transparency stays live text.
        s.execute("frame.create", &json!({"rect": [72, 400, 400, 450], "content": "text", "text": "Still vector", "caret": false})).unwrap();
        let r = s.execute("file.exportPdf", &json!({"flatten": "low"})).unwrap();
        let pdf = super::super::file::base64_decode(r["base64"].as_str().unwrap());
        assert!(String::from_utf8_lossy(&pdf).contains("/FontFile"), "the text is still text");
        assert!(r["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap().contains("flattened: 1 ")), "{r}");
        assert_eq!(r["pages"], 3);
        let bytes = super::super::file::base64_decode(r["base64"].as_str().unwrap());
        assert_eq!(designcraft_render::pdf_page_count(&bytes), Some(3));
        // The document itself is untouched.
        assert!(s.doc().unwrap().doc.item(designcraft_doc::ItemId(id.as_u64().unwrap())).is_some());
        assert!(s.execute("file.exportPdf", &json!({"flatten": 5})).is_err());
        // Interactive view options land in the catalog and on the pages.
        let r = s.execute("file.exportPdf", &json!({"fullScreen": true, "pageLayout": "single", "view": "fitPage", "advanceSeconds": 5})).unwrap();
        let t = String::from_utf8_lossy(&super::super::file::base64_decode(r["base64"].as_str().unwrap())).to_string();
        assert!(
            t.contains("/PageMode/FullScreen") && t.contains("/PageLayout/SinglePage") && t.contains("/OpenAction[") && t.contains("/Dur 5.00"),
            "view options"
        );
        assert!(s.execute("file.exportPdf", &json!({"pageLayout": "sideways"})).is_err());
        // Unflattened, the page blends in the document's blend space (CMYK for print).
        let r = s.execute("file.exportPdf", &json!({})).unwrap();
        let text = String::from_utf8_lossy(&super::super::file::base64_decode(r["base64"].as_str().unwrap())).to_string();
        assert_eq!(text.matches("/S/Transparency/CS/DeviceCMYK").count(), 1, "only the page with transparency");
    }

    #[test]
    fn objects_with_soft_effects_keep_their_text_wrap_in_pdf() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let text = "Words wrap around the objects on this page. ".repeat(60);
        let t = s.execute("frame.create", &json!({"rect": [36, 36, 576, 756], "content": "text", "text": text, "caret": false})).unwrap()["id"]
            .as_u64()
            .unwrap();
        // A feathered frame with a contour wrap of the frame itself.
        let a = s.execute("frame.create", &json!({"rect": [100, 100, 200, 200]})).unwrap()["id"].clone();
        s.execute("object.fill", &json!({"swatch": "[Black]", "ids": [a]})).unwrap();
        s.execute("object.textWrap", &json!({"ids": [a], "mode": "contour", "contour": "sameAsClipping", "offset": 6})).unwrap();
        // A feathered frame whose placed graphic has a bounding-box wrap.
        let png = designcraft_render::Rendered { width: 40, height: 40, pixels: vec![90; 40 * 40 * 4] }.to_png();
        let b = s.execute("frame.create", &json!({"rect": [300, 400, 400, 500]})).unwrap()["id"].as_u64().unwrap();
        s.execute("file.place", &json!({"base64": super::super::file::base64_encode(&png), "name": "a.png", "frame": b})).unwrap();
        s.execute("object.textWrap", &json!({"ids": [b], "content": true, "mode": "boundingBox", "offset": 6})).unwrap();
        s.execute("object.feather", &json!({"ids": [a, b], "width": 14})).unwrap();

        let d = &s.doc().unwrap().doc;
        let sid = d.item(designcraft_doc::ItemId(t)).and_then(|it| it.text_frame()).unwrap().story;
        let before = designcraft_compose::frame_specs(d, sid)[0].exclusions.clone();
        assert_eq!(before.len(), 2);
        let (fx, n) = super::rasterize_effects(d, &s.cache);
        assert_eq!(n, 2);
        let fx = fx.unwrap();
        assert_eq!(designcraft_compose::frame_specs(&fx, sid)[0].exclusions, before);
        // The objects that keep the wrap don't print; their images do.
        let sp = &fx.spreads[0];
        assert_eq!(sp.items.iter().filter(|it| it.nonprinting).count(), 2);
        assert_eq!(sp.items.iter().filter(|it| !it.nonprinting && it.graphic().is_some()).count(), 2);
    }

    #[test]
    fn export_story_as_text_and_rtf() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 400, 300], "content": "text", "text": "One\nTwo"})).unwrap();
        assert!(s.execute("file.exportText", &json!({"frame": r["id"]})).unwrap()["text"] == "One\r\nTwo");
        let html = s.execute("file.exportHtml", &json!({})).unwrap();
        assert!(html["text"].as_str().unwrap().contains("One</p>"));
        let tagged = s.execute("file.exportText", &json!({"frame": r["id"], "format": "tagged"})).unwrap();
        assert!(tagged["text"].as_str().unwrap().contains("<ParaStyle:"));
        let rtf = s.execute("file.exportText", &json!({"frame": r["id"], "format": "rtf"})).unwrap();
        assert!(rtf["text"].as_str().unwrap().starts_with("{\\rtf1"));
        let dir = std::env::temp_dir().join(format!("dc-export-text-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("story.rtf");
        s.execute("file.exportText", &json!({"frame": r["id"], "path": path.to_string_lossy()})).unwrap();
        assert!(std::fs::read_to_string(&path).unwrap().starts_with("{\\rtf1"));
        std::fs::remove_dir_all(&dir).ok();
    }
}

#[cfg(test)]
mod tagged_tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn tagged_pdf_has_structure_and_alt_text() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("frame.create", &json!({"rect": [72, 72, 400, 300], "content": "text", "text": "Hello"})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 320, 200, 420]})).unwrap();
        s.execute("object.altText", &json!({"text": "A plain box", "ids": [r["id"]]})).unwrap();
        let pdf = |s: &mut Session, tagged: bool| {
            let b64 = s.execute("file.exportPdf", &json!({"tagged": tagged})).unwrap()["base64"].as_str().unwrap().to_string();
            String::from_utf8_lossy(&super::super::file::base64_decode(&b64)).into_owned()
        };
        let t = pdf(&mut s, true);
        assert!(t.contains("/StructTreeRoot") && t.contains("/Figure") && t.contains("A plain box"), "tagged");
        assert!(t.contains("/P") && t.contains("/MarkInfo"));
        assert!(!pdf(&mut s, false).contains("/StructTreeRoot"));
    }
}

#[cfg(test)]
mod variable_font_tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn variable_font_instance_exports_to_pdf() {
        let Ok(data) = std::fs::read("/System/Library/Fonts/Supplemental/Skia.ttf") else {
            return;
        };
        designcraft_fonts::FontDb::global().add_font(data);
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 400, 300], "content": "text", "text": "Variable"})).unwrap();
        s.execute("text.select", &json!({"story": r["story"], "anchor": 0, "focus": 8})).unwrap();
        s.execute("type.char", &json!({"fontFamily": "Skia", "fontStyle": "Black"})).unwrap();
        let out = s.execute("file.exportPdf", &json!({})).unwrap();
        assert!(out["bytes"].as_u64().unwrap() > 1000);
        let fonts = s.execute("font.list", &json!({})).unwrap();
        assert!(fonts.to_string().contains("Black"), "{fonts}");
    }
}

#[cfg(test)]
mod booklet_tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn booklet_imposes_pages_in_pairs() {
        use designcraft_pdf::{BookletKind, booklet_pairs};
        assert_eq!(booklet_pairs(8, BookletKind::SaddleStitch), [(Some(7), Some(0)), (Some(1), Some(6)), (Some(5), Some(2)), (Some(3), Some(4))]);
        assert_eq!(booklet_pairs(6, BookletKind::SaddleStitch)[0], (None, Some(0)), "padded with blanks");
        assert_eq!(booklet_pairs(3, BookletKind::TwoUpConsecutive), [(Some(0), Some(1)), (Some(2), None)]);
        let mut s = Session::new();
        s.execute("file.new", &json!({"pages": 4})).unwrap();
        let r = s.execute("file.printBooklet", &json!({"spaceBetween": 18})).unwrap();
        assert_eq!(r["sheets"], 2);
        let pdf = String::from_utf8_lossy(&super::super::file::base64_decode(r["base64"].as_str().unwrap())).into_owned();
        let mb = pdf.find("/MediaBox").map(|i| pdf[i..i + 40].to_string()).unwrap_or_default();
        assert!(mb.contains("1242"), "two 612 pt pages and an 18 pt gap: {mb}");
        assert!(s.execute("file.printBooklet", &json!({"type": "perfectBound"})).is_err());
        // Page 1's black box lands on the right half of the first sheet (page 4 is on the left).
        let id = s.execute("frame.create", &json!({"rect": [100, 100, 300, 300]})).unwrap()["id"].clone();
        s.execute("object.fill", &json!({"swatch": "[Black]", "ids": [id]})).unwrap();
        let r = s.execute("file.printBooklet", &json!({})).unwrap();
        let bytes = super::super::file::base64_decode(r["base64"].as_str().unwrap());
        let px = designcraft_render::decode_pixmap_page(&bytes, 0).expect("rasterised");
        let (w, h) = (px.width() as f64, px.height() as f64);
        let at = |x: f64, y: f64| px.sample((x / 1224.0 * w) as u16, (y / 792.0 * h) as u16);
        let (ink, blank) = (at(612.0 + 200.0, 200.0), at(200.0, 200.0));
        assert!(ink.a > 200 && ink.r < 60, "right half has page 1: {ink:?}");
        assert!(blank.a < 30 || blank.r > 200, "left half (page 4) is empty: {blank:?}");
    }
}

#[cfg(test)]
mod placed_pdf_print_tests {
    use serde_json::json;

    use crate::Session;
    use crate::cmd::{base64_decode, base64_encode};

    /// A one-page PDF 1.7 with a black box, as VectorCraft writes a logo.
    fn logo() -> Vec<u8> {
        let mut s = Session::new();
        s.execute("file.new", &json!({"width": 100, "height": 50})).unwrap();
        s.execute("frame.create", &json!({"rect": [10, 10, 90, 40]})).unwrap();
        s.execute("object.fill", &json!({"swatch": "[Black]"})).unwrap();
        let r = s.execute("file.exportPdf", &json!({})).unwrap();
        base64_decode(r["base64"].as_str().unwrap())
    }

    fn page_with(pdf: &[u8]) -> Session {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("file.place", &json!({"base64": base64_encode(pdf), "name": "logo.pdf", "x": 72, "y": 72})).unwrap();
        s
    }

    /// A placed PDF 1.7 failed the PDF/X-4 (PDF 1.6) export with
    /// "PDF writer error: Pdf(PdfDocument(PdfDocumentRepr { .. }), VersionMismatch(Pdf17), None)".
    #[test]
    fn pdf_17_placed_in_pdfx4() {
        let logo = logo();
        assert!(logo.starts_with(b"%PDF-1.7"));
        let mut s = page_with(&logo);
        let r = s.execute("file.exportPdf", &json!({"standard": "x4"})).unwrap();
        assert!(r["warnings"].to_string().contains("logo.pdf: a PDF 1.7 placed in a PDF 1.6 file"), "{}", r["warnings"]);
        assert!(base64_decode(r["base64"].as_str().unwrap()).starts_with(b"%PDF-1.6"));
        // PDF 2.0 can't simply be relabelled: the export fails and names the file.
        let mut v20 = logo.clone();
        v20[5..8].copy_from_slice(b"2.0");
        let mut s = page_with(&v20);
        let e = s.execute("file.exportPdf", &json!({"standard": "x4"})).unwrap_err().to_string();
        assert!(e.contains("logo.pdf is PDF 2.0") && e.contains("PDF 1.6"), "{e}");
    }

    /// A one-page PDF with a black box whose catalog says `/Version /1.7`, as TCPDF writes it.
    fn catalog_v17(header: &str) -> Vec<u8> {
        let content = "0 0 0 1 k 0 0 200 200 re f";
        let mut pdf = format!("{header}\n").into_bytes();
        let mut offsets = Vec::new();
        for body in [
            "<</Type/Catalog/Pages 2 0 R/Version /1.7>>".to_owned(),
            "<</Type/Pages/Kids[3 0 R]/Count 1>>".to_owned(),
            "<</Type/Page/Parent 2 0 R/MediaBox[0 0 200 200]/Contents 4 0 R/Resources<<>>>>".to_owned(),
            format!("<</Length {}>>\nstream\n{content}\nendstream", content.len()),
        ] {
            offsets.push(pdf.len());
            pdf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", offsets.len()).as_bytes());
        }
        let xref = pdf.len();
        pdf.extend_from_slice(b"xref\n0 5\n0000000000 65535 f\r\n");
        for offset in offsets {
            pdf.extend_from_slice(format!("{offset:010} 00000 n\r\n").as_bytes());
        }
        pdf.extend_from_slice(format!("trailer\n<</Size 5/Root 1 0 R>>\nstartxref\n{xref}\n%%EOF\n").as_bytes());
        pdf
    }

    /// A placed PDF whose catalog `/Version /1.7` overrides its header failed the PDF/X-4 export
    /// with "logo.pdf is PDF 1.7", though a 1.7 header alone is relabelled (#907).
    #[test]
    fn catalog_version_17_placed_in_pdfx4() {
        for header in ["%PDF-1.7", "%PDF-1.4"] {
            let pdf = catalog_v17(header);
            assert_eq!(hayro_syntax::Pdf::new(pdf.clone()).unwrap().version(), hayro_syntax::PdfVersion::Pdf17);
            let mut s = page_with(&pdf);
            let r = s.execute("file.exportPdf", &json!({"standard": "x4"})).unwrap_or_else(|e| panic!("{header}: {e}"));
            assert!(r["warnings"].to_string().contains("logo.pdf: a PDF 1.7 placed in a PDF 1.6 file"), "{}", r["warnings"]);
            let out = base64_decode(r["base64"].as_str().unwrap());
            assert!(out.starts_with(b"%PDF-1.6"));
            assert_eq!(hayro_syntax::Pdf::new(out).unwrap().version(), hayro_syntax::PdfVersion::Pdf16);
        }
    }
}

#[cfg(test)]
mod print_group_tests {
    use serde_json::json;

    use crate::Session;
    use crate::cmd::{base64_decode, base64_encode};

    /// A one-page PDF 1.7 with a black box, as VectorCraft writes a logo.
    fn logo() -> Vec<u8> {
        let mut s = Session::new();
        s.execute("file.new", &json!({"width": 100, "height": 50})).unwrap();
        s.execute("frame.create", &json!({"rect": [10, 10, 90, 40]})).unwrap();
        s.execute("object.fill", &json!({"swatch": "[Black]"})).unwrap();
        let r = s.execute("file.exportPdf", &json!({})).unwrap();
        base64_decode(r["base64"].as_str().unwrap())
    }

    fn page_with(pdf: &[u8]) -> Session {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("file.place", &json!({"base64": base64_encode(pdf), "name": "logo.pdf", "x": 72, "y": 72})).unwrap();
        s
    }

    /// The placed PDF's transparency group blended in DeviceRGB in a print document
    /// (`/Group<</Type/Group/S/Transparency/I true/CS/DeviceRGB>>`).
    #[test]
    fn print_transparency_groups_blend_in_cmyk() {
        let mut s = page_with(&logo());
        let text = |s: &mut Session| {
            let r = s.execute("file.exportPdf", &json!({"compressImages": false})).unwrap();
            String::from_utf8_lossy(&base64_decode(r["base64"].as_str().unwrap())).into_owned()
        };
        let t = text(&mut s);
        assert!(t.contains("/S/Transparency/I true/CS/DeviceCMYK>>"), "CMYK group");
        assert!(!t.contains("DeviceRGB"), "no RGB left");
        assert!(!designcraft_pdf::has_rgb_groups(t.as_bytes()));
        // The file still reads, with the same page.
        let r = s.execute("file.exportPdf", &json!({})).unwrap();
        let bytes = base64_decode(r["base64"].as_str().unwrap());
        assert_eq!(hayro_syntax::Pdf::new(bytes).unwrap().pages().len(), 1);
        // An RGB blend space keeps RGB groups.
        s.execute("edit.transparencyBlendSpace", &json!({"space": "rgb"})).unwrap();
        let t = text(&mut s);
        assert!(t.contains("/CS/DeviceRGB") && designcraft_pdf::has_rgb_groups(t.as_bytes()));
    }
}

#[cfg(test)]
mod pptx_tests {
    use serde_json::json;

    use crate::Session;

    fn unzip(b64: &str) -> zip::ZipArchive<std::io::Cursor<Vec<u8>>> {
        zip::ZipArchive::new(std::io::Cursor::new(super::super::file::base64_decode(b64))).unwrap()
    }

    fn read(z: &mut zip::ZipArchive<std::io::Cursor<Vec<u8>>>, name: &str) -> String {
        let mut s = String::new();
        std::io::Read::read_to_string(&mut z.by_name(name).unwrap(), &mut s).unwrap();
        s
    }

    #[test]
    fn sample_exports_editable_slides() {
        let mut s = Session::new();
        s.execute("file.newSample", &json!({})).unwrap();
        let r = s.execute("file.exportPptx", &json!({})).unwrap();
        assert_eq!(r["slides"], 4);
        let mut z = unzip(r["base64"].as_str().unwrap());
        // The swatch table is a PowerPoint table; the cover picture is embedded; text is text.
        assert!(read(&mut z, "ppt/slides/slide4.xml").contains("<a:tbl>"));
        assert!(read(&mut z, "ppt/slides/slide1.xml").contains("<p:pic>"));
        let text: String = read(&mut z, "ppt/slides/slide2.xml").split("<a:t>").skip(1).filter_map(|t| t.split("</a:t>").next()).collect();
        assert!(text.contains("Notes on the Grid"), "{text}");
        assert!(read(&mut z, "ppt/presentation.xml").contains("<p:embeddedFontLst>"));

        let r = s.execute("file.exportPptx", &json!({"pages": "2-3", "embedFonts": false, "lineBreaks": "reflow"})).unwrap();
        assert_eq!(r["slides"], 2);
        assert!(r["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap().contains("fonts are not embedded")));
        assert!(s.execute("file.exportPptx", &json!({"lineBreaks": "wrap"})).is_err());
        assert!(s.execute("file.exportPptx", &json!({"pages": [9]})).is_err());
    }

    #[test]
    fn rasterize_option_places_a_picture() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 200, 200], "shape": "ellipse"})).unwrap();
        s.execute("object.fill", &json!({"swatch": "[Black]", "ids": [r["id"]]})).unwrap();
        let shapes = |s: &mut Session| {
            let r = s.execute("file.exportPptx", &json!({})).unwrap();
            read(&mut unzip(r["base64"].as_str().unwrap()), "ppt/slides/slide1.xml")
        };
        assert!(shapes(&mut s).contains("prst=\"ellipse\""));
        s.execute("object.exportOptions", &json!({"ids": [r["id"]], "rasterize": true})).unwrap();
        let x = shapes(&mut s);
        assert!(x.contains("<p:pic>") && !x.contains("prst=\"ellipse\""), "{x}");
    }
}

#[cfg(test)]
mod page_list_tests {
    use serde_json::{Value, json};

    use crate::Session;

    fn doc(pages: u64) -> Session {
        let mut s = Session::new();
        s.execute("file.new", &json!({"width": 200, "height": 200, "pages": pages, "facingPages": false, "margins": 0})).unwrap();
        s
    }

    /// The same selection written two ways exported different files: `"2,2"` is one page, because
    /// `parse_page_range` keeps the first of a repeat, while `[2, 2]` was two copies of page 2.
    /// The list says which pages to export, not how many times.
    #[test]
    fn a_page_named_twice_is_exported_once_either_way() {
        let mut s = doc(4);
        for (pages, want) in [
            (json!("2,2"), 1),
            (json!([2, 2]), 1),
            (json!([2, 2, 2]), 1),
            (json!("2-2,2"), 1),
            (json!([1, 2, 1]), 2),
            (json!("1,2,1"), 2),
            (json!([4, 1]), 2),
            (json!("4,1"), 2),
            (json!("all"), 4),
        ] {
            let r = s.execute("file.exportPdf", &json!({"pages": pages.clone()})).unwrap();
            assert_eq!(r["pages"].as_u64(), Some(want), "pdf {pages}");
            let r = s.execute("file.exportPptx", &json!({"pages": pages.clone()})).unwrap();
            assert_eq!(r["slides"].as_u64(), Some(want), "pptx {pages}");
        }
        // The two forms now agree byte for byte, and the pages come out in the document's order
        // whichever order the list names them in (which is how the export already behaved).
        let mut file = |pages: Value| {
            let r = s.execute("file.exportPdf", &json!({"pages": pages})).unwrap();
            r["base64"].as_str().unwrap_or_default().to_string()
        };
        assert_eq!(file(json!([4, 1])), file(json!("4,1")), "the array and the string agree");
        assert_eq!(file(json!([4, 1])), file(json!([1, 4])), "pages 1 and 4, in the document's order");
        assert_eq!(file(json!([2, 2])), file(json!("2")), "a repeat is the single page");
        // A page that does not exist is still refused, and an empty list still exports nothing.
        assert!(s.execute("file.exportPdf", &json!({"pages": [5]})).is_err());
        assert!(s.execute("file.exportPdf", &json!({"pages": []})).is_err());
    }
}
