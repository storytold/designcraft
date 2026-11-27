//! White or [Paper] set to overprint (#2401). A paint that lays down no ink knocks out what is
//! beneath it, which is how white shows on press. Set to overprint, it leaves the inks beneath
//! alone instead, so the object is gone from the printed page while the screen, without
//! Overprint Preview, still shows it.
//!
//! What can overprint: a page item's fill, stroke and stroke gap (Attributes panel), and ruby and
//! kenten set to overprint. Text itself has no overprint attribute, so white text knocks out and
//! prints; the frame's Overprint Fill covers the frame's own fill only.

use std::collections::{BTreeMap, HashSet};
use std::ops::Range;
use std::sync::Arc;

use designcraft_color::{Color, ColorType, SwatchValue, swatch};
use designcraft_compose::Cache;
use designcraft_doc::cjk_settings::AdornmentOverprint;
use designcraft_doc::{AnchoredObject, CharProps, Content, Document, Item, ItemId, SpreadRef, Story, StrokeType};
use designcraft_geom::Affine;
use designcraft_pdf::{drawn_children, item_exported, layer_exported, shown_parent_items};

use super::{Issue, page_of};

/// The issue's `kind`.
pub(super) const KIND: &str = "whiteOverprint";

/// Swatch tints that refer to tints, followed before giving up (as `swatch::resolve` does).
const MAX_TINT_DEPTH: usize = 8;

/// Whether a paint lays down no ink: [Paper] (whatever colour it is shown in), or a colour or
/// tint that comes out white (C=0 M=0 Y=0 K=0, a 0% tint, RGB white). A spot colour prints on
/// its own plate whatever colour stands in for it (a white ink, a varnish), so only a 0% tint of
/// it is white. [None] and gradients are not white.
fn prints_no_ink(d: &Document, name: &str, tint: f32) -> bool {
    const INK: f32 = 0.001;
    let (mut base, mut t) = (name, tint);
    for _ in 0..=MAX_TINT_DEPTH {
        match d.swatches.iter().find(|s| s.name == base).map(|s| &s.value) {
            Some(SwatchValue::Paper { .. }) => return true,
            Some(SwatchValue::Tint { base: b, tint: bt }) => (base, t) = (b, t * bt),
            // A NaN tint compares false: not white.
            Some(SwatchValue::Color { color_type: ColorType::Spot, .. }) => return t <= INK,
            Some(SwatchValue::Color { .. } | SwatchValue::Registration) => break,
            Some(SwatchValue::None | SwatchValue::Gradient { .. }) | None => return false,
        }
    }
    // A NaN component compares false: not white.
    match d.resolve_color(name, tint) {
        Some(Color::Cmyk { c, m, y, k }) => [c, m, y, k].iter().all(|v| *v <= INK),
        Some(Color::Gray { k }) => k <= INK,
        Some(Color::Rgb { r, g, b }) => [r, g, b].iter().all(|v| *v >= 1.0 - INK),
        None => false,
    }
}

/// "fill", "fill and stroke", "fill, stroke and stroke gap".
fn list(parts: &[&str]) -> String {
    match parts.split_last() {
        Some((last, rest)) if !rest.is_empty() => format!("{} and {last}", rest.join(", ")),
        Some((last, _)) => (*last).to_string(),
        None => String::new(),
    }
}

/// The warning for `parts` (paints, or ruby and kenten) of one item; none for no parts.
pub(super) fn issue(parts: &[&str], item: Option<u64>, page: Option<usize>) -> Option<Issue> {
    (!parts.is_empty()).then(|| Issue {
        severity: "warning",
        kind: KIND,
        message: format!("White or [Paper] {} set to overprint will not print", list(parts)),
        item,
        page,
    })
}

/// A page item to look at: a spread's or a parent page's (or inside one), or an anchored object.
enum Node {
    Item(Arc<Item>),
    Anchored(Arc<AnchoredObject>),
}

