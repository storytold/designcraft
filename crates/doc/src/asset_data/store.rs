//! The process-wide store of linked files' bytes ([`super::AssetData::load`]).

use std::collections::HashMap;
use std::sync::{Arc, Condvar, LazyLock, Mutex, MutexGuard, PoisonError, RwLock};

use super::{Linked, LinkedSource, SourceKey, Unavailable};
use crate::datamerge::Fingerprint;

/// Linked bytes kept in memory at most, over all files. A file larger than this is read for the
/// one load that asks for it and not kept.
pub const STORE_BUDGET: u64 = 512 << 20;

/// The largest linked file read: a larger one is [`Unavailable::TooLarge`].
pub const MAX_LINKED_BYTES: u64 = 1 << 30;

/// A linked file's bytes as a [`Loader`] read them.
pub struct Loaded {
    pub bytes: Vec<u8>,
    /// The fingerprint the file had, when its bytes were found to be the expected ones by their
    /// hash: later loads trust that fingerprint as they trust [`LinkedSource::fingerprint`].
    pub fingerprint: Option<Fingerprint>,
}

/// Reads linked files (the engine's, [`install_loader`]).
pub trait Loader: Send + Sync {
    /// The bytes of the file `source` names, if it holds the expected ones: its size is
    /// `source.len`, and its fingerprint is `source.fingerprint` or `trusted`, or its content
    /// hash is `source.hash`. Other bytes are [`Unavailable::Modified`]; no file is
    /// [`Unavailable::Missing`]; one over [`MAX_LINKED_BYTES`] is [`Unavailable::TooLarge`].
    fn load(&self, source: &LinkedSource, trusted: Option<Fingerprint>) -> Result<Loaded, Unavailable>;

    /// The bytes the file `source` names has now, whatever they are (for output that uses the
    /// current file of a modified link). Never cached.
    fn load_current(&self, _source: &LinkedSource) -> Result<Vec<u8>, Unavailable> {
        Err(Unavailable::NoLoader)
    }
}

static LOADER: RwLock<Option<Arc<dyn Loader>>> = RwLock::new(None);

/// Read linked files with `loader` from now on (in the whole process).
pub fn install_loader(loader: Arc<dyn Loader>) {
    *LOADER.write().unwrap_or_else(PoisonError::into_inner) = Some(loader);
}

pub(super) fn installed() -> Option<Arc<dyn Loader>> {
    LOADER.read().unwrap_or_else(PoisonError::into_inner).clone()
}

pub(super) fn global() -> &'static Store {
    static STORE: LazyLock<Store> = LazyLock::new(|| Store::new(STORE_BUDGET, MAX_LINKED_BYTES));
    &STORE
}

type Outcome = Result<Arc<Vec<u8>>, Unavailable>;

/// One load in progress, which the other loads of the same bytes wait for.
#[derive(Default)]
struct Flight {
    outcome: Mutex<Option<Outcome>>,
    landed: Condvar,
}

impl Flight {
    fn wait(&self) -> Outcome {
        let mut g = self.outcome.lock().unwrap_or_else(PoisonError::into_inner);
        loop {
            if let Some(outcome) = g.as_ref() {
                return outcome.clone();
            }
            g = self.landed.wait(g).unwrap_or_else(PoisonError::into_inner);
        }
    }
}

struct Kept {
    bytes: Arc<Vec<u8>>,
    stamp: u64,
}

#[derive(Default)]
struct Inner {
    kept: HashMap<SourceKey, Kept>,
    flights: HashMap<SourceKey, Arc<Flight>>,
    bytes: u64,
    clock: u64,
}

impl Inner {
    /// The bytes kept under `key`, marked used.
    fn touch(&mut self, key: SourceKey) -> Option<Arc<Vec<u8>>> {
        self.clock += 1;
        let clock = self.clock;
        self.kept.get_mut(&key).map(|k| {
            k.stamp = clock;
            Arc::clone(&k.bytes)
        })
    }

    /// Keep `bytes` under `key`, dropping the least recently used others beyond `budget`.
    fn keep(&mut self, key: SourceKey, bytes: Arc<Vec<u8>>, budget: u64) {
        self.clock += 1;
        let n = bytes.len() as u64;
        if let Some(old) = self.kept.insert(key, Kept { bytes, stamp: self.clock }) {
            self.bytes = self.bytes.saturating_sub(old.bytes.len() as u64);
        }
        self.bytes = self.bytes.saturating_add(n);
        while self.bytes > budget {
            let Some(oldest) = self.kept.iter().filter(|(k, _)| **k != key).min_by_key(|(_, k)| k.stamp).map(|(k, _)| *k) else { break };
            if let Some(old) = self.kept.remove(&oldest) {
                self.bytes = self.bytes.saturating_sub(old.bytes.len() as u64);
            }
        }
    }
}

