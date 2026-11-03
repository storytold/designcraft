use super::common::{tf, to_path};
use super::images::ImageContentCache;
use super::metadata::{date_time, export_time, file_info_metadata};
use super::*;

/// Export `doc` as PDF bytes.
pub fn export_pdf(doc: &Document, cache: &Cache, opts: &PdfOptions) -> Result<Vec<u8>> {
    export_pdf_with_report(doc, cache, opts).map(|r| r.bytes)
}

/// One output page: a document page or a spread, in spread coordinates.
pub(crate) struct Sheet {
    pub spread: usize,
    pub trim: Rect,
    pub bleed: Rect,
    pub media: Rect,
    /// Page name(s) for the page-information mark.
    pub label: String,
}

pub(crate) const MARK_LEN: f64 = 18.0;

/// Whether a layer's items go into the PDF.
pub(crate) fn layer_exported(layer: &designcraft_doc::Layer) -> bool {
    layer.visible && layer.printable
}

/// Whether an item and everything inside it go into the PDF, its layer aside.
pub(crate) fn item_exported(it: &Item) -> bool {
    !it.hidden && !it.nonprinting
}

/// Every item of spread `si` that goes into the PDF, with its parent's transform: the items on
/// exported layers, leaving out hidden and non-printing items with everything inside them.
pub(crate) fn exported_items(doc: &Document, si: usize) -> Vec<(Affine, &Item)> {
    let Some(sp) = doc.spreads.get(si) else { return Vec::new() };
    let mut stack: Vec<(Affine, &Item)> =
        sp.items.iter().filter(|it| doc.layer(it.layer).is_some_and(layer_exported)).map(|it| (Affine::IDENTITY, &**it)).collect();
    let mut out = Vec::new();
    while let Some((xf, it)) = stack.pop() {
        if !item_exported(it) {
            continue;
        }
        stack.extend(it.shown_children().map(|c| (xf * it.xf, &**c)));
        out.push((xf, it));
    }
    out
}

fn bleed_lr(side: PageSide, b: [f64; 4]) -> (f64, f64) {
    // Bleed is top, bottom, inside, outside; single-sided pages read inside as left.
    if side == PageSide::Left { (b[3], b[2]) } else { (b[2], b[3]) }
}

/// The spread each exported PDF page shows (in page order).
pub fn sheet_spreads(doc: &Document, opts: &PdfOptions) -> Vec<usize> {
    sheets(doc, opts).map(|v| v.iter().map(|s| s.spread).collect()).unwrap_or_default()
}

fn sheets(doc: &Document, opts: &PdfOptions) -> Result<Vec<Sheet>> {
    let count = doc.page_count();
    let pages: Vec<usize> = match &opts.pages {
        Some(v) => v.clone(),
        None => (0..count).collect(),
    };
    if pages.is_empty() {
        return Err(PdfError::NoPages);
    }
    if let Some(bad) = pages.iter().find(|p| **p >= count) {
        return Err(PdfError::BadPage(bad.saturating_add(1)));
    }
    let b = doc.settings.bleed.map(|v| if opts.bleed { v.max(0.0) } else { 0.0 });
    let mut out: Vec<Sheet> = Vec::new();
    let mut seen_spreads: Vec<usize> = Vec::new();
    for abs in pages {
        let (si, pi) = doc.page_loc(abs).ok_or(PdfError::BadPage(abs.saturating_add(1)))?;
        let sp: &Spread = &doc.spreads[si];
        let (trim, l, r, label) = if opts.spreads {
            if seen_spreads.contains(&si) {
                continue;
            }
            seen_spreads.push(si);
            let first = doc.first_page_of_spread(si);
            // The outer edges by position: a right-to-left spread lists its right page first.
            let outer = sp.outer_page_indices();
            let (l, _) = outer.and_then(|(l, _)| sp.pages.get(l)).map(|p| bleed_lr(p.side, b)).unwrap_or((b[2], b[3]));
            let (_, r) = outer.and_then(|(_, r)| sp.pages.get(r)).map(|p| bleed_lr(p.side, b)).unwrap_or((b[2], b[3]));
            let names: Vec<String> = (0..sp.pages.len()).map(|i| doc.page_name(first + i)).collect();
            (sp.bounds(), l, r, names.join("–"))
        } else {
            let p = &sp.pages[pi];
            let (l, r) = bleed_lr(p.side, b);
            (p.bounds(), l, r, doc.page_name(abs))
        };
        let bleed = Rect::new(trim.x0 - l, trim.y0 - b[0], trim.x1 + r, trim.y1 + b[1]);
        let media = if opts.marks.any() {
            let m = mark_start(opts, b) + MARK_LEN + 6.0;
            trim.inflate(m, m)
        } else {
            bleed
        };
        out.push(Sheet { spread: si, trim, bleed, media, label });
    }
    Ok(out)
}

/// Distance from the trim edge where marks start: the offset, but never inside the bleed.
pub(crate) fn mark_start(opts: &PdfOptions, bleed: [f64; 4]) -> f64 {
    bleed.iter().copied().fold(opts.marks.offset.max(0.0), f64::max)
}

