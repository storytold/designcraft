use super::page_decor::dashed;
use super::render::c32;
use super::rulers::hair;
use super::text_overlay::draw_text_selection;
use super::{
    Affine, CanvasLayout, Color32, Content, DRect, DesignApp, Document, Item, Point, Pos2, Rect, Selection, Stroke, StrokeKind, Xf, compose, pos2,
    vec2,
};

/// Spread-space → canvas transform for an item at `loc`.
pub(super) fn item_canvas_xf(doc: &Document, layout: &CanvasLayout, id: designcraft_doc::ItemId) -> Option<(Affine, &'static str)> {
    let loc = doc.find(id)?;
    // Items on spreads that aren't laid out (document pages while parents are being edited, or
    // the reverse) have no place on the canvas.
    let slot = layout.slot(loc.spread)?;
    Some((slot.xf * doc.parent_xf(&loc), ""))
}

pub(super) fn path_screen(it: &Item, xf: &Xf, a: Affine) -> Vec<Vec<Pos2>> {
    let bp = it.path.to_bezpath();
    let full = a * it.xf;
    let mut out = Vec::new();
    let mut cur: Vec<Pos2> = Vec::new();
    designcraft_geom::kurbo::flatten(&bp, 0.25 / xf.zoom.max(0.01), |el| {
        use designcraft_geom::PathEl::*;
        match el {
            MoveTo(p) => {
                if cur.len() > 1 {
                    out.push(std::mem::take(&mut cur));
                }
                cur.clear();
                cur.push(xf.to_screen(full * p));
            }
            LineTo(p) => cur.push(xf.to_screen(full * p)),
            ClosePath => {
                if let Some(f) = cur.first().copied() {
                    cur.push(f);
                }
            }
            _ => {}
        }
    });
    if cur.len() > 1 {
        out.push(cur);
    }
    out
}

pub(super) fn draw_frames(app: &DesignApp, painter: &egui::Painter, xf: &Xf, doc: &Document, layout: &CanvasLayout) {
    if !app.ui.frame_edges {
        return;
    }
    let ported = ported_frames(app, doc);
    for slot in &layout.slots {
        let Some(sp) = doc.spread(slot.spread) else { continue };
        let a = slot.xf;
        // Parent items on document pages, and base parents' items on a parent spread: dotted edges.
        for (pi, page) in sp.pages.iter().enumerate() {
            for (ppi, ppg) in doc.shown_parent_pages(slot.spread, pi) {
                let Some(parent) = doc.parents.get(ppi) else { continue };
                let Some(ppage) = parent.pages.get(ppg) else { continue };
                let dx = page.x - ppage.x;
                for it in &parent.items {
                    if (parent.pages.len() > 1 && parent.page_at_x(it.bounds().center().x) != Some(ppg)) || page.overridden.contains(&it.id) {
                        continue;
                    }
                    let col = doc.layer(it.layer).map(|l| c32(l.color)).unwrap_or(Color32::LIGHT_BLUE);
                    for poly in path_screen(it, xf, a * Affine::translate((dx, 0.0))) {
                        for w in poly.windows(2) {
                            dashed(painter, w[0], w[1], Stroke::new(hair(painter), col.gamma_multiply(0.8)), 2.0, 2.0);
                        }
                    }
                }
            }
        }
        for it in &sp.items {
            draw_item_edges(painter, xf, doc, it, a);
        }
        // Every overset frame, selected or not and inside groups too, shows its red mark at the
        // out port (InDesign shows it only on the selected frame; a report in Preflight is easy
        // to miss while laying out).
        for (it, c) in overset_marks(app, doc, layout, &sp.items, xf.zoom, &ported) {
            let c = xf.to_screen(c);
            let s = designcraft_tools::thread::PORT_SIZE_PX as f32;
            painter.rect_filled(Rect::from_center_size(c, vec2(s, s)), 0.0, Color32::WHITE);
            painter.rect_stroke(Rect::from_center_size(c, vec2(s, s)), 0.0, Stroke::new(hair(painter), layer_color(doc, it)), StrokeKind::Inside);
            let red = Color32::from_rgb(230, 20, 20);
            painter.line_segment([c - vec2(2.5, 0.0), c + vec2(2.5, 0.0)], Stroke::new(1.5, red));
            painter.line_segment([c - vec2(0.0, 2.5), c + vec2(0.0, 2.5)], Stroke::new(1.5, red));
        }
        // Tagged frames in their tag's colour.
        if app.ui.tagged_frames {
            for top in &sp.items {
                top.walk(&mut |it: &Item| {
                    if it.xml_tag.is_empty() {
                        return;
                    }
                    let Some(t) = doc.xml.tags.iter().find(|t| t.name == it.xml_tag) else { return };
                    let col = Color32::from_rgb(t.color[0], t.color[1], t.color[2]);
                    for poly in path_screen(it, xf, a) {
                        for w in poly.windows(2) {
                            painter.line_segment([w[0], w[1]], Stroke::new(2.5, col));
                        }
                    }
                });
            }
        }
    }
}