impl Node {
    fn item(&self) -> &Item {
        match self {
            Node::Item(it) => it,
            Node::Anchored(o) => &o.item,
        }
    }
}

/// Where an item is drawn: on spread `si` under its parent's transform (the page is the one it
/// sits on), or on a page given (a parent page's item, an anchored object).
#[derive(Clone, Copy)]
enum On {
    Spread(usize, Affine),
    Page(Option<usize>),
}

/// The page items PDF export draws, as it draws them: on visible, printable layers, without the
/// hidden and nonprinting ones and everything inside them, the parent pages' items on the pages
/// that show them, and anchored objects in the frames that place them. Returns the white
/// overprint issues of those items, and the page each text frame among them is first drawn on
/// (the frames whose ruby and kenten print).
pub(super) fn printed(cache: &Cache, d: &Document) -> (Vec<Issue>, BTreeMap<ItemId, Option<usize>>) {
    let mut issues = Vec::new();
    let mut frames = BTreeMap::new();
    // A parent page's item shows on every page based on it: it is reported once, on the first.
    // This also stops an anchored object that would contain its own frame.
    let mut seen = HashSet::new();
    for (si, sp) in d.spreads.iter().enumerate() {
        let first = d.first_page_of_spread(si);
        // With the page the PDF composes the item's text for (a parent page's, per page).
        let mut stack: Vec<(Node, On, Option<usize>)> = Vec::new();
        for pi in (0..sp.pages.len()).rev() {
            let page = Some(first + pi);
            stack.extend(shown_parent_items(d, si, pi).into_iter().rev().map(|(_, it)| (Node::Item(it.clone()), On::Page(page), page)));
        }
        let exported = sp.items.iter().rev().filter(|it| d.layer(it.layer).is_some_and(layer_exported));
        stack.extend(exported.map(|it| (Node::Item(it.clone()), On::Spread(si, Affine::IDENTITY), None)));
        while let Some((node, on, composed)) = stack.pop() {
            let it = node.item();
            if !item_exported(it) || !seen.insert(it.id) {
                continue;
            }
            let (page, inside) = match on {
                On::Spread(si, parent) => (page_of(d, SpreadRef::Doc(si), it, parent), On::Spread(si, parent * it.xf)),
                On::Page(page) => (page, on),
            };
            issues.extend(item_issue(d, it, page));
            let children: Vec<_> = drawn_children(it).map(|c| (Node::Item(c.clone()), inside, composed)).collect();
            if let Content::Text(tf) = &it.content {
                frames.entry(it.id).or_insert(page);
                let cs = cache.get(d, tf.story, composed);
                // The frame's anchored objects, and those in its footnotes and table cells.
                let mut pending: Vec<_> = cs.frame(it.id).into_iter().collect();
                while let Some(frame) = pending.pop() {
                    stack.extend(frame.objects.iter().map(|o| (Node::Anchored(o.object.clone()), On::Page(page), composed)));
                    pending.extend(frame.notes.iter().filter_map(|n| n.text.frames.first()));
                    pending.extend(frame.tables.iter().flat_map(|t| &t.cells).filter_map(|c| c.text.frames.first()));
                }
            }
            stack.extend(children.into_iter().rev());
        }
    }
    (issues, frames)
}

/// The byte ranges `story`'s frames show, for [`frames_showing`]: none for a story with no frame
/// of its own.
pub(super) fn frame_ranges(cache: &Cache, d: &Document, story: &Story) -> Vec<(ItemId, Range<usize>)> {
    if story.frames.is_empty() {
        return Vec::new();
    }
    cache.get(d, story.id, None).frames.iter().map(|f| (f.frame, f.range.clone())).collect()
}

