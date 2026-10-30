use super::frames::{item_canvas_xf, layer_color, path_screen};
use super::{Affine, CanvasLayout, Color32, DesignApp, Document, Point, Pos2, Rect, Stroke, Xf, compose, pos2};

pub(super) fn draw_text_selection(
    app: &DesignApp,
    painter: &egui::Painter,
    xf: &Xf,
    doc: &Document,
    layout: &CanvasLayout,
    ts: designcraft_doc::TextSel,
    cells: Option<designcraft_doc::TableSel>,
) {
    let cs = app.session.cache.get(doc, ts.story, None);
    let range = ts.range();
    let tint = Color32::from_rgba_unmultiplied(80, 140, 255, 110);
    // Selected table cells: tint every selected cell.
    if let Some(sel) = cells.filter(|c| c.story == ts.story) {
        let selected = doc.story(sel.story).and_then(|s| s.tables.get(&sel.table)).map(|t| t.selected_owners(sel.range)).unwrap_or_default();
        for ft in &cs.frames {
            let (Some((a, _)), Some(it)) = (item_canvas_xf(doc, layout, ft.frame), doc.item(ft.frame)) else { continue };
            let m = a * doc.text_xf(it);
            for t in ft.tables.iter().filter(|t| t.table == sel.table) {
                for c in t.cells.iter().filter(|c| selected.binary_search(&(c.row, c.col)).is_ok()) {
                    let q = [
                        xf.to_screen(m * Point::new(c.rect.x0, c.rect.y0)),
                        xf.to_screen(m * Point::new(c.rect.x1, c.rect.y0)),
                        xf.to_screen(m * Point::new(c.rect.x1, c.rect.y1)),
                        xf.to_screen(m * Point::new(c.rect.x0, c.rect.y1)),
                    ];
                    painter.add(egui::Shape::convex_polygon(q.to_vec(), Color32::from_rgba_unmultiplied(80, 140, 255, 90), Stroke::NONE));
                }
            }
        }
        return;
    }
    if let Some(cell) = ts.cell {
        draw_cell_selection(app, painter, xf, doc, layout, &cs, ts, cell);
        return;
    }
    let mut runs: Vec<(usize, Affine, Vec<[Point; 4]>, Vec<(usize, usize)>)> = Vec::new();
    for (fi, ft) in cs.frames.iter().enumerate() {
        let Some((a, _)) = item_canvas_xf(doc, layout, ft.frame) else { continue };
        let Some(it) = doc.item(ft.frame) else { continue };
        let m = a * doc.text_xf(it);
        // Show the frame edge while typing.
        for poly in path_screen(it, xf, a) {
            painter.add(egui::Shape::line(poly, Stroke::new(1.0, layer_color(doc, it).gamma_multiply(0.6))));
        }
        if range.is_empty() {
            continue;
        }
        // Type on a path: the highlight bends with the text.
        if let Some(map) = compose::PathTextMap::new(it, ft) {
            for l in &ft.lines {
                let (s, e) = (range.start.max(l.range.start), range.end.min(l.range.end));
                if s >= e {
                    continue;
                }
                for q in compose::highlight_quads(l, s, e, false) {
                    let (x0, x1) = (q[0].x.min(q[1].x), q[0].x.max(q[1].x));
                    let (y0, y1) = (q[0].y.min(q[2].y), q[0].y.max(q[2].y));
                    // Short slices, each turned with the path.
                    let n = ((x1 - x0) / 2.0).ceil().clamp(1.0, 2000.0) as usize;
                    for k in 0..n {
                        let (a, b) = (x0 + (x1 - x0) * k as f64 / n as f64, x0 + (x1 - x0) * (k + 1) as f64 / n as f64);
                        let poly =
                            [Point::new(a, y0), Point::new(b, y0), Point::new(b, y1), Point::new(a, y1)].map(|p| xf.to_screen(m * map.to_path(p)));
                        painter.add(egui::Shape::convex_polygon(poly.to_vec(), tint, Stroke::NONE));
                    }
                }
            }
            continue;
        }
        let mut quads = Vec::new();
        let mut glyphs = Vec::new();
        for (li, l) in ft.lines.iter().enumerate() {
            let s = range.start.max(l.range.start);
            let e = range.end.min(l.range.end);
            if s > e || (s == e && !(l.range.end < range.end && e == l.range.end)) {
                continue;
            }
            let past = e == l.range.end && range.end > l.range.end;
            quads.extend(compose::highlight_quads(l, s, e, past));
            // A selected drop cap is highlighted down to its baseline.
            if let Some(dc) = l.drop_cap {
                let sel = l.glyphs.iter().filter(|g| g.len > 0 && g.byte < dc.end && g.byte >= s && g.byte < e);
                let (a, b) = sel.fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), g| (a.min(g.x), b.max(g.x + g.adv)));
                if a < b {
                    let r = dc.rect;
                    quads.push([Point::new(a, r.y0), Point::new(b, r.y0), Point::new(b, r.y1), Point::new(a, r.y1)]);
                }
            }
            for (gi, g) in l.glyphs.iter().enumerate() {
                if g.visible && g.len > 0 && g.byte >= s && g.byte < e {
                    glyphs.push((li, gi));
                }
            }
        }
        if !quads.is_empty() {
            runs.push((fi, m, quads, glyphs));
        }
    }
    if !runs.is_empty() {
        draw_inverse_highlight(app, painter, xf, doc, &cs, ts.story, &runs);
    }
    if ts.is_caret()
        && let Some((fi, x, bl, asc, desc)) = compose::caret(&cs, ts.focus)
        && let Some(ft) = cs.frames.get(fi)
        && let (Some((a, _)), Some(it)) = (item_canvas_xf(doc, layout, ft.frame), doc.item(ft.frame))
    {
        let m = a * doc.text_xf(it);
        // Type on a path: the caret stands across the path where it is along it.
        let on = |p: Point| compose::PathTextMap::new(it, ft).map_or(p, |map| map.to_path(p));
        let p0 = xf.to_screen(m * on(Point::new(x, bl - asc)));
        let p1 = xf.to_screen(m * on(Point::new(x, bl + desc)));
        let blink = (painter.ctx().input(|i| i.time) * 1.6) as i64 % 2 == 0;
        if blink {
            painter.line_segment([p0, p1], Stroke::new(1.0, Color32::BLACK));
        }
        painter.ctx().request_repaint_after(std::time::Duration::from_millis(330));
    }
}

