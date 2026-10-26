//! PDF/X-1a:2003: the PDF/X-4 print file flattened by craft-prepress (no transparency left; RGB
//! and ICC color converted into the output intent's CMYK; inks, overprint and live text kept),
//! written by krilla as PDF 1.4, given the PDF/X-1a identification and output intent, then
//! checked.

use std::collections::{HashMap, HashSet};

use craft_pdf::write::backend::configure::PdfVersion;
use craft_prepress::flatten::pdf::ReadOptions;
use craft_prepress::flatten::{FlattenDocumentOptions, FlattenOptions, FlattenTotals, flatten_document};
use hayro_syntax::content::UntypedIter;
use hayro_syntax::object::{Array, Dict, Name, Object, ObjectIdentifier, Stream};
use hayro_syntax::page::Resources;

use crate::PdfError;
use crate::pdfx::MAX_DEPTH;
use crate::transitions::{find, int_after, rfind};

/// `GTS_PDFXVersion` of PDF/X-1a:2003 (ISO 15930-4): the base level, PDF/X-1:2003.
const VERSION: &str = "PDF/X-1:2003";

/// `GTS_PDFXConformance` of PDF/X-1a:2003 (ISO 15930-4): with [`VERSION`], it names the
/// conformance level.
const CONFORMANCE: &str = "PDF/X-1a:2003";

/// `/RegistryName` of the ICC's registry of characterized printing conditions. An output intent
/// naming one of them needs no `/DestOutputProfile` in PDF/X.
const ICC_REGISTRY: &str = "http://www.color.org";

/// The graphics placed on one output page, to name in errors: the page's name and the graphics'
/// names.
#[derive(Clone, Debug, Default)]
pub(crate) struct PageGraphics {
    pub label: String,
    pub graphics: Vec<String>,
}

/// The PDF/X-1a file for `pdf`, the PDF/X-4 print file: flattened at `dpi`, written at PDF 1.4,
/// identified, checked. `pages` names each output page and its graphics for errors. Fails when
/// the flattener fails or leaves an image out, when the output intent's profile can't go into a
/// PDF 1.4 file, or when the result still has transparency or color outside DeviceCMYK,
/// DeviceGray, Separation and DeviceN; other check findings become warnings.
pub(crate) fn flatten_x1a(pdf: &[u8], dpi: f32, pages: &[PageGraphics], warnings: &mut Vec<String>) -> crate::Result<Vec<u8>> {
    let dpi = if dpi.is_finite() { dpi.clamp(36.0, 2400.0) } else { 300.0 };
    let opts = FlattenDocumentOptions {
        read: ReadOptions { raster_dpi: dpi, ..ReadOptions::default() },
        flatten: FlattenOptions { raster_dpi: dpi, ..FlattenOptions::default() },
        producer: "DesignCraft".into(),
        version: PdfVersion::Pdf14,
        pdfx_version: Some(VERSION.into()),
        ..FlattenDocumentOptions::default()
    };
    let (bytes, report) = flatten_document(pdf, &opts).map_err(|e| PdfError::Write(format!("PDF/X-1a: the transparency flattener failed: {e}")))?;
    let unread: Vec<String> = report
        .pages
        .iter()
        .filter(|(_, r)| r.read.unreadable_images > 0)
        .map(|(page, r)| {
            let n = r.read.unreadable_images;
            let images = if n == 1 { "1 image".to_string() } else { format!("{n} images") };
            let fallback = PageGraphics { label: page.saturating_add(1).to_string(), graphics: Vec::new() };
            let at = pages.get(*page).unwrap_or(&fallback);
            let names: Vec<String> = at.graphics.iter().take(5).map(|g| format!("`{g}`")).collect();
            let more = if at.graphics.len() > 5 { ", …" } else { "" };
            let placed = if names.is_empty() { String::new() } else { format!(" (graphics on it: {}{more})", names.join(", ")) };
            format!("{images} on page {}{placed}", at.label)
        })
        .collect();
    if !unread.is_empty() {
        return Err(PdfError::Write(format!(
            "PDF/X-1a: the transparency flattener couldn't read {}, and a PDF/X file can't leave images out: replace the graphic, or export without PDF/X-1a",
            unread.join("; ")
        )));
    }
    warnings.extend(flatten_warnings(&report.totals, dpi));
    let bytes = finish(&bytes).map_err(|e| PdfError::Write(format!("PDF/X-1a: {e}")))?;
    let (fatal, other) = checks(&bytes);
    if !fatal.is_empty() {
        return Err(PdfError::Write(format!("PDF/X-1a: after flattening, {}", fatal.join("; "))));
    }
    warnings.extend(other.into_iter().map(|issue| format!("PDF/X-1a: {issue}")));
    Ok(bytes)
}

