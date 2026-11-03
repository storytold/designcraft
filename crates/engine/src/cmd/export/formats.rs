use super::flatten::{rasterize_where, threaded};
use super::pdf::pages_param;
use super::{Arc, Cow, EngineError, Result, Session, Value, bad, json, str_param};

pub(super) fn print_booklet(s: &mut Session, p: &Value) -> Result<Value> {
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
    let r = designcraft_pdf::export_booklet(&crate::cmd::interchange::with_pdf_boxes(&st.doc), &s.cache, &opts)
        .map_err(|e| EngineError::Other(e.to_string()))?;
    match str_param(p, "path") {
        Some(path) => {
            #[cfg(not(target_arch = "wasm32"))]
            crate::io::write_file(path, &r.bytes).map_err(EngineError::Other)?;
            Ok(json!({"path": path, "bytes": r.bytes.len(), "sheets": r.pages, "warnings": r.warnings}))
        }
        None => Ok(json!({"base64": crate::cmd::file::base64_encode(&r.bytes), "bytes": r.bytes.len(), "sheets": r.pages, "warnings": r.warnings})),
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

pub(super) fn export_html(s: &mut Session, p: &Value) -> Result<Value> {
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

pub(super) fn export_pptx(s: &mut Session, p: &Value) -> Result<Value> {
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
        None => Ok(json!({"base64": crate::cmd::file::base64_encode(&r.bytes), "bytes": r.bytes.len(), "slides": r.slides, "warnings": r.warnings})),
    }
}

pub(super) fn has_story_target(s: &Session) -> std::result::Result<(), String> {
    crate::cmd::text::story_of(s, &Value::Null).map(|_| ()).ok_or_else(|| "edit text or select a text frame".into())
}

pub(super) fn export_text(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "file.exportText";
    let sid = crate::cmd::text::story_of(s, p).ok_or_else(|| bad(ID, "edit text or select a text frame"))?;
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

pub(super) fn export_epub(s: &mut Session, p: &Value) -> Result<Value> {
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
        Some(path) => {
            Some(crate::cmd::datamerge::read_path_capped(path, crate::cmd::datamerge::MAX_IMAGE_FILE_BYTES).map_err(crate::EngineError::Other)?)
        }
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
            None => Ok(serde_json::json!({"base64": crate::cmd::base64_encode(&bytes), "bytes": bytes.len(), "pages": pages.len()})),
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
        None => Ok(serde_json::json!({"base64": crate::cmd::file::base64_encode(&bytes), "bytes": bytes.len(), "warnings": warnings})),
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn write_file(path: &str, bytes: &[u8]) -> Result<()> {
    crate::io::write_file(path, bytes).map_err(EngineError::Other)
}

#[cfg(target_arch = "wasm32")]
pub(super) fn write_file(path: &str, _bytes: &[u8]) -> Result<()> {
    Err(EngineError::Other(format!("{path}: no file system on the web; omit `path` to get the bytes")))
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
        let pdf = String::from_utf8_lossy(&crate::cmd::file::base64_decode(r["base64"].as_str().unwrap())).into_owned();
        let mb = pdf.find("/MediaBox").map(|i| pdf[i..i + 40].to_string()).unwrap_or_default();
        assert!(mb.contains("1242"), "two 612 pt pages and an 18 pt gap: {mb}");
        assert!(s.execute("file.printBooklet", &json!({"type": "perfectBound"})).is_err());
        // Page 1's black box lands on the right half of the first sheet (page 4 is on the left).
        let id = s.execute("frame.create", &json!({"rect": [100, 100, 300, 300]})).unwrap()["id"].clone();
        s.execute("object.fill", &json!({"swatch": "[Black]", "ids": [id]})).unwrap();
        let r = s.execute("file.printBooklet", &json!({})).unwrap();
        let bytes = crate::cmd::file::base64_decode(r["base64"].as_str().unwrap());
        let px = designcraft_render::decode_pixmap_page(&bytes, 0).expect("rasterised");
        let (w, h) = (px.width() as f64, px.height() as f64);
        let at = |x: f64, y: f64| px.sample((x / 1224.0 * w) as u16, (y / 792.0 * h) as u16);
        let (ink, blank) = (at(612.0 + 200.0, 200.0), at(200.0, 200.0));
        assert!(ink.a > 200 && ink.r < 60, "right half has page 1: {ink:?}");
        assert!(blank.a < 30 || blank.r > 200, "left half (page 4) is empty: {blank:?}");
    }
}

#[cfg(test)]
mod pptx_tests {
    use serde_json::json;

    use crate::Session;

    fn unzip(b64: &str) -> zip::ZipArchive<std::io::Cursor<Vec<u8>>> {
        zip::ZipArchive::new(std::io::Cursor::new(crate::cmd::file::base64_decode(b64))).unwrap()
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
