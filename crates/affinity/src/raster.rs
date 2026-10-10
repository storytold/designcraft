//! Pixel layers (`Rstr`) and placed images (`ImgN`). A placed image keeps its original file
//! (`DyBm.Bckg` → archive entry `c/N`), which is imported as is; other pixels are planar tiles of
//! 256 bytes × 256 rows per channel, each its own archive entry.

use std::collections::HashMap;

use crate::Error;
use crate::model::{Affine, Image, Pixels, Reader};
use crate::stream::{self, ObjId, Tag, Value};

/// Pixels decoded per layer (the content's bounding box, not the canvas).
const MAX_PIXELS: u64 = 64 << 20;
const TILE: usize = 256;
const TILE_BYTES: usize = TILE * TILE;

struct CachedPixels {
    tiles: HashMap<String, Vec<u8>>,
    source: Option<(usize, Vec<u8>)>,
}

pub(crate) fn node_image(r: &mut Reader, id: ObjId, world: Affine) -> Result<Option<Image>, Error> {
    let s = r.s;
    let Some(bitmap) = s.obj(id, b"Bitm") else {
        r.warn("pixel layers without pixels");
        return Ok(None);
    };
    if !s.is(bitmap, b"DyBm") {
        r.warn("pixel layers stored in an unknown way");
        return Ok(None);
    }
    let width = s.int(bitmap, b"BmpW").and_then(|v| u32::try_from(v).ok()).unwrap_or(0);
    let height = s.int(bitmap, b"BmpH").and_then(|v| u32::try_from(v).ok()).unwrap_or(0);
    if width == 0 || height == 0 {
        return Ok(None);
    }
    // A placed image: the original file, byte for byte.
    let has_tiles = s.field(bitmap, b"Sta1").is_some();
    if let Some(name) = s.entry(bitmap, b"Bckg").filter(|n| !n.is_empty())
        && (s.is(id, b"ImgN") || !has_tiles)
    {
        match original(r, name) {
            // HEIF/HEIC (phone photos) and AVIF originals can't be decoded here: the same pixels
            // stored as tiles are used instead (see `decode` when the tiles point back at the original).
            Ok(bytes) if is_heif(&bytes) && has_tiles => {}
            Ok(bytes) => return Ok(Some(Image { width, height, pixels: Pixels::Encoded(bytes), transform: world })),
            Err(Error::Limit(e)) => return Err(Error::Limit(e)),
            Err(_) => r.warn("an embedded image file that could not be read (its cached pixels were used)"),
        }
    }
    // The content's bounding box, so a mostly empty canvas-sized layer stays small.
    let crop = s
        .field(id, b"BitR")
        .and_then(|v| if let Value::Ints(i) = v { Some(i.clone()) } else { None })
        .filter(|v| v.len() == 4 && v[0] > i64::from(i32::MIN) + 1)
        .map(|v| {
            (
                v[0].clamp(0, i64::from(width)) as u32,
                v[1].clamp(0, i64::from(height)) as u32,
                v[2].clamp(0, i64::from(width)) as u32,
                v[3].clamp(0, i64::from(height)) as u32,
            )
        })
        .unwrap_or((0, 0, width, height));
    let source_tiles_only =
        (1..=4).all(|c| matches!(s.field(bitmap, &tag(b"Sta", c)), Some(Value::Array(a)) if !a.is_empty() && a.iter().all(|v| *v == Value::UInt(5))));
    if s.field(bitmap, b"Bckg").is_some() && has_tiles && !source_tiles_only && !s.is(id, b"ImgN") {
        r.warn("edited placed images use their stored pixels");
    }
    decode(r, bitmap, crop, world)
}

/// An ISO base media file holding a HEIF image (HEIC, AVIF): `ftyp` with an image brand.
fn is_heif(bytes: &[u8]) -> bool {
    bytes.get(4..8) == Some(b"ftyp".as_slice())
        && bytes.get(8..12).is_some_and(|brand| {
            [b"heic", b"heix", b"hevc", b"hevx", b"heim", b"heis", b"mif1", b"msf1", b"avif", b"avis"].iter().any(|b| brand == b.as_slice())
        })
}

