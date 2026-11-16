//! Resolve one physical margin's notes across independently composed stories.
use std::collections::{HashMap, HashSet};

use designcraft_doc::{Document, MarginSide, NotePlacement, StoryId};

use crate::{ComposeOptions, ComposedStory, FrameSpec};

struct Candidate {
    page: usize,
    left: bool,
    baseline: f64,
    first: f64,
    height: f64,
    gap: f64,
    story: StoryId,
    index: usize,
    frame: usize,
    note: usize,
    page_y: f64,
    bottom_limit: f64,
}

fn candidates(doc: &Document, cs: &ComposedStory, frames: &[FrameSpec], pages: &HashSet<usize>, out: &mut Vec<Candidate>) {
    for (fi, (ft, f)) in cs.frames.iter().zip(frames).enumerate() {
        let (Some(page), Some((page_rect, margin))) = (f.page, f.page_rect) else { continue };
        if !pages.contains(&page) || !doc.item_is_visible(ft.frame) {
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
            out.push(Candidate {
                page,
                left,
                baseline: n.reference_baseline - page_rect.y0,
                first: first.baseline,
                height: n.rect.height(),
                gap: o.space_between,
                story: cs.story,
                index: n.index,
                frame: fi,
                note: ni,
                page_y: page_rect.y0,
                bottom_limit: margin.y1 - page_rect.y0,
            });
        }
    }
}

/// Local note composition sets reference baselines and text metrics. Only the document-facing
/// entry points call this final pass; neighbour layouts never recurse into page stacking.
pub(crate) fn stack_page_margins(doc: &Document, sid: StoryId, opts: &ComposeOptions, out: &mut ComposedStory) {
    if !out.frames.iter().flat_map(|f| &f.notes).any(|n| !doc.note_options(n.series).placement.is_column_foot()) {
        return;
    }
    let frames = crate::frame_specs_placed(doc, sid, &opts.placed);
    let pages: HashSet<_> = frames.iter().filter_map(|f| f.page).collect();
    let mut all = Vec::new();
    candidates(doc, out, &frames, &pages, &mut all);
    for story in doc.stories.values().filter(|s| s.id != sid && s.notes.iter().any(|n| !doc.note_options(n.series).placement.is_column_foot())) {
        let frames = crate::frame_specs(doc, story.id);
        if !frames.iter().any(|f| f.page.is_some_and(|p| pages.contains(&p))) {
            continue;
        }
        let cs = crate::compose_story_unstacked(doc, story.id, &ComposeOptions::default());
        candidates(doc, &cs, &frames, &pages, &mut all);
    }
    all.sort_by(|a, b| a.page.cmp(&b.page).then(a.baseline.total_cmp(&b.baseline)).then(a.story.0.cmp(&b.story.0)).then(a.index.cmp(&b.index)));
    let mut bottoms: HashMap<(usize, bool), f64> = HashMap::new();
    out.warnings.retain(|w| !w.starts_with("Margin note "));
    for c in all {
        let requested = c.baseline - c.first;
        let top = bottoms.get(&(c.page, c.left)).map_or(requested, |bottom| requested.max(bottom + c.gap));
        let bottom = top + c.height;
        bottoms.insert((c.page, c.left), bottom);
        if c.story != sid {
            continue;
        }
        let Some(n) = out.frames.get_mut(c.frame).and_then(|f| f.notes.get_mut(c.note)) else { continue };
        let y = top + c.page_y;
        n.origin.y += y - n.rect.y0;
        n.rect.y1 += y - n.rect.y0;
        n.rect.y0 = y;
        n.overflow = bottom > c.bottom_limit + 0.01;
        if n.overflow {
            out.warnings.push(format!("Margin note {} in series {} runs below the bottom margin on page {}", n.id, n.series, c.page + 1));
        }
    }
}
