use super::frames::{item_canvas_xf, selection_color};
use super::page_decor::dashed;
use super::render::c32;
use super::{CanvasLayout, Color32, Content, DesignApp, Document, Overlay, Point, Pos2, Rect, Stroke, StrokeKind, Tokens, Xf, pos2, vec2};

pub(super) fn draw_tool_overlays(app: &mut DesignApp, painter: &egui::Painter, xf: &Xf) {
    if let (Some((z, c)), Some(rect)) = (app.power_zoom, app.canvas_rect) {
        // The view that releasing returns to.
        let (w, h) = (rect.width() as f64 / z, rect.height() as f64 / z);
        let r = xf.rect(designcraft_geom::Rect::new(c.x - w / 2.0, c.y - h / 2.0, c.x + w / 2.0, c.y + h / 2.0));
        painter.rect_stroke(r, 0.0, egui::Stroke::new(1.5, Color32::from_rgb(230, 40, 40)), egui::StrokeKind::Middle);
    }
    let tok = Tokens::get(painter.ctx());
    let ov = app.session.overlays(app.view_info());
    let doc = app.session.active().map(|st| st.doc.clone());
    let layout = app.session.layout();
    let sel_color = selection_color(app);
    for o in ov {
        match o {
            Overlay::Marquee(r) => {
                let r = xf.rect(r);
                painter.rect_filled(r, 0.0, Color32::from_rgba_unmultiplied(80, 140, 255, 30));
                for (a, b) in [
                    (r.left_top(), r.right_top()),
                    (r.right_top(), r.right_bottom()),
                    (r.right_bottom(), r.left_bottom()),
                    (r.left_bottom(), r.left_top()),
                ] {
                    dashed(painter, a, b, Stroke::new(1.0, Color32::from_gray(200)), 3.0, 3.0);
                }
            }
            Overlay::Measure { p, text } => {
                let s = xf.to_screen(p) + vec2(14.0, 14.0);
                let g = painter.layout_no_wrap(text, egui::FontId::proportional(11.0), tok.readout_text);
                let r = Rect::from_min_size(s, g.size() + vec2(10.0, 6.0));
                painter.rect_filled(r, 3.0, tok.readout_bg);
                painter.galley(r.min + vec2(5.0, 3.0), g, tok.readout_text);
            }
            Overlay::Line { a, b, color, dashed: d } => {
                let st = Stroke::new(1.0, c32(color));
                if d {
                    dashed(painter, xf.to_screen(a), xf.to_screen(b), st, 3.0, 3.0);
                } else {
                    painter.line_segment([xf.to_screen(a), xf.to_screen(b)], st);
                }
            }
            Overlay::Guide { a, b } => {
                painter.line_segment([xf.to_screen(a), xf.to_screen(b)], Stroke::new(1.0, tok.smart_guide));
            }
            Overlay::Gap { a, b, label } => {
                let (a, b) = (xf.to_screen(a), xf.to_screen(b));
                painter.line_segment([a, b], Stroke::new(1.0, tok.smart_guide));
                let s = a + (b - a) * 0.5;
                let g = painter.layout_no_wrap(label, egui::FontId::proportional(11.0), tok.measure_text);
                let r = Rect::from_min_size(s, g.size() + vec2(10.0, 6.0));
                painter.rect_filled(r, 3.0, tok.measure_bg);
                painter.galley(r.min + vec2(5.0, 3.0), g, tok.measure_text);
            }
            Overlay::Path { path, color, dashed: d } => draw_path_overlay(painter, xf, &path, c32(color), d),
            Overlay::ContentGhost(id) => {
                if let Some(doc) = &doc {
                    draw_content_ghost(painter, xf, doc, &layout, id);
                }
            }
            Overlay::LiveCornerWidget(p) => {
                let r = Rect::from_center_size(xf.to_screen(p), vec2(6.0, 6.0));
                painter.rect_filled(r, 0.0, tok.live_corner);
                painter.rect_stroke(r, 0.0, Stroke::new(1.0, sel_color), StrokeKind::Inside);
            }
            Overlay::LiveCornerDiamond(p) => {
                let c = xf.to_screen(p);
                let h = 4.5;
                let pts = vec![c + vec2(0.0, -h), c + vec2(h, 0.0), c + vec2(0.0, h), c + vec2(-h, 0.0)];
                painter.add(egui::Shape::convex_polygon(pts, tok.live_corner, Stroke::new(1.0, sel_color)));
            }
            Overlay::PageHandles(r) => {
                // The Page tool's selected page: outline and eight handles, as InDesign draws them.
                let r = xf.rect(r);
                let col = Color32::from_rgb(79, 153, 255);
                painter.rect_stroke(r, 0.0, Stroke::new(1.0, col), StrokeKind::Middle);
                for i in (0..9).filter(|i| *i != 4) {
                    let c = pos2(r.min.x + r.width() * (i % 3) as f32 / 2.0, r.min.y + r.height() * (i / 3) as f32 / 2.0);
                    let h = Rect::from_center_size(c, vec2(7.0, 7.0));
                    painter.rect_filled(h, 0.0, Color32::WHITE);
                    painter.rect_stroke(h, 0.0, Stroke::new(1.0, col), StrokeKind::Inside);
                }
            }
            Overlay::Tag { p, text } => {
                let text = crate::i18n::tr(&app.ui.language, &text).to_owned();
                let s = xf.to_screen(p);
                let g = painter.layout_no_wrap(text, egui::FontId::proportional(10.0), tok.measure_text);
                let r = Rect::from_min_size(s - vec2(0.0, g.size().y + 4.0), g.size() + vec2(8.0, 4.0));
                painter.rect_filled(r, 2.0, tok.measure_bg);
                painter.rect_stroke(r, 2.0, Stroke::new(1.0, Color32::from_rgb(79, 153, 255)), StrokeKind::Inside);
                painter.galley(r.min + vec2(4.0, 2.0), g, tok.measure_text);
            }
            Overlay::Tip { p, text } => {
                let text = crate::i18n::tr(&app.ui.language, &text).to_owned();
                let s = xf.to_screen(p) + vec2(14.0, 14.0);
                let g = painter.layout(text, egui::FontId::proportional(11.0), tok.measure_text, 260.0);
                let r = Rect::from_min_size(s, g.size() + vec2(10.0, 6.0));
                painter.rect_filled(r, 3.0, tok.measure_bg);
                painter.galley(r.min + vec2(5.0, 3.0), g, tok.measure_text);
            }
            Overlay::LiquidPins { object, page, pins, resize } => draw_liquid_pins(painter, xf.rect(object), xf.rect(page), pins, resize),
        }
    }
}

