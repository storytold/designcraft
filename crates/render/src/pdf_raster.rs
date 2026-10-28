use super::types::Rendered;
use super::*;

/// MIME type guess for encoded image bytes.
/// Is this a PDF file (placed PDFs are graphics)?
pub fn is_pdf(bytes: &[u8]) -> bool {
    bytes.starts_with(b"%PDF")
}

/// Page count of a PDF.
pub fn pdf_page_count(bytes: &[u8]) -> Option<usize> {
    let pdf = craft_pdf::open_slice(bytes, None).ok()?;
    Some(pdf.pages().len())
}

/// Size of PDF page `page` (0-based), in points, as it shows (crop box, rotation applied).
pub fn pdf_page_size(bytes: &[u8], page: usize) -> Option<(f64, f64)> {
    let pdf = craft_pdf::open_slice(bytes, None).ok()?;
    let p = pdf.pages().get(page)?;
    let (w, h) = p.render_dimensions();
    Some((w as f64, h as f64))
}

/// A placed page could not be rasterized completely for export.
#[derive(Debug, thiserror::Error)]
pub enum PdfRasterError {
    #[error("{0}")]
    Input(#[source] craft_pdf::OpenError),
    #[error("{0}")]
    Render(#[from] craft_pdf::raster::Error),
    #[error("the PDF page could not be read or rendered")]
    InvalidPage,
    #[error("PDF page {page} is unavailable; choose an existing page in Image Import Options or place the replacement PDF again")]
    MissingPage { page: usize },
    #[error("the selected PDF layers could not be hidden")]
    LayerRewrite,
    #[error("an image could not be decoded or exceeded the PDF renderer's resource limits; repair or replace this placed PDF before exporting again")]
    ImageDecode,
    #[error("the PDF renderer encountered an unsupported font")]
    UnsupportedFont,
    #[error(
        "the PDF renderer could not read all page content or reached its resource limits; repair or replace this placed PDF before exporting again"
    )]
    ContentTruncated,
}

/// Reject oversized PDFs before placement, layer rewriting or output allocation.
pub fn check_pdf_input_size(size: usize) -> Result<(), PdfRasterError> {
    craft_pdf::check_input_size(size).map_err(PdfRasterError::Input)
}

/// PDF page `page` as PNG, its longer side about `max_side` pixels, with the `hidden` layers off.
/// Only the `shown` part ([`designcraft_doc::Asset::shown_box`]) is kept when given.
/// Returns `None` on the same failures as [`pdf_page_png_checked`].
pub fn pdf_page_png(bytes: &[u8], page: usize, max_side: u32, hidden: &[String], shown: Option<[f64; 4]>) -> Option<Vec<u8>> {
    pdf_page_png_checked(bytes, page, max_side, hidden, shown).ok()
}

/// Rasterize a placed page for export, preserving available interpreter diagnostics.
/// This cannot detect backend approximations that do not emit an interpreter warning.
pub fn pdf_page_png_checked(bytes: &[u8], page: usize, max_side: u32, hidden: &[String], shown: Option<[f64; 4]>) -> Result<Vec<u8>, PdfRasterError> {
    pdf_page_png_options_checked(bytes, page, max_side, (!hidden.is_empty()).then_some(hidden), shown)
}

/// Rasterize with an explicit visibility override: `Some(&[])` enables every layer;
/// `None` retains file defaults. The legacy entry point keeps its empty-list semantics.
pub fn pdf_page_png_options_checked(
    bytes: &[u8],
    page: usize,
    max_side: u32,
    hidden: Option<&[String]>,
    shown: Option<[f64; 4]>,
) -> Result<Vec<u8>, PdfRasterError> {
    check_pdf_input_size(bytes.len())?;
    let data = if let Some(hidden) = hidden {
        std::borrow::Cow::Owned(pdf_hide_layers(bytes, hidden).ok_or(PdfRasterError::LayerRewrite)?)
    } else {
        std::borrow::Cow::Borrowed(bytes)
    };
    let raster = render_pdf_raster(&data, page, max_side)?;
    if raster.diagnostics.image_decode_failed {
        return Err(PdfRasterError::ImageDecode);
    }
    if raster.diagnostics.unsupported_font {
        return Err(PdfRasterError::UnsupportedFont);
    }
    if raster.diagnostics.content_truncated {
        return Err(PdfRasterError::ContentTruncated);
    }
    let pm = raster_pixmap(&raster);
    let pm = match shown {
        Some(frac) => crop_pixmap(&pm, frac).ok_or(PdfRasterError::InvalidPage)?,
        None => pm,
    };
    let pixels = pm.data().iter().flat_map(|p| [p.r, p.g, p.b, p.a]).collect();
    Ok(Rendered { width: u32::from(pm.width()), height: u32::from(pm.height()), pixels }.to_png())
}

