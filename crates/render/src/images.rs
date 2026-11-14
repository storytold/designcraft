//! Decoded placed images, shared by every [`crate::Renderer`] in the process (canvas worker,
//! thumbnails, exports), so each asset is decoded once per size it is drawn at.
//!
//! Entries are keyed by the [`SourceKey`] of the asset's bytes and don't hold the bytes: decoded
//! levels stay usable after the encoded bytes are dropped. Least recently used levels are
//! evicted beyond a memory budget; at most `MAX_BAD` failed assets are remembered.
//!
//! [`mip`] keeps only the level it was asked for, not the full-resolution decode it was built
//! from: a 12-megapixel photo drawn a few hundred pixels wide costs its proxy, not 48 MB, so a
//! book of photos fits the budget instead of evicting itself on every edit. [`mip_now`] is the
//! canvas's version: it never waits for a decode.
//!
//! Linked bytes are loaded ([`AssetData::load`]) only to decode a level that isn't cached, and
//! on the canvas only on a decode thread. A linked file that can't be had ([`Unavailable`]) is
//! remembered apart from failed decodes: it is asked for again once
//! [`designcraft_doc::asset_data::link_generation`] has advanced (a relink, an update).

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};

use designcraft_doc::{AssetData, SourceKey, Unavailable};
use vello_cpu::Pixmap;

use crate::decode::{decode_pixmap_reduced, decode_sizes, drawn_pixels};
use crate::{decode_pixmap_page, halve};

/// Decoded pixels kept (bytes).
const BUDGET: usize = 768 << 20;

/// Failed assets remembered; the oldest is forgotten (and retried if drawn again) beyond this.
const MAX_BAD: usize = 64;

/// Linked files remembered as unavailable; those recorded before the last link change go first.
const MAX_UNAVAILABLE: usize = 1024;

/// Coarsest mip level.
const MAX_LEVEL: u8 = 8;

/// The largest full-resolution decode kept beside the level drawn when an image is zoomed into
/// (a finer level asked for while a coarser one is cached): each further step in is then halved
/// from it instead of decoded again.
const KEEP_FULL: usize = BUDGET / 8;

/// Full decodes kept for zooming in at once: an older one is evicted for a newer one, so a
/// spread of photos each with a coarser level cached doesn't fill the cache with full decodes.
const MAX_KEPT_FULL: usize = 2;

struct Entry {
    pm: Arc<Pixmap>,
    stamp: u64,
}

#[derive(Default)]
struct Cache {
    /// (bytes, PDF page, mip level).
    map: HashMap<(SourceKey, u32, u8), Entry>,
    bytes: usize,
    clock: u64,
    /// Assets that failed to decode (don't retry every frame), with their insertion stamp.
    bad: HashMap<(SourceKey, u32), u64>,
    /// Pixel size of each asset at full resolution (as decoded, else as its header gives it), so
    /// the level to draw is known without decoding.
    dims: HashMap<(SourceKey, u32), (u32, u32)>,
    /// The colour settings the pixels were converted with ([`designcraft_color::cms::generation`]).
    colour: u64,
    /// Advances each time the cache is emptied, so records of decodes into the old cache
    /// (the background queue's recent decodes) no longer count.
    epoch: u64,
    /// Full decodes kept for a zoom-in ([`KEEP_FULL`]), oldest first; at most [`MAX_KEPT_FULL`].
    kept_full: std::collections::VecDeque<(SourceKey, u32)>,
    /// Linked bytes that couldn't be loaded: why, the link generation then, and a stamp.
    unavailable: HashMap<SourceKey, (Unavailable, u64, u64)>,
}

impl Cache {
    /// Remember a failed asset, evicting the oldest failures beyond [`MAX_BAD`].
    fn mark_bad(&mut self, key: (SourceKey, u32)) {
        self.clock += 1;
        self.bad.insert(key, self.clock);
        while self.bad.len() > MAX_BAD {
            let Some(oldest) = self.bad.iter().min_by_key(|(_, stamp)| **stamp).map(|(k, _)| *k) else { break };
            self.bad.remove(&oldest);
        }
    }