/// What the flattener's `t` (at `dpi`) tells the user: rasterized transparency, and spot inks past
/// the ink set's capacity, whose tints the flattener drops.
pub(crate) fn flatten_warnings(t: &FlattenTotals, dpi: f32) -> Vec<String> {
    let mut out = Vec::new();
    if t.complex_regions > 0 || t.complex_groups > 0 {
        let at = t.lowest_dpi.map(|d| format!(", some below {dpi:.0} ppi (lowest {d:.0} ppi)")).unwrap_or_default();
        out.push(format!("PDF/X-1a: transparency flattened: {} region(s) rasterized at {dpi:.0} ppi{at}", t.complex_regions + t.complex_groups));
    }
    if t.dropped_inks > 0 {
        out.push(format!(
            "PDF/X-1a: the document has more spot inks than the flattener can separate: {} tint(s) in the inks past its limit were dropped and won't print",
            t.dropped_inks
        ));
    }
    out
}

/// PDF/X-1a:2003 checks on an exported file (empty = passes them): PDF 1.4 or lower, the
/// identification (`GTS_PDFXVersion` and `GTS_PDFXConformance`), a CMYK `GTS_PDFX` output intent
/// whose profile, when it has one, is ICC v2 (and without one, names a registered printing
/// condition), `/Trapped` True or False, trim boxes, no encryption or interpolation; no
/// transparency (soft masks, transparency groups, constant opacity below 1, blend modes other
/// than Normal); only DeviceCMYK, DeviceGray, Separation and DeviceN (and Indexed and Pattern
/// spaces over them), a spot's alternate space process; every font embedded.
pub fn check_pdfx1a(pdf: &[u8]) -> Vec<String> {
    let (mut issues, other) = checks(pdf);
    issues.extend(other);
    issues
}

/// [`check_pdfx1a`]'s findings: transparency and color spaces first, then the rest.
fn checks(pdf: &[u8]) -> (Vec<String>, Vec<String>) {
    let mut issues = Vec::new();
    let text = String::from_utf8_lossy(pdf);
    if !pdf.starts_with(b"%PDF-1.") || pdf.get(7).is_none_or(|v| *v > b'4') {
        issues.push("the PDF version must be 1.4 or lower".into());
    }
    if !text.contains("/S/GTS_PDFX") {
        issues.push("no PDF/X output intent".into());
    }
    if !text.contains(&format!("/GTS_PDFXVersion({VERSION})")) {
        issues.push(format!("the document information doesn't name {VERSION}"));
    }
    if !text.contains(&format!("/GTS_PDFXConformance({CONFORMANCE})")) {
        issues.push(format!("the document information doesn't name the conformance level {CONFORMANCE}"));
    }
    if !text.contains("/Trapped/False") && !text.contains("/Trapped/True") && !text.contains("/Trapped /False") && !text.contains("/Trapped /True") {
        issues.push("the document information needs /Trapped True or False".into());
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
    let (content, fonts) = content(pdf);
    issues.extend(fonts);
    (content, issues)
}

/// Transparency and color spaces, then the output intent and fonts, walked through every page's
/// content and what it uses.
fn content(pdf: &[u8]) -> (Vec<String>, Vec<String>) {
    let Ok(file) = hayro_syntax::Pdf::new(pdf.to_vec()) else {
        return (vec!["the file couldn't be read to check its transparency and color spaces".into()], Vec::new());
    };
    let mut fonts = intent_issues(&file);
    let mut found: Vec<(String, Vec<usize>)> = Vec::new();
    let mut font_found: Vec<(String, Vec<usize>)> = Vec::new();
    for (i, page) in file.pages().iter().enumerate() {
        let mut scan = Scan::default();
        let res = page.resources();
        if let Some(group) = page.raw().get::<Dict<'_>>(b"Group") {
            scan.group(&group);
        }
        if let Some(content) = page.page_stream() {
            scan.content(content, res, 0);
        }
        for (list, items) in [(&mut found, scan.found), (&mut font_found, scan.font_issues)] {
            for what in items {
                match list.iter_mut().find(|(w, _)| *w == what) {
                    Some((_, pages)) => pages.push(i + 1),
                    None => list.push((what, vec![i + 1])),
                }
            }
        }
    }
    let on_pages = |(what, pages): (String, Vec<usize>)| {
        let list: Vec<String> = pages.iter().take(10).map(|p| p.to_string()).collect();
        let more = if pages.len() > 10 { ", …" } else { "" };
        let s = if pages.len() == 1 { "" } else { "s" };
        format!("{what} on page{s} {}{more}", list.join(", "))
    };
    fonts.extend(font_found.into_iter().map(on_pages));
    (found.into_iter().map(on_pages).collect(), fonts)
}

