//! Shared, bounded decoded artwork. Small mip levels are looked up before full-resolution
//! pixels, so revisiting a page does not decode an original that has already been evicted.
//! Native hosts may also enable a disposable, content-addressed preview cache on disk.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};

use vello_cpu::Pixmap;

use crate::decode::decode_pixmap_reduced;
use crate::{decode_pixmap_page, halve};

const FULL_BUDGET: usize = 192 << 20;
const PREVIEW_BUDGET: usize = 96 << 20;
type Key = (usize, u32, u8);

struct Colour {
    generation: u64,
    cms: Arc<designcraft_color::cms::Cms>,
}
impl Colour {
    fn current() -> Self {
        Self { generation: designcraft_color::cms::generation(), cms: designcraft_color::cms::active() }
    }
    #[cfg(not(target_arch = "wasm32"))]
    fn is_current(&self) -> bool {
        self.generation == designcraft_color::cms::generation() && Arc::ptr_eq(&self.cms, &designcraft_color::cms::active())
    }
}

/// Failed assets remembered; the oldest is forgotten (and retried if drawn again) beyond this.
const MAX_BAD: usize = 64;

struct Entry {
    owner: Weak<Vec<u8>>,
    pm: Arc<Pixmap>,
    stamp: u64,
}

struct Shape {
    owner: Weak<Vec<u8>>,
    width: u32,
    height: u32,
}

#[derive(Clone, Copy, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    pub decoded_bytes: usize,
    pub full_decodes: u64,
    pub preview_hits: u64,
    pub disk_hits: u64,
}

#[derive(Default)]
struct Cache {
    map: HashMap<Key, Entry>,
    /// One current original may exceed the cache budget. Retaining it avoids decoding a
    /// large print image again on every full-quality frame; older originals still evict.
    current_original: Option<Key>,
    shapes: HashMap<(usize, u32), Shape>,
    clock: u64,
    /// Assets that failed to decode (don't retry every frame).
    /// Value: (insertion stamp, the encoded bytes keeping the address reserved).
    bad: HashMap<(usize, u32), (u64, Weak<Vec<u8>>)>,
    stats: Stats,
    /// The colour settings the pixels were converted with ([`designcraft_color::cms::generation`]).
    colour: u64,
    cms: Option<Arc<designcraft_color::cms::Cms>>,
}

impl Cache {
    /// Remember a failed asset, evicting the oldest failures beyond [`MAX_BAD`].
    fn mark_bad(&mut self, key: (usize, u32), bytes: &Arc<Vec<u8>>) {
        self.clock = self.clock.wrapping_add(1);
        self.bad.insert(key, (self.clock, Arc::downgrade(bytes)));
        while self.bad.len() > MAX_BAD {
            let Some(oldest) = self.bad.iter().min_by_key(|(_, (stamp, _))| *stamp).map(|(k, _)| *k) else { break };
            self.bad.remove(&oldest);
        }
    }
}

static CACHE: Mutex<Option<Cache>> = Mutex::new(None);
// Limit peak decode memory, including when the canvas and thumbnail workers miss together.
static DECODE: Mutex<()> = Mutex::new(());

fn size_of(pm: &Pixmap) -> usize {
    (pm.width() as usize).saturating_mul(pm.height() as usize).saturating_mul(4)
}

fn with<R>(f: impl FnOnce(&mut Cache) -> R) -> R {
    let mut g = CACHE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let c = g.get_or_insert_with(Cache::default);
    // Decoding converts CMYK through the working space: new colour settings, new pixels.
    let colour = designcraft_color::cms::generation();
    let cms = designcraft_color::cms::active();
    if c.colour != colour || c.cms.as_ref().is_none_or(|old| !Arc::ptr_eq(old, &cms)) {
        *c = Cache { clock: c.clock, colour, cms: Some(cms), ..Cache::default() };
    }
    f(c)
}

fn lookup(key: Key) -> Option<Arc<Pixmap>> {
    with(|c| {
        c.clock = c.clock.wrapping_add(1);
        let clock = c.clock;
        if key.2 == 0 && c.map.get(&key).is_some_and(|e| e.owner.strong_count() > 0) {
            c.current_original = Some(key);
        }
        c.map.get_mut(&key).filter(|e| e.owner.strong_count() > 0).map(|e| {
            e.stamp = clock;
            if key.2 > 0 {
                c.stats.preview_hits = c.stats.preview_hits.saturating_add(1);
            }
            e.pm.clone()
        })
    })
}

