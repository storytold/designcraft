//! PDF/X-4: the output intent (our Generic CMYK profile), the PDF/X version in the document
//! information and XMP, written as an incremental update of krilla's PDF 1.6 output; and a
//! check of the requirements this exporter is responsible for.

use std::collections::HashSet;

use hayro_syntax::content::UntypedIter;
use hayro_syntax::object::{Array, Dict, Name, Object, ObjectIdentifier, Stream};
use hayro_syntax::page::Resources;

use crate::transitions::{find, int_after, object, rfind};

/// Escape a PDF literal string.
fn lit(s: &str) -> String {
    let mut o = String::from("(");
    for c in s.chars() {
        if matches!(c, '(' | ')' | '\\') {
            o.push('\\');
        }
        o.push(c);
    }
    o.push(')');
    o
}

/// Turn krilla's output into PDF/X-4. `None` when the file isn't the simple kind this
/// understands (a classic cross-reference table and plain dictionaries).
pub fn make_pdfx4(pdf: &[u8], title: &str) -> Option<Vec<u8>> {
    let t_at = rfind(pdf, b"trailer")?;
    let trailer = String::from_utf8_lossy(&pdf[t_at..]).to_string();
    let size = int_after(&trailer, "/Size")?;
    let root = int_after(&trailer, "/Root")?;
    let prev = int_after(&trailer, "startxref")?;
    let info = int_after(&trailer, "/Info");
    let id = trailer.find("/ID").and_then(|i| trailer[i..].find(']').map(|j| trailer[i..i + j + 1].to_string()));
    let cat = String::from_utf8_lossy(&pdf[object(pdf, root)?]).to_string();
    let mut out = pdf.to_vec();
    if !out.ends_with(b"\n") {
        out.push(b'\n');
    }
    let mut entries: Vec<(usize, usize)> = Vec::new();
    let mut put = |out: &mut Vec<u8>, id: usize, body: &[u8]| {
        entries.push((id, out.len()));
        out.extend_from_slice(format!("{id} 0 obj\n").as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    };
    // The output profile and the output intent.
    let icc = designcraft_color::cms::icc_out::generic_cmyk_icc();
    let (icc_id, oi_id) = (size, size + 1);
    let mut s = format!("<</N 4/Length {}>>\nstream\n", icc.len()).into_bytes();
    s.extend_from_slice(icc);
    s.extend_from_slice(b"\nendstream");
    put(&mut out, icc_id, &s);
    let cond = designcraft_color::cms::GENERIC_CMYK;
    put(
        &mut out,
        oi_id,
        format!(
            "<</Type/OutputIntent/S/GTS_PDFX/OutputConditionIdentifier{}/OutputCondition{}/Info{}/RegistryName(http://www.color.org)/DestOutputProfile {icc_id} 0 R>>",
            lit("Custom"),
            lit(cond),
            lit(cond)
        )
        .as_bytes(),
    );
    // The catalog with the intent.
    let close = cat.rfind(">>")?;
    let cat_new = format!("{}/OutputIntents[{oi_id} 0 R]{}", cat[..close].trim(), &cat[close..]);
    put(&mut out, root, cat_new.trim().as_bytes());
    // Document information: the PDF/X version and trapping state.
    let mut next = size + 2;
    let info_id = match info {
        Some(i) => {
            let body = String::from_utf8_lossy(&pdf[object(pdf, i)?]).to_string();
            let close = body.rfind(">>")?;
            put(&mut out, i, format!("{}/GTS_PDFXVersion(PDF/X-4)/Trapped/False{}", body[..close].trim(), &body[close..]).trim().as_bytes());
            i
        }
        None => {
            let i = next;
            next += 1;
            put(&mut out, i, format!("<</Title{}/GTS_PDFXVersion(PDF/X-4)/Trapped/False>>", lit(title)).as_bytes());
            i
        }
    };
    // XMP: the PDF/X identification schema, and the trapping state that Info also states.
    if let Some(meta) = int_after(&cat, "/Metadata") {
        let r = object(pdf, meta)?;
        let body = &pdf[r];
        let s0 = find(body, b"stream", 0)? + b"stream".len();
        let s0 = s0 + body[s0..].iter().take_while(|c| **c == b'\r' || **c == b'\n').count();
        let s1 = rfind(body, b"endstream")?;
        let xml = String::from_utf8_lossy(&body[s0..s1]).trim_end().to_string();
        let dict = String::from_utf8_lossy(&body[..find(body, b"stream", 0)?]).to_string();
        if dict.contains("/Filter") {
            return None;
        }
        let add = concat!(
            "<rdf:Description rdf:about=\"\" xmlns:pdfxid=\"http://www.npes.org/pdfx/ns/id/\"><pdfxid:GTS_PDFXVersion>PDF/X-4</pdfxid:GTS_PDFXVersion></rdf:Description>",
            "<rdf:Description rdf:about=\"\" xmlns:pdf=\"http://ns.adobe.com/pdf/1.3/\"><pdf:Trapped>False</pdf:Trapped></rdf:Description>"
        );
        let xml = xml.replacen("</rdf:RDF>", &format!("{add}</rdf:RDF>"), 1);
        let dict = {
            let i = dict.find("/Length")?;
            let rest = &dict[i + 7..];
            let rest = rest.trim_start();
            let end = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
            format!("{}/Length {}{}", &dict[..i], xml.len(), &rest[end..])
        };
        let mut b = dict.trim().as_bytes().to_vec();
        b.extend_from_slice(b"\nstream\n");
        b.extend_from_slice(xml.as_bytes());
        b.extend_from_slice(b"\nendstream");
        put(&mut out, meta, &b);
    }
    // Cross-reference section and trailer.
    let xref_at = out.len();
    entries.sort();
    let mut x = String::from("xref\n");
    for (oid, off) in &entries {
        x.push_str(&format!("{oid} 1\n{off:010} 00000 n\r\n"));
    }
    let id = id.unwrap_or_default();
    x.push_str(&format!("trailer\n<</Size {next}/Root {root} 0 R/Info {info_id} 0 R{id}/Prev {prev}>>\nstartxref\n{xref_at}\n%%EOF\n"));
    out.extend_from_slice(x.as_bytes());
    Some(out)
}

/// krilla gives every transparency group it writes (placed PDF pages, opacity, isolated
/// blending) a DeviceRGB blending space. In a print document that puts DeviceRGB in a CMYK file
/// (PDF/X-4 forbids it under a CMYK output intent) and blends CMYK art through RGB, which can
/// turn a K-only black into a four-colour black. This rewrites those group dictionaries to
/// DeviceCMYK in place and at the same length (the optional `/Type/Group` gives the room), so
/// every cross-reference offset stays valid. Only object dictionaries are touched, never stream
/// data. Returns how many groups changed.
/// Whether a transparency group dictionary still names DeviceRGB as its blending space.
pub fn has_rgb_groups(pdf: &[u8]) -> bool {
    pdf.windows(b"/CS/DeviceRGB".len()).enumerate().any(|(i, w)| {
        w == b"/CS/DeviceRGB" && pdf.get(i.saturating_sub(64)..i).is_some_and(|before| before.windows(15).any(|b| b == b"/S/Transparency"))
    })
}

pub fn cmyk_group_spaces(pdf: &mut [u8]) -> usize {
    const FORMS: [(&[u8], &[u8]); 2] = [
        (b"/Group<</Type/Group/S/Transparency/I true/CS/DeviceRGB>>", b"/Group<</S/Transparency/I true/CS/DeviceCMYK>>"),
        (b"/Group<</Type/Group/S/Transparency/CS/DeviceRGB>>", b"/Group<</S/Transparency/CS/DeviceCMYK>>"),
    ];
    let Some(dicts) = dictionary_ranges(pdf) else { return 0 };
    let mut changed = 0;
    for r in dicts {
        for (from, to) in FORMS {
            let mut at = r.start;
            while let Some(i) = find(pdf.get(..r.end).unwrap_or_default(), from, at) {
                if let Some(dst) = pdf.get_mut(i..i + from.len()) {
                    dst.fill(b' ');
                    if let Some(head) = dst.get_mut(..to.len()) {
                        head.copy_from_slice(to);
                        changed += 1;
                    }
                }
                at = i + from.len();
            }
        }
    }
    changed
}

/// Byte ranges of the dictionary part of every object in a file with one classic
/// cross-reference table (krilla's output): from the object's offset to its `stream` keyword or
/// `endobj`. `None` when the table can't be read.
fn dictionary_ranges(pdf: &[u8]) -> Option<Vec<std::ops::Range<usize>>> {
    let tail = String::from_utf8_lossy(pdf.get(rfind(pdf, b"startxref")?..)?).to_string();
    let xref = int_after(&tail, "startxref")?;
    let table = pdf.get(xref..)?;
    if !table.starts_with(b"xref") {
        return None;
    }
    let end = find(table, b"trailer", 0)?;
    let text = String::from_utf8_lossy(table.get(4..end)?).to_string();
    let mut words = text.split_ascii_whitespace();
    let mut out = Vec::new();
    while let (Some(_first), Some(count)) = (words.next(), words.next()) {
        let count: usize = count.parse().ok()?;
        for _ in 0..count {
            let (off, _gen, kind) = (words.next()?, words.next()?, words.next()?);
            if kind != "n" {
                continue;
            }
            let start: usize = off.parse().ok()?;
            let endobj = find(pdf, b"endobj", start)?;
            let stop = find(pdf.get(..endobj)?, b"stream", start).unwrap_or(endobj);
            out.push(start..stop);
        }
    }
    Some(out)
}

/// PDF/X-4 checks on an exported file: what's missing or not allowed, device RGB under the CMYK
/// output intent included (empty = passes these checks).
pub fn check_pdfx4(pdf: &[u8]) -> Vec<String> {
    let mut issues = Vec::new();
    let text = String::from_utf8_lossy(pdf);
    if !pdf.starts_with(b"%PDF-1.") || pdf.get(7).is_none_or(|v| *v > b'6') {
        issues.push("the PDF version must be 1.6 or lower".into());
    }
    if !text.contains("/S/GTS_PDFX") {
        issues.push("no PDF/X output intent".into());
    }
    if !text.contains("/DestOutputProfile") {
        issues.push("the output intent has no destination profile".into());
    }
    if !text.contains("/GTS_PDFXVersion(PDF/X-4)") {
        issues.push("the document information doesn't name PDF/X-4".into());
    }
    if !text.contains("pdfxid:GTS_PDFXVersion>PDF/X-4<") {
        issues.push("the XMP metadata doesn't name PDF/X-4".into());
    }
    if text.contains("/Interpolate true") {
        issues.push("an image asks for interpolation".into());
    }
    if text.contains("/Encrypt") {
        issues.push("the file is encrypted".into());
    }
    let pages = text.matches("/Type/Page/").count() + text.matches("/Type/Page>").count();
    let trims = text.matches("/TrimBox").count();
    if pages > 0 && trims < pages {
        issues.push("every page needs a trim box".into());
    }
    if text.contains("/Subtype/Type3") {
        issues.push("Type 3 fonts aren't allowed".into());
    }
    issues.extend(device_rgb(pdf));
    issues
}

/// Device-dependent RGB under a CMYK output intent. PDF/X-4 (ISO 15930-7) lets RGB in only when
/// it is colour-managed: ICCBased RGB, or DeviceRGB remapped by a `DefaultRGB` colour space in the
/// resources. DeviceCMYK and DeviceGray are the output intent's own device values, so they stay.
/// This walks every page's content, the images, forms, patterns, shadings and soft masks it uses,
/// and the transparency groups, and reports each kind of device RGB it finds with the pages it is
/// on. Nothing is reported when the file has no CMYK PDF/X output intent.
fn device_rgb(pdf: &[u8]) -> Vec<String> {
    let Ok(file) = hayro_syntax::Pdf::new(pdf.to_vec()) else {
        return vec!["the file couldn't be read to check its colour spaces".into()];
    };
    if !cmyk_output_intent(&file) {
        return Vec::new();
    }
    let mut found: Vec<(String, Vec<usize>)> = Vec::new();
    for (i, page) in file.pages().iter().enumerate() {
        let mut scan = RgbScan::default();
        let res = page.resources();
        if let Some(group) = page.raw().get::<Dict<'_>>(b"Group") {
            scan.group(&group, res);
        }
        if let Some(content) = page.page_stream() {
            scan.content(content, res, 0);
        }
        for what in scan.found {
            match found.iter_mut().find(|(w, _)| *w == what) {
                Some((_, pages)) => pages.push(i + 1),
                None => found.push((what, vec![i + 1])),
            }
        }
    }
    found
        .into_iter()
        .map(|(what, pages)| {
            let list: Vec<String> = pages.iter().take(10).map(|p| p.to_string()).collect();
            let more = if pages.len() > 10 { ", …" } else { "" };
            let s = if pages.len() == 1 { "" } else { "s" };
            format!("{what} on page{s} {}{more}: under a CMYK output intent RGB needs an ICC profile", list.join(", "))
        })
        .collect()
}

