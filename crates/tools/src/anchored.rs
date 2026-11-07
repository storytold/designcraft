//! Anchored objects on the canvas: where composition placed them, so the Selection tool can
//! select and resize them (they live in the text, not on a spread).

use designcraft_compose::Cache;
use designcraft_doc::{AnchoredSel, Document, SpreadRef, StoryId};
use designcraft_geom::{Affine, Rect};

use crate::layout::CanvasLayout;

/// An anchored object as placed: its spread, its bounds there and on the canvas, and whether its
/// frame turns it (a turned object is selected but not resized by its handles).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placed {
    pub sel: AnchoredSel,
    pub spread: SpreadRef,
    pub spread_rect: Rect,
    pub canvas_rect: Rect,
    pub turned: bool,
}

/// The anchored objects of `story` (every story's when `None`) composed in frames on the
/// canvas's spreads, in text order. Objects in footnotes and table cells are not included.
pub fn placed(doc: &Document, cache: &Cache, layout: &CanvasLayout, story: Option<StoryId>) -> Vec<Placed> {
    let mut out = Vec::new();
    let stories: Vec<StoryId> = match story {
        Some(s) => vec![s],
        None => doc.stories.iter().filter(|(_, s)| !s.objects.is_empty()).map(|(id, _)| *id).collect(),
    };
    for sid in stories {
        if doc.story(sid).is_none_or(|s| s.objects.is_empty()) {
            continue;
        }
        let cs = cache.get(doc, sid, None);
        for ft in &cs.frames {
            if ft.objects.is_empty() {
                continue;
            }
            let (Some(loc), Some(frame)) = (doc.find(ft.frame), doc.item(ft.frame)) else { continue };
            let layer = doc.layer(frame.layer);
            if frame.hidden || layer.is_some_and(|l| !l.visible || l.locked) {
                continue;
            }
            let Some(slot) = layout.slot(loc.spread) else { continue };
            let xf = doc.parent_xf(&loc) * doc.text_xf(frame);
            let [a, b, c, d, _, _] = xf.as_coeffs();
            let turned = b.abs() > 1e-9 || c.abs() > 1e-9 || a <= 0.0 || d <= 0.0;
            for o in &ft.objects {
                // A locked object (or one on a locked layer) can't be selected, as other items.
                let it = &o.object.item;
                if it.locked || doc.layer(it.layer).is_some_and(|l| l.locked) {
                    continue;
                }
                let r = o.object.item.bounds();
                let r = Affine::translate(o.origin.to_vec2()).transform_rect_bbox(r);
                let spread_rect = xf.transform_rect_bbox(r);
                if ![spread_rect.x0, spread_rect.y0, spread_rect.x1, spread_rect.y1].iter().all(|v| v.is_finite()) {
                    continue;
                }
                out.push(Placed {
                    sel: AnchoredSel { story: sid, index: o.index },
                    spread: loc.spread,
                    spread_rect,
                    canvas_rect: slot.xf.transform_rect_bbox(spread_rect),
                    turned,
                });
            }
        }
    }
    out
}

/// The selected anchored object, as placed (`None` when it isn't shown on these spreads).
pub fn selected(doc: &Document, cache: &Cache, layout: &CanvasLayout, sel: Option<AnchoredSel>) -> Option<Placed> {
    let sel = sel?;
    placed(doc, cache, layout, Some(sel.story)).into_iter().find(|p| p.sel == sel)
}

/// The anchored object under canvas point `p` (the last placed, so an object drawn over another
/// wins).
pub fn at(doc: &Document, cache: &Cache, layout: &CanvasLayout, p: designcraft_geom::Point) -> Option<Placed> {
    placed(doc, cache, layout, None).into_iter().rev().find(|o| o.canvas_rect.contains(p))
}
