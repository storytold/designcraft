//! The bytes of a placed file ([`crate::Asset::data`]): held in memory (embedded) or read from
//! the linked file when needed. Caches of decoded images key by the value's [`SourceKey`],
//! which outlives the bytes: a decoded image stays usable after the encoded bytes are dropped.

use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Weak};

/// Names one version of an asset's bytes. Every [`AssetData`] made from new bytes gets a key no
/// other value in this process has had; clones share it. Keys are never reused, so a cache
/// entry under a key can't be mistaken for different bytes allocated later. Counting starts at
/// 1: `SourceKey(0)` names no bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SourceKey(pub u64);

impl SourceKey {
    fn next() -> Self {
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

#[derive(Clone)]
enum Repr {
    Embedded(Arc<Vec<u8>>),
    Linked { len: u64 },
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
        AssetData { key: SourceKey::next(), repr: Repr::Embedded(bytes.into()) }
    }

    /// The bytes of a linked file of `len` bytes, under a new key.
    pub fn linked(len: u64) -> Self {
        AssetData { key: SourceKey::next(), repr: Repr::Linked { len } }
    }

    /// The bytes, when they are embedded.
    pub fn embedded_bytes(&self) -> Option<&Arc<Vec<u8>>> {
        match &self.repr {
            Repr::Embedded(b) => Some(b),
            Repr::Linked { .. } => None,
        }
    }

    /// The bytes: embedded ones, or the linked file's.
    pub fn load(&self) -> Result<Arc<Vec<u8>>, Unavailable> {
        match &self.repr {
            Repr::Embedded(b) => Ok(Arc::clone(b)),
            Repr::Linked { .. } => Err(Unavailable::NoLoader),
        }
    }

    pub fn key(&self) -> SourceKey {
        self.key
    }

    /// The size of the bytes.
    pub fn len(&self) -> u64 {
        match &self.repr {
            Repr::Embedded(b) => b.len() as u64,
            Repr::Linked { len } => *len,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn is_linked(&self) -> bool {
        matches!(self.repr, Repr::Linked { .. })
    }

    /// A handle that doesn't keep embedded bytes alive, and upgrades to this value (same key)
    /// while anything else does.
    pub fn downgrade(&self) -> WeakAssetData {
        let repr = match &self.repr {
            Repr::Embedded(b) => WeakRepr::Embedded(Arc::downgrade(b)),
            Repr::Linked { len } => WeakRepr::Linked { len: *len },
        };
        WeakAssetData { key: self.key, repr }
    }
}

#[derive(Clone)]
enum WeakRepr {
    Embedded(Weak<Vec<u8>>),
    Linked { len: u64 },
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
            WeakRepr::Linked { len } => Repr::Linked { len: *len },
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
            (Repr::Linked { len: a }, Repr::Linked { len: b }) => self.key == other.key && a == b,
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
    fn linked_bytes_need_a_loader() {
        let l = AssetData::linked(10);
        assert!(l.is_linked());
        assert_eq!(l.len(), 10);
        assert!(l.embedded_bytes().is_none());
        assert_eq!(l.load(), Err(Unavailable::NoLoader));
        assert!(AssetData::default().is_empty());
    }
}