/// Caret / text highlight inside a table cell (the cell's lines are offset by the cell origin).
#[allow(clippy::too_many_arguments)]
fn draw_cell_selection(
    _app: &DesignApp,
    painter: &egui::Painter,
    xf: &Xf,
    doc: &Document,
    layout: &CanvasLayout,
    cs: &compose::ComposedStory,
    ts: designcraft_doc::TextSel,
    cell: designcraft_doc::CellAddr,
) {
    // Footnote text or a cell: its composed story and where it sits in the frame.
    let found = match cell.footnote_id() {
        Some(id) => compose::find_note(cs, id).map(|(fi, n)| (fi, n.origin, n.text.clone())),
        None => compose::find_cell(cs, cell.table, cell.row, cell.col).map(|(fi, _, pc)| (fi, pc.origin, pc.text.clone())),
    };
    let Some((fi, origin, text)) = found else { return };
    let Some(ft) = cs.frames.get(fi) else { return };
    let (Some((a, _)), Some(it)) = (item_canvas_xf(doc, layout, ft.frame), doc.item(ft.frame)) else { return };
    let m = a * doc.text_xf(it) * designcraft_geom::Affine::translate(origin.to_vec2());
    let range = ts.range();
    if !range.is_empty()
        && let Some(cft) = text.frames.first()
    {
        for l in &cft.lines {
            let s = range.start.max(l.range.start);
            let e = range.end.min(l.range.end);
            if s > e || (s == e && !(l.range.end < range.end && e == l.range.end)) {
                continue;
            }
            let past = e == l.range.end && range.end > l.range.end;
            for q in compose::highlight_quads(l, s, e, past) {
                let poly = q.map(|p| xf.to_screen(m * p));
                painter.add(egui::Shape::convex_polygon(poly.to_vec(), Color32::from_rgba_unmultiplied(80, 140, 255, 110), Stroke::NONE));
            }
        }
    }
    if ts.is_caret()
        && let Some((_, x, bl, asc, desc)) = match cell.footnote_id() {
            Some(id) => compose::note_caret(cs, id, ts.focus),
            None => compose::cell_caret(cs, cell.table, cell.row, cell.col, ts.focus),
        }
    {
        let m = a * doc.text_xf(it);
        let (p0, p1) = (xf.to_screen(m * Point::new(x, bl - asc)), xf.to_screen(m * Point::new(x, bl + desc)));
        let blink = (painter.ctx().input(|i| i.time) * 1.6) as i64 % 2 == 0;
        if blink {
            painter.line_segment([p0, p1], Stroke::new(1.0, Color32::BLACK));
        }
        painter.ctx().request_repaint_after(std::time::Duration::from_millis(330));
    }
}

