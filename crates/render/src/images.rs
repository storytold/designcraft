//! Decoded placed images, shared by every [`crate::Renderer`] in the process (canvas worker,
//! thumbnails, exports), so each asset is decoded once per size it is drawn at.
//!
//! Entries are keyed by the address of the asset's encoded bytes (`Arc<Vec<u8>>`) and hold a
//! clone of that `Arc`, so the address can't be reused while cached. Least recently used levels
//! are evicted beyond a memory budget; at most `MAX_BAD` failed assets are remembered.
//!
//! [`mip`] keeps only the level it was asked for, not the full-resolution decode it was built
//! from: a 100-megapixel photo drawn a few hundred pixels wide costs its proxy, not 400 MB, so a
//! spread of large photos fits the budget instead of evicting itself on every frame.
//! [`mip_now`] is the canvas's version: it never waits for a decode.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};

use vello_cpu::Pixmap;

use crate::{decode_pixmap_page, halve};

/// Decoded pixels kept (bytes).
const BUDGET: usize = 768 << 20;

/// Failed assets remembered; the oldest is forgotten (and retried if drawn again) beyond this.
const MAX_BAD: usize = 64;

/// Coarsest mip level.
const MAX_LEVEL: u8 = 8;

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
    /// Pixel size of each asset's full decode, so the level to draw is known without decoding
    /// again. The weak reference keeps the address reserved without keeping the bytes.
    dims: HashMap<(usize, u32), (Weak<Vec<u8>>, (u32, u32))>,
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

fn is_bad(id: usize, page: u32) -> bool {
    with(|c| c.bad.contains_key(&(id, page)))
}

/// The full-resolution pixel size of `bytes`, once it has been decoded.
fn dims_of(bytes: &Arc<Vec<u8>>, page: u32) -> Option<(u32, u32)> {
    let id = Arc::as_ptr(bytes) as usize;
    with(|c| c.dims.get(&(id, page)).filter(|(w, _)| w.strong_count() > 0).map(|(_, d)| *d))
}

/// Decode `bytes` at full resolution without caching the pixels (its size is remembered).
fn decode_full(bytes: &Arc<Vec<u8>>, page: u32) -> Option<Arc<Pixmap>> {
    let id = Arc::as_ptr(bytes) as usize;
    if is_bad(id, page) {
        return None;
    }
    // Decode outside the lock: other threads keep drawing cached images meanwhile.
    match decode_pixmap_page(bytes, page) {
        Some(pm) => {
            let size = (u32::from(pm.width()), u32::from(pm.height()));
            with(|c| {
                // Forget assets that are gone before adding one.
                if c.dims.len() >= 1024 {
                    c.dims.retain(|_, (w, _)| w.strong_count() > 0);
                }
                c.dims.insert((id, page), (Arc::downgrade(bytes), size));
            });
            Some(Arc::new(pm))
        }
        None => {
            with(|c| c.mark_bad((id, page), bytes));
            None
        }
    }
}

/// Full-resolution decode of `bytes` (cached).
pub fn decoded(bytes: &Arc<Vec<u8>>, page: u32) -> Option<Arc<Pixmap>> {
    let id = Arc::as_ptr(bytes) as usize;
    if let Some(pm) = lookup((id, page, 0)) {
        return Some(pm);
    }
    let pm = decode_full(bytes, page)?;
    store(bytes, page, 0, pm.clone());
    Some(pm)
}

/// The mip level of an image `size` pixels at full resolution closest above `on_screen` device
/// pixels per point when drawn `width_pt` points wide.
fn level_for(size: (u32, u32), width_pt: f64, on_screen: f64) -> u8 {
    let ppt = f64::from(size.0) / width_pt.max(1e-6);
    let (mut w, mut h, mut level) = (size.0, size.1, 0u8);
    while ppt / (1u64 << (level + 1)) as f64 >= on_screen * 1.2 && w > 64 && h > 64 && level < MAX_LEVEL {
        level += 1;
        w = (w / 2).max(1);
        h = (h / 2).max(1);
    }
    level
}

/// Halve `pm`, at level `from`, down to level `to`.
fn halve_to(mut pm: Arc<Pixmap>, from: u8, to: u8) -> Arc<Pixmap> {
    for _ in from..to {
        pm = Arc::new(halve(&pm));
    }
    pm
}

/// Level `level` of `bytes` from the cache: that level, or one built from the nearest finer
/// cached level (no decode).
fn cached_level(bytes: &Arc<Vec<u8>>, page: u32, level: u8) -> Option<Arc<Pixmap>> {
    let id = Arc::as_ptr(bytes) as usize;
    if let Some(pm) = lookup((id, page, level)) {
        return Some(pm);
    }
    let (from, finer) = (0..level).rev().find_map(|l| lookup((id, page, l)).map(|pm| (l, pm)))?;
    let pm = halve_to(finer, from, level);
    store(bytes, page, level, pm.clone());
    Some(pm)
}