/// A mask layer (`MRst`): one channel of coverage as grey pixels (white shows, black hides), read
/// over `over` (document pixels) when given; outside its pixels a mask hides everything.
pub(crate) fn mask(r: &mut Reader, id: ObjId, world: Affine, over: Option<crate::model::Rect>) -> Result<Option<Image>, Error> {
    let s = r.s;
    let Some(bitmap) = s.obj(id, b"Bitm") else { return Ok(None) };
    let width = s.int(bitmap, b"BmpW").and_then(|v| u32::try_from(v).ok()).unwrap_or(0);
    let height = s.int(bitmap, b"BmpH").and_then(|v| u32::try_from(v).ok()).unwrap_or(0);
    if width == 0 || height == 0 || !matches!(s.enumeration(bitmap, b"Frmt"), Some((6 | 7, _))) {
        r.warn("pixel masks in an unknown format (imported without them)");
        return Ok(None);
    }
    let crop = match over.map(|o| crop_to(o, world, width, height)) {
        Some(Some(c)) => c,
        // What it masks lies outside it: all hidden, which one black pixel says as well.
        Some(None) => return Ok(Some(Image { width: 1, height: 1, pixels: Pixels::Rgba8(vec![0, 0, 0, 255]), transform: world })),
        None => (0, 0, width, height),
    };
    decode(r, bitmap, crop, world)
}

/// The bitmap pixels (x0, y0, x1, y1) of a `width` × `height` bitmap placed by `world` that cover
/// document rectangle `over`, one pixel wider all round; `None` if they don't overlap.
fn crop_to(over: crate::model::Rect, world: Affine, width: u32, height: u32) -> Option<(u32, u32, u32, u32)> {
    let inv = invert(world)?;
    let pts = [(over.x0, over.y0), (over.x1, over.y0), (over.x1, over.y1), (over.x0, over.y1)].map(|(x, y)| inv.apply(crate::model::Point { x, y }));
    let (mut x0, mut y0, mut x1, mut y1) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for p in pts {
        if !(p.x.is_finite() && p.y.is_finite()) {
            return Some((0, 0, width, height));
        }
        (x0, y0, x1, y1) = (x0.min(p.x), y0.min(p.y), x1.max(p.x), y1.max(p.y));
    }
    let clamp = |v: f64, max: u32| v.clamp(0.0, f64::from(max)) as u32;
    let (cx0, cy0, cx1, cy1) =
        (clamp(x0.floor() - 1.0, width), clamp(y0.floor() - 1.0, height), clamp(x1.ceil() + 1.0, width), clamp(y1.ceil() + 1.0, height));
    (cx1 > cx0 && cy1 > cy0).then_some((cx0, cy0, cx1, cy1))
}

fn invert(m: Affine) -> Option<Affine> {
    let [a, b, c, d, e, f] = m.0;
    let det = a * d - b * c;
    if !(det.is_finite() && det.abs() > 1e-12) {
        return None;
    }
    let (ia, ib, ic, id) = (d / det, -b / det, -c / det, a / det);
    Some(Affine([ia, ib, ic, id, -(ia * e + ic * f), -(ib * e + id * f)]))
}

/// An embedded document or symbol instance (`EmbN`), as the picture of it Affinity caches.
pub(crate) fn embedded(r: &mut Reader, id: ObjId, world: Affine) -> Result<Option<Image>, Error> {
    let s = r.s;
    let Some(cache) = s.obj(id, b"Bitm").and_then(|e| s.obj(e, b"Cach")) else { return Ok(None) };
    let Some(bbox) = s.obj(id, b"Bitm").and_then(|e| s.floats::<4>(e, b"ChBB")) else { return Ok(None) };
    let width = s.int(cache, b"BmpW").and_then(|v| u32::try_from(v).ok()).unwrap_or(0);
    let height = s.int(cache, b"BmpH").and_then(|v| u32::try_from(v).ok()).unwrap_or(0);
    if width == 0 || height == 0 {
        return Ok(None);
    }
    let [x0, y0, x1, y1] = bbox;
    // The instance's origin is the centre of the embedded content (checked against the
    // thumbnails of documents with embedded documents and SVG files).
    let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    let place = Affine([(x1 - x0) / f64::from(width), 0.0, 0.0, (y1 - y0) / f64::from(height), x0 - cx, y0 - cy]).then(world);
    decode(r, cache, (0, 0, width, height), place)
}