impl Cache {
    fn matches_colour(&self, colour: &Colour) -> bool {
        self.colour == colour.generation && self.cms.as_ref().is_some_and(|cms| Arc::ptr_eq(cms, &colour.cms))
    }
    fn store(&mut self, bytes: &Arc<Vec<u8>>, page: u32, level: u8, pm: Arc<Pixmap>, colour: &Colour) {
        // A decode can finish after Edit > Color Settings invalidated the cache. Its
        // pixels still belong to the old generation, not the new cache installed by with().
        if !self.matches_colour(colour) {
            return;
        }
        self.clock = self.clock.wrapping_add(1);
        self.map.insert((Arc::as_ptr(bytes) as usize, page, level), Entry { owner: Arc::downgrade(bytes), pm, stamp: self.clock });
        if level == 0 {
            self.current_original = Some((Arc::as_ptr(bytes) as usize, page, level));
        }
        self.prune(FULL_BUDGET, PREVIEW_BUDGET);
    }

    fn prune(&mut self, full: usize, previews: usize) {
        self.map.retain(|_, e| e.owner.strong_count() > 0);
        self.shapes.retain(|_, e| e.owner.strong_count() > 0);
        self.bad.retain(|_, e| e.1.strong_count() > 0);
        self.current_original = self.current_original.filter(|key| self.map.contains_key(key));
        for (original, budget) in [(true, full), (false, previews)] {
            let mut entries: Vec<_> = self.map.iter().filter(|(k, _)| (k.2 == 0) == original).map(|(k, e)| (e.stamp, *k, size_of(&e.pm))).collect();
            let mut size: usize = entries.iter().map(|e| e.2).fold(0usize, usize::saturating_add);
            entries.sort_unstable();
            for (_, key, n) in entries {
                if size <= budget {
                    break;
                }
                if original && self.current_original == Some(key) {
                    continue;
                }
                self.map.remove(&key);
                size = size.saturating_sub(n);
            }
        }
        self.stats.decoded_bytes = self.map.values().map(|e| size_of(&e.pm)).fold(0usize, usize::saturating_add);
    }
}

fn store(bytes: &Arc<Vec<u8>>, page: u32, level: u8, pm: Arc<Pixmap>, colour: &Colour) {
    with(|c| c.store(bytes, page, level, pm, colour));
}

/// Full-resolution pixels, loaded only when a visible graphic needs them. Exports can continue
/// using the original encoded data; screen previews never replace it.
pub fn decoded(bytes: &Arc<Vec<u8>>, page: u32) -> Option<Arc<Pixmap>> {
    let colour = Colour::current();
    let id = Arc::as_ptr(bytes) as usize;
    if let Some(pm) = lookup((id, page, 0)) {
        return Some(pm);
    }
    let _decode = DECODE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(pm) = lookup((id, page, 0)) {
        return Some(pm);
    }
    if with(|c| c.bad.get(&(id, page)).is_some_and(|w| w.1.strong_count() > 0)) {
        return None;
    }
    with(|c| c.stats.full_decodes = c.stats.full_decodes.saturating_add(1));
    match decode_pixmap_page(bytes, page) {
        Some(pm) => {
            let pm = Arc::new(pm);
            with(|c| {
                c.shapes.insert((id, page), Shape { owner: Arc::downgrade(bytes), width: pm.width() as u32, height: pm.height() as u32 });
            });
            store(bytes, page, 0, pm.clone(), &colour);
            Some(pm)
        }
        None => {
            with(|c| {
                if c.matches_colour(&colour) {
                    c.mark_bad((id, page), bytes);
                }
            });
            None
        }
    }
}

