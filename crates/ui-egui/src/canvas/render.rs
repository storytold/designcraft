use super::*;

pub(super) fn c32(c: [u8; 3]) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

pub(super) fn paint_page_previews(
    previews: &mut crate::canvas_preview::Previews,
    painter: &egui::Painter,
    xf: &Xf,
    doc: &Document,
    layout: &CanvasLayout,
    screen: Rect,
) {
    let [r, g, b, _] = doc.resolve_color(designcraft_color::swatch::PAPER, 1.0).unwrap_or(designcraft_color::Color::WHITE).to_rgba8(1.0);
    let paper = Color32::from_rgb(r, g, b);
    for slot in &layout.slots {
        if !xf.rect(slot.bounds).expand(2.0).intersects(screen) {
            continue;
        }
        let Some(spread) = doc.spread(slot.spread) else { continue };
        let texture = previews.get(slot.spread);
        let mesh = texture.map(|texture| {
            let b = slot.bounds;
            let mut mesh = egui::Mesh::with_texture(texture.id());
            for (point, uv) in [
                (Point::new(b.x0, b.y0), pos2(0.0, 0.0)),
                (Point::new(b.x1, b.y0), pos2(1.0, 0.0)),
                (Point::new(b.x1, b.y1), pos2(1.0, 1.0)),
                (Point::new(b.x0, b.y1), pos2(0.0, 1.0)),
            ] {
                mesh.vertices.push(egui::epaint::Vertex { pos: xf.to_screen(point), uv, color: Color32::WHITE });
            }
            mesh.indices.extend([0, 1, 2, 0, 2, 3]);
            mesh
        });
        for page in &spread.pages {
            let r = xf.rect(slot.xf.transform_rect_bbox(page.bounds()));
            painter.rect_filled(r.translate(vec2(1.5, 1.5)), 0.0, Color32::BLACK);
            painter.rect_filled(r, 0.0, paper);
            if let Some(mesh) = &mesh {
                painter.with_clip_rect(r.intersect(screen)).add(egui::Shape::mesh(mesh.clone()));
            }
        }
    }
}

/// Render options for the canvas texture.
pub(super) fn screen_options(app: &DesignApp, layout: &CanvasLayout, preview: bool, ppp: f64) -> designcraft_render::RenderOptions {
    designcraft_render::RenderOptions {
        printing_only: preview,
        greek_below_px: 3.0 * ppp,
        highlight_missing_fonts: !preview && app.session.prefs.highlight_substituted_fonts,
        highlight_hj: !preview && app.session.prefs.highlight_hj,
        condition_indicators: !preview,
        note_indicators: !preview && app.session.prefs.show_note_anchors,
        change_markup: !preview && app.session.prefs.show_added_text,
        tag_markers: !preview && app.ui.tag_markers,
        highlight_keeps: !preview && app.session.prefs.highlight_keeps,
        highlight_custom_tracking: !preview && app.session.prefs.highlight_custom_tracking,
        quality: app.ui.display_quality,
        rich_black: app.ui.rich_black,
        overprint_preview: app.ui.overprint_preview,
        blend_space_view: !app.ui.proof_colors,
        document_grid: if app.ui.document_grid && !preview { grid_areas(layout) } else { vec![] },
        ..Default::default()
    }
}

/// The area each spread's document grid covers, in spread coordinates: the full-width band of
/// the pasteboard nearest that spread, the band in which `CanvasLayout::spread_at` (and so
/// snapping) picks it.
fn grid_areas(layout: &CanvasLayout) -> Vec<(designcraft_doc::SpreadRef, DRect)> {
    let pb = layout.pasteboard;
    let slots = &layout.slots;
    slots
        .iter()
        .enumerate()
        .map(|(i, slot)| {
            let lo = i.checked_sub(1).and_then(|j| slots.get(j)).map_or(pb.y0, |prev| (prev.bounds.y1 + slot.bounds.y0) / 2.0);
            let hi = slots.get(i + 1).map_or(pb.y1, |next| (slot.bounds.y1 + next.bounds.y0) / 2.0);
            (slot.spread, slot.xf.inverse().transform_rect_bbox(DRect::new(pb.x0, lo, pb.x1, hi)))
        })
        .collect()
}

/// The display options that change how the canvas texture looks without touching the document.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DisplayKey {
    quality: designcraft_render::DisplayQuality,
    rich_black: bool,
    overprint_preview: bool,
    proof_colors: bool,
    tag_markers: bool,
    document_grid: bool,
    highlight_substituted_fonts: bool,
    highlight_hj: bool,
    show_note_anchors: bool,
    show_added_text: bool,
    highlight_keeps: bool,
    highlight_custom_tracking: bool,
}