/// Decode the tiles of a bitmap inside `crop` (pixels), placed by `world` (bitmap pixels to document).
fn decode(r: &mut Reader, bitmap: ObjId, crop: (u32, u32, u32, u32), world: Affine) -> Result<Option<Image>, Error> {
    decode_within(r, bitmap, crop, world, MAX_PIXELS)
}

/// [`decode`] with at most `max_pixels` decoded pixels.
fn decode_within(r: &mut Reader, bitmap: ObjId, crop: (u32, u32, u32, u32), world: Affine, max_pixels: u64) -> Result<Option<Image>, Error> {
    let s = r.s;
    let (x0, y0, x1, y1) = crop;
    if x1 <= x0 || y1 <= y0 {
        return Ok(None);
    }
    let (w, h) = (x1 - x0, y1 - y0);
    // Larger layers (mostly masks of whole spreads) are read at a half, a quarter… of their
    // resolution: every `step`th pixel of every `step`th row.
    let mut step = 1u32;
    while u64::from(w.div_ceil(step)) * u64::from(h.div_ceil(step)) > max_pixels {
        step *= 2;
        if step > 1 << 12 {
            r.warn("pixel layers too large to read (left out)");
            return Ok(None);
        }
    }
    let format = s.enumeration(bitmap, b"Frmt").map(|(f, _)| f);
    let (channels, bps) = match format {
        Some(0) => (4, 1),
        Some(1) => (4, 2),
        Some(6) => (1, 1),
        Some(7) => (1, 2),
        Some(4) => {
            r.warn("CMYK pixel layers (converted to RGB without the document's colour profile)");
            (5, 1)
        }
        _ => {
            r.warn("pixel layers in grey, Lab or 32-bit formats");
            return Ok(None);
        }
    };
    let mut planes = Vec::with_capacity(channels);
    // Current .af documents can keep level-zero tiles in their embedded JPEG/PNG instead of
    // duplicating the pixels. State 5 is checked in the public Patchy embedded-jpeg fixture.
    // Support only the observed, same-size RGBA8 image with a zero bitmap origin; other
    // representations still receive the existing loss warning rather than guessing a mapping.
    let needs_source = (1..=channels).any(|c| matches!(s.field(bitmap, &tag(b"Sta", c)), Some(Value::Array(a)) if a.contains(&Value::UInt(5))));
    let source = if needs_source && format == Some(0) && s.int(bitmap, b"LInf") == Some(0) && s.int(bitmap, b"TInf") == Some(0) {
        if let Some(name) = s.entry(bitmap, b"Bckg") {
            let width = s.int(bitmap, b"BmpW").and_then(|v| u32::try_from(v).ok()).unwrap_or(0);
            let height = s.int(bitmap, b"BmpH").and_then(|v| u32::try_from(v).ok()).unwrap_or(0);
            match original(r, name).and_then(|bytes| source_pixels(&bytes, width, height)) {
                Ok(pixels) => Some((width as usize, pixels)),
                Err(Error::Limit(e)) => return Err(Error::Limit(e)),
                Err(_) => None,
            }
        } else {
            None
        }
    } else {
        None
    };
    // Tiles kept only in an original that can't be decoded here are left empty (transparent). Where
    // a tile's colour is in the original but its alpha is stored, as in CMYK layers, it would come
    // out opaque white instead: such a layer (a HEIC photo in a CMYK document) is left out, and said.
    if needs_source && source.is_none() && channels > 1 {
        let states = |c: usize| match s.field(bitmap, &tag(b"Sta", c)) {
            Some(Value::Array(a)) => a.clone(),
            _ => Vec::new(),
        };
        let alpha = states(channels);
        let blank = (1..channels).any(|c| states(c).iter().enumerate().any(|(i, v)| *v == Value::UInt(5) && alpha.get(i) != Some(&Value::UInt(5))));
        if blank {
            r.warn("placed images whose pixels are only in an original DesignCraft can't decode, such as HEIC photos (left out)");
            return Ok(None);
        }
    }
    if step > 1 {
        // Its stored tiles must fit what is left of the import's extraction budget.
        let stored: usize = (1..=channels).map(|c| tiles_in(s, bitmap, c, bps, (x0, y0, w, h)).len()).sum();
        if stored.saturating_mul(TILE_BYTES) > r.archive.remaining().max_total {
            r.warn("pixel layers too large to read (left out)");
            return Ok(None);
        }
        r.warn("pixel layers larger than 64 megapixels (read at a lower resolution)");
    }
    let mut cache = CachedPixels { tiles: HashMap::new(), source };
    for c in 1..=channels {
        planes.push(plane(r, bitmap, c, bps, (x0, y0, w, h), step, &mut cache)?);
    }
    let (w, h) = (w.div_ceil(step), h.div_ceil(step));
    let n = (w as usize) * (h as usize);
    let mut rgba = vec![0u8; n * 4];
    let sample = |p: &Vec<u8>, i: usize| -> f64 {
        if bps == 2 {
            f64::from(u16::from_le_bytes([p.get(2 * i).copied().unwrap_or(0), p.get(2 * i + 1).copied().unwrap_or(0)])) / 65535.0
        } else {
            f64::from(p.get(i).copied().unwrap_or(0)) / 255.0
        }
    };
    for (i, px) in rgba.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let v: Vec<f64> = planes.iter().map(|p| sample(p, i)).collect();
        let (r2, g, b, a) = if channels == 1 {
            (v[0], v[0], v[0], 1.0)
        } else if channels == 5 {
            let k = 1.0 - v[3];
            ((1.0 - v[0]) * k, (1.0 - v[1]) * k, (1.0 - v[2]) * k, v[4])
        } else {
            (v[0], v[1], v[2], v[3])
        };
        px.copy_from_slice(&[r2, g, b, a].map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8));
    }
    let k = f64::from(step);
    let transform = Affine([k, 0.0, 0.0, k, f64::from(x0), f64::from(y0)]).then(world);
    Ok(Some(Image { width: w, height: h, pixels: Pixels::Rgba8(rgba), transform }))
}