/// Whether the file's PDF/X output intent has a four-channel (CMYK) destination profile.
fn cmyk_output_intent(file: &hayro_syntax::Pdf) -> bool {
    let xref = file.xref();
    let Some(catalog) = xref.get::<Dict<'_>>(xref.root_id()) else { return false };
    let Some(intents) = catalog.get::<Array<'_>>(b"OutputIntents") else { return false };
    intents.iter::<Dict<'_>>().any(|oi| {
        oi.get::<Name<'_>>(b"S").is_some_and(|s| s.as_ref() == b"GTS_PDFX")
            && oi.get::<Stream<'_>>(b"DestOutputProfile").and_then(|p| p.dict().get::<i32>(b"N")) == Some(4)
    })
}

/// Whether a colour space is device RGB: `/DeviceRGB` (`/RGB` in inline images), or an
/// `/Indexed` space over it.
fn is_device_rgb(cs: &Object<'_>) -> bool {
    match cs {
        Object::Name(n) => matches!(n.as_ref(), b"DeviceRGB" | b"RGB"),
        Object::Array(a) => {
            let mut items = a.iter::<Object<'_>>();
            match items.next() {
                Some(Object::Name(n)) if matches!(n.as_ref(), b"Indexed" | b"I") => {
                    // The base space is a name or a (non-indexed) array; no deeper nesting.
                    matches!(items.next(), Some(Object::Name(b)) if matches!(b.as_ref(), b"DeviceRGB" | b"RGB"))
                }
                _ => false,
            }
        }
        _ => false,
    }
}

