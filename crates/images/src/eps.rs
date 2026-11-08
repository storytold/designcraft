//! EPS: there's no general PostScript interpreter here, so a placed EPS shows (and prints)
//! through a proxy, as InDesign does on screen: the file's TIFF preview when it has one, else the
//! rectangles of a simple one drawn here (InDesign's QR codes), else a placeholder the size of its
//! bounding box.

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
        // Lines may end in `\r` alone (classic Mac files).
        head.split(['\n', '\r'])
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
    // Finite corners and a finite size (subtracting extreme corners can overflow to infinity).
    let finite = [bb[0], bb[1], bb[2], bb[3], bb[2] - bb[0], bb[3] - bb[1]].iter().all(|n| n.is_finite());
    (words.next().is_none() && finite && bb[2] > bb[0] && bb[3] > bb[1]).then_some(bb)
}

fn trailer_box(tail: &str, key: &str) -> Option<[f64; 4]> {
    // Work backwards from the outer EOF so nesting remains known even when the bounded
    // window begins inside a child document. Opaque DSC data/resource blocks are skipped too.
    let mut nested = 0usize;
    for line in tail.split(['\n', '\r']).rev() {
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
    let mut out = Vec::new();
    image::DynamicImage::ImageRgba8(img).write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).ok()?;
    Some((out, (w, h)))
}

/// Pixels per point for rendered EPS (a QR code's modules stay crisp when scaled up on screen).
const RENDER_SCALE: f64 = 8.0;
/// The most rectangles and operators read from an EPS before giving up on drawing it.
const MAX_RECTS: usize = 200_000;
const MAX_TOKENS: usize = 2_000_000;
/// The most pixels painted, in multiples of the image's area (overlapping rectangles repaint).
const MAX_PAINT_AREAS: u64 = 4;
/// The deepest `gsave`/`save` nesting read.
const MAX_SAVE_DEPTH: usize = 64;

/// PostScript tokens without comments, `{` and `}` as tokens of their own. Lazy and borrowed, so
/// a huge input costs no memory beyond itself; lines may end in `\n`, `\r\n` or `\r`.
fn ps_tokens(text: &str) -> impl Iterator<Item = &str> {
    text.split(['\n', '\r']).flat_map(|line| {
        line.split('%').next().unwrap_or("").split_whitespace().flat_map(|w| w.split_inclusive(['{', '}'])).flat_map(|piece| {
            let (body, brace) = match piece.char_indices().last() {
                Some((at, '{' | '}')) => (piece.get(..at).unwrap_or(""), piece.get(at..)),
                _ => (piece, None),
            };
            [Some(body).filter(|b| !b.is_empty()), brace].into_iter().flatten()
        })
    })
}