/// The original image file: entry `c/N` is a stream whose root (`Blck`) holds it as `Data`.
fn original(r: &mut Reader, name: &str) -> Result<Vec<u8>, Error> {
    let bytes = r.archive.read(name)?;
    let s = stream::parse(&bytes)?;
    let root = s.object(s.root).ok_or(Error::Malformed("embedded image"))?;
    match root.get(Tag::of(b"Data")) {
        Some(Value::Blob(b)) if !b.is_empty() => Ok(b.clone()),
        _ => Err(Error::Malformed("embedded image without data")),
    }
}

/// Decode the source under the same pixel budget as tiled layers, checking its actual
/// dimensions before allocating decoded pixels. Only JPEG and PNG sources are supported.
fn source_pixels(bytes: &[u8], width: u32, height: u32) -> Result<Vec<u8>, Error> {
    use image::{ImageFormat, ImageReader};
    use std::io::Cursor;

    if u64::from(width) * u64::from(height) > MAX_PIXELS {
        return Err(Error::Limit("embedded image pixels"));
    }
    let mut reader = ImageReader::new(Cursor::new(bytes)).with_guessed_format().map_err(|_| Error::Malformed("embedded image format"))?;
    if !matches!(reader.format(), Some(ImageFormat::Jpeg | ImageFormat::Png)) {
        return Err(Error::Unsupported("embedded source pixel format"));
    }
    let dimensions = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| Error::Malformed("embedded image format"))?
        .into_dimensions()
        .map_err(|_| Error::Malformed("embedded image dimensions"))?;
    if dimensions != (width, height) || width == 0 || height == 0 {
        return Err(Error::Malformed("embedded source and bitmap dimensions differ"));
    }
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(width);
    limits.max_image_height = Some(height);
    limits.max_alloc = Some(MAX_PIXELS * 8);
    reader.limits(limits);
    let decoded = reader.decode().map_err(|e| match e {
        image::ImageError::Limits(_) => Error::Limit("embedded image decoding"),
        _ => Error::Malformed("embedded image decoding"),
    })?;
    Ok(decoded.into_rgba8().into_raw())
}