fn display_key(app: &DesignApp) -> DisplayKey {
    let (ui, prefs) = (&app.ui, &app.session.prefs);
    DisplayKey {
        quality: ui.display_quality,
        rich_black: ui.rich_black,
        overprint_preview: ui.overprint_preview,
        proof_colors: ui.proof_colors,
        tag_markers: ui.tag_markers,
        document_grid: ui.document_grid,
        highlight_substituted_fonts: prefs.highlight_substituted_fonts,
        highlight_hj: prefs.highlight_hj,
        show_note_anchors: prefs.show_note_anchors,
        show_added_text: prefs.show_added_text,
        highlight_keeps: prefs.highlight_keeps,
        highlight_custom_tracking: prefs.highlight_custom_tracking,
    }
}

/// When a display option changed, drop the texture and any render in flight: they were drawn with the old options.
pub(super) fn invalidate_on_display_change(app: &mut DesignApp) {
    let key = display_key(app);
    if app.canvas.display_key != Some(key) {
        app.canvas.display_key = Some(key);
        app.canvas.shown = None;
        app.canvas.pending = None;
        app.canvas.pending_doc = None;
    }
}

/// Keep the canvas texture current. Panning inside the overscanned region just shifts the
/// texture; zooming shows the old texture scaled until the background render arrives; document
/// edits render synchronously so editing never lags.
pub(super) fn render_texture(app: &mut DesignApp, ctx: &egui::Context, rect: Rect, xf: &Xf, layout: &CanvasLayout, preview: bool) {
    invalidate_on_display_change(app);
    let Some(st) = app.session.active() else { return };
    let ppp = ctx.pixels_per_point() as f64;
    let doc_key = (st.uid, std::sync::Arc::as_ptr(&st.doc) as u64, st.revision, preview);
    let editing_parents = st.editing_parents;
    // Overscan: 25% of the viewport on every side, within the GPU's maximum texture size.
    let max_side = ctx.input(|i| i.max_texture_side) as f32 / ppp as f32;
    let (ow, oh) =
        ((rect.width() * 0.25).min((max_side - rect.width()) / 2.0).max(0.0), (rect.height() * 0.25).min((max_side - rect.height()) / 2.0).max(0.0));
    let want = |origin: Point| crate::Shown {
        doc: doc_key,
        editing_parents,
        origin: Point::new(origin.x - ow as f64 / xf.zoom, origin.y - oh as f64 / xf.zoom),
        zoom: xf.zoom,
        size: (rect.width() + 2.0 * ow, rect.height() + 2.0 * oh),
    };
    let target = want(xf.origin);
    // Does the current texture cover the viewport at this zoom and document state?
    let covers = |s: &crate::Shown| {
        s.doc == doc_key && s.editing_parents == editing_parents && (s.zoom - xf.zoom).abs() < 1e-9 && {
            let x0 = (xf.origin.x - s.origin.x) * s.zoom;
            let y0 = (xf.origin.y - s.origin.y) * s.zoom;
            x0 >= -0.5 && y0 >= -0.5 && x0 + rect.width() as f64 <= s.size.0 as f64 + 0.5 && y0 + rect.height() as f64 <= s.size.1 as f64 + 0.5
        }
    };
    // Collect finished background renders.
    #[cfg(not(target_arch = "wasm32"))]
    if let (Some(w), Some((tok, shown))) = (app.canvas.worker.as_ref(), app.canvas.pending)
        && let Some(done) = w.poll()
        && done.token == tok
    {
        upload(app, ctx, done.image);
        app.canvas.shown = Some(shown);
        app.canvas.shown_doc = app.canvas.pending_doc.take();
        app.canvas.pending = None;
        app.canvas.defer_full = false;
        app.canvas.low_quality = false;
        app.perf.render_ms = done.ms;
    }
    let expected_resolution = app.canvas.shown.zip(app.canvas.texture.as_ref()).is_some_and(|(sh, tex)| {
        let expected = crate::widgets::fit_texture_size(canvas_pixels(sh, ppp), ctx.input(|i| i.max_texture_side));
        tex.size() == expected
    });
    if ((!app.canvas.low_quality && expected_resolution) || (app.canvas.low_quality && app.canvas.drag_active))
        && app.canvas.shown.is_some_and(|s| covers(&s))
    {
        // A small edit may already be fully painted; an empty interaction needs no refresh.
        if !app.canvas.drag_active && !app.canvas.low_quality {
            app.canvas.defer_full = false;
        }
        return;
    }
    let view_covered = app.canvas.shown.is_some_and(|s| covers(&crate::Shown { doc: doc_key, ..s }));
    let doc_changed = app.canvas.shown.is_none_or(|s| s.doc != doc_key);
    let mode_changed = app.canvas.shown.is_some_and(|s| s.editing_parents != editing_parents);
    let zoom_same = app.canvas.shown.is_some_and(|s| (s.zoom - xf.zoom).abs() < 1e-9);
    let job = |app: &DesignApp| {
        let st = app.session.active()?;
        let placed: Vec<designcraft_render::Placed> =
            layout.slots.iter().map(|s| designcraft_render::Placed { spread: s.spread, xf: s.xf }).collect();
        let render_ppp = if app.canvas.drag_active { ppp.min(0.5) } else { ppp };
        let w = (target.size.0 as f64 * render_ppp).round().max(1.0) as u32;
        let h = (target.size.1 as f64 * render_ppp).round().max(1.0) as u32;
        let view = Affine::scale(render_ppp) * Affine::scale(xf.zoom) * Affine::translate((-target.origin.x, -target.origin.y));
        let mut opts = screen_options(app, layout, preview, ppp);
        if app.canvas.drag_active && opts.quality == designcraft_render::DisplayQuality::High {
            opts.quality = designcraft_render::DisplayQuality::Typical;
        }
        Some((st.doc.clone(), placed, w, h, view, opts))
    };
    // Synchronous path: document edits (keep editing crisp), first frame, or no worker.
    #[cfg(not(target_arch = "wasm32"))]
    if !app.canvas.worker_started {
        app.canvas.worker_started = true;
        app.canvas.worker = render_worker_spawn(ctx);
    }
    #[cfg(not(target_arch = "wasm32"))]
    let has_worker = app.canvas.worker.is_some();
    #[cfg(target_arch = "wasm32")]
    let has_worker = false;
    // Preserve full-quality partial redraws for small changes, including live interactions.
    // Only a large or unbounded change needs the draft whole-canvas fallback.
    if !app.canvas.defer_full
        && !app.canvas.low_quality
        && doc_changed
        && zoom_same
        && view_covered
        && patch_texture(app, layout, ppp, doc_key, preview)
    {
        return;
    }
    // A mode switch changes every spread's meaning, even if the document has no damage.
    // Show the new canvas immediately, including when fitting it changes the zoom.
    if !has_worker || app.canvas.shown.is_none() || mode_changed || app.canvas.drag_active || (doc_changed && zoom_same && !app.canvas.defer_full) {
        let t0 = crate::now_ms();
        let Some((doc, placed, w, h, view, opts)) = job(app) else { return };
        let img = app.canvas.renderer.render(&doc, &app.session.cache, &placed, w, h, view, &opts);
        designcraft_render::images::trim_originals();
        upload(app, ctx, img);
        app.canvas.shown = Some(target);
        app.canvas.shown_doc = Some(doc);
        app.canvas.low_quality = app.canvas.drag_active;
        app.canvas.defer_full = false;
        app.canvas.pending = None;
        app.canvas.pending_doc = None;
        app.perf.render_ms = crate::now_ms() - t0;
        return;
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        // Already rendering this view?
        if app.canvas.pending.is_some_and(|(_, p)| {
            p.doc == doc_key && p.editing_parents == editing_parents && (p.zoom - xf.zoom).abs() < 1e-9 && {
                let x0 = (xf.origin.x - p.origin.x) * p.zoom;
                let y0 = (xf.origin.y - p.origin.y) * p.zoom;
                x0 >= 0.0 && y0 >= 0.0 && x0 + rect.width() as f64 <= p.size.0 as f64 && y0 + rect.height() as f64 <= p.size.1 as f64
            }
        }) {
            return;
        }
        let Some((doc, placed, w, h, view, opts)) = job(app) else { return };
        app.canvas.pending_doc = Some(doc.clone());
        app.canvas.token += 1;
        let token = app.canvas.token;
        if let Some(wk) = app.canvas.worker.as_ref() {
            wk.submit(crate::render_worker::Job { token, doc, cache: app.session.cache.clone(), placed, w, h, view, opts });
        }
        app.canvas.pending = Some((token, target));
    }
}