/// A `GTS_PDFX` output intent, as PDF/X-1a judges it.
struct Intent {
    /// Its object number, when it is an indirect object.
    obj: Option<usize>,
    /// `/OutputCondition`, else `/OutputConditionIdentifier`, for messages.
    name: String,
    /// An `/OutputConditionIdentifier` other than `Custom` in the ICC registry.
    registered: bool,
    /// Whether it has a `/DestOutputProfile`, and the profile's object number.
    has_profile: bool,
    profile_obj: Option<usize>,
    /// The profile's `/N` and decoded bytes.
    channels: Option<i32>,
    profile: Option<Vec<u8>>,
}

impl Intent {
    /// The profile's ICC major version (byte 8 of its header).
    fn major(&self) -> Option<u8> {
        self.profile.as_deref()?.get(8).copied()
    }

    /// "ICC v4", or what keeps the version from being read.
    fn version(&self) -> String {
        self.major().map_or_else(|| "unreadable".to_string(), |v| format!("ICC v{v}"))
    }
}

/// The catalog's `GTS_PDFX` output intents.
fn pdfx_intents(file: &hayro_syntax::Pdf) -> Vec<Intent> {
    let xref = file.xref();
    let Some(catalog) = xref.get::<Dict<'_>>(xref.root_id()) else { return Vec::new() };
    let Some(intents) = catalog.get::<Array<'_>>(b"OutputIntents") else { return Vec::new() };
    let number = |n: i32| usize::try_from(n).ok();
    intents
        .iter::<Dict<'_>>()
        .filter(|oi| oi.get::<Name<'_>>(b"S").is_some_and(|s| s.as_ref() == b"GTS_PDFX"))
        .map(|oi| {
            let text = |key: &[u8]| {
                oi.get::<hayro_syntax::object::String<'_>>(key)
                    .map(|s| String::from_utf8_lossy(s.as_bytes()).trim().to_string())
                    .filter(|s| !s.is_empty())
            };
            let identifier = text(b"OutputConditionIdentifier");
            let registry = text(b"RegistryName");
            let profile = oi.get::<Stream<'_>>(b"DestOutputProfile");
            Intent {
                obj: oi.obj_id().and_then(|id| number(id.obj_number)),
                name: text(b"OutputCondition").or_else(|| identifier.clone()).unwrap_or_else(|| "?".into()),
                registered: registry.is_some_and(|r| r.trim_end_matches('/') == ICC_REGISTRY) && identifier.as_deref().is_some_and(|i| i != "Custom"),
                has_profile: oi.contains_key(b"DestOutputProfile"),
                profile_obj: oi.get_ref(b"DestOutputProfile").and_then(|r| number(r.obj_number)),
                channels: profile.as_ref().and_then(|p| p.dict().get::<i32>(b"N")),
                profile: profile.and_then(|p| p.decoded().ok()).map(|b| b.into_owned()),
            }
        })
        .collect()
}

/// What PDF/X-1a finds wrong with the file's output intent: a CMYK profile, ICC v2 (PDF 1.4
/// allows no later version), or none for a registered printing condition.
fn intent_issues(file: &hayro_syntax::Pdf) -> Vec<String> {
    let Some(oi) = pdfx_intents(file).into_iter().next() else { return Vec::new() };
    let mut issues = Vec::new();
    if oi.has_profile {
        if oi.channels != Some(4) {
            issues.push("the output intent has no CMYK destination profile".into());
        }
        if oi.major() != Some(2) {
            issues.push(format!("the output intent's profile is {}, and PDF/X-1a (PDF 1.4) allows only ICC v2 profiles", oi.version()));
        }
    } else if !oi.registered {
        issues
            .push(format!("the output intent has no destination profile and doesn't name a printing condition in the ICC registry ({ICC_REGISTRY})"));
    }
    issues
}

/// krilla's PDF 1.4 file with what PDF/X-1a needs and krilla doesn't write, rewritten as one
/// revision: `GTS_PDFXConformance` in the document information and the XMP, and the output intent
/// of a registered printing condition without `/DestOutputProfile` (its profile is left out of
/// the file). An unregistered condition keeps its profile, which must then be ICC v2.
pub(crate) fn finish(pdf: &[u8]) -> Result<Vec<u8>, String> {
    let unreadable = || "the flattened file couldn't be read to add the PDF/X-1a identification".to_string();
    let layout = Layout::read(pdf).ok_or_else(unreadable)?;
    let file = hayro_syntax::Pdf::new(pdf.to_vec()).map_err(|_| unreadable())?;
    let mut bodies: HashMap<usize, Vec<u8>> = HashMap::new();
    let info = int_after(&layout.trailer, "/Info").ok_or_else(unreadable)?;
    let body = layout.body(pdf, info).ok_or_else(unreadable)?;
    let (head, close) = body.split_at_checked(rfind(body, b">>").ok_or_else(unreadable)?).ok_or_else(unreadable)?;
    bodies.insert(info, [head, format!("/GTS_PDFXConformance({CONFORMANCE})").as_bytes(), close].concat());
    let xref = file.xref();
    if let Some(meta) = xref.get::<Dict<'_>>(xref.root_id()).and_then(|c| c.get_ref(b"Metadata")).and_then(|r| usize::try_from(r.obj_number).ok())
        && let Some(xmp) = layout.body(pdf, meta).and_then(xmp_with_conformance)
    {
        bodies.insert(meta, xmp);
    }
    let mut stripped: Vec<usize> = Vec::new();
    for oi in pdfx_intents(&file) {
        if !oi.has_profile {
            continue;
        }
        if !oi.registered {
            if oi.major() != Some(2) {
                return Err(format!(
                    "the output intent's profile `{}` is {}, and PDF/X-1a (PDF 1.4) allows only ICC v2 profiles: choose a CMYK working space with an ICC v2 profile, or Coated FOGRA39",
                    oi.name,
                    oi.version()
                ));
            }
            continue;
        }
        let (Some(obj), Some(profile)) = (oi.obj, oi.profile_obj) else { return Err(unreadable()) };
        let body = match bodies.get(&obj) {
            Some(b) => b.clone(),
            None => layout.body(pdf, obj).ok_or_else(unreadable)?.to_vec(),
        };
        let key = format!("/DestOutputProfile {profile} 0 R");
        let at = find(&body, key.as_bytes(), 0).ok_or_else(unreadable)?;
        let (before, rest) = body.split_at_checked(at).ok_or_else(unreadable)?;
        let after = rest.get(key.len()..).ok_or_else(unreadable)?;
        bodies.insert(obj, [before, after].concat());
        stripped.push(profile);
    }
    // A profile only the stripped intents named goes too.
    let dropped: HashSet<usize> = stripped.iter().copied().filter(|p| references(pdf, *p) == stripped.iter().filter(|q| *q == p).count()).collect();
    layout.write(pdf, &bodies, &dropped).ok_or_else(unreadable)
}

/// How often `n 0 R` appears in `pdf` (a stream that happens to hold those bytes counts too, which
/// only ever keeps an object).
fn references(pdf: &[u8], n: usize) -> usize {
    let needle = format!("{n} 0 R");
    let mut count = 0;
    let mut from = 0;
    while let Some(at) = find(pdf, needle.as_bytes(), from) {
        let before = at.checked_sub(1).and_then(|i| pdf.get(i)).is_none_or(|c| !c.is_ascii_digit());
        let after = pdf.get(at + needle.len()).is_none_or(|c| !c.is_ascii_alphanumeric());
        if before && after {
            count += 1;
        }
        from = at + needle.len();
    }
    count
}

/// The body of an uncompressed XMP metadata stream object with the Adobe `pdfx` properties of
/// the identification (`GTS_PDFXVersion`, `GTS_PDFXConformance`) added and its `/Length` updated;
/// `None` when the stream is filtered or isn't XMP.
fn xmp_with_conformance(body: &[u8]) -> Option<Vec<u8>> {
    let s0 = find(body, b"stream", 0)?;
    let dict = String::from_utf8_lossy(body.get(..s0)?).to_string();
    let s0 = s0 + b"stream".len();
    let s0 = s0 + body.get(s0..)?.iter().take_while(|c| **c == b'\r' || **c == b'\n').count();
    let s1 = rfind(body, b"endstream")?;
    let xml = String::from_utf8_lossy(body.get(s0..s1)?).trim_end().to_string();
    if dict.contains("/Filter") || !xml.contains("</rdf:RDF>") {
        return None;
    }
    let ids = format!(
        "<rdf:Description rdf:about=\"\" xmlns:pdfx=\"http://ns.adobe.com/pdfx/1.3/\"><pdfx:GTS_PDFXVersion>{VERSION}</pdfx:GTS_PDFXVersion>\
         <pdfx:GTS_PDFXConformance>{CONFORMANCE}</pdfx:GTS_PDFXConformance></rdf:Description>"
    );
    let xml = xml.replacen("</rdf:RDF>", &format!("{ids}</rdf:RDF>"), 1);
    let i = dict.find("/Length")?;
    let rest = dict.get(i + 7..)?.trim_start();
    let end = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
    let dict = format!("{}/Length {}{}", dict.get(..i)?, xml.len(), rest.get(end..)?);
    Some(format!("\n{}\nstream\n{xml}\nendstream\n", dict.trim()).into_bytes())
}

/// Where the objects of a single-revision file with a classic cross-reference table are.
struct Layout {
    /// (object number, start, end), by start: from `n 0 obj` up to the next object or the table.
    objects: Vec<(usize, usize, usize)>,
    /// Where the cross-reference table starts.
    table: usize,
    /// The trailer dictionary.
    trailer: String,
    size: usize,
}

impl Layout {
    fn read(pdf: &[u8]) -> Option<Layout> {
        let sx = rfind(pdf, b"startxref")?;
        let table: usize = std::str::from_utf8(pdf.get(sx + b"startxref".len()..)?).ok()?.split_whitespace().next()?.parse().ok()?;
        let section = pdf.get(table..sx)?;
        let t = find(section, b"trailer", 0)?;
        let trailer = String::from_utf8_lossy(section.get(t + b"trailer".len()..)?).trim().to_string();
        if trailer.contains("/Prev") || trailer.contains("/XRefStm") {
            return None;
        }
        let size = int_after(&trailer, "/Size")?;
        let mut tokens = std::str::from_utf8(section.get(..t)?).ok()?.split_whitespace();
        if tokens.next()? != "xref" {
            return None;
        }
        let mut starts: Vec<(usize, usize)> = Vec::new();
        let mut seen = HashSet::new();
        while let Some(first) = tokens.next() {
            let first: usize = first.parse().ok()?;
            let count: usize = tokens.next()?.parse().ok()?;
            for i in 0..count {
                let (offset, generation, kind) = (tokens.next()?, tokens.next()?, tokens.next()?);
                if kind != "n" {
                    continue;
                }
                let n = first.checked_add(i)?;
                let offset: usize = offset.parse().ok()?;
                if generation != "00000" || offset >= table || n >= size || !seen.insert(n) {
                    return None;
                }
                starts.push((n, offset));
            }
        }
        // Free entries past the objects would only make the rewritten table longer.
        let max = starts.iter().map(|(n, _)| *n).max().unwrap_or(0);
        if size > max.saturating_mul(2).saturating_add(16) {
            return None;
        }
        starts.sort_by_key(|(_, offset)| *offset);
        let mut objects = Vec::with_capacity(starts.len());
        for (i, &(n, start)) in starts.iter().enumerate() {
            let end = starts.get(i + 1).map_or(table, |(_, s)| *s);
            if !pdf.get(start..end)?.starts_with(format!("{n} 0 obj").as_bytes()) {
                return None;
            }
            objects.push((n, start, end));
        }
        Some(Layout { objects, table, trailer, size })
    }

    /// Object `n` between `n 0 obj` and `endobj`.
    fn body<'p>(&self, pdf: &'p [u8], n: usize) -> Option<&'p [u8]> {
        let &(_, start, end) = self.objects.iter().find(|(m, _, _)| *m == n)?;
        let obj = pdf.get(start..end)?;
        obj.get(format!("{n} 0 obj").len()..rfind(obj, b"endobj")?)
    }

    /// The file with the objects in `bodies` replaced and those in `dropped` left out, and a new
    /// cross-reference table.
    fn write(&self, pdf: &[u8], bodies: &HashMap<usize, Vec<u8>>, dropped: &HashSet<usize>) -> Option<Vec<u8>> {
        let first = self.objects.first().map_or(self.table, |o| o.1);
        let mut out = pdf.get(..first)?.to_vec();
        let mut offsets = HashMap::new();
        for &(n, start, end) in &self.objects {
            if dropped.contains(&n) {
                continue;
            }
            offsets.insert(n, out.len());
            match bodies.get(&n) {
                Some(body) => {
                    out.extend_from_slice(format!("{n} 0 obj").as_bytes());
                    out.extend_from_slice(body);
                    out.extend_from_slice(b"endobj\n");
                }
                None => out.extend_from_slice(pdf.get(start..end)?),
            }
        }
        if !out.ends_with(b"\n") {
            out.push(b'\n');
        }
        let table = out.len();
        // The free entries, each pointing at the next.
        let free: Vec<usize> = (1..self.size).filter(|n| !offsets.contains_key(n)).collect();
        let next: HashMap<usize, usize> = free.iter().copied().zip(free.iter().skip(1).copied().chain([0])).collect();
        out.extend_from_slice(format!("xref\n0 {}\n{:010} 65535 f\r\n", self.size, free.first().copied().unwrap_or(0)).as_bytes());
        for n in 1..self.size {
            let next = next.get(&n).copied().unwrap_or(0);
            let entry = match offsets.get(&n) {
                Some(offset) => format!("{offset:010} 00000 n\r\n"),
                // A dropped object's number could be used again, at the next generation.
                None if dropped.contains(&n) => format!("{next:010} 00001 f\r\n"),
                None => format!("{next:010} 65535 f\r\n"),
            };
            out.extend_from_slice(entry.as_bytes());
        }
        out.extend_from_slice(format!("trailer\n{}\nstartxref\n{table}\n%%EOF\n", self.trailer).as_bytes());
        Some(out)
    }
}

