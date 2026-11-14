//! The bytes of a placed file ([`crate::Asset::data`]): held in memory (embedded) or read from
//! the linked file when needed. Caches of decoded images key by the value's [`SourceKey`],
//! which outlives the bytes: a decoded image stays usable after the encoded bytes are dropped.
//!
//! Linked bytes are read by the [`Loader`] the engine installs ([`install_loader`]; this crate
//! does no file I/O) and kept in a process-wide store of at most [`STORE_BUDGET`] bytes, least
//! recently used first out. A file over [`MAX_LINKED_BYTES`] is never read.

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError, Weak};

use crate::datamerge::Fingerprint;

mod store;

pub use store::{Loaded, Loader, MAX_LINKED_BYTES, STORE_BUDGET, install_loader};

/// Names one version of an asset's bytes. Every [`AssetData`] made from new bytes gets a key no
/// other value in this process has had; clones share it. Keys are never reused, so a cache
/// entry under a key can't be mistaken for different bytes allocated later. Counting starts at
/// 1: `SourceKey(0)` names no bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SourceKey(pub u64);

impl SourceKey {
    /// A key no other value has had.
    pub fn fresh() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        SourceKey(NEXT.fetch_add(1, Ordering::Relaxed))
    }
}

/// Why an asset's bytes can't be had.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Unavailable {
    #[error("the linked file is missing")]
    Missing,
    #[error("the linked file has changed")]
    Modified,
    #[error("the linked file is too large to load")]
    TooLarge,
    #[error("no loader for linked files")]
    NoLoader,
}

/// Where a linked asset's bytes are, and what they must be: the [`crate::Asset`] link record
/// ([`LinkedSource::of`]) with the document it is resolved against.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkedSource {
    /// The file's absolute path ([`crate::Asset::link`]), tried first.
    pub path: String,
    /// The path relative to the document's folder ([`crate::Asset::link_relative`]), tried when
    /// the absolute path doesn't hold the bytes.
    pub relative: Option<String>,
    /// The document file `relative` is resolved against.
    pub document: Option<PathBuf>,
    /// The size of the bytes.
    pub len: u64,
    /// Size and modification time of the file when it was known to hold the bytes: a file that
    /// still has them is read without hashing.
    pub fingerprint: Option<Fingerprint>,
    /// The bytes' content hash ([`crate::Asset::link_hash`], `sha256:` and hex digits), checked
    /// when the file's fingerprint differs.
    pub hash: Option<String>,
}

impl LinkedSource {
    /// The source `asset`'s link record names, resolved against the document file at
    /// `document`. `None` without a link, or without the size its fingerprint records.
    pub fn of(asset: &crate::Asset, document: Option<&Path>) -> Option<Self> {
        let fingerprint = asset.link_fingerprint?;
        Some(LinkedSource {
            path: asset.link.clone()?,
            relative: asset.link_relative.clone(),
            document: document.map(Path::to_path_buf),
            len: fingerprint.size,
            fingerprint: Some(fingerprint),
            hash: asset.link_hash.clone(),
        })
    }
}

/// A linked source, with the fingerprint a load last found the file at (hash-checked), so later
/// loads of a file whose time changed but whose bytes didn't skip the hash.
struct Linked {
    source: LinkedSource,
    trusted: Mutex<Option<Fingerprint>>,
}

impl Linked {
    fn trusted(&self) -> Option<Fingerprint> {
        *self.trusted.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn trust(&self, fingerprint: Fingerprint) {
        *self.trusted.lock().unwrap_or_else(PoisonError::into_inner) = Some(fingerprint);
    }
}

#[derive(Clone)]
enum Repr {
    Embedded(Arc<Vec<u8>>),
    Linked(Arc<Linked>),
}

/// An asset's bytes: embedded in the document, or linked (read from the file on demand).
/// Cloning is cheap and keeps the [`SourceKey`].
#[derive(Clone)]
pub struct AssetData {
    key: SourceKey,
    repr: Repr,
}

impl AssetData {
    /// Embedded bytes, under a new key.
    pub fn embedded(bytes: impl Into<Arc<Vec<u8>>>) -> Self {
        AssetData { key: SourceKey::fresh(), repr: Repr::Embedded(bytes.into()) }
    }

    /// Embedded bytes under `key`, which the caller made ([`SourceKey::fresh`]) for exactly these
    /// bytes: bytes derived again from the same input (a layer variant of the same file) keep
    /// the key their decodes are cached under. Different bytes under one key would draw stale
    /// pixels.
    pub fn embedded_under(key: SourceKey, bytes: impl Into<Arc<Vec<u8>>>) -> Self {
        AssetData { key, repr: Repr::Embedded(bytes.into()) }
    }

    /// The bytes of the linked file `source` names, under a new key.
    pub fn linked(source: LinkedSource) -> Self {
        AssetData { key: SourceKey::fresh(), repr: Repr::Linked(Arc::new(Linked { source, trusted: Mutex::new(None) })) }
    }