/// A path overlay (canvas coordinates; the Pencil's stroke while drawing, the Smooth and Erase
/// drags) as screen polylines, its curves flattened to about a quarter of a screen pixel.
fn draw_path_overlay(painter: &egui::Painter, xf: &Xf, path: &designcraft_geom::BezPath, color: Color32, dashed_line: bool) {
    use designcraft_geom::PathEl;
    let stroke = Stroke::new(1.0, color);
    let mut runs: Vec<Vec<Pos2>> = Vec::new();
    designcraft_geom::kurbo::flatten(path, 0.25 / xf.zoom.max(1e-9), |el| match el {
        PathEl::MoveTo(p) => runs.push(vec![xf.to_screen(p)]),
        PathEl::LineTo(p) => {
            if let Some(run) = runs.last_mut() {
                run.push(xf.to_screen(p));
            }
        }
        PathEl::ClosePath => {
            if let Some(run) = runs.last_mut()
                && let Some(&first) = run.first()
            {
                run.push(first);
            }
        }
        // Flattening yields only moves, lines and closes.
        PathEl::QuadTo(..) | PathEl::CurveTo(..) => {}
    });
    for run in runs.into_iter().filter(|r| r.len() >= 2) {
        if dashed_line {
            painter.extend(egui::Shape::dashed_line(&run, stroke, 3.0, 3.0));
        } else {
            painter.add(egui::Shape::line(run, stroke));
        }
    }
}