/// PostScript that only fills rectangles in solid colours (`setcmykcolor`, `setrgbcolor`,
/// `setgray`, `rectfill`, and procedures that stand for one of them, such as InDesign's
/// `/rf { rectfill } bind def`), drawn as a PNG of the bounding box `bb`. None for anything else
/// (paths, text, images, transforms), which keeps its placeholder, or when nothing is drawn.
fn render_rectangles(ps: &[u8], bb: [f64; 4]) -> Option<Vec<u8>> {
    let text = std::str::from_utf8(ps).ok()?;
    let (w, h) = (bb[2] - bb[0], bb[3] - bb[1]);
    if !(w > 0.0 && h > 0.0) {
        return None;
    }
    let scale = RENDER_SCALE.min(4000.0 / w.max(h));
    let (pw, ph) = ((w * scale).round().max(1.0) as u32, (h * scale).round().max(1.0) as u32);
    let mut img = image::RgbaImage::from_pixel(pw, ph, image::Rgba([0, 0, 0, 0]));
    let mut aliases: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let mut stack: Vec<f64> = Vec::new();
    let mut color = [0u8, 0, 0, 255];
    let mut drawn = 0usize;
    let mut painted = 0u64;
    let paint_budget = u64::from(pw) * u64::from(ph) * MAX_PAINT_AREAS;
    let mut saved: Vec<[u8; 4]> = Vec::new();
    let mut tokens = ps_tokens(text);
    let mut read = 0usize;
    let mut next = || {
        read += 1;
        if read > MAX_TOKENS { None } else { Some(tokens.next()) }
    };
    let unit = |v: f64| v.clamp(0.0, 1.0);
    // `None` from `next`: over the token limit; `Some(None)`: the end.
    while let Some(tok) = next()? {
        if let Ok(v) = tok.parse::<f64>() {
            if !v.is_finite() {
                return None;
            }
            stack.push(v);
            continue;
        }
        // `/name { op } bind def`: name stands for op.
        if let Some(name) = tok.strip_prefix('/') {
            // At most `{ op } bind`: anything longer isn't an alias this reader knows.
            let mut body: Vec<&str> = Vec::new();
            while body.len() < 5 {
                match next()? {
                    Some("def") | None => break,
                    Some(t) => body.push(t),
                }
            }
            let op = match body.as_slice() {
                ["{", op, "}"] | ["{", op, "}", _] => (*op).to_string(),
                _ => return None,
            };
            aliases.insert(name.to_string(), op);
            continue;
        }
        let op = aliases.get(tok).map_or(tok, String::as_str);
        let mut pop = |n: usize| -> Option<Vec<f64>> { (stack.len() >= n).then(|| stack.split_off(stack.len() - n)) };
        match op {
            "setcmykcolor" => {
                let v = pop(4)?;
                let k = unit(v[3]);
                let ch = |c: f64| ((1.0 - unit(c)) * (1.0 - k) * 255.0).round() as u8;
                color = [ch(v[0]), ch(v[1]), ch(v[2]), 255];
            }
            "setrgbcolor" => {
                let v = pop(3)?;
                let ch = |c: f64| (unit(c) * 255.0).round() as u8;
                color = [ch(v[0]), ch(v[1]), ch(v[2]), 255];
            }
            "setgray" => {
                let v = pop(1)?;
                let g = (unit(v[0]) * 255.0).round() as u8;
                color = [g, g, g, 255];
            }
            "rectfill" => {
                let v = pop(4)?;
                drawn += 1;
                if drawn > MAX_RECTS {
                    return None;
                }
                // PostScript y runs up; the image's down. Edges round to whole pixels, so
                // touching rectangles (QR code modules) meet without gaps.
                let (x0, x1) = ((v[0] - bb[0]) * scale, (v[0] + v[2] - bb[0]) * scale);
                let (y0, y1) = ((bb[3] - (v[1] + v[3])) * scale, (bb[3] - v[1]) * scale);
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
                        img.put_pixel(x, y, image::Rgba(color));
                    }
                }
            }
            "gsave" | "save" => {
                if saved.len() >= MAX_SAVE_DEPTH {
                    return None;
                }
                saved.push(color);
            }
            "grestore" | "restore" => {
                if let Some(c) = saved.pop() {
                    color = c;
                }
            }
            "pop" => {
                pop(1)?;
            }
            "bind" | "newpath" | "showpage" | "end" | "begin" => {}
            _ => return None,
        }
    }
    if drawn == 0 {
        return None;
    }
    let mut out = Vec::new();
    image::DynamicImage::ImageRgba8(img).write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).ok()?;
    Some(out)
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
        let deep = format!("{} 0 0 2 2 rectfill", "gsave ".repeat(MAX_SAVE_DEPTH + 1));
        assert_eq!(render_rectangles(deep.as_bytes(), [0.0, 0.0, 4.0, 4.0]), None);
    }

    #[test]
    fn a_huge_token_stream_on_one_line_is_refused() {
        // One line of more operands than the token limit, then a rectangle.
        let ps = format!("{}\n0 0 2 2 rectfill\n", "0 ".repeat(MAX_TOKENS));
        assert_eq!(render_rectangles(ps.as_bytes(), [0.0, 0.0, 4.0, 4.0]), None);
        let ps = format!("{}\n0 0 2 2 rectfill\n", "0 ".repeat(1000));
        assert!(render_rectangles(ps.as_bytes(), [0.0, 0.0, 4.0, 4.0]).is_some(), "under the limit it is drawn");
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
