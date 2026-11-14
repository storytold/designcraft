//! EPS: there's no general PostScript interpreter here, so a placed EPS shows (and prints)
//! through a proxy, as InDesign does on screen: the file's TIFF preview when it has one, else the
//! rectangles of a simple one drawn here (InDesign's QR codes), else a placeholder the size of its
//! bounding box.

/// The PostScript section and the TIFF preview (DOS EPS), read by craft-eps.
fn sections(bytes: &[u8]) -> (&[u8], Option<&[u8]>) {
    craft_eps::sections(bytes).map_or((&[][..], None), |parts| (parts.postscript, parts.tiff))
}

/// The bounding box in points: (x0, y0, x1, y1), the high-resolution one when given.
pub fn bounding_box(bytes: &[u8]) -> Option<[f64; 4]> {
    let parts = craft_eps::sections(bytes).ok()?;
    craft_eps::Comments::read(parts.postscript).page_box()
}

/// Natural EPS dimensions in points. Bound sizes before fitting or allocating a proxy:
/// sub-thousandth-point artwork and dimensions over 20,000 points aren't usable layouts.
pub fn eps_size(bytes: &[u8]) -> Option<(f64, f64)> {
    let bb = bounding_box(bytes)?;
    let (w, h) = (bb[2] - bb[0], bb[3] - bb[1]);
    ((0.001..=20_000.0).contains(&w) && (0.001..=20_000.0).contains(&h)).then_some((w, h))
}

fn preview(bytes: &[u8]) -> Option<(&[u8], (u32, u32))> {
    let (_, tiff) = sections(bytes);
    let tiff = tiff?;
    let (w, h) = crate::pixel_size(tiff)?;
    // Match the decoder's allocation budget, including conversion to RGBA.
    (w > 0 && h > 0 && u64::from(w) * u64::from(h) <= 256 * 1024 * 1024 / 4).then_some((tiff, (w, h)))
}

/// Proxy pixel dimensions, without encoding the placeholder just to read its size.
pub fn eps_pixel_size(bytes: &[u8]) -> Option<(u32, u32)> {
    let (w, h) = eps_size(bytes)?;
    Some(
        preview(bytes)
            .map(|(_, px)| px)
            .unwrap_or_else(|| ((w * 2.0).round().clamp(2.0, 4000.0) as u32, (h * 2.0).round().clamp(2.0, 4000.0) as u32)),
    )
}

/// A browser-readable EPS preview. Keep the original TIFF for print exports (CMYK inks),
/// but HTML and EPUB need PNG rather than a TIFF data URI or a mislabeled .png file.
pub fn eps_png(bytes: &[u8]) -> Option<Vec<u8>> {
    let (proxy, _) = eps_proxy(bytes)?;
    if proxy.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some(proxy);
    }
    let rgba = crate::decode_rgba(&proxy)?;
    encode_png(rgba)
}

/// The proxy for a placed EPS: (image bytes, its size in points). The TIFF preview when present,
/// else a light grey placeholder with a cross, at 2 pixels per point.
pub fn eps_proxy(bytes: &[u8]) -> Option<(Vec<u8>, (f64, f64))> {
    let (w, h) = eps_size(bytes)?;
    if let Some((tiff, _)) = preview(bytes) {
        return Some((tiff.to_vec(), (w, h)));
    }
    // Simple PostScript drawn here: InDesign's QR codes are coloured rectangles only.
    if let Some(bb) = bounding_box(bytes)
        && let Some(png) = render_rectangles(sections(bytes).0, bb)
    {
        return Some((png, (w, h)));
    }
    let (pw, ph) = eps_pixel_size(bytes)?;
    let mut img = image::RgbaImage::from_pixel(pw, ph, image::Rgba([228, 228, 228, 255]));
    // Diagonals and a border mark it as a stand-in.
    let n = pw.max(ph);
    for i in 0..n {
        let (x, y) = ((i as f64 / n as f64 * pw as f64) as u32, (i as f64 / n as f64 * ph as f64) as u32);
        for (px, py) in [(x, y), (x, ph - 1 - y.min(ph - 1))] {
            img.put_pixel(px.min(pw - 1), py.min(ph - 1), image::Rgba([150, 150, 150, 255]));
        }
    }
    for x in 0..pw {
        img.put_pixel(x, 0, image::Rgba([150, 150, 150, 255]));
        img.put_pixel(x, ph - 1, image::Rgba([150, 150, 150, 255]));
    }
    for y in 0..ph {
        img.put_pixel(0, y, image::Rgba([150, 150, 150, 255]));
        img.put_pixel(pw - 1, y, image::Rgba([150, 150, 150, 255]));
    }
    Some((encode_png(img)?, (w, h)))
}