/// Whether an export is CMYK print output: transparency blends in CMYK, in the document's CMYK
/// working space (which PDF/X also carries as its output intent). True when the document blends
/// in CMYK, and always in PDF/X; PDF/A exports are RGB.
pub fn print_color(doc: &Document, opts: &PdfOptions) -> bool {
    opts.standard != Standard::PdfA2b && (doc.settings.blend_space == designcraft_doc::BlendSpace::Cmyk || opts.standard.is_pdfx())
}

/// Like [`export_pdf`], also returning warnings about approximated or dropped features.
pub fn export_pdf_with_report(doc: &Document, cache: &Cache, opts: &PdfOptions) -> Result<ExportReport> {
    write::guard(|| export_pdf_with_report_inner(doc, cache, opts))
}

/// Form fields (Buttons and Forms) and, for interactive PDF, video and sound; not in PDF/X
/// or PDF/A: this update comes after krilla's validator ran, and its NeedAppearances fields and
/// Screen annotations aren't allowed in PDF/A.
fn add_form_fields(bytes: &mut Vec<u8>, warnings: &mut Vec<String>, doc: &Document, sheets: &[Sheet], opts: &PdfOptions) {
    let fields = crate::forms::collect(doc, sheets);
    let media = if opts.media { crate::forms::collect_media(doc, sheets) } else { Vec::new() };
    if fields.is_empty() && media.is_empty() {
        return;
    }
    match opts.standard {
        Standard::PdfX4 => warnings.push("PDF/X-4: form fields and media were left out".into()),
        Standard::PdfX1a => warnings.push("PDF/X-1a: form fields and media were left out".into()),
        Standard::PdfA2b => warnings.push("PDF/A-2b: form fields and media were left out".into()),
        _ => match crate::forms::add_fields(&*bytes, &fields, &media) {
            Some(b) => *bytes = b,
            None => warnings.push("form fields and media couldn't be added to this PDF".into()),
        },
    }
}

fn export_pdf_with_report_inner(doc: &Document, cache: &Cache, opts: &PdfOptions) -> Result<ExportReport> {
    let sheets = sheets(doc, opts)?;
    let (version, archival) = match opts.standard {
        Standard::None => (PdfVersion::Pdf17, None),
        // PDF/X-1a starts as the PDF/X-4 print file, then is flattened.
        Standard::PdfX4 | Standard::PdfX1a => (PdfVersion::Pdf16, None),
        Standard::PdfA2b => (PdfVersion::Pdf17, Some(Archival::A2_B)),
    };
    let configuration = write::configuration(version, archival)?;
    let mut warnings = Vec::new();
    let base = if print_color(doc, opts) {
        // An output intent only for PDF/X, as in InDesign.
        let (settings, warning) = crate::pdfx::print_settings(opts.standard.is_pdfx())?;
        warnings.extend(warning);
        let pdfx_version = (opts.standard == Standard::PdfX4).then(|| "PDF/X-4".to_string());
        krilla::SerializeSettings { pdfx_version, ..settings }
    } else {
        krilla::SerializeSettings::default()
    };
    let settings = krilla::SerializeSettings { compress_content_streams: opts.compress, configuration, ..base };
    let title = opts.title.clone().unwrap_or_else(|| doc.title.clone());
    let created = opts.created.or_else(export_time);
    let metadata = Metadata { created: created.map(date_time), ..file_info_metadata(doc, opts.title.as_deref(), opts.author.as_deref()) };
    let mut writer = Writer::new(settings, metadata);
    let pdf = writer.document_mut();

    let mut ex = Exporter {
        drawing_items: Vec::new(),
        inks: crate::inks::ink_set(doc),
        doc,
        cache,
        composed: HashMap::new(),
        opts,
        clip: Rect::ZERO,
        warnings,
        images: HashMap::new(),
        images_by_content: ImageContentCache::default(),
        #[cfg(test)]
        image_conversions: 0,
        pdfs: HashMap::new(),
        variants: HashMap::new(),
        has_optional_content: false,
        #[cfg(test)]
        pdf_preparations: 0,
        layer_error: None,
        version,
        too_new: Vec::new(),
        svgs: HashMap::new(),
        fonts: HashMap::new(),
        reverse_cmaps: HashMap::new(),
        rgb_only: archival.is_some(),
        tags: Vec::new(),
        story_tags: HashMap::new(),
        tag_story: None,
        text_area: None,
        para_tags: HashMap::new(),
    };
    if ex.rgb_only {
        ex.warn("PDF/A: CMYK colours were converted to RGB (no CMYK output intent profile is available yet)");
    }
    if let Some(o) = crate::links::outline(doc, &sheets) {
        pdf.set_outline(o);
    }
    for (sheet_idx, sh) in sheets.iter().enumerate() {
        let size = Size::from_wh(sh.media.width().max(1.0) as f32, sh.media.height().max(1.0) as f32).ok_or(PdfError::NoPages)?;
        let local = |r: Rect| {
            krilla::geom::Rect::from_ltrb(
                (r.x0 - sh.media.x0) as f32,
                (r.y0 - sh.media.y0) as f32,
                (r.x1 - sh.media.x0) as f32,
                (r.y1 - sh.media.y0) as f32,
            )
        };
        let settings = PageSettings::new(size).with_trim_box(local(sh.trim)).with_bleed_box(local(sh.bleed));
        let mut page = pdf.start_page_with(settings);
        let mut s = page.surface();
        #[cfg(all(test, not(target_arch = "wasm32")))]
        unwind_tests::inject(&mut s);
        s.push_transform(&tf(Affine::translate(-sh.media.origin().to_vec2())));
        ex.clip = sh.bleed;
        if let Some(clip) = to_path(&sh.bleed.to_path(0.1)) {
            s.push_clip_path(&clip, &FillRule::NonZero);
            ex.spread(&mut s, sh.spread);
            s.pop();
        }
        if opts.tagged {
            s.start_tagged(ContentTag::Artifact(Artifact::new(ArtifactType::Page, None)));
        }
        ex.marks(&mut s, sh, &title, created);
        if opts.tagged {
            s.end_tagged();
        }
        s.pop();
        s.finish();
        for a in crate::links::annotations(doc, cache, &sheets, sheet_idx) {
            page.add_annotation(a);
        }
        page.finish();
    }
    if opts.tagged {
        pdf.set_tag_tree(ex.tag_tree());
    }
    ex.check_placed_versions()?;
    if let Some(error) = ex.layer_error {
        return Err(error);
    }
    let mut bytes = writer.finish().map_err(PdfError::from)?;
    let mut warnings = ex.warnings;
    if ex.has_optional_content {
        bytes = crate::optional_content::register(&bytes)?;
    }
    add_form_fields(&mut bytes, &mut warnings, doc, &sheets, opts);
    if opts.standard == Standard::PdfX4 {
        // The XMP dates krilla doesn't write, then check the result.
        match crate::pdfx::make_pdfx4(&bytes, created) {
            Some(b) => bytes = b,
            None => warnings.push("PDF/X-4: the XMP metadata date couldn't be added".into()),
        }
        for issue in crate::pdfx::check_pdfx4(&bytes) {
            warnings.push(format!("PDF/X-4: {issue}"));
        }
    }
    if opts.standard == Standard::PdfX1a {
        bytes = crate::pdfx1a::flatten_x1a(&bytes, opts.flatten_dpi, &page_graphics(doc, &sheets), &mut warnings)?;
    }
    warnings.dedup();
    Ok(ExportReport { bytes, pages: sheets.len(), warnings })
}