/// The mip level of `bytes` closest above `on_screen` device pixels per point for a graphic
/// `width_pt` points wide. Only that level is cached, not the full decode it is built from.
pub fn mip(bytes: &Arc<Vec<u8>>, page: u32, width_pt: f64, on_screen: f64) -> Option<Arc<Pixmap>> {
    if let Some(size) = dims_of(bytes, page)
        && let Some(pm) = cached_level(bytes, page, level_for(size, width_pt, on_screen))
    {
        return Some(pm);
    }
    let full = decode_full(bytes, page)?;
    let level = level_for((u32::from(full.width()), u32::from(full.height())), width_pt, on_screen);
    let pm = halve_to(full, 0, level);
    store(bytes, page, level, pm.clone());
    Some(pm)
}

/// What [`mip_now`] has to draw.
pub enum Mip {
    /// The level asked for.
    Ready(Arc<Pixmap>),
    /// Still decoding; a coarser level to stretch in its place, if one is cached.
    Pending(Option<Arc<Pixmap>>),
    /// The image can't be decoded.
    Failed,
}

/// [`mip`] without waiting: a level that isn't cached is decoded on a background thread and
/// [`generation`] advances when it is ready (the [`set_waker`] callback runs too). On wasm it
/// decodes in place.
pub fn mip_now(bytes: &Arc<Vec<u8>>, page: u32, width_pt: f64, on_screen: f64) -> Mip {
    let id = Arc::as_ptr(bytes) as usize;
    if is_bad(id, page) {
        return Mip::Failed;
    }
    let level = dims_of(bytes, page).map(|size| level_for(size, width_pt, on_screen));
    if let Some(pm) = level.and_then(|l| cached_level(bytes, page, l)) {
        return Mip::Ready(pm);
    }
    if !background::queue(bytes, page, width_pt, on_screen) {
        return mip(bytes, page, width_pt, on_screen).map_or(Mip::Failed, Mip::Ready);
    }
    DEFERRED.with(|d| d.set(true));
    let coarser = level.and_then(|l| (l + 1..=MAX_LEVEL).find_map(|c| lookup((id, page, c))));
    Mip::Pending(coarser)
}

/// Advances each time a background decode queued by [`mip_now`] finishes.
pub fn generation() -> u64 {
    background::GENERATION.load(std::sync::atomic::Ordering::Acquire)
}

/// Called (from a decode thread) each time a background decode finishes, e.g. to request a
/// repaint.
pub fn set_waker(waker: impl Fn() + Send + Sync + 'static) {
    *background::WAKER.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Arc::new(waker));
}

thread_local! {
    /// Set when [`mip_now`] left an image to a background decode on this thread.
    static DEFERRED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Whether [`mip_now`] deferred an image on this thread since the last call (and reset).
pub(crate) fn take_deferred() -> bool {
    DEFERRED.with(|d| d.replace(false))
}

/// Drop every decoded image.
pub fn clear() {
    with(|c| {
        c.map.clear();
        c.bad.clear();
        c.dims.clear();
        c.bytes = 0;
    });
}

#[cfg(not(target_arch = "wasm32"))]
mod background {
    //! A few decode threads, newest request first: what was just scrolled into view decodes
    //! before what was scrolled past.

    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Arc, Condvar, Mutex, PoisonError};

    /// Decode threads. Each full decode of a large photo briefly holds several hundred MB.
    const THREADS: usize = 3;
    /// Requests kept waiting; the oldest are dropped beyond this (they are asked for again if
    /// they are still in view).
    const MAX_QUEUED: usize = 256;
    /// An image asked for again this soon after its decode finished was evicted at once: the
    /// view holds more than the cache. It is decoded in place, as without deferral, rather than
    /// in a background loop that never settles.
    const RECENT: std::time::Duration = std::time::Duration::from_secs(2);

    pub(super) static GENERATION: AtomicU64 = AtomicU64::new(0);
    #[allow(clippy::type_complexity)]
    pub(super) static WAKER: Mutex<Option<Arc<dyn Fn() + Send + Sync>>> = Mutex::new(None);

    struct Job {
        bytes: Arc<Vec<u8>>,
        page: u32,
        width_pt: f64,
        on_screen: f64,
    }

    impl Job {
        fn key(&self) -> (usize, u32) {
            (Arc::as_ptr(&self.bytes) as usize, self.page)
        }
    }

    struct Queue {
        jobs: VecDeque<Job>,
        /// Being decoded now (one per thread).
        busy: Vec<(usize, u32)>,
        /// The last decodes finished, newest last.
        done: VecDeque<((usize, u32), std::time::Instant)>,
        threads: usize,
    }

    static QUEUE: Mutex<Queue> = Mutex::new(Queue { jobs: VecDeque::new(), busy: Vec::new(), done: VecDeque::new(), threads: 0 });
    static READY: Condvar = Condvar::new();

    /// Ask for a background decode; false when no decode thread can run (decode in place).
    pub(super) fn queue(bytes: &Arc<Vec<u8>>, page: u32, width_pt: f64, on_screen: f64) -> bool {
        let job = Job { bytes: bytes.clone(), page, width_pt, on_screen };
        let key = job.key();
        let mut q = QUEUE.lock().unwrap_or_else(PoisonError::into_inner);
        if q.busy.contains(&key) {
            // Finishing it advances the generation; a different size is asked for then.
            return true;
        }
        if q.done.iter().any(|(k, at)| *k == key && at.elapsed() < RECENT) {
            return false;
        }
        q.jobs.retain(|j| j.key() != key);
        q.jobs.push_back(job);
        while q.jobs.len() > MAX_QUEUED {
            q.jobs.pop_front();
        }
        if q.threads < THREADS {
            if std::thread::Builder::new().name("designcraft-decode".into()).spawn(run).is_err() {
                q.jobs.retain(|j| j.key() != key);
                return q.threads > 0;
            }
            q.threads += 1;
        }
        READY.notify_one();
        true
    }

    fn run() {
        loop {
            let job = {
                let mut q = QUEUE.lock().unwrap_or_else(PoisonError::into_inner);
                loop {
                    if let Some(job) = q.jobs.pop_back() {
                        q.busy.push(job.key());
                        break job;
                    }
                    q = READY.wait(q).unwrap_or_else(PoisonError::into_inner);
                }
            };
            // A decoder panic must not take the thread with it.
            let key = job.key();
            let decoded = matches!(std::panic::catch_unwind(|| super::mip(&job.bytes, job.page, job.width_pt, job.on_screen)), Ok(Some(_)));
            {
                let mut q = QUEUE.lock().unwrap_or_else(PoisonError::into_inner);
                q.busy.retain(|k| *k != key);
                q.done.push_back((key, std::time::Instant::now()));
                while q.done.len() > 64 {
                    q.done.pop_front();
                }
            }
            // A failure is remembered (`Mip::Failed`); only new pixels need a render.
            if !decoded {
                continue;
            }
            GENERATION.fetch_add(1, Ordering::AcqRel);
            let waker = WAKER.lock().unwrap_or_else(PoisonError::into_inner).clone();
            if let Some(wake) = waker {
                wake();
            }
        }
    }
}

