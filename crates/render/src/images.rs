//! Decoded placed images, shared by every [`crate::Renderer`] in the process (canvas worker,
//! thumbnails, exports), so each asset is decoded once and each mip level built once.
//!
//! Entries are keyed by the address of the asset's encoded bytes (`Arc<Vec<u8>>`) and hold a
//! clone of that `Arc`, so the address can't be reused while cached. Least recently used levels
//! are evicted beyond a memory budget; at most `MAX_BAD` failed assets are remembered.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};

use vello_cpu::Pixmap;

use crate::decode::decode_sizes;
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
    let pm = decode_pixmap_page(bytes, page);
    #[cfg(test)]
    DECODED_ON.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push((id, std::thread::current().id()));
    match pm {
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

/// The decodes [`decoded`] ran: the address of the encoded bytes and the thread.
#[cfg(test)]
pub(crate) static DECODED_ON: Mutex<Vec<(usize, std::thread::ThreadId)>> = Mutex::new(Vec::new());

/// A placed image a render draws, as [`mip`] takes it: the encoded bytes, the PDF page, the
/// width (points) and the device pixels per point.
pub(crate) struct Want {
    pub bytes: Arc<Vec<u8>>,
    pub page: u32,
    pub width_pt: f64,
    pub on_screen: f64,
}

/// Threads [`warm`] decodes on (1 on the web, which has none).
fn workers() -> usize {
    #[cfg(target_arch = "wasm32")]
    {
        1
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::thread::available_parallelism().map_or(2, std::num::NonZero::get).clamp(2, 8)
    }
}

/// Whether [`warm`] can decode on several threads here.
pub(crate) fn can_warm() -> bool {
    workers() >= 2
}

/// Whether a decode of about `size` bytes may start while decodes of `in_flight` bytes run: when
/// none run, or when together they take at most half the budget.
fn admits(in_flight: usize, size: usize) -> bool {
    in_flight == 0 || in_flight.saturating_add(size) <= BUDGET / 2
}

/// Bytes [`warm`] is decoding at once in every render in the process (canvas worker, thumbnails,
/// exports), with the signal that some finished.
static GATE: (Mutex<usize>, Condvar) = (Mutex::new(0), Condvar::new());

/// Takes a finished decode's bytes off [`GATE`] and off its own call's count when dropped.
struct Release<'a>(&'a AtomicUsize, usize);

impl Drop for Release<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(self.1, Ordering::Relaxed);
        let mut g = GATE.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        *g = g.saturating_sub(self.1);
        drop(g);
        GATE.1.notify_all();
    }
}

/// Decode the images in `wants` that are not cached yet on several threads at once, with the
/// mip levels they are drawn at, so the render that draws them next finds them cached instead
/// of decoding them one after another. Does nothing when fewer than two images need decoding.
/// The images in view that are cached count against the cache budget first; a new image that
/// would take the total past what the cache keeps (by its pixel size) is left to the render.
/// Decodes wait while those in flight in every render together would take more than half the
/// budget. Returns the most bytes (estimated) this call had decoding at once.
pub(crate) fn warm(wants: Vec<Want>) -> usize {
    let workers = workers();
    if workers < 2 {
        return 0;
    }
    // What is drawn of each asset and page, in drawing order.
    let mut assets: Vec<((usize, u32), Vec<Want>)> = Vec::new();
    let mut index: HashMap<(usize, u32), usize> = HashMap::new();
    for w in wants {
        let key = (Arc::as_ptr(&w.bytes) as usize, w.page);
        match index.get(&key).and_then(|&i| assets.get_mut(i)) {
            Some(a) => a.1.push(w),
            None => {
                index.insert(key, assets.len());
                assets.push((key, vec![w]));
            }
        }
    }
    // The bytes each asset in view holds in the cache, when it is cached or failed, and the bytes
    // of everything else cached. Its levels become the most recently used, so the stores below
    // evict other images first.
    let (held, others) = with(|c| {
        c.clock += 1;
        let clock = c.clock;
        let mut held: HashMap<(usize, u32), usize> = HashMap::new();
        for (k, e) in c.map.iter_mut() {
            if index.contains_key(&(k.0, k.1)) {
                e.stamp = clock;
                *held.entry((k.0, k.1)).or_default() += size_of(&e.pm);
            }
        }
        let in_view = held.values().sum::<usize>();
        // Smaller levels alone don't make an asset cached.
        held.retain(|k, _| c.map.contains_key(&(k.0, k.1, 0)));
        for k in index.keys() {
            if c.bad.contains_key(k) {
                held.entry(*k).or_default();
            }
        }
        (held, c.bytes.saturating_sub(in_view))
    });
    // One image to decode gains nothing from threads (and is not worth sizing).
    if assets.iter().filter(|(key, _)| !held.contains_key(key)).count() < 2 {
        return 0;
    }
    let mut pixels = held.values().fold(0usize, |n, b| n.saturating_add(*b));
    let mut jobs: Vec<(usize, Vec<Want>)> = Vec::new();
    for (key, wanted) in assets {
        if held.contains_key(&key) {
            continue;
        }
        let (size, decoding) = wanted.first().map_or((0, 0), |w| decode_sizes(&w.bytes, w.page));
        // A store past the budget evicts down to three quarters of it: the images in view must
        // fit in that, unless they and the rest fit in the budget.
        let total = pixels.saturating_add(size);
        if total > BUDGET * 3 / 4 && others.saturating_add(total) > BUDGET {
            continue;
        }
        pixels = total;
        jobs.push((decoding, wanted));
    }
    if jobs.len() < 2 {
        return 0;
    }
    // While it converts the pixels, a decode holds the buffers `decode_sizes` counts: a
    // decode waits while those in flight in any render would take it past half the budget (one
    // always runs).
    let lock = || GATE.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let next = AtomicUsize::new(0);
    // This call's bytes decoding, and the most there were.
    let (mine, peak) = (AtomicUsize::new(0), AtomicUsize::new(0));
    std::thread::scope(|s| {
        for _ in 0..workers.min(jobs.len()) {
            let work = || {
                while let Some((size, job)) = jobs.get(next.fetch_add(1, Ordering::Relaxed)) {
                    let mut g = lock();
                    while !admits(*g, *size) {
                        #[cfg(test)]
                        waited(job);
                        g = GATE.1.wait(g).unwrap_or_else(std::sync::PoisonError::into_inner);
                    }
                    *g = g.saturating_add(*size);
                    let now = mine.fetch_add(*size, Ordering::Relaxed).saturating_add(*size);
                    peak.fetch_max(now, Ordering::Relaxed);
                    drop(g);
                    // Released even if the decode panics, so no other decode waits forever.
                    let _release = Release(&mine, *size);
                    for w in job {
                        let _ = mip(&w.bytes, w.page, w.width_pt, w.on_screen);
                    }
                }
            };
            // A thread that can't start leaves its share to the others, or to the render.
            if std::thread::Builder::new().name("image decode".into()).spawn_scoped(s, work).is_err() {
                break;
            }
        }
    });
    peak.into_inner()
}

