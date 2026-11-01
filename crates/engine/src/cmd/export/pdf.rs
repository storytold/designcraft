use super::flatten::{flatten, rasterize_effects, rasterize_layered, shadow_masks, transparent};
use super::formats::write_file;
use super::{EngineError, Marks, PdfOptions, Result, Session, Standard, Value, bad, json, str_param};

/// The `pages` parameter: a range string ("1-3,5"), a list or a number of 1-based positions.
pub(super) fn pages_param(id: &str, p: &Value, page_count: usize) -> Result<Option<Vec<usize>>> {
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
        Some(s) => Standard::parse(s).ok_or_else(|| bad(ID, format!("unknown standard `{s}` (none, x4, x1a, a2b)")))?,
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

/// Object number of the first page of a PDF (its page tree's first kid).
fn first_page_ref(pdf: &[u8]) -> Option<usize> {
    let s = String::from_utf8_lossy(pdf);
    let i = s.find("/Kids[")? + 6;
    s[i..].split_whitespace().next()?.parse().ok()
}

pub(super) fn export_pdf(s: &mut Session, p: &Value) -> Result<Value> {
    let r = pdf_export(s, p)?;
    match str_param(p, "path") {
        Some(path) => {
            write_file(path, &r.bytes)?;
            Ok(json!({"path": path, "bytes": r.bytes.len(), "pages": r.pages, "warnings": r.warnings}))
        }
        None => Ok(json!({"base64": crate::cmd::base64_encode(&r.bytes), "bytes": r.bytes.len(), "pages": r.pages, "warnings": r.warnings})),
    }
}

/// Transparency Flattener presets (`flatten`): High / Medium / Low Resolution, or a ppi.
pub(crate) fn flatten_ppi(id: &str, p: &Value) -> Result<Option<f64>> {
    Ok(match p.get("flatten") {
        None | Some(Value::Null) | Some(Value::Bool(false)) => None,
        Some(Value::String(s)) if s == "high" => Some(300.0),
        Some(Value::String(s)) if s == "medium" => Some(150.0),
        Some(Value::String(s)) if s == "low" => Some(72.0),
        Some(Value::Bool(true)) => Some(300.0),
        Some(v) => Some(v.as_f64().filter(|v| (36.0..=1200.0).contains(v)).ok_or_else(|| bad(id, "`flatten`: high|medium|low or 36–1200 ppi"))?),
    })
}

/// The print PDF of `opts`' pages: the transparency flattener at `ppi` (if any), placed PDFs
/// with hidden layers and soft effects as images, the PDF boxes of placed graphics, the PDF
/// itself and its transparency blend space. Export PDF and File › Print add their view options
/// to it ([`pdf_export`]); Separations Preview separates it as it is.
pub(crate) fn print_pdf(
    d: &designcraft_doc::Document,
    cache: &designcraft_compose::Cache,
    opts: &PdfOptions,
    ppi: Option<f64>,
) -> Result<designcraft_pdf::ExportReport> {
    // PDF/X-1a: craft-prepress flattens the PDF in its inks; `ppi` sets its resolution.
    let x1a = opts.standard == Standard::PdfX1a;
    let mut opts = opts.clone();
    if x1a {
        opts.flatten_dpi = ppi.unwrap_or(300.0) as f32;
    }
    let flattened = if x1a { None } else { ppi.map(|ppi| flatten(d, cache, ppi)) };
    let base: &designcraft_doc::Document = flattened.as_ref().map_or(d, |f| &f.0);
    let (layered, layer_count) = if opts.standard == Standard::PdfA2b { rasterize_layered(base)? } else { (None, 0) };
    let base2: &designcraft_doc::Document = layered.as_ref().unwrap_or(base);
    let shadows = if x1a { shadow_masks(base2, cache, f64::from(opts.flatten_dpi)) } else { Default::default() };
    let (fx_doc, fx_count) = rasterize_effects(base2, cache, &shadows);
    opts.shadows = std::sync::Arc::new(shadows);
    let opts = &opts;
    let doc: &designcraft_doc::Document = fx_doc.as_ref().unwrap_or(base2);
    let boxed = crate::cmd::interchange::with_pdf_boxes(doc);
    let doc: &designcraft_doc::Document = &boxed;
    let mut r = designcraft_pdf::export_pdf_with_report(doc, cache, opts).map_err(|e| EngineError::Other(e.to_string()))?;
    // Flattening and effect rasterization replace placements, so inspect the source too.
    if d.has_graphic_layer_visibility_overrides() && !r.warnings.iter().any(|w| w == designcraft_doc::GRAPHIC_LAYER_RENDERING_WARNING) {
        r.warnings.push(designcraft_doc::GRAPHIC_LAYER_RENDERING_WARNING.into());
    }
    // Transparency Blend Space: the page group of pages with transparency. CMYK print output
    // gets its CMYK page groups from the writer.
    let spreads = designcraft_pdf::sheet_spreads(doc, opts);
    let blended: Vec<bool> = spreads.iter().map(|si| doc.spreads.get(*si).is_some_and(|sp| sp.items.iter().any(|it| transparent(it)))).collect();
    if !designcraft_pdf::print_color(doc, opts) && blended.iter().any(|b| *b) {
        match designcraft_pdf::add_blend_space(&r.bytes, &blended, doc.settings.blend_space == designcraft_doc::BlendSpace::Cmyk) {
            Some(b) => r.bytes = b,
            None => r.warnings.push("the transparency blend space couldn't be set".into()),
        }
    }
    if fx_count > 0 {
        r.warnings.push(format!("{fx_count} object(s) with soft effects (shadows, glows, feathers, bevels) exported as images"));
    }
    if layer_count > 0 {
        r.warnings.push(format!("{layer_count} placed PDF(s) with hidden layers exported as images"));
    }
    if let Some((_, n)) = &flattened
        && *n > 0
    {
        r.warnings.push(format!("transparency flattened: {n} region(s) rasterised"));
    }
    Ok(r)
}

/// The PDF that File › Export PDF writes for `p`, with its warnings: the print PDF
/// ([`print_pdf`]) with its view options, timings and transitions. File › Print prints it.
pub(crate) fn pdf_export(s: &Session, p: &Value) -> Result<designcraft_pdf::ExportReport> {
    pdf_report(&s.doc()?.doc, &s.cache, p)
}

/// [`pdf_export`] of `doc`: `file.package` writes its PDF of the packaged copy with it.
pub(crate) fn pdf_report(doc: &designcraft_doc::Document, cache: &designcraft_compose::Cache, p: &Value) -> Result<designcraft_pdf::ExportReport> {
    let opts = options(p, doc.page_count())?;
    let ppi = flatten_ppi("file.exportPdf", p)?;
    let (cat, dest) = view_options(p)?;
    let mut r = print_pdf(doc, cache, &opts, ppi)?;
    let spreads = designcraft_pdf::sheet_spreads(doc, &opts);
    let mut cat = cat;
    if let Some(dest) = dest
        && let Some(first) = first_page_ref(&r.bytes)
    {
        cat.push_str(&format!("/OpenAction[{first} 0 R{dest}]"));
    }
    if !cat.is_empty() {
        match designcraft_pdf::add_catalog_entries(&r.bytes, &cat) {
            Some(b) => r.bytes = b,
            None => r.warnings.push("the PDF view options couldn't be set".into()),
        }
    }
    // Flip pages every N seconds (presentation).
    if let Some(secs) = p.get("advanceSeconds").and_then(Value::as_f64).filter(|s| *s > 0.0) {
        let dur: Vec<Option<String>> = (0..spreads.len()).map(|_| Some(format!("/Dur {secs:.2}"))).collect();
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
    Ok(r)
}

/// The catalog entries of the interactive view options in `p` (page mode and layout) and the
/// destination the first page opens at (`view`); unknown values are refused.
fn view_options(p: &Value) -> Result<(String, Option<&'static str>)> {
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
    // The first page opens with this view (pages are objects in order, so refer by index 0).
    let dest = match str_param(p, "view") {
        None => None,
        Some("fitPage") => Some("/Fit"),
        Some("fitWidth") => Some("/FitH null"),
        Some("actual") => Some("/XYZ null null 1"),
        Some(o) => return Err(bad("file.exportPdf", format!("unknown view `{o}` (fitPage, fitWidth, actual)"))),
    };
    Ok((cat, dest))
}

/// Refuses PDF options `p` that [`pdf_report`] would refuse for a document of `pages` pages,
/// without exporting: `file.package` checks them before it writes anything.
pub(crate) fn check_pdf_options(p: &Value, pages: usize) -> Result<()> {
    options(p, pages)?;
    flatten_ppi("file.exportPdf", p)?;
    view_options(p)?;
    Ok(())
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
            String::from_utf8_lossy(&crate::cmd::file::base64_decode(&b64)).into_owned()
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

    /// A placed PDF's transparency group blends in CMYK in a print document and in RGB in a
    /// screen one.
    #[test]
    fn print_transparency_groups_blend_in_cmyk() {
        let mut s = page_with(&logo());
        let text = |s: &mut Session| {
            let r = s.execute("file.exportPdf", &json!({"compressImages": false})).unwrap();
            String::from_utf8_lossy(&base64_decode(r["base64"].as_str().unwrap())).into_owned()
        };
        let t = text(&mut s);
        // The placed page's group inherits the page group, which blends in CMYK.
        assert!(t.contains("/Group<</Type/Group/S/Transparency/I true>>"), "placed page group without a blending space");
        assert!(t.contains("/Group<</Type/Group/S/Transparency/CS/DeviceCMYK>>"), "CMYK page group");
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
        // A fixed creation time: exports a second apart would differ in their dates.
        let mut file = |pages: Value| {
            let r = s.execute("file.exportPdf", &json!({"pages": pages, "created": 0})).unwrap();
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
