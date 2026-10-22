//! Stable identifiers. All ids are unique within a document (one counter).

use serde::{Deserialize, Serialize};

macro_rules! id_type {
    ($name:ident, $prefix:literal) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Default)]
        #[serde(transparent)]
        pub struct $name(pub u64);
        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, concat!($prefix, "{}"), self.0)
            }
        }
    };
}

id_type!(ItemId, "i");
id_type!(StoryId, "s");
id_type!(SpreadId, "sp");
id_type!(PageId, "p");
id_type!(LayerId, "l");
id_type!(AssetId, "a");

/// Fresh story-local id. Keep monotonically increasing ids normally; a loaded MAX id must
/// not make the next insertion overwrite it. In that case choose the first unused gap.
pub(crate) fn next_local_id(ids: impl Iterator<Item = u64> + Clone) -> u64 {
    if let Some(next) = ids.clone().max().unwrap_or(0).checked_add(1) {
        return next;
    }
    let mut used: Vec<_> = ids.collect();
    used.sort_unstable();
    let mut next = 1u64;
    for id in used {
        if id > next {
            break;
        }
        if id == next {
            // An allocated Vec cannot contain every u64 id, so this always finds a gap.
            next = next.saturating_add(1);
        }
    }
    next
}
