//! CMYK print output: the serialization settings (the working CMYK space's profile as the output
//! intent, CMYK blending), the XMP dates PDF/X-4 requires, written as an incremental update of
//! krilla's PDF 1.6 output, and a check of the PDF/X-4 requirements this exporter is responsible
//! for.

use std::collections::HashSet;

use hayro_syntax::content::UntypedIter;
use hayro_syntax::object::{Array, Dict, Name, Object, ObjectIdentifier, Stream};
use hayro_syntax::page::Resources;

use craft_pdf::write::WriteError;
use craft_pdf::write::backend as krilla;
use craft_pdf::write::print::{self, PrintColor};

use crate::transitions::{find, int_after, object, rfind};

mod fonts;

/// The serialization settings for CMYK print output: DeviceCMYK and CMYK blending groups in the
/// document's CMYK working space (Coated FOGRA39 when it has none or its profile can't be
/// written). With `intent` (PDF/X), the working space is also the `GTS_PDFX` output intent, with
/// `/Trapped /False`. The second value is a warning when the working space's profile was replaced.
pub(crate) fn print_settings(intent: bool) -> crate::Result<(krilla::SerializeSettings, Option<String>)> {
    print_settings_with(&designcraft_color::cms::active(), intent)
}

/// [`print_settings`] for the working spaces of `cms`.
pub(crate) fn print_settings_with(cms: &designcraft_color::cms::Cms, intent: bool) -> crate::Result<(krilla::SerializeSettings, Option<String>)> {
    let write = |color: PrintColor<'_>| if intent { print::settings(color) } else { print::cmyk_blending(color.profile) };
    let fogra39 = || write(PrintColor::fogra39(craft_cms::builtin::COATED_FOGRA39_ICC));
    let Ok(space) = cms.output_intent() else { return Ok((fogra39()?, None)) };
    let registered = space.identifier != "Custom";
    let color = PrintColor {
        profile: &space.icc,
        condition_identifier: space.identifier,
        condition: Some(&space.condition),
        registry_name: registered.then_some("http://www.color.org"),
        info: Some(&space.condition),
    };
    match write(color) {
        Ok(settings) => Ok((settings, None)),
        Err(WriteError::InvalidProfile) => {
            Ok((fogra39()?, Some(format!("the CMYK working space `{}` couldn't be written to the PDF; Coated FOGRA39 was used", space.condition))))
        }
        Err(e) => Err(e.into()),
    }
}

