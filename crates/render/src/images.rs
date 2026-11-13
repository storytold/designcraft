//! Decoded placed images, shared by every [`crate::Renderer`] in the process (canvas worker,
//! thumbnails, exports), so each asset is decoded once and each mip level built once.
//!
//! Entries are keyed by the address of the asset's encoded bytes (`Arc<Vec<u8>>`) and hold a
//! clone of that `Arc`, so the address can't be reused while cached. Least recently used levels
//! are evicted beyond a memory budget; at most `MAX_BAD` failed assets are remembered.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use vello_cpu::Pixmap;

use crate::decode::decode_pixmap_reduced;
use crate::{decode_pixmap_page, halve};

/// Decoded pixels kept (bytes).
const BUDGET: usize = 768 << 20;

/// Failed assets remembered; the oldest is forgotten (and retried if drawn again) beyond this.
const MAX_BAD: usize = 64;

struct Entry {
    _bytes: Arc<Vec<u8>>,
    pm: Arc<Pixmap>,
    stamp: u64,
}

#[derive(Default)]
struct Cache {
    /// (bytes address, PDF page, mip level).
    map: HashMap<(usize, u32, u8), Entry>,
    bytes: usize,
    clock: u64,
    /// Assets that failed to decode (don't retry every frame).
    /// Value: (insertion stamp, the encoded bytes keeping the address reserved).
    bad: HashMap<(usize, u32), (u64, Arc<Vec<u8>>)>,
    /// The colour settings the pixels were converted with ([`designcraft_color::cms::generation`]).
    colour: u64,
}

impl Cache {
    /// Remember a failed asset, evicting the oldest failures beyond [`MAX_BAD`].
    fn mark_bad(&mut self, key: (usize, u32), bytes: &Arc<Vec<u8>>) {
        self.clock += 1;
        self.bad.insert(key, (self.clock, bytes.clone()));
        while self.bad.len() > MAX_BAD {
            let Some(oldest) = self.bad.iter().min_by_key(|(_, (stamp, _))| *stamp).map(|(k, _)| *k) else { break };
            self.bad.remove(&oldest);
        }
    }
}

static CACHE: Mutex<Option<Cache>> = Mutex::new(None);

fn size_of(pm: &Pixmap) -> usize {
    pm.width() as usize * pm.height() as usize * 4
}

fn with<R>(f: impl FnOnce(&mut Cache) -> R) -> R {
    let mut g = CACHE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let c = g.get_or_insert_with(Cache::default);
    // Decoding converts CMYK through the working space: new colour settings, new pixels.
    let colour = designcraft_color::cms::generation();
    if c.colour != colour {
        *c = Cache { clock: c.clock, colour, ..Cache::default() };
    }
    f(c)
}

fn lookup(key: (usize, u32, u8)) -> Option<Arc<Pixmap>> {
    with(|c| {
        c.clock += 1;
        let clock = c.clock;
        c.map.get_mut(&key).map(|e| {
            e.stamp = clock;
            e.pm.clone()
        })
    })
}

fn store(bytes: &Arc<Vec<u8>>, page: u32, level: u8, pm: Arc<Pixmap>) {
    with(|c| {
        c.clock += 1;
        let key = (Arc::as_ptr(bytes) as usize, page, level);
        let n = size_of(&pm);
        if let Some(old) = c.map.insert(key, Entry { _bytes: bytes.clone(), pm, stamp: c.clock }) {
            c.bytes -= size_of(&old.pm);
        }
        c.bytes += n;
        if c.bytes > BUDGET {
            let mut v: Vec<(u64, (usize, u32, u8))> = c.map.iter().map(|(k, e)| (e.stamp, *k)).collect();
            v.sort_unstable();
            for (_, k) in v {
                if c.bytes <= BUDGET * 3 / 4 {
                    break;
                }
                if k == key {
                    continue;
                }
                if let Some(e) = c.map.remove(&k) {
                    c.bytes -= size_of(&e.pm);
                }
            }
        }
    });
}