/// Expected pixel allocation for a full-quality texture, separate from logical view coverage.
fn canvas_pixels(shown: crate::Shown, ppp: f64) -> [usize; 2] {
    [(shown.size.0 as f64 * ppp).round().max(1.0) as usize, (shown.size.1 as f64 * ppp).round().max(1.0) as usize]
}

/// Repaint only the damaged regions of the current texture after an edit (typing, nudging).
/// Returns false when a full render is needed (view moved, too much changed, or damage unknown).
pub(super) fn patch_texture(app: &mut DesignApp, layout: &CanvasLayout, ppp: f64, doc_key: (u64, u64, u64, bool), preview: bool) -> bool {
    let (Some(sh), Some(old), Some(st)) = (app.canvas.shown, app.canvas.shown_doc.clone(), app.session.active()) else { return false };
    if sh.doc.0 != doc_key.0 || sh.doc.3 != doc_key.3 || sh.editing_parents != st.editing_parents {
        return false;
    }
    // Patches are in full-resolution pixels: a texture `upload` shrank to the GPU's limit
    // (a viewport wider than `max_texture_side`) takes full renders instead.
    // Native DPI/UI scaling can change after the first frame or between monitors.
    // Patches use current pixels-per-point, so an older-sized texture cannot accept them.
    let expected = canvas_pixels(sh, ppp);
    if app.canvas.texture.as_ref().is_none_or(|tex| tex.size() != expected) {
        return false;
    }
    let new = st.doc.clone();
    let Some(regions) = designcraft_render::damage::damage_with(&old, &new, Some(&app.session.cache)) else { return false };
    let t0 = crate::now_ms();
    let (tw, th) = (expected[0] as i64, expected[1] as i64);
    let k = sh.zoom * ppp;
    let mut px: Option<(i64, i64, i64, i64)> = None;
    for (r, rect) in &regions {
        let c = layout.xf(*r).transform_rect_bbox(*rect);
        let x0 = ((c.x0 - sh.origin.x) * k).floor() as i64 - 1;
        let y0 = ((c.y0 - sh.origin.y) * k).floor() as i64 - 1;
        let x1 = ((c.x1 - sh.origin.x) * k).ceil() as i64 + 1;
        let y1 = ((c.y1 - sh.origin.y) * k).ceil() as i64 + 1;
        px = Some(px.map_or((x0, y0, x1, y1), |p| (p.0.min(x0), p.1.min(y0), p.2.max(x1), p.3.max(y1))));
    }
    let done = |app: &mut DesignApp| {
        app.canvas.shown = Some(crate::Shown { doc: doc_key, ..sh });
        app.canvas.shown_doc = Some(new.clone());
        // A background render of the old document must not overwrite this.
        app.canvas.pending = None;
        app.canvas.pending_doc = None;
    };
    let Some((x0, y0, x1, y1)) = px.map(|p| (p.0.max(0), p.1.max(0), p.2.min(tw), p.3.min(th))) else {
        done(app);
        return true;
    };
    if x1 <= x0 || y1 <= y0 {
        done(app);
        return true;
    }
    // Big changes: a full render costs about the same.
    if ((x1 - x0) * (y1 - y0)) as f64 > 0.5 * (tw * th) as f64 {
        return false;
    }
    let placed: Vec<designcraft_render::Placed> = layout.slots.iter().map(|s| designcraft_render::Placed { spread: s.spread, xf: s.xf }).collect();
    let view = Affine::translate((-(x0 as f64), -(y0 as f64)))
        * Affine::scale(ppp)
        * Affine::scale(sh.zoom)
        * Affine::translate((-sh.origin.x, -sh.origin.y));
    let opts = screen_options(app, layout, preview, ppp);
    let mut img = app.canvas.patcher.render(&new, &app.session.cache, &placed, (x1 - x0) as u32, (y1 - y0) as u32, view, &opts);
    process_canvas_colors(app, &mut img);
    let Some(tex) = app.canvas.texture.as_mut() else { return false };
    let size = tex.size();
    if (x0 as usize).checked_add(img.width as usize).is_none_or(|end| end > size[0])
        || (y0 as usize).checked_add(img.height as usize).is_none_or(|end| end > size[1])
    {
        return false;
    }
    let ci = egui::ColorImage::from_rgba_premultiplied([img.width as usize, img.height as usize], &img.pixels);
    tex.set_partial([x0 as usize, y0 as usize], ci, egui::TextureOptions::LINEAR);
    done(app);
    app.canvas.patches += 1;
    app.perf.render_ms = crate::now_ms() - t0;
    true
}

