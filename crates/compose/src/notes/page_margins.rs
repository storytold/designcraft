//! Resolve one physical margin's notes across independently composed stories.
use std::collections::{HashMap, HashSet};

use designcraft_doc::{Document, MarginSide, NotePlacement, StoryId};
use designcraft_geom::{Affine, Point};

use crate::{ComposeOptions, ComposedStory, FrameSpec, Placement};
use designcraft_doc::ItemId;
use std::sync::Arc;

struct Candidate {
    page: usize,
    left: bool,
    baseline: f64,
    requested: f64,
    height: f64,
    gap: f64,
    story: StoryId,
    index: usize,
    frame: usize,
    note: usize,
    current_top: f64,
    to_local: Affine,
    bottom_limit: f64,
}

fn candidates(
    doc: &Document,
    cs: &ComposedStory,
    frames: &[FrameSpec],
    placed: &[(ItemId, Placement)],
    pages: &HashSet<usize>,
    out: &mut Vec<Candidate>,
) {
    for (fi, (ft, f)) in cs.frames.iter().zip(frames).enumerate() {
        let (Some(page), Some(_)) = (f.page, f.page_rect) else { continue };
        let Some(page_spec) = doc.page(page) else { continue };
        let xf = if let Some(loc) = doc.find(ft.frame) {
            let Some(item) = doc.item_at(&loc) else { continue };
            if !doc.item_is_visible(ft.frame) {
                continue;
            }
            doc.parent_xf(&loc) * doc.text_xf(item)
        } else {
            let Some(item) = doc.anchored_item(ft.frame) else { continue };
            let Some((_, placement)) = placed.iter().find(|(id, _)| *id == ft.frame) else { continue };
            if item.hidden || doc.layer(item.layer).is_some_and(|layer| !layer.visible) {
                continue;
            }
            placement.xf * doc.text_xf(item)
        };
        let to_local = xf.inverse();
        if !to_local.as_coeffs().iter().all(|v| v.is_finite()) {
            continue;
        }
        let vertical_scale = ((xf * Point::new(0.0, 1.0)) - (xf * Point::ORIGIN)).hypot();
        if !pages.contains(&page) {
            continue;
        }
        for (ni, n) in ft.notes.iter().enumerate() {
            let o = doc.note_options(n.series);
            let NotePlacement::Margin { side, .. } = o.placement else { continue };
            let Some(first) = n.text.frames.first().and_then(|f| f.lines.first()) else { continue };
            let left = match side {
                MarginSide::Left => true,
                MarginSide::Right => false,
                MarginSide::Inside => !f.left_page,
                MarginSide::Outside => f.left_page,
            };
            let physical = xf.transform_rect_bbox(n.rect);
            let mut requested = n.rect;
            let unstack = n.reference_baseline - first.baseline - n.origin.y;
            requested.y0 += unstack;
            requested.y1 += unstack;
            let requested = xf.transform_rect_bbox(requested);
            out.push(Candidate {
                page,
                left,
                baseline: (xf * Point::new(n.origin.x, n.reference_baseline)).y,
                requested: requested.y0,
                height: physical.height(),
                gap: o.space_between * vertical_scale,
                story: cs.story,
                index: n.index,
                frame: fi,
                note: ni,
                current_top: physical.y0,
                to_local,
                bottom_limit: page_spec.margin_rect().y1,
            });
        }
    }
}

// The uncached entry point resolves placements internally without changing its caller's opts.
// Reuse the same bounded host resolver here so anchored neighbours have page geometry too.
fn placements(doc: &Document, sid: StoryId, opts: &ComposeOptions) -> Vec<(ItemId, Placement)> {
    let host_opts = ComposeOptions { xrefs: opts.xrefs.clone(), running: opts.running.clone(), ..Default::default() };
    let visible = crate::placed::visible_placements(doc, sid, &mut |host| Arc::new(crate::compose_story_unstacked(doc, host, &host_opts)));
    if opts.placed.is_empty() {
        return visible;
    }
    // Cache callers provide authoritative transforms; use the traversal only for eligibility.
    opts.placed.iter().filter(|(id, _)| visible.iter().any(|(shown, _)| shown == id)).copied().collect()
}

/// Local note composition sets reference baselines and text metrics. Only the document-facing
/// entry points call this final pass; neighbour layouts never recurse into page stacking.
pub(crate) fn stack_page_margins(doc: &Document, sid: StoryId, opts: &ComposeOptions, out: &mut ComposedStory) {
    if !out.frames.iter().flat_map(|f| &f.notes).any(|n| matches!(doc.note_options(n.series).placement, NotePlacement::Margin { .. })) {
        return;
    }
    let placed = placements(doc, sid, opts);
    let frames = crate::frame_specs_placed(doc, sid, &placed);
    let pages: HashSet<_> = frames.iter().filter_map(|f| f.page).collect();
    let neighbour_opts = ComposeOptions { xrefs: opts.xrefs.clone(), running: opts.running.clone(), ..Default::default() };
    let mut all = Vec::new();
    candidates(doc, out, &frames, &placed, &pages, &mut all);
    for story in doc
        .stories
        .values()
        .filter(|s| s.id != sid && s.notes.iter().any(|n| matches!(doc.note_options(n.series).placement, NotePlacement::Margin { .. })))
    {
        let placed = placements(doc, story.id, &neighbour_opts);
        let frames = crate::frame_specs_placed(doc, story.id, &placed);
        if !frames.iter().any(|f| f.page.is_some_and(|p| pages.contains(&p))) {
            continue;
        }
        let cs = crate::compose_story_unstacked(doc, story.id, &ComposeOptions { placed: placed.clone(), ..neighbour_opts.clone() });
        candidates(doc, &cs, &frames, &placed, &pages, &mut all);
    }
    all.sort_by(|a, b| a.page.cmp(&b.page).then(a.baseline.total_cmp(&b.baseline)).then(a.story.0.cmp(&b.story.0)).then(a.index.cmp(&b.index)));
    let mut bottoms: HashMap<(usize, bool), f64> = HashMap::new();
    out.warnings.retain(|w| !w.starts_with("Margin note "));
    for c in all {
        let top = bottoms.get(&(c.page, c.left)).map_or(c.requested, |bottom| c.requested.max(bottom + c.gap));
        let bottom = top + c.height;
        bottoms.insert((c.page, c.left), bottom);
        if c.story != sid {
            continue;
        }
        let Some(n) = out.frames.get_mut(c.frame).and_then(|f| f.notes.get_mut(c.note)) else { continue };
        let delta = (c.to_local * Point::new(0.0, top - c.current_top)) - (c.to_local * Point::ORIGIN);
        n.origin.x += delta.x;
        n.origin.y += delta.y;
        n.rect.x0 += delta.x;
        n.rect.x1 += delta.x;
        n.rect.y0 += delta.y;
        n.rect.y1 += delta.y;
        n.overflow = bottom > c.bottom_limit + 0.01;
        if n.overflow {
            out.warnings.push(format!("Margin note {} in series {} runs below the bottom margin on page {}", n.id, n.series, c.page + 1));
        }
    }
}
