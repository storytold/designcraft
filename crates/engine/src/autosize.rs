//! Text frame auto-size (Text Frame Options › Auto-Size): frames grow and shrink to fit their text.
//!
//! [`apply`] runs after every undoable command (so the resize is part of the same undo step). Opening
//! a file leaves frames at the sizes it gives them: those were fitted to the document's own fonts,
//! and a substitute font would otherwise move frames around text nobody edited. It measures each auto-size frame whose story, styles or own item changed,
//! with the frame opened to [`MAX_SIDE`], and resizes it in its inner space around its reference
//! point, so that point stays put in spread space whatever the frame's transform.
//!
//! Scope: an unthreaded frame or the last frame of a thread. Vertical (Japanese) frames and type
//! on a path are left as they are.

use std::sync::Arc;

use designcraft_compose::{ComposeOptions, ComposedStory, FrameSpec, compose, frame_specs};
use designcraft_doc::{AutoSize, Document, Item, ItemId, StoryId};
use designcraft_geom::{Affine, Rect, shapes};

/// Auto-size reference points, row-major from top-left (`TextFrameOptions::auto_size_ref`).
pub const REFERENCES: [&str; 9] =
    ["topLeft", "topCenter", "topRight", "leftCenter", "center", "rightCenter", "bottomLeft", "bottomCenter", "bottomRight"];

/// Largest side an auto-sized frame grows to (points).
pub const MAX_SIDE: f64 = 10_000.0;
/// Size differences below this are not worth a resize (points).
const EPS: f64 = 0.01;
/// Bisection steps for multi-column and proportional fitting.
const STEPS: usize = 24;

/// Resize the auto-size frames of `d` that `before` (the document before the edit) had with other
/// text, styles or geometry; every auto-size frame when `before` is `None`. Returns whether any
/// frame changed.
pub fn apply(d: &mut Document, before: Option<&Document>) -> bool {
    let todo = candidates(d, before);
    fit(d, todo)
}

/// Resize the frames `todo` to fit their text. Measuring is exact, so the second pass only
/// re-checks the frames the first one resized (their own wrap or neighbours may differ); the cap
/// keeps a pathological wrap from oscillating.
pub fn fit(d: &mut Document, mut todo: Vec<(ItemId, StoryId)>) -> bool {
    let mut changed = false;
    for _ in 0..3 {
        if todo.is_empty() {
            break;
        }
        let mut resized = Vec::new();
        for (fid, sid) in todo {
            if let Some(r) = fitted(d, fid, sid)
                && resize(d, fid, r)
            {
                resized.push((fid, sid));
            }
        }
        changed |= !resized.is_empty();
        todo = resized;
    }
    changed
}

/// Auto-size frames (id, story) whose story, styles or item changed since `before` (all of them
/// without it).
pub fn candidates(d: &Document, before: Option<&Document>) -> Vec<(ItemId, StoryId)> {
    fn walk(items: &[Arc<Item>], out: &mut Vec<(ItemId, StoryId)>) {
        for it in items {
            if let Some(tf) = it.text_frame()
                && tf.options.auto_size != AutoSize::Off
            {
                out.push((it.id, tf.story));
            }
            walk(it.children(), out);
        }
    }
    let mut all = Vec::new();
    for sp in d.spreads.iter().chain(d.parents.iter()) {
        walk(&sp.items, &mut all);
    }
    let Some(b) = before else { return all };
    let styles_same = Arc::ptr_eq(&d.styles, &b.styles);
    all.retain(|(fid, sid)| {
        let story_same = match (d.stories.get(sid), b.stories.get(sid)) {
            (Some(x), Some(y)) => Arc::ptr_eq(x, y),
            _ => false,
        };
        // The frame and the frames before it in the thread (they decide where its text starts).
        let same = |id: &ItemId| match (d.item(*id), b.item(*id)) {
            (Some(x), Some(y)) => x == y,
            _ => false,
        };
        let frames_same = same(fid) && d.stories.get(sid).is_some_and(|st| st.frames.iter().all(same));
        !(styles_same && story_same && frames_same)
    });
    all
}

/// What one composition of the frame says: the text height from the area top, the width the
/// longest line needs, and whether all of the story fits.
struct Measure {
    height: f64,
    width: f64,
    fits: bool,
}

