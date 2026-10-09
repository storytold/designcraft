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

/// A mask layer (`MRst`): one channel of coverage as grey pixels (white shows, black hides).
pub(crate) fn mask(r: &mut Reader, id: ObjId, world: Affine) -> Result<Option<Image>, Error> {
    let s = r.s;
    let Some(bitmap) = s.obj(id, b"Bitm") else { return Ok(None) };
    let width = s.int(bitmap, b"BmpW").and_then(|v| u32::try_from(v).ok()).unwrap_or(0);
    let height = s.int(bitmap, b"BmpH").and_then(|v| u32::try_from(v).ok()).unwrap_or(0);
    if width == 0 || height == 0 || !matches!(s.enumeration(bitmap, b"Frmt"), Some((6 | 7, _))) {
        r.warn("pixel masks in an unknown format (imported without them)");
        return Ok(None);
    }
    decode(r, bitmap, (0, 0, width, height), world)
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
    let s = r.s;
    let (x0, y0, x1, y1) = crop;
    if x1 <= x0 || y1 <= y0 {
        return Ok(None);
    }
    let (w, h) = (x1 - x0, y1 - y0);
    if u64::from(w) * u64::from(h) > MAX_PIXELS {
        r.warn("pixel layers larger than 64 megapixels");
        return Ok(None);
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
    let mut cache = CachedPixels { tiles: HashMap::new(), source };
    for c in 1..=channels {
        planes.push(plane(r, bitmap, c, bps, (x0, y0, w, h), &mut cache)?);
    }
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
    let transform = Affine([1.0, 0.0, 0.0, 1.0, f64::from(x0), f64::from(y0)]).then(world);
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

/// One channel of level 0, cropped to (x0, y0, w, h) pixels; `bps` bytes per sample.
fn plane(
    r: &mut Reader,
    bitmap: ObjId,
    c: usize,
    bps: usize,
    (x0, y0, w, h): (u32, u32, u32, u32),
    cache: &mut CachedPixels,
) -> Result<Vec<u8>, Error> {
    let CachedPixels { tiles: cache, source } = cache;
    let s = r.s;
    let width = s.int(bitmap, b"BmpW").and_then(|v| usize::try_from(v).ok()).unwrap_or(0);
    let height = s.int(bitmap, b"BmpH").and_then(|v| usize::try_from(v).ok()).unwrap_or(0);
    let tw = s.int(bitmap, &tag(b"TWi", c)).and_then(|v| usize::try_from(v).ok()).unwrap_or_else(|| (width * bps).div_ceil(TILE));
    let th = s.int(bitmap, &tag(b"THi", c)).and_then(|v| usize::try_from(v).ok()).unwrap_or_else(|| height.div_ceil(TILE));
    let states: Vec<u8> = match s.field(bitmap, &tag(b"Sta", c)) {
        Some(Value::Array(a)) => a.iter().map(|v| if let Value::UInt(u) = v { u8::try_from(*u).unwrap_or(255) } else { 255 }).collect(),
        _ => Vec::new(),
    };
    let tiles = s.objs(bitmap, &tag(b"Idx", c));
    let row = (w as usize) * bps;
    let mut out = vec![0u8; row * h as usize];
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
                if !cache.contains_key(&name) {
                    let mut data = r.archive.read(&name)?;
                    // Older files wrap a tile in a one-field stream: header, then a 64 KiB blob.
                    if data.len() == TILE_BYTES + 22 && data.starts_with(b"\x00\xffKS") {
                        data = data.get(21..21 + TILE_BYTES).ok_or(Error::Malformed("pixel tile"))?.to_vec();
                    }
                    if data.len() != TILE_BYTES {
                        return Err(Error::Malformed("pixel tile size"));
                    }
                    cache.insert(name.clone(), data);
                }
                let tile = cache.get(&name).ok_or(Error::Malformed("pixel tile"))?;
                copy(&mut out, row, (bx0, py0, bx1, py1), (ox, oy), |x, y| tile.get(y * TILE + x).copied().unwrap_or(0));
                continue;
            }
            3 => {
                r.warn("32-bit pixel layers");
                None
            }
            5 => {
                if let Some((source_width, pixels)) = source.as_ref() {
                    let (ox, oy) = (tx * TILE, ty * TILE);
                    copy(&mut out, row, (bx0, py0, bx1, py1), (ox, oy), |x, y| {
                        pixels.get(((oy + y) * source_width + ox + x) * 4 + c - 1).copied().unwrap_or(0)
                    });
                    continue;
                }
                r.warn("pixel layers drawn from an embedded image (left empty)");
                None
            }
            _ => return Err(Error::Malformed("unknown pixel tile state")),
        };
        if let Some(v) = fill {
            copy(&mut out, row, (bx0, py0, bx1, py1), (tx * TILE, ty * TILE), |_, _| v);
        }
    }
    Ok(out)
}

/// Copy the overlap of a tile at byte column `ox`, row `oy` into the cropped plane.
fn copy(out: &mut [u8], row: usize, (bx0, py0, bx1, py1): (usize, usize, usize, usize), (ox, oy): (usize, usize), at: impl Fn(usize, usize) -> u8) {
    for y in oy.max(py0)..(oy + TILE).min(py1) {
        for x in ox.max(bx0)..(ox + TILE).min(bx1) {
            if let Some(b) = out.get_mut((y - py0) * row + (x - bx0)) {
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

    fn png() -> Vec<u8> {
        let pixels = image::RgbaImage::from_fn(300, 2, |x, y| image::Rgba([(x % 251) as u8, (y * 31) as u8, 77, 255]));
        let mut bytes = std::io::Cursor::new(Vec::new());
        pixels.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
        bytes.into_inner()
    }

    /// An original synthetic mixed source/cache bitmap. It exercises reader recovery,
    /// not a claim that Affinity itself accepts the synthetic container.
    fn fixture(source: &[u8], origin: i32) -> Vec<u8> {
        let mut bitmap = vec![
            (tag(b"Frmt"), F::Enum(0, 0)),
            (tag(b"BmpW"), F::I32(300)),
            (tag(b"BmpH"), F::I32(2)),
            (tag(b"LInf"), F::I32(origin)),
            (tag(b"TInf"), F::I32(0)),
            (tag(b"Bckg"), F::Entry("c/1".into())),
        ];
        for c in 1..=4 {
            bitmap.push((tag(&super::tag(b"TWi", c)), F::I32(2)));
            bitmap.push((tag(&super::tag(b"THi", c)), F::I32(1)));
            bitmap.push((tag(&super::tag(b"Sta", c)), F::U8s(vec![5, 4])));
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
        let tiles: Vec<_> = [200u8, 10, 30, 255].into_iter().map(|v| vec![v; TILE_BYTES]).collect();
        synth::container(
            &[
                ("doc.dat", &doc, Method::Zlib),
                ("c/1", &block, Method::Zlib),
                ("d/1", &tiles[0], Method::Zlib),
                ("d/2", &tiles[1], Method::Zlib),
                ("d/3", &tiles[2], Method::Zlib),
                ("d/4", &tiles[3], Method::Zlib),
            ],
            None,
        )
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
