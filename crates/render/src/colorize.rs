//! A swatch applied to a placed greyscale or bitmap image (#1094). A greyscale pixel becomes the
//! swatch at the pixel's ink coverage (its darkness), white staying opaque; a bitmap's black
//! takes the swatch and its white turns transparent, so the frame's fill shows through.

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::{Arc, Mutex, PoisonError};

use designcraft_color::swatch::apply_tint;
use designcraft_doc::{Document, Fill};
use designcraft_images::Tone;
use vello_cpu::Pixmap;
use vello_cpu::color::PremulRgba8;

/// The tone of each image file a swatch was applied to, by the address of its bytes (kept alive
/// so the address stays theirs). A GIF is decoded to know, so this is not done every frame.
fn tone_of(bytes: &Arc<Vec<u8>>) -> Option<Tone> {
    type Tones = HashMap<usize, (Arc<Vec<u8>>, Option<Tone>)>;
    const MAX: usize = 256;
    static TONES: Mutex<Option<Tones>> = Mutex::new(None);
    let key = Arc::as_ptr(bytes) as usize;
    if let Some((_, t)) = TONES.lock().unwrap_or_else(PoisonError::into_inner).as_ref().and_then(|m| m.get(&key)) {
        return *t;
    }
    let tone = designcraft_images::tone(bytes);
    let mut guard = TONES.lock().unwrap_or_else(PoisonError::into_inner);
    let map = guard.get_or_insert_with(HashMap::new);
    if map.len() >= MAX {
        map.clear();
    }
    map.insert(key, (bytes.clone(), tone));
    tone
}

/// `pm` (decoded from `bytes`) coloured by `fill`, cached per image and on-screen colour. `None`
/// when the image keeps its own colours: no swatch, one that no longer exists, or a colour image.
pub(crate) fn colorized(doc: &Document, fill: &Fill, bytes: &Arc<Vec<u8>>, pm: &Arc<Pixmap>) -> Option<Arc<Pixmap>> {
    if fill.is_none() {
        return None;
    }
    let color = doc.resolve_color(&fill.swatch, fill.tint)?;
    let tone = tone_of(bytes)?;
    // The swatch at each ink coverage as the screen shows it (or the plate being previewed).
    let lut: Vec<[u8; 4]> = (0..=255u8)
        .map(|i| {
            let c = crate::color_of(&apply_tint(color, f32::from(i) / 255.0), 1.0).to_rgba8();
            [c.r, c.g, c.b, c.a]
        })
        .collect();
    let mut h = DefaultHasher::new();
    lut.hash(&mut h);
    let key = (Arc::as_ptr(pm) as usize, tone, h.finish());

    // As for plate previews: source pixmaps are kept alive (their addresses are keys), new colour
    // settings start over, and the derived pixels have a memory budget.
    type Entry = (Arc<Pixmap>, Arc<Pixmap>);
    type Cache = (designcraft_color::cms::ColorSettings, HashMap<(usize, Tone, u64), Entry>, usize);
    const BUDGET: usize = 64 << 20;
    static CACHE: Mutex<Option<Cache>> = Mutex::new(None);
    let settings = designcraft_color::cms::active_settings();
    {
        let mut guard = CACHE.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some((previous, map, _)) = guard.as_ref() {
            if *previous == settings {
                if let Some((_, derived)) = map.get(&key) {
                    return Some(derived.clone());
                }
            } else {
                *guard = None;
            }
        }
    }
    let premul = |[r, g, b, _]: [u8; 4], a: u8| {
        let m = |v: u8| ((u16::from(v) * u16::from(a) + 127) / 255) as u8;
        PremulRgba8 { r: m(r), g: m(g), b: m(b), a }
    };
    let solid = lut.last().copied().unwrap_or([0, 0, 0, 255]);
    let data: Vec<PremulRgba8> = pm
        .data()
        .iter()
        .map(|p| {
            if p.a == 0 {
                return *p;
            }
            // The image is grey: its red is its grey level (premultiplied).
            let grey = (u16::from(p.r) * 255 / u16::from(p.a)).min(255);
            let coverage = 255 - grey;
            match tone {
                Tone::Gray => premul(lut.get(usize::from(coverage)).copied().unwrap_or(solid), p.a),
                Tone::Bitmap => premul(solid, ((coverage * u16::from(p.a) + 127) / 255) as u8),
            }
        })
        .collect();
    let out = Arc::new(Pixmap::from_parts(data, pm.width(), pm.height()));
    let bytes = (pm.width() as usize).saturating_mul(pm.height() as usize).saturating_mul(8);
    if bytes <= BUDGET {
        let mut guard = CACHE.lock().unwrap_or_else(PoisonError::into_inner);
        let (previous, map, used) = guard.get_or_insert_with(|| (settings.clone(), HashMap::new(), 0));
        if *previous != settings || used.saturating_add(bytes) > BUDGET {
            map.clear();
            *used = 0;
            *previous = settings;
        }
        if map.insert(key, (pm.clone(), out.clone())).is_none() {
            *used += bytes;
        }
    }
    Some(out)
}