fn draw_item_edges(painter: &egui::Painter, xf: &Xf, doc: &Document, it: &Item, a: Affine) {
    if it.hidden || doc.layer(it.layer).is_some_and(|l| !l.visible) {
        return;
    }
    let col = doc.layer(it.layer).map(|l| c32(l.color)).unwrap_or(Color32::LIGHT_BLUE);
    if let Content::Group { items } = &it.content {
        for c in items {
            draw_item_edges(painter, xf, doc, c, a * it.xf);
        }
        return;
    }
    // Frames without a stroke show their edge; shapes with strokes are visible already.
    let show = it.stroke.is_none() || matches!(it.content, Content::Text(_) | Content::Graphic(_));
    if show {
        for poly in path_screen(it, xf, a) {
            painter.add(egui::Shape::line(poly, Stroke::new(hair(painter), col.gamma_multiply(0.75))));
        }
    }
    // Empty graphic frame: the X.
    if it.object_style == designcraft_doc::BASIC_GRAPHICS_FRAME && matches!(it.content, Content::Unassigned) {
        let r = it.inner_bounds();
        let m = a * it.xf;
        let p = |x: f64, y: f64| xf.to_screen(m * Point::new(x, y));
        painter.line_segment([p(r.x0, r.y0), p(r.x1, r.y1)], Stroke::new(hair(painter), col.gamma_multiply(0.75)));
        painter.line_segment([p(r.x1, r.y0), p(r.x0, r.y1)], Stroke::new(hair(painter), col.gamma_multiply(0.75)));
    }
}

/// Layer colour of the first selected item: live-corner marks are outlined in it.
pub(super) fn selection_color(app: &DesignApp) -> Color32 {
    let fallback = Color32::from_rgb(79, 153, 255);
    let Some(st) = app.session.active() else { return fallback };
    st.selection.items.first().and_then(|id| st.doc.item(*id)).map_or(fallback, |it| layer_color(&st.doc, it))
}

pub(super) fn layer_color(doc: &Document, it: &Item) -> Color32 {
    doc.layer(it.layer).map(|l| c32(l.color)).unwrap_or(Color32::from_rgb(79, 153, 255))
}