/// Full-resolution decode of `bytes` (cached).
pub fn decoded(bytes: &Arc<Vec<u8>>, page: u32) -> Option<Arc<Pixmap>> {
    let id = Arc::as_ptr(bytes) as usize;
    if let Some(pm) = lookup((id, page, 0)) {
        return Some(pm);
    }
    if with(|c| c.bad.contains_key(&(id, page))) {
        return None;
    }
    // Decode outside the lock: other threads keep drawing cached images meanwhile.
    match decode_pixmap_page(bytes, page) {
        Some(pm) => {
            let pm = Arc::new(pm);
            store(bytes, page, 0, pm.clone());
            Some(pm)
        }
        None => {
            with(|c| c.mark_bad((id, page), bytes));
            None
        }
    }
}

/// The mip level of `bytes` closest above `on_screen` device pixels per point for a graphic
/// `width_pt` points wide.
pub fn mip(bytes: &Arc<Vec<u8>>, page: u32, width_pt: f64, on_screen: f64) -> Option<Arc<Pixmap>> {
    if lookup((Arc::as_ptr(bytes) as usize, page, 0)).is_none()
        && let Some(pm) = reduced(bytes, page, width_pt, on_screen)
    {
        return Some(pm);
    }
    let full = decoded(bytes, page)?;
    let ppt = full.width() as f64 / width_pt.max(1e-6);
    let id = Arc::as_ptr(bytes) as usize;
    let mut pm = full;
    let mut level = 0u8;
    while ppt / (1u64 << (level + 1)) as f64 >= on_screen * 1.2 && pm.width() > 64 && pm.height() > 64 && level < 8 {
        level += 1;
        pm = match lookup((id, page, level)) {
            Some(p) => p,
            None => {
                let half = Arc::new(halve(&pm));
                store(bytes, page, level, half.clone());
                half
            }
        };
    }
    Some(pm)
}

/// [`mip`]'s level for a JPEG, decoded at 1/2, 1/4 or 1/8 of its size straight from its blocks
/// and halved from there, so a screen-sized level needs no full-size decode. `None` when the
/// full size is drawn, for other formats and for JPEGs only a full decode reads (CMYK).
fn reduced(bytes: &Arc<Vec<u8>>, page: u32, width_pt: f64, on_screen: f64) -> Option<Arc<Pixmap>> {
    if page != 0 || !bytes.starts_with(&[0xFF, 0xD8]) {
        return None;
    }
    let id = Arc::as_ptr(bytes) as usize;
    if with(|c| c.bad.contains_key(&(id, page))) {
        return None;
    }
    // The level `mip` picks from a full decode, with each level's size as `halve` gives it.
    let (mut w, mut h) = crate::image_size(bytes)?;
    let ppt = w as f64 / width_pt.max(1e-6);
    let mut sizes = vec![(w, h)];
    while ppt / (1u64 << sizes.len()) as f64 >= on_screen * 1.2 && w > 64 && h > 64 && sizes.len() <= 8 {
        (w, h) = ((w / 2).max(1), (h / 2).max(1));
        sizes.push((w, h));
    }
    let level = u8::try_from(sizes.len() - 1).ok()?;
    if level == 0 {
        return None;
    }
    if let Some(pm) = lookup((id, page, level)) {
        return Some(pm);
    }
    let shrink = level.min(designcraft_images::MAX_REDUCED_SHRINK);
    let mut pm = match lookup((id, page, shrink)) {
        Some(p) => p,
        None => {
            // Cut to `halve`'s size (it rounds down, the decoder up), as the level a full
            // decode makes.
            let (sw, sh) = sizes[usize::from(shrink)];
            let p = Arc::new(crop(decode_pixmap_reduced(bytes, shrink)?, sw, sh)?);
            store(bytes, page, shrink, p.clone());
            p
        }
    };
    for l in shrink + 1..=level {
        pm = match lookup((id, page, l)) {
            Some(p) => p,
            None => {
                let half = Arc::new(halve(&pm));
                store(bytes, page, l, half.clone());
                half
            }
        };
    }
    Some(pm)
}

/// The top-left `w` × `h` of `pm`; `None` when `pm` is smaller.
fn crop(pm: Pixmap, w: u32, h: u32) -> Option<Pixmap> {
    let (pw, ph) = (u32::from(pm.width()), u32::from(pm.height()));
    if (pw, ph) == (w, h) {
        return Some(pm);
    }
    if w > pw || h > ph || w == 0 || h == 0 {
        return None;
    }
    let (pw, w, h) = (pw as usize, w as usize, h as usize);
    let data: Vec<_> = pm.data().chunks_exact(pw).take(h).flat_map(|row| &row[..w]).copied().collect();
    Some(Pixmap::from_parts(data, u16::try_from(w).ok()?, u16::try_from(h).ok()?))
}

