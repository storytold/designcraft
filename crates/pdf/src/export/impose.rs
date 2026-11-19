//! File › Export Imposed PDF: copies of pages n-up on press sheets (step and repeat). Each copy
//! keeps the document bleed up to half the gap to its neighbours; crop marks go at its corners or
//! along the sheet border.

use designcraft_doc::Page;
use designcraft_geom::Line;
use designcraft_geom::impose::{self as geo, CropMarks, Grid, Placement, Rotation, Spec};

use super::booklet::{finish_imposed, imposed_exporter, imposed_writer};
use super::common::{tf, to_path};
use super::pipeline::bleed_lr;
use super::*;

/// The most copies one export makes.
pub const MAX_COPIES: usize = 100_000;

/// File › Export Imposed PDF settings.
#[derive(Clone, Debug, PartialEq)]
pub struct ImposeOptions {
    pub spec: Spec,
    /// Absolute page indices, in order (`None`: every page).
    pub pages: Option<Vec<usize>>,
    /// Copies of each page; a page's copies follow each other.
    pub copies: usize,
    /// Each copy keeps the document bleed.
    pub bleed: bool,
    pub crop_marks: CropMarks,
    /// Mark line weight in points.
    pub mark_weight: f64,
    /// Distance of the marks from the trim edge in points.
    pub mark_offset: f64,
    pub title: Option<String>,
}

impl Default for ImposeOptions {
    fn default() -> Self {
        Self {
            spec: Spec {
                sheet: designcraft_geom::Size::new(595.276, 841.89),
                margins: [0.0; 4],
                gap: [0.0; 2],
                rotation: Rotation::Auto,
                center: true,
            },
            pages: None,
            copies: 1,
            bleed: false,
            crop_marks: CropMarks::None,
            mark_weight: crate::Marks::NONE.weight,
            mark_offset: crate::Marks::NONE.offset,
            title: None,
        }
    }
}

/// Where every copy goes.
#[derive(Clone, Debug, PartialEq)]
pub struct Imposition {
    pub spec: Spec,
    pub grid: Grid,
    /// The size every copy is placed at: the largest page imposed (smaller pages are centred).
    pub page: designcraft_geom::Size,
    /// Each copy in order: its document page (absolute index) and its place.
    pub copies: Vec<(usize, Placement)>,
    pub sheets: usize,
    /// The bleed each copy keeps (top, bottom, inside, outside); zero without bleed.
    pub bleed: [f64; 4],
}

impl Imposition {
    /// The copies on `sheet`.
    pub fn on_sheet(&self, sheet: usize) -> impl Iterator<Item = &(usize, Placement)> {
        let per = self.grid.per_sheet();
        self.copies.iter().skip(sheet.saturating_mul(per)).take(per)
    }

    /// The crop marks on `sheet`, `offset` pt from the trim edges.
    pub fn marks(&self, sheet: usize, mode: CropMarks, offset: f64) -> Vec<Line> {
        let on: Vec<Placement> = self.on_sheet(sheet).map(|(_, p)| *p).collect();
        let bleed = self.bleed.iter().copied().fold(0.0, f64::max);
        geo::crop_marks(&self.spec, &on, mode, offset, bleed, MARK_LEN)
    }
}

fn page_at(doc: &Document, abs: usize) -> Option<(usize, &Page)> {
    let (si, pi) = doc.page_loc(abs)?;
    Some((si, doc.spreads.get(si)?.pages.get(pi)?))
}

