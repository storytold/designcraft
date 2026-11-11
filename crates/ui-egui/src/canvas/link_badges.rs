//! Link badges: a small mark at the top-left corner of a graphic frame whose linked file changed
//! (an amber triangle with `!`) or is missing (a red octagon with `?`), as the reference app marks
//! them. Clicking a modified badge updates the graphic (one undo step); clicking a missing one
//! asks for the file to relink to. The badge stays the same size on screen at any zoom.
//!
//! They show with the frame edges (View › Extras › Hide Frame Edges hides them) and not in the
//! preview screen modes, exports or prints: they are drawn over the canvas, never rendered into
//! the page. What they show comes from [`crate::link_watch`], so drawing them reads no file.

use crate::link_watch::LinkState;
use designcraft_doc::{Content, Item};
use egui::{Align2, FontId, Pos2, Rect, Shape, Stroke, pos2, vec2};

use super::{Affine, CanvasLayout, DesignApp, Tokens, Xf};

/// The badge's side, in points on screen.
pub const SIZE: f32 = 16.0;
/// How far inside the frame's top-left corner it sits.
const INSET: f32 = 3.0;

/// A badge on screen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Badge {
    pub rect: Rect,
    pub asset: u64,
    pub state: LinkState,
}

/// The badges the canvas shows now, in drawing order (the frontmost last).
pub(crate) fn badges(app: &DesignApp, xf: &Xf, screen: Rect) -> Vec<Badge> {
    let mut out = Vec::new();
    if !app.ui.frame_edges || app.ui.screen_mode != crate::ScreenMode::Normal {
        return out;
    }
    let Some(st) = app.session.active() else { return out };
    let w = &app.link_watch;
    // Most frames: nothing to mark, nothing to walk.
    if !w.flagged(st.uid) {
        return out;
    }
    let doc = &st.doc;
    let layout = CanvasLayout::new(doc, st.editing_parents);
    for slot in &layout.slots {
        let Some(sp) = doc.spread(slot.spread) else { continue };
        for it in &sp.items {
            collect(doc, it, slot.xf, xf, screen, &mut |asset| w.state(st.uid, asset), &mut out);
        }
    }
    out
}

fn collect(
    doc: &designcraft_doc::Document,
    it: &Item,
    a: Affine,
    xf: &Xf,
    screen: Rect,
    state: &mut impl FnMut(u64) -> Option<LinkState>,
    out: &mut Vec<Badge>,
) {
    if it.hidden || doc.layer(it.layer).is_some_and(|l| !l.visible) {
        return;
    }
    match &it.content {
        Content::Group { items } => {
            for c in items {
                collect(doc, c, a * it.xf, xf, screen, state, out);
            }
        }
        Content::Graphic(g) => {
            let Some(s) = state(g.asset.0).filter(|s| matches!(s, LinkState::Modified | LinkState::Missing)) else { return };
            let frame = xf.rect((a * it.xf).transform_rect_bbox(it.inner_bounds()));
            if !frame.is_finite() || !frame.intersects(screen) {
                return;
            }
            let rect = Rect::from_min_size(frame.min + vec2(INSET, INSET), vec2(SIZE, SIZE));
            out.push(Badge { rect, asset: g.asset.0, state: s });
        }
        _ => {}
    }
}

/// The frontmost badge at screen point `p`.
pub(crate) fn badge_at(app: &DesignApp, xf: &Xf, screen: Rect, p: Pos2) -> Option<Badge> {
    badges(app, xf, screen).into_iter().rev().find(|b| b.rect.contains(p))
}

/// Draws the badges.
pub(super) fn draw(app: &DesignApp, painter: &egui::Painter, xf: &Xf, screen: Rect, t: &Tokens) {
    for b in badges(app, xf, screen) {
        paint(painter, b, t);
    }
}

/// One badge, drawn in code.
pub(crate) fn paint(painter: &egui::Painter, b: Badge, t: &Tokens) {
    let r = b.rect;
    let c = r.center();
    let edge = Stroke::new(1.0, egui::Color32::from_black_alpha(140));
    match b.state {
        LinkState::Modified => {
            let tri = vec![pos2(c.x, r.min.y + 1.0), pos2(r.max.x - 0.5, r.max.y - 1.5), pos2(r.min.x + 0.5, r.max.y - 1.5)];
            painter.add(Shape::convex_polygon(tri, t.link_modified, edge));
            painter.text(pos2(c.x, c.y + 2.0), Align2::CENTER_CENTER, "!", FontId::proportional(11.0), t.link_modified_ink);
        }
        _ => {
            // A stop sign: an octagon with flat top and sides.
            let r8 = SIZE / 2.0 - 0.5;
            let octagon = (0..8)
                .map(|k| {
                    let a = std::f32::consts::FRAC_PI_8 + k as f32 * std::f32::consts::FRAC_PI_4;
                    pos2(c.x + r8 * a.cos(), c.y + r8 * a.sin())
                })
                .collect();
            painter.add(Shape::convex_polygon(octagon, t.link_missing, edge));
            painter.text(c, Align2::CENTER_CENTER, "?", FontId::proportional(11.0), t.link_missing_ink);
        }
    }
}

/// The hover tooltips (translated where they are shown).
pub(crate) const TIP_MODIFIED: &str = "The linked file was changed. Click to update it.";
pub(crate) const TIP_MISSING: &str = "The linked file is missing. Click to relink it.";

/// The hover tooltip's text.
pub(crate) fn tip(state: LinkState) -> &'static str {
    match state {
        LinkState::Modified => TIP_MODIFIED,
        _ => TIP_MISSING,
    }
}

/// A badge was clicked: update a modified graphic, relink a missing one.
pub(crate) fn activate(app: &mut DesignApp, b: Badge) {
    match b.state {
        LinkState::Modified => {
            crate::link_watch::update_one(app, b.asset);
        }
        LinkState::Missing => crate::panels::properties::relink(app, serde_json::json!({ "asset": b.asset, "path": null })),
        _ => {}
    }
}
