//! Where anchored text frames sit on their spread.
//!
//! An anchored frame is not a spread item: its host story's composition places it on a line. Its
//! own story is composed against that placement (page, page side, page rect, baseline grid), so
//! the host story is composed first.

use std::collections::HashMap;
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

/// The placements of `sid`'s anchored frames, from their host stories composed by `host`. Each
/// host is asked for once, however many of the frames it holds.
pub(crate) fn placements(doc: &Document, sid: StoryId, host: &mut dyn FnMut(StoryId) -> Arc<ComposedStory>) -> Vec<(ItemId, Placement)> {
    placements_with_visibility(doc, sid, host, false)
}

/// Placements that actually paint: hidden host frames, containers and inactive states are skipped.
pub(crate) fn visible_placements(doc: &Document, sid: StoryId, host: &mut dyn FnMut(StoryId) -> Arc<ComposedStory>) -> Vec<(ItemId, Placement)> {
    placements_with_visibility(doc, sid, host, true)
}

fn placements_with_visibility(
    doc: &Document,
    sid: StoryId,
    host: &mut dyn FnMut(StoryId) -> Arc<ComposedStory>,
    visible_only: bool,
) -> Vec<(ItemId, Placement)> {
    let Some(st) = doc.story(sid) else { return Vec::new() };
    let mut anchored = None;
    let mut hosts: HashMap<StoryId, Arc<ComposedStory>> = HashMap::new();
    let mut out = Vec::new();
    for &fid in &st.frames {
        if doc.find(fid).is_some() {
            continue;
        }
        let items = anchored.get_or_insert_with(|| doc.anchored_items());
        if let Some(p) = locate(doc, items, sid, fid, &mut hosts, host, 0, visible_only) {
            out.push((fid, p));
        }
    }
    out
}

/// Where the host story of `fid`, an anchored frame of story `sid`, places it: in its frames'
/// anchored objects, footnotes and table cells. A host frame that is itself anchored is placed
/// the same way first, up to [`MAX_DEPTH`] hosts out. `items` is [`Document::anchored_items`].
#[allow(clippy::too_many_arguments)] // The optional paint-visibility policy follows the existing host traversal.
fn locate(
    doc: &Document,
    items: &[(StoryId, &Item)],
    sid: StoryId,
    fid: ItemId,
    hosts: &mut HashMap<StoryId, Arc<ComposedStory>>,
    host: &mut dyn FnMut(StoryId) -> Arc<ComposedStory>,
    depth: usize,
    visible_only: bool,
) -> Option<Placement> {
    if depth > MAX_DEPTH {
        return None;
    }
    // A frame anchored in its own story would need its own composition to place it.
    let owner = items.iter().find_map(|(owner, it)| (it.id == fid).then_some(*owner)).filter(|o| *o != sid)?;
    let cs = hosts.entry(owner).or_insert_with(|| host(owner)).clone();
    cs.frames.iter().find_map(|ft| {
        if let Some(loc) = doc.find(ft.frame) {
            if visible_only && !doc.item_is_visible(ft.frame) {
                return None;
            }
            let frame = doc.item_at(&loc)?;
            let xf = in_frame(doc, ft, doc.parent_xf(&loc) * doc.text_xf(frame), fid, 0, visible_only)?;
            return Some(Placement { spread: loc.spread, xf });
        }
        // The host frame is anchored too: find `fid` in it before placing the frame.
        let local = in_frame(doc, ft, Affine::IDENTITY, fid, 0, visible_only)?;
        let frame = items.iter().find_map(|(_, it)| (it.id == ft.frame).then_some(*it))?;
        let outer = locate(doc, items, owner, ft.frame, hosts, host, depth + 1, visible_only)?;
        Some(Placement { spread: outer.spread, xf: outer.xf * doc.text_xf(frame) * local })
    })
}

/// The parent space of `fid` among the objects of `ft`, whose text space maps to the spread by
/// `xf`.
fn in_frame(doc: &Document, ft: &FrameText, xf: Affine, fid: ItemId, depth: usize, visible_only: bool) -> Option<Affine> {
    if depth > MAX_DEPTH {
        return None;
    }
    let objects = ft.objects.iter().find_map(|o| in_item(doc, &o.object.item, xf * Affine::translate(o.origin.to_vec2()), fid, 0, visible_only));
    objects
        .or_else(|| {
            ft.notes
                .iter()
                .find_map(|n| in_frame(doc, n.text.frames.first()?, xf * Affine::translate(n.origin.to_vec2()), fid, depth + 1, visible_only))
        })
        .or_else(|| {
            ft.tables
                .iter()
                .flat_map(|t| &t.cells)
                .find_map(|c| in_frame(doc, c.text.frames.first()?, xf * Affine::translate(c.origin.to_vec2()), fid, depth + 1, visible_only))
        })
}

/// The parent space of `fid` in `it` (it or a group member), `it`'s parent space mapping to the
/// spread by `parent`.
fn in_item(doc: &Document, it: &Item, parent: Affine, fid: ItemId, depth: usize, visible_only: bool) -> Option<Affine> {
    if visible_only && (it.hidden || doc.layer(it.layer).is_some_and(|layer| !layer.visible)) {
        return None;
    }
    if it.id == fid {
        return Some(parent);
    }
    if depth > crate::MAX_WRAP_DEPTH {
        return None;
    }
    let xf = parent * it.xf;
    if visible_only {
        return it.shown_children().find_map(|c| in_item(doc, c, xf, fid, depth + 1, true));
    }
    it.children().iter().find_map(|c| in_item(doc, c, xf, fid, depth + 1, false))
}
