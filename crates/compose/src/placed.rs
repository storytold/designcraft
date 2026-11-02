//! Where anchored text frames sit on their spread.
//!
//! An anchored frame is not a spread item: its host story's composition places it on a line. Its
//! own story is composed against that placement (page, page side, page rect, baseline grid), so
//! the host story is composed first.

use std::sync::Arc;

use designcraft_doc::{Document, Item, ItemId, SpreadRef, StoryId};
use designcraft_geom::Affine;

use crate::{ComposedStory, FrameText};

/// Hosts nested deeper than this (a note anchored in a note anchored in …, or stories anchored in
/// each other in a cycle) are composed where they are stored.
pub(crate) const MAX_DEPTH: usize = 8;

/// An anchored item's parent space on its spread, where its host story's composition placed it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    pub spread: SpreadRef,
    /// The item's parent space → spread space.
    pub xf: Affine,
}

/// The placements of `sid`'s anchored frames, from their host stories composed by `host`.
pub(crate) fn placements(doc: &Document, sid: StoryId, host: &mut dyn FnMut(StoryId) -> Arc<ComposedStory>) -> Vec<(ItemId, Placement)> {
    let Some(st) = doc.story(sid) else { return Vec::new() };
    let mut anchored = None;
    let mut out = Vec::new();
    for &fid in &st.frames {
        if doc.find(fid).is_some() {
            continue;
        }
        let items = anchored.get_or_insert_with(|| doc.anchored_items());
        // A frame anchored in its own story would need its own composition to place it.
        let Some(owner) = items.iter().find_map(|(owner, it)| (it.id == fid).then_some(*owner)).filter(|o| *o != sid) else { continue };
        if let Some(p) = locate(doc, &host(owner), fid) {
            out.push((fid, p));
        }
    }
    out
}

/// Where `cs` (a host story's composition) places item `fid`: in its frames' anchored objects,
/// footnotes and table cells.
fn locate(doc: &Document, cs: &ComposedStory, fid: ItemId) -> Option<Placement> {
    cs.frames.iter().find_map(|ft| {
        let loc = doc.find(ft.frame)?;
        let frame = doc.item_at(&loc)?;
        let xf = in_frame(ft, doc.parent_xf(&loc) * doc.text_xf(frame), fid, 0)?;
        Some(Placement { spread: loc.spread, xf })
    })
}

/// The parent space of `fid` among the objects of `ft`, whose text space maps to the spread by
/// `xf`.
fn in_frame(ft: &FrameText, xf: Affine, fid: ItemId, depth: usize) -> Option<Affine> {
    if depth > MAX_DEPTH {
        return None;
    }
    let objects = ft.objects.iter().find_map(|o| in_item(&o.object.item, xf * Affine::translate(o.origin.to_vec2()), fid, 0));
    objects
        .or_else(|| ft.notes.iter().find_map(|n| in_frame(n.text.frames.first()?, xf * Affine::translate(n.origin.to_vec2()), fid, depth + 1)))
        .or_else(|| {
            ft.tables
                .iter()
                .flat_map(|t| &t.cells)
                .find_map(|c| in_frame(c.text.frames.first()?, xf * Affine::translate(c.origin.to_vec2()), fid, depth + 1))
        })
}

/// The parent space of `fid` in `it` (it or a group member), `it`'s parent space mapping to the
/// spread by `parent`.
fn in_item(it: &Item, parent: Affine, fid: ItemId, depth: usize) -> Option<Affine> {
    if it.id == fid {
        return Some(parent);
    }
    if depth > crate::MAX_WRAP_DEPTH {
        return None;
    }
    let xf = parent * it.xf;
    it.children().iter().find_map(|c| in_item(c, xf, fid, depth + 1))
}