fn tag(prefix: &[u8; 3], c: usize) -> [u8; 4] {
    [prefix[0], prefix[1], prefix[2], b'0' + c as u8]
}

/// Channel `c`'s tile grid: (tiles across, tiles down, each tile's state).
fn grid(s: &stream::Stream, bitmap: ObjId, c: usize, bps: usize) -> (usize, usize, Vec<u8>) {
    let width = s.int(bitmap, b"BmpW").and_then(|v| usize::try_from(v).ok()).unwrap_or(0);
    let height = s.int(bitmap, b"BmpH").and_then(|v| usize::try_from(v).ok()).unwrap_or(0);
    let tw = s.int(bitmap, &tag(b"TWi", c)).and_then(|v| usize::try_from(v).ok()).unwrap_or_else(|| width.saturating_mul(bps).div_ceil(TILE));
    let th = s.int(bitmap, &tag(b"THi", c)).and_then(|v| usize::try_from(v).ok()).unwrap_or_else(|| height.div_ceil(TILE));
    let states: Vec<u8> = match s.field(bitmap, &tag(b"Sta", c)) {
        Some(Value::Array(a)) => a.iter().map(|v| if let Value::UInt(u) = v { u8::try_from(*u).unwrap_or(255) } else { 255 }).collect(),
        _ => Vec::new(),
    };
    (tw, th, states)
}

/// The stored tiles (state 4) of channel `c` that overlap the crop, by their index in `Idx`.
fn tiles_in(s: &stream::Stream, bitmap: ObjId, c: usize, bps: usize, (x0, y0, w, h): (u32, u32, u32, u32)) -> Vec<usize> {
    let (tw, th, states) = grid(s, bitmap, c, bps);
    let (bx0, bx1) = (x0 as usize * bps, (x0 + w) as usize * bps);
    let (py0, py1) = (y0 as usize, (y0 + h) as usize);
    let mut out = Vec::new();
    let mut next = 0usize;
    for (t, state) in states.iter().enumerate().take(tw.saturating_mul(th)) {
        if *state != 4 {
            continue;
        }
        let (ox, oy) = ((t % tw.max(1)) * TILE, (t / tw.max(1)) * TILE);
        if !(ox >= bx1 || ox + TILE <= bx0 || oy >= py1 || oy + TILE <= py0) {
            out.push(next);
        }
        next += 1;
    }
    out
}