/// Rasterize PDF page `page` so its longer side is about `max_side` pixels (screen display).
/// Export callers that need failure diagnostics use [`pdf_page_png_checked`].
pub fn render_pdf_page(bytes: &[u8], page: usize, max_side: u32) -> Option<Pixmap> {
    #[cfg(test)]
    PDF_RENDERS.with(|n| n.set(n.get() + 1));
    let raster = render_pdf_raster(bytes, page, max_side).ok()?;
    Some(raster_pixmap(&raster))
}

fn raster_pixmap(raster: &craft_pdf::raster::Raster) -> Pixmap {
    let data = raster.bytes().as_chunks::<4>().0.iter().map(|c| vello_cpu::color::PremulRgba8 { r: c[0], g: c[1], b: c[2], a: c[3] }).collect();
    Pixmap::from_parts(data, raster.width as u16, raster.height as u16)
}

#[cfg(test)]
thread_local! {
    /// How many PDF pages [`render_pdf_page`] rasterized on this thread.
    pub(crate) static PDF_RENDERS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn render_pdf_raster(bytes: &[u8], page: usize, max_side: u32) -> Result<craft_pdf::raster::Raster, PdfRasterError> {
    let pdf = craft_pdf::open_slice(bytes, None).map_err(|e| match e {
        craft_pdf::OpenError::Load(_) => PdfRasterError::InvalidPage,
        other => PdfRasterError::Input(other),
    })?;
    let p = pdf.pages().get(page).ok_or(PdfRasterError::MissingPage { page: page.saturating_add(1) })?;
    craft_pdf::raster::render(
        p,
        &craft_pdf::raster::RenderCache::new(),
        &craft_pdf::interpret::InterpreterSettings::default(),
        &pdf_screen_options(p, max_side),
    )
    .map_err(PdfRasterError::Render)
}

/// A page box of a PDF page (`crop`, `trim`, `bleed`, `art`, `media`) as (x, y, w, h) in points
/// from the top-left of the page as rendered (its visible crop box); missing boxes fall back to the
/// crop box, as PDF readers do.
pub fn pdf_page_box(bytes: &[u8], page: usize, kind: &str) -> Option<(f64, f64, f64, f64)> {
    use craft_pdf::syntax::object::{Rect as PRect, dict::keys};
    let pdf = craft_pdf::open_slice(bytes, None).ok()?;
    let p = pdf.pages().get(page)?;
    let visible = p.intersected_crop_box();
    let page_box = match kind {
        "media" => p.media_box(),
        "trim" | "bleed" | "art" => {
            let key: &[u8] = match kind {
                "trim" => keys::TRIM_BOX,
                "bleed" => keys::BLEED_BOX,
                _ => keys::ART_BOX,
            };
            p.raw().get::<PRect>(key).map(|b| b.intersect(p.media_box())).unwrap_or(visible)
        }
        _ => visible,
    };
    let transform = Affine::new(p.initial_transform(true).as_coeffs());
    let b = transform.transform_rect_bbox(Rect::new(page_box.x0, page_box.y0, page_box.x1, page_box.y1));
    b.is_finite().then_some((b.x0, b.y0, b.width(), b.height()))
}

/// Use the same sizing contract as the PDF rasterizer without rasterizing the page. Source
/// bytes are shared with the document; page points must not be mistaken for screen pixels.
pub(crate) fn pdf_raster_size(bytes: &Arc<Vec<u8>>, page: u32) -> Option<(u32, u32)> {
    let pdf = craft_pdf::open(bytes.clone(), None).ok()?;
    let page = pdf.pages().get(page as usize)?;
    let [w, h] = craft_pdf::raster::dimensions(page, &pdf_screen_options(page, 3000)).ok()?;
    Some((w, h))
}

fn pdf_screen_options(page: &craft_pdf::syntax::page::Page<'_>, max_side: u32) -> craft_pdf::raster::Options {
    let (w, h) = page.render_dimensions();
    let scale = (max_side as f32 / w.max(h).max(1.0)).min(8.0);
    craft_pdf::raster::Options { scale: [scale; 2], ..Default::default() }
}

/// The part of PDF page `page` that `crop` shows: its left, top, right and bottom edges as
/// fractions of the page as rendered (its visible crop box, rotated as it displays), and its size
/// in points. A box the page doesn't define is its crop box, as PDF readers do. The content boxes
/// are the bounds of what the page draws, found from its drawing operations without rendering it
/// (the whole page when it draws nothing).
pub fn pdf_crop_box(bytes: &[u8], page: usize, crop: designcraft_doc::PdfCrop) -> Option<([f64; 4], (f64, f64))> {
    use craft_pdf::syntax::object::{Rect as PRect, dict::keys};
    use designcraft_doc::PdfCrop;
    const WHOLE: [f64; 4] = [0.0, 0.0, 1.0, 1.0];
    let pdf = craft_pdf::open_slice(bytes, None).ok()?;
    let p = pdf.pages().get(page)?;
    let (w, h) = p.render_dimensions();
    let (w, h) = (f64::from(w), f64::from(h));
    if !(w > 0.0 && h > 0.0) {
        return None;
    }
    let frac = match crop {
        PdfCrop::Crop => WHOLE,
        PdfCrop::ContentVisible => pdf_bounds::page_content_bounds(bytes, page).unwrap_or(WHOLE),
        PdfCrop::ContentAll => {
            if pdf_layers::has_optional_content(&pdf) {
                // If all-on rewriting is unsupported, a visible-only box could cut
                // off hidden content. Keep the entire page conservatively instead.
                pdf_hide_layers(bytes, &[]).and_then(|all_on| pdf_bounds::page_content_bounds(&all_on, page)).unwrap_or(WHOLE)
            } else {
                pdf_bounds::page_content_bounds(bytes, page).unwrap_or(WHOLE)
            }
        }
        PdfCrop::Art | PdfCrop::Trim | PdfCrop::Bleed | PdfCrop::Media => {
            let media = p.media_box();
            let b = match crop {
                PdfCrop::Media => media,
                _ => {
                    let key: &[u8] = match crop {
                        PdfCrop::Trim => keys::TRIM_BOX,
                        PdfCrop::Bleed => keys::BLEED_BOX,
                        _ => keys::ART_BOX,
                    };
                    p.raw().get::<PRect>(key).map(|b| b.intersect(media)).unwrap_or_else(|| p.intersected_crop_box())
                }
            };
            // Into rendered page space (y down, rotated, origin at the crop box).
            let r = Affine::new(p.initial_transform(true).as_coeffs()).transform_rect_bbox(Rect::new(b.x0, b.y0, b.x1, b.y1));
            [r.x0 / w, r.y0 / h, r.x1 / w, r.y1 / h]
        }
    };
    let size = ((frac[2] - frac[0]) * w, (frac[3] - frac[1]) * h);
    (frac.iter().all(|v| v.is_finite()) && size.0 > 0.0 && size.1 > 0.0).then_some((frac, size))
}

/// The part of the page a placed PDF shows ([`designcraft_doc::Asset::shown_box`]). A Bounding
/// Box crop that import left unmeasured is measured here, once per file, page and crop: that
/// interprets the page's drawing, which only drawing or exporting the graphic needs.
pub fn shown_box(asset: &designcraft_doc::Asset) -> Option<[f64; 4]> {
    if !asset.content_box_pending() {
        return asset.shown_box();
    }
    asset.shown(content_box(&asset.data, asset.page, asset.pdf_crop)?)
}

type ContentBoxes = HashMap<(usize, u32, designcraft_doc::PdfCrop), (std::sync::Weak<Vec<u8>>, Option<[f64; 4]>)>;

/// [`pdf_crop_box`]'s box, cached by the file's allocation. The weak reference keeps that address
/// from naming another file while the entry lives.
fn content_box(bytes: &Arc<Vec<u8>>, page: u32, crop: designcraft_doc::PdfCrop) -> Option<[f64; 4]> {
    static CACHE: std::sync::Mutex<Option<ContentBoxes>> = std::sync::Mutex::new(None);
    let key = (Arc::as_ptr(bytes) as usize, page, crop);
    let cached = CACHE.lock().unwrap_or_else(std::sync::PoisonError::into_inner).as_ref().and_then(|c| c.get(&key)).and_then(|(file, b)| {
        let same = file.upgrade().is_some_and(|f| Arc::ptr_eq(&f, bytes));
        same.then_some(*b)
    });
    if let Some(b) = cached {
        return b;
    }
    // Render outside the lock: other graphics keep drawing meanwhile.
    let b = pdf_crop_box(bytes, page as usize, crop).map(|(frac, _)| frac);
    let mut cache = CACHE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let cache = cache.get_or_insert_with(HashMap::new);
    cache.retain(|_, (file, _)| file.strong_count() > 0);
    cache.insert(key, (Arc::downgrade(bytes), b));
    b
}

/// The part of `pm` inside `frac` (left, top, right, bottom as fractions of it); what lies outside
/// `pm` is transparent.
pub fn crop_pixmap(pm: &Pixmap, frac: [f64; 4]) -> Option<Pixmap> {
    let (pw, ph) = (f64::from(pm.width()), f64::from(pm.height()));
    let [l, t, r, b] = [frac[0] * pw, frac[1] * ph, frac[2] * pw, frac[3] * ph].map(f64::round);
    let (w, h) = (r - l, b - t);
    if !(w >= 1.0 && h >= 1.0 && w <= f64::from(u16::MAX) && h <= f64::from(u16::MAX)) {
        return None;
    }
    let (w, h) = (w as u16, h as u16);
    let mut out = Pixmap::new(w, h);
    let src = pm.data();
    let dst = out.data_mut();
    for y in 0..usize::from(h) {
        let sy = y as f64 + t;
        if sy < 0.0 || sy >= ph {
            continue;
        }
        for x in 0..usize::from(w) {
            let sx = x as f64 + l;
            if sx < 0.0 || sx >= pw {
                continue;
            }
            let (Some(p), Some(q)) = (src.get(sy as usize * usize::from(pm.width()) + sx as usize), dst.get_mut(y * usize::from(w) + x)) else {
                continue;
            };
            *q = *p;
        }
    }
    Some(out)
}