/// Draw selection chrome; returns the selection's screen bounds (for the Contextual Task Bar).
pub(super) fn draw_selection(app: &DesignApp, painter: &egui::Painter, xf: &Xf, doc: &Document, layout: &CanvasLayout) -> Option<Rect> {
    let st = app.session.active()?;
    let sel: &Selection = &st.selection;
    let mut text_rect = None;
    // Text selection / caret.
    if let Some(ts) = sel.text {
        draw_text_selection(app, painter, xf, doc, layout, ts, sel.cells);
        let fid = ts.frame.or_else(|| {
            let cs = app.session.cache.get(doc, ts.story, None);
            compose::caret(&cs, ts.focus).and_then(|c| cs.frames.get(c.0)).map(|f| f.frame)
        });
        if let Some(fid) = fid
            && let (Some(it), Some((a, _))) = (doc.item(fid), item_canvas_xf(doc, layout, fid))
        {
            text_rect = Some(xf.rect(a.transform_rect_bbox(it.bounds())));
        }
    }
    let mut union: Option<Rect> = None;
    let mut color = Color32::from_rgb(79, 153, 255);
    let direct = matches!(app.session.tool_id(), "directSelection" | "pen" | "addAnchor" | "deleteAnchor" | "convertDirection");
    for id in &sel.items {
        let (Some(it), Some((a, _))) = (doc.item(*id), item_canvas_xf(doc, layout, *id)) else { continue };
        color = layer_color(doc, it);
        for poly in path_screen(it, xf, a) {
            painter.add(egui::Shape::line(poly, Stroke::new(1.0, color)));
        }
        // InDesign draws a 1 pt bounding box around the selection; with the Direct Selection and
        // pen tools, only the path and its points.
        if sel.items.len() == 1 && sel.text.is_none() && !direct {
            painter.rect_stroke(xf.rect(a.transform_rect_bbox(it.bounds())), 0.0, Stroke::new(1.0, color), StrokeKind::Middle);
        } else if sel.key == Some(*id) {
            // The key object for Align: a thicker bounding box, as in InDesign.
            painter.rect_stroke(xf.rect(a.transform_rect_bbox(it.bounds())), 0.0, Stroke::new(3.0, color), StrokeKind::Middle);
        }
        let b = xf.rect(a.transform_rect_bbox(it.bounds()));
        union = Some(union.map_or(b, |u| u.union(b)));
        // Text frame ports, also of the frames inside a selected group (so an overset frame in a
        // group shows its red mark).
        let mut frames = Vec::new();
        text_frames_in(it, 0, &mut frames);
        for (frame, story) in frames {
            draw_ports(app, painter, xf, doc, layout, frame, story, color);
        }
        if let Content::Graphic(g) = &it.content
            && sel.content
        {
            let gb = xf.rect((a * it.xf).transform_rect_bbox(g.xf.transform_rect_bbox(DRect::new(0.0, 0.0, g.size.0, g.size.1))));
            let brown = Color32::from_rgb(196, 111, 43);
            painter.rect_stroke(gb, 0.0, Stroke::new(1.0, brown), StrokeKind::Middle);
            // Its own handles resize the graphic (InDesign's brown content bounding box).
            if sel.items.len() == 1 {
                for h in designcraft_tools::select::handles(DRect::new(gb.min.x as f64, gb.min.y as f64, gb.max.x as f64, gb.max.y as f64)) {
                    let r = Rect::from_center_size(pos2(h.x as f32, h.y as f32), vec2(6.5, 6.5));
                    painter.rect_filled(r, 0.0, Color32::WHITE);
                    painter.rect_stroke(r, 0.0, Stroke::new(hair(painter), brown), StrokeKind::Inside);
                }
            }
        }
    }
    // Direct Selection shows only the handles around the selected anchors, as InDesign; the pen
    // and the anchor tools show every handle they work on.
    let some_handles = app.session.tool_id() == "directSelection";
    if direct {
        // Looked up once per anchor drawn: a large marquee selection stays linear.
        let chosen_set: std::collections::HashSet<(designcraft_doc::ItemId, usize, usize)> = sel.anchors.iter().copied().collect();
        for id in &sel.items {
            let (Some(it), Some((a, _))) = (doc.item(*id), item_canvas_xf(doc, layout, *id)) else { continue };
            let m = a * it.xf;
            let col = layer_color(doc, it);
            for (si, sp) in it.path.subpaths.iter().enumerate() {
                for (ai, an) in sp.anchors.iter().enumerate() {
                    let p = xf.to_screen(m * an.p);
                    // A selected anchor point is solid, the others hollow (InDesign).
                    let chosen = chosen_set.contains(&(*id, si, ai));
                    let shown = |out: bool| {
                        !some_handles
                            || designcraft_tools::select::handle_shown(sp.anchors.len(), sp.closed, ai, out, |a| chosen_set.contains(&(*id, si, a)))
                    };
                    for (h, has) in [(an.h_in, an.has_in() && shown(false)), (an.h_out, an.has_out() && shown(true))] {
                        if has {
                            let hp = xf.to_screen(m * h);
                            painter.line_segment([p, hp], Stroke::new(1.0, col));
                            painter.circle_filled(hp, 2.5, col);
                        }
                    }
                    let r = Rect::from_center_size(p, vec2(5.0, 5.0));
                    painter.rect_filled(r, 0.0, if chosen { col } else { Color32::WHITE });
                    painter.rect_stroke(r, 0.0, Stroke::new(1.0, col), StrokeKind::Inside);
                }
            }
        }
    }
    if let Some(u) = union
        && sel.text.is_none()
        && !direct
    {
        if sel.items.len() > 1 {
            painter.rect_stroke(u, 0.0, Stroke::new(1.0, color), StrokeKind::Middle);
        }
        let hs = 6.5;
        // Centre point. The Selection tool draws the live-corner widget as an overlay.
        painter.rect_filled(Rect::from_center_size(u.center(), vec2(3.5, 3.5)), 0.0, color);
        // A selected placed graphic has its own handles instead of the frame's.
        let graphic_selected =
            sel.content && sel.items.len() == 1 && doc.item(sel.items[0]).is_some_and(|i| matches!(i.content, Content::Graphic(_)));
        for h in designcraft_tools::select::handles(DRect::new(u.min.x as f64, u.min.y as f64, u.max.x as f64, u.max.y as f64))
            .into_iter()
            .filter(|_| !graphic_selected)
        {
            let r = Rect::from_center_size(pos2(h.x as f32, h.y as f32), vec2(hs, hs));
            painter.rect_filled(r, 0.0, Color32::WHITE);
            painter.rect_stroke(r, 0.0, Stroke::new(hair(painter), color), StrokeKind::Inside);
        }
    }
    // Threads.
    if app.ui.text_threads || sel.items.iter().any(|i| doc.item(*i).is_some_and(|x| x.is_text_frame())) {
        for id in &sel.items {
            let Some(sid) = doc.item(*id).and_then(|i| i.text_frame()).map(|t| t.story) else { continue };
            let Some(story) = doc.story(sid) else { continue };
            for w in story.frames.windows(2) {
                let (Some(a), Some(b)) = (port_pos(doc, layout, w[0], true), port_pos(doc, layout, w[1], false)) else { continue };
                let col = doc.item(w[0]).map(|i| layer_color(doc, i)).unwrap_or(color);
                painter.line_segment([xf.to_screen(a), xf.to_screen(b)], Stroke::new(1.0, col));
            }
        }
    }
    text_rect.or(union)
}

