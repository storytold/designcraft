//! Damage: which parts of the spreads changed between two versions of a document, so a view can
//! repaint just those regions (typing in a frame repaints the frame, not the window).
//!
//! Conservative: anything whose effect can reach beyond the changed items' own bounds (styles,
//! swatches, layers, parent pages, text wrap, effects, text that other stories show through
//! cross-references, variables or footnote numbers) reports "everything" (`None`).

use std::collections::HashMap;
use std::sync::Arc;

use designcraft_doc::{Document, Item, ItemId, SpreadRef, Story, StoryId};
use designcraft_geom::Rect;

/// Changed regions in spread coordinates, or `None` when the whole view must be repainted.
pub fn damage(old: &Document, new: &Document) -> Option<Vec<(SpreadRef, Rect)>> {
    damage_with(old, new, None)
}

/// [`damage`] using the composition cache: of a changed story, only the frames whose composed
/// text actually changed are damaged (typing on page 40 of a chapter repaints page 40).
pub fn damage_with(old: &Document, new: &Document, cache: Option<&designcraft_compose::Cache>) -> Option<Vec<(SpreadRef, Rect)>> {
    if std::ptr::eq(old, new) {
        return Some(vec![]);
    }
    if !Arc::ptr_eq(&old.styles, &new.styles)
        || old.swatches != new.swatches
        || old.layers != new.layers
        || old.settings != new.settings
        || old.sections != new.sections
        || old.text_variables != new.text_variables
        || old.footnote_options != new.footnote_options
        || old.xref_formats != new.xref_formats
        || old.spreads.len() != new.spreads.len()
        || old.parents.len() != new.parents.len()
        || old.parents.iter().zip(&new.parents).any(|(a, b)| !Arc::ptr_eq(a, b))
    {
        return None;
    }
    let mut out: Vec<(SpreadRef, Rect)> = Vec::new();
    // Stories whose text or frames changed (a resized frame reflows the rest of its thread).
    let mut stories: std::collections::BTreeSet<StoryId> = std::collections::BTreeSet::new();
    // Items.
    for (si, (a, b)) in old.spreads.iter().zip(&new.spreads).enumerate() {
        if Arc::ptr_eq(a, b) {
            continue;
        }
        if a.pages != b.pages {
            return None;
        }
        let olds: HashMap<ItemId, &Arc<Item>> = a.items.iter().map(|i| (i.id, i)).collect();
        let news: HashMap<ItemId, &Arc<Item>> = b.items.iter().map(|i| (i.id, i)).collect();
        // Stacking order changes affect overlaps: repaint the moved items' areas too.
        for (id, it) in &news {
            match olds.get(id) {
                Some(o) if Arc::ptr_eq(o, it) => {}
                Some(o) => {
                    out.push((SpreadRef::Doc(si), item_area(o)?));
                    out.push((SpreadRef::Doc(si), item_area(it)?));
                    stories.extend(text_stories(o).into_iter().chain(text_stories(it)));
                }
                None => {
                    out.push((SpreadRef::Doc(si), item_area(it)?));
                    stories.extend(text_stories(it));
                }
            }
        }
        for (id, o) in &olds {
            if !news.contains_key(id) {
                out.push((SpreadRef::Doc(si), item_area(o)?));
                stories.extend(text_stories(o));
            }
        }
        let order = |v: &[Arc<Item>]| v.iter().map(|i| i.id).filter(|id| olds.contains_key(id) && news.contains_key(id)).collect::<Vec<_>>();
        if order(&a.items) != order(&b.items) {
            // Reordered: repaint every item whose rank changed (cheap to over-approximate).
            for it in b.items.iter() {
                out.push((SpreadRef::Doc(si), item_area(it)?));
            }
        }
    }
    // Stories.
    let ids: std::collections::BTreeSet<StoryId> = old.stories.keys().chain(new.stories.keys()).copied().collect();
    let mut changed_text = false;
    for sid in ids {
        let (a, b) = (old.stories.get(&sid), new.stories.get(&sid));
        let edited = !matches!((a, b), (Some(a), Some(b)) if Arc::ptr_eq(a, b));
        if !edited && !stories.contains(&sid) {
            continue;
        }
        changed_text |= edited;
        // Footnote numbers continue across stories.
        if a.map_or(0, |s| s.notes.len()) != b.map_or(0, |s| s.notes.len()) {
            return None;
        }
        // Its anchored objects' text wrap reflows the other stories on its spreads.
        for d in [old, new] {
            for other in designcraft_compose::wrap::wrapped_by(d, sid) {
                for f in d.story(other).map(|s| s.frames.clone()).unwrap_or_default() {
                    out.push(frame_area(d, f)?);
                }
            }
        }
        // Only the frames whose composition changed, when the previous composition is known.
        if let (Some(c), Some(_), Some(_)) = (cache, a, b)
            && let Some(prev) = c.composed_for(old, sid)
        {
            let cur = c.get(new, sid, None);
            let changed = designcraft_compose::Cache::changed_frames(&prev, &cur);
            for f in changed {
                for d in [old, new] {
                    if d.find(f).is_some() {
                        out.push(frame_area(d, f)?);
                    }
                }
            }
            continue;
        }
        for (d, st) in [(old, a), (new, b)] {
            let Some(st) = st else { continue };
            for f in &st.frames {
                let (r, area) = frame_area(d, *f)?;
                out.push((r, area));
            }
        }
    }
    // Other stories can show this text (cross-references, running headers) or its pages.
    if changed_text && new.stories.values().any(|s| shows_other_text(new, s)) {
        return None;
    }
    // Parent-page frames aren't in `out` (parents are compared above); anything on a parent
    // spread already returned None.
    if out.iter().any(|(r, _)| !matches!(r, SpreadRef::Doc(_))) {
        return None;
    }
    Some(out)
}

