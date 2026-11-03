use super::common::{device, tf, to_path};
use super::images::{CMYK_FALLBACK_WARNING, ConvertedImage, RasterMode, cmyk_tiff, embedded_icc, jpeg, lossless, recompress};
use super::pipeline::{relabel_catalog_version, relabel_pdf_header};
use super::*;

impl Exporter<'_> {
    /// A placed PDF as krilla embeds it. krilla refuses a PDF newer than the file it writes (the
    /// export fails at the end with `VersionMismatch(Pdf17)`), and PDF/X-4 is PDF 1.6 while
    /// VectorCraft and most tools write 1.7. PDF 1.7 changed nothing in how a page draws, so a 1.7
    /// file goes in as 1.6 (its header and catalog `/Version` relabelled, as
    /// `qpdf --force-version=1.6` does). A newer
    /// file is left out and the export fails naming it ([`Exporter::check_placed_versions`]).
    /// Returns the document with its page count.
    fn placed_pdf(&mut self, asset: &designcraft_doc::Asset, data: Arc<Vec<u8>>) -> Option<(krilla::pdf::PdfDocument, usize)> {
        use hayro_syntax::PdfVersion as V;
        let Ok(pdf) = krilla::pdf::Pdf::new(data.clone()) else {
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
            let pages = pdf.pages().len();
            return Some((krilla::pdf::PdfDocument::new(Arc::new(pdf)), pages));
        }
        if have == V::Pdf17
            && max == V::Pdf16
            && let bytes = relabel_pdf_header(&data, b"%PDF-1.7", b"%PDF-1.6").unwrap_or_else(|| data.to_vec())
            && let Ok(pdf) = krilla::pdf::Pdf::new(Arc::new(relabel_catalog_version(bytes)))
            && pdf.version() <= max
        {
            self.warn(format!("{}: a PDF 1.7 placed in a PDF 1.6 file, embedded as PDF 1.6", asset.name));
            let pages = pdf.pages().len();
            return Some((krilla::pdf::PdfDocument::new(Arc::new(pdf)), pages));
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
    pub(super) fn check_placed_versions(&self) -> Result<()> {
        if self.too_new.is_empty() {
            return Ok(());
        }
        let v = self.version.as_str();
        Err(PdfError::Write(format!(
            "a placed PDF is newer than the {v} this export writes ({}): save it as {v} or older and place it again",
            self.too_new.join(", ")
        )))
    }

    pub(super) fn load_image(&mut self, id: AssetId) -> Option<Image> {
        if let Some(i) = self.images.get(&id) {
            return i.clone();
        }
        let asset = self.doc.assets.get(&id)?.clone();
        // Use the same EPS preview as the renderer, preserving CMYK TIFF inks below. The
        // content cache is keyed on the bytes actually converted, so equal previews share one.
        let data = if designcraft_images::is_eps(&asset.data) {
            let Some((proxy, _)) = designcraft_images::eps_proxy(&asset.data) else {
                self.warn(format!("image `{}` could not be decoded and was skipped", asset.name));
                self.images.insert(id, None);
                return None;
            };
            Arc::new(proxy)
        } else {
            asset.data.clone()
        };
        if let Some(converted) = self.images_by_content.get(&data).cloned() {
            if let Some(warning) = converted.warning {
                self.warn(format!("{}: {warning}", asset.name));
            }
            self.images.insert(id, Some(converted.image.clone()));
            return Some(converted.image);
        }
        let fmt = image::guess_format(&data).ok();
        #[cfg(test)]
        {
            self.image_conversions += 1;
        }
        let mut warning = None;
        let mode = RasterMode::for_standard(self.opts.standard);
        let img = match fmt {
            Some(image::ImageFormat::Jpeg) => jpeg(data.clone().into(), || embedded_icc(&data), mode),
            // A CMYK TIFF keeps its ink values (`image` decodes it to RGB, and 100% K would print
            // as four-colour black). PDF/A exports are RGB only.
            Some(image::ImageFormat::Tiff) if !self.rgb_only && designcraft_images::is_cmyk_tiff(&data) => match cmyk_tiff(&data, mode) {
                Some(img) => Some(img),
                None => {
                    warning = Some(CMYK_FALLBACK_WARNING);
                    self.warn(format!("{}: {CMYK_FALLBACK_WARNING}", asset.name));
                    lossless(&data, fmt, mode)
                }
            },
            _ if self.opts.compress_images => recompress(&data, mode).or_else(|| lossless(&data, fmt, mode)),
            _ => lossless(&data, fmt, mode),
        };
        if img.is_none() {
            if data.is_empty() {
                // A linked file that wasn't found when the document was opened: nothing to decode.
                let at = asset.link.as_deref().map(|l| format!(" ({l})")).unwrap_or_default();
                self.warn(format!("image `{}` is missing{at} and was skipped", asset.name));
            } else {
                self.warn(format!("image `{}` could not be decoded and was skipped", asset.name));
            }
        }
        if let Some(image) = &img {
            // Share the original encoded payload. HashMap checks exact bytes after hashing,
            // so collisions cannot merge distinct images or their diagnostics.
            self.images_by_content.insert(data, ConvertedImage { image: image.clone(), warning });
        }
        self.images.insert(id, img.clone());
        img
    }

    /// Draw a placed graphic, inside its clipping path when it has one. `hidden` is the
    /// placed PDF's layer choice: `None` keeps the file's defaults, `Some(&[])` shows every layer.
    /// Retained IDML graphic-layer options are not applied when drawing, so they warn here.
    pub(crate) fn graphic(&mut self, s: &mut Surface, g: &designcraft_doc::Graphic, hidden: Option<&[String]>) {
        if g.layer_options.as_ref().is_some_and(designcraft_doc::GraphicLayerOptions::has_visibility_overrides) {
            self.warn(designcraft_doc::GRAPHIC_LAYER_RENDERING_WARNING);
        }
        match g.clip_region().map(|(region, even_odd)| (to_path(&region), even_odd)) {
            Some((Some(clip), even_odd)) => {
                s.push_clip_path(&clip, if even_odd { &FillRule::EvenOdd } else { &FillRule::NonZero });
                self.graphic_unclipped(s, g, hidden);
                s.pop();
            }
            // A degenerate outline shows nothing, or everything when inverted.
            Some((None, even_odd)) => {
                if even_odd {
                    self.graphic_unclipped(s, g, hidden);
                }
            }
            None => self.graphic_unclipped(s, g, hidden),
        }
    }

    pub(super) fn graphic_unclipped(&mut self, s: &mut Surface, g: &designcraft_doc::Graphic, hidden: Option<&[String]>) {
        if self.graphic_media_placeholder(s, g) || self.graphic_placed_pdf(s, g, hidden) || self.graphic_placed_svg(s, g) {
            return;
        }
        if self.colorized(s, g) {
            return;
        }
        let Some(img) = self.load_image(g.asset) else { return };
        let Some(size) = Size::from_wh(g.size.0.max(1e-3) as f32, g.size.1.max(1e-3) as f32) else { return };
        s.push_transform(&tf(g.xf));
        s.draw_image(img, size);
        s.pop();
    }

    /// Video or sound without a poster: a dark frame with a play mark. `false` when `g` is not one.
    fn graphic_media_placeholder(&self, s: &mut Surface, g: &designcraft_doc::Graphic) -> bool {
        let Some(asset) = self.doc.assets.get(&g.asset) else { return false };
        if designcraft_doc::media_kind(&asset.mime).is_none() {
            return false;
        }
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
        true
    }

    /// A placed PDF goes in as vectors (the page as a form XObject). `false` when `g` is not one.
    fn graphic_placed_pdf(&mut self, s: &mut Surface, g: &designcraft_doc::Graphic, hidden: Option<&[String]>) -> bool {
        let doc = self.doc;
        let Some(asset) = doc.assets.get(&g.asset).filter(|asset| asset.data.starts_with(b"%PDF")) else { return false };
        // Cropped to a box: the whole page, placed so that the box spans graphic space.
        let shown = asset.shown_box();
        let [l, t, r, b] = shown.unwrap_or([0.0, 0.0, 1.0, 1.0]);
        let (page_w, page_h) = (g.size.0 / (r - l), g.size.1 / (b - t));
        let Some(size) = Size::from_wh(page_w.max(1e-3) as f32, page_h.max(1e-3) as f32) else { return true };
        let request = (g.asset, hidden.map(<[String]>::to_vec));
        let variant = match self.variants.get(&request) {
            Some(variant) => variant.clone(),
            None => {
                #[cfg(test)]
                {
                    self.pdf_preparations += 1;
                }
                match crate::optional_content::prepare(&asset.data, hidden) {
                    Ok(variant) => {
                        self.variants.insert(request, variant.clone());
                        variant
                    }
                    Err(error) => {
                        let message = match error {
                            PdfError::Write(message) => message,
                            other => other.to_string(),
                        };
                        self.layer_error.get_or_insert(PdfError::Write(format!("{}: {message}", asset.name)));
                        return true;
                    }
                }
            }
        };
        if let Some(warning) = variant.warning {
            self.warn(format!("{}: {warning}", asset.name));
        }
        let key = (g.asset, variant.hidden);
        let placed = match self.pdfs.get(&key) {
            Some(p) => p.clone(),
            None => {
                let p = self.placed_pdf(asset, variant.data);
                self.pdfs.insert(key, p.clone());
                p
            }
        };
        match placed {
            // krilla would fail the whole export at the end on a page the PDF doesn't have.
            Some((_, pages)) if asset.page as usize >= pages => {
                self.warn(format!("{}: the placed PDF has no page {}", asset.name, u64::from(asset.page) + 1));
            }
            Some((placed, _)) => {
                self.has_optional_content |= variant.has_optional_content;
                s.push_transform(&tf(g.xf));
                let clip = shown.and_then(|_| to_path(&Rect::new(0.0, 0.0, g.size.0, g.size.1).to_path(0.1)));
                if let Some(clip) = &clip {
                    s.push_clip_path(clip, &FillRule::NonZero);
                }
                s.push_transform(&tf(Affine::translate((-l * page_w, -t * page_h))));
                s.draw_pdf_page(&placed, size, asset.page as usize);
                s.pop();
                if clip.is_some() {
                    s.pop();
                }
                s.pop();
            }
            None => {}
        }
        true
    }

    /// A placed SVG goes in as vectors too. `false` when `g` is not one.
    fn graphic_placed_svg(&mut self, s: &mut Surface, g: &designcraft_doc::Graphic) -> bool {
        let Some(asset) = self.doc.assets.get(&g.asset).filter(|asset| designcraft_images::is_svg(&asset.data)) else { return false };
        let Some(size) = Size::from_wh(g.size.0.max(1e-3) as f32, g.size.1.max(1e-3) as f32) else { return true };
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
        true
    }

    /// A greyscale or bitmap image coloured by the swatch applied to it (#1094): the swatch is
    /// painted through the image's ink coverage (its darkness), over 0% ink for a greyscale
    /// image, whose white is opaque. `false`, drawing nothing, when the image keeps its own
    /// colours: no swatch, one that no longer exists, a colour image.
    fn colorized(&mut self, s: &mut Surface, g: &designcraft_doc::Graphic) -> bool {
        if g.fill.is_none() {
            return false;
        }
        let Some(asset) = self.doc.assets.get(&g.asset) else { return false };
        let Some(tone) = designcraft_images::tone(&asset.data) else { return false };
        let Some(paint) = self.swatch_color(&g.fill.swatch, g.fill.tint) else { return false };
        let Some(coverage) = designcraft_images::decode_rgba(&asset.data).and_then(|img| CoverageImage::new(&img)) else { return false };
        let interpolate = RasterMode::for_standard(self.opts.standard).interpolate;
        let Ok(mask) = Image::from_custom(coverage, interpolate) else { return false };
        let size = Size::from_wh(g.size.0.max(1e-3) as f32, g.size.1.max(1e-3) as f32);
        let (Some(size), Some(rect)) = (size, to_path(&Rect::new(0.0, 0.0, g.size.0, g.size.1).to_path(0.1))) else { return false };
        s.push_transform(&tf(g.xf));
        if tone == designcraft_images::Tone::Gray {
            let blank =
                self.swatch_color(designcraft_color::swatch::PAPER, 1.0).unwrap_or_else(|| device(&Color::cmyk(0.0, 0.0, 0.0, 0.0), self.rgb_only));
            s.set_fill(Some(Fill { paint: blank.into(), ..Default::default() }));
            s.draw_path(&rect);
        }
        let mut sb = s.stream_builder();
        let mut ms = sb.surface();
        ms.draw_image(mask, size);
        ms.finish();
        let stream = sb.finish();
        s.push_mask(krilla::mask::Mask::new(stream, krilla::mask::MaskType::Luminosity));
        s.set_fill(Some(Fill { paint: paint.into(), ..Default::default() }));
        s.draw_path(&rect);
        s.pop();
        s.set_fill(None);
        s.pop();
        true
    }
}

/// `paint` through `mask`'s coverage, a luminosity mask: the color's own inks (spot, process),
/// never an RGB raster. The shadow multiplies with what is beneath it, as InDesign's drop shadow
/// does by default (the shadow model has no blend mode of its own), so a spot under a black
/// shadow keeps its full ink.
pub(super) fn draw_coverage(s: &mut Surface, mask: &crate::ShadowMask, paint: krilla::color::Color) {
    let r = mask.rect;
    let (Some(size), Some(rect), Some(image)) = (
        Size::from_wh(r.width().max(1e-3) as f32, r.height().max(1e-3) as f32),
        to_path(&r.to_path(0.1)),
        CoverageImage::from_grey(mask.coverage.clone(), (mask.width, mask.height)),
    ) else {
        return;
    };
    let Ok(image) = Image::from_custom(image, false) else { return };
    let mut sb = s.stream_builder();
    let mut ms = sb.surface();
    ms.push_transform(&tf(Affine::translate((r.x0, r.y0))));
    ms.draw_image(image, size);
    ms.pop();
    ms.finish();
    let stream = sb.finish();
    s.push_blend_mode(krilla::blend::BlendMode::Multiply);
    s.push_mask(krilla::mask::Mask::new(stream, krilla::mask::MaskType::Luminosity));
    s.set_stroke(None);
    s.set_fill(Some(Fill { paint: paint.into(), ..Default::default() }));
    s.draw_path(&rect);
    s.set_fill(None);
    s.pop();
    s.pop();
}

/// The ink coverage of a greyscale or bitmap image (255 = black, 0 = white or transparent) as a
/// DeviceGray image, painted into a luminosity mask.
#[derive(Clone, Hash)]
pub(super) struct CoverageImage {
    pub(super) grey: Arc<Vec<u8>>,
    pub(super) size: (u32, u32),
}
