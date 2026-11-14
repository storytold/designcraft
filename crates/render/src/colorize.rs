//! A swatch applied to a placed greyscale or bitmap image (#1094). A greyscale pixel becomes the
//! swatch at the pixel's ink coverage (its darkness), white staying opaque; a bitmap's black
//! takes the swatch and its white turns transparent, so the frame's fill shows through.

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::{Arc, Mutex, PoisonError};

use designcraft_color::swatch::apply_tint;
use designcraft_doc::{AssetData, Document, Fill, SourceKey};
use designcraft_images::Tone;
use vello_cpu::Pixmap;
use vello_cpu::color::PremulRgba8;

use crate::prepare::Prepared;

type Tones = HashMap<SourceKey, Option<Tone>>;
/// Tones remembered; all are forgotten beyond this.
const MAX_TONES: usize = 256;
/// The tone of each image file a swatch was applied to. A GIF is decoded to know, so this is
/// not done every frame.
static TONES: Mutex<Option<Tones>> = Mutex::new(None);

fn known_tone(key: SourceKey) -> Option<Option<Tone>> {
    TONES.lock().unwrap_or_else(PoisonError::into_inner).as_ref().and_then(|m| m.get(&key).copied())
}

/// Find the tone of `bytes` (named by `key`, outside the lock) and remember it.
fn find_tone(key: SourceKey, bytes: &[u8]) -> Option<Tone> {
    let tone = designcraft_images::tone(bytes);
    let mut guard = TONES.lock().unwrap_or_else(PoisonError::into_inner);
    let map = guard.get_or_insert_with(HashMap::new);
    if map.len() >= MAX_TONES {
        map.clear();
    }
    map.insert(key, tone);
    tone
}

/// The tone of `data`. With `wait` it is found here (reading a linked file); without, in the
/// background. A linked file that can't be had isn't asked for again until the links change
/// ([`crate::images::unavailable`]).
fn tone_of(data: &AssetData, wait: bool) -> Prepared<Option<Tone>> {
    let key = data.key();
    if let Some(t) = known_tone(key) {
        return Prepared::Ready(t);
    }
    if crate::images::unavailable(data).is_some() {
        return Prepared::Failed;
    }
    if wait {
        return crate::images::load_bytes(data, false).map_or(Prepared::Failed, |bytes| Prepared::Ready(find_tone(key, &bytes)));
    }
    let source = data.clone();
    let queued = crate::prepare::later(crate::prepare::job_key(("tone", key)), move || {
        if let Some(bytes) = crate::images::load_bytes(&source, true) {
            find_tone(key, &bytes);
        }
    });
    match queued {
        crate::prepare::Queued::Yes => Prepared::Pending,
        crate::prepare::Queued::Ran => known_tone(key).map_or(Prepared::Failed, Prepared::Ready),
    }
}

/// `pm` (decoded from `data`) coloured by `fill`, cached per image and on-screen colour; `None`
/// when the image keeps its own colours: no swatch, one that no longer exists, a colour image,
/// or one whose file can't be had. Without `wait`, an image whose tone isn't known yet is
/// [`Prepared::Pending`] while it is found in the background.
pub(crate) fn colorized(doc: &Document, fill: &Fill, data: &AssetData, pm: &Arc<Pixmap>, wait: bool) -> Prepared<Option<Arc<Pixmap>>> {
    if fill.is_none() {
        return Prepared::Ready(None);
    }
    let Some(color) = doc.resolve_color(&fill.swatch, fill.tint) else { return Prepared::Ready(None) };
    match tone_of(data, wait) {
        Prepared::Ready(Some(tone)) => Prepared::Ready(Some(coloured(color, tone, pm))),
        Prepared::Ready(None) | Prepared::Failed => Prepared::Ready(None),
        Prepared::Pending => Prepared::Pending,
    }
}

/// `pm`, a `tone` image, coloured `color`.
fn coloured(color: designcraft_color::Color, tone: Tone, pm: &Arc<Pixmap>) -> Arc<Pixmap> {
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
                    return derived.clone();
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
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::images::tests::linked;

    /// A linked image whose file is missing is read once for its tone, not on every frame; a
    /// frame that doesn't wait finds the tone in the background.
    #[test]
    fn a_tone_is_found_off_the_drawing_thread_and_a_missing_file_is_read_once() {
        let generation = designcraft_doc::asset_data::link_generation();
        let missing = linked::file("mem:/missing-tone.png", None, 100);
        for _ in 0..5 {
            assert!(matches!(tone_of(&missing, true), Prepared::Failed));
            assert!(matches!(tone_of(&missing, false), Prepared::Failed));
        }
        if designcraft_doc::asset_data::link_generation() == generation {
            assert_eq!(linked::reads("mem:/missing-tone.png"), 1);
        }
        let mut grey = Vec::new();
        image::GrayImage::from_pixel(4, 4, image::Luma([128])).write_to(&mut std::io::Cursor::new(&mut grey), image::ImageFormat::Png).unwrap();
        let data = linked::file("mem:/grey-tone.png", Some(grey.clone()), grey.len() as u64);
        let mut tone = None;
        for _ in 0..500 {
            match tone_of(&data, false) {
                Prepared::Ready(t) => {
                    tone = t;
                    break;
                }
                Prepared::Pending => std::thread::sleep(std::time::Duration::from_millis(10)),
                Prepared::Failed => break,
            }
        }
        assert_eq!(tone, designcraft_images::tone(&grey));
        assert!(tone.is_some());
    }
}