/// How deep a selected group is walked for the text frames whose ports are drawn.
const PORT_GROUP_DEPTH: usize = 32;

/// The text frames whose ports `draw_selection` draws: the selected text frames and the text
/// frames inside selected groups.
pub(super) fn ported_frames(app: &DesignApp, doc: &Document) -> std::collections::HashSet<designcraft_doc::ItemId> {
    let mut frames = Vec::new();
    if let Some(st) = app.session.active() {
        for it in st.selection.items.iter().filter_map(|id| doc.item(*id)) {
            text_frames_in(it, 0, &mut frames);
        }
    }
    frames.into_iter().map(|(f, _)| f.id).collect()
}

/// Canvas centres of the always-on overset marks among `items`: the out port of every visible
/// frame that ends an overset story, except the frames in `ported`, whose out port `draw_ports`
/// already marks.
pub(super) fn overset_marks<'a>(
    app: &DesignApp,
    doc: &Document,
    layout: &CanvasLayout,
    items: &'a [std::sync::Arc<Item>],
    zoom: f64,
    ported: &std::collections::HashSet<designcraft_doc::ItemId>,
) -> Vec<(&'a Item, Point)> {
    let mut frames = Vec::new();
    overset_frames(app, doc, items, 0, &mut frames);
    frames
        .into_iter()
        .filter(|it| !ported.contains(&it.id))
        .filter_map(|it| Some((it, designcraft_tools::thread::port_center(doc, layout, it.id, true, zoom)?)))
        .collect()
}