fn shape(bytes: &Arc<Vec<u8>>, page: u32) -> Option<(u32, u32)> {
    let key = (Arc::as_ptr(bytes) as usize, page);
    if let Some(s) = with(|c| c.shapes.get(&key).filter(|s| s.owner.strong_count() > 0).map(|s| (s.width, s.height))) {
        return Some(s);
    }
    // SVG can depend on document/system fonts. PDF uses the pinned renderer's own fonts;
    // query its exact screen dimensions before looking up persistent page previews.
    if designcraft_images::is_svg(bytes) {
        return None;
    }
    let (width, height) = if crate::is_pdf(bytes) { crate::pdf_raster_size(bytes, page)? } else { designcraft_images::pixel_size(bytes)? };
    if width == 0 || height == 0 || width > u16::MAX as u32 || height > u16::MAX as u32 {
        return None;
    }
    with(|c| {
        c.shapes.insert(key, Shape { owner: Arc::downgrade(bytes), width, height });
    });
    Some((width, height))
}

fn level_for(w: u32, h: u32, width_pt: f64, on_screen: f64) -> u8 {
    let ppt = w as f64 / width_pt.max(1e-6);
    let mut level = 0;
    while level < 8 && ppt / (1u64 << (level + 1)) as f64 >= on_screen.max(1e-6) * 1.2 && (w >> level) > 64 && (h >> level) > 64 {
        level += 1;
    }
    level
}

/// The closest mip above the requested display resolution. Look up the requested preview
/// first, then a larger cached mip, and decode the original only as a last resort.
pub fn mip(bytes: &Arc<Vec<u8>>, page: u32, width_pt: f64, on_screen: f64) -> Option<Arc<Pixmap>> {
    if !width_pt.is_finite() || !on_screen.is_finite() {
        return None;
    }
    let colour = Colour::current();
    let size = shape(bytes, page).or_else(|| decoded(bytes, page).map(|pm| (pm.width() as u32, pm.height() as u32)))?;
    let level = level_for(size.0, size.1, width_pt, on_screen);
    let id = Arc::as_ptr(bytes) as usize;
    if let Some(pm) = lookup((id, page, level)) {
        return Some(pm);
    }
    #[cfg(not(target_arch = "wasm32"))]
    let disk_path = (level > 0 && !designcraft_images::is_svg(bytes)).then(|| disk::path_for(bytes, page, level, &colour.cms)).flatten();
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(path) = &disk_path
        && let Some(pm) = disk::load_path(path, (size.0 >> level, size.1 >> level))
    {
        let pm = Arc::new(pm);
        with(|c| c.stats.disk_hits = c.stats.disk_hits.saturating_add(1));
        store(bytes, page, level, pm.clone(), &colour);
        return Some(pm);
    }
    let (mut current, mut pm) = (0, None);
    for candidate in (1..level).rev() {
        if let Some(found) = lookup((id, page, candidate)) {
            current = candidate;
            pm = Some(found);
            break;
        }
    }
    if pm.is_none()
        && lookup((id, page, 0)).is_none()
        && let Some(found) = reduced(bytes, page, level, size, &colour)
    {
        current = level;
        pm = Some(found);
    }
    let mut pm = pm.or_else(|| decoded(bytes, page))?;
    while current < level {
        current += 1;
        pm = match lookup((id, page, current)) {
            Some(found) => found,
            None => {
                let half = Arc::new(halve(&pm));
                store(bytes, page, current, half.clone(), &colour);
                half
            }
        };
    }
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(path) = &disk_path
        && colour.is_current()
    {
        // Keep the path captured before decoding: recomputing it here could name new
        // colour settings even though these pixels were converted with the old ones.
        disk::store_path(path, &pm);
    }
    Some(pm)
}