    /// The linked file `source` names, under `key`: embedded bytes found identical to the file
    /// (hash-checked by the caller) become a link without decoding them again. As with
    /// [`AssetData::embedded_under`], other bytes under one key would draw stale pixels.
    pub fn linked_under(key: SourceKey, source: LinkedSource) -> Self {
        AssetData { key, repr: Repr::Linked(Arc::new(Linked { source, trusted: Mutex::new(None) })) }
    }

    /// [`AssetData::linked_under`] with `bytes` (the file's, just read) already in the store, so
    /// the first draw doesn't read the file again.
    pub fn linked_resident(key: SourceKey, source: LinkedSource, bytes: Arc<Vec<u8>>) -> Self {
        store::global().keep(key, bytes);
        AssetData::linked_under(key, source)
    }

    /// This value pointing at `source` (the file moved, the document was saved elsewhere, the
    /// fingerprint was brought up to date). It keeps the key when both name the same bytes
    /// (same size and content hash), so nothing decoded is decoded again.
    pub fn with_source(&self, source: LinkedSource) -> Self {
        let same = self.linked_source().is_some_and(|old| old.len == source.len && old.hash.is_some() && old.hash == source.hash);
        let mut new = AssetData::linked(source);
        if same {
            new.key = self.key;
        }
        new
    }

    /// The bytes, when they are embedded.
    pub fn embedded_bytes(&self) -> Option<&Arc<Vec<u8>>> {
        match &self.repr {
            Repr::Embedded(b) => Some(b),
            Repr::Linked(_) => None,
        }
    }

    /// Where linked bytes are read from.
    pub fn linked_source(&self) -> Option<&LinkedSource> {
        match &self.repr {
            Repr::Embedded(_) => None,
            Repr::Linked(l) => Some(&l.source),
        }
    }

    /// The bytes: embedded ones, or the linked file's (read once by the installed [`Loader`],
    /// and kept while the store's budget allows). Concurrent loads of one value read it once.
    pub fn load(&self) -> Result<Arc<Vec<u8>>, Unavailable> {
        match &self.repr {
            Repr::Embedded(b) => Ok(Arc::clone(b)),
            Repr::Linked(l) => store::global().load(self.key, l, store::installed().as_deref()),
        }
    }

    /// The bytes without reading anything: embedded ones, or linked ones the store holds.
    pub fn resident(&self) -> Option<Arc<Vec<u8>>> {
        match &self.repr {
            Repr::Embedded(b) => Some(Arc::clone(b)),
            Repr::Linked(_) => store::global().resident(self.key),
        }
    }

    /// Keep `bytes`, just read from the linked file and checked by the caller to be the ones its
    /// source names, in the store, so the first [`AssetData::load`] doesn't read the file again.
    /// Embedded values, bytes of another size and bytes over the store's budget are ignored.
    pub fn keep_resident(&self, bytes: Arc<Vec<u8>>) {
        if let Repr::Linked(l) = &self.repr
            && bytes.len() as u64 == l.source.len
        {
            store::global().keep(self.key, bytes);
        }
    }

    /// Drop the store's copy of linked bytes (they are read again when next loaded).
    pub fn evict(&self) {
        if self.is_linked() {
            store::global().evict(self.key);
        }
    }

    pub fn key(&self) -> SourceKey {
        self.key
    }