/// The frame's new inner bounds, if auto-size applies and they differ from the current ones.
pub fn fitted(d: &Document, fid: ItemId, sid: StoryId) -> Option<Rect> {
    let item = d.item(fid)?;
    let tf = item.text_frame()?;
    let o = &tf.options;
    if o.auto_size == AutoSize::Off || o.path.is_some() || d.frame_vertical(item) {
        return None;
    }
    let story = d.story(sid)?;
    if story.frames.last() != Some(&fid) {
        return None;
    }
    let r = item.inner_bounds();
    let (w, h) = (r.width(), r.height());
    if !(w.is_finite() && h.is_finite() && r.x0.is_finite() && r.y0.is_finite()) {
        return None;
    }
    let inset = o.inset.map(|v| if v.is_finite() { v.clamp(0.0, MAX_SIDE) } else { 0.0 });
    let (pad_w, pad_h) = (inset[1] + inset[3], inset[0] + inset[2]);
    let specs = frame_specs(d, sid);
    let idx = specs.iter().position(|s| s.id == fid)?;
    let columns = o.columns.max(1);
    let gutter = if o.gutter.is_finite() { o.gutter.clamp(0.0, MAX_SIDE) } else { 0.0 };
    // Compose with the frame's text area set to `aw` × `ah`.
    let measure = |aw: f64, ah: f64| -> Option<Measure> {
        let mut specs: Vec<FrameSpec> = specs.clone();
        let s = specs.get_mut(idx)?;
        let a = s.area;
        let area = Rect::new(a.x0, a.y0, a.x0 + aw.clamp(0.0, MAX_SIDE), a.y0 + ah.clamp(0.0, MAX_SIDE));
        s.area = area;
        let cs: ComposedStory = compose(d, story, &specs, &ComposeOptions::default());
        let ft = cs.frame(fid)?;
        let width = ft
            .lines
            .iter()
            .map(|l| {
                let col = ft.columns.get(l.column as usize).copied().unwrap_or(area);
                let (lo, hi) = l.glyphs.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), g| (lo.min(g.x), hi.max(g.x + g.adv)));
                let ink = if hi >= lo { hi - lo } else { 0.0 };
                // Indents: the line slot is the column minus them.
                ink + (l.x0 - col.x0).max(0.0) + (col.x1 - l.x1).max(0.0)
            })
            .fold(0.0, f64::max);
        let width = columns as f64 * width + (columns - 1) as f64 * gutter;
        let height = if ft.lines.is_empty() { 0.0 } else { ft.content_height };
        Some(Measure { height, width, fits: cs.overset_at.is_none() })
    };
    // Height of the text set `aw` wide (a multi-column frame by bisection: its text spreads over the
    // columns).
    let text_height = |aw: f64| -> Option<f64> {
        let tall = measure(aw, MAX_SIDE)?;
        if columns == 1 || !tall.fits {
            return Some(tall.height);
        }
        let (mut lo, mut hi) = (0.0, tall.height);
        for _ in 0..STEPS {
            if hi - lo < EPS {
                break;
            }
            let mid = (lo + hi) / 2.0;
            if measure(aw, mid).is_some_and(|m| m.fits) { hi = mid } else { lo = mid }
        }
        Some(hi)
    };
    // Width of the longest line set without breaks (plus a hair, so it doesn't break when set).
    let unbroken_width = || -> Option<f64> { measure(MAX_SIDE, MAX_SIDE).map(|m| m.width + EPS) };
    let minimum = |on: bool, v: f64| if on && v.is_finite() { v.clamp(0.0, MAX_SIDE) } else { 0.0 };
    let min_h = minimum(o.auto_size_use_min_height, o.auto_size_min_height);
    let min_w = minimum(o.auto_size_use_min_width, o.auto_size_min_width);
    let nlb = o.auto_size_no_line_breaks;
    let (nw, nh) = match o.auto_size {
        AutoSize::Off => return None,
        AutoSize::HeightOnly => (w, text_height(w - pad_w)? + pad_h),
        AutoSize::WidthOnly => (unbroken_width()? + pad_w, h),
        AutoSize::HeightAndWidth if nlb => {
            let nw = (unbroken_width()? + pad_w).max(min_w);
            (nw, text_height(nw - pad_w)? + pad_h)
        }
        AutoSize::HeightAndWidth => (w, text_height(w - pad_w)? + pad_h),
        AutoSize::HeightAndWidthProportionally => {
            if w < EPS || h < EPS {
                return None;
            }
            let k = if nlb {
                let nw = (unbroken_width()? + pad_w).max(min_w);
                let nh = text_height(nw - pad_w)? + pad_h;
                (nw / w).max(nh / h)
            } else {
                // The smallest scale at which the text fits the scaled frame: the text gets
                // shorter as the frame widens, the frame taller, so one crossing. (Columns are not
                // balanced here: the text height is measured as one column.)
                let fits = |k: f64| measure(w * k - pad_w, MAX_SIDE).is_some_and(|m| m.height + pad_h <= h * k + EPS);
                let (mut lo, mut hi) = (EPS / w.min(h), MAX_SIDE / w.max(h));
                if !fits(hi) {
                    lo = hi;
                }
                for _ in 0..STEPS {
                    if hi - lo < 1e-4 {
                        break;
                    }
                    let mid = (lo + hi) / 2.0;
                    if fits(mid) { hi = mid } else { lo = mid }
                }
                hi.max(min_w / w).max(min_h / h)
            };
            (w * k, h * k)
        }
    };
    let grows_w = !matches!(o.auto_size, AutoSize::HeightOnly);
    let grows_h = !matches!(o.auto_size, AutoSize::WidthOnly);
    let nw = if grows_w { nw.max(min_w).max(pad_w) } else { w };
    let nh = if grows_h { nh.max(min_h).max(pad_h) } else { h };
    if !(nw.is_finite() && nh.is_finite()) {
        return None;
    }
    let (nw, nh) = (nw.clamp(0.0, MAX_SIDE), nh.clamp(0.0, MAX_SIDE));
    if (nw - w).abs() < EPS && (nh - h).abs() < EPS {
        return None;
    }
    // The reference point (row-major from top-left) stays where it is.
    let at = |lo: f64, size: f64, new: f64, k: u8| match k {
        0 => lo,
        1 => lo + (size - new) / 2.0,
        _ => lo + size - new,
    };
    let rp = o.auto_size_ref.min(8);
    let x0 = at(r.x0, w, nw, rp % 3);
    let y0 = at(r.y0, h, nh, rp / 3);
    Some(Rect::new(x0, y0, x0 + nw, y0 + nh))
}

/// Give frame `fid` the inner bounds `to` (its path scaled into them).
pub(crate) fn resize(d: &mut Document, fid: ItemId, to: Rect) -> bool {
    let Some(it) = d.item_mut(fid) else { return false };
    let r = it.inner_bounds();
    if r.width() < EPS || r.height() < EPS {
        it.path = shapes::rectangle(to);
    } else {
        let xf = Affine::translate((to.x0, to.y0))
            * Affine::scale_non_uniform(to.width() / r.width(), to.height() / r.height())
            * Affine::translate((-r.x0, -r.y0));
        it.path.transform(xf);
    }
    true
}