/// The frames of `story` that show the bytes `run` (`shown`: [`frame_ranges`]). `None` stands
/// for a story with no frame of its own (a note's, a cell's, an unplaced one); text no frame has
/// room for (overset) is in none.
pub(super) fn frames_showing(story: &Story, shown: &[(ItemId, Range<usize>)], run: Range<usize>) -> Vec<Option<ItemId>> {
    if story.frames.is_empty() {
        return vec![None];
    }
    shown.iter().filter(|(_, r)| r.start < run.end && r.end > run.start).map(|(id, _)| Some(*id)).collect()
}

/// A page item's fill, stroke or stroke gap that is white and set to overprint.
fn item_issue(d: &Document, it: &Item, page: Option<usize>) -> Option<Issue> {
    if it.nonprinting {
        return None;
    }
    let mut parts = Vec::new();
    if it.fill.overprint && !it.fill.is_none() && prints_no_ink(d, &it.fill.swatch, it.fill.tint) {
        parts.push("fill");
    }
    let st = &it.stroke;
    if st.overprint && !st.is_none() && prints_no_ink(d, &st.swatch, st.tint) {
        parts.push("stroke");
    }
    // The gap colour shows between dashes, dots and stripes only.
    if st.gap_overprint && !st.is_none() && !matches!(d.stroke_kind(&st.kind), StrokeType::Solid) && prints_no_ink(d, &st.gap_swatch, st.gap_tint) {
        parts.push("stroke gap");
    }
    issue(&parts, Some(it.id.0), page)
}

/// Ruby and kenten of a run that are white and set to overprint, with the colours composition
/// gives them (`kenten::mark_props`, `ruby::ruby_style`): an empty swatch is the text's.
pub(super) fn adornments(d: &Document, p: &CharProps) -> Vec<&'static str> {
    let tint = |t: Option<f32>| t.filter(|t| t.is_finite()).map(|t| t.clamp(0.0, 1.0));
    let white = |on: AdornmentOverprint, name: &str, tint: f32, weight: Option<f64>| {
        on == AdornmentOverprint::On && name != swatch::NONE && weight.is_none_or(|w| w.is_finite() && w > 0.0) && prints_no_ink(d, name, tint)
    };
    let mut out = Vec::new();
    if p.kenten {
        let fill = if p.kenten_fill.is_empty() { &p.fill } else { &p.kenten_fill };
        let stroke = if p.kenten_stroke.is_empty() { &p.stroke } else { &p.kenten_stroke };
        let weight = p.kenten_stroke_weight.filter(|w| w.is_finite()).unwrap_or(p.stroke_weight);
        if white(p.kenten_overprint_fill, fill, tint(p.kenten_fill_tint).unwrap_or(p.fill_tint), None)
            || white(p.kenten_overprint_stroke, stroke, tint(p.kenten_stroke_tint).unwrap_or(p.stroke_tint), Some(weight))
        {
            out.push("kenten");
        }
    }
    if !p.ruby.is_empty() {
        // A ruby colour of its own is at 100% unless it has a tint; without one, the text's.
        let (fill, fill_tint) = if p.ruby_fill.is_empty() {
            (&p.fill, tint(p.ruby_fill_tint).unwrap_or(p.fill_tint))
        } else {
            (&p.ruby_fill, tint(p.ruby_fill_tint).unwrap_or(1.0))
        };
        let (stroke, stroke_tint) = if p.ruby_stroke.is_empty() {
            (&p.stroke, tint(p.ruby_stroke_tint).unwrap_or(p.stroke_tint))
        } else {
            (&p.ruby_stroke, tint(p.ruby_stroke_tint).unwrap_or(1.0))
        };
        let weight = p.ruby_stroke_weight.filter(|w| w.is_finite()).unwrap_or(p.stroke_weight);
        if white(p.ruby_overprint_fill, fill, fill_tint, None) || white(p.ruby_overprint_stroke, stroke, stroke_tint, Some(weight)) {
            out.push("ruby");
        }
    }
    out
}

#[cfg(test)]
mod tests;