/// Lay out the copies of `opts`' pages on sheets.
pub fn impose(doc: &Document, opts: &ImposeOptions) -> Result<Imposition> {
    let pages: Vec<usize> = match &opts.pages {
        Some(v) => v.clone(),
        None => (0..doc.page_count()).collect(),
    };
    if pages.is_empty() {
        return Err(PdfError::NoPages);
    }
    let (mut w, mut h) = (0.0f64, 0.0f64);
    for &abs in &pages {
        let (_, p) = page_at(doc, abs).ok_or(PdfError::BadPage(abs.saturating_add(1)))?;
        w = w.max(p.width);
        h = h.max(p.height);
    }
    let each = opts.copies.max(1);
    let total =
        pages.len().checked_mul(each).filter(|n| *n <= MAX_COPIES).ok_or_else(|| PdfError::Impose(format!("more than {MAX_COPIES} copies")))?;
    let page = designcraft_geom::Size::new(w, h);
    let grid = geo::grid(&opts.spec, page).map_err(|e| PdfError::Impose(e.to_string()))?;
    let copies = pages.iter().flat_map(|&p| std::iter::repeat_n(p, each)).enumerate().map(|(i, p)| (p, grid.place(&opts.spec, i))).collect();
    let bleed = doc.settings.bleed.map(|v| if opts.bleed && v.is_finite() { v.max(0.0) } else { 0.0 });
    Ok(Imposition { spec: opts.spec, grid, page, copies, sheets: grid.sheets(total), bleed })
}

/// Page `p` (spread space) onto its place on the sheet: centred in the copy's trim, turned 90°
/// clockwise when the copy is rotated.
fn page_transform(p: &Page, pl: &Placement) -> Affine {
    let t = pl.trim;
    if pl.rotated {
        let e = t.x0 + (t.width() - p.height) / 2.0 + p.height;
        let f = t.y0 + (t.height() - p.width) / 2.0;
        Affine::new([0.0, 1.0, -1.0, 0.0, e, f]) * Affine::translate((-p.x, 0.0))
    } else {
        Affine::translate((t.x0 + (t.width() - p.width) / 2.0 - p.x, t.y0 + (t.height() - p.height) / 2.0))
    }
}

/// An imposed PDF: one PDF page per sheet.
pub fn export_imposed(doc: &Document, cache: &Cache, opts: &ImposeOptions) -> Result<ExportReport> {
    write::guard(|| export_imposed_inner(doc, cache, opts))
}

fn export_imposed_inner(doc: &Document, cache: &Cache, opts: &ImposeOptions) -> Result<ExportReport> {
    let im = impose(doc, opts)?;
    let pdf_opts = PdfOptions { title: opts.title.clone(), bleed: opts.bleed, ..PdfOptions::default() };
    let (mut writer, cmyk_icc, warnings) = imposed_writer(doc, &pdf_opts)?;
    let size = Size::from_wh(im.spec.sheet.width as f32, im.spec.sheet.height as f32).ok_or(PdfError::NoPages)?;
    let pdf = writer.document_mut();
    let mut ex = imposed_exporter(doc, cache, &pdf_opts, warnings);
    for sheet in 0..im.sheets {
        let mut page = pdf.start_page_with(PageSettings::new(size));
        let mut s = page.surface();
        #[cfg(all(test, not(target_arch = "wasm32")))]
        unwind_tests::inject(&mut s);
        for (abs, pl) in im.on_sheet(sheet) {
            let Some((si, p)) = page_at(doc, *abs) else { continue };
            let (l, r) = bleed_lr(p.side, im.bleed);
            let bleed = Rect::new(p.x - l, -im.bleed[0], p.x + p.width + r, p.height + im.bleed[1]);
            let (Some(cell), Some(clip)) = (to_path(&pl.clip.to_path(0.1)), to_path(&bleed.to_path(0.1))) else { continue };
            s.push_clip_path(&cell, &FillRule::NonZero);
            s.push_transform(&tf(page_transform(p, pl)));
            s.push_clip_path(&clip, &FillRule::NonZero);
            ex.clip = bleed;
            ex.spread(&mut s, si);
            s.pop();
            s.pop();
            s.pop();
        }
        let mut marks = BezPath::new();
        for l in im.marks(sheet, opts.crop_marks, opts.mark_offset) {
            marks.move_to(l.p0);
            marks.line_to(l.p1);
        }
        ex.stroke_marks(&mut s, &marks, opts.mark_weight);
        s.finish();
        page.finish();
    }
    let (bytes, warnings) = finish_imposed(writer, ex, cmyk_icc.as_deref())?;
    Ok(ExportReport { bytes, pages: im.sheets, warnings })
}
