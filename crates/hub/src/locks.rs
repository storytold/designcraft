//! Region locks (CraftHub spec §6.7, §7.6): DesignCraft is the adoption table's `PageRange`
//! row — "region locks of the `PageRange` shape". Locks sync as **document properties**
//! (§6.7's rule), and this module is that property's shape.
//!
//! The lock types live in the shared checkout's `crafthub-authz` crate
//! (`crates/crafthub-authz/src/locks.rs`), not in the pinned `craft-collab` dependency, so
//! their shape is mirrored here field for field — same wire form, same variant names — and
//! credited to its origin. When the org adopts `craft-libs`/`crafthub` as the canonical home
//! and the lock types move into the core, this module collapses to a re-export.
//!
//! ## Where the list lives, honestly
//!
//! DesignCraft's `Document` has no lock field yet: nothing in the file format stores regions.
//! Until the session wiring lands (behind the `crafthub` flag), the adapter holds the lock
//! list beside the document — exactly where it holds the sibling position keys — and exposes
//! it as the `regionLocks` document property on the wire (`view` emits it, `apply` accepts
//! it, the schema validates its shape). The session layer reads the list from the adapter to
//! tint locked page ranges and refuse covered edits with the reason; persisting locks into
//! the model is a one-field follow-up that needs no wire change, because the property already
//! exists.
//!
//! ## The DesignCraft meaning of a scope
//!
//! * [`LockScope::PageRange`] is the native shape: whole pages (and everything on them) lock,
//!   by **absolute page index** in the document's page order (the `page_count()` indexing,
//!   0-based — the same indexing `page_loc` and the Pages panel use; the *displayed* number
//!   per section is a projection of it).
//! * [`LockScope::Subtree`] locks a group (an item id); [`LockScope::ObjectProps`] locks
//!   properties by name (e.g. everyone edits content, nobody touches layout) — the schema
//!   validates those names against the property tables.
//! * The other variants (text ranges, timeline ranges, tracks) belong to the siblings
//!   (WordCraft, FilmCraft, SoundCraft); the shape stays complete on the wire so a lock list
//!   round-trips through every app, and a variant DesignCraft cannot resolve locally is
//!   simply never created here.

use serde::{Deserialize, Serialize};

/// What a lock does (the `crafthub-authz` shape, §7.6).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LockEffect {
    /// No edits at all inside the scope.
    ReadOnly,
    /// Edits divert into contributions (§6.7 `guard_batch`).
    SuggestOnly,
    /// A full-access island inside a stricter outer scope.
    Full,
}

/// A lock's scope — the types cover every family app (the `crafthub-authz` shape, §7.6).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum LockScope {
    /// Everything inside a group/page (`FigCraft`, `PhotoCraft`...; here: a group item).
    Subtree(u64),
    /// Specific layers (by `LayerId`).
    LayerSet(Vec<u64>),
    /// A text range (`WordCraft`/`DeckCraft`): node + char range.
    TextRange {
        /// The text node.
        node: u64,
        /// The first character.
        start: usize,
        /// The last character (exclusive).
        end: usize,
    },
    /// A timeline range (`FilmCraft`/`EffectCraft`).
    TimelineRange {
        /// The range's first tick.
        start: u64,
        /// The range's last tick.
        end: u64,
    },
    /// Whole tracks (`SoundCraft`).
    Tracks(Vec<u32>),
    /// A page/slide range — **DesignCraft's native shape**: absolute page indexes, inclusive.
    PageRange {
        /// The first page.
        first: u32,
        /// The last page.
        last: u32,
    },
    /// Specific object properties (e.g. everyone edits content, nobody touches layout).
    ObjectProps(Vec<String>),
}

/// One region lock (the `crafthub-authz` shape, §7.6).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RegionLock {
    /// The scope.
    pub scope: LockScope,
    /// Who the lock binds: role names or user ids.
    pub subjects: Vec<String>,
    /// What it does.
    pub effect: LockEffect,
    /// The label users see.
    pub label: String,
    /// An optional color for the region shading.
    pub color: Option<String>,
}

impl RegionLock {
    /// Parse one lock from its wire value (the `regionLocks` property is an array of these).
    ///
    /// Wrong shapes are errors, never panics — a malformed lock is refused with its reason.
    pub fn from_value(value: &craft_collab::Value) -> Result<Self, String> {
        serde_json::from_value(value.to_json()).map_err(|e| format!("region lock: {e}"))
    }

    /// Whether this lock covers absolute page index `page` — the Pages-panel tint question.
    ///
    /// Only a [`LockScope::PageRange`] covers pages directly; a subtree or layer lock covers a
    /// page only through the objects on it, which is the session layer's lookup, not the lock's.
    #[must_use]
    pub fn covers_page(&self, page: usize) -> bool {
        matches!(&self.scope, LockScope::PageRange { first, last } if page >= *first as usize && page <= *last as usize)
    }

    /// The property name the lock list syncs under (§6.7: locks are document properties).
    pub const PROP: &'static str = "regionLocks";
}

/// Parse a whole `regionLocks` property value; every entry must be a well-shaped lock.
pub fn parse_locks(value: &craft_collab::Value) -> Result<Vec<RegionLock>, String> {
    let json = value.to_json();
    let array = json.as_array().ok_or_else(|| "regionLocks must be an array of locks".to_string())?;
    array.iter().cloned().map(|v| serde_json::from_value::<RegionLock>(v).map_err(|e| format!("region lock: {e}"))).collect()
}