/// Drop every decoded image.
pub fn clear() {
    with(|c| {
        c.map.clear();
        c.bad.clear();
        c.bytes = 0;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_assets_are_capped_and_oldest_is_dropped() {
        let mut c = Cache::default();
        let buffers: Vec<Arc<Vec<u8>>> = (0..MAX_BAD + 10).map(|i| Arc::new(vec![i as u8; 4])).collect();
        for b in &buffers {
            c.mark_bad((Arc::as_ptr(b) as usize, 0), b);
        }
        assert_eq!(c.bad.len(), MAX_BAD);
        let first = &buffers[0];
        assert!(!c.bad.contains_key(&(Arc::as_ptr(first) as usize, 0)));
        assert_eq!(Arc::strong_count(first), 1, "evicted buffer must be released");
        let last = &buffers[MAX_BAD + 9];
        assert!(c.bad.contains_key(&(Arc::as_ptr(last) as usize, 0)));
    }

    fn jpeg(w: u32, h: u32) -> Arc<Vec<u8>> {
        let img = image::RgbImage::from_fn(w, h, |x, y| image::Rgb([(x * 255 / w) as u8, (y * 255 / h) as u8, 128]));
        let mut out = Vec::new();
        img.write_with_encoder(image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 90)).unwrap();
        Arc::new(out)
    }

    #[test]
    fn screen_levels_of_a_jpeg_need_no_full_size_decode() {
        let bytes = jpeg(1024, 768);
        let id = Arc::as_ptr(&bytes) as usize;
        // 1 px per point drawn at 0.1: level 3, an eighth, read straight from the blocks.
        let pm = mip(&bytes, 0, 1024.0, 0.1).unwrap();
        assert_eq!((pm.width(), pm.height()), (128, 96));
        assert!(lookup((id, 0, 0)).is_none(), "no full-size decode is kept");
        // Close to the full decode halved three times.
        let mut want = decode_pixmap_page(&bytes, 0).unwrap();
        for _ in 0..3 {
            want = halve(&want);
        }
        let diff: u64 = pm.data().iter().zip(want.data()).map(|(p, q)| u64::from(p.r.abs_diff(q.r)) + u64::from(p.g.abs_diff(q.g))).sum();
        assert!(diff / (2 * 128 * 96) <= 3, "mean difference {}", diff / (2 * 128 * 96));
        // Smaller levels are halved from that eighth, down to 64 px as before.
        let small = mip(&bytes, 0, 1024.0, 0.02).unwrap();
        assert_eq!((small.width(), small.height()), (64, 48));
        assert!(lookup((id, 0, 0)).is_none(), "no full-size decode is kept");
        // The full size is still a full decode.
        let full = mip(&bytes, 0, 1024.0, 1.0).unwrap();
        assert_eq!((full.width(), full.height()), (1024, 768));
    }

    #[test]
    fn reduced_levels_of_odd_sized_jpegs_have_the_halved_size() {
        let bytes = jpeg(1001, 771);
        let mut want = decode_pixmap_page(&bytes, 0).unwrap();
        for _ in 0..3 {
            want = halve(&want);
        }
        let pm = mip(&bytes, 0, 1001.0, 0.1).unwrap();
        assert!(lookup((Arc::as_ptr(&bytes) as usize, 0, 0)).is_none());
        assert_eq!((pm.width(), pm.height()), (want.width(), want.height()));
        assert_eq!((pm.width(), pm.height()), (125, 96));
    }

    #[test]
    fn a_jpeg_that_does_not_decode_is_remembered_as_failed() {
        let mut broken = jpeg(256, 256).to_vec();
        broken.truncate(40);
        let bytes = Arc::new(broken);
        assert!(mip(&bytes, 0, 256.0, 0.1).is_none());
        assert!(with(|c| c.bad.contains_key(&(Arc::as_ptr(&bytes) as usize, 0))));
        assert!(mip(&bytes, 0, 256.0, 0.1).is_none());
    }
}
