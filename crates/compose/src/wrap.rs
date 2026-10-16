//! Text wrap around anchored objects.
//!
//! Page items wrap text through [`crate::frame_specs`]. An anchored object's position is known
//! only once its story is composed, so its wrap is applied afterwards:
//!
//! - **Other stories.** A story whose frames share a spread with another story's wrapping
//!   anchored objects is composed with those objects as obstacles, where the other story's own
//!   composition places them ([`neighbours`], [`compose_among`]). That composition doesn't
//!   include obstacles from further stories, so two stories whose anchored objects wrap each
//!   other's text don't depend on each other in a cycle.
//! - **Its own story.** A custom or above-line anchored object wraps the lines of its own story
//!   that come after the line holding its anchor; that line and the lines before it are never
//!   wrapped by it (an inline object sits in its line and wraps none of its own story). As in
//!   InDesign, the object is placed when its anchor line is set and wraps the text that follows
//!   in the same composition (`own_wrap` in the composer), so the layout doesn't depend on where
//!   an earlier composition put the objects.

use designcraft_doc::{AnchorPosition, AnchoredObject, Document, OBJECT_MARK, SpreadRef, Story, StoryId, WrapMode};
use designcraft_geom::{Affine, Rect};

use crate::{ComposeOptions, ComposedStory, Exclusion, FrameSpec};

/// A wrapping anchored object where its story's composition placed it.
#[derive(Clone, Debug)]
pub(crate) struct Obstacle {
    spread: SpreadRef,
    /// Wrap area (the object's bounds plus its wrap offsets) in spread space.
    rect: Rect,
    mode: WrapMode,
    /// Story byte of the anchor, for obstacles in their own story.
    anchor: Option<usize>,
}

fn wraps(o: &AnchoredObject) -> bool {
    o.item.wrap.mode != WrapMode::None && !o.item.hidden
}

/// Does `story` have an anchored object whose wrap can push text?
pub fn has_wrapping_objects(story: &Story) -> bool {
    story.objects.iter().any(|o| wraps(o))
}

/// Stories other than `sid` with wrapping anchored objects and a frame on a spread with one of
/// `sid`'s frames (in story id order).
pub fn neighbours(doc: &Document, sid: StoryId) -> Vec<StoryId> {
    let mut candidates: Vec<StoryId> = doc.stories.iter().filter(|(id, st)| **id != sid && has_wrapping_objects(st)).map(|(id, _)| *id).collect();
    if candidates.is_empty() {
        return candidates;
    }
    let spreads = story_spreads(doc, sid);
    candidates.retain(|c| story_spreads(doc, *c).iter().any(|s| spreads.contains(s)));
    candidates.sort_by_key(|c| c.0);
    candidates
}

/// Stories other than `sid` that `sid`'s wrapping anchored objects can push aside: those with a
/// frame on a spread with one of `sid`'s frames.
pub fn wrapped_by(doc: &Document, sid: StoryId) -> Vec<StoryId> {
    if !doc.story(sid).is_some_and(has_wrapping_objects) {
        return Vec::new();
    }
    let spreads = story_spreads(doc, sid);
    let mut out: Vec<StoryId> =
        doc.stories.keys().filter(|id| **id != sid && story_spreads(doc, **id).iter().any(|s| spreads.contains(s))).copied().collect();
    out.sort_by_key(|c| c.0);
    out
}

fn story_spreads(doc: &Document, sid: StoryId) -> Vec<SpreadRef> {
    let mut out = Vec::new();
    for f in doc.story(sid).map(|s| s.frames.as_slice()).unwrap_or_default() {
        if let Some(loc) = doc.find(*f)
            && !out.contains(&loc.spread)
        {
            out.push(loc.spread);
        }
    }
    out
}