/// InDesign draws selected text inverted: a black highlight with the selected glyphs in the
/// inverse of their colour (white for black text). The highlight and glyph outlines are
/// rasterised with `vello_cpu` into a small screen-space texture over the selection.
fn draw_inverse_highlight(
    app: &DesignApp,
    painter: &egui::Painter,
    xf: &Xf,
    doc: &Document,
    cs: &compose::ComposedStory,
    story: designcraft_doc::StoryId,
    runs: &[(usize, Affine, Vec<[Point; 4]>, Vec<(usize, usize)>)],
) {
    use designcraft_render::vello_cpu::{self, kurbo, peniko};
    let ctx = painter.ctx();
    let ppp = ctx.pixels_per_point();
    let to_screen = |m: Affine, p: Point| xf.to_screen(m * p);
    let mut bbox = Rect::NOTHING;
    for (_, m, quads, _) in runs {
        for q in quads {
            for p in q {
                bbox.extend_with(to_screen(*m, *p));
            }
        }
    }
    let bbox = bbox.intersect(painter.clip_rect());
    if !bbox.is_positive() {
        return;
    }
    // Snap to device pixels so the texture maps 1:1.
    let bbox = Rect::from_min_max(
        pos2((bbox.min.x * ppp).floor() / ppp, (bbox.min.y * ppp).floor() / ppp),
        pos2((bbox.max.x * ppp).ceil() / ppp, (bbox.max.y * ppp).ceil() / ppp),
    );
    let (w, h) = (((bbox.width() * ppp).round() as u32).clamp(1, 8192), ((bbox.height() * ppp).round() as u32).clamp(1, 8192));
    let Some(st) = app.session.active() else { return };
    let key = egui::Id::unique(("inverse_sel", st.uid, st.revision, std::sync::Arc::as_ptr(&st.doc) as usize, story.0))
        .with((runs.iter().map(|r| r.2.len() + r.3.len() * 7919).sum::<usize>(), w, h))
        .with((bbox.min.x.to_bits(), bbox.min.y.to_bits(), xf.zoom.to_bits(), st.selection.text.map(|t| (t.anchor, t.focus))));
    let cache_id = egui::Id::unique("inverse_sel_tex");
    let cached: Option<(egui::Id, egui::TextureHandle)> = ctx.data(|d| d.get_temp(cache_id));
    let tex = match cached {
        Some((k, t)) if k == key => t,
        _ => {
            let view = xf.affine(ppp as f64, bbox.min);
            let mut rc = vello_cpu::RenderContext::new_with(w as u16, h as u16, vello_cpu::RenderSettings { num_threads: 0, ..Default::default() });
            rc.set_paint(peniko::Color::from_rgba8(0, 0, 0, 255));
            for (_, m, quads, _) in runs {
                rc.set_transform(view * *m);
                for q in quads {
                    let mut bp = kurbo::BezPath::new();
                    bp.move_to(q[0]);
                    for p in &q[1..] {
                        bp.line_to(*p);
                    }
                    bp.close_path();
                    rc.fill_path(&bp);
                }
            }
            let db = designcraft_fonts::FontDb::global();
            for (fi, m, _, glyphs) in runs {
                let Some(ft) = cs.frames.get(*fi) else { continue };
                for &(li, gi) in glyphs {
                    let Some(l) = ft.lines.get(li) else { continue };
                    let Some(g) = l.glyphs.get(gi) else { continue };
                    let Some(style) = cs.styles.get(g.style as usize) else { continue };
                    let Some(c) = doc.resolve_color(&style.fill, style.fill_tint) else { continue };
                    let [r, gg, b, _] = c.to_rgba8(1.0);
                    rc.set_paint(peniko::Color::from_rgba8(255 - r, 255 - gg, 255 - b, 255));
                    let outline = db.outline(&g.face, g.gid);
                    let skew =
                        if style.skew != 0.0 { Affine::new([1.0, 0.0, -style.skew.to_radians().tan(), 1.0, 0.0, 0.0]) } else { Affine::IDENTITY };
                    let shatai = style.shatai_xf(g, l.baseline).unwrap_or(Affine::IDENTITY);
                    let ga = shatai * Affine::translate((g.x, l.baseline + g.y)) * skew * Affine::scale_non_uniform(g.sx, g.sy);
                    rc.set_transform(view * *m * ga);
                    rc.fill_path(&outline);
                }
            }
            rc.flush();
            let mut pm = vello_cpu::Pixmap::new(w as u16, h as u16);
            let mut res = vello_cpu::Resources::new();
            rc.render(&mut pm, &mut res);
            let ci = egui::ColorImage::from_rgba_premultiplied([w as usize, h as usize], pm.data_as_u8_slice());
            let t = crate::widgets::load_texture(ctx, "inverse_selection", ci, egui::TextureOptions::NEAREST);
            ctx.data_mut(|d| d.insert_temp(cache_id, (key, t.clone())));
            t
        }
    };
    painter.image(tex.id(), bbox, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
}

/// View → Show Hidden Characters: ¶ paragraph ends, » tabs, · spaces, ¬ forced line breaks, # end of story.
pub(super) fn draw_hidden_characters(app: &DesignApp, painter: &egui::Painter, xf: &Xf, doc: &Document, layout: &CanvasLayout) {
    for story in doc.stories.values() {
        let cs = app.session.cache.get(doc, story.id, None);
        for ft in &cs.frames {
            let Some((loc, it)) = hidden_characters_frame(doc, ft.frame) else { continue };
            let a = layout.xf(loc.spread) * doc.parent_xf(&loc);
            let m = a * doc.text_xf(it);
            let col = layer_color(doc, it);
            for l in &ft.lines {
                let size = (l.ascent * 0.75 * xf.zoom).clamp(6.0, 40.0) as f32;
                let font = egui::FontId::proportional(size);
                for (x, mark, centered) in hidden_marks(&story.text, l) {
                    let p = xf.to_screen(m * Point::new(x, l.baseline));
                    let align = if centered { egui::Align2::CENTER_BOTTOM } else { egui::Align2::LEFT_BOTTOM };
                    painter.text(p, align, mark, font.clone(), col);
                }
            }
        }
    }
}

/// Resolve a visible frame once for both the visibility check and painting, including its groups.
fn hidden_characters_frame(doc: &Document, frame: designcraft_doc::ItemId) -> Option<(designcraft_doc::ItemLoc, &designcraft_doc::Item)> {
    let loc = doc.find(frame)?;
    let spread = doc.spread(loc.spread)?;
    let mut items = spread.items.as_slice();
    for &index in &loc.path {
        let item = items.get(index)?;
        if item.hidden || doc.layer(item.layer).is_some_and(|layer| !layer.visible) {
            return None;
        }
        items = item.children();
    }
    let item = doc.item_at(&loc)?;
    Some((loc, item))
}

/// The hidden-character marks on line `l` of a story with text `text`: frame-space x, mark, and
/// whether the mark is centered on x (spaces) or starts at it.
pub(super) fn hidden_marks(text: &str, l: &compose::Line) -> Vec<(f64, &'static str, bool)> {
    let mut out = Vec::new();
    for g in &l.glyphs {
        if g.len == 0 {
            continue;
        }
        let Some(ch) = text.get(g.byte..).and_then(|t| t.chars().next()) else { continue };
        let mark = match ch {
            ' ' => "·",
            '\t' => "»",
            '\u{2028}' => "¬",
            '\u{A0}' => "°",
            _ => continue,
        };
        if ch == ' ' {
            out.push((g.x + g.adv / 2.0, mark, true));
        } else {
            out.push((g.x, mark, false));
        }
    }
    if l.last_in_para {
        // The spaces trimmed off the line's end sit past `end_x`; the paragraph mark follows them.
        let end = l.glyphs.iter().filter(|g| g.len > 0 && !g.visible && g.x >= l.end_x - 1e-9).map(|g| g.x + g.adv).fold(l.end_x, f64::max);
        let mark = if l.range.end >= text.len() { "#" } else { "¶" };
        out.push((end + 1.0, mark, false));
    }
    out
}

/// Edit › Spelling › Dynamic Spelling: red squiggles under misspelled words (checked once per
/// story version).
pub(super) fn draw_dynamic_spelling(app: &DesignApp, ctx: &egui::Context, painter: &egui::Painter, xf: &Xf, doc: &Document, layout: &CanvasLayout) {
    for story in doc.stories.values() {
        let key = egui::Id::unique(("dyn_spell", std::sync::Arc::as_ptr(story) as usize, doc.user_words.len()));
        let bad: std::sync::Arc<Vec<std::ops::Range<usize>>> = match ctx.data(|d| d.get_temp(key)) {
            Some(v) => v,
            None => {
                let v = std::sync::Arc::new(designcraft_engine::cmd::spelling::misspellings(doc, story, &doc.user_words));
                ctx.data_mut(|d| d.insert_temp(key, v.clone()));
                v
            }
        };
        if bad.is_empty() {
            continue;
        }
        let cs = app.session.cache.get(doc, story.id, None);
        let red = Stroke::new(1.0, Color32::from_rgb(230, 30, 30));
        for ft in &cs.frames {
            let (Some((a, _)), Some(it)) = (item_canvas_xf(doc, layout, ft.frame), doc.item(ft.frame)) else { continue };
            let m = a * doc.text_xf(it);
            for l in &ft.lines {
                for r in bad.iter().filter(|r| r.start < l.range.end && r.end > l.range.start) {
                    let gs: Vec<_> = l.glyphs.iter().filter(|g| g.len > 0 && g.byte >= r.start && g.byte < r.end).collect();
                    let (Some(x0), Some(x1)) = (gs.iter().map(|g| g.x).reduce(f64::min), gs.iter().map(|g| g.x + g.adv).reduce(f64::max)) else {
                        continue;
                    };
                    let y = l.baseline + l.descent * 0.5;
                    let (p0, p1) = (xf.to_screen(m * Point::new(x0, y)), xf.to_screen(m * Point::new(x1, y)));
                    // A zigzag 2 px high, 4 px per wave.
                    let n = ((p1.x - p0.x) / 2.0).max(1.0) as usize;
                    let pts: Vec<Pos2> =
                        (0..=n).map(|k| pos2(p0.x + (p1.x - p0.x) * k as f32 / n as f32, p0.y + if k % 2 == 0 { 0.0 } else { 2.0 })).collect();
                    painter.add(egui::Shape::line(pts, red));
                }
            }
        }
    }
}

/// Plain text of the current text selection (story markers resolved to readable characters).
pub fn selected_text(app: &DesignApp) -> Option<String> {
    let st = app.session.active()?;
    let t = st.selection.text?;
    let story = st.doc.text_story(t.story, t.cell)?;
    let s = story.slice(t.range());
    if s.is_empty() {
        return None;
    }
    Some(s.replace(designcraft_doc::story::FORCED_LINE_BREAK, "\n"))
}