/// Why a color space isn't allowed in PDF/X-1a, or `None` when it is.
fn disallowed(cs: &Object<'_>, res: &Resources<'_>, depth: usize) -> Option<String> {
    if depth > 4 {
        return Some("a color space nested too deeply".into());
    }
    match cs {
        Object::Name(n) => match n.as_ref() {
            b"DeviceCMYK" | b"CMYK" | b"DeviceGray" | b"G" | b"Pattern" => None,
            b"DeviceRGB" | b"RGB" => Some("DeviceRGB color".into()),
            b"Indexed" | b"I" => None,
            other => match res.get_color_space(&Name::new_unescaped(other)) {
                Some(named) if !matches!(named, Object::Name(_)) => disallowed(&named, res, depth + 1),
                Some(Object::Name(m)) if m.as_ref() != other => disallowed(&Object::Name(m), res, depth + 1),
                _ => Some(format!("the color space /{}", String::from_utf8_lossy(other))),
            },
        },
        Object::Array(a) => {
            let mut items = a.iter::<Object<'_>>();
            let family = match items.next() {
                Some(Object::Name(n)) => n,
                _ => return Some("an unreadable color space".into()),
            };
            match family.as_ref() {
                b"ICCBased" => Some("ICC-based color".into()),
                b"CalRGB" | b"CalGray" => Some("CIE-based (CalRGB/CalGray) color".into()),
                b"Lab" => Some("Lab color".into()),
                b"DeviceRGB" | b"RGB" => Some("DeviceRGB color".into()),
                b"DeviceCMYK" | b"CMYK" | b"DeviceGray" | b"G" => None,
                b"Indexed" | b"I" => items.next().and_then(|base| disallowed(&base, res, depth + 1)),
                b"Pattern" => items.next().and_then(|base| disallowed(&base, res, depth + 1)),
                b"Separation" | b"DeviceN" => {
                    let _names = items.next();
                    let alt = items.next()?;
                    if let Some(why) = disallowed(&alt, res, depth + 1) {
                        return Some(format!("a spot color whose alternate space is {why}"));
                    }
                    let _tint = items.next();
                    // NChannel attributes are PDF 1.6, past the 1.4 that PDF/X-1a:2003 allows.
                    let attrs = items.next().and_then(|o| match o {
                        Object::Dict(d) => Some(d),
                        _ => None,
                    });
                    attrs
                        .filter(|a| a.get::<Name<'_>>(b"Subtype").is_some_and(|n| n.as_ref() == b"NChannel") || a.contains_key(b"Process"))
                        .map(|_| "a DeviceN space with NChannel attributes (PDF 1.6)".into())
                }
                other => Some(format!("the color space /{}", String::from_utf8_lossy(other))),
            }
        }
        _ => Some("an unreadable color space".into()),
    }
}

/// What one page has that PDF/X-1a doesn't allow, in the order found.
#[derive(Default)]
struct Scan {
    found: Vec<String>,
    font_issues: Vec<String>,
    seen: HashSet<ObjectIdentifier>,
    fonts: HashSet<ObjectIdentifier>,
}

impl Scan {
    fn add(&mut self, what: String) {
        if !self.found.contains(&what) {
            self.found.push(what);
        }
    }

    fn space(&mut self, cs: &Object<'_>, res: &Resources<'_>) {
        if let Some(why) = disallowed(cs, res, 0) {
            self.add(why);
        }
    }

    fn content(&mut self, data: &[u8], res: &Resources<'_>, depth: usize) {
        if depth > MAX_DEPTH {
            return;
        }
        let mut ops = UntypedIter::new(data);
        while let Some(op) = ops.next() {
            let operator: &[u8] = op.operator;
            let last = op.operands().last().cloned();
            let name = || last.clone().and_then(Object::into_name);
            match operator {
                b"rg" | b"RG" => self.add("DeviceRGB color".into()),
                b"cs" | b"CS" => {
                    if let Some(n) = name() {
                        self.space(&Object::Name(n), res);
                    }
                }
                b"sh" => {
                    if let Some(sh) = name().and_then(|n| res.get_shading(&n)) {
                        self.shading(sh, res);
                    }
                }
                b"scn" | b"SCN" => {
                    if let Some(p) = name().and_then(|n| res.get_pattern(&n)) {
                        self.pattern(p, res, depth);
                    }
                }
                b"gs" => {
                    if let Some(gs) = name().and_then(|n| res.get_ext_g_state(&n)) {
                        self.ext_g_state(&gs, res, depth);
                    }
                }
                b"Tf" => {
                    let font = op.operands().next().cloned().and_then(Object::into_name).and_then(|n| res.get_font(&n));
                    if let Some(font) = font {
                        self.font(&font, res, depth);
                    }
                }
                b"Do" => {
                    if let Some(xo) = name().and_then(|n| res.get_x_object(&n)) {
                        match xo.dict().get::<Name<'_>>(b"Subtype").as_deref() {
                            Some(b"Image") => self.image(&xo, res),
                            Some(b"Form") => self.form(&xo, res, depth),
                            _ => {}
                        }
                    }
                }
                b"BI" | b"EI" => {
                    if let Some(Object::Stream(img)) = last
                        && let Some(cs) = img.dict().get::<Object<'_>>(b"CS").or_else(|| img.dict().get::<Object<'_>>(b"ColorSpace"))
                    {
                        self.space(&cs, res);
                    }
                }
                _ => {}
            }
        }
    }

    fn ext_g_state(&mut self, gs: &Dict<'_>, res: &Resources<'_>, depth: usize) {
        match gs.get::<Object<'_>>(b"SMask") {
            None => {}
            Some(Object::Name(n)) if n.as_ref() == b"None" => {}
            Some(mask) => {
                self.add("a soft mask (transparency)".into());
                if let Some(form) = match mask {
                    Object::Dict(d) => d.get::<Stream<'_>>(b"G"),
                    _ => None,
                } {
                    self.form(&form, res, depth);
                }
            }
        }
        for key in [&b"CA"[..], b"ca"] {
            if gs.get::<f32>(key).is_some_and(|a| a < 1.0) {
                self.add("constant opacity below 100% (transparency)".into());
            }
        }
        let normal = |n: &[u8]| matches!(n, b"Normal" | b"Compatible");
        match gs.get::<Object<'_>>(b"BM") {
            Some(Object::Name(n)) if !normal(n.as_ref()) => {
                self.add(format!("the blend mode /{} (transparency)", String::from_utf8_lossy(n.as_ref())))
            }
            Some(Object::Array(a)) if a.iter::<Name<'_>>().any(|n| !normal(n.as_ref())) => {
                self.add("a blend mode other than Normal (transparency)".into())
            }
            _ => {}
        }
    }

    fn image(&mut self, img: &Stream<'_>, res: &Resources<'_>) {
        if !self.seen.insert(img.obj_id()) {
            return;
        }
        let d = img.dict();
        if d.get::<Object<'_>>(b"SMask").is_some() {
            self.add(format!("an image with a soft mask (object {}, transparency)", img.obj_id().obj_number));
        }
        if d.get::<i32>(b"SMaskInData").is_some_and(|v| v != 0) {
            self.add(format!("an image with transparency in its data (object {})", img.obj_id().obj_number));
        }
        if d.get::<bool>(b"ImageMask") == Some(true) {
            return;
        }
        match d.get::<Object<'_>>(b"ColorSpace") {
            Some(cs) => {
                if let Some(why) = disallowed(&cs, res, 0) {
                    let (w, h) = (d.get::<i32>(b"Width").unwrap_or(0), d.get::<i32>(b"Height").unwrap_or(0));
                    self.add(format!("{why} in an image (object {}, {w} × {h} px)", img.obj_id().obj_number));
                }
            }
            // JPX images may carry their own space; X-1a doesn't allow them.
            None => self.add(format!("an image without a color space (object {})", img.obj_id().obj_number)),
        }
    }

    fn form(&mut self, form: &Stream<'_>, parent: &Resources<'_>, depth: usize) {
        if depth >= MAX_DEPTH || !self.seen.insert(form.obj_id()) {
            return;
        }
        let res = Resources::from_parent(form.dict().get::<Dict<'_>>(b"Resources").unwrap_or_default(), parent.clone());
        if let Some(group) = form.dict().get::<Dict<'_>>(b"Group") {
            self.group(&group);
        }
        if let Ok(data) = form.decoded() {
            self.content(&data, &res, depth + 1);
        }
    }

    fn group(&mut self, group: &Dict<'_>) {
        if group.get::<Name<'_>>(b"S").is_some_and(|s| s.as_ref() == b"Transparency") {
            self.add("a transparency group".into());
        }
    }

    fn shading(&mut self, sh: Object<'_>, res: &Resources<'_>) {
        let dict = match sh {
            Object::Dict(d) => d,
            Object::Stream(s) => s.dict().clone(),
            _ => return,
        };
        if let Some(cs) = dict.get::<Object<'_>>(b"ColorSpace") {
            self.space(&cs, res);
        }
    }

    fn pattern(&mut self, p: Object<'_>, res: &Resources<'_>, depth: usize) {
        match p {
            Object::Dict(d) => {
                if let Some(sh) = d.get::<Object<'_>>(b"Shading") {
                    self.shading(sh, res);
                }
                if let Some(gs) = d.get::<Dict<'_>>(b"ExtGState") {
                    self.ext_g_state(&gs, res, depth);
                }
            }
            Object::Stream(s) => self.form(&s, res, depth),
            _ => {}
        }
    }

    /// Embedded: a simple font's descriptor, or a Type 0 font's descendant's, has a font file. A
    /// Type 3 font's glyphs are content streams in the file, scanned like the page's.
    fn font(&mut self, font: &Dict<'_>, parent: &Resources<'_>, depth: usize) {
        if let Some(id) = font.obj_id()
            && !self.fonts.insert(id)
        {
            return;
        }
        let name = font.get::<Name<'_>>(b"BaseFont").map(|n| String::from_utf8_lossy(n.as_ref()).to_string()).unwrap_or_else(|| "?".into());
        let subtype = font.get::<Name<'_>>(b"Subtype");
        let descriptor = match subtype.as_deref() {
            Some(b"Type3") => return self.type3(font, parent, depth),
            Some(b"Type0") => {
                font.get::<Array<'_>>(b"DescendantFonts").and_then(|a| a.iter::<Dict<'_>>().next()).and_then(|d| d.get::<Dict<'_>>(b"FontDescriptor"))
            }
            _ => font.get::<Dict<'_>>(b"FontDescriptor"),
        };
        let embedded = descriptor.is_some_and(|d| [&b"FontFile"[..], b"FontFile2", b"FontFile3"].iter().any(|k| d.get::<Stream<'_>>(k).is_some()));
        if !embedded {
            self.font_issues.push(format!("the font {name} isn't embedded"));
        }
    }

    /// A Type 3 font's glyph procedures, with the font's resources over `parent`'s.
    fn type3(&mut self, font: &Dict<'_>, parent: &Resources<'_>, depth: usize) {
        if depth >= MAX_DEPTH {
            return;
        }
        let res = Resources::from_parent(font.get::<Dict<'_>>(b"Resources").unwrap_or_default(), parent.clone());
        let Some(procs) = font.get::<Dict<'_>>(b"CharProcs") else { return };
        for key in procs.keys() {
            if let Some(glyph) = procs.get::<Stream<'_>>(key.as_ref())
                && self.seen.insert(glyph.obj_id())
                && let Ok(data) = glyph.decoded()
            {
                self.content(&data, &res, depth + 1);
            }
        }
    }
}