/// How deep forms, patterns and soft masks may nest before the walk stops.
const MAX_DEPTH: usize = 16;

/// What one page uses in device RGB, in the order found.
#[derive(Default)]
struct RgbScan {
    found: Vec<String>,
    /// Forms, images and patterns already looked at on this page.
    seen: HashSet<ObjectIdentifier>,
}

impl RgbScan {
    fn add(&mut self, what: String) {
        if !self.found.contains(&what) {
            self.found.push(what);
        }
    }

    /// `DefaultRGB` in scope makes DeviceRGB colour-managed.
    fn managed(res: &Resources<'_>) -> bool {
        res.get_color_space(&Name::new_unescaped(b"DefaultRGB")).is_some()
    }

    /// A content stream (a page's or a form's) with its resources.
    fn content(&mut self, data: &[u8], res: &Resources<'_>, depth: usize) {
        if depth > MAX_DEPTH {
            return;
        }
        let managed = Self::managed(res);
        let mut ops = UntypedIter::new(data);
        while let Some(op) = ops.next() {
            let operator: &[u8] = op.operator;
            let last = op.operands().last().cloned();
            let name = || last.clone().and_then(Object::into_name);
            match operator {
                b"rg" | b"RG" if !managed => self.add("RGB fills or strokes".into()),
                b"cs" | b"CS" if !managed => {
                    let rgb = name().is_some_and(|n| n.as_ref() == b"DeviceRGB" || res.get_color_space(&n).is_some_and(|cs| is_device_rgb(&cs)));
                    if rgb {
                        self.add("RGB fills or strokes".into());
                    }
                }
                b"sh" => {
                    if let Some(sh) = name().and_then(|n| res.get_shading(&n)) {
                        self.shading(sh, managed);
                    }
                }
                b"scn" | b"SCN" => {
                    if let Some(p) = name().and_then(|n| res.get_pattern(&n)) {
                        self.pattern(p, res, managed, depth);
                    }
                }
                b"gs" => {
                    let mask = name().and_then(|n| res.get_ext_g_state(&n)).and_then(|gs| gs.get::<Dict<'_>>(b"SMask"));
                    if let Some(form) = mask.and_then(|m| m.get::<Stream<'_>>(b"G")) {
                        self.form(&form, res, depth);
                    }
                }
                b"Do" => {
                    if let Some(xo) = name().and_then(|n| res.get_x_object(&n)) {
                        match xo.dict().get::<Name<'_>>(b"Subtype").as_deref() {
                            Some(b"Image") => self.image(&xo, managed),
                            Some(b"Form") => self.form(&xo, res, depth),
                            _ => {}
                        }
                    }
                }
                b"BI" | b"EI" if !managed => {
                    if let Some(Object::Stream(img)) = last {
                        let cs = img.dict().get::<Object<'_>>(b"CS").or_else(|| img.dict().get::<Object<'_>>(b"ColorSpace"));
                        let cs = match cs {
                            Some(Object::Name(n))
                                if !matches!(n.as_ref(), b"DeviceRGB" | b"RGB" | b"G" | b"DeviceGray" | b"CMYK" | b"DeviceCMYK") =>
                            {
                                res.get_color_space(&n)
                            }
                            cs => cs,
                        };
                        if cs.is_some_and(|cs| is_device_rgb(&cs)) {
                            self.add("an RGB inline image".into());
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn image(&mut self, img: &Stream<'_>, managed: bool) {
        let id = img.obj_id();
        if managed || !self.seen.insert(id) || img.dict().get::<bool>(b"ImageMask") == Some(true) {
            return;
        }
        if img.dict().get::<Object<'_>>(b"ColorSpace").is_some_and(|cs| is_device_rgb(&cs)) {
            let (w, h) = (img.dict().get::<i32>(b"Width").unwrap_or(0), img.dict().get::<i32>(b"Height").unwrap_or(0));
            self.add(format!("an RGB image (object {}, {w} × {h} px)", id.obj_number));
        }
    }

    /// A form XObject (placed PDF page, group, soft mask): its group, then its content.
    fn form(&mut self, form: &Stream<'_>, parent: &Resources<'_>, depth: usize) {
        if depth >= MAX_DEPTH || !self.seen.insert(form.obj_id()) {
            return;
        }
        let res = Resources::from_parent(form.dict().get::<Dict<'_>>(b"Resources").unwrap_or_default(), parent.clone());
        if let Some(group) = form.dict().get::<Dict<'_>>(b"Group") {
            self.group(&group, &res);
        }
        if let Ok(data) = form.decoded() {
            self.content(&data, &res, depth + 1);
        }
    }

    fn group(&mut self, group: &Dict<'_>, res: &Resources<'_>) {
        if !Self::managed(res) && group.get::<Object<'_>>(b"CS").is_some_and(|cs| is_device_rgb(&cs)) {
            self.add("a transparency group blending in RGB".into());
        }
    }

    /// A shading dictionary or stream (gradients).
    fn shading(&mut self, sh: Object<'_>, managed: bool) {
        let dict = match sh {
            Object::Dict(d) => d,
            Object::Stream(s) => s.dict().clone(),
            _ => return,
        };
        if !managed && dict.get::<Object<'_>>(b"ColorSpace").is_some_and(|cs| is_device_rgb(&cs)) {
            self.add("RGB gradients".into());
        }
    }

    /// A shading pattern (its shading) or a tiling pattern (its cell, like a form).
    fn pattern(&mut self, p: Object<'_>, res: &Resources<'_>, managed: bool, depth: usize) {
        match p {
            Object::Dict(d) => {
                if let Some(sh) = d.get::<Object<'_>>(b"Shading") {
                    self.shading(sh, managed);
                }
            }
            Object::Stream(s) => self.form(&s, res, depth),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use designcraft_color::{Color, Swatch};
    use designcraft_compose::Cache;
    use designcraft_doc::build::NewDocument;
    use designcraft_doc::{Asset, AssetId, Content, Document, Fill, Fitting, Graphic, Item, ItemId, Shape, SpreadRef};
    use designcraft_geom::{Affine, Rect};
    use hayro_syntax::object::{Name, Object};
    use image::{ExtendedColorType, ImageEncoder};

    use crate::{ExportReport, PdfOptions, Standard, check_pdfx4, export_pdf_with_report};

    fn place(d: &mut Document, name: &str, data: Vec<u8>, size: (f64, f64), at: (f64, f64)) {
        let asset = AssetId(d.alloc());
        let mime = designcraft_images::mime(&data).into();
        let pixels = designcraft_images::pixel_size(&data);
        d.assets.insert(asset, Arc::new(Asset { id: asset, name: name.into(), mime, data: data.into(), pixels, ..Default::default() }));
        let lid = d.default_layer();
        let id = ItemId(d.alloc());
        let r = Rect::new(at.0, at.1, at.0 + 100.0, at.1 + 50.0);
        let mut it = Item::new(id, lid, Shape::Rectangle, designcraft_geom::shapes::rectangle(r));
        it.content = Content::Graphic(Graphic {
            asset,
            size,
            xf: Affine::translate(at) * Affine::scale(100.0 / size.0),
            auto_fit: Fitting::FillProportionally,
            fit_align: 4,
            crop: [0.0; 4],
        });
        d.insert_item(SpreadRef::Doc(0), it, None).unwrap();
    }

    fn add_box(d: &mut Document, r: Rect, swatch: &str) {
        let lid = d.default_layer();
        let mut b = Item::new(ItemId(d.alloc()), lid, Shape::Rectangle, designcraft_geom::shapes::rectangle(r));
        b.fill = Fill::swatch(swatch);
        d.insert_item(SpreadRef::Doc(0), b, None).unwrap();
    }

    /// A 4 × 2 PNG: red RGB or mid grey, optionally carrying an ICC profile.
    fn png(gray: bool, icc: Option<Vec<u8>>) -> Vec<u8> {
        let mut b = Vec::new();
        let mut enc = image::codecs::png::PngEncoder::new(&mut b);
        if let Some(p) = icc {
            enc.set_icc_profile(p).unwrap();
        }
        let (px, ty) = if gray { (vec![128u8; 8], ExtendedColorType::L8) } else { ([255u8, 0, 0].repeat(8), ExtendedColorType::Rgb8) };
        enc.write_image(&px, 4, 2, ty).unwrap();
        b
    }

    /// The header of an ICC v2.1 RGB display profile: what a PDF writer reads to tag an image
    /// `/ICCBased` (the check looks at the colour space, not at the profile's tables).
    fn rgb_icc() -> Vec<u8> {
        let mut p = vec![0u8; 132];
        p[..4].copy_from_slice(&132u32.to_be_bytes());
        p[8] = 2;
        p[9] = 0x10;
        p[12..16].copy_from_slice(b"mntr");
        p[16..20].copy_from_slice(b"RGB ");
        p[20..24].copy_from_slice(b"XYZ ");
        p[36..40].copy_from_slice(b"acsp");
        p
    }

    /// A 4 × 2 CMYK image (100% K) as krilla writes it.
    #[derive(Clone, Hash)]
    struct CmykImage(Vec<u8>);

    impl krilla::image::CustomImage for CmykImage {
        fn color_channel(&self) -> &[u8] {
            &self.0
        }
        fn alpha_channel(&self) -> Option<&[u8]> {
            None
        }
        fn bits_per_component(&self) -> krilla::image::BitsPerComponent {
            krilla::image::BitsPerComponent::Eight
        }
        fn size(&self) -> (u32, u32) {
            (4, 2)
        }
        fn icc_profile(&self) -> Option<&[u8]> {
            None
        }
        fn color_space(&self) -> krilla::image::ImageColorspace {
            krilla::image::ImageColorspace::Cmyk
        }
    }

    /// A one-page PDF 1.6 holding a CMYK image, as another app would hand over a placed graphic.
    fn pdf_with_cmyk_image() -> Vec<u8> {
        pdf_with_image(krilla::image::Image::from_custom(CmykImage([0u8, 0, 0, 255].repeat(8)), false).unwrap())
    }

    /// A one-page PDF 1.6 holding `img`.
    fn pdf_with_image(img: krilla::image::Image) -> Vec<u8> {
        let configuration = krilla::configure::ConfigurationBuilder::new().with_version(krilla::configure::PdfVersion::Pdf16).finish().unwrap();
        let mut pdf = krilla::Document::new_with(krilla::SerializeSettings { configuration, ..Default::default() });
        let size = krilla::geom::Size::from_wh(100.0, 50.0).unwrap();
        let mut page = pdf.start_page_with(krilla::page::PageSettings::new(size));
        let mut s = page.surface();
        s.draw_image(img, size);
        s.finish();
        page.finish();
        pdf.finish().unwrap()
    }

    fn x4(d: &Document) -> ExportReport {
        export_pdf_with_report(d, &Cache::new(), &PdfOptions { standard: Standard::PdfX4, ..Default::default() }).unwrap()
    }

    /// The `/ColorSpace` of every image XObject: a name, or `[` for an array (ICCBased, Indexed…).
    fn image_spaces(bytes: &[u8]) -> Vec<String> {
        let pdf = hayro_syntax::Pdf::new(bytes.to_vec()).unwrap();
        let mut out: Vec<String> = pdf
            .objects()
            .into_iter()
            .filter_map(|o| o.into_stream())
            .filter(|s| s.dict().get::<Name>(b"Subtype").is_some_and(|n| n.as_str() == "Image") && s.dict().get::<bool>(b"ImageMask").is_none())
            .filter_map(|s| match s.dict().get::<Object>(b"ColorSpace")? {
                Object::Name(n) => Some(n.as_str().to_string()),
                Object::Array(a) => a.iter::<Name>().next().map(|n| format!("[{}", n.as_str())),
                _ => None,
            })
            .collect();
        out.sort();
        out
    }

    /// A DeviceRGB image under the CMYK output intent is reported. A placed PDF carries its
    /// images into the export unchanged.
    #[test]
    fn device_rgb_image_is_flagged() {
        let mut d = Document::new(&NewDocument::default());
        let photo = krilla::image::Image::from_png(png(false, None).into(), false).unwrap();
        place(&mut d, "photo.pdf", pdf_with_image(photo), (100.0, 50.0), (72.0, 72.0));
        let r = x4(&d);
        assert_eq!(image_spaces(&r.bytes), ["DeviceRGB"]);
        let rgb: Vec<&String> = r.warnings.iter().filter(|w| w.contains("RGB")).collect();
        assert_eq!(rgb.len(), 1, "{:?}", r.warnings);
        assert!(rgb[0].starts_with("PDF/X-4: an RGB image (object ") && rgb[0].contains("4 × 2 px) on page 1:"), "{}", rgb[0]);
        // Without a PDF/X output intent there is nothing to check RGB against.
        let plain = export_pdf_with_report(&d, &Cache::new(), &PdfOptions::default()).unwrap();
        assert!(!check_pdfx4(&plain.bytes).iter().any(|i| i.contains("RGB")));
    }

    /// CMYK and grey are the output intent's own values and ICC-tagged RGB is colour-managed:
    /// none of them is flagged, whether placed directly or inside a placed PDF.
    #[test]
    fn cmyk_grey_and_icc_rgb_pass() {
        let mut d = Document::new(&NewDocument::default());
        add_box(&mut d, Rect::new(36.0, 400.0, 136.0, 500.0), "C=100 M=0 Y=0 K=0");
        place(&mut d, "grey.png", png(true, None), (4.0, 2.0), (72.0, 72.0));
        place(&mut d, "tagged.png", png(false, Some(rgb_icc())), (4.0, 2.0), (72.0, 172.0));
        place(&mut d, "logo.pdf", pdf_with_cmyk_image(), (100.0, 50.0), (72.0, 272.0));
        let r = x4(&d);
        assert_eq!(image_spaces(&r.bytes), ["DeviceCMYK", "DeviceGray", "[ICCBased"]);
        assert!(r.warnings.is_empty(), "{:?}", r.warnings);
    }

    /// RGB swatches paint in DeviceRGB.
    #[test]
    fn device_rgb_fills_are_flagged() {
        let mut d = Document::new(&NewDocument { pages: 2, ..NewDocument::default() });
        d.swatches.push(Swatch::color("Brand Red", Color::rgb(0.9, 0.1, 0.1)));
        add_box(&mut d, Rect::new(36.0, 400.0, 136.0, 500.0), "Brand Red");
        let r = x4(&d);
        assert_eq!(r.warnings, ["PDF/X-4: RGB fills or strokes on page 1: under a CMYK output intent RGB needs an ICC profile"]);
    }

    /// A minimal PDF with a CMYK (N 4) PDF/X output intent and one page with `entries` (its
    /// resources, a group…) and `content`.
    fn tiny_pdf(entries: &str, content: &str) -> Vec<u8> {
        let objects = [
            "<</Type/Catalog/Pages 2 0 R/OutputIntents[<</Type/OutputIntent/S/GTS_PDFX/DestOutputProfile 4 0 R>>]>>".to_string(),
            "<</Type/Pages/Kids[3 0 R]/Count 1>>".to_string(),
            format!("<</Type/Page/Parent 2 0 R/MediaBox[0 0 100 100]{entries}/Contents 5 0 R>>"),
            "<</N 4/Length 0>>\nstream\n\nendstream".to_string(),
            format!("<</Length {}>>\nstream\n{content}\nendstream", content.len()),
            "<</N 3/Length 0>>\nstream\n\nendstream".to_string(),
        ];
        let mut out = b"%PDF-1.6\n".to_vec();
        let mut offsets = Vec::new();
        for (i, o) in objects.iter().enumerate() {
            offsets.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
        }
        let xref = out.len();
        out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f\r\n", objects.len() + 1).as_bytes());
        for off in offsets {
            out.extend_from_slice(format!("{off:010} 00000 n\r\n").as_bytes());
        }
        out.extend_from_slice(format!("trailer\n<</Size {}/Root 1 0 R>>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1).as_bytes());
        out
    }

    /// DeviceRGB is allowed when a `DefaultRGB` colour space makes it colour-managed.
    #[test]
    fn default_rgb_makes_device_rgb_managed() {
        let rgb = |i: &String| i.contains("RGB");
        let content = "1 0 0 rg 0 0 10 10 re f /DeviceRGB CS 0 0 1 SC 0 0 m 10 10 l S";
        let bare = check_pdfx4(&tiny_pdf("/Resources<<>>", content));
        assert_eq!(bare.iter().filter(|i| rgb(i)).count(), 1, "{bare:?}");
        let managed = check_pdfx4(&tiny_pdf("/Resources<</ColorSpace<</DefaultRGB[/ICCBased 6 0 R]>>>>", content));
        assert!(!managed.iter().any(rgb), "{managed:?}");
        // CMYK and grey need no default space.
        let cmyk = check_pdfx4(&tiny_pdf("/Resources<<>>", "0 0 0 1 k 0 0 10 10 re f 0.5 G 0 0 m 10 10 l S"));
        assert!(!cmyk.iter().any(rgb), "{cmyk:?}");
    }

    /// An RGB blending space on the page group and an RGB inline image are device RGB too.
    #[test]
    fn rgb_groups_and_inline_images_are_flagged() {
        let group = check_pdfx4(&tiny_pdf("/Resources<<>>/Group<</S/Transparency/CS/DeviceRGB>>", "0 0 0 1 k"));
        assert!(
            group.iter().any(|i| i == "a transparency group blending in RGB on page 1: under a CMYK output intent RGB needs an ICC profile"),
            "{group:?}"
        );
        let inline = check_pdfx4(&tiny_pdf("/Resources<<>>", "q BI /W 1 /H 1 /CS /RGB /BPC 8 /F /AHx ID ff0000> EI Q"));
        assert!(inline.iter().any(|i| i.starts_with("an RGB inline image on page 1")), "{inline:?}");
        let grey = check_pdfx4(&tiny_pdf("/Resources<<>>", "q BI /W 1 /H 1 /CS /G /BPC 8 /F /AHx ID 80> EI Q"));
        assert!(!grey.iter().any(|i| i.contains("RGB")), "{grey:?}");
    }

    /// A 4 × 2 RGB image in every raster path the exporter has, none with a profile. Each has
    /// its own colour, so no two share an image XObject.
    fn rgb_rasters() -> Vec<(&'static str, Vec<u8>)> {
        let px = |c: [u8; 3]| c.repeat(8);
        let encode = |f: &dyn Fn(&mut Vec<u8>)| {
            let mut b = Vec::new();
            f(&mut b);
            b
        };
        let jpeg = encode(&|b| image::codecs::jpeg::JpegEncoder::new(b).write_image(&px([0, 0, 255]), 4, 2, ExtendedColorType::Rgb8).unwrap());
        let gif = encode(&|b| image::codecs::gif::GifEncoder::new(b).encode(&[0u8, 255, 0, 255].repeat(8), 4, 2, ExtendedColorType::Rgba8).unwrap());
        let webp =
            encode(&|b| image::codecs::webp::WebPEncoder::new_lossless(b).write_image(&px([255, 255, 0]), 4, 2, ExtendedColorType::Rgb8).unwrap());
        let tiff = encode(&|b| {
            image::codecs::tiff::TiffEncoder::new(std::io::Cursor::new(b)).write_image(&px([0, 255, 255]), 4, 2, ExtendedColorType::Rgb8).unwrap()
        });
        vec![("photo.png", png(false, None)), ("photo.jpg", jpeg), ("photo.gif", gif), ("photo.webp", webp), ("photo.tif", tiff)]
    }

    /// The decoded `/ICCBased` profile of every image XObject that has one.
    fn image_profiles(bytes: &[u8]) -> Vec<Vec<u8>> {
        let pdf = hayro_syntax::Pdf::new(bytes.to_vec()).unwrap();
        pdf.objects()
            .into_iter()
            .filter_map(|o| o.into_stream())
            .filter(|s| s.dict().get::<Name>(b"Subtype").is_some_and(|n| n.as_str() == "Image"))
            .filter_map(|s| match s.dict().get::<hayro_syntax::object::Array>(b"ColorSpace")?.iter::<Object>().nth(1)? {
                Object::Stream(p) => Some(p.decoded().ok()?.into_owned()),
                _ => None,
            })
            .collect()
    }

    /// PDF/X-4 wrote placed RGB images as DeviceRGB under the CMYK output intent, and only warned.
    /// Each raster path now tags them with sRGB, re-encoded as JPEG or not. A plain export keeps
    /// DeviceRGB.
    #[test]
    fn rgb_images_carry_srgb_in_pdfx4() {
        let mut d = Document::new(&NewDocument::default());
        for (i, (name, data)) in rgb_rasters().into_iter().enumerate() {
            place(&mut d, name, data, (4.0, 2.0), (72.0, 72.0 + 60.0 * i as f64));
        }
        for compress_images in [false, true] {
            let opts = PdfOptions { standard: Standard::PdfX4, compress_images, ..Default::default() };
            let r = export_pdf_with_report(&d, &Cache::new(), &opts).unwrap();
            assert!(r.warnings.is_empty(), "compress_images {compress_images}: {:?}", r.warnings);
            assert_eq!(image_spaces(&r.bytes), ["[ICCBased"; 5], "compress_images {compress_images}");
            let profiles = image_profiles(&r.bytes);
            assert_eq!(profiles.len(), 5);
            assert!(profiles.iter().all(|p| p == crate::export::SRGB_ICC), "every image carries the sRGB profile");
        }
        let plain = export_pdf_with_report(&d, &Cache::new(), &PdfOptions::default()).unwrap();
        assert_eq!(image_spaces(&plain.bytes), ["DeviceRGB"; 5]);
    }

    /// An image's own profile and a grey image are left as they are in PDF/X-4.
    #[test]
    fn own_profiles_and_grey_stay() {
        let mut d = Document::new(&NewDocument::default());
        place(&mut d, "grey.png", png(true, None), (4.0, 2.0), (72.0, 72.0));
        place(&mut d, "tagged.png", png(false, Some(rgb_icc())), (4.0, 2.0), (72.0, 172.0));
        let r = x4(&d);
        assert_eq!(image_spaces(&r.bytes), ["DeviceGray", "[ICCBased"]);
        assert_eq!(image_profiles(&r.bytes), [rgb_icc()]);
    }

    /// Every image constructor asked for interpolation, which PDF/X-4 and PDF/A forbid: PDF/A
    /// exports with a placed image failed (#71). Plain exports keep it.
    #[test]
    fn no_interpolation_in_pdfx4_or_pdfa() {
        let mut d = Document::new(&NewDocument::default());
        for (i, (name, data)) in rgb_rasters().into_iter().enumerate() {
            place(&mut d, name, data, (4.0, 2.0), (72.0, 72.0 + 60.0 * i as f64));
        }
        let interpolated = |b: &[u8]| String::from_utf8_lossy(b).matches("/Interpolate true").count();
        for compress_images in [false, true] {
            for standard in [Standard::PdfX4, Standard::PdfA2b] {
                let opts = PdfOptions { standard, compress_images, ..Default::default() };
                let r = export_pdf_with_report(&d, &Cache::new(), &opts).unwrap_or_else(|e| panic!("{standard:?}: {e:?}"));
                assert_eq!(interpolated(&r.bytes), 0, "{standard:?}, compress_images {compress_images}");
            }
            let plain = export_pdf_with_report(&d, &Cache::new(), &PdfOptions { compress_images, ..Default::default() }).unwrap();
            // The TIFF goes through the decoded fallback, which never asked for interpolation,
            // unless it is re-encoded as JPEG.
            assert_eq!(interpolated(&plain.bytes), if compress_images { 5 } else { 4 }, "compress_images {compress_images}");
            assert!(check_pdfx4(&plain.bytes).iter().any(|i| i == "an image asks for interpolation"));
        }
        let tif = cmyk_tiff();
        let mut c = Document::new(&NewDocument::default());
        place(&mut c, "scan.tif", tif, (4.0, 2.0), (72.0, 72.0));
        assert_eq!(interpolated(&x4(&c).bytes), 0, "CMYK TIFF");
    }

    /// A 4 × 2 CMYK TIFF, 100% K.
    fn cmyk_tiff() -> Vec<u8> {
        let mut b = Vec::new();
        tiff::encoder::TiffEncoder::new(std::io::Cursor::new(&mut b))
            .unwrap()
            .write_image::<tiff::encoder::colortype::CMYK8>(4, 2, &[0u8, 0, 0, 255].repeat(8))
            .unwrap();
        b
    }

    /// The XMP states the trapping state as the document information does.
    #[test]
    fn xmp_states_trapped() {
        let r = x4(&Document::new(&NewDocument::default()));
        let pdf = hayro_syntax::Pdf::new(r.bytes.clone()).unwrap();
        let xref = pdf.xref();
        let catalog = xref.get::<hayro_syntax::object::Dict>(xref.root_id()).unwrap();
        let xmp = catalog.get::<hayro_syntax::object::Stream>(b"Metadata").unwrap().decoded().unwrap().into_owned();
        let xmp = String::from_utf8_lossy(&xmp);
        assert!(xmp.contains("xmlns:pdf=\"http://ns.adobe.com/pdf/1.3/\"><pdf:Trapped>False</pdf:Trapped>"), "{xmp}");
        assert!(String::from_utf8_lossy(&r.bytes).contains("/Trapped/False"));
        assert!(r.warnings.is_empty(), "{:?}", r.warnings);
    }
}