/// [`mip`]'s level for a JPEG, decoded at 1/2, 1/4 or 1/8 of its size straight from its blocks
/// and halved from there, so a screen-sized level needs no full-size decode. `None` when the
/// full size is drawn, for other formats and for JPEGs only a full decode reads (CMYK).
fn reduced(bytes: &Arc<Vec<u8>>, page: u32, level: u8, size: (u32, u32), colour: &Colour) -> Option<Arc<Pixmap>> {
    if page != 0 || !bytes.starts_with(&[0xFF, 0xD8]) {
        return None;
    }
    let id = Arc::as_ptr(bytes) as usize;
    if with(|c| c.bad.get(&(id, page)).is_some_and(|entry| entry.1.strong_count() > 0)) {
        return None;
    }
    if level == 0 {
        return None;
    }
    if let Some(pm) = lookup((id, page, level)) {
        return Some(pm);
    }
    let _decode = DECODE.lock().unwrap_or_else(|e| e.into_inner());
    let shrink = level.min(designcraft_images::MAX_REDUCED_SHRINK);
    let mut pm = match lookup((id, page, shrink)) {
        Some(p) => p,
        None => {
            // Cut to `halve`'s size (it rounds down, the decoder up), as the level a full
            // decode makes.
            let (sw, sh) = ((size.0 >> shrink).max(1), (size.1 >> shrink).max(1));
            let p = Arc::new(crop(decode_pixmap_reduced(bytes, shrink)?, sw, sh)?);
            store(bytes, page, shrink, p.clone(), colour);
            p
        }
    };
    for l in shrink + 1..=level {
        pm = match lookup((id, page, l)) {
            Some(p) => p,
            None => {
                let half = Arc::new(halve(&pm));
                store(bytes, page, l, half.clone(), colour);
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

/// Release unreferenced originals after a view has rendered; retain small previews for returns.
/// Pixmaps still in an active renderer remain alive until that render completes.
pub fn trim_originals() {
    with(|c| c.prune(FULL_BUDGET, PREVIEW_BUDGET));
}

pub fn stats() -> Stats {
    with(|c| {
        c.prune(FULL_BUDGET, PREVIEW_BUDGET);
        c.stats
    })
}

/// Drop memory caches. Disposable disk previews are independent of document state.
pub fn clear() {
    with(|c| *c = Cache::default());
}

#[cfg(not(target_arch = "wasm32"))]
#[path = "preview_disk.rs"]
mod disk;
#[cfg(not(target_arch = "wasm32"))]
pub use disk::set_directory as set_preview_directory;
#[cfg(not(target_arch = "wasm32"))]
pub use disk::set_enabled as set_preview_enabled;

#[cfg(test)]
mod preview_tests {
    use super::*;
    use std::io::Cursor;

    fn graphic() -> Arc<Vec<u8>> {
        let img = image::RgbaImage::from_fn(1024, 768, |x, y| image::Rgba([(x % 120) as u8, (y % 100) as u8, 30, 128]));
        let mut data = Cursor::new(Vec::new());
        img.write_to(&mut data, image::ImageFormat::Png).unwrap();
        Arc::new(data.into_inner())
    }

    #[test]
    fn cached_preview_survives_original_eviction_without_decoding_again() {
        let bytes = graphic();
        let id = Arc::as_ptr(&bytes) as usize;
        let preview = mip(&bytes, 0, 512.0, 0.25).unwrap();
        assert!(preview.width() < 1024);
        with(|c| {
            c.map.remove(&(id, 0, 0));
        });
        let same = mip(&bytes, 0, 512.0, 0.25).unwrap();
        assert!(Arc::ptr_eq(&preview, &same));
        assert!(lookup((id, 0, 0)).is_none(), "cached small preview must not recreate full pixels");
        let finer = mip(&bytes, 0, 512.0, 1.0).unwrap();
        assert!(finer.width() > preview.width(), "zooming in must still get a sharper image");
        let mut expected = decode_pixmap_page(&bytes, 0).unwrap();
        while expected.width() > preview.width() {
            expected = halve(&expected);
        }
        assert_eq!(preview.data(), expected.data(), "alpha and filtering remain identical");
    }

    #[test]
    fn closed_document_does_not_stay_alive_in_the_decoded_cache() {
        let bytes = graphic();
        let id = Arc::as_ptr(&bytes) as usize;
        let _ = mip(&bytes, 0, 512.0, 0.25);
        let weak = Arc::downgrade(&bytes);
        drop(bytes);
        assert!(weak.upgrade().is_none());
        with(|c| {
            c.prune(FULL_BUDGET, PREVIEW_BUDGET);
            assert!(!c.map.keys().any(|key| key.0 == id));
        });
    }

    #[test]
    fn trimming_keeps_a_current_high_resolution_graphic_within_budget() {
        let bytes = Arc::new(vec![19, 23]);
        let id = Arc::as_ptr(&bytes) as usize;
        let pm = Arc::new(Pixmap::new(3200, 3000));
        store(&bytes, 0, 0, pm.clone(), &Colour::current());
        trim_originals();
        assert!(
            lookup((id, 0, 0)).is_some_and(|cached| Arc::ptr_eq(&cached, &pm)),
            "panning at high zoom must not immediately discard a 36 MiB current image"
        );
    }

    #[test]
    fn budgets_evict_old_full_pixels_independently_of_previews() {
        let owner = Arc::new(vec![7]);
        let mut cache = Cache::default();
        for level in 0..3 {
            cache.map.insert((1, 0, level), Entry { owner: Arc::downgrade(&owner), pm: Arc::new(Pixmap::new(8, 8)), stamp: level as u64 });
        }
        cache.prune(0, 256);
        assert!(!cache.map.contains_key(&(1, 0, 0)));
        assert!(cache.map.contains_key(&(1, 0, 2)));
        assert_eq!(cache.stats.decoded_bytes, 256);
    }

    #[test]
    fn a_completed_decode_cannot_enter_a_new_colour_generation() {
        let bytes = Arc::new(vec![7]);
        let key = (Arc::as_ptr(&bytes) as usize, 0, 0);
        let old_pixels = Arc::new(Pixmap::new(8, 8));
        let cms = designcraft_color::cms::active();
        let mut cache = Cache { colour: 12, cms: Some(cms.clone()), ..Default::default() };
        let old = Colour { generation: 11, cms: cms.clone() };
        // A full decode or a mip prepared with generation 11 completes after the reset.
        for level in [0, 2] {
            cache.store(&bytes, 0, level, old_pixels.clone(), &old);
        }
        assert!(cache.map.is_empty());
        assert_eq!(cache.stats.decoded_bytes, 0);
        assert!(cache.current_original.is_none());
        let current = Arc::new(Pixmap::new(8, 8));
        let matching = Colour { generation: 12, cms };
        cache.store(&bytes, 0, 0, current.clone(), &matching);
        assert!(Arc::ptr_eq(&cache.map[&key].pm, &current));
        // Replacing a named profile can replace the active context without changing
        // the settings' names or their generation. Old work must still be refused.
        let replacement = Arc::new(designcraft_color::cms::Cms::default());
        let mut replaced = Cache { colour: 12, cms: Some(replacement.clone()), ..Default::default() };
        replaced.store(&bytes, 0, 0, old_pixels, &matching);
        assert!(replaced.map.is_empty());
        replaced.store(&bytes, 0, 0, current.clone(), &Colour { generation: 12, cms: replacement });
        assert!(Arc::ptr_eq(&replaced.map[&key].pm, &current));
    }

    #[test]
    fn oversized_current_original_survives_trim_and_preview_insertion() {
        // Use a tiny budget to reproduce a print original larger than FULL_BUDGET.
        let owner = Arc::new(vec![7]);
        let mut cache = Cache::default();
        let entry = |stamp| Entry { owner: Arc::downgrade(&owner), pm: Arc::new(Pixmap::new(8, 8)), stamp };
        cache.map.insert((1, 0, 0), entry(1));
        cache.map.insert((2, 0, 0), entry(2));
        cache.current_original = Some((2, 0, 0));
        let current = cache.map[&(2, 0, 0)].pm.clone();
        cache.prune(128, 256);
        assert!(!cache.map.contains_key(&(1, 0, 0)));
        assert!(Arc::ptr_eq(&cache.map[&(2, 0, 0)].pm, &current));
        cache.map.insert((2, 0, 1), entry(3));
        cache.prune(128, 256);
        assert!(Arc::ptr_eq(&cache.map[&(2, 0, 0)].pm, &current), "preview insertion and trimming must not evict the current original");
        cache.map.insert((3, 0, 0), entry(4));
        cache.current_original = Some((3, 0, 0));
        cache.prune(128, 256);
        assert!(!cache.map.contains_key(&(2, 0, 0)), "a different original replaces the oversized exception");
        assert!(cache.map.contains_key(&(3, 0, 0)));
        assert!(cache.map.contains_key(&(2, 0, 1)), "preview budget remains independent");
        drop(owner);
        cache.prune(128, 256);
        assert!(cache.map.is_empty());
        assert!(cache.current_original.is_none());
    }
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