/// Stories of the text frames in an item (groups included).
fn text_stories(it: &Item) -> Vec<StoryId> {
    let mut v = Vec::new();
    it.walk(&mut |i| {
        if let Some(t) = i.text_frame() {
            v.push(t.story);
        }
    });
    v
}

/// Does the story show text generated from other stories or pages?
fn shows_other_text(d: &Document, s: &Story) -> bool {
    !s.xrefs.is_empty() || s.text.chars().any(|c| designcraft_doc::vars::var_index(c).is_some()) || d.toc.as_ref().is_some_and(|t| t.story == s.id)
}

/// Area an item paints (spread coordinates), or None when it can't be bounded cheaply.
fn item_area(it: &Item) -> Option<Rect> {
    let mut ok = true;
    it.walk(&mut |i| {
        // Effects spill outside; text wrap reflows other frames.
        if i.effects.any() || i.has_wrap() {
            ok = false;
        }
    });
    if !ok {
        return None;
    }
    let pad = it.stroke.extent().max(0.0) + 2.0 + if it.is_text_frame() { 24.0 } else { 0.0 };
    Some(painted_bounds(it).inflate(pad, pad))
}

/// An item's bounds, grown to its text: text is composed in the box around the frame's path in
/// the frame's own coordinates, which a sheared then rotated frame turns past its path's bounds.
fn painted_bounds(it: &Item) -> Rect {
    let mut b = it.bounds();
    if it.is_text_frame() {
        b = b.union(it.xf.transform_rect_bbox(it.inner_bounds()));
    }
    for c in it.children() {
        b = b.union(it.xf.transform_rect_bbox(painted_bounds(c)));
    }
    b
}