/// Each sheet's name and the names of the graphics placed on it, for PDF/X-1a's errors.
fn page_graphics(doc: &Document, sheets: &[Sheet]) -> Vec<crate::pdfx1a::PageGraphics> {
    sheets
        .iter()
        .map(|sh| {
            let mut graphics: Vec<String> = Vec::new();
            for (xf, it) in exported_items(doc, sh.spread) {
                let Some(asset) = it.graphic().and_then(|g| doc.assets.get(&g.asset)) else { continue };
                let shown = xf.transform_rect_bbox(it.bounds()).intersect(sh.media);
                if shown.width() > 0.0 && shown.height() > 0.0 && !graphics.contains(&asset.name) {
                    graphics.push(asset.name.clone());
                }
            }
            crate::pdfx1a::PageGraphics { label: sh.label.clone(), graphics }
        })
        .collect()
}

/// `data` with its `%PDF-x.y` header (within the first 1024 bytes, where readers look for it)
/// changed from `from` to `to`, both the same length. `None` when the header isn't `from`.
pub(super) fn relabel_pdf_header(data: &[u8], from: &[u8], to: &[u8]) -> Option<Vec<u8>> {
    if from.len() != to.len() {
        return None;
    }
    let head = data.get(..data.len().min(1024))?;
    let at = head.windows(from.len()).position(|w| w == from)?;
    let mut out = data.to_vec();
    out.get_mut(at..at + to.len())?.copy_from_slice(to);
    Some(out)
}

/// `data` with every `/Version /1.7` changed to `/Version /1.6` in place, so the xref offsets
/// hold. The catalog's `/Version` wins over the header (TCPDF writes both), so relabelling the
/// header alone leaves the file a PDF 1.7. A catalog inside a compressed object stream is left as
/// it is, and the version check still names the file.
pub(super) fn relabel_catalog_version(mut data: Vec<u8>) -> Vec<u8> {
    const KEY: &[u8] = b"/Version";
    let mut from = 0;
    while let Some(at) = data.get(from..).and_then(|rest| rest.windows(KEY.len()).position(|w| w == KEY)) {
        let mut i = from + at + KEY.len();
        while data.get(i).is_some_and(|b| b" \t\r\n\x0c\0".contains(b)) {
            i += 1;
        }
        let ends = data.get(i + 4).is_none_or(|b| !b.is_ascii_alphanumeric() && *b != b'.');
        if data.get(i..i + 4) == Some(b"/1.7".as_slice())
            && ends
            && let Some(b) = data.get_mut(i + 3)
        {
            *b = b'6';
        }
        from = i;
    }
    data
}