/// Finish krilla's PDF/X-4 output with the XMP entries PDF/X-4 requires and krilla doesn't write:
/// `xmp:MetadataDate` and `xmpMM:VersionID`, as an incremental update. `date` (Unix seconds, UTC)
/// is the metadata date; `None` (no clock, as on wasm) writes the epoch. `None` back when the
/// file isn't the simple kind this understands (a classic cross-reference table, uncompressed
/// XMP).
pub fn make_pdfx4(pdf: &[u8], date: Option<i64>) -> Option<Vec<u8>> {
    let t_at = rfind(pdf, b"trailer")?;
    let trailer = String::from_utf8_lossy(pdf.get(t_at..)?).to_string();
    let size = int_after(&trailer, "/Size")?;
    let root = int_after(&trailer, "/Root")?;
    let prev = int_after(&trailer, "startxref")?;
    let info = int_after(&trailer, "/Info").map(|i| format!("/Info {i} 0 R")).unwrap_or_default();
    let id = trailer.find("/ID").and_then(|i| trailer.get(i..)?.find(']').and_then(|j| trailer.get(i..i + j + 1)).map(str::to_string));
    let cat = String::from_utf8_lossy(pdf.get(object(pdf, root)?)?).to_string();
    let meta = int_after(&cat, "/Metadata")?;
    let body = pdf.get(object(pdf, meta)?)?;
    let s0 = find(body, b"stream", 0)? + b"stream".len();
    let s0 = s0 + body.get(s0..)?.iter().take_while(|c| **c == b'\r' || **c == b'\n').count();
    let s1 = rfind(body, b"endstream")?;
    let xml = String::from_utf8_lossy(body.get(s0..s1)?).trim_end().to_string();
    let dict = String::from_utf8_lossy(body.get(..find(body, b"stream", 0)?)?).to_string();
    if dict.contains("/Filter") || !xml.contains("</rdf:RDF>") {
        return None;
    }
    let (y, mo, d, h, mi, se) = crate::export::civil(date.unwrap_or(0));
    let stamp = format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{se:02}Z");
    let dates = format!(
        "<rdf:Description rdf:about=\"\" xmlns:xmp=\"http://ns.adobe.com/xap/1.0/\" xmlns:xmpMM=\"http://ns.adobe.com/xap/1.0/mm/\">\
         <xmp:MetadataDate>{stamp}</xmp:MetadataDate><xmpMM:VersionID>1</xmpMM:VersionID></rdf:Description>"
    );
    let xml = xml.replacen("</rdf:RDF>", &format!("{dates}</rdf:RDF>"), 1);
    let dict = {
        let i = dict.find("/Length")?;
        let rest = dict.get(i + 7..)?.trim_start();
        let end = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
        format!("{}/Length {}{}", dict.get(..i)?, xml.len(), rest.get(end..)?)
    };
    let mut out = pdf.to_vec();
    if !out.ends_with(b"\n") {
        out.push(b'\n');
    }
    let at = out.len();
    out.extend_from_slice(format!("{meta} 0 obj\n").as_bytes());
    out.extend_from_slice(dict.trim().as_bytes());
    out.extend_from_slice(b"\nstream\n");
    out.extend_from_slice(xml.as_bytes());
    out.extend_from_slice(b"\nendstream\nendobj\n");
    let xref_at = out.len();
    let id = id.unwrap_or_default();
    out.extend_from_slice(
        format!("xref\n{meta} 1\n{at:010} 00000 n\r\ntrailer\n<</Size {size}/Root {root} 0 R{info}{id}/Prev {prev}>>\nstartxref\n{xref_at}\n%%EOF\n")
            .as_bytes(),
    );
    Some(out)
}

/// Whether a transparency group dictionary still names DeviceRGB as its blending space.
pub fn has_rgb_groups(pdf: &[u8]) -> bool {
    pdf.windows(b"/CS/DeviceRGB".len()).enumerate().any(|(i, w)| {
        w == b"/CS/DeviceRGB" && pdf.get(i.saturating_sub(64)..i).is_some_and(|before| before.windows(15).any(|b| b == b"/S/Transparency"))
    })
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
    if !text.contains("<xmp:MetadataDate>") {
        issues.push("the XMP metadata has no xmp:MetadataDate".into());
    }
    if !text.contains("<xmpMM:VersionID>") {
        issues.push("the XMP metadata has no xmpMM:VersionID".into());
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
    issues.extend(fonts::unembedded_fonts(pdf));
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
            let list: Vec<String> = pages.iter().take(10).map(ToString::to_string).collect();
            let more = if pages.len() > 10 { ", …" } else { "" };
            let s = if pages.len() == 1 { "" } else { "s" };
            format!("{what} on page{s} {}{more}: under a CMYK output intent RGB needs an ICC profile", list.join(", "))
        })
        .collect()
}

/// Whether the file's PDF/X output intent has a four-channel (CMYK) destination profile.
pub(crate) fn cmyk_output_intent(file: &hayro_syntax::Pdf) -> bool {
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
pub(crate) const MAX_DEPTH: usize = 16;

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
                        self.inline_image(&img, res);
                    }
                }
                _ => {}
            }
        }
    }

    /// An inline image (`BI … EI`) in an RGB space, named directly or through the resources.
    fn inline_image(&mut self, img: &Stream<'_>, res: &Resources<'_>) {
        let cs = img.dict().get::<Object<'_>>(b"CS").or_else(|| img.dict().get::<Object<'_>>(b"ColorSpace"));
        let cs = match cs {
            Some(Object::Name(n)) if !matches!(n.as_ref(), b"DeviceRGB" | b"RGB" | b"G" | b"DeviceGray" | b"CMYK" | b"DeviceCMYK") => {
                res.get_color_space(&n)
            }
            cs => cs,
        };
        if cs.is_some_and(|cs| is_device_rgb(&cs)) {
            self.add("an RGB inline image".into());
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
mod tests;