/// One channel of level 0, cropped to (x0, y0, w, h) pixels and taking every `step`th pixel of
/// every `step`th row; `bps` bytes per sample.
fn plane(
    r: &mut Reader,
    bitmap: ObjId,
    c: usize,
    bps: usize,
    (x0, y0, w, h): (u32, u32, u32, u32),
    step: u32,
    cache: &mut CachedPixels,
) -> Result<Vec<u8>, Error> {
    let CachedPixels { tiles: cache, source } = cache;
    let s = r.s;
    let (tw, th, states) = grid(s, bitmap, c, bps);
    let tiles = s.objs(bitmap, &tag(b"Idx", c));
    let at = Crop { bps, x0: x0 as usize, y0: y0 as usize, w: w as usize, h: h as usize, step: step.max(1) as usize };
    let row = at.w.div_ceil(at.step) * bps;
    let mut out = vec![0u8; row * at.h.div_ceil(at.step)];
    let (bx0, bx1) = (x0 as usize * bps, (x0 + w) as usize * bps);
    let (py0, py1) = (y0 as usize, (y0 + h) as usize);
    let mut next = 0usize;
    for (t, state) in states.iter().enumerate().take(tw.saturating_mul(th)) {
        let (ty, tx) = (t / tw.max(1), t % tw.max(1));
        let fill = match state {
            0 | 1 => None,
            2 => Some(0xFF),
            4 => {
                let Some(blck) = tiles.get(next).copied() else { return Err(Error::Malformed("missing pixel tile")) };
                next += 1;
                let (ox, oy) = (tx * TILE, ty * TILE);
                if ox >= bx1 || ox + TILE <= bx0 || oy >= py1 || oy + TILE <= py0 {
                    continue;
                }
                let name = s.entry(blck, b"Data").ok_or(Error::Malformed("pixel tile without data"))?.to_string();
                let read = |r: &mut Reader| -> Result<Vec<u8>, Error> {
                    let mut data = r.archive.read(&name)?;
                    // Older files wrap a tile in a one-field stream: header, then a 64 KiB blob.
                    if data.len() == TILE_BYTES + 22 && data.starts_with(b"\x00\xffKS") {
                        data = data.get(21..21 + TILE_BYTES).ok_or(Error::Malformed("pixel tile"))?.to_vec();
                    }
                    if data.len() != TILE_BYTES {
                        return Err(Error::Malformed("pixel tile size"));
                    }
                    Ok(data)
                };
                // A layer read at a lower resolution is too large to keep its tiles around.
                if at.step > 1 {
                    let tile = read(r)?;
                    copy(&mut out, &at, (ox, oy), |x, y| tile.get(y * TILE + x).copied().unwrap_or(0));
                    continue;
                }
                if !cache.contains_key(&name) {
                    let data = read(r)?;
                    cache.insert(name.clone(), data);
                }
                let tile = cache.get(&name).ok_or(Error::Malformed("pixel tile"))?;
                copy(&mut out, &at, (ox, oy), |x, y| tile.get(y * TILE + x).copied().unwrap_or(0));
                continue;
            }
            3 => {
                r.warn("32-bit pixel layers");
                None
            }
            5 => {
                if let Some((source_width, pixels)) = source.as_ref() {
                    let (ox, oy) = (tx * TILE, ty * TILE);
                    copy(&mut out, &at, (ox, oy), |x, y| pixels.get(((oy + y) * source_width + ox + x) * 4 + c - 1).copied().unwrap_or(0));
                    continue;
                }
                r.warn("pixel layers drawn from an embedded image (left empty)");
                None
            }
            _ => return Err(Error::Malformed("unknown pixel tile state")),
        };
        if let Some(v) = fill {
            copy(&mut out, &at, (tx * TILE, ty * TILE), |_, _| v);
        }
    }
    Ok(out)
}

/// Where a plane's pixels come from: the crop (pixels) and the sampling step.
struct Crop {
    bps: usize,
    x0: usize,
    y0: usize,
    w: usize,
    h: usize,
    step: usize,
}