/// A frame's whole content drawn faintly over the canvas, the part outside the frame included,
/// with its bounds outlined (while the content moves inside the frame).
fn draw_content_ghost(painter: &egui::Painter, xf: &Xf, doc: &Document, layout: &CanvasLayout, id: designcraft_doc::ItemId) {
    let Some(it) = doc.item(id) else { return };
    let Some((a, _)) = item_canvas_xf(doc, layout, id) else { return };
    let outline = Stroke::new(1.0, Color32::from_rgb(196, 111, 43));
    let Content::Graphic(g) = &it.content else {
        for c in it.children() {
            let r = xf.rect((a * it.xf).transform_rect_bbox(c.bounds()));
            painter.rect_stroke(r, 0.0, outline, StrokeKind::Middle);
        }
        return;
    };
    let m = a * it.xf * g.xf;
    let (w, h) = g.size;
    let corners = [Point::new(0.0, 0.0), Point::new(w, 0.0), Point::new(w, h), Point::new(0.0, h)].map(|p| xf.to_screen(m * p));
    if corners.iter().any(|p| !p.x.is_finite() || !p.y.is_finite()) {
        return;
    }
    if let Some(tex) = ghost_texture(painter.ctx(), doc, g) {
        let mut mesh = egui::Mesh::with_texture(tex.id());
        let tint = Color32::from_white_alpha(90);
        for (p, uv) in corners.iter().zip([pos2(0.0, 0.0), pos2(1.0, 0.0), pos2(1.0, 1.0), pos2(0.0, 1.0)]) {
            mesh.vertices.push(egui::epaint::Vertex { pos: *p, uv, color: tint });
        }
        mesh.indices.extend([0, 1, 2, 0, 2, 3]);
        painter.add(egui::Shape::mesh(mesh));
    }
    painter.add(egui::Shape::closed_line(corners.to_vec(), outline));
}

/// A screen-resolution proxy of a placed graphic for the ghost, cached per asset.
fn ghost_texture(ctx: &egui::Context, doc: &Document, g: &designcraft_doc::Graphic) -> Option<egui::TextureHandle> {
    let asset = doc.assets.get(&g.asset)?;
    let key = egui::Id::unique(("content_ghost", std::sync::Arc::as_ptr(&asset.data) as usize, asset.data.len(), asset.page));
    if let Some(t) = ctx.data(|d| d.get_temp::<egui::TextureHandle>(key)) {
        return Some(t);
    }
    // A 72 ppi proxy is enough for a faint preview.
    let pm = designcraft_render::images::mip(&asset.data, asset.page, g.size.0, 1.0)?;
    let (pw, ph) = (pm.width() as usize, pm.height() as usize);
    if pw == 0 || ph == 0 {
        return None;
    }
    let ci = egui::ColorImage::from_rgba_premultiplied([pw, ph], pm.data_as_u8_slice());
    let max = ctx.input(|i| i.max_texture_side).min(4096);
    let t = ctx.load_texture("content_ghost", crate::widgets::fit_texture(ci, max), egui::TextureOptions::LINEAR);
    ctx.data_mut(|d| d.insert_temp(key, t.clone()));
    Some(t)
}

