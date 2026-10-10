//! EPS: there's no PostScript interpreter here, so a placed EPS shows (and prints) through a
//! proxy, as InDesign does on screen: the file's TIFF preview when it has one, else a placeholder
//! the size of its bounding box.

/// An EPS file (plain, or DOS EPS with a binary header)?
pub fn is_eps(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0xC5, 0xD0, 0xD3, 0xC6])
        || (bytes.starts_with(b"%!PS-Adobe") && String::from_utf8_lossy(&bytes[..bytes.len().min(64)]).contains("EPSF"))
}

fn u32le(b: &[u8], at: usize) -> Option<usize> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?) as usize)
}

/// The PostScript section and the TIFF preview (DOS EPS).
fn sections(bytes: &[u8]) -> (&[u8], Option<&[u8]>) {
    if bytes.starts_with(&[0xC5, 0xD0, 0xD3, 0xC6]) {
        let ps = u32le(bytes, 4).zip(u32le(bytes, 8)).and_then(|(o, l)| bytes.get(o..o.checked_add(l)?)).unwrap_or(&[]);
        let tiff = u32le(bytes, 20).zip(u32le(bytes, 24)).filter(|(o, l)| *o > 0 && *l > 0).and_then(|(o, l)| bytes.get(o..o.checked_add(l)?));
        return (ps, tiff);
    }
    (bytes, None)
}

/// The bounding box in points: (x0, y0, x1, y1), the high-resolution one when given.
pub fn bounding_box(bytes: &[u8]) -> Option<[f64; 4]> {
    let (ps, _) = sections(bytes);
    let head = String::from_utf8_lossy(&ps[..ps.len().min(64 * 1024)]);
    // DSC allows (atend): scan a bounded trailer as well, without reading the whole program.
    let tail = String::from_utf8_lossy(&ps[ps.len().saturating_sub(64 * 1024)..]);
    let header_value = |key: &str| {
        head.lines()
            .take_while(|l| {
                (l.starts_with('%') || l.trim().is_empty())
                    && ![
                        "%%EndComments",
                        "%%BeginDocument",
                        "%%BeginProlog",
                        "%%BeginSetup",
                        "%%BeginData",
                        "%%BeginBinary",
                        "%%BeginResource",
                        "%%Page:",
                    ]
                    .iter()
                    .any(|k| l.starts_with(k))
            })
            .find_map(|l| l.strip_prefix(key))
    };
    let hires = header_value("%%HiResBoundingBox:");
    let bbox = header_value("%%BoundingBox:");
    let at_end = |v: Option<&str>| v.is_some_and(|v| v.trim() == "(atend)");
    // An outer header box must not be replaced by a nested document's trailer comments.
    // Inspect the trailer only when the outer header delegates its bounds there.
    hires
        .and_then(parse_box)
        .or_else(|| (at_end(hires) || at_end(bbox)).then(|| trailer_box(&tail, "%%HiResBoundingBox:")).flatten())
        .or_else(|| bbox.and_then(parse_box))
        .or_else(|| at_end(bbox).then(|| trailer_box(&tail, "%%BoundingBox:")).flatten())
}

fn parse_box(value: &str) -> Option<[f64; 4]> {
    let mut words = value.split_whitespace();
    let mut bb = [0.0f64; 4];
    for n in &mut bb {
        *n = words.next()?.parse().ok()?;
    }
    (words.next().is_none() && bb.iter().all(|n| n.is_finite()) && bb[2] > bb[0] && bb[3] > bb[1]).then_some(bb)
}

fn trailer_box(tail: &str, key: &str) -> Option<[f64; 4]> {
    // Work backwards from the outer EOF so nesting remains known even when the bounded
    // window begins inside a child document. Opaque DSC data/resource blocks are skipped too.
    let mut nested = 0usize;
    for line in tail.lines().rev() {
        if ["%%EndDocument", "%%EndData", "%%EndBinary", "%%EndResource"].iter().any(|k| line.starts_with(k)) {
            nested = nested.checked_add(1)?;
        } else if ["%%BeginDocument", "%%BeginData", "%%BeginBinary", "%%BeginResource"].iter().any(|k| line.starts_with(k)) {
            nested = nested.checked_sub(1)?;
        } else if nested == 0
            && let Some(bb) = line.strip_prefix(key).and_then(parse_box)
        {
            return Some(bb);
        }
    }
    None
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
    let mut png = Vec::new();
    image::DynamicImage::ImageRgba8(rgba).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).ok()?;
    Some(png)
}

/// The proxy for a placed EPS: (image bytes, its size in points). The TIFF preview when present,
/// else a light grey placeholder with a cross, at 2 pixels per point.
pub fn eps_proxy(bytes: &[u8]) -> Option<(Vec<u8>, (f64, f64))> {
    let (w, h) = eps_size(bytes)?;
    if let Some((tiff, _)) = preview(bytes) {
        return Some((tiff.to_vec(), (w, h)));
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
    let mut out = Vec::new();
    image::DynamicImage::ImageRgba8(img).write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).ok()?;
    Some((out, (w, h)))
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
    fn eps_bounding_box_and_proxies() {
        let eps = b"%!PS-Adobe-3.0 EPSF-3.0\n%%BoundingBox: 0 0 100 50\n%%HiResBoundingBox: 0 0 100.5 50.25\nnewpath\n%%EOF\n";
        assert!(is_eps(eps));
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
        assert!(is_eps(&dos));
        let (p, size) = eps_proxy(&dos).unwrap();
        assert_eq!(size, (100.5, 50.25));
        assert_eq!(crate::pixel_size(&p), Some((10, 5)));
        assert!(!is_eps(b"%!PS-Adobe-3.0\nnot encapsulated"));
    }
}