    /// Remember that the bytes `id` names can't be had, as of link generation `generation`.
    fn mark_unavailable(&mut self, id: SourceKey, why: Unavailable, generation: u64) {
        self.clock += 1;
        self.unavailable.insert(id, (why, generation, self.clock));
        if self.unavailable.len() > MAX_UNAVAILABLE {
            self.unavailable.retain(|_, (_, g, _)| *g == generation);
        }
        while self.unavailable.len() > MAX_UNAVAILABLE {
            let Some(oldest) = self.unavailable.iter().min_by_key(|(_, (_, _, stamp))| *stamp).map(|(k, _)| *k) else { break };
            self.unavailable.remove(&oldest);
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
        *c = Cache { clock: c.clock, colour, epoch: c.epoch.wrapping_add(1), ..Cache::default() };
    }
    f(c)
}

fn lookup(key: (SourceKey, u32, u8)) -> Option<Arc<Pixmap>> {
    with(|c| {
        c.clock += 1;
        let clock = c.clock;
        c.map.get_mut(&key).map(|e| {
            e.stamp = clock;
            e.pm.clone()
        })
    })
}

/// Whether any level of the bytes `id` names is cached.
pub(crate) fn has_decodes(id: SourceKey) -> bool {
    with(|c| c.map.keys().any(|k| k.0 == id))
}

/// Whether `key` is cached (without marking it used).
fn is_cached(key: (SourceKey, u32, u8)) -> bool {
    with(|c| c.map.contains_key(&key))
}

/// The cache's [`Cache::epoch`].
#[cfg(not(target_arch = "wasm32"))]
fn epoch() -> u64 {
    with(|c| c.epoch)
}

fn is_bad(id: SourceKey, page: u32) -> bool {
    with(|c| c.bad.contains_key(&(id, page)))
}

/// Why the bytes `id` names couldn't be loaded, when that was found since the links last
/// changed.
fn unavailable_now(id: SourceKey) -> Option<Unavailable> {
    let generation = designcraft_doc::asset_data::link_generation();
    with(|c| c.unavailable.get(&id).filter(|(_, g, _)| *g == generation).map(|(why, ..)| *why))
}

/// Why `data` couldn't be loaded for a decode, when it couldn't (since the links last changed):
/// it is drawn from its proxy meanwhile.
pub fn unavailable(data: &AssetData) -> Option<Unavailable> {
    if data.is_linked() { unavailable_now(data.key()) } else { None }
}

/// The bytes of `data` to decode. Linked bytes the store doesn't hold are loaded, `gate`d at
/// [`GATE`] for their size; a failure is remembered ([`unavailable_now`]).
fn load_bytes(data: &AssetData, gate: bool) -> Option<Arc<Vec<u8>>> {
    if let Some(bytes) = data.resident() {
        return Some(bytes);
    }
    let generation = designcraft_doc::asset_data::link_generation();
    let fits = data.len() <= designcraft_doc::asset_data::MAX_LINKED_BYTES;
    let _reserved = (gate && fits).then(|| reserve(usize::try_from(data.len()).unwrap_or(usize::MAX), data.key(), None));
    match data.load() {
        Ok(bytes) => Some(bytes),
        Err(why) => {
            with(|c| c.mark_unavailable(data.key(), why, generation));
            None
        }
    }
}

/// Cache `pm` as `level` of the bytes `id` names. `colour` is the
/// [`designcraft_color::cms::generation`] read before the pixels were looked up or decoded:
/// pixels converted under settings changed since are not kept (the next request decodes them
/// again).
fn store(id: SourceKey, page: u32, level: u8, pm: Arc<Pixmap>, colour: u64) {
    with(|c| {
        if c.colour != colour {
            return;
        }
        c.clock += 1;
        let key = (id, page, level);
        let n = size_of(&pm);
        if let Some(old) = c.map.insert(key, Entry { pm, stamp: c.clock }) {
            c.bytes -= size_of(&old.pm);
        }
        c.bytes += n;
        if c.bytes > BUDGET {
            let mut v: Vec<(u64, (SourceKey, u32, u8))> = c.map.iter().map(|(k, e)| (e.stamp, *k)).collect();
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

/// Sizes remembered ([`Cache::dims`]) before those of images with nothing cached are forgotten
/// (a header gives them again).
const MAX_DIMS: usize = 1024;

/// Remember `size` as the full-resolution pixel size of the bytes `id` names; a header's estimate
/// (`exact` false) doesn't replace a decode's size.
fn set_dims(id: SourceKey, page: u32, size: (u32, u32), exact: bool) {
    let key = (id, page);
    with(|c| {
        if !exact && c.dims.contains_key(&key) {
            return;
        }
        if c.dims.len() >= MAX_DIMS {
            let Cache { dims, map, .. } = c;
            dims.retain(|k, _| (0..=MAX_LEVEL).any(|l| map.contains_key(&(k.0, k.1, l))));
            if dims.len() >= MAX_DIMS {
                dims.clear();
            }
        }
        c.dims.insert(key, size);
    });
}

/// The remembered full-resolution pixel size of the bytes `id` names.
fn known_dims(id: SourceKey, page: u32) -> Option<(u32, u32)> {
    with(|c| c.dims.get(&(id, page)).copied())
}

/// The full-resolution pixel size of `bytes` (named by `id`): as decoded, else as its header
/// gives it.
fn dims_from(id: SourceKey, bytes: &[u8], page: u32) -> Option<(u32, u32)> {
    if let Some(size) = known_dims(id, page) {
        return Some(size);
    }
    let size = drawn_pixels(bytes, page)?;
    set_dims(id, page, size, false);
    Some(size)
}

/// The full-resolution pixel size of `data` (loading it if need be: a failure is remembered).
fn dims_of(data: &AssetData, page: u32) -> Option<(u32, u32)> {
    match known_dims(data.key(), page) {
        Some(size) => Some(size),
        None if unavailable_now(data.key()).is_some() => None,
        None => dims_from(data.key(), &load_bytes(data, false)?, page),
    }
}

/// [`dims_of`] without loading anything: `None` for linked bytes not in memory.
fn dims_now(data: &AssetData, page: u32) -> Option<(u32, u32)> {
    match known_dims(data.key(), page) {
        Some(size) => Some(size),
        None => dims_from(data.key(), &data.resident()?, page),
    }
}

/// Decode `bytes` (named by `id`) at full resolution without caching the pixels (its size is
/// remembered); a failure marks it bad.
fn decode_full(id: SourceKey, bytes: &[u8], page: u32) -> Option<Arc<Pixmap>> {
    // Decode outside the lock: other threads keep drawing cached images meanwhile.
    let pm = decode_pixmap_page(bytes, page);
    log_decode(id);
    match pm {
        Some(pm) => {
            set_dims(id, page, (u32::from(pm.width()), u32::from(pm.height())), true);
            Some(Arc::new(pm))
        }
        None => {
            with(|c| c.mark_bad((id, page)));
            None
        }
    }
}

#[cfg(test)]
fn log_decode(id: SourceKey) {
    DECODED_ON.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push((id, std::thread::current().id()));
}

#[cfg(not(test))]
fn log_decode(_: SourceKey) {}

/// Images whose background decode or warm planning panics (tests: a decoder that panics).
#[cfg(test)]
static FAULTS: Mutex<Vec<SourceKey>> = Mutex::new(Vec::new());

#[cfg(test)]
#[allow(clippy::panic)]
fn fault(id: SourceKey) {
    if FAULTS.lock().unwrap_or_else(std::sync::PoisonError::into_inner).contains(&id) {
        panic!("test fault");
    }
}

#[cfg(not(test))]
fn fault(_: SourceKey) {}

/// Full-resolution decode of `data` (cached).
pub fn decoded(data: &AssetData, page: u32) -> Option<Arc<Pixmap>> {
    let id = data.key();
    let colour = designcraft_color::cms::generation();
    if let Some(pm) = lookup((id, page, 0)) {
        return Some(pm);
    }
    if is_bad(id, page) || unavailable_now(id).is_some() {
        return None;
    }
    let bytes = load_bytes(data, false)?;
    let pm = decode_full(id, &bytes, page)?;
    store(id, page, 0, pm.clone(), colour);
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

/// `size` halved `times` times, as [`halve`] does.
fn halved((mut w, mut h): (u32, u32), times: u8) -> (u32, u32) {
    for _ in 0..times {
        (w, h) = ((w / 2).max(1), (h / 2).max(1));
    }
    (w, h)
}

/// Halve `pm`, at level `from`, down to level `to`.
fn halve_to(mut pm: Arc<Pixmap>, from: u8, to: u8) -> Arc<Pixmap> {
    for _ in from..to {
        pm = Arc::new(halve(&pm));
    }
    pm
}

/// Level `level` of the bytes `id` names from the cache: that level, or one built from the
/// nearest finer cached level (no decode).
fn cached_level(id: SourceKey, page: u32, level: u8, colour: u64) -> Option<Arc<Pixmap>> {
    if let Some(pm) = lookup((id, page, level)) {
        return Some(pm);
    }
    let (from, finer) = (0..level).rev().find_map(|l| lookup((id, page, l)).map(|pm| (l, pm)))?;
    let pm = halve_to(finer, from, level);
    store(id, page, level, pm.clone(), colour);
    Some(pm)
}

/// The mip level of `data` closest above `on_screen` device pixels per point for a graphic
/// `width_pt` points wide. Only that level is cached, not the full decode it is built from,
/// except while an image is zoomed into (see [`KEEP_FULL`]).
pub fn mip(data: &AssetData, page: u32, width_pt: f64, on_screen: f64) -> Option<Arc<Pixmap>> {
    mip_gated(data, page, width_pt, on_screen, false)
}

/// [`mip`]; with `gate`, a decode first waits at [`GATE`] for its peak bytes ([`reserve`]),
/// counted for the decode path it takes.
///
/// The bytes are loaded only when the level isn't cached (nor a finer one to halve). Gated, a
/// load of linked bytes waits at [`GATE`] for their size first, and the decode counts them too
/// when the store didn't keep them (two reservations one after the other, never one inside the
/// other: a decode holding one while waiting for another could wait for itself).
fn mip_gated(data: &AssetData, page: u32, width_pt: f64, on_screen: f64, gate: bool) -> Option<Arc<Pixmap>> {
    let id = data.key();
    if is_bad(id, page) || unavailable_now(id).is_some() {
        return None;
    }
    let colour = designcraft_color::cms::generation();
    let cached = |size: (u32, u32)| {
        let level = level_for(size, width_pt, on_screen);
        let pm = cached_level(id, page, level, colour)?;
        if level == 0 {
            drawn_full((id, page));
        }
        Some(pm)
    };
    if let Some(pm) = known_dims(id, page).and_then(cached) {
        return Some(pm);
    }
    let bytes = load_bytes(data, gate)?;
    // Encoded bytes this decode alone holds.
    let held = if gate && data.is_linked() && data.resident().is_none() { bytes.len() } else { 0 };
    let mut zoom_in = false;
    if let Some(size) = dims_from(id, &bytes, page) {
        if let Some(pm) = cached(size) {
            return Some(pm);
        }
        let level = level_for(size, width_pt, on_screen);
        if let Some(pm) = reduced(id, &bytes, page, level, gate.then_some(held), colour) {
            return Some(pm);
        }
        zoom_in = (level + 1..=MAX_LEVEL).any(|l| is_cached((id, page, l)));
    }
    let _reserved = gate.then(|| reserve((decode_sizes(&bytes, page).1.saturating_mul(3) / 2).saturating_add(held), id, None));
    let full = decode_full(id, &bytes, page)?;
    let level = level_for((u32::from(full.width()), u32::from(full.height())), width_pt, on_screen);
    if level == 0 {
        store(id, page, 0, full.clone(), colour);
        drawn_full((id, page));
    } else if zoom_in && size_of(&full) <= KEEP_FULL {
        store(id, page, 0, full.clone(), colour);
        keep_full((id, page));
    }
    let pm = halve_to(full, 0, level);
    if level > 0 {
        store(id, page, level, pm.clone(), colour);
    }
    Some(pm)
}

/// The full decode of `key` is drawn, not kept for a zoom-in: another image's zoom-in doesn't
/// evict it ([`keep_full`]).
fn drawn_full(key: (SourceKey, u32)) {
    with(|c| c.kept_full.retain(|k| *k != key));
}

/// Count the full decode of `key` just stored as kept for a zoom-in, evicting the oldest such
/// decodes beyond [`MAX_KEPT_FULL`].
fn keep_full(key: (SourceKey, u32)) {
    with(|c| {
        c.kept_full.retain(|k| *k != key);
        c.kept_full.push_back(key);
        while c.kept_full.len() > MAX_KEPT_FULL {
            let Some(old) = c.kept_full.pop_front() else { break };
            if let Some(e) = c.map.remove(&(old.0, old.1, 0)) {
                c.bytes = c.bytes.saturating_sub(size_of(&e.pm));
            }
        }
    });
}

/// What [`mip_now`] has to draw.
pub enum Mip {
    /// The level asked for.
    Ready(Arc<Pixmap>),
    /// Still decoding (or loading); another cached level to draw in its place (the nearest
    /// coarser one, else the nearest finer one), if any.
    Pending(Option<Arc<Pixmap>>),
    /// The image can't be decoded, or its linked file can't be had ([`unavailable`]).
    Failed,
}

/// [`mip`] without waiting: a level that isn't cached is decoded on a background thread and
/// [`generation`] advances when it is ready (the [`set_waker`] callback runs too). On wasm it
/// decodes in place.
pub fn mip_now(data: &AssetData, page: u32, width_pt: f64, on_screen: f64) -> Mip {
    let id = data.key();
    if is_bad(id, page) || unavailable_now(id).is_some() {
        return Mip::Failed;
    }
    // Linked bytes not in memory are left to the decode thread to load.
    let level = dims_now(data, page).map(|size| level_for(size, width_pt, on_screen));
    // Only the exact level: building it from a finer one is left to the decode thread too.
    if let Some((l, pm)) = level.and_then(|l| lookup((id, page, l)).map(|pm| (l, pm))) {
        if l == 0 {
            drawn_full((id, page));
        }
        return Mip::Ready(pm);
    }
    let want = Want { bytes: data.clone(), page, width_pt, on_screen };
    match background::queue(want, level) {
        background::Queued::Yes | background::Queued::Recent => {}
        background::Queued::InPlace => return mip(data, page, width_pt, on_screen).map_or(Mip::Failed, Mip::Ready),
    }
    DEFERRED.with(|d| d.set(true));
    let stand_in =
        level.and_then(|l| (l + 1..=MAX_LEVEL).find_map(|c| lookup((id, page, c))).or_else(|| (0..l).rev().find_map(|f| lookup((id, page, f)))));
    Mip::Pending(stand_in)
}

/// Advances each time a background decode queued by [`mip_now`] finishes.
pub fn generation() -> u64 {
    background::GENERATION.load(Ordering::Acquire)
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

/// Run `f` and say whether it deferred an image ([`mip_now`]); the thread's flag still holds
/// every deferral for [`take_deferred`].
pub(crate) fn deferring<R>(f: impl FnOnce() -> R) -> (R, bool) {
    let before = take_deferred();
    let r = f();
    let deferred = DEFERRED.with(|d| d.replace(before || d.get()));
    (r, deferred)
}

#[cfg(not(target_arch = "wasm32"))]
mod background {
    //! A few decode threads, newest request first: what was just scrolled into view decodes
    //! before what was scrolled past. Their decodes count at [`super::GATE`] as [`super::warm`]'s
    //! do, so they wait while others hold half the budget.

    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Arc, Condvar, Mutex, PoisonError};
    use std::time::Instant;

    use designcraft_doc::SourceKey;

    use super::Want;

    /// Decode threads.
    const THREADS: usize = 3;
    /// Requests kept waiting; the oldest are dropped beyond this (they are asked for again if
    /// they are still in view).
    const MAX_QUEUED: usize = 256;
    /// An image asked for again this soon after its decode finished was evicted at once: the
    /// view holds more than the cache. It isn't decoded again until this has passed (the canvas
    /// draws what is cached of it meanwhile), so decodes don't loop and never run in place on
    /// the canvas.
    const RECENT: std::time::Duration = std::time::Duration::from_secs(2);

    pub(super) static GENERATION: AtomicU64 = AtomicU64::new(0);
    #[allow(clippy::type_complexity)]
    pub(super) static WAKER: Mutex<Option<Arc<dyn Fn() + Send + Sync>>> = Mutex::new(None);

    struct Job {
        want: Want,
        level: Option<u8>,
    }

    impl Job {
        fn key(&self) -> (SourceKey, u32) {
            (self.want.bytes.key(), self.want.page)
        }
    }

    /// A decode that finished.
    struct Done {
        key: (SourceKey, u32),
        level: Option<u8>,
        at: Instant,
        /// The cache's epoch then: a record of a decode into a cache emptied since doesn't count.
        epoch: u64,
        /// A request turned away as [`Queued::Recent`] scheduled a wake for this decode.
        woken: bool,
        /// One did for an earlier decode of the same image, level and epoch. Turned away again,
        /// this one schedules none: the view holds more than the cache (or than the list of
        /// failed images), and each wake would decode it again, every [`RECENT`] without end.
        rewoken: bool,
    }

    impl Done {
        /// Whether this is `key` at `level` decoded into the cache of `epoch`.
        fn is_of(&self, key: (SourceKey, u32), level: Option<u8>, epoch: u64) -> bool {
            self.key == key && self.level == level && self.epoch == epoch
        }

        /// Whether this is `key` at `level`, finished too recently to decode in the background
        /// again (into the cache of `epoch`).
        fn is_recent(&self, key: (SourceKey, u32), level: Option<u8>, epoch: u64) -> bool {
            self.is_of(key, level, epoch) && self.at.elapsed() < RECENT
        }
    }

    struct Queue {
        jobs: VecDeque<Job>,
        /// Being decoded now (one per thread).
        busy: Vec<(SourceKey, u32)>,
        /// The last decodes finished, newest last.
        done: VecDeque<Done>,
        threads: usize,
        /// When to advance the generation once more: a request turned away as [`Queued::Recent`]
        /// is drawn when its window has passed.
        wake_at: Option<Instant>,
    }

    static QUEUE: Mutex<Queue> = Mutex::new(Queue { jobs: VecDeque::new(), busy: Vec::new(), done: VecDeque::new(), threads: 0, wake_at: None });
    static READY: Condvar = Condvar::new();

    fn lock() -> std::sync::MutexGuard<'static, Queue> {
        QUEUE.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// What [`queue`] did with a request.
    pub(super) enum Queued {
        /// Queued, or already decoding.
        Yes,
        /// Decoded moments ago and evicted since: not queued again yet (the generation advances
        /// when that may happen).
        Recent,
        /// No decode thread can run: decode in place.
        InPlace,
    }

    /// Ask for a background decode of `want`, whose level is `level` when its size is known.
    pub(super) fn queue(want: Want, level: Option<u8>) -> Queued {
        let job = Job { want, level };
        let key = job.key();
        let epoch = super::epoch();
        let mut q = lock();
        if q.busy.contains(&key) {
            // Finishing it advances the generation; a different size is asked for then.
            return Queued::Yes;
        }
        let q = &mut *q;
        if let Some(d) = q.done.iter_mut().find(|d| d.is_recent(key, level, epoch)) {
            if !d.rewoken
                && let Some(at) = d.at.checked_add(RECENT)
            {
                #[cfg(test)]
                if !d.woken {
                    WOKEN.lock().unwrap_or_else(PoisonError::into_inner).push(key);
                }
                d.woken = true;
                if q.wake_at.is_none_or(|w| at < w) {
                    q.wake_at = Some(at);
                    READY.notify_one();
                }
            }
            return Queued::Recent;
        }
        q.jobs.retain(|j| j.key() != key);
        q.jobs.push_back(job);
        while q.jobs.len() > MAX_QUEUED {
            q.jobs.pop_front();
        }
        if q.threads < THREADS {
            if std::thread::Builder::new().name("designcraft-decode".into()).spawn(run).is_ok() {
                q.threads += 1;
            } else if q.threads == 0 {
                q.jobs.retain(|j| j.key() != key);
                return Queued::InPlace;
            }
        }
        READY.notify_one();
        Queued::Yes
    }

    /// The next job, or `None` when [`Queue::wake_at`] has come.
    fn next() -> Option<Job> {
        let mut q = lock();
        loop {
            if let Some(job) = q.jobs.pop_back() {
                q.busy.push(job.key());
                // This thread may have been the one waiting for `wake_at`: another takes over.
                if q.wake_at.is_some() {
                    READY.notify_one();
                }
                return Some(job);
            }
            match q.wake_at {
                Some(at) if Instant::now() >= at => {
                    q.wake_at = None;
                    return None;
                }
                Some(at) => {
                    let wait = at.saturating_duration_since(Instant::now());
                    q = READY.wait_timeout(q, wait).map_or_else(|e| e.into_inner().0, |(g, _)| g);
                }
                None => q = READY.wait(q).unwrap_or_else(PoisonError::into_inner),
            }
        }
    }

    /// Advance the generation and run the waker.
    fn advance() {
        GENERATION.fetch_add(1, Ordering::AcqRel);
        let waker = WAKER.lock().unwrap_or_else(PoisonError::into_inner).clone();
        if let Some(wake) = waker {
            wake();
        }
    }

    /// Takes a job off the busy list and records it as done when dropped, however the job ended.
    struct Busy<'a>(&'a Job);

    impl Drop for Busy<'_> {
        fn drop(&mut self) {
            let (key, level, epoch) = (self.0.key(), self.0.level, super::epoch());
            let mut q = lock();
            q.busy.retain(|k| *k != key);
            // This decode replaces the earlier records of the image at this level.
            let mut rewoken = false;
            q.done.retain(|d| {
                let earlier = d.is_of(key, level, epoch);
                rewoken |= earlier && (d.woken || d.rewoken);
                !earlier
            });
            q.done.push_back(Done { key, level, at: Instant::now(), epoch, woken: false, rewoken });
            while q.done.len() > 64 {
                q.done.pop_front();
            }
        }
    }

    /// Counts its thread out of the decode threads when dropped (the thread ended), so a new
    /// one is started for the next request.
    struct Alive;

    impl Drop for Alive {
        fn drop(&mut self) {
            let mut q = lock();
            q.threads = q.threads.saturating_sub(1);
        }
    }

    fn run() {
        let _alive = Alive;
        loop {
            let Some(job) = next() else {
                advance();
                continue;
            };
            let key = job.key();
            let busy = Busy(&job);
            let w = &job.want;
            // A panic anywhere in the job (sizing, waiting, decoding) must not take the thread
            // with it, nor leave the image drawn as decoding.
            let run = std::panic::AssertUnwindSafe(|| {
                super::fault(key.0);
                // An unavailable file is drawn again too: from its proxy.
                super::mip_gated(&w.bytes, w.page, w.width_pt, w.on_screen, true).is_some() || super::unavailable_now(key.0).is_some()
            });
            let ok = std::panic::catch_unwind(run).unwrap_or_else(|_| {
                super::with(|c| c.mark_bad(key));
                false
            });
            drop(busy);
            // A failure redraws the image as failed (no placeholder), unless the failures fill
            // the list of bad images: forgotten ones would be asked for again, without end.
            if ok || super::with(|c| c.bad.len() < super::MAX_BAD) {
                advance();
            }
        }
    }

    /// Whether the image `id` names is being decoded.
    #[cfg(test)]
    pub(super) fn is_busy(id: SourceKey) -> bool {
        lock().busy.iter().any(|k| k.0 == id)
    }

    /// The decodes (image, page) a turned-away request scheduled a wake for.
    #[cfg(test)]
    static WOKEN: Mutex<Vec<(SourceKey, u32)>> = Mutex::new(Vec::new());

    /// The wakes scheduled for `key` so far.
    #[cfg(test)]
    pub(super) fn wakes(key: (SourceKey, u32)) -> usize {
        WOKEN.lock().unwrap_or_else(PoisonError::into_inner).iter().filter(|k| **k == key).count()
    }

    /// Move the records of `key`'s decodes back past [`RECENT`], as if it had passed.
    #[cfg(test)]
    pub(super) fn age(key: (SourceKey, u32)) {
        for d in lock().done.iter_mut().filter(|d| d.key == key) {
            d.at = d.at.checked_sub(RECENT).unwrap();
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// A record counts only for its own image, page, level and cache: new bytes are decoded
        /// in the background, not skipped.
        #[test]
        fn a_finished_decode_counts_only_for_its_own_image() {
            let bytes = designcraft_doc::AssetData::embedded(vec![1u8]);
            let key = (bytes.key(), 0);
            let done = Done { key, level: Some(1), at: Instant::now(), epoch: 3, woken: false, rewoken: false };
            assert!(done.is_recent(key, Some(1), 3));
            assert!(!done.is_recent(key, Some(2), 3), "another level");
            assert!(!done.is_recent((key.0, 1), Some(1), 3), "another page");
            assert!(!done.is_recent(key, Some(1), 4), "the cache was emptied since");
            let other = designcraft_doc::AssetData::embedded(vec![1u8]);
            assert!(!done.is_recent((other.key(), 0), Some(1), 3), "other bytes");
        }
    }
}

#[cfg(target_arch = "wasm32")]
mod background {
    use std::sync::atomic::AtomicU64;
    use std::sync::{Arc, Mutex};

    use super::Want;

    pub(super) static GENERATION: AtomicU64 = AtomicU64::new(0);
    #[allow(clippy::type_complexity)]
    pub(super) static WAKER: Mutex<Option<Arc<dyn Fn() + Send + Sync>>> = Mutex::new(None);

    pub(super) enum Queued {
        #[allow(dead_code)]
        Yes,
        #[allow(dead_code)]
        Recent,
        InPlace,
    }

    /// No threads: always decode in place.
    pub(super) fn queue(_: Want, _: Option<u8>) -> Queued {
        Queued::InPlace
    }
}

/// The decodes run, full or reduced: the bytes decoded and the thread.
#[cfg(test)]
pub(crate) static DECODED_ON: Mutex<Vec<(SourceKey, std::thread::ThreadId)>> = Mutex::new(Vec::new());

/// A placed image a render draws, as [`mip`] takes it: the encoded bytes, the PDF page, the
/// width (points) and the device pixels per point.
pub(crate) struct Want {
    pub bytes: AssetData,
    pub page: u32,
    pub width_pt: f64,
    pub on_screen: f64,
}

/// Dimensions of the JPEG mip level selected for a request. `None` means the request uses the
/// full decode or is not an eligible JPEG, so warming must reserve its full decode size.
fn reduced_sizes(bytes: &[u8], page: u32, width_pt: f64, on_screen: f64) -> Option<Vec<(u32, u32)>> {
    if page != 0 || !bytes.starts_with(&[0xFF, 0xD8]) {
        return None;
    }
    let (mut w, mut h) = crate::image_size(bytes)?;
    let ppt = w as f64 / width_pt.max(1e-6);
    let mut sizes = vec![(w, h)];
    while ppt / (1u64 << sizes.len()) as f64 >= on_screen * 1.2 && w > 64 && h > 64 && sizes.len() <= 8 {
        (w, h) = ((w / 2).max(1), (h / 2).max(1));
        sizes.push((w, h));
    }
    Some(sizes)
}

/// Cache and peak decode estimates for a group of requests that all use reduced JPEG mips.
fn reduced_estimates(wants: &[Want]) -> Option<(usize, usize)> {
    let mut dims = None;
    let mut levels = [false; 9];
    let mut peak = 0usize;
    for w in wants {
        let bytes = w.bytes.load().ok()?;
        let sizes = reduced_sizes(&bytes, w.page, w.width_pt, w.on_screen)?;
        let level = sizes.len() - 1;
        if level == 0 {
            return None;
        }
        let shrink = level.min(usize::from(designcraft_images::MAX_REDUCED_SHRINK));
        let (_, _, decode_peak) = designcraft_images::reduced_jpeg_estimate(&bytes, shrink as u8)?;
        for cached in levels.iter_mut().take(level + 1).skip(shrink) {
            *cached = true;
        }
        peak = peak.max(decode_peak);
        if dims.as_ref().is_none_or(|d: &Vec<(u32, u32)>| d.len() < sizes.len()) {
            dims = Some(sizes);
        }
    }
    let dims = dims?;
    let cached = levels.iter().enumerate().filter(|(_, keep)| **keep).fold(0usize, |n, (level, _)| {
        let Some(&(w, h)) = dims.get(level) else { return n };
        n.saturating_add((w as usize).saturating_mul(h as usize).saturating_mul(4))
    });
    Some((cached, peak))
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

/// Bytes decoding at once in the process ([`warm`] in every render and the canvas's background
/// decodes), with the signal that some finished.
static GATE: (Mutex<usize>, Condvar) = (Mutex::new(0), Condvar::new());

/// Takes a finished decode's bytes off its own call's count, if any, and off [`GATE`] when
/// dropped.
struct Release<'a>(Option<&'a AtomicUsize>, usize);

impl Drop for Release<'_> {
    fn drop(&mut self) {
        if let Some(mine) = self.0 {
            mine.fetch_sub(self.1, Ordering::Relaxed);
        }
        let mut g = GATE.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        *g = g.saturating_sub(self.1);
        drop(g);
        GATE.1.notify_all();
    }
}

/// Wait at [`GATE`] until a decode of about `size` bytes of the image `id` names may start
/// and count it there (and in `mine`) until the guard is dropped, even if the decode panics, so
/// no other decode waits forever. Also returns `mine` with it added.
fn reserve(size: usize, id: SourceKey, mine: Option<&AtomicUsize>) -> (Release<'_>, usize) {
    let mut g = GATE.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    while !admits(*g, size) {
        #[cfg(test)]
        waited(id);
        g = GATE.1.wait(g).unwrap_or_else(std::sync::PoisonError::into_inner);
    }
    #[cfg(not(test))]
    let _ = id;
    *g = g.saturating_add(size);
    let now = mine.map_or(size, |m| m.fetch_add(size, Ordering::Relaxed).saturating_add(size));
    drop(g);
    (Release(mine, size), now)
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
    // Sizing reads image headers: a parser panic skips warming, and the render draws the images
    // as it would without it.
    let Ok(jobs) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| plan(wants))) else { return 0 };
    if jobs.len() < 2 {
        return 0;
    }
    run_jobs(workers, jobs)
}

/// [`warm`]'s decodes: the bytes to reserve for each and the drawings it serves. Empty when
/// fewer than two images need decoding.
fn plan(wants: Vec<Want>) -> Vec<(usize, Vec<Want>)> {
    // What is drawn of each asset and page, in drawing order.
    let mut assets: Vec<((SourceKey, u32), Vec<Want>)> = Vec::new();
    let mut index: HashMap<(SourceKey, u32), usize> = HashMap::new();
    for w in wants {
        let key = (w.bytes.key(), w.page);
        match index.get(&key).and_then(|&i| assets.get_mut(i)) {
            Some(a) => a.1.push(w),
            None => {
                index.insert(key, assets.len());
                assets.push((key, vec![w]));
            }
        }
    }
    // Each asset's full pixel size and the level [`mip`] takes for each drawing of it.
    let levels: HashMap<(SourceKey, u32), ((u32, u32), Vec<u8>)> = assets
        .iter()
        .filter_map(|(key, wanted)| {
            let w = wanted.first()?;
            let size = dims_of(&w.bytes, w.page)?;
            Some((*key, (size, wanted.iter().map(|w| level_for(size, w.width_pt, w.on_screen)).collect())))
        })
        .collect();
    // The bytes each asset in view holds in the cache, when it is cached or failed, and the bytes
    // of everything else cached. Its levels become the most recently used, so the stores below
    // evict other images first.
    let (held, others) = with(|c| {
        c.clock += 1;
        let clock = c.clock;
        let mut held: HashMap<(SourceKey, u32), usize> = HashMap::new();
        for (k, e) in c.map.iter_mut() {
            if index.contains_key(&(k.0, k.1)) {
                e.stamp = clock;
                *held.entry((k.0, k.1)).or_default() += size_of(&e.pm);
            }
        }
        // An asset is cached when every drawing of it finds its level, or a finer one to halve.
        held.retain(|k, _| levels.get(k).is_some_and(|(_, ls)| ls.iter().all(|l| (0..=*l).any(|f| c.map.contains_key(&(k.0, k.1, f))))));
        let in_view = held.values().sum::<usize>();
        let generation = designcraft_doc::asset_data::link_generation();
        for k in index.keys() {
            if c.bad.contains_key(k) || c.unavailable.get(&k.0).is_some_and(|(_, g, _)| *g == generation) {
                held.entry(*k).or_default();
            }
        }
        (held, c.bytes.saturating_sub(in_view))
    });
    // One image to decode gains nothing from threads (and is not worth sizing).
    if assets.iter().filter(|(key, _)| !held.contains_key(key)).count() < 2 {
        return Vec::new();
    }
    let mut pixels = held.values().fold(0usize, |n, b| n.saturating_add(*b));
    let mut jobs: Vec<(usize, Vec<Want>)> = Vec::new();
    for (key, wanted) in assets {
        if held.contains_key(&key) {
            continue;
        }
        fault(key.0);
        let (size, decoding) = reduced_estimates(&wanted).unwrap_or_else(|| {
            let full = wanted.first().and_then(|w| Some(decode_sizes(&w.bytes.load().ok()?, w.page))).unwrap_or((0, 0));
            // Only the levels drawn are kept.
            let kept = levels.get(&key).map_or(full.0, |(size, ls)| {
                let mut ls = ls.clone();
                ls.sort_unstable();
                ls.dedup();
                ls.iter().fold(0usize, |n, l| {
                    let (w, h) = halved(*size, *l);
                    n.saturating_add((w as usize).saturating_mul(h as usize).saturating_mul(4))
                })
            });
            (kept, full.1)
        });
        // A store past the budget evicts down to three quarters of it: the images in view must
        // fit in that, unless they and the rest fit in the budget. A decode that alone holds
        // more than the budget is left to the render too.
        let total = pixels.saturating_add(size);
        if (total > BUDGET * 3 / 4 && others.saturating_add(total) > BUDGET) || decoding > BUDGET {
            continue;
        }
        pixels = total;
        // The estimate covers the decoded image and its converted pixels; leave headroom for
        // temporary buffers inside individual format decoders.
        jobs.push((decoding.saturating_mul(3) / 2, wanted));
    }
    jobs
}

/// Run [`warm`]'s `jobs` on up to `workers` threads; returns the most bytes reserved at once.
fn run_jobs(workers: usize, jobs: Vec<(usize, Vec<Want>)>) -> usize {
    // While it converts the pixels, a decode holds the buffers `decode_sizes` counts: a
    // decode waits while those in flight in any render would take it past half the budget (one
    // always runs).
    let next = AtomicUsize::new(0);
    // This call's bytes decoding, and the most there were.
    let (mine, peak) = (AtomicUsize::new(0), AtomicUsize::new(0));
    std::thread::scope(|s| {
        for _ in 0..workers.min(jobs.len()) {
            let work = || {
                while let Some((size, job)) = jobs.get(next.fetch_add(1, Ordering::Relaxed)) {
                    let id = job.first().map_or(SourceKey(0), |w| w.bytes.key());
                    let (_release, now) = reserve(*size, id, Some(&mine));
                    peak.fetch_max(now, Ordering::Relaxed);
                    for w in job {
                        // A decoder panic would otherwise reach the render through the scope and
                        // be retried on every frame; mark the image bad as a failed decode is.
                        let run = std::panic::AssertUnwindSafe(|| mip(&w.bytes, w.page, w.width_pt, w.on_screen));
                        if std::panic::catch_unwind(run).is_err() {
                            with(|c| c.mark_bad((w.bytes.key(), w.page)));
                        }
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

/// The images whose decode waited at [`GATE`], with the signal that one did.
#[cfg(test)]
static WAITED: (Mutex<Vec<SourceKey>>, Condvar) = (Mutex::new(Vec::new()), Condvar::new());

#[cfg(test)]
fn waited(id: SourceKey) {
    WAITED.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push(id);
    WAITED.1.notify_all();
}

/// Level `level` of a JPEG for [`mip`], decoded at 1/2, 1/4 or 1/8 of its size straight from
/// its blocks and halved from there, so a screen-sized level needs no full-size decode. `None`
/// when the full size is drawn, for other formats and for JPEGs only a full decode reads (CMYK).
///
/// `gate`: wait at [`GATE`] for the decode's peak bytes, plus the encoded bytes it holds.
fn reduced(id: SourceKey, bytes: &[u8], page: u32, level: u8, gate: Option<usize>, colour: u64) -> Option<Arc<Pixmap>> {
    if level == 0 || page != 0 {
        return None;
    }
    if !bytes.starts_with(&[0xFF, 0xD8]) {
        return None;
    }
    let shrink = level.min(designcraft_images::MAX_REDUCED_SHRINK);
    let mut pm = match lookup((id, page, shrink)) {
        Some(p) => p,
        None => {
            // Cut to `halve`'s size (it rounds down, the decoder up), as the level a full
            // decode makes.
            let (sw, sh) = halved(crate::image_size(bytes)?, shrink);
            // Gated, a JPEG without an estimate (one only a full decode reads) isn't tried here.
            let _reserved = match gate {
                Some(held) => {
                    let (_, _, peak) = designcraft_images::reduced_jpeg_estimate(bytes, shrink)?;
                    Some(reserve((peak.saturating_mul(3) / 2).saturating_add(held), id, None))
                }
                None => None,
            };
            let p = decode_pixmap_reduced(bytes, shrink)?;
            log_decode(id);
            let p = Arc::new(crop(p, sw, sh)?);
            store(id, page, shrink, p.clone(), colour);
            p
        }
    };
    for l in shrink + 1..=level {
        pm = match lookup((id, page, l)) {
            Some(p) => p,
            None => {
                let half = Arc::new(halve(&pm));
                store(id, page, l, half.clone(), colour);
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
        c.unavailable.clear();
        c.dims.clear();
        c.kept_full.clear();
        c.bytes = 0;
        c.epoch = c.epoch.wrapping_add(1);
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
        let buffers: Vec<AssetData> = (0..MAX_BAD + 10).map(|i| AssetData::embedded(vec![i as u8; 4])).collect();
        for b in &buffers {
            c.mark_bad((b.key(), 0));
        }
        assert_eq!(c.bad.len(), MAX_BAD);
        let first = &buffers[0];
        assert!(!c.bad.contains_key(&(first.key(), 0)));
        assert_eq!(Arc::strong_count(first.embedded_bytes().unwrap()), 1, "failures hold no bytes");
        let last = &buffers[MAX_BAD + 9];
        assert!(c.bad.contains_key(&(last.key(), 0)));
    }

    fn png(side: u32, value: u8) -> AssetData {
        let mut out = Vec::new();
        let img = image::RgbaImage::from_pixel(side, side, image::Rgba([value, 0, 0, 255]));
        img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).unwrap();
        AssetData::embedded(out)
    }

    fn want(bytes: &AssetData) -> Want {
        Want { bytes: bytes.clone(), page: 0, width_pt: 100.0, on_screen: 1.0 }
    }

    /// A PSD header claiming `side` pixels square (its pixels don't decode).
    fn psd(side: u32) -> AssetData {
        let mut psd = b"8BPS".to_vec();
        psd.resize(26, 0);
        // Version 1, three channels, 8 bits, RGB.
        psd[4..6].copy_from_slice(&1u16.to_be_bytes());
        psd[12..14].copy_from_slice(&3u16.to_be_bytes());
        psd[22..24].copy_from_slice(&8u16.to_be_bytes());
        psd[24..26].copy_from_slice(&3u16.to_be_bytes());
        psd[14..18].copy_from_slice(&side.to_be_bytes());
        psd[18..22].copy_from_slice(&side.to_be_bytes());
        AssetData::embedded(psd)
    }

    /// Where the decode log ends now.
    fn mark() -> usize {
        DECODED_ON.lock().unwrap().len()
    }

    /// The threads that decoded `bytes` after `since`.
    fn decoders(bytes: &AssetData, since: usize) -> Vec<std::thread::ThreadId> {
        let id = bytes.key();
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
        // Two 5,000 pixel square images take less than half the budget cached, but decoding both
        // at once would reserve more than half of it; the small image still decodes beside one.
        let (d, e, small) = (psd(5000), psd(5000), png(8, 15));
        let since = mark();
        let large = decoded_bytes(&d.load().unwrap(), 0);
        assert!(2 * large <= BUDGET / 2);
        let decoding = decode_peak_bytes(&d.load().unwrap(), 0);
        let reserved = decoding * 3 / 2;
        assert!(admits(0, reserved) && !admits(reserved, reserved) && admits(reserved, decode_peak_bytes(&small.load().unwrap(), 0) * 3 / 2));
        assert!(admits(0, usize::MAX), "an image over half the budget still decodes alone");
        let peak = warm(vec![want(&d), want(&e), want(&small)]);
        assert!(peak <= BUDGET / 2, "{peak} bytes decoding at once");
        assert!(peak >= reserved, "the large images were decoded ahead");
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
        let decoding = decode_peak_bytes(&f.load().unwrap(), 0);
        assert_eq!(decoding, 5500 * 5500 * 9, "9 bytes a pixel for a PSD");
        let reserved = decoding * 3 / 2;
        assert!(2 * decoded_bytes(&f.load().unwrap(), 0) <= BUDGET / 2 && !admits(reserved, decode_peak_bytes(&g.load().unwrap(), 0) * 3 / 2));
        assert_eq!(warm(vec![want(&f), want(&g)]), reserved, "one decode at a time");
        // 16-bit RGBA decodes at 8 bytes a pixel, held twice while it is turned upright.
        let mut deep = Vec::new();
        let img = image::ImageBuffer::<image::Rgba<u16>, Vec<u16>>::from_pixel(4, 4, image::Rgba([9, 0, 0, 65535]));
        img.write_to(&mut std::io::Cursor::new(&mut deep), image::ImageFormat::Png).unwrap();
        assert_eq!(decode_peak_bytes(&deep, 0), 16 * 16);
        assert_eq!(decode_peak_bytes(&png(8, 1).load().unwrap(), 0), 64 * 8);
    }

    #[test]
    fn warm_waits_for_decodes_in_other_renders() {
        // Another render's decodes take half the budget: this render's decodes wait for them.
        let (a, b) = (png(8, 17), png(8, 19));
        let ids = [a.key(), b.key()];
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
            let seen = |w: &Vec<SourceKey>| w.iter().skip(since).any(|id| ids.contains(id));
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

    fn jpeg(w: u32, h: u32) -> AssetData {
        let img = image::RgbImage::from_fn(w, h, |x, y| image::Rgb([(x * 255 / w) as u8, (y * 255 / h) as u8, 128]));
        let mut out = Vec::new();
        img.write_with_encoder(image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 90)).unwrap();
        AssetData::embedded(out)
    }

    #[test]
    fn warm_counts_cached_reduced_jpeg_levels_as_cached() {
        // Drawn at a tenth of their pixels, two JPEGs decode at a reduced level only; once that
        // level is cached, a later warm has nothing to decode.
        let (a, b) = (jpeg(1024, 768), jpeg(1024, 768));
        let small = |bytes: &AssetData| Want { bytes: bytes.clone(), page: 0, width_pt: 1024.0, on_screen: 0.1 };
        assert!(warm(vec![small(&a), small(&b)]) > 0, "decoded ahead");
        assert!(lookup((a.key(), 0, 0)).is_none(), "no full-size decode is kept");
        assert_eq!(warm(vec![small(&a), small(&b)]), 0, "nothing left to decode");
    }

    #[test]
    fn screen_levels_of_a_jpeg_need_no_full_size_decode() {
        let bytes = jpeg(1024, 768);
        let id = bytes.key();
        // 1 px per point drawn at 0.1: level 3, an eighth, read straight from the blocks.
        let pm = mip(&bytes, 0, 1024.0, 0.1).unwrap();
        assert_eq!((pm.width(), pm.height()), (128, 96));
        assert!(lookup((id, 0, 0)).is_none(), "no full-size decode is kept");
        // Close to the full decode halved three times.
        let mut want = decode_pixmap_page(&bytes.load().unwrap(), 0).unwrap();
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
    fn warm_estimates_a_jpegs_reduced_levels() {
        let bytes = jpeg(1024, 768);
        let want = Want { bytes: bytes.clone(), page: 0, width_pt: 1024.0, on_screen: 0.1 };
        let (cached, peak) = reduced_estimates(&[want]).unwrap();
        assert_eq!(cached, 128 * 96 * 4, "only the selected eighth-size mip is cached");
        assert_eq!(peak, 128 * 96 * 8, "only the selected eighth-size mip is decoded");
        let full = decode_sizes(&bytes.load().unwrap(), 0);
        assert!(cached < full.0 && peak < full.1, "reduced estimates {cached}/{peak}, full {full:?}");
    }

    #[test]
    fn reduced_levels_of_odd_sized_jpegs_have_the_halved_size() {
        let bytes = jpeg(1001, 771);
        let mut want = decode_pixmap_page(&bytes.load().unwrap(), 0).unwrap();
        for _ in 0..3 {
            want = halve(&want);
        }
        let pm = mip(&bytes, 0, 1001.0, 0.1).unwrap();
        assert!(lookup((bytes.key(), 0, 0)).is_none());
        assert_eq!((pm.width(), pm.height()), (want.width(), want.height()));
        assert_eq!((pm.width(), pm.height()), (125, 96));
    }

    #[test]
    fn a_jpeg_that_does_not_decode_is_remembered_as_failed() {
        let mut broken = jpeg(256, 256).load().unwrap().to_vec();
        broken.truncate(40);
        let bytes = AssetData::embedded(broken);
        assert!(mip(&bytes, 0, 256.0, 0.1).is_none());
        assert!(with(|c| c.bad.contains_key(&(bytes.key(), 0))));
        assert!(mip(&bytes, 0, 256.0, 0.1).is_none());
    }

    fn png_sized(w: u32, h: u32) -> AssetData {
        let img = image::RgbaImage::from_pixel(w, h, image::Rgba([200, 100, 50, 255]));
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).unwrap();
        AssetData::embedded(out.into_inner())
    }

    /// Polls `mip_now` until it has the level asked for and the decode thread is done with it.
    fn ready(bytes: &AssetData, width_pt: f64, on_screen: f64) -> Option<Arc<Pixmap>> {
        for _ in 0..500 {
            if let Mip::Ready(pm) = mip_now(bytes, 0, width_pt, on_screen)
                && !background::is_busy(bytes.key())
            {
                return Some(pm);
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        None
    }

    #[test]
    fn mip_keeps_only_the_level_drawn() {
        let bytes = png_sized(1024, 512);
        let id = bytes.key();
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
    fn decoded_levels_outlive_the_encoded_bytes() {
        let bytes = png_sized(1024, 512);
        let key = bytes.key();
        let copy = bytes.clone();
        assert!(mip(&bytes, 0, 128.0, 1.0).is_some());
        drop((bytes, copy));
        // Every handle to the bytes is gone; the level drawn and the size are still cached.
        assert_eq!(lookup((key, 0, 2)).map(|pm| (pm.width(), pm.height())), Some((256, 128)));
        assert_eq!(with(|c| c.dims.get(&(key, 0)).copied()), Some((1024, 512)));
    }

    /// Taken by the tests that keep full decodes for zooming in: they evict each other's.
    static ZOOMING: Mutex<()> = Mutex::new(());

    #[test]
    fn zooming_in_step_by_step_decodes_an_image_at_most_twice() {
        let _zooming = ZOOMING.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let bytes = png_sized(1024, 1024);
        let since = mark();
        // 8 px per point, from 1/16 to 4 device pixels per point: level 4 (64 px) to level 0.
        let mut widths = Vec::new();
        for step in 0..7 {
            let on_screen = 0.0625 * f64::from(1u32 << step);
            widths.push(mip(&bytes, 0, 128.0, on_screen).unwrap().width());
        }
        assert_eq!(widths, [64, 64, 64, 128, 256, 512, 1024]);
        // The first step in decodes again and keeps the full size; later steps halve it.
        assert_eq!(decoders(&bytes, since).len(), 2);
        // Back out and in again: no decode.
        for on_screen in [0.0625, 1.0, 4.0] {
            assert!(mip(&bytes, 0, 128.0, on_screen).is_some());
        }
        assert_eq!(decoders(&bytes, since).len(), 2);
    }

    #[test]
    fn warm_counts_cached_levels_of_any_format_as_cached() {
        // Drawn at a tenth of their pixels, two PNGs keep only that level; once it is cached, a
        // later warm has nothing to decode.
        let (a, b) = (png(256, 21), png(256, 23));
        let small = |bytes: &AssetData| Want { bytes: bytes.clone(), page: 0, width_pt: 256.0, on_screen: 0.1 };
        assert!(warm(vec![small(&a), small(&b)]) > 0, "decoded ahead");
        assert!(lookup((a.key(), 0, 0)).is_none(), "no full-size decode is kept");
        assert_eq!(warm(vec![small(&a), small(&b)]), 0, "nothing left to decode");
    }

    #[test]
    fn mip_now_decodes_in_the_background() {
        let bytes = png_sized(512, 512);
        let before = generation();
        let first = mip_now(&bytes, 0, 256.0, 1.0);
        assert!(matches!(first, Mip::Pending(None)));
        assert!(take_deferred());
        assert_eq!(ready(&bytes, 256.0, 1.0).map(|pm| pm.width()), Some(512));
        assert!(generation() > before);
    }

    #[test]
    fn mip_now_leaves_building_a_coarser_level_to_the_decode_thread() {
        let bytes = png_sized(512, 512);
        let id = bytes.key();
        // 512 px across 256 pt at 1 px/pt: level 0.
        assert_eq!(mip(&bytes, 0, 256.0, 1.0).map(|pm| pm.width()), Some(512));
        // Zoomed out to 0.25 px/pt: level 2 (128 px), not cached yet. The finer level stands in
        // while it is built off this thread.
        match mip_now(&bytes, 0, 256.0, 0.25) {
            Mip::Pending(Some(pm)) => assert_eq!(pm.width(), 512),
            Mip::Pending(None) => panic!("the cached finer level should stand in"),
            Mip::Ready(_) => panic!("the coarser level was built on the drawing thread"),
            Mip::Failed => panic!("decodable bytes reported as failed"),
        }
        assert_eq!(ready(&bytes, 256.0, 0.25).map(|pm| pm.width()), Some(128));
        assert!(lookup((id, 0, 2)).is_some());
    }

    #[test]
    fn mip_now_reports_undecodable_bytes() {
        let bytes = AssetData::embedded(b"not an image".to_vec());
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

    #[test]
    fn an_image_evicted_right_after_its_decode_is_not_decoded_on_the_canvas_thread() {
        let bytes = png_sized(256, 256);
        let id = bytes.key();
        assert!(ready(&bytes, 256.0, 1.0).is_some());
        // Evicted at once, as when the view holds more than the cache.
        with(|c| {
            let gone: Vec<_> = c.map.keys().filter(|k| k.0 == id).copied().collect();
            for k in gone {
                if let Some(e) = c.map.remove(&k) {
                    c.bytes -= size_of(&e.pm);
                }
            }
        });
        let since = mark();
        assert!(matches!(mip_now(&bytes, 0, 256.0, 1.0), Mip::Pending(None)), "drawn as still decoding");
        std::thread::sleep(std::time::Duration::from_millis(50));
        assert!(decoders(&bytes, since).is_empty(), "not decoded again, in place or in the background");
    }

    fn fault_on(bytes: &AssetData, on: bool) {
        let id = bytes.key();
        let mut f = FAULTS.lock().unwrap();
        f.retain(|i| *i != id);
        if on {
            f.push(id);
        }
    }

    /// Drop every level of `bytes`, as an eviction does.
    fn evict(bytes: &AssetData) {
        let id = bytes.key();
        with(|c| {
            let gone: Vec<_> = c.map.keys().filter(|k| k.0 == id).copied().collect();
            for k in gone {
                if let Some(e) = c.map.remove(&k) {
                    c.bytes -= size_of(&e.pm);
                }
            }
        });
    }

    #[test]
    fn a_panic_in_a_background_decode_marks_the_image_bad_and_the_worker_goes_on() {
        let bytes = png_sized(64, 64);
        let id = bytes.key();
        fault_on(&bytes, true);
        let mut failed = false;
        for _ in 0..500 {
            if matches!(mip_now(&bytes, 0, 64.0, 1.0), Mip::Failed) {
                failed = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        fault_on(&bytes, false);
        assert!(failed, "marked bad");
        // Forgotten as bad, it decodes again: the job left the busy list and a thread runs it.
        with(|c| c.bad.remove(&(id, 0)));
        assert!(ready(&bytes, 64.0, 1.0).is_some());
    }

    #[test]
    fn a_panic_while_warm_sizes_its_decodes_skips_warming() {
        let (a, b) = (png(8, 51), png(8, 53));
        fault_on(&a, true);
        let peak = warm(vec![want(&a), want(&b)]);
        fault_on(&a, false);
        assert_eq!(peak, 0);
    }

    #[test]
    fn an_image_turned_away_as_recent_is_woken_for_once() {
        let bytes = png_sized(96, 96);
        let key = (bytes.key(), 0);
        let before = background::wakes(key);
        assert!(ready(&bytes, 96.0, 1.0).is_some());
        evict(&bytes);
        assert!(matches!(mip_now(&bytes, 0, 96.0, 1.0), Mip::Pending(None)));
        assert_eq!(background::wakes(key), before + 1, "the generation advances once the window has passed");
        // Its window passed, it is decoded again and evicted at once again: turned away, it
        // waits for the next redraw instead of decoding every two seconds.
        background::age(key);
        assert!(ready(&bytes, 96.0, 1.0).is_some(), "decoded again once the window passed");
        evict(&bytes);
        assert!(matches!(mip_now(&bytes, 0, 96.0, 1.0), Mip::Pending(None)));
        assert_eq!(background::wakes(key), before + 1, "no second wake");
    }

    #[test]
    fn pixels_made_under_old_colour_settings_are_not_cached() {
        let bytes = png_sized(64, 64);
        let id = bytes.key();
        let pm = Arc::new(Pixmap::new(64, 64));
        let old = designcraft_color::cms::generation().wrapping_sub(1);
        store(bytes.key(), 0, 0, pm.clone(), old);
        assert!(!is_cached((id, 0, 0)), "converted before the settings changed");
        store(bytes.key(), 0, 0, pm, designcraft_color::cms::generation());
        assert!(is_cached((id, 0, 0)));
    }

    #[test]
    fn at_most_two_full_decodes_are_kept_for_zooming_in() {
        let _zooming = ZOOMING.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let images: Vec<_> = (0..3).map(|_| png_sized(512, 512)).collect();
        for img in &images {
            // Level 2, then zoomed in to level 1: the full decode is kept.
            assert_eq!(mip(img, 0, 128.0, 0.5).map(|pm| pm.width()), Some(128));
            assert_eq!(mip(img, 0, 128.0, 1.0).map(|pm| pm.width()), Some(256));
        }
        let full: Vec<bool> = images.iter().map(|img| is_cached((img.key(), 0, 0))).collect();
        assert_eq!(full, [false, true, true], "the oldest is let go");
    }

    /// Linked files for these tests: bytes by path, served by a loader that counts its reads.
    mod linked {
        use super::*;
        use designcraft_doc::asset_data::{Loaded, Loader};
        use designcraft_doc::{Fingerprint, LinkedSource};
        use std::collections::HashMap;

        static FILES: Mutex<Option<HashMap<String, Vec<u8>>>> = Mutex::new(None);
        static READS: Mutex<Vec<String>> = Mutex::new(Vec::new());

        struct Files;

        impl Loader for Files {
            fn load(&self, source: &LinkedSource, _: Option<Fingerprint>) -> Result<Loaded, Unavailable> {
                READS.lock().unwrap().push(source.path.clone());
                let bytes = FILES.lock().unwrap().as_ref().and_then(|f| f.get(&source.path).cloned()).ok_or(Unavailable::Missing)?;
                Ok(Loaded { bytes, fingerprint: None })
            }
        }

        /// A linked asset whose file is `bytes` (`None`: missing for now) at the unique `path`.
        pub(super) fn file(path: &str, bytes: Option<Vec<u8>>, len: u64) -> AssetData {
            static INSTALL: std::sync::Once = std::sync::Once::new();
            INSTALL.call_once(|| designcraft_doc::asset_data::install_loader(Arc::new(Files)));
            if let Some(b) = bytes {
                put(path, b);
            }
            AssetData::linked(LinkedSource { path: path.into(), relative: None, document: None, len, fingerprint: None, hash: None })
        }

        pub(super) fn put(path: &str, bytes: Vec<u8>) {
            FILES.lock().unwrap().get_or_insert_with(HashMap::new).insert(path.into(), bytes);
        }

        pub(super) fn reads(path: &str) -> usize {
            READS.lock().unwrap().iter().filter(|p| *p == path).count()
        }
    }

    fn png_bytes(w: u32, h: u32, rgba: [u8; 4]) -> Vec<u8> {
        let mut out = Vec::new();
        image::RgbaImage::from_pixel(w, h, image::Rgba(rgba)).write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).unwrap();
        out
    }

    /// A linked file is read on the decode thread, and a level once decoded is drawn again
    /// without reading the file, even after the store let go of its bytes.
    #[test]
    fn a_linked_image_is_loaded_off_the_canvas_and_its_cached_levels_need_no_reload() {
        let bytes = png_bytes(256, 256, [0, 0, 255, 255]);
        let data = linked::file("mem:/cached.png", Some(bytes.clone()), bytes.len() as u64);
        assert!(matches!(mip_now(&data, 0, 128.0, 1.0), Mip::Pending(None)), "not loaded on the drawing thread");
        assert_eq!(ready(&data, 128.0, 1.0).map(|pm| pm.width()), Some(256));
        assert_eq!(linked::reads("mem:/cached.png"), 1);
        data.evict();
        assert!(data.resident().is_none());
        assert!(matches!(mip_now(&data, 0, 128.0, 1.0), Mip::Ready(_)));
        assert!(mip(&data, 0, 128.0, 1.0).is_some());
        assert_eq!(mip(&data, 0, 128.0, 0.25).map(|pm| pm.width()), Some(64), "a coarser level is halved from the cached one");
        assert_eq!(linked::reads("mem:/cached.png"), 1, "no level drawn needed the file again");
    }

    /// A linked file that can't be had isn't a bad image: it is asked for again once the links
    /// changed (the file came back).
    #[test]
    fn an_unavailable_linked_file_is_tried_again_after_the_links_change() {
        let bytes = png_bytes(8, 8, [0, 255, 0, 255]);
        let data = linked::file("mem:/comes-back.png", None, bytes.len() as u64);
        assert!(mip(&data, 0, 8.0, 1.0).is_none());
        assert_eq!(unavailable(&data), Some(Unavailable::Missing));
        assert!(!is_bad(data.key(), 0));
        assert!(matches!(mip_now(&data, 0, 8.0, 1.0), Mip::Failed));
        assert!(mip(&data, 0, 8.0, 1.0).is_none());
        assert_eq!(linked::reads("mem:/comes-back.png"), 1, "not read again before the links change");
        linked::put("mem:/comes-back.png", bytes);
        designcraft_doc::asset_data::links_changed();
        assert_eq!(unavailable(&data), None);
        assert_eq!(mip(&data, 0, 8.0, 1.0).map(|pm| pm.width()), Some(8));
    }

    /// The page draws a missing linked image from its proxy, at the graphic's place.
    #[test]
    fn a_missing_linked_image_draws_its_proxy() {
        use designcraft_doc::build::NewDocument;
        use designcraft_doc::{Document, Item, SpreadRef};
        use designcraft_geom::kurbo::{Affine, Rect};
        let original = png_bytes(64, 64, [0, 0, 255, 255]);
        for defer in [false, true] {
            let path = format!("mem:/missing-{defer}.png");
            let mut d = Document::new(&NewDocument::default());
            let lid = d.default_layer();
            let aid = designcraft_doc::AssetId(d.alloc());
            let proxy =
                designcraft_doc::Proxy { mime: "image/png".into(), pixels: (4, 4), data: AssetData::embedded(png_bytes(4, 4, [255, 0, 0, 255])) };
            d.assets.insert(
                aid,
                Arc::new(designcraft_doc::Asset {
                    id: aid,
                    name: "photo.png".into(),
                    mime: "image/png".into(),
                    link: Some(path.clone()),
                    storage: designcraft_doc::Storage::Linked,
                    data: linked::file(&path, None, original.len() as u64),
                    proxy: Some(proxy),
                    pixels: Some((64, 64)),
                    ..Default::default()
                }),
            );
            let id = designcraft_doc::ItemId(d.alloc());
            let mut it =
                Item::new(id, lid, designcraft_doc::Shape::Rectangle, designcraft_geom::shapes::rectangle(Rect::new(100.0, 100.0, 200.0, 200.0)));
            it.content = designcraft_doc::Content::Graphic(designcraft_doc::Graphic {
                layer_options: None,
                asset: aid,
                size: (100.0, 100.0),
                xf: Affine::translate((100.0, 100.0)),
                clip: None,
                auto_fit: Default::default(),
                fit_align: 4,
                crop: [0.0; 4],
                wrap: Default::default(),
                fill: Default::default(),
            });
            d.insert_item(SpreadRef::Doc(0), it, None).unwrap();
            let cache = crate::Cache::new();
            let mut r = crate::Renderer::new();
            r.threads = 0;
            let opts = crate::RenderOptions { defer_images: defer, ..Default::default() };
            let mut px = [0; 4];
            for _ in 0..500 {
                px = r.render_page(&d, &cache, 0, 1.0, false, &opts).unwrap().pixel(150, 150);
                if px[..3] == [255, 0, 0] {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            assert_eq!(px[..3], [255, 0, 0], "defer {defer}: the proxy stands in");
        }
    }
}