/// A single-flight, least-recently-used store of loaded bytes by [`SourceKey`].
pub(super) struct Store {
    inner: Mutex<Inner>,
    budget: u64,
    cap: u64,
}

/// Lands a flight when dropped: with its outcome, or as missing when the loader panicked, so
/// the loads waiting for it don't wait forever.
struct Landing<'a> {
    store: &'a Store,
    key: SourceKey,
    flight: Arc<Flight>,
    outcome: Option<Outcome>,
}

impl Drop for Landing<'_> {
    fn drop(&mut self) {
        let outcome = self.outcome.take().unwrap_or(Err(Unavailable::Missing));
        {
            let mut g = self.store.lock();
            g.flights.remove(&self.key);
            if let Ok(bytes) = &outcome
                && bytes.len() as u64 <= self.store.budget
            {
                g.keep(self.key, Arc::clone(bytes), self.store.budget);
            }
        }
        *self.flight.outcome.lock().unwrap_or_else(PoisonError::into_inner) = Some(outcome);
        self.flight.landed.notify_all();
    }
}

impl Store {
    pub(super) fn new(budget: u64, cap: u64) -> Self {
        Store { inner: Mutex::new(Inner::default()), budget, cap }
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The bytes of `linked`, kept under `key`: from the store, from a load of them in progress,
    /// or read by `loader`.
    pub(super) fn load(&self, key: SourceKey, linked: &Linked, loader: Option<&dyn Loader>) -> Outcome {
        if linked.source.len > self.cap {
            return Err(Unavailable::TooLarge);
        }
        let flight = {
            let mut g = self.lock();
            if let Some(bytes) = g.touch(key) {
                return Ok(bytes);
            }
            if let Some(f) = g.flights.get(&key).cloned() {
                drop(g);
                return f.wait();
            }
            let Some(loader) = loader else { return Err(Unavailable::NoLoader) };
            let f = Arc::new(Flight::default());
            g.flights.insert(key, Arc::clone(&f));
            (f, loader)
        };
        let (flight, loader) = flight;
        let mut landing = Landing { store: self, key, flight, outcome: None };
        // Read outside the lock: other bytes load and are found meanwhile.
        let outcome = match loader.load(&linked.source, linked.trusted()) {
            Ok(l) if l.bytes.len() as u64 > self.cap => Err(Unavailable::TooLarge),
            Ok(l) => {
                if let Some(fingerprint) = l.fingerprint {
                    linked.trust(fingerprint);
                }
                Ok(Arc::new(l.bytes))
            }
            Err(e) => Err(e),
        };
        landing.outcome = Some(outcome.clone());
        drop(landing);
        outcome
    }

    /// The bytes kept under `key`, without loading them.
    pub(super) fn resident(&self, key: SourceKey) -> Option<Arc<Vec<u8>>> {
        self.lock().touch(key)
    }

    /// Keep `bytes` under `key` as if a load had read them (not when over the budget).
    pub(super) fn keep(&self, key: SourceKey, bytes: Arc<Vec<u8>>) {
        if bytes.len() as u64 <= self.budget {
            self.lock().keep(key, bytes, self.budget);
        }
    }

    pub(super) fn evict(&self, key: SourceKey) {
        let mut g = self.lock();
        if let Some(old) = g.kept.remove(&key) {
            g.bytes = g.bytes.saturating_sub(old.bytes.len() as u64);
        }
    }

    #[cfg(test)]
    fn kept_bytes(&self) -> u64 {
        self.lock().bytes
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::source;
    use super::super::{AssetData, Repr};
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Serves `len` bytes of `fill` for every path, counting its reads; slow, so loads overlap.
    #[derive(Default)]
    struct Counting {
        reads: AtomicUsize,
        delay_ms: u64,
        fail: Option<Unavailable>,
        trusted: Mutex<Vec<Option<Fingerprint>>>,
    }

    impl Loader for Counting {
        fn load(&self, source: &LinkedSource, trusted: Option<Fingerprint>) -> Result<Loaded, Unavailable> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            self.trusted.lock().unwrap().push(trusted);
            std::thread::sleep(std::time::Duration::from_millis(self.delay_ms));
            if let Some(e) = self.fail {
                return Err(e);
            }
            let fill = source.path.bytes().last().unwrap_or(0);
            Ok(Loaded { bytes: vec![fill; source.len as usize], fingerprint: Some(Fingerprint { size: source.len, mtime: 5 }) })
        }
    }

    fn linked(data: &AssetData) -> &Linked {
        match &data.repr {
            Repr::Linked(l) => l,
            Repr::Embedded(_) => panic!("not linked"),
        }
    }

    fn load(store: &Store, data: &AssetData, loader: &dyn Loader) -> Outcome {
        store.load(data.key(), linked(data), Some(loader))
    }

    #[test]
    fn linked_bytes_are_read_once_and_kept() {
        let store = Store::new(100, 1000);
        let loader = Counting::default();
        let a = AssetData::linked(source("a", 10));
        assert_eq!(load(&store, &a, &loader).unwrap().as_slice(), [b'a'; 10]);
        assert_eq!(load(&store, &a.clone(), &loader).unwrap().len(), 10);
        assert_eq!(loader.reads.load(Ordering::SeqCst), 1);
        assert_eq!(store.resident(a.key()).map(|b| b.len()), Some(10));
        assert_eq!(*loader.trusted.lock().unwrap(), [None]);
        store.evict(a.key());
        load(&store, &a, &loader).unwrap();
        assert_eq!(loader.reads.load(Ordering::SeqCst), 2, "evicted bytes are read again");
        assert_eq!(
            loader.trusted.lock().unwrap().last(),
            Some(&Some(Fingerprint { size: 10, mtime: 5 })),
            "a hash-checked fingerprint is trusted next time"
        );
        assert_eq!(AssetData::linked(source("x", 1)).load(), Err(Unavailable::NoLoader), "no loader installed in these tests");
    }

    #[test]
    fn the_least_recently_used_bytes_go_past_the_budget() {
        let store = Store::new(100, 1000);
        let loader = Counting::default();
        let [a, b, c] = ["a", "b", "c"].map(|p| AssetData::linked(source(p, 40)));
        load(&store, &a, &loader).unwrap();
        load(&store, &b, &loader).unwrap();
        load(&store, &a, &loader).unwrap();
        load(&store, &c, &loader).unwrap();
        assert!(store.resident(b.key()).is_none(), "b was used least recently");
        assert!(store.resident(a.key()).is_some() && store.resident(c.key()).is_some());
        assert_eq!(store.kept_bytes(), 80);
    }

    #[test]
    fn a_file_over_the_budget_is_returned_but_not_kept_and_one_over_the_cap_is_not_read() {
        let store = Store::new(100, 1000);
        let loader = Counting::default();
        let small = AssetData::linked(source("s", 50));
        load(&store, &small, &loader).unwrap();
        let big = AssetData::linked(source("b", 200));
        assert_eq!(load(&store, &big, &loader).unwrap().len(), 200);
        assert!(store.resident(big.key()).is_none());
        assert!(store.resident(small.key()).is_some(), "an unkept file evicts nothing");
        let huge = AssetData::linked(source("h", 1001));
        assert_eq!(load(&store, &huge, &loader), Err(Unavailable::TooLarge));
        assert_eq!(loader.reads.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn concurrent_loads_of_the_same_bytes_read_once() {
        let store = Store::new(1000, 1000);
        let loader = Counting { delay_ms: 50, ..Default::default() };
        let a = AssetData::linked(source("a", 10));
        let got: Vec<Outcome> = std::thread::scope(|s| {
            let handles: Vec<_> = (0..8).map(|_| s.spawn(|| load(&store, &a, &loader))).collect();
            handles.into_iter().map(|h| h.join().unwrap()).collect()
        });
        assert!(got.iter().all(|g| g.as_ref().is_ok_and(|b| b.len() == 10)));
        assert_eq!(loader.reads.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_failed_load_is_shared_and_tried_again_later() {
        let store = Store::new(1000, 1000);
        let loader = Counting { delay_ms: 30, fail: Some(Unavailable::Modified), ..Default::default() };
        let a = AssetData::linked(source("a", 10));
        let got: Vec<Outcome> = std::thread::scope(|s| {
            let handles: Vec<_> = (0..4).map(|_| s.spawn(|| load(&store, &a, &loader))).collect();
            handles.into_iter().map(|h| h.join().unwrap()).collect()
        });
        assert!(got.iter().all(|g| *g == Err(Unavailable::Modified)));
        assert!(store.resident(a.key()).is_none(), "nothing adopted");
        let reads = loader.reads.load(Ordering::SeqCst);
        load(&store, &a, &loader).unwrap_err();
        assert_eq!(loader.reads.load(Ordering::SeqCst), reads + 1, "failures aren't remembered here");
    }

    /// A loader that panics lands its flight: the loads waiting for it return.
    #[test]
    fn a_panicking_loader_does_not_hang_the_waiters() {
        struct Panics;
        impl Loader for Panics {
            fn load(&self, _: &LinkedSource, _: Option<Fingerprint>) -> Result<Loaded, Unavailable> {
                std::thread::sleep(std::time::Duration::from_millis(30));
                panic!("loader fault");
            }
        }
        let store = Store::new(1000, 1000);
        let a = AssetData::linked(source("a", 10));
        std::thread::scope(|s| {
            let first = s.spawn(|| load(&store, &a, &Panics));
            std::thread::sleep(std::time::Duration::from_millis(5));
            let waiter = s.spawn(|| load(&store, &a, &Panics));
            assert!(first.join().is_err());
            assert!(waiter.join().is_ok_and(|r| r.is_err()));
        });
    }
}