/// The visible text frames among `items` (groups walked) that end an overset story.
pub(super) fn overset_frames<'a>(app: &DesignApp, doc: &Document, items: &'a [std::sync::Arc<Item>], depth: usize, out: &mut Vec<&'a Item>) {
    if depth > PORT_GROUP_DEPTH {
        return;
    }
    for it in items {
        if it.hidden || doc.layer(it.layer).is_some_and(|l| !l.visible) {
            continue;
        }
        match &it.content {
            Content::Text(tf) => {
                let Some(story) = doc.story(tf.story) else { continue };
                if story.frames.last() == Some(&it.id) && app.session.cache.get(doc, tf.story, None).is_overset() {
                    out.push(it);
                }
            }
            Content::Group { items } => overset_frames(app, doc, items, depth + 1, out),
            _ => {}
        }
    }
}

/// `it` when it is a text frame, or every text frame nested in it when it is a group, each with
/// its story.
pub(super) fn text_frames_in<'a>(it: &'a Item, depth: usize, out: &mut Vec<(&'a Item, designcraft_doc::StoryId)>) {
    match &it.content {
        Content::Text(tf) => out.push((it, tf.story)),
        Content::Group { items } if depth < PORT_GROUP_DEPTH => {
            for child in items {
                text_frames_in(child, depth + 1, out);
            }
        }
        _ => {}
    }
}

/// Port location (canvas): out port at bottom-right, in port at top-left.
fn port_pos(doc: &Document, layout: &CanvasLayout, id: designcraft_doc::ItemId, out: bool) -> Option<Point> {
    let it = doc.item(id)?;
    let (a, _) = item_canvas_xf(doc, layout, id)?;
    let r = it.inner_bounds();
    let p = if out { Point::new(r.x1 - 10.0, r.y1) } else { Point::new(r.x0 + 10.0, r.y0) };
    Some(a * it.xf * p)
}

#[allow(clippy::too_many_arguments)]
fn draw_ports(
    app: &DesignApp,
    painter: &egui::Painter,
    xf: &Xf,
    doc: &Document,
    layout: &CanvasLayout,
    it: &Item,
    sid: designcraft_doc::StoryId,
    color: Color32,
) {
    let Some(story) = doc.story(sid) else { return };
    let pos = story.frames.iter().position(|f| *f == it.id).unwrap_or(0);
    let is_last = pos + 1 == story.frames.len();
    let cs = app.session.cache.get(doc, sid, None);
    let overset = is_last && cs.is_overset();
    let s = designcraft_tools::thread::PORT_SIZE_PX as f32;
    // In port on the left edge below the top-left corner; out port on the right edge above the
    // bottom-right corner. The Selection tool hit-tests the same places.
    let port = |out: bool| designcraft_tools::thread::port_center(doc, layout, it.id, out, xf.zoom).map(|c| xf.to_screen(c));
    let (Some(inp), Some(outp)) = (port(false), port(true)) else { return };
    for (c, has_link, is_out) in [(inp, pos > 0, false), (outp, !is_last, true)] {
        let pr = Rect::from_center_size(c, vec2(s, s));
        painter.rect_filled(pr, 0.0, Color32::WHITE);
        painter.rect_stroke(pr, 0.0, Stroke::new(hair(painter), color), StrokeKind::Inside);
        if is_out && overset {
            let red = Color32::from_rgb(230, 20, 20);
            painter.line_segment([c - vec2(2.5, 0.0), c + vec2(2.5, 0.0)], Stroke::new(1.5, red));
            painter.line_segment([c - vec2(0.0, 2.5), c + vec2(0.0, 2.5)], Stroke::new(1.5, red));
        } else if has_link {
            painter.add(egui::Shape::convex_polygon(vec![c + vec2(-2.0, -2.5), c + vec2(2.5, 0.0), c + vec2(-2.0, 2.5)], color, Stroke::NONE));
        }
    }
}
