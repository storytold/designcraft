use super::common::{tf, to_path};
use super::images::ImageContentCache;
use super::metadata::file_info_metadata;
use super::pipeline::print_color;
use super::*;

/// One PDF from several (a book's documents), every page carried over as vectors; each part
/// comes with its pages' sizes (points).
pub fn merge_pdfs(parts: &[(Vec<u8>, Vec<(f32, f32)>)], title: Option<&str>) -> Result<Vec<u8>> {
    write::guard(|| merge_pdfs_inner(parts, title))
}

fn merge_pdfs_inner(parts: &[(Vec<u8>, Vec<(f32, f32)>)], title: Option<&str>) -> Result<Vec<u8>> {
    let metadata = Metadata { creator: "DesignCraft".into(), producer: "DesignCraft".into(), title: title.map(str::to_owned), ..Default::default() };
    let mut writer = Writer::new(krilla::SerializeSettings::default(), metadata);
    let pdf = writer.document_mut();
    let mut pages = 0;
    let mut has_optional_content = false;
    for (bytes, sizes) in parts {
        let variant = crate::optional_content::prepare(&Arc::new(bytes.clone()), None)?;
        has_optional_content |= !sizes.is_empty() && variant.has_optional_content;
        let src = krilla::pdf::Pdf::new(variant.data).map_err(|e| PdfError::Write(format!("{e:?}")))?;
        let doc = krilla::pdf::PdfDocument::new(Arc::new(src));
        for (i, &(w, h)) in sizes.iter().enumerate() {
            let size = Size::from_wh(w.max(1.0), h.max(1.0)).ok_or(PdfError::NoPages)?;
            let mut page = pdf.start_page_with(PageSettings::new(size));
            let mut s = page.surface();
            #[cfg(all(test, not(target_arch = "wasm32")))]
            unwind_tests::inject(&mut s);
            s.draw_pdf_page(&doc, size, i);
            s.finish();
            page.finish();
            pages += 1;
        }
    }
    if pages == 0 {
        return Err(PdfError::NoPages);
    }
    let bytes = writer.finish().map_err(PdfError::from)?;
    if has_optional_content { crate::optional_content::register(&bytes) } else { Ok(bytes) }
}

/// File › Print Booklet: how pages pair up on printer spreads.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BookletKind {
    /// Folded and stapled: pages padded to a multiple of 4, outer pages together.
    #[default]
    SaddleStitch,
    /// Two pages side by side in reading order.
    TwoUpConsecutive,
}

#[derive(Clone, Debug, Default)]
pub struct BookletOptions {
    pub kind: BookletKind,
    /// Gap between the two pages of a printer spread (points).
    pub space_between: f64,
    pub title: Option<String>,
}

/// The printer spreads for `n` pages: (left, right) absolute page indices, `None` = blank.
pub fn booklet_pairs(n: usize, kind: BookletKind) -> Vec<(Option<usize>, Option<usize>)> {
    let page = |i: usize| (i < n).then_some(i);
    match kind {
        BookletKind::TwoUpConsecutive => (0..n.div_ceil(2)).map(|k| (page(2 * k), page(2 * k + 1))).collect(),
        BookletKind::SaddleStitch => {
            let m = n.div_ceil(4).max(1) * 4;
            (0..m / 2).map(|i| if i % 2 == 0 { (page(m - 1 - i), page(i)) } else { (page(i), page(m - 1 - i)) }).collect()
        }
    }
}

/// An imposed PDF: two document pages per sheet, in booklet order.
pub fn export_booklet(doc: &Document, cache: &Cache, opts: &BookletOptions) -> Result<ExportReport> {
    write::guard(|| export_booklet_inner(doc, cache, opts))
}

fn export_booklet_inner(doc: &Document, cache: &Cache, opts: &BookletOptions) -> Result<ExportReport> {
    let n = doc.page_count();
    if n == 0 {
        return Err(PdfError::NoPages);
    }
    let pdf_opts = PdfOptions { title: opts.title.clone(), ..PdfOptions::default() };
    let mut warnings = Vec::new();
    let base = if print_color(doc, &pdf_opts) {
        let (settings, warning) = crate::pdfx::print_settings(false)?;
        warnings.extend(warning);
        settings
    } else {
        krilla::SerializeSettings::default()
    };
    let settings = krilla::SerializeSettings { compress_content_streams: true, ..base };
    let metadata = file_info_metadata(doc, opts.title.as_deref(), None);
    let mut writer = Writer::new(settings, metadata);
    let pdf = writer.document_mut();
    let mut ex = Exporter {
        drawing_items: Vec::new(),
        inks: crate::inks::ink_set(doc),
        doc,
        cache,
        composed: HashMap::new(),
        opts: &pdf_opts,
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
        // krilla's default.
        version: PdfVersion::Pdf17,
        too_new: Vec::new(),
        svgs: HashMap::new(),
        fonts: HashMap::new(),
        reverse_cmaps: HashMap::new(),
        rgb_only: false,
        tags: Vec::new(),
        story_tags: HashMap::new(),
        tag_story: None,
        text_area: None,
        para_tags: HashMap::new(),
    };
    let pairs = booklet_pairs(n, opts.kind);
    // Sheet size from the largest page.
    let (mut pw, mut ph) = (0.0f64, 0.0f64);
    for i in 0..n {
        if let Some((si, pi)) = doc.page_loc(i) {
            let p = &doc.spreads[si].pages[pi];
            pw = pw.max(p.width);
            ph = ph.max(p.height);
        }
    }
    let gap = opts.space_between.max(0.0);
    let size = Size::from_wh((2.0 * pw + gap) as f32, ph as f32).ok_or(PdfError::NoPages)?;
    for (left, right) in &pairs {
        let mut page = pdf.start_page_with(PageSettings::new(size));
        let mut s = page.surface();
        #[cfg(all(test, not(target_arch = "wasm32")))]
        unwind_tests::inject(&mut s);
        for (slot, abs) in [(0.0, left), (pw + gap, right)] {
            let Some((si, pi)) = abs.and_then(|a| doc.page_loc(a)) else { continue };
            let p = &doc.spreads[si].pages[pi];
            let r = Rect::new(p.x, 0.0, p.x + p.width, p.height);
            // Pages narrower than the slot sit against the fold.
            let dx = if slot == 0.0 { pw - p.width } else { 0.0 };
            s.push_transform(&tf(Affine::translate((slot + dx - p.x, (ph - p.height) / 2.0))));
            ex.clip = r;
            if let Some(clip) = to_path(&r.to_path(0.1)) {
                s.push_clip_path(&clip, &FillRule::NonZero);
                ex.spread(&mut s, si);
                s.pop();
            }
            s.pop();
        }
        s.finish();
        page.finish();
    }
    ex.check_placed_versions()?;
    if let Some(error) = ex.layer_error {
        return Err(error);
    }
    let mut bytes = writer.finish().map_err(PdfError::from)?;
    if ex.has_optional_content {
        bytes = crate::optional_content::register(&bytes)?;
    }
    let mut warnings = ex.warnings;
    warnings.dedup();
    Ok(ExportReport { bytes, pages: pairs.len(), warnings })
}