#[cfg(not(target_arch = "wasm32"))]
fn render_worker_spawn(ctx: &egui::Context) -> Option<crate::render_worker::Worker> {
    if std::env::var_os("DESIGNCRAFT_SYNC_RENDER").is_some() {
        return None;
    }
    crate::render_worker::Worker::spawn(ctx.clone())
}

fn upload(app: &mut DesignApp, ctx: &egui::Context, mut img: designcraft_render::Rendered) {
    process_canvas_colors(app, &mut img);
    let ci = egui::ColorImage::from_rgba_premultiplied([img.width as usize, img.height as usize], &img.pixels);
    let ci = crate::widgets::fit_texture(ci, ctx.input(|i| i.max_texture_side));
    match &mut app.canvas.texture {
        Some(tex) => tex.set(ci, egui::TextureOptions::LINEAR),
        None => app.canvas.texture = Some(ctx.load_texture("canvas", ci, egui::TextureOptions::LINEAR)),
    }
}

/// Full uploads and damage patches must use the same proof transform.
/// Plate selection and ink-limit visualization are painted by the separations overlay.
fn process_canvas_colors(app: &DesignApp, img: &mut designcraft_render::Rendered) {
    if app.ui.proof_colors {
        designcraft_render::proof_view(img, &app.ui.proof_setup);
    }
}