#[cfg(target_arch = "wasm32")]
mod background {
    use std::sync::atomic::AtomicU64;
    use std::sync::{Arc, Mutex};

    pub(super) static GENERATION: AtomicU64 = AtomicU64::new(0);
    #[allow(clippy::type_complexity)]
    pub(super) static WAKER: Mutex<Option<Arc<dyn Fn() + Send + Sync>>> = Mutex::new(None);

    /// No threads: always decode in place.
    pub(super) fn queue(_: &Arc<Vec<u8>>, _: u32, _: f64, _: f64) -> bool {
        false
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

    fn png(w: u32, h: u32) -> Arc<Vec<u8>> {
        let img = image::RgbaImage::from_pixel(w, h, image::Rgba([200, 100, 50, 255]));
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).unwrap();
        Arc::new(out.into_inner())
    }

    #[test]
    fn mip_keeps_only_the_level_drawn() {
        let bytes = png(1024, 512);
        let id = Arc::as_ptr(&bytes) as usize;
        // 1024 px across 128 pt at 1 px/pt: level 2 (256 px) is the closest above 1.2 px/pt.
        let pm = mip(&bytes, 0, 128.0, 1.0).unwrap();
        assert_eq!((pm.width(), pm.height()), (256, 128));
        assert!(lookup((id, 0, 2)).is_some());
        assert!(lookup((id, 0, 0)).is_none(), "the full decode is not kept");
        assert_eq!(dims_of(&bytes, 0), Some((1024, 512)));
        // A coarser level comes from the cached one (halving stops at 64 px tall).
        let small = mip(&bytes, 0, 128.0, 0.25).unwrap();
        assert_eq!((small.width(), small.height()), (128, 64));
    }

    #[test]
    fn mip_now_decodes_in_the_background() {
        let bytes = png(512, 512);
        let before = generation();
        let first = mip_now(&bytes, 0, 256.0, 1.0);
        assert!(matches!(first, Mip::Pending(None)));
        assert!(take_deferred());
        let mut ready = None;
        for _ in 0..500 {
            if let Mip::Ready(pm) = mip_now(&bytes, 0, 256.0, 1.0) {
                ready = Some(pm);
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(ready.map(|pm| pm.width()), Some(512));
        assert!(generation() > before);
    }

    #[test]
    fn mip_now_reports_undecodable_bytes() {
        let bytes = Arc::new(b"not an image".to_vec());
        assert!(matches!(mip_now(&bytes, 0, 100.0, 1.0), Mip::Pending(None)));
        let mut failed = false;
        for _ in 0..500 {
            if matches!(mip_now(&bytes, 0, 100.0, 1.0), Mip::Failed) {
                failed = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(failed);
    }
}