/// A text frame's area: its spread and bounds with room for glyph overhang.
fn frame_area(d: &Document, f: ItemId) -> Option<(SpreadRef, Rect)> {
    let loc = d.find(f)?;
    let it = d.item_at(&loc)?;
    let b = d.parent_xf(&loc).transform_rect_bbox(painted_bounds(it));
    // Overhang of italics, swashes and big initials, and anchored objects pinned to lines.
    Some((loc.spread, b.inflate(24.0 + 0.1 * b.width().max(b.height()), 24.0)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use designcraft_doc::ParaFormat;
    use designcraft_doc::build::NewDocument;

    #[test]
    fn typing_damages_only_the_frame() {
        let mut d = Document::new(&NewDocument { pages: 2, ..Default::default() });
        let lid = d.default_layer();
        let (_, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(72.0, 72.0, 300.0, 200.0), lid, "Hello", ParaFormat::default()).unwrap();
        let (_, _) = d.add_text_frame(SpreadRef::Doc(1), Rect::new(72.0, 72.0, 300.0, 200.0), lid, "Other", ParaFormat::default()).unwrap();
        let old = d.clone();
        d.story_mut(sid).unwrap().insert(5, " world");
        let dmg = damage(&old, &d).unwrap();
        assert!(!dmg.is_empty());
        assert!(dmg.iter().all(|(r, rect)| *r == SpreadRef::Doc(0) && rect.contains(designcraft_geom::Point::new(100.0, 100.0))));
        // Nothing changed.
        assert_eq!(damage(&d, &d.clone()).unwrap().len(), 0);
        // A style change repaints everything.
        let old = d.clone();
        d.styles_mut().paragraph.push(designcraft_doc::ParagraphStyle {
            name: "X".into(),
            based_on: None,
            next_style: None,
            para: Default::default(),
            chars: Default::default(),
            shortcut: String::new(),
        });
        assert!(damage(&old, &d).is_none());
        // Moving an item damages where it was and where it is.
        let old = d.clone();
        let id = d.spreads[0].items[0].id;
        d.item_mut(id).unwrap().xf = designcraft_geom::Affine::translate((100.0, 0.0));
        let dmg = damage(&old, &d).unwrap();
        assert!(dmg.iter().any(|(_, r)| r.contains(designcraft_geom::Point::new(100.0, 100.0))));
        assert!(dmg.iter().any(|(_, r)| r.contains(designcraft_geom::Point::new(350.0, 100.0))));
    }

    #[test]
    fn editing_a_wrapping_anchored_object_damages_the_text_it_wraps() {
        let mut d = Document::new(&NewDocument { pages: 1, ..Default::default() });
        let lid = d.default_layer();
        let (_, a) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(36.0, 36.0, 200.0, 200.0), lid, "Anchor", ParaFormat::default()).unwrap();
        let (_, _) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(300.0, 400.0, 500.0, 600.0), lid, "Wrapped", ParaFormat::default()).unwrap();
        let mut it = designcraft_doc::Item::new(
            designcraft_doc::ItemId(d.alloc()),
            lid,
            designcraft_doc::Shape::Rectangle,
            designcraft_geom::shapes::rectangle(Rect::new(0.0, 0.0, 20.0, 20.0)),
        );
        it.wrap.mode = designcraft_doc::WrapMode::BoundingBox;
        let old = d.clone();
        d.story_mut(a).unwrap().insert_object(0, designcraft_doc::AnchoredObject::new(it, Default::default()));
        let dmg = damage(&old, &d).unwrap();
        assert!(dmg.iter().any(|(_, r)| r.contains(designcraft_geom::Point::new(400.0, 500.0))));
    }

    /// Channels where `old`'s render patched over `new`'s damage differs from `new`'s full render.
    fn patched_diff(old: &Document, new: &Document, (w, h): (u32, u32), zoom: f64) -> usize {
        use crate::{Placed, RenderOptions, Renderer};
        use designcraft_geom::{Affine, Vec2};
        let cache = designcraft_compose::Cache::new();
        let placed = [Placed { spread: SpreadRef::Doc(0), xf: Affine::translate(Vec2::ZERO) }];
        let view = Affine::scale(zoom);
        let mut r = Renderer::new();
        r.threads = 0;
        let opts = RenderOptions::default();
        let mut img = r.render(old, &cache, &placed, w, h, view, &opts);
        let full = r.render(new, &cache, &placed, w, h, view, &opts);
        // Patch the damaged pixels.
        let regions = damage(old, new).unwrap();
        let u = regions.iter().map(|(_, r)| *r).reduce(|a, b| a.union(b)).unwrap();
        let x0 = ((u.x0 * zoom).floor().max(0.0)) as u32;
        let y0 = ((u.y0 * zoom).floor().max(0.0)) as u32;
        let x1 = ((u.x1 * zoom).ceil() as u32).min(w);
        let y1 = ((u.y1 * zoom).ceil() as u32).min(h);
        let patch = r.render(new, &cache, &placed, x1 - x0, y1 - y0, Affine::translate((-(x0 as f64), -(y0 as f64))) * view, &opts);
        for y in y0..y1 {
            for x in x0..x1 {
                let pi = (((y - y0) * (x1 - x0) + (x - x0)) * 4) as usize;
                let ii = ((y * w + x) * 4) as usize;
                img.pixels[ii..ii + 4].copy_from_slice(&patch.pixels[pi..pi + 4]);
            }
        }
        img.pixels.iter().zip(&full.pixels).filter(|(a, b)| a.abs_diff(**b) > 2).count()
    }

    #[test]
    fn patched_render_equals_full_render() {
        let mut d = Document::new(&NewDocument { pages: 1, ..Default::default() });
        let lid = d.default_layer();
        let (_, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(72.0, 72.0, 300.0, 200.0), lid, "Hello there", ParaFormat::default()).unwrap();
        let old = d.clone();
        d.story_mut(sid).unwrap().insert(5, ", typed");
        let diff = patched_diff(&old, &d, (400, 300), 1.3);
        assert_eq!(diff, 0, "patched image differs from the full render in {diff} channels");
    }

    #[test]
    fn moving_a_sheared_turned_text_frame_leaves_no_trail() {
        use designcraft_geom::Affine;
        let mut d = Document::new(&NewDocument { pages: 1, ..Default::default() });
        let lid = d.default_layer();
        let text = "Lorem ipsum dolor sit amet. ".repeat(60);
        let (f, _) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(0.0, 0.0, 300.0, 200.0), lid, &text, ParaFormat::default()).unwrap();
        // Sheared outline (its box's corners are outside it, the text's first and last lines in
        // them), then turned: the text reaches past the outline's bounds.
        let it = d.item_mut(f).unwrap();
        it.path = designcraft_geom::shapes::rectangle(Rect::new(0.0, 0.0, 300.0, 200.0)).transformed(Affine::skew(0.5, 0.0));
        it.xf = Affine::translate((150.0, 60.0)) * Affine::rotate(30f64.to_radians());
        let old = d.clone();
        d.item_mut(f).unwrap().xf = Affine::translate((210.0, 90.0)) * Affine::rotate(30f64.to_radians());
        let diff = patched_diff(&old, &d, (700, 560), 1.0);
        assert_eq!(diff, 0, "the moved text left {diff} stale channels behind");
    }

    #[test]
    fn only_changed_frames_of_a_thread_are_damaged() {
        let mut d = Document::new(&NewDocument { pages: 3, facing_pages: false, ..Default::default() });
        let lid = d.default_layer();
        let text = "Lorem ipsum dolor sit amet. ".repeat(40);
        let (f1, sid) = d.add_text_frame(SpreadRef::Doc(0), Rect::new(72.0, 72.0, 300.0, 300.0), lid, &text, ParaFormat::default()).unwrap();
        let (f2, _) = d.add_text_frame(SpreadRef::Doc(1), Rect::new(72.0, 72.0, 300.0, 300.0), lid, "", ParaFormat::default()).unwrap();
        let (f3, _) = d.add_text_frame(SpreadRef::Doc(2), Rect::new(72.0, 72.0, 300.0, 300.0), lid, "", ParaFormat::default()).unwrap();
        d.thread(f1, f2).unwrap();
        d.thread(f2, f3).unwrap();
        let cache = designcraft_compose::Cache::new();
        let _ = cache.get(&d, sid, None);
        // Change a word in the last frame's text (same length): only that frame changes.
        let old = d.clone();
        let st = d.story_mut(sid).unwrap();
        let at = st.text.len() - 10;
        st.replace(at..at + 5, "XXXXX");
        let dmg = damage_with(&old, &d, Some(&cache)).unwrap();
        let spreads: Vec<SpreadRef> = dmg.iter().map(|(r, _)| *r).collect();
        assert!(!spreads.contains(&SpreadRef::Doc(0)), "first frame untouched: {dmg:?}");
        assert!(!dmg.is_empty());
        // Without the cache every frame of the thread is damaged.
        let mut all: Vec<SpreadRef> = Vec::new();
        for (r, _) in damage(&old, &d).unwrap() {
            if !all.contains(&r) {
                all.push(r);
            }
        }
        assert_eq!(all.len(), 3);
        // Resizing the first frame reflows the thread: later frames are damaged too.
        let old = d.clone();
        let _ = cache.get(&d, sid, None);
        let it = d.item_mut(f1).unwrap();
        it.path = designcraft_geom::shapes::rectangle(Rect::new(72.0, 72.0, 300.0, 150.0));
        let dmg = damage_with(&old, &d, Some(&cache)).unwrap();
        let spreads: Vec<SpreadRef> = dmg.iter().map(|(r, _)| *r).collect();
        assert!(spreads.contains(&SpreadRef::Doc(1)), "{dmg:?}");
    }
}
