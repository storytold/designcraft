//! Document → PDF. The item walk mirrors `designcraft-render`'s `draw_spread` / `draw_item`.

use std::collections::HashMap;
use std::sync::Arc;

use designcraft_color::swatch::{ColorType, SwatchValue};
use designcraft_color::{BlendMode as DcBlend, Color, GradientKind};
use designcraft_compose::Cache;
use designcraft_doc::{AssetId, Content, Document, Item, PageSide, Spread, StrokeAlign, StrokeType};
use designcraft_geom::kurbo::{PathEl, Shape as _};
use designcraft_geom::{Affine, BezPath, Rect, Vec2, corners};
use krilla::color::{cmyk, luma, rgb};
use krilla::configure::{Archival, ConfigurationBuilder, PdfVersion};
use krilla::geom::{Path, PathBuilder, Size, Transform};
use krilla::image::Image;
use krilla::metadata::Metadata;
use krilla::num::NormalizedF32;
use krilla::page::PageSettings;
use krilla::paint::{Fill, FillRule, LinearGradient, RadialGradient, SpreadMethod, Stop, Stroke, StrokeDash};
use krilla::surface::Surface;
use krilla::tagging::{Artifact, ArtifactType, ContentTag, Identifier, Tag, TagGroup, TagTree};

use crate::{ExportReport, PdfError, PdfOptions, Result, Standard};

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
        return Err(PdfError::BadPage(bad + 1));
    }
    let b = doc.settings.bleed.map(|v| if opts.bleed { v.max(0.0) } else { 0.0 });
    let mut out: Vec<Sheet> = Vec::new();
    let mut seen_spreads: Vec<usize> = Vec::new();
    for abs in pages {
        let (si, pi) = doc.page_loc(abs).ok_or(PdfError::BadPage(abs + 1))?;
        let sp: &Spread = &doc.spreads[si];
        let (trim, l, r, label) = if opts.spreads {
            if seen_spreads.contains(&si) {
                continue;
            }
            seen_spreads.push(si);
            let first = doc.first_page_of_spread(si);
            let (l, _) = sp.pages.first().map(|p| bleed_lr(p.side, b)).unwrap_or((b[2], b[3]));
            let (_, r) = sp.pages.last().map(|p| bleed_lr(p.side, b)).unwrap_or((b[2], b[3]));
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

/// Like [`export_pdf`], also returning warnings about approximated or dropped features.
pub fn export_pdf_with_report(doc: &Document, cache: &Cache, opts: &PdfOptions) -> Result<ExportReport> {
    let sheets = sheets(doc, opts)?;
    let warnings = Vec::new();
    let (version, archival) = match opts.standard {
        Standard::None => (PdfVersion::Pdf17, None),
        Standard::PdfX4 => (PdfVersion::Pdf16, None),
        Standard::PdfA2b => (PdfVersion::Pdf17, Some(Archival::A2_B)),
    };
    let mut cb = ConfigurationBuilder::new().with_version(version);
    if let Some(a) = archival {
        cb = cb.with_archival_validator(a);
    }
    let configuration = cb.finish().map_err(|e| PdfError::Write(format!("{e:?}")))?;
    let settings = krilla::SerializeSettings { compress_content_streams: opts.compress, configuration, ..Default::default() };
    let mut pdf = krilla::Document::new_with(settings);

    let title = opts.title.clone().unwrap_or_else(|| doc.title.clone());
    let mut meta = Metadata::new().creator("DesignCraft".into()).producer("DesignCraft".into());
    if !title.is_empty() {
        meta = meta.title(title.clone());
    }
    if let Some(a) = &opts.author {
        meta = meta.authors(vec![a.clone()]);
    }
    let created = opts.created.or_else(now_unix);
    if let Some(t) = created {
        let c = civil(t);
        meta = meta.creation_date(
            krilla::metadata::DateTime::new(c.0.clamp(0, 9999) as u16)
                .month(c.1)
                .day(c.2)
                .hour(c.3)
                .minute(c.4)
                .second(c.5)
                .utc_offset_hour(0)
                .utc_offset_minute(0),
        );
    }
    pdf.set_metadata(meta);

    let mut ex = Exporter {
        doc,
        cache,
        opts,
        clip: Rect::ZERO,
        warnings,
        images: HashMap::new(),
        pdfs: HashMap::new(),
        version,
        too_new: Vec::new(),
        svgs: HashMap::new(),
        fonts: HashMap::new(),
        reverse_cmaps: HashMap::new(),
        rgb_only: archival.is_some(),
        interpolate: archival.is_none(),
        tags: Vec::new(),
        story_tags: HashMap::new(),
        tag_story: None,
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
    let mut bytes = pdf.finish().map_err(|e| PdfError::Write(format!("{e:?}")))?;
    // Transparency blends in the document's blend space: CMYK for print (and always in PDF/X).
    let mut warnings = ex.warnings;
    if !ex.rgb_only && (doc.settings.blend_space == designcraft_doc::BlendSpace::Cmyk || opts.standard == Standard::PdfX4) {
        // The rewrite matches krilla's exact output; in PDF/X-4, `check_pdfx4` reports any
        // group that got past it, with its page.
        crate::pdfx::cmyk_group_spaces(&mut bytes);
    }
    // Form fields (Buttons and Forms) and, for interactive PDF, video and sound; not in PDF/X.
    let fields = crate::forms::collect(doc, &sheets);
    let media = if opts.media { crate::forms::collect_media(doc, &sheets) } else { Vec::new() };
    if !fields.is_empty() || !media.is_empty() {
        if opts.standard == Standard::PdfX4 {
            warnings.push("PDF/X-4: form fields and media were left out".into());
        } else {
            match crate::forms::add_fields(&bytes, &fields, &media) {
                Some(b) => bytes = b,
                None => warnings.push("form fields and media couldn't be added to this PDF".into()),
            }
        }
    }
    if opts.standard == Standard::PdfX4 {
        // Output intent and PDF/X identification, then check the result.
        match crate::pdfx::make_pdfx4(&bytes, &title) {
            Some(b) => bytes = b,
            None => warnings.push("PDF/X-4: the output intent couldn't be added".into()),
        }
        for issue in crate::pdfx::check_pdfx4(&bytes) {
            warnings.push(format!("PDF/X-4: {issue}"));
        }
    }
    warnings.dedup();
    Ok(ExportReport { bytes, pages: sheets.len(), warnings })
}

/// One PDF from several (a book's documents), every page carried over as vectors; each part
/// comes with its pages' sizes (points).
pub fn merge_pdfs(parts: &[(Vec<u8>, Vec<(f32, f32)>)], title: Option<&str>) -> Result<Vec<u8>> {
    let mut pdf = krilla::Document::new_with(krilla::SerializeSettings::default());
    let mut meta = Metadata::new().creator("DesignCraft".into()).producer("DesignCraft".into());
    if let Some(t) = title {
        meta = meta.title(t.to_string());
    }
    pdf.set_metadata(meta);
    let mut pages = 0;
    for (bytes, sizes) in parts {
        let src = krilla::pdf::Pdf::new(Arc::new(bytes.clone())).map_err(|e| PdfError::Write(format!("{e:?}")))?;
        let doc = krilla::pdf::PdfDocument::new(Arc::new(src));
        for (i, &(w, h)) in sizes.iter().enumerate() {
            let size = Size::from_wh(w.max(1.0), h.max(1.0)).ok_or(PdfError::NoPages)?;
            let mut page = pdf.start_page_with(PageSettings::new(size));
            let mut s = page.surface();
            s.draw_pdf_page(&doc, size, i);
            s.finish();
            page.finish();
            pages += 1;
        }
    }
    if pages == 0 {
        return Err(PdfError::NoPages);
    }
    pdf.finish().map_err(|e| PdfError::Write(format!("{e:?}")))
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
    let n = doc.page_count();
    if n == 0 {
        return Err(PdfError::NoPages);
    }
    let pdf_opts = PdfOptions { title: opts.title.clone(), ..PdfOptions::default() };
    let settings = krilla::SerializeSettings { compress_content_streams: true, ..Default::default() };
    let mut pdf = krilla::Document::new_with(settings);
    let title = opts.title.clone().unwrap_or_else(|| doc.title.clone());
    let mut meta = Metadata::new().creator("DesignCraft".into()).producer("DesignCraft".into());
    if !title.is_empty() {
        meta = meta.title(title);
    }
    pdf.set_metadata(meta);
    let mut ex = Exporter {
        doc,
        cache,
        opts: &pdf_opts,
        clip: Rect::ZERO,
        warnings: Vec::new(),
        images: HashMap::new(),
        pdfs: HashMap::new(),
        // krilla's default.
        version: PdfVersion::Pdf17,
        too_new: Vec::new(),
        svgs: HashMap::new(),
        fonts: HashMap::new(),
        reverse_cmaps: HashMap::new(),
        rgb_only: false,
        interpolate: true,
        tags: Vec::new(),
        story_tags: HashMap::new(),
        tag_story: None,
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
    let mut bytes = pdf.finish().map_err(|e| PdfError::Write(format!("{e:?}")))?;
    if doc.settings.blend_space == designcraft_doc::BlendSpace::Cmyk {
        crate::pdfx::cmyk_group_spaces(&mut bytes);
    }
    let mut warnings = ex.warnings;
    warnings.dedup();
    Ok(ExportReport { bytes, pages: pairs.len(), warnings })
}

#[cfg(not(target_arch = "wasm32"))]
fn now_unix() -> Option<i64> {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).ok().map(|d| d.as_secs() as i64)
}
#[cfg(target_arch = "wasm32")]
fn now_unix() -> Option<i64> {
    None
}

/// Unix seconds (UTC) → (year, month, day, hour, minute, second), proleptic Gregorian.
pub(crate) fn civil(t: i64) -> (i64, u8, u8, u8, u8, u8) {
    let days = t.div_euclid(86_400);
    let secs = t.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month as u8, day as u8, (secs / 3600) as u8, (secs / 60 % 60) as u8, (secs % 60) as u8)
}

pub(crate) struct Exporter<'a> {
    pub doc: &'a Document,
    pub cache: &'a Cache,
    pub opts: &'a PdfOptions,
    /// Current sheet's visible area (spread space) for culling.
    pub clip: Rect,
    pub warnings: Vec<String>,
    images: HashMap<AssetId, Option<Image>>,
    /// Placed PDFs (embedded as vector pages).
    pdfs: HashMap<AssetId, Option<krilla::pdf::PdfDocument>>,
    /// The PDF version written; placed PDFs must not be newer.
    version: PdfVersion,
    /// Placed PDFs too new to embed in this version ("name is PDF 2.0"): the export fails.
    too_new: Vec<String>,
    /// Parsed placed SVGs.
    svgs: HashMap<AssetId, Option<Arc<designcraft_images::usvg::Tree>>>,
    pub fonts: HashMap<u32, Option<krilla::text::Font>>,
    pub reverse_cmaps: HashMap<u32, Arc<HashMap<u32, char>>>,
    /// Convert CMYK to RGB (PDF/A: krilla needs a CMYK output profile we don't ship yet).
    pub rgb_only: bool,
    /// Ask viewers to smooth upscaled images (`/Interpolate`). Off for PDF/A, which forbids it.
    interpolate: bool,
    /// Tagged PDF: the structure in reading order (stories gather their frames' content).
    tags: Vec<TagEntry>,
    story_tags: HashMap<designcraft_doc::StoryId, Vec<Identifier>>,
    /// While drawing a story's frame for the structure tree: the story (text runs are tagged
    /// per paragraph, everything else in the frame is an artifact).
    pub(crate) tag_story: Option<designcraft_doc::StoryId>,
    /// Marked content of each (story, paragraph index).
    pub(crate) para_tags: HashMap<(designcraft_doc::StoryId, usize), Vec<Identifier>>,
}

enum TagEntry {
    Story(designcraft_doc::StoryId),
    Figure(TagGroup),
}

pub(crate) fn tf(a: Affine) -> Transform {
    let c = a.as_coeffs();
    Transform::from_row(c[0] as f32, c[1] as f32, c[2] as f32, c[3] as f32, c[4] as f32, c[5] as f32)
}

pub(crate) fn to_path(bp: &BezPath) -> Option<Path> {
    let mut pb = PathBuilder::new();
    for el in bp.elements() {
        match *el {
            PathEl::MoveTo(p) => pb.move_to(p.x as f32, p.y as f32),
            PathEl::LineTo(p) => pb.line_to(p.x as f32, p.y as f32),
            PathEl::QuadTo(a, p) => pb.quad_to(a.x as f32, a.y as f32, p.x as f32, p.y as f32),
            PathEl::CurveTo(a, b, p) => pb.cubic_to(a.x as f32, a.y as f32, b.x as f32, b.y as f32, p.x as f32, p.y as f32),
            PathEl::ClosePath => pb.close(),
        }
    }
    pb.finish()
}

pub(crate) fn norm(v: f32) -> NormalizedF32 {
    NormalizedF32::new(if v.is_finite() { v.clamp(0.0, 1.0) } else { 1.0 }).unwrap_or(NormalizedF32::ONE)
}

fn q(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

fn regular(c: &Color, rgb_only: bool) -> krilla::color::RegularColor {
    use krilla::color::RegularColor as R;
    if rgb_only && matches!(c, Color::Cmyk { .. }) {
        let [r, g, b] = c.to_rgb();
        return R::Rgb(rgb::Color::new(q(r), q(g), q(b)));
    }
    match *c {
        Color::Rgb { r, g, b } => R::Rgb(rgb::Color::new(q(r), q(g), q(b))),
        Color::Cmyk { c, m, y, k } => R::Cmyk(cmyk::Color::new(q(c), q(m), q(y), q(k))),
        // DesignCraft grey is ink coverage (0 = white); PDF DeviceGray is lightness.
        Color::Gray { k } => R::Luma(luma::Color::new(q(1.0 - k))),
    }
}

/// A process colour in its own device space (CMYK → DeviceCMYK, RGB → DeviceRGB, Gray → DeviceGray).
/// `rgb_only` converts CMYK to RGB (PDF/A without a CMYK output profile).
pub(crate) fn device(c: &Color, rgb_only: bool) -> krilla::color::Color {
    match regular(c, rgb_only) {
        krilla::color::RegularColor::Rgb(v) => v.into(),
        krilla::color::RegularColor::Cmyk(v) => v.into(),
        krilla::color::RegularColor::Luma(v) => v.into(),
    }
}

/// `[Registration]` prints on every plate: `/Separation /All`.
pub(crate) fn registration(tint: f32, rgb_only: bool) -> krilla::color::Color {
    use krilla::color::separation::{Color as SepColor, SeparationColorant, SeparationSpace};
    let alt = regular(&Color::cmyk(1.0, 1.0, 1.0, 1.0), rgb_only);
    SepColor::new(q(tint), SeparationSpace::new(SeparationColorant::AllColorants, alt)).into()
}

fn blend(b: DcBlend) -> krilla::blend::BlendMode {
    use krilla::blend::BlendMode as K;
    match b {
        DcBlend::Normal => K::Normal,
        DcBlend::Darken => K::Darken,
        DcBlend::Multiply => K::Multiply,
        DcBlend::ColorBurn => K::ColorBurn,
        DcBlend::Lighten => K::Lighten,
        DcBlend::Screen => K::Screen,
        DcBlend::ColorDodge => K::ColorDodge,
        DcBlend::Overlay => K::Overlay,
        DcBlend::SoftLight => K::SoftLight,
        DcBlend::HardLight => K::HardLight,
        DcBlend::Difference => K::Difference,
        DcBlend::Exclusion => K::Exclusion,
        DcBlend::Hue => K::Hue,
        DcBlend::Saturation => K::Saturation,
        DcBlend::Color => K::Color,
        DcBlend::Luminosity => K::Luminosity,
    }
}

fn rects_overlap(a: Rect, b: Rect) -> bool {
    a.x0 <= b.x1 && b.x0 <= a.x1 && a.y0 <= b.y1 && b.y0 <= a.y1
}

pub(crate) fn solid_fill(c: krilla::color::Color, opacity: f32) -> Fill {
    Fill { paint: c.into(), opacity: norm(opacity), rule: FillRule::NonZero }
}

impl Exporter<'_> {
    /// A print document, or a PDF/X file (whose output intent is CMYK).
    pub(crate) fn for_print(&self) -> bool {
        self.doc.settings.intent == designcraft_doc::Intent::Print || self.opts.standard == Standard::PdfX4
    }

    pub(crate) fn warn(&mut self, w: impl Into<String>) {
        let w = w.into();
        if !self.warnings.contains(&w) {
            self.warnings.push(w);
        }
    }

    /// A swatch + tint as a PDF colour: spot swatches (and tints of them) become `/Separation`
    /// with the swatch's values as the alternate space; process colours stay in their device space.
    pub(crate) fn swatch_color(&self, name: &str, tint: f32) -> Option<krilla::color::Color> {
        use krilla::color::separation::{Color as SepColor, SeparationColorant, SeparationSpace};
        let mut n = name;
        let mut t = tint;
        for _ in 0..8 {
            let sw = self.doc.swatch(n)?;
            match &sw.value {
                SwatchValue::None => return None,
                // [Paper] prints no ink (it knocks out what lies below); its colour is only what
                // the screen shows. Written as RGB white (`1 1 1 rg`) it would put DeviceRGB in a
                // CMYK print file.
                SwatchValue::Paper { .. } if self.for_print() => return Some(device(&Color::cmyk(0.0, 0.0, 0.0, 0.0), self.rgb_only)),
                SwatchValue::Tint { base, tint: bt } => {
                    t *= bt;
                    n = base;
                }
                SwatchValue::Registration => return Some(registration(t, self.rgb_only)),
                SwatchValue::Color { color: _, color_type: ColorType::Spot } => {
                    // Ink Manager: an alias prints on another ink; converted inks go to process.
                    let (ink, process) = self.doc.inks.resolve(&sw.name);
                    let target =
                        self.doc.swatch(ink).filter(|w| matches!(w.value, SwatchValue::Color { color_type: ColorType::Spot, .. })).unwrap_or(sw);
                    let SwatchValue::Color { color, .. } = &target.value else { break };
                    if process {
                        return Some(device(&designcraft_color::swatch::apply_tint(*color, t), self.rgb_only));
                    }
                    let space = SeparationSpace::new(SeparationColorant::Custom(target.name.clone()), regular(color, self.rgb_only));
                    return Some(SepColor::new(q(t), space).into());
                }
                _ => break,
            }
        }
        self.doc.resolve_color(name, tint).map(|c| device(&c, self.rgb_only))
    }

    fn spread(&mut self, s: &mut Surface, si: usize) {
        let doc = self.doc;
        let Some(sp) = doc.spreads.get(si) else { return };
        let first = doc.first_page_of_spread(si);
        for layer in doc.layers.iter().rev() {
            if !layer.visible || !layer.printable {
                continue;
            }
            for (pi, page) in sp.pages.iter().enumerate() {
                if !page.show_parent_items {
                    continue;
                }
                let Some((ppi, ppage)) = doc.parent_page_for(first + pi) else { continue };
                let parent = &doc.parents[ppi];
                let dx = page.x - parent.pages[ppage].x;
                let page_name = doc.page_name(first + pi);
                for it in &parent.items {
                    if it.layer != layer.id || page.overridden.contains(&it.id) {
                        continue;
                    }
                    // Parent items belong to the parent page they sit on.
                    if parent.page_at_x(it.bounds().center().x) != Some(ppage) && parent.pages.len() > 1 {
                        continue;
                    }
                    self.top_item(s, it, Affine::translate((dx, 0.0)), Some(&page_name), true);
                }
            }
            for it in &sp.items {
                if it.layer == layer.id {
                    self.top_item(s, it, Affine::IDENTITY, None, false);
                }
            }
        }
    }

    /// Push blend/opacity for an item drawn as a group. Returns the number of pushes.
    fn push_group(s: &mut Surface, opacity: f32, mode: DcBlend) -> usize {
        let mut n = 0;
        if mode != DcBlend::Normal {
            s.push_blend_mode(blend(mode));
            n += 1;
        }
        if opacity < 0.999 {
            s.push_opacity(norm(opacity));
            n += 1;
        } else if mode != DcBlend::Normal {
            s.push_isolated();
            n += 1;
        }
        n
    }

    /// Isolate Blending / Knockout groups: an isolated transparency group (knockout is drawn
    /// isolated too).
    fn push_isolation(s: &mut Surface, it: &Item) -> usize {
        if it.isolate || it.knockout {
            s.push_isolated();
            1
        } else {
            0
        }
    }

    /// A spread or parent item, inside its structure tag when tagging.
    fn top_item(&mut self, s: &mut Surface, it: &Item, parent: Affine, page_name: Option<&str>, on_parent: bool) {
        if !self.opts.tagged || it.hidden || it.nonprinting {
            return self.item(s, it, parent, page_name);
        }
        let story = it.text_frame().map(|t| t.story);
        let figure = !it.alt_text.is_empty() || matches!(it.content, Content::Graphic(_)) || it.has_nested_items();
        if on_parent || it.export_options.artifact || (story.is_none() && !figure) {
            // Page furniture and decoration.
            let kind = if on_parent { ArtifactType::Page } else { ArtifactType::Other };
            s.start_tagged(ContentTag::Artifact(Artifact::new(kind, None)));
            self.item(s, it, parent, page_name);
            s.end_tagged();
            return;
        }
        if let Some(sid) = story {
            // Text runs tag themselves by paragraph; the rest of the frame is an artifact.
            if let std::collections::hash_map::Entry::Vacant(e) = self.story_tags.entry(sid) {
                e.insert(Vec::new());
                self.tags.push(TagEntry::Story(sid));
            }
            self.tag_story = Some(sid);
            self.item(s, it, parent, page_name);
            self.tag_story = None;
            return;
        }
        let id = s.start_tagged(ContentTag::Other);
        self.item(s, it, parent, page_name);
        s.end_tagged();
        let alt = (!it.alt_text.is_empty()).then(|| it.alt_text.clone());
        let mut g = TagGroup::new(Tag::Figure(alt));
        g.push(id);
        self.tags.push(TagEntry::Figure(g));
    }

    /// The structure tree: stories (one paragraph each, its frames in thread order of appearance)
    /// and figures, in the order they first appear.
    fn tag_tree(&mut self) -> TagTree {
        let mut tree = TagTree::new();
        for e in std::mem::take(&mut self.tags) {
            match e {
                TagEntry::Story(sid) => {
                    // One structure element per paragraph: headings and quotes from the
                    // paragraph style's Export Tagging, else P.
                    let mut paras: Vec<usize> = self.para_tags.keys().filter(|(s, _)| *s == sid).map(|(_, p)| *p).collect();
                    paras.sort_unstable();
                    let story = self.doc.story(sid);
                    for pi in paras {
                        let ids = self.para_tags.remove(&(sid, pi)).unwrap_or_default();
                        let style = story.and_then(|st| st.paras.get(pi)).map(|p| p.style.clone()).unwrap_or_default();
                        let tag = self.doc.styles.export_tag(&style, false).map(|e| e.tag.clone()).unwrap_or_default();
                        let text = || story.and_then(|st| st.para_ranges().get(pi).map(|r| st.text[r.clone()].trim().to_string()));
                        let kind: krilla::tagging::TagKind = match tag.as_str() {
                            h if h.len() == 2 && h.starts_with('h') => {
                                let n = h[1..].parse::<u16>().unwrap_or(1).clamp(1, 6);
                                Tag::Hn(std::num::NonZeroU16::new(n).unwrap_or(std::num::NonZeroU16::MIN), text()).into()
                            }
                            "blockquote" => Tag::BlockQuote.into(),
                            _ => Tag::P.into(),
                        };
                        let mut g = TagGroup::new(kind);
                        for id in ids {
                            g.push(id);
                        }
                        tree.push(g);
                    }
                }
                TagEntry::Figure(g) => tree.push(g),
            }
        }
        tree
    }

    fn item(&mut self, s: &mut Surface, it: &Item, parent: Affine, page_name: Option<&str>) {
        let gf = &it.effects.gradient_feather;
        if !gf.on || it.hidden || it.nonprinting {
            return self.item_inner(s, it, parent, page_name);
        }
        // Gradient feather: a luminosity mask (white = opaque) over the object.
        let xf = parent * it.xf;
        let (p0, p1) = gf.points(it.inner_bounds());
        let gray = |a: f32| -> krilla::color::Color { luma::Color::new((a.clamp(0.0, 1.0) * 255.0).round() as u8).into() };
        let stops = vec![
            Stop { offset: NormalizedF32::ZERO, color: gray(gf.start), opacity: NormalizedF32::ONE },
            Stop { offset: NormalizedF32::ONE, color: gray(gf.end), opacity: NormalizedF32::ONE },
        ];
        let paint: krilla::paint::Paint = if gf.radial {
            let r = (p1 - p0).hypot().max(1e-3) as f32;
            RadialGradient {
                fx: p0.x as f32,
                fy: p0.y as f32,
                fr: 0.0,
                cx: p0.x as f32,
                cy: p0.y as f32,
                cr: r,
                transform: Transform::identity(),
                spread_method: SpreadMethod::Pad,
                stops,
                anti_alias: false,
            }
            .into()
        } else {
            LinearGradient {
                x1: p0.x as f32,
                y1: p0.y as f32,
                x2: p1.x as f32,
                y2: p1.y as f32,
                transform: Transform::identity(),
                spread_method: SpreadMethod::Pad,
                stops,
                anti_alias: false,
            }
            .into()
        };
        let area = it.inner_bounds().inflate(it.stroke.extent() + 4.0, it.stroke.extent() + 4.0);
        let mut sb = s.stream_builder();
        let mut ms = sb.surface();
        ms.push_transform(&tf(xf));
        if let Some(p) = to_path(&area.to_path(0.1)) {
            ms.set_fill(Some(Fill { paint, opacity: NormalizedF32::ONE, rule: FillRule::NonZero }));
            ms.draw_path(&p);
        }
        ms.pop();
        ms.finish();
        let stream = sb.finish();
        s.push_mask(krilla::mask::Mask::new(stream, krilla::mask::MaskType::Luminosity));
        self.item_inner(s, it, parent, page_name);
        s.pop();
    }

    fn item_inner(&mut self, s: &mut Surface, it: &Item, parent: Affine, page_name: Option<&str>) {
        if it.hidden || it.nonprinting {
            return;
        }
        let ds = &it.effects.drop_shadow;
        let grow = it.stroke.extent() + 2.0 + if ds.on { ds.distance.abs() + ds.size.abs() } else { 0.0 };
        let vb = parent.transform_rect_bbox(it.bounds()).inflate(grow, grow);
        if !rects_overlap(vb, self.clip) {
            return;
        }
        let xf = parent * it.xf;
        if it.has_nested_items() {
            // A frame with items pasted into it: fill, the items clipped to the frame, stroke.
            let bp = if it.corners.is_none() { it.path.to_bezpath() } else { corners::apply(&it.path, &it.corners) };
            let path = to_path(&bp);
            let pushes = Self::push_group(s, it.opacity, it.blend);
            s.push_transform(&tf(xf));
            if !it.fill.is_none()
                && let Some(p) = &path
                && let Some(paint) = self.fill_paint(&it.fill, bp.bounding_box())
            {
                s.set_stroke(None);
                s.set_fill(Some(Fill { paint, opacity: NormalizedF32::ONE, rule: FillRule::NonZero }));
                s.draw_path(p);
                s.set_fill(None);
            }
            s.pop();
            // The clip in page space, so the children keep their absolute transforms.
            let mut page_bp = bp.clone();
            page_bp.apply_affine(xf);
            let clip = to_path(&page_bp);
            if let Some(c) = &clip {
                s.push_clip_path(c, &FillRule::NonZero);
            }
            for c in it.children() {
                self.item(s, c, xf, page_name);
            }
            if clip.is_some() {
                s.pop();
            }
            if !it.stroke.is_none()
                && let Some(p) = &path
            {
                s.push_transform(&tf(xf));
                self.stroke(s, it, &bp, p);
                s.pop();
            }
            for _ in 0..pushes {
                s.pop();
            }
            return;
        }
        if !it.children().is_empty() {
            let pushes = Self::push_group(s, it.opacity, it.blend) + Self::push_isolation(s, it);
            for c in it.shown_children() {
                self.item(s, c, xf, page_name);
            }
            for _ in 0..pushes {
                s.pop();
            }
            return;
        }
        let doc = self.doc;
        let bp = if it.corners.is_none() { it.path.to_bezpath() } else { corners::apply(&it.path, &it.corners) };
        let path = to_path(&bp);
        let pushes = Self::push_group(s, it.opacity, it.blend);
        let frame_art = self.tag_story.is_some();
        if frame_art {
            s.start_tagged(ContentTag::Artifact(Artifact::new(ArtifactType::Other, None)));
        }
        // Drop shadow (simple offset silhouette, like the renderer).
        if ds.on
            && let Some(p) = &path
            && let Some(c) = self.swatch_color(&ds.color, 1.0)
        {
            let a = self.doc.light_angle(ds.angle, ds.global_light).to_radians();
            let off = Vec2::new(-a.cos() * ds.distance, a.sin() * ds.distance);
            s.push_transform(&tf(Affine::translate(off) * xf));
            s.set_stroke(None);
            s.set_fill(Some(solid_fill(c, ds.opacity)));
            s.draw_path(p);
            s.set_fill(None);
            s.pop();
        }
        s.push_transform(&tf(xf));
        // Fill.
        if !it.fill.is_none()
            && it.path.is_closed()
            && let Some(p) = &path
            && let Some(paint) = self.fill_paint(&it.fill, bp.bounding_box())
        {
            if it.fill.overprint {
                self.warn("overprint is not exported yet");
            }
            s.set_stroke(None);
            s.set_fill(Some(Fill { paint, opacity: NormalizedF32::ONE, rule: FillRule::NonZero }));
            s.draw_path(p);
            s.set_fill(None);
        }
        if frame_art {
            s.end_tagged();
        }
        // Content.
        match &it.content {
            Content::Graphic(g) => {
                let g = &if it.media.is_some() { it.drawn_graphic(&self.doc.assets).unwrap_or_else(|| g.clone()) } else { g.clone() };
                if let Some(p) = &path {
                    s.push_clip_path(p, &FillRule::NonZero);
                    self.graphic(s, g);
                    s.pop();
                }
            }
            Content::Text(tfr) => {
                let cs = self.cache.get(doc, tfr.story, page_name);
                if let (Some(ft), Some(pt)) = (cs.frame(it.id), &tfr.options.path) {
                    // Type on a path: the glyphs as outlines along the path.
                    for (si, gp) in designcraft_compose::path_glyphs(&cs, ft, &bp, pt) {
                        let st = &cs.styles[si as usize];
                        if let (Some(c), Some(p)) = (self.swatch_color(&st.fill, st.fill_tint), to_path(&gp)) {
                            s.set_fill(Some(Fill { paint: c.into(), opacity: NormalizedF32::ONE, rule: FillRule::NonZero }));
                            s.draw_path(&p);
                            s.set_fill(None);
                        }
                    }
                } else if let Some(ft) = cs.frame(it.id) {
                    let text = doc.story(tfr.story).map(|st| st.text.as_str()).unwrap_or("");
                    let local = doc.text_local(it);
                    if local != Affine::IDENTITY {
                        s.push_transform(&tf(local));
                    }
                    self.frame_text(s, &cs, ft, text);
                    if local != Affine::IDENTITY {
                        s.pop();
                    }
                }
            }
            _ => {}
        }
        // Stroke.
        if !it.stroke.is_none()
            && let Some(p) = &path
        {
            if frame_art {
                s.start_tagged(ContentTag::Artifact(Artifact::new(ArtifactType::Other, None)));
            }
            self.stroke(s, it, &bp, p);
            if frame_art {
                s.end_tagged();
            }
        }
        s.pop();
        for _ in 0..pushes {
            s.pop();
        }
        // Anchored objects in the frame's text.
        if let Content::Text(tfr) = &it.content
            && let Some(st) = doc.story(tfr.story).filter(|st| !st.objects.is_empty())
        {
            let cs = self.cache.get(doc, tfr.story, page_name);
            if let Some(ft) = cs.frame(it.id) {
                for o in &ft.objects {
                    if let Some(obj) = st.objects.get(o.index) {
                        self.item(s, &obj.item, xf * Affine::translate(o.origin.to_vec2()), page_name);
                    }
                }
            }
        }
    }

    /// A swatch fill (solid or gradient) in the item's inner space. `None` for [None]/unknown.
    fn fill_paint(&mut self, fill: &designcraft_doc::Fill, bounds: Rect) -> Option<krilla::paint::Paint> {
        let (swatch, tint, angle) = (fill.swatch.as_str(), fill.tint, fill.gradient_angle);
        if let Some(g) = designcraft_color::swatch::resolve_gradient(&self.doc.swatches, swatch) {
            let kind = g.kind;
            let expanded = g.expanded_stops();
            // A PDF shading needs one colour space. Midpoint expansion can introduce RGB even
            // when every authored stop is CMYK/Gray; PDF/A can also change the output space.
            let components = |c: &Color| match c {
                Color::Cmyk { .. } if !self.rgb_only => 4,
                Color::Gray { .. } => 1,
                _ => 3,
            };
            let mixed = expanded.first().is_some_and(|(_, first, _)| expanded.iter().any(|(_, c, _)| components(c) != components(first)));
            if mixed {
                self.warn(format!("gradient '{swatch}': mixed color spaces converted to RGB (colour appearance and separations may change)"));
            }
            let mut stops: Vec<Stop> = Vec::new();
            let mut last = 0.0f32;
            for (o, c, a) in expanded {
                let o = o.clamp(last, 1.0);
                last = o;
                let color = if mixed && !matches!(c, Color::Rgb { .. }) {
                    let [r, g, b] = c.to_rgb();
                    rgb::Color::new(q(r), q(g), q(b)).into()
                } else {
                    // Preserve RGB values, including already-interpolated display RGB, and
                    // leave every homogeneous gradient on its existing device-space path.
                    device(&c, self.rgb_only)
                };
                stops.push(Stop { offset: norm(o), color, opacity: norm(a) });
            }
            if stops.is_empty() {
                return None;
            }
            let c = bounds.center();
            let v = fill.gradient_vector;
            return Some(match kind {
                GradientKind::Radial => {
                    let (c, r) = match v {
                        Some([x0, y0, x1, y1]) => (designcraft_geom::Point::new(x0, y0), Vec2::new(x1 - x0, y1 - y0).hypot()),
                        None => (c, bounds.width().max(bounds.height()) / 2.0),
                    };
                    let r = r.max(1e-3) as f32;
                    let (cx, cy) = (c.x as f32, c.y as f32);
                    RadialGradient {
                        fx: cx,
                        fy: cy,
                        fr: 0.0,
                        cx,
                        cy,
                        cr: r,
                        transform: Transform::identity(),
                        spread_method: SpreadMethod::Pad,
                        stops,
                        anti_alias: false,
                    }
                    .into()
                }
                _ => {
                    let a = angle.unwrap_or(0.0).to_radians();
                    let half = ((bounds.width() * a.cos().abs() + bounds.height() * a.sin().abs()) / 2.0).max(1e-3);
                    let d = Vec2::new(a.cos(), -a.sin()) * half;
                    let (p0, p1) = match v {
                        Some([x0, y0, x1, y1]) => (designcraft_geom::Point::new(x0, y0), designcraft_geom::Point::new(x1, y1)),
                        None => (c - d, c + d),
                    };
                    LinearGradient {
                        x1: p0.x as f32,
                        y1: p0.y as f32,
                        x2: p1.x as f32,
                        y2: p1.y as f32,
                        transform: Transform::identity(),
                        spread_method: SpreadMethod::Pad,
                        stops,
                        anti_alias: false,
                    }
                    .into()
                }
            });
        }
        self.swatch_color(swatch, tint).map(Into::into)
    }

    fn stroke(&mut self, s: &mut Surface, it: &Item, bp: &BezPath, path: &Path) {
        let st = &it.stroke;
        let Some(c) = self.swatch_color(&st.swatch, st.tint) else { return };
        let closed = it.path.is_closed();
        let mut cap = match st.cap {
            designcraft_doc::Cap::Butt => krilla::paint::LineCap::Butt,
            designcraft_doc::Cap::Round => krilla::paint::LineCap::Round,
            designcraft_doc::Cap::Projecting => krilla::paint::LineCap::Square,
        };
        let kind = self.doc.stroke_kind(&st.kind);
        // Stripes, wavy and hash strokes: a filled outline (gap colour under it).
        if let Some(o) = kind.outline(bp, st.weight, 0.05)
            && let Some(op) = to_path(&o)
        {
            s.set_stroke(None);
            if let Some(g) = self.swatch_color(&st.gap_swatch, st.gap_tint)
                && st.gap_swatch != designcraft_color::swatch::NONE
            {
                s.set_stroke(Some(Stroke { paint: g.into(), width: st.weight as f32, opacity: NormalizedF32::ONE, ..Default::default() }));
                s.set_fill(None);
                s.draw_path(path);
                s.set_stroke(None);
            }
            s.set_fill(Some(Fill { paint: c.into(), opacity: NormalizedF32::ONE, rule: FillRule::NonZero }));
            s.draw_path(&op);
            s.set_fill(None);
            return;
        }
        let dash = match &kind {
            StrokeType::Dashed { pattern } if pattern.iter().any(|v| *v > 0.0) => {
                let mut pat: Vec<f32> = pattern.iter().map(|v| v.max(0.0) as f32).collect();
                if pat.len() % 2 == 1 {
                    pat.extend(pat.clone());
                }
                Some(StrokeDash { array: pat, offset: 0.0 })
            }
            StrokeType::Dotted => {
                cap = krilla::paint::LineCap::Round;
                Some(StrokeDash { array: vec![0.0, (st.weight * 2.0) as f32], offset: 0.0 })
            }
            _ => None,
        };
        let arrows = designcraft_doc::arrow::apply(bp, st, closed);
        let trimmed = arrows.as_ref().and_then(|a| to_path(&a.0));
        let path = trimmed.as_ref().unwrap_or(path);
        let aligned = closed && st.align != StrokeAlign::Center;
        let width = if aligned { st.weight * 2.0 } else { st.weight };
        let mut pushes = 0;
        match st.align {
            StrokeAlign::Inside if closed => {
                s.push_clip_path(path, &FillRule::NonZero);
                pushes += 1;
            }
            StrokeAlign::Outside if closed => {
                // Clip to everything outside the path: a big frame plus the path, even-odd.
                let b = bp.bounding_box().inflate(width * 2.0 + 10.0, width * 2.0 + 10.0);
                let mut outside = b.to_path(0.1);
                outside.extend(bp.iter());
                if let Some(p) = to_path(&outside) {
                    s.push_clip_path(&p, &FillRule::EvenOdd);
                    pushes += 1;
                }
            }
            _ => {}
        }
        s.set_fill(None);
        if dash.is_some()
            && pushes == 0
            && let Some(g) = self.swatch_color(&st.gap_swatch, st.gap_tint)
        {
            // Gap colour under dashes and dots.
            s.set_stroke(Some(Stroke { paint: g.into(), width: width as f32, opacity: NormalizedF32::ONE, ..Default::default() }));
            s.draw_path(path);
        }
        s.set_stroke(Some(Stroke {
            paint: c.clone().into(),
            width: width as f32,
            miter_limit: st.miter_limit.max(1.0) as f32,
            line_cap: cap,
            line_join: match st.join {
                designcraft_doc::Join::Miter => krilla::paint::LineJoin::Miter,
                designcraft_doc::Join::Round => krilla::paint::LineJoin::Round,
                designcraft_doc::Join::Bevel => krilla::paint::LineJoin::Bevel,
            },
            opacity: NormalizedF32::ONE,
            dash,
        }));
        s.draw_path(path);
        s.set_stroke(None);
        for _ in 0..pushes {
            s.pop();
        }
        for h in arrows.iter().flat_map(|a| &a.1) {
            let Some(p) = to_path(&h.path) else { continue };
            match h.outline {
                Some(w) => {
                    s.set_stroke(Some(Stroke { paint: c.clone().into(), width: w as f32, opacity: NormalizedF32::ONE, ..Default::default() }));
                    s.draw_path(&p);
                    s.set_stroke(None);
                }
                None => {
                    s.set_fill(Some(krilla::paint::Fill { paint: c.clone().into(), opacity: NormalizedF32::ONE, rule: FillRule::NonZero }));
                    s.draw_path(&p);
                    s.set_fill(None);
                }
            }
        }
    }

    /// A placed PDF as krilla embeds it. krilla refuses a PDF newer than the file it writes (the
    /// export fails at the end with `VersionMismatch(Pdf17)`), and PDF/X-4 is PDF 1.6 while
    /// VectorCraft and most tools write 1.7. PDF 1.7 changed nothing in how a page draws, so a 1.7
    /// file goes in as 1.6 (its header relabelled, as `qpdf --force-version=1.6` does). A newer
    /// file is left out and the export fails naming it ([`Exporter::check_placed_versions`]).
    fn placed_pdf(&mut self, asset: &designcraft_doc::Asset) -> Option<krilla::pdf::PdfDocument> {
        use hayro_syntax::PdfVersion as V;
        let Ok(pdf) = krilla::pdf::Pdf::new(asset.data.clone()) else {
            self.warn(format!("{}: can't read the placed PDF", asset.name));
            return None;
        };
        let max = match self.version {
            PdfVersion::Pdf14 => V::Pdf14,
            PdfVersion::Pdf15 => V::Pdf15,
            PdfVersion::Pdf16 => V::Pdf16,
            PdfVersion::Pdf17 => V::Pdf17,
            PdfVersion::Pdf20 => V::Pdf20,
        };
        let have = pdf.version();
        if have <= max {
            return Some(krilla::pdf::PdfDocument::new(Arc::new(pdf)));
        }
        if have == V::Pdf17
            && max == V::Pdf16
            && let Some(bytes) = relabel_pdf_header(&asset.data, b"%PDF-1.7", b"%PDF-1.6")
            && let Ok(pdf) = krilla::pdf::Pdf::new(Arc::new(bytes))
            && pdf.version() <= max
        {
            self.warn(format!("{}: a PDF 1.7 placed in a PDF 1.6 file, embedded as PDF 1.6", asset.name));
            return Some(krilla::pdf::PdfDocument::new(Arc::new(pdf)));
        }
        let v = match have {
            V::Pdf17 => "PDF 1.7",
            V::Pdf20 => "PDF 2.0",
            _ => "a newer PDF",
        };
        self.too_new.push(format!("{} is {v}", asset.name));
        None
    }

    /// Fail with the names of placed PDFs too new for this export (see [`Exporter::placed_pdf`]).
    fn check_placed_versions(&self) -> Result<()> {
        if self.too_new.is_empty() {
            return Ok(());
        }
        let v = self.version.as_str();
        Err(PdfError::Write(format!(
            "a placed PDF is newer than the {v} this export writes ({}): save it as {v} or older and place it again",
            self.too_new.join(", ")
        )))
    }

    fn load_image(&mut self, id: AssetId) -> Option<Image> {
        if let Some(i) = self.images.get(&id) {
            return i.clone();
        }
        let asset = self.doc.assets.get(&id)?.clone();
        let data = asset.data.clone();
        let fmt = image::guess_format(&data).ok();
        let img = match fmt {
            Some(image::ImageFormat::Jpeg) => Image::from_jpeg(data.clone().into(), self.interpolate).ok(),
            // A CMYK TIFF keeps its ink values (`image` decodes it to RGB, and 100% K would print
            // as four-colour black). PDF/A exports are RGB only.
            Some(image::ImageFormat::Tiff) if !self.rgb_only && designcraft_images::is_cmyk_tiff(&data) => match cmyk_tiff(&data, self.interpolate) {
                Some(img) => Some(img),
                None => {
                    self.warn(format!(
                        "{}: converted to RGB (a CMYK TIFF that is planar, has premultiplied alpha, uses other inks or is over 256 MiB)",
                        asset.name
                    ));
                    lossless(&data, fmt, self.interpolate)
                }
            },
            _ if self.opts.compress_images => recompress(&data, self.interpolate).or_else(|| lossless(&data, fmt, self.interpolate)),
            _ => lossless(&data, fmt, self.interpolate),
        };
        if img.is_none() {
            self.warn(format!("image `{}` could not be decoded and was skipped", asset.name));
        }
        self.images.insert(id, img.clone());
        img
    }

    pub(crate) fn graphic(&mut self, s: &mut Surface, g: &designcraft_doc::Graphic) {
        // Video / sound without a poster: a dark frame with a play mark.
        if let Some(asset) = self.doc.assets.get(&g.asset)
            && designcraft_doc::media_kind(&asset.mime).is_some()
        {
            let (w, h) = g.size;
            let (cx, cy, r) = (w / 2.0, h / 2.0, w.min(h) * 0.18);
            let mut tri = BezPath::new();
            tri.move_to((cx - r * 0.6, cy - r));
            tri.line_to((cx + r, cy));
            tri.line_to((cx - r * 0.6, cy + r));
            tri.close_path();
            s.push_transform(&tf(g.xf));
            s.set_stroke(None);
            for (bp, c) in [(Rect::new(0.0, 0.0, w, h).to_path(0.1), rgb::Color::new(48, 48, 52)), (tri, rgb::Color::new(235, 235, 235))] {
                if let Some(p) = to_path(&bp) {
                    s.set_fill(Some(Fill { paint: c.into(), ..Default::default() }));
                    s.draw_path(&p);
                }
            }
            s.set_fill(None);
            s.pop();
            return;
        }
        // Placed PDFs go in as vectors (the page as a form XObject).
        let doc = self.doc;
        if let Some(asset) = doc.assets.get(&g.asset)
            && asset.data.starts_with(b"%PDF")
        {
            let Some(size) = Size::from_wh(g.size.0.max(1e-3) as f32, g.size.1.max(1e-3) as f32) else { return };
            let placed = match self.pdfs.get(&g.asset) {
                Some(p) => p.clone(),
                None => {
                    let p = self.placed_pdf(asset);
                    self.pdfs.insert(g.asset, p.clone());
                    p
                }
            };
            if let Some(placed) = placed {
                s.push_transform(&tf(g.xf));
                s.draw_pdf_page(&placed, size, asset.page as usize);
                s.pop();
            }
            return;
        }
        // Placed SVGs go in as vectors too.
        if let Some(asset) = self.doc.assets.get(&g.asset)
            && designcraft_images::is_svg(&asset.data)
        {
            let Some(size) = Size::from_wh(g.size.0.max(1e-3) as f32, g.size.1.max(1e-3) as f32) else { return };
            let tree = self.svgs.entry(g.asset).or_insert_with(|| designcraft_images::svg_tree(&asset.data).map(Arc::new)).clone();
            match tree {
                Some(tree) => {
                    use krilla_svg::SurfaceExt;
                    s.push_transform(&tf(g.xf));
                    if s.draw_svg(&tree, size, krilla_svg::SvgSettings::default()).is_none() {
                        self.warn(format!("{}: parts of the placed SVG could not be drawn", asset.name));
                    }
                    s.pop();
                }
                None => self.warn(format!("{}: can't read the placed SVG", asset.name)),
            }
            return;
        }
        let Some(img) = self.load_image(g.asset) else { return };
        let Some(size) = Size::from_wh(g.size.0.max(1e-3) as f32, g.size.1.max(1e-3) as f32) else { return };
        s.push_transform(&tf(g.xf));
        s.draw_image(img, size);
        s.pop();
    }
}

/// `data` with its `%PDF-x.y` header (within the first 1024 bytes, where readers look for it)
/// changed from `from` to `to`, both the same length. `None` when the header isn't `from`.
fn relabel_pdf_header(data: &[u8], from: &[u8], to: &[u8]) -> Option<Vec<u8>> {
    if from.len() != to.len() {
        return None;
    }
    let head = data.get(..data.len().min(1024))?;
    let at = head.windows(from.len()).position(|w| w == from)?;
    let mut out = data.to_vec();
    out.get_mut(at..at + to.len())?.copy_from_slice(to);
    Some(out)
}

fn lossless(data: &Arc<Vec<u8>>, fmt: Option<image::ImageFormat>, interpolate: bool) -> Option<Image> {
    // krilla reads PNG, GIF and WebP lazily: a damaged file is accepted here and fails the whole
    // export when the PDF is written. Decoding it first means it is skipped, like any other
    // image that can't be decoded.
    let rgba = designcraft_images::decode_rgba(data)?;
    let direct = match fmt {
        Some(image::ImageFormat::Png) => Image::from_png(data.clone().into(), interpolate).ok(),
        Some(image::ImageFormat::Gif) => Image::from_gif(data.clone().into(), interpolate).ok(),
        Some(image::ImageFormat::WebP) => Image::from_webp(data.clone().into(), interpolate).ok(),
        _ => None,
    };
    direct.or_else(|| {
        // TIFF, BMP, PSD composites…
        let (w, h) = rgba.dimensions();
        Some(Image::from_rgba8(rgba.into_raw(), w, h))
    })
}

/// A CMYK raster as a krilla image: its samples are written unchanged as DeviceCMYK, like a
/// CMYK JPEG's. An embedded ICC profile is left out, as it is for JPEGs: the inks are meant for
/// the output intent's press, and tagging them with another CMYK profile would let a RIP
/// convert 100% K to four-colour black.
#[derive(Clone, Hash)]
struct CmykImage(Arc<designcraft_images::CmykRaster>);

impl krilla::image::CustomImage for CmykImage {
    fn color_channel(&self) -> &[u8] {
        &self.0.cmyk
    }
    fn alpha_channel(&self) -> Option<&[u8]> {
        self.0.alpha.as_deref()
    }
    fn bits_per_component(&self) -> krilla::image::BitsPerComponent {
        krilla::image::BitsPerComponent::Eight
    }
    fn size(&self) -> (u32, u32) {
        (self.0.width, self.0.height)
    }
    fn icc_profile(&self) -> Option<&[u8]> {
        None
    }
    fn color_space(&self) -> krilla::image::ImageColorspace {
        krilla::image::ImageColorspace::Cmyk
    }
}

/// A CMYK TIFF as a DeviceCMYK image. `None` when it can't be read as CMYK.
fn cmyk_tiff(data: &[u8], interpolate: bool) -> Option<Image> {
    let r = designcraft_images::decode_cmyk_tiff(data)?;
    // krilla panics when the channel lengths don't match the size.
    let pixels = usize::try_from(r.width).ok()?.checked_mul(usize::try_from(r.height).ok()?)?;
    if r.cmyk.len() != pixels.checked_mul(4)? || r.alpha.as_ref().is_some_and(|a| a.len() != pixels) {
        return None;
    }
    Image::from_custom(CmykImage(Arc::new(r)), interpolate).ok()
}

/// Opaque raster → JPEG (quality 90). `None` when the image has transparency or can't be decoded.
fn recompress(data: &[u8], interpolate: bool) -> Option<Image> {
    let img = image::load_from_memory(data).ok()?;
    if img.color().has_alpha() && img.to_rgba8().pixels().any(|p| p[3] < 255) {
        return None;
    }
    let rgb = img.to_rgb8();
    let mut buf = Vec::new();
    let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, 90);
    image::ImageEncoder::write_image(enc, rgb.as_raw(), rgb.width(), rgb.height(), image::ExtendedColorType::Rgb8).ok()?;
    Image::from_jpeg(buf.into(), interpolate).ok()
}