/// Copy the overlap of a tile at byte column `ox`, row `oy` into the cropped plane, keeping every
/// `step`th pixel of every `step`th row.
fn copy(out: &mut [u8], c: &Crop, (ox, oy): (usize, usize), at: impl Fn(usize, usize) -> u8) {
    let row = c.w.div_ceil(c.step) * c.bps;
    let (bx0, bx1) = (c.x0 * c.bps, (c.x0 + c.w) * c.bps);
    for y in oy.max(c.y0)..(oy + TILE).min(c.y0 + c.h) {
        if !(y - c.y0).is_multiple_of(c.step) {
            continue;
        }
        let out_row = (y - c.y0) / c.step * row;
        for x in ox.max(bx0)..(ox + TILE).min(bx1) {
            let px = x / c.bps - c.x0;
            if !px.is_multiple_of(c.step) {
                continue;
            }
            if let Some(b) = out.get_mut(out_row + px / c.step * c.bps + x % c.bps) {
                *b = at(x - ox, y - oy);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Kind;
    use crate::synth::{self, F, Method, tag};

    #[test]
    fn heif_originals_are_recognised_by_their_brand() {
        assert!(is_heif(b"\0\0\0\x18ftypheic\0\0\0\0"));
        assert!(is_heif(b"\0\0\0\x1cftypavif"));
        assert!(!is_heif(b"\0\0\0\x18ftypisom"), "a video is not a photo");
        assert!(!is_heif(&png()));
        assert!(!is_heif(b"ftyp"));
    }

    fn png() -> Vec<u8> {
        let pixels = image::RgbaImage::from_fn(300, 2, |x, y| image::Rgba([(x % 251) as u8, (y * 31) as u8, 77, 255]));
        let mut bytes = std::io::Cursor::new(Vec::new());
        pixels.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
        bytes.into_inner()
    }

    /// An original synthetic mixed source/cache bitmap. It exercises reader recovery,
    /// not a claim that Affinity itself accepts the synthetic container.
    fn fixture(source: &[u8], origin: i32) -> Vec<u8> {
        fixture_in(source, origin, false)
    }

    /// `cmyk`: a CMYK bitmap whose colour tiles are all in the source and whose alpha is stored.
    fn fixture_in(source: &[u8], origin: i32, cmyk: bool) -> Vec<u8> {
        let channels = if cmyk { 5 } else { 4 };
        let mut bitmap = vec![
            (tag(b"Frmt"), F::Enum(if cmyk { 4 } else { 0 }, 0)),
            (tag(b"BmpW"), F::I32(300)),
            (tag(b"BmpH"), F::I32(2)),
            (tag(b"LInf"), F::I32(origin)),
            (tag(b"TInf"), F::I32(0)),
            (tag(b"Bckg"), F::Entry("c/1".into())),
        ];
        for c in 1..=channels {
            let states = match (cmyk, c) {
                (false, _) => vec![5, 4],
                (true, 5) => vec![4, 4],
                (true, _) => vec![5, 5],
            };
            bitmap.push((tag(&super::tag(b"TWi", c)), F::I32(2)));
            bitmap.push((tag(&super::tag(b"THi", c)), F::I32(1)));
            bitmap.push((tag(&super::tag(b"Sta", c)), F::U8s(states)));
            bitmap.push((
                tag(&super::tag(b"Idx", c)),
                F::Shared(vec![F::Def(c as u32, vec![tag(b"Blck")], vec![(tag(b"Data"), F::Entry(format!("d/{c}")))])]),
            ));
        }
        let mut crop = vec![0x17];
        crop.extend(tag(b"BitR").0.to_le_bytes());
        crop.extend([250i32, 0, 270, 2].into_iter().flat_map(i32::to_le_bytes));
        let raster = F::Def(
            10,
            vec![tag(b"Rstr")],
            vec![
                (tag(b"Bitm"), F::Obj(tag(b"DyBm"), bitmap)),
                (tag(b"BitR"), F::Raw(crop)),
                (tag(b"Xfrm"), F::F64s(vec![0.0, -2.0, 10.0, 2.0, 0.0, 20.0])),
            ],
        );
        let spread = F::Def(11, vec![tag(b"Sprd")], vec![(tag(b"Chld"), F::Shared(vec![raster]))]);
        let doc = synth::stream(&[(tag(b"DocR"), F::Obj(tag(b"DocN"), vec![(tag(b"Chld"), F::Shared(vec![spread]))]))]);
        let mut blob = vec![0x2d];
        blob.extend(tag(b"Data").0.to_le_bytes());
        blob.extend((source.len() as u32).to_le_bytes());
        blob.extend(source);
        let block = synth::stream(&[(tag(b"Data"), F::Raw(blob))]);
        let tiles: Vec<_> = [200u8, 10, 30, 255, 255].into_iter().map(|v| vec![v; TILE_BYTES]).collect();
        let names: Vec<String> = (1..=channels).map(|c| format!("d/{c}")).collect();
        let mut entries = vec![("doc.dat", doc.as_slice(), Method::Zlib), ("c/1", block.as_slice(), Method::Zlib)];
        for (name, tile) in names.iter().zip(&tiles) {
            entries.push((name.as_str(), tile.as_slice(), Method::Zlib));
        }
        synth::container(&entries, None)
    }

    #[test]
    fn a_cmyk_layer_whose_colours_are_only_in_an_unreadable_original_is_left_out_not_white() {
        let doc = crate::read(&fixture_in(b"\0\0\0\x18ftypheic not decodable", 0, true), crate::Limits::default()).unwrap();
        assert!(doc.warnings.iter().any(|w| w.starts_with("placed images whose pixels are only in an original")), "{:?}", doc.warnings);
        assert!(matches!(doc.spreads[0].nodes[0].kind, Kind::Unsupported), "{:?}", doc.spreads[0].nodes[0].kind);
    }

    #[test]
    fn layers_over_the_pixel_budget_are_read_at_a_lower_resolution() {
        let bytes = fixture(&png(), 0);
        let mut archive = crate::Archive::open(&bytes, crate::Limits::default()).unwrap();
        let doc = archive.read("doc.dat").unwrap();
        let s = stream::parse(&doc).unwrap();
        let bitmap = s.objects.iter().position(|o| o.class == Tag::of(b"DyBm")).unwrap();
        let mut r = Reader::new(&s, &mut archive, Affine::IDENTITY, 0);
        // 20 × 2 pixels over a budget of 10: every other pixel of every other row.
        let image = decode_within(&mut r, bitmap, (250, 0, 270, 2), Affine::IDENTITY, 10).unwrap().unwrap();
        assert_eq!((image.width, image.height), (10, 1));
        assert_eq!(image.transform, Affine([2.0, 0.0, 0.0, 2.0, 250.0, 0.0]), "each pixel covers two");
        let Pixels::Rgba8(pixels) = &image.pixels else { panic!() };
        for x in 0..10 {
            let sx = 2 * x;
            let expected = if sx + 250 < 256 { [((sx + 250) % 251) as u8, 0, 77, 255] } else { [200, 10, 30, 255] };
            assert_eq!(&pixels[x * 4..x * 4 + 4], &expected, "pixel {x}");
        }
        assert!(r.warnings.contains_key("pixel layers larger than 64 megapixels (read at a lower resolution)"));
        // Within budget, nothing changes.
        let image = decode_within(&mut r, bitmap, (250, 0, 270, 2), Affine::IDENTITY, 40).unwrap().unwrap();
        assert_eq!((image.width, image.height), (20, 2));
    }

    #[test]
    fn source_tiles_and_cached_edits_survive_crop_rotation_and_tile_boundaries() {
        let doc = crate::read(&fixture(&png(), 0), crate::Limits::default()).unwrap();
        assert_eq!(doc.warnings, ["edited placed images use their stored pixels"]);
        let Kind::Image(image) = &doc.spreads[0].nodes[0].kind else { panic!() };
        assert_eq!((image.width, image.height), (20, 2));
        assert_eq!(image.transform, Affine([0.0, 2.0, -2.0, 0.0, 10.0, 520.0]));
        let Pixels::Rgba8(pixels) = &image.pixels else { panic!() };
        for y in 0..2 {
            for x in 0..20 {
                let expected = if x + 250 < 256 { [((x + 250) % 251) as u8, (y * 31) as u8, 77, 255] } else { [200, 10, 30, 255] };
                assert_eq!(&pixels[(y * 20 + x) * 4..(y * 20 + x + 1) * 4], &expected);
            }
        }
    }

    #[test]
    fn unreadable_sources_and_unknown_origins_preserve_cached_edits_and_warn() {
        for bytes in [fixture(b"invalid image", 0), fixture(&png(), 1)] {
            let doc = crate::read(&bytes, crate::Limits::default()).unwrap();
            assert!(doc.warnings.iter().any(|w| w == "pixel layers drawn from an embedded image (left empty) (4×)"));
            let Kind::Image(image) = &doc.spreads[0].nodes[0].kind else { panic!() };
            let Pixels::Rgba8(pixels) = &image.pixels else { panic!() };
            assert_eq!(&pixels[..4], &[0, 0, 0, 0]);
            assert_eq!(&pixels[6 * 4..7 * 4], &[200, 10, 30, 255]);
        }
    }

    #[test]
    fn embedded_source_dimensions_and_pixel_budget_are_checked_before_decode() {
        let bytes = png();
        assert!(matches!(source_pixels(&bytes, 299, 2), Err(Error::Malformed(_))));
        assert!(matches!(source_pixels(&bytes, u32::MAX, u32::MAX), Err(Error::Limit(_))));
        assert_eq!(source_pixels(&bytes, 300, 2).unwrap().len(), 300 * 2 * 4);
    }
}