/// The wrapping anchored objects `cs` (a composition of `story`) places. `own`: obstacles for the
/// story itself (inline objects left out, anchors recorded).
pub(crate) fn obstacles(doc: &Document, story: &Story, cs: &ComposedStory, own: bool) -> Vec<Obstacle> {
    let mut out = Vec::new();
    if !has_wrapping_objects(story) {
        return out;
    }
    let anchors: Vec<usize> = story.text.match_indices(OBJECT_MARK).map(|(i, _)| i).collect();
    for ft in cs.frames.iter().filter(|ft| !ft.objects.is_empty()) {
        let Some(loc) = doc.find(ft.frame) else { continue };
        let Some(frame) = doc.item_at(&loc) else { continue };
        // Text that isn't shown pushes nothing aside.
        if frame.hidden || doc.layer(frame.layer).is_some_and(|l| !l.visible) {
            continue;
        }
        let fxf = doc.parent_xf(&loc) * doc.text_xf(frame);
        for o in &ft.objects {
            let Some(obj) = story.objects.get(o.index).filter(|obj| wraps(obj)) else { continue };
            if own && matches!(obj.position, AnchorPosition::Inline { .. }) {
                continue;
            }
            let w = obj.item.wrap.offsets;
            let b = obj.item.bounds();
            let r = Rect::new(b.x0 - w[1], b.y0 - w[0], b.x1 + w[3], b.y1 + w[2]);
            let rect = (fxf * Affine::translate(o.origin.to_vec2())).transform_rect_bbox(r);
            if !rect.is_finite() {
                continue;
            }
            let anchor = if own {
                let Some(&a) = anchors.get(o.index) else { continue };
                Some(a)
            } else {
                None
            };
            out.push(Obstacle { spread: loc.spread, rect, mode: obj.item.wrap.mode, anchor });
        }
    }
    out
}

/// The exclusions `obs` add to each of `specs` (frames that ignore wrap, type on a path and
/// frames the obstacle doesn't reach get none).
pub(crate) fn exclusions(doc: &Document, specs: &[FrameSpec], obs: &[Obstacle]) -> Vec<Vec<Exclusion>> {
    let mut out = vec![Vec::new(); specs.len()];
    if obs.is_empty() {
        return out;
    }
    for (spec, ex) in specs.iter().zip(&mut out) {
        if spec.opts.ignore_wrap || spec.opts.path.is_some() {
            continue;
        }
        let Some(loc) = doc.find(spec.id) else { continue };
        let Some(item) = doc.item_at(&loc) else { continue };
        let inv = (doc.parent_xf(&loc) * doc.text_xf(item)).inverse();
        let a = spec.area;
        for ob in obs.iter().filter(|o| o.spread == loc.spread) {
            let r = inv.transform_rect_bbox(ob.rect);
            if r.is_finite() && r.x0 < a.x1 && r.x1 > a.x0 && r.y0 < a.y1 && r.y1 > a.y0 {
                ex.push(Exclusion { rect: r, mode: ob.mode, after: ob.anchor });
            }
        }
    }
    out
}

fn with_exclusions(specs: &[FrameSpec], added: &[Vec<Exclusion>]) -> Vec<FrameSpec> {
    specs
        .iter()
        .zip(added)
        .map(|(s, ex)| {
            let mut s = s.clone();
            s.exclusions.extend(ex.iter().cloned());
            s
        })
        .collect()
}

/// Compose story `sid` with the wrapping anchored objects of `others` (compositions of its
/// [`neighbours`]) and of its own.
pub(crate) fn compose_among(doc: &Document, sid: StoryId, opts: &ComposeOptions, others: &[&ComposedStory]) -> ComposedStory {
    let Some(st) = doc.story(sid) else { return ComposedStory { story: sid, ..Default::default() } };
    let mut specs = crate::frame_specs(doc, sid);
    if !others.is_empty() {
        let mut obs = Vec::new();
        for cs in others {
            if let Some(other) = doc.story(cs.story) {
                obs.extend(obstacles(doc, other, cs, false));
            }
        }
        let added = exclusions(doc, &specs, &obs);
        if added.iter().any(|e| !e.is_empty()) {
            specs = with_exclusions(&specs, &added);
        }
    }
    crate::compose(doc, st, &specs, opts)
}