/// An object's liquid controls on an object-based page, as InDesign draws them in orange: a
/// dashed line across the object for width and height with a padlock (closed: locked), and a pin
/// outside each edge (pinned: a line to the page edge).
fn draw_liquid_pins(painter: &egui::Painter, o: Rect, page: Rect, pins: [bool; 4], resize: [bool; 2]) {
    let col = Color32::from_rgb(178, 88, 18);
    let st = Stroke::new(1.5, col);
    let c = o.center();
    let mids = [pos2(c.x, o.min.y), pos2(o.max.x, c.y), pos2(c.x, o.max.y), pos2(o.min.x, c.y)];
    // The width and height lines between the edges' middles.
    for (a, b) in [(mids[3], mids[1]), (mids[0], mids[2])] {
        dashed(painter, a, b, Stroke::new(1.0, col), 3.0, 3.0);
    }
    for m in mids {
        painter.circle_filled(m, 3.5, col);
    }
    let (po, lo) = (designcraft_tools::PIN_OFFSET_PX as f32, designcraft_tools::LOCK_OFFSET_PX as f32);
    let outward = [vec2(0.0, -1.0), vec2(1.0, 0.0), vec2(0.0, 1.0), vec2(-1.0, 0.0)];
    let edge = [pos2(c.x, page.min.y), pos2(page.max.x, c.y), pos2(c.x, page.max.y), pos2(page.min.x, c.y)];
    for i in 0..4 {
        if pins[i] {
            painter.line_segment([mids[i], edge[i]], st);
            painter.circle_filled(edge[i], 4.0, col);
        } else {
            painter.circle_stroke(mids[i] + outward[i] * po, 4.0, st);
        }
    }
    // Padlocks: the width lock right of the centre, the height lock above it.
    for (at, locked) in [(c + vec2(lo, 0.0), !resize[0]), (c + vec2(0.0, -lo), !resize[1])] {
        let body = Rect::from_center_size(at + vec2(0.0, 2.0), vec2(8.0, 6.0));
        painter.rect_filled(body.expand(1.0), 1.0, Color32::WHITE);
        painter.rect_filled(body, 1.0, col);
        // The shackle: over the body when locked, swung open when not.
        let base = if locked { body.center().x } else { body.center().x + 3.0 };
        let pts: Vec<_> = (0..=8)
            .map(|k| {
                let a = std::f32::consts::PI * k as f32 / 8.0;
                pos2(base - 2.6 * a.cos(), body.min.y - 3.0 * a.sin())
            })
            .collect();
        painter.add(egui::Shape::line(pts, Stroke::new(1.3, col)));
    }
}

#[cfg(test)]
mod pencil_preview_tests {
    use egui::{Pos2, pos2, vec2};

    /// Lines painted in `color` (egui paths) and the points they pass through.
    fn lines_in(h: &egui_kittest::Harness<'static, crate::test_window::Window>, color: egui::Color32) -> Vec<Vec<Pos2>> {
        fn walk(s: &egui::Shape, color: egui::Color32, out: &mut Vec<Vec<Pos2>>) {
            match s {
                egui::Shape::Path(p) if p.stroke.color == egui::epaint::ColorMode::Solid(color) && p.points.len() >= 2 => out.push(p.points.clone()),
                egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, color, out)),
                _ => {}
            }
        }
        let mut out = vec![];
        h.output().shapes.iter().for_each(|s| walk(&s.shape, color, &mut out));
        out
    }

    #[test]
    fn the_pencil_stroke_shows_while_drawing() {
        let mut app = crate::DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", serde_json::json!({})).unwrap();
        app.run("tool.select", serde_json::json!({"tool": "pencil"})).unwrap();
        let mut h = crate::test_window::open(app, vec2(1400.0, 900.0));
        let doc = &h.state().app.session.doc().unwrap().doc;
        let layer = doc.layers[0].color;
        let color = egui::Color32::from_rgb(layer[0], layer[1], layer[2]);
        let items = |h: &egui_kittest::Harness<'static, crate::test_window::Window>| h.state().app.session.doc().unwrap().doc.spreads[0].items.len();
        let before = items(&h);
        // Press on the page and drag, without letting go.
        let start = pos2(520.0, 380.0);
        h.hover_at(start);
        h.drag_at(start);
        h.run_steps(2);
        let path: Vec<Pos2> = (1..=12).map(|i| start + vec2(i as f32 * 12.0, (i as f32 * 0.6).sin() * 30.0)).collect();
        for p in &path {
            h.hover_at(*p);
            h.run_steps(1);
        }
        let drawn = lines_in(&h, color);
        let stroke = drawn.iter().max_by_key(|l| l.len()).expect("the stroke is painted while the button is down");
        let near = |a: Pos2, b: Pos2| (a - b).length() < 2.0;
        assert!(near(stroke[0], start), "it starts where the press was: {:?}", stroke[0]);
        assert!(path.iter().all(|p| stroke.iter().any(|q| near(*p, *q))), "it follows the pointer: {stroke:?}");
        assert_eq!(items(&h), before, "nothing is made before the release");
        // Letting go makes the path and ends the preview.
        h.drop_at(*path.last().unwrap());
        h.run_steps(3);
        assert_eq!(items(&h), before + 1);
        let app = &mut h.state_mut().app;
        let view = app.view_info();
        assert!(app.session.overlays(view).is_empty(), "the preview is gone");
    }
}