fn encode_png(img: image::RgbaImage) -> Option<Vec<u8>> {
    let (width, height) = img.dimensions();
    let image = craft_image_codecs::Image::from_u8(width, height, craft_image_codecs::ChannelLayout::Rgba, img.into_raw()).ok()?;
    craft_image_codecs::encode(&image, craft_image_codecs::Format::Png, &Default::default()).ok()
}

/// Pixels per point for rendered EPS (a QR code's modules stay crisp when scaled up on screen).
const RENDER_SCALE: f64 = 8.0;
/// The most pixels painted, in multiples of the image's area (overlapping rectangles repaint).
const MAX_PAINT_AREAS: u64 = 4;

/// Rasterize rectangles read by craft-eps; unsupported PostScript keeps the placeholder.
fn render_rectangles(ps: &[u8], bb: [f64; 4]) -> Option<Vec<u8>> {
    let rectangles = craft_eps::simple_rectangles(ps)?;
    let (w, h) = (bb[2] - bb[0], bb[3] - bb[1]);
    if !(w > 0.0 && h > 0.0) {
        return None;
    }
    let scale = RENDER_SCALE.min(4000.0 / w.max(h));
    let (pw, ph) = ((w * scale).round().max(1.0) as u32, (h * scale).round().max(1.0) as u32);
    let mut img = image::RgbaImage::from_pixel(pw, ph, image::Rgba([0, 0, 0, 0]));
    let mut painted = 0u64;
    let paint_budget = u64::from(pw) * u64::from(ph) * MAX_PAINT_AREAS;
    for rect in rectangles {
        let (x0, x1) = ((rect.x - bb[0]) * scale, (rect.x + rect.width - bb[0]) * scale);
        let (y0, y1) = ((bb[3] - (rect.y + rect.height)) * scale, (bb[3] - rect.y) * scale);
        let px = |a: f64, b: f64, max: u32| {
            let (lo, hi) = (a.min(b).round().max(0.0), a.max(b).round().min(max as f64));
            (lo as u32, hi.max(lo) as u32)
        };
        let ((xa, xb), (ya, yb)) = (px(x0, x1, pw), px(y0, y1, ph));
        painted = painted.saturating_add(u64::from(xb - xa) * u64::from(yb - ya));
        if painted > paint_budget {
            return None;
        }
        for y in ya..yb {
            for x in xa..xb {
                img.put_pixel(x, y, image::Rgba(rect.color));
            }
        }
    }
    encode_png(img)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eps_rejects_unsafe_bounds_and_accepts_trailer_bounds() {
        for bounds in ["0 0 1e-320 1e-320", "0 0 1e308 1e308", "0 0 inf 10"] {
            let eps = format!("%!PS-Adobe-3.0 EPSF-3.0\n%%BoundingBox: {bounds}\n%%EOF\n");
            assert!(eps_proxy(eps.as_bytes()).is_none(), "{bounds}");
        }
        let mut eps = b"%!PS-Adobe-3.0 EPSF-3.0\n%%BoundingBox: (atend)\n".to_vec();
        eps.extend(vec![b' '; 70_000]);
        eps.extend(b"\n%%Trailer\n%%BoundingBox: 0 0 20 10\n%%EOF\n");
        assert_eq!(eps_proxy(&eps).unwrap().1, (20.0, 10.0));
    }

    #[test]
    fn eps_nested_document_bounds_do_not_override_outer_artwork() {
        for (deferred, padding, child_padding) in [(false, 0, 0), (false, 8_000, 0), (true, 8_000, 0), (true, 0, 8_000)] {
            let mut eps =
                format!("%!PS-Adobe-3.0 EPSF-3.0\n%%BoundingBox: {}\n%%EndComments\n", if deferred { "(atend)" } else { "0 0 100 50" }).into_bytes();
            eps.extend(b"% padding\n".repeat(padding));
            eps.extend(b"%%BeginDocument: child.eps\n%!PS-Adobe-3.0 EPSF-3.0\n%%BoundingBox: (atend)\n%%EndComments\n");
            eps.extend(b"% child padding\n".repeat(child_padding));
            eps.extend(b"%%Trailer\n%%BoundingBox: 0 0 10 20\n%%HiResBoundingBox: 0 0 10 20\n%%EOF\n%%EndDocument\n");
            if deferred {
                eps.extend(b"%%Trailer\n%%BoundingBox: 0 0 100 50\n");
            }
            eps.extend(b"%%EOF\n");
            assert_eq!(eps_size(&eps), Some((100.0, 50.0)), "deferred={deferred}, padding={padding}, child_padding={child_padding}");
        }
        let eps = b"%!PS-Adobe-3.0 EPSF-3.0\n%%BoundingBox: (atend)\n%%HiResBoundingBox: (atend)\n%%EndComments\nnewpath\n%%Trailer\n%%BoundingBox: 0 0 101 51\n%%HiResBoundingBox: 0 0 100.5 50.25\n%%EOF\n";
        assert_eq!(eps_size(eps), Some((100.5, 50.25)));
    }

    #[test]
    fn simple_rectangle_eps_is_drawn_like_a_qr_code() {
        // As InDesign writes QR codes: a procedure for rectfill, a colour, then modules.
        let eps = b"%!PS-Adobe-3.1 EPSF-3.0\n%%BoundingBox: 0 0 4 4\n%%EndComments\n/rf { rectfill } bind def\n0 0 0 1 setcmykcolor\n0 2 2 2 rf\n2 0 2 2 rf\n%%EOF\n";
        let (png, size) = eps_proxy(eps).unwrap();
        assert_eq!(size, (4.0, 4.0));
        let img = image::load_from_memory(&png).unwrap().to_rgba8();
        assert_eq!(img.dimensions(), (32, 32), "8 pixels per point");
        // PostScript y runs up: (0, 2)–(2, 4) is the image's top left.
        assert_eq!(img.get_pixel(4, 4).0, [0, 0, 0, 255]);
        assert_eq!(img.get_pixel(28, 28).0, [0, 0, 0, 255]);
        assert_eq!(img.get_pixel(28, 4).0[3], 0, "undrawn stays transparent");
        assert_eq!(img.get_pixel(4, 28).0[3], 0);
        // Anything else (here a path) keeps the placeholder.
        let curve = b"%!PS-Adobe-3.0 EPSF-3.0\n%%BoundingBox: 0 0 4 4\n0 0 moveto 4 4 lineto stroke\n%%EOF\n";
        let (png, _) = eps_proxy(curve).unwrap();
        let img = image::load_from_memory(&png).unwrap().to_rgba8();
        assert_eq!(img.get_pixel(2, 4).0, [228, 228, 228, 255]);
    }

    /// The colour at pixel (x, y) of a drawn simple EPS (8 pixels per point).
    fn drawn(eps: &[u8], x: u32, y: u32) -> [u8; 4] {
        let (png, _) = eps_proxy(eps).unwrap();
        image::load_from_memory(&png).unwrap().to_rgba8().get_pixel(x, y).0
    }

    #[test]
    fn gsave_grestore_pop_and_cr_line_ends_in_simple_eps() {
        // `\r`-only lines; red inside gsave/grestore, black again after; `pop` drops the 7.
        let eps = b"%!PS-Adobe-3.1 EPSF-3.0\r%%BoundingBox: 0 0 4 4\r/rf { rectfill } bind def\r0 0 0 1 setcmykcolor\rgsave 1 0 0 setrgbcolor 0 2 2 2 7 pop rf grestore\r2 0 2 2 rf\r%%EOF\r";
        assert_eq!(drawn(eps, 4, 4), [255, 0, 0, 255], "top left: red");
        assert_eq!(drawn(eps, 28, 28), [0, 0, 0, 255], "bottom right: the colour from before gsave");
        assert_eq!(drawn(eps, 28, 4)[3], 0, "top right: nothing (pop removed the extra operand)");
        // Unbounded nesting isn't drawn.
        let deep = format!("{} 0 0 2 2 rectfill", "gsave ".repeat(craft_eps::MAX_SIMPLE_RECTANGLE_SAVE_DEPTH + 1));
        assert_eq!(render_rectangles(deep.as_bytes(), [0.0, 0.0, 4.0, 4.0]), None);
    }

    #[test]
    fn a_huge_token_stream_on_one_line_is_refused() {
        // One line of more operands than the token limit, then a rectangle.
        let ps = format!("{}\n0 0 2 2 rectfill\n", "0 ".repeat(craft_eps::MAX_SIMPLE_RECTANGLE_TOKENS));
        assert_eq!(render_rectangles(ps.as_bytes(), [0.0, 0.0, 4.0, 4.0]), None);
        let ps = format!("{}\n0 0 2 2 rectfill\n", "0 ".repeat(1000));
        assert!(render_rectangles(ps.as_bytes(), [0.0, 0.0, 4.0, 4.0]).is_some(), "under the limit it is drawn");
    }

    #[test]
    fn eps_bounding_box_and_proxies() {
        let eps = b"%!PS-Adobe-3.0 EPSF-3.0\n%%BoundingBox: 0 0 100 50\n%%HiResBoundingBox: 0 0 100.5 50.25\nnewpath\n%%EOF\n";
        assert!(craft_eps::is_eps(eps));
        assert_eq!(bounding_box(eps), Some([0.0, 0.0, 100.5, 50.25]));
        let (png, size) = eps_proxy(eps).unwrap();
        assert_eq!(size, (100.5, 50.25));
        assert_eq!(crate::pixel_size(&png), Some((201, 101)));
        // DOS EPS with a TIFF preview: the preview is the proxy.
        let mut tiff = Vec::new();
        image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(10, 5, image::Rgba([255, 0, 0, 255])))
            .write_to(&mut std::io::Cursor::new(&mut tiff), image::ImageFormat::Tiff)
            .unwrap();
        let mut dos = vec![0xC5, 0xD0, 0xD3, 0xC6];
        let ps_off = 30usize;
        let tiff_off = ps_off + eps.len();
        for v in [ps_off, eps.len(), 0, 0, tiff_off, tiff.len()] {
            dos.extend((v as u32).to_le_bytes());
        }
        dos.extend([0xFF, 0xFF]);
        dos.extend_from_slice(eps);
        dos.extend_from_slice(&tiff);
        assert!(craft_eps::is_eps(&dos));
        let (p, size) = eps_proxy(&dos).unwrap();
        assert_eq!(size, (100.5, 50.25));
        assert_eq!(crate::pixel_size(&p), Some((10, 5)));
        assert!(!craft_eps::is_eps(b"%!PS-Adobe-3.0\nnot encapsulated"));
    }

    #[test]
    fn dos_eps_wmf_only_is_not_read_as_tiff() {
        let eps = b"%!PS-Adobe-3.0 EPSF-3.0\n%%BoundingBox: 0 0 100 50\nnewpath\n%%EOF\n";
        let wmf = b"WMFDATA!";
        let mut dos = vec![0xC5, 0xD0, 0xD3, 0xC6];
        let ps_off = 30usize;
        let wmf_off = ps_off + eps.len();
        for v in [ps_off, eps.len(), wmf_off, wmf.len(), 0, 0] {
            dos.extend((v as u32).to_le_bytes());
        }
        dos.extend([0xFF, 0xFF]);
        dos.extend_from_slice(eps);
        dos.extend_from_slice(wmf);
        assert!(craft_eps::is_eps(&dos));
        // The WMF bytes are not a TIFF preview.
        assert!(sections(&dos).1.is_none());
        // The PostScript section still parses, and the proxy is the no-preview placeholder.
        assert_eq!(bounding_box(&dos), Some([0.0, 0.0, 100.0, 50.0]));
        let (png, size) = eps_proxy(&dos).unwrap();
        assert_eq!(size, (100.0, 50.0));
        assert_eq!(crate::pixel_size(&png), Some((200, 100)));
    }

    #[test]
    fn eps_bounding_box_rejects_nonfinite_sizes() {
        let bb = |line: &str| bounding_box(format!("%!PS-Adobe-3.0 EPSF-3.0\n{line}\n").as_bytes());
        assert_eq!(bb("%%BoundingBox: 0 0 100 50"), Some([0.0, 0.0, 100.0, 50.0]));
        assert_eq!(bb("%%BoundingBox: 0 0 inf 50"), None);
        assert_eq!(bb("%%BoundingBox: -inf 0 100 50"), None);
        assert_eq!(bb("%%BoundingBox: -1e308 0 1e308 50"), None);
        assert_eq!(bb("%%BoundingBox: 100 50 0 0"), None);
        // A bad high-resolution box falls back to the plain one.
        assert_eq!(bb("%%HiResBoundingBox: 0 0 inf inf\n%%BoundingBox: 0 0 10 20"), Some([0.0, 0.0, 10.0, 20.0]));
    }
}