/// The images whose decode waited at [`GATE`] (the address of the encoded bytes), with the
/// signal that one did.
#[cfg(test)]
static WAITED: (Mutex<Vec<usize>>, Condvar) = (Mutex::new(Vec::new()), Condvar::new());

#[cfg(test)]
fn waited(job: &[Want]) {
    let id = job.first().map_or(0, |w| Arc::as_ptr(&w.bytes) as usize);
    WAITED.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push(id);
    WAITED.1.notify_all();
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
    use crate::decode::decoded_bytes;

    fn decode_peak_bytes(bytes: &[u8], page: u32) -> usize {
        decode_sizes(bytes, page).1
    }

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

    fn png(side: u32, value: u8) -> Arc<Vec<u8>> {
        let mut out = Vec::new();
        let img = image::RgbaImage::from_pixel(side, side, image::Rgba([value, 0, 0, 255]));
        img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).unwrap();
        Arc::new(out)
    }

    fn want(bytes: &Arc<Vec<u8>>) -> Want {
        Want { bytes: bytes.clone(), page: 0, width_pt: 100.0, on_screen: 1.0 }
    }

    /// A PSD header claiming `side` pixels square (its pixels don't decode).
    fn psd(side: u32) -> Arc<Vec<u8>> {
        let mut psd = b"8BPS".to_vec();
        psd.resize(26, 0);
        psd[14..18].copy_from_slice(&side.to_be_bytes());
        psd[18..22].copy_from_slice(&side.to_be_bytes());
        Arc::new(psd)
    }

    /// Where the decode log ends now. Taken after the test's images are made: while they live,
    /// no other buffer has their address, so later entries with it are theirs.
    fn mark() -> usize {
        DECODED_ON.lock().unwrap().len()
    }

    /// The threads that decoded `bytes` after `since`.
    fn decoders(bytes: &Arc<Vec<u8>>, since: usize) -> Vec<std::thread::ThreadId> {
        let id = Arc::as_ptr(bytes) as usize;
        DECODED_ON.lock().unwrap().iter().skip(since).filter(|(a, _)| *a == id).map(|(_, t)| *t).collect()
    }

    #[test]
    fn warm_decodes_several_images_off_the_calling_thread() {
        let images: Vec<_> = (0..3u8).map(|i| png(8, 40 + i)).collect();
        let since = mark();
        // The same image drawn at two sizes is one decode.
        let mut wants: Vec<Want> = images.iter().map(want).collect();
        wants.push(Want { on_screen: 0.1, ..want(&images[0]) });
        warm(wants);
        let me = std::thread::current().id();
        for img in &images {
            let on = decoders(img, since);
            assert_eq!(on.len(), 1, "decoded once");
            assert_ne!(on[0], me, "decoded on a worker");
        }
    }

    #[test]
    fn warm_leaves_one_image_or_an_over_budget_one_to_the_render() {
        // A header claiming 100,000 pixels square is far past the budget.
        let (one, huge, a, b, c) = (png(8, 7), psd(100_000), png(8, 9), png(8, 11), png(8, 13));
        let since = mark();
        warm(vec![want(&one)]);
        assert!(decoders(&one, since).is_empty(), "one image: no threads");
        // The huge one, first or not, is left to the render; the small images beside it are
        // decoded ahead.
        warm(vec![want(&huge), want(&a), want(&b)]);
        assert!(decoders(&huge, since).is_empty(), "over budget: not decoded ahead");
        assert_eq!((decoders(&a, since).len(), decoders(&b, since).len()), (1, 1));
        // With the huge one and a single small image, nothing is left to do together.
        warm(vec![want(&c), want(&huge)]);
        assert!(decoders(&c, since).is_empty() && decoders(&huge, since).is_empty());
    }

    #[test]
    fn warm_decodes_large_images_one_at_a_time() {
        // Two 6,500 pixel square images fit the budget together, but decoding both at once would
        // take more than half of it; the small image still decodes beside them.
        let (d, e, small) = (psd(6500), psd(6500), png(8, 15));
        let since = mark();
        let large = decoded_bytes(&d, 0);
        assert!(large > BUDGET / 4 && 2 * large <= BUDGET * 3 / 4);
        let decoding = decode_peak_bytes(&d, 0);
        assert!(admits(0, decoding) && !admits(decoding, decoding) && admits(decoding, decode_peak_bytes(&small, 0)));
        assert!(admits(0, usize::MAX), "an image over half the budget still decodes alone");
        let peak = warm(vec![want(&d), want(&e), want(&small)]);
        assert!(peak <= BUDGET / 2, "{peak} bytes decoding at once");
        assert!(peak >= decoding, "the large images were decoded ahead");
        let me = std::thread::current().id();
        for img in [&d, &e, &small] {
            let on = decoders(img, since);
            assert!(on.len() == 1 && on[0] != me);
        }
    }

    #[test]
    fn warm_counts_a_decode_at_its_peak() {
        // A decode holds the RGBA copy beside the decoded or the premultiplied pixels: two 5,500
        // pixel square images would take less than half the budget cached, but more than half of
        // it while both decode, so they decode one at a time.
        let (f, g) = (psd(5500), psd(5500));
        let decoding = decode_peak_bytes(&f, 0);
        assert_eq!(decoding, 5500 * 5500 * 9, "9 bytes a pixel for a PSD");
        assert!(2 * decoded_bytes(&f, 0) <= BUDGET / 2 && !admits(decoding, decode_peak_bytes(&g, 0)));
        assert_eq!(warm(vec![want(&f), want(&g)]), decoding, "one decode at a time");
        // 16-bit RGBA decodes at 8 bytes a pixel before its RGBA copy.
        let mut deep = Vec::new();
        let img = image::ImageBuffer::<image::Rgba<u16>, Vec<u16>>::from_pixel(4, 4, image::Rgba([9, 0, 0, 65535]));
        img.write_to(&mut std::io::Cursor::new(&mut deep), image::ImageFormat::Png).unwrap();
        assert_eq!(decode_peak_bytes(&deep, 0), 16 * 12);
        assert_eq!(decode_peak_bytes(&png(8, 1), 0), 64 * 8);
    }

    #[test]
    fn warm_waits_for_decodes_in_other_renders() {
        // Another render's decodes take half the budget: this render's decodes wait for them.
        let (a, b) = (png(8, 17), png(8, 19));
        let ids = [Arc::as_ptr(&a) as usize, Arc::as_ptr(&b) as usize];
        let (since, decoded_since) = (WAITED.0.lock().unwrap().len(), mark());
        let other = BUDGET / 2;
        *GATE.0.lock().unwrap() += other;
        let done = std::sync::atomic::AtomicBool::new(false);
        let (waited, peak) = std::thread::scope(|s| {
            let render = s.spawn(|| {
                let peak = warm(vec![want(&a), want(&b)]);
                let w = WAITED.0.lock().unwrap();
                done.store(true, Ordering::SeqCst);
                drop(w);
                WAITED.1.notify_all();
                peak
            });
            let seen = |w: &Vec<usize>| w.iter().skip(since).any(|id| ids.contains(id));
            let mut w = WAITED.0.lock().unwrap();
            while !seen(&w) && !done.load(Ordering::SeqCst) {
                w = WAITED.1.wait(w).unwrap();
            }
            let waited = seen(&w);
            drop(w);
            *GATE.0.lock().unwrap() -= other;
            GATE.1.notify_all();
            (waited, render.join().unwrap())
        });
        assert!(waited, "decoded beside the other render's decodes");
        assert_eq!((decoders(&a, decoded_since).len(), decoders(&b, decoded_since).len()), (1, 1));
        assert!(peak > 0 && peak <= BUDGET / 2, "{peak} bytes decoding at once");
    }
}