    /// The size of the bytes.
    pub fn len(&self) -> u64 {
        match &self.repr {
            Repr::Embedded(b) => b.len() as u64,
            Repr::Linked(l) => l.source.len,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn is_linked(&self) -> bool {
        matches!(self.repr, Repr::Linked(_))
    }

    /// A handle that doesn't keep embedded bytes alive, and upgrades to this value (same key)
    /// while anything else does.
    pub fn downgrade(&self) -> WeakAssetData {
        let repr = match &self.repr {
            Repr::Embedded(b) => WeakRepr::Embedded(Arc::downgrade(b)),
            Repr::Linked(l) => WeakRepr::Linked(Arc::clone(l)),
        };
        WeakAssetData { key: self.key, repr }
    }
}

/// Advances when linked files may have come back or changed ([`links_changed`]).
static LINK_GENERATION: AtomicU64 = AtomicU64::new(0);

/// Say that linked files may have changed on disk (a relink, an update, a status check): caches
/// that remembered a linked file as unavailable ask for it again.
pub fn links_changed() {
    LINK_GENERATION.fetch_add(1, Ordering::AcqRel);
}

/// The count [`links_changed`] advances.
pub fn link_generation() -> u64 {
    LINK_GENERATION.load(Ordering::Acquire)
}

#[derive(Clone)]
enum WeakRepr {
    Embedded(Weak<Vec<u8>>),
    Linked(Arc<Linked>),
}

/// [`AssetData::downgrade`].
#[derive(Clone)]
pub struct WeakAssetData {
    key: SourceKey,
    repr: WeakRepr,
}

impl WeakAssetData {
    /// The value, while its embedded bytes are held elsewhere.
    pub fn upgrade(&self) -> Option<AssetData> {
        let repr = match &self.repr {
            WeakRepr::Embedded(w) => Repr::Embedded(w.upgrade()?),
            WeakRepr::Linked(l) => Repr::Linked(Arc::clone(l)),
        };
        Some(AssetData { key: self.key, repr })
    }

    pub fn key(&self) -> SourceKey {
        self.key
    }
}

impl fmt::Debug for WeakAssetData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WeakAssetData").field("key", &self.key.0).finish()
    }
}

/// New embedded bytes ([`AssetData::embedded`]).
impl From<Vec<u8>> for AssetData {
    fn from(bytes: Vec<u8>) -> Self {
        AssetData::embedded(bytes)
    }
}

impl Default for AssetData {
    fn default() -> Self {
        AssetData::embedded(Vec::new())
    }
}

/// Embedded bytes compare by content; linked ones are equal only as the same version.
impl PartialEq for AssetData {
    fn eq(&self, other: &Self) -> bool {
        match (&self.repr, &other.repr) {
            (Repr::Embedded(a), Repr::Embedded(b)) => a == b,
            (Repr::Linked(a), Repr::Linked(b)) => self.key == other.key && a.source.len == b.source.len,
            _ => false,
        }
    }
}

impl fmt::Debug for AssetData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = if self.is_linked() { "Linked" } else { "Embedded" };
        f.debug_struct(kind).field("key", &self.key.0).field("len", &self.len()).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) fn source(path: &str, len: u64) -> LinkedSource {
        LinkedSource { path: path.into(), relative: None, document: None, len, fingerprint: None, hash: None }
    }

    #[test]
    fn clones_keep_the_key_and_new_bytes_get_a_new_one() {
        let a = AssetData::embedded(vec![1, 2, 3]);
        let b = a.clone();
        assert_eq!(a.key(), b.key());
        let c = AssetData::embedded(vec![1, 2, 3]);
        assert_ne!(a.key(), c.key());
        assert_eq!(a, c, "embedded bytes compare by content");
        let shared = AssetData::embedded(Arc::clone(a.embedded_bytes().unwrap()));
        assert_ne!(a.key(), shared.key(), "wrapping bytes again names a new version");
        assert_eq!(a.load().unwrap().as_slice(), [1, 2, 3]);
        assert_eq!(a.len(), 3);
        let weak = a.downgrade();
        assert_eq!(weak.upgrade().map(|d| d.key()), Some(a.key()));
        drop((a, b));
        assert!(weak.upgrade().is_some(), "`shared` still holds the bytes");
        drop(shared);
        assert!(weak.upgrade().is_none());
    }

    #[test]
    fn linked_assets_keep_their_key_only_for_the_same_bytes() {
        let mut s = source("/nowhere/no-loader.png", 10);
        let l = AssetData::linked(s.clone());
        assert!(l.is_linked());
        assert_eq!(l.len(), 10);
        assert!(l.embedded_bytes().is_none() && l.resident().is_none());
        assert!(AssetData::default().is_empty());
        assert_ne!(l.with_source(s.clone()).key(), l.key(), "no hash: nothing says the bytes are the same");
        s.hash = Some("sha256:00".into());
        let hashed = AssetData::linked(s.clone());
        s.path = "/elsewhere/no-loader.png".into();
        let moved = hashed.with_source(s.clone());
        assert_eq!((moved.key(), moved.linked_source().map(|s| s.path.as_str())), (hashed.key(), Some("/elsewhere/no-loader.png")));
        s.hash = Some("sha256:01".into());
        assert_ne!(hashed.with_source(s).key(), hashed.key(), "other bytes, another key");
        let weak = hashed.downgrade();
        drop(hashed);
        assert!(weak.upgrade().is_some(), "a linked value holds no bytes to let go of");
    }

    #[test]
    fn a_link_record_names_its_source() {
        let fingerprint = Fingerprint { size: 7, mtime: 9 };
        let asset = crate::Asset {
            link: Some("/a/b.png".into()),
            link_relative: Some("b.png".into()),
            link_fingerprint: Some(fingerprint),
            link_hash: Some("sha256:ab".into()),
            ..Default::default()
        };
        let s = LinkedSource::of(&asset, Some(Path::new("/a/doc.dcx"))).unwrap();
        assert_eq!(
            (s.len, s.fingerprint, s.relative.as_deref(), s.document.as_deref()),
            (7, Some(fingerprint), Some("b.png"), Some(Path::new("/a/doc.dcx")))
        );
        assert!(LinkedSource::of(&crate::Asset { link_fingerprint: None, ..asset }, None).is_none(), "the size comes from the fingerprint");
    }
}
